# ECS Command Buffer Design

This document outlines the design for a deferred command buffer system that allows systems to schedule entity operations without exclusive world access.

## Overview

The command buffer provides deferred execution of entity operations (spawn, despawn, add/remove components) from systems that only have partial world access. Commands are collected during system execution and flushed at designated points in the schedule.

## Design Goals

1. **Concurrent Access**: Multiple systems can write commands simultaneously
2. **Deferred Execution**: Operations execute at controlled flush points
3. **Entity ID Reservation**: Spawn commands return usable entity IDs immediately
4. **Lock-Free**: Minimize contention between systems
5. **Order Preservation**: Commands execute in submission order when order matters
6. **System Parameter**: Accessed via `Commands` parameter, not world methods

## API Design

### System Parameter Usage

```rust
fn spawner_system(query: Query<&Spawner>, mut commands: Commands) {
    for spawner in query {
        if spawner.should_spawn {
            // Returns reserved entity ID immediately
            let entity = commands.spawn((Position::default(), Velocity::default()));

            // Can use entity ID in subsequent commands
            commands.add_components(entity, (Health { current: 100.0, max: 100.0 },));
        }
    }
}

fn cleanup_system(query: Query<(Entity, &Health)>, mut commands: Commands) {
    for (entity, health) in query {
        if health.current <= 0.0 {
            commands.despawn(entity);
        }
    }
}

fn component_modifier(query: Query<(Entity, &Damaged)>, mut commands: Commands) {
    for (entity, damaged) in query {
        commands.remove_components::<(Damaged,)>(entity);
        commands.add_components(entity, (Healing { rate: 5.0 },));
    }
}
```

### Commands API

```rust
/// Handle for submitting deferred entity commands.
///
/// Commands are collected and executed at the next flush point
/// (typically between schedule phases).
pub struct Commands<'a> {
    buffer: &'a CommandBuffer,
    allocator: &'a SyncAllocator,  // Thread-safe world allocator
    registry: &'a TypeRegistry,
}

impl<'a> Commands<'a> {
    /// Spawn a new entity with components.
    /// Returns an allocated entity ID that will be valid after flush.
    ///
    /// The entity ID is allocated from the world's thread-safe allocator,
    /// preserving generation tracking and ID reuse.
    ///
    /// Accepts any type implementing `storage::Values` (single component or tuple).
    pub fn spawn<V: storage::Values>(&self, values: V) -> Entity {
        let entity = self.allocator.alloc();  // Thread-safe allocation
        self.buffer.push(Command::Spawn {
            entity,
            components: BoxedValues::new(values, self.registry),
        });
        entity
    }

    /// Despawn an entity.
    ///
    /// The entity will be removed from storage at flush time.
    /// Its ID will be returned to the allocator's dead pool with
    /// an incremented generation for future reuse.
    pub fn despawn(&self, entity: Entity) {
        self.buffer.push(Command::Despawn { entity });
    }

    /// Add components to an existing entity.
    ///
    /// Accepts any type implementing `storage::Values` (single component or tuple).
    pub fn add_components<V: storage::Values>(&self, entity: Entity, values: V) {
        self.buffer.push(Command::AddComponents {
            entity,
            components: BoxedValues::new(values, self.registry),
        });
    }

    /// Remove components from an entity by type.
    ///
    /// Accepts any type implementing `IntoSpec` to specify which components to remove.
    pub fn remove_components<S: component::IntoSpec>(&self, entity: Entity) {
        self.buffer.push(Command::RemoveComponents {
            entity,
            spec: S::into_spec(self.registry),
        });
    }
}
```

Note: `Commands` methods take `&self` (not `&mut self`) because both `SyncAllocator::alloc()`
and `CommandBuffer::push()` are thread-safe and work with shared references.

## Entity ID Allocation

### Problem

Systems need entity IDs immediately when spawning, but actual entity creation is deferred. Without careful design, this could:
- Cause ID collisions between concurrent systems
- Exhaust the ID space rapidly
- Create IDs that reference non-existent entities

### Current World Allocator Design

The existing `entity::Allocator` provides:
- **Generation tracking**: Each entity slot has a generation counter incremented on reuse
- **ID reuse**: Freed entities go to a dead pool for recycling
- **Stale reference detection**: Generation mismatches identify invalid entity handles

```rust
pub struct Allocator {
    dead_pool: Vec<Entity>,  // Freed entities with bumped generation
    next_id: u32,            // Next fresh ID
}

fn alloc(&mut self) -> Entity {
    self.dead_pool.pop().unwrap_or_else(|| {
        let id = Id(self.next_id);
        self.next_id += 1;
        Entity::new_with_generation(id, Generation::FIRST)
    })
}

fn free(&mut self, entity: Entity) {
    self.dead_pool.push(entity.genned());  // Increment generation
}
```

### Thread-Safe Allocator Options

Three approaches for making the allocator thread-safe, preserving generation semantics:

---

### Option A: Mutex-Protected Allocator (Simplest)

Wrap the existing allocator in a mutex. Preserves all semantics with minimal code changes.

```rust
use std::sync::Mutex;
use crossbeam::sync::ShardedLock;  // Or parking_lot::Mutex for better performance

/// Thread-safe wrapper around the standard allocator.
pub struct SyncAllocator {
    inner: Mutex<Allocator>,
}

impl SyncAllocator {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Allocator::new()),
        }
    }

    /// Allocate an entity (acquires lock briefly).
    pub fn alloc(&self) -> Entity {
        self.inner.lock().unwrap().alloc()
    }

    /// Free an entity for reuse (acquires lock briefly).
    pub fn free(&self, entity: Entity) {
        self.inner.lock().unwrap().free(entity);
    }

    /// Batch allocate for better throughput.
    pub fn alloc_many(&self, count: usize) -> Vec<Entity> {
        self.inner.lock().unwrap().alloc_many(count)
    }

    /// Get mutable access for single-threaded phases.
    pub fn get_mut(&mut self) -> &mut Allocator {
        self.inner.get_mut().unwrap()
    }
}
```

**Advantages:**
- Trivial implementation (~30 lines)
- Preserves ALL existing semantics (generations, reuse, dead pool)
- No architectural changes needed
- Can use `parking_lot::Mutex` for ~2x better performance than std

**Trade-offs:**
- Lock contention under very high spawn rates
- ~20-50ns per allocation (with parking_lot)

**Expected Performance:**
- `parking_lot::Mutex` uncontended: ~15-20ns
- Light contention (4 threads): ~30-50ns
- For comparison: one `HashMap::insert` is ~50-100ns

**When to use:**
- Spawn rates < 50k/frame (vast majority of games)
- When correctness and simplicity are priorities
- As the starting implementation

---

### Option B: Lock-Free with Generation Array (Most Complete)

Separate generation tracking from the allocation path for fully lock-free operation.

```rust
use crossbeam::queue::SegQueue;
use std::sync::atomic::{AtomicU32, Ordering};

/// Fully lock-free allocator with proper generation tracking.
pub struct LockFreeAllocator {
    /// Generation counter for each ID slot.
    /// Indexed by entity ID, stores current generation.
    generations: GenerationArray,

    /// Pool of IDs available for reuse (just the ID, not full Entity).
    dead_pool: SegQueue<u32>,

    /// Next fresh ID to allocate.
    next_id: AtomicU32,
}

/// Growable array of atomic generations.
struct GenerationArray {
    chunks: RwLock<Vec<Box<[AtomicU32; CHUNK_SIZE]>>>,
}

const CHUNK_SIZE: usize = 4096;

impl LockFreeAllocator {
    pub fn new() -> Self {
        Self {
            generations: GenerationArray::new(),
            dead_pool: SegQueue::new(),
            next_id: AtomicU32::new(0),
        }
    }

    /// Allocate an entity (lock-free).
    pub fn alloc(&self) -> Entity {
        // Try to reuse from dead pool first
        if let Some(id) = self.dead_pool.pop() {
            // Get current generation for this ID
            let gen = self.generations.get(id);
            return Entity::new_with_generation(Id(id), Generation(gen));
        }

        // Allocate fresh ID
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.generations.ensure_capacity(id);
        Entity::new_with_generation(Id(id), Generation::FIRST)
    }

    /// Free an entity for reuse (lock-free).
    pub fn free(&self, entity: Entity) {
        let id = entity.id().0;
        // Bump generation atomically
        self.generations.increment(id);
        // Return ID to pool
        self.dead_pool.push(id);
    }
}

impl GenerationArray {
    fn get(&self, id: u32) -> u32 {
        let chunk_idx = id as usize / CHUNK_SIZE;
        let slot_idx = id as usize % CHUNK_SIZE;

        let chunks = self.chunks.read().unwrap();
        if chunk_idx < chunks.len() {
            chunks[chunk_idx][slot_idx].load(Ordering::Acquire)
        } else {
            0  // Fresh ID, generation 0
        }
    }

    fn increment(&self, id: u32) {
        let chunk_idx = id as usize / CHUNK_SIZE;
        let slot_idx = id as usize % CHUNK_SIZE;

        self.ensure_capacity(id);
        let chunks = self.chunks.read().unwrap();
        chunks[chunk_idx][slot_idx].fetch_add(1, Ordering::Release);
    }

    fn ensure_capacity(&self, id: u32) {
        let chunk_idx = id as usize / CHUNK_SIZE;
        let chunks_len = self.chunks.read().unwrap().len();

        if chunk_idx >= chunks_len {
            let mut chunks = self.chunks.write().unwrap();
            while chunks.len() <= chunk_idx {
                chunks.push(Box::new(std::array::from_fn(|_| AtomicU32::new(0))));
            }
        }
    }
}
```

**Advantages:**
- Fully lock-free on the hot path (alloc/free)
- Preserves generation semantics completely
- Supports ID reuse with correct generations
- Scales well to many threads

**Trade-offs:**
- More complex implementation (~100 lines)
- Memory overhead for generation array (~4 bytes per ever-allocated entity)
- RwLock for chunk growth (rare, only when exceeding capacity)

**When to use:**
- Very high spawn/despawn rates (>50k/frame)
- Many parallel systems spawning concurrently
- When benchmarks show mutex is a bottleneck

---

### Option C: Hybrid Approach (Practical Compromise)

Use lock-free for fresh IDs, mutex only for dead pool access.

```rust
/// Hybrid allocator: lock-free fresh IDs, mutex for reuse.
pub struct HybridAllocator {
    /// Dead pool protected by mutex (reuse is less frequent).
    dead_pool: Mutex<Vec<Entity>>,

    /// Fresh ID allocation is lock-free.
    next_id: AtomicU32,
}

impl HybridAllocator {
    pub fn alloc(&self) -> Entity {
        // Try dead pool first (takes lock, but reuse is less common)
        if let Some(entity) = self.dead_pool.lock().unwrap().pop() {
            return entity;  // Already has correct generation
        }

        // Fresh allocation is lock-free
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Entity::new_with_generation(Id(id), Generation::FIRST)
    }

    pub fn free(&self, entity: Entity) {
        self.dead_pool.lock().unwrap().push(entity.genned());
    }
}
```

**Advantages:**
- Simpler than full lock-free (~40 lines)
- Fresh allocation (common case) is lock-free
- Preserves generation semantics
- Dead pool contention only matters when many entities being freed

**Trade-offs:**
- Lock contention on free() and when pool has items
- Less benefit if spawning mostly reuses freed IDs

---

### Comparison

| Aspect | Mutex (A) | Lock-Free (B) | Hybrid (C) |
|--------|-----------|---------------|------------|
| Complexity | ~30 lines | ~100 lines | ~40 lines |
| alloc() fresh | Lock | Atomic | Atomic |
| alloc() reuse | Lock | Lock-free | Lock |
| free() | Lock | Lock-free | Lock |
| Generation tracking | ✓ Full | ✓ Full | ✓ Full |
| ID reuse | ✓ Full | ✓ Full | ✓ Full |
| Memory overhead | Minimal | ~4B/entity | Minimal |
| Best spawn rate | <50k/frame | Any | <100k/frame |

### Recommendation

**Start with Option A (Mutex)**. It's the simplest, preserves all semantics, and `parking_lot::Mutex` is fast enough for the vast majority of games. The ~30ns overhead per spawn is negligible compared to actual entity storage operations.

Upgrade path if needed:
1. **Option A** → Profile shows contention → **Option C** (hybrid)
2. **Option C** → Still bottlenecked → **Option B** (full lock-free)

---

### Entity States

Entities allocated via Commands have a special state during the frame:

```
[Not Allocated] --alloc()--> [Allocated/Pending] --flush()--> [Spawned/Active]
                                                                    |
                                                    despawn --------+
                                                                    v
                                                            [Freed/Pooled]
                                                    (generation incremented)
```

- **Allocated/Pending**: ID exists but entity not yet in world storage
- **Spawned/Active**: Entity exists in storage, queryable
- **Freed/Pooled**: ID returned to pool with incremented generation
- Accessing pending entities before flush returns error/empty
- Entity IDs from `commands.spawn()` become queryable after flush

## Command Buffer Implementation

### Lock-Free Queue

Using crossbeam's `SegQueue` for lock-free command collection:

```rust
use crossbeam::queue::SegQueue;

/// Thread-safe command buffer using lock-free queue.
pub struct CommandBuffer {
    commands: SegQueue<Command>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self {
            commands: SegQueue::new(),
        }
    }

    /// Push a command (lock-free, wait-free for producers).
    pub fn push(&self, command: Command) {
        self.commands.push(command);
    }

    /// Drain all commands for execution (single consumer).
    pub fn drain(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        while let Some(cmd) = self.commands.pop() {
            commands.push(cmd);
        }
        commands
    }
}
```

### Command Enum

```rust
/// A deferred entity command.
pub enum Command {
    Spawn {
        entity: Entity,
        components: BoxedValues,
    },
    Despawn {
        entity: Entity,
    },
    AddComponents {
        entity: Entity,
        components: BoxedValues,
    },
    RemoveComponents {
        entity: Entity,
        spec: component::Spec,
    },
}
```

### BoxedValues

Type-erased wrapper for `storage::Values` that captures the apply function:

```rust
/// Type-erased container for deferred component values.
///
/// This wraps any `V: storage::Values` by capturing:
/// 1. The component `Spec` for archetype lookup
/// 2. A boxed closure that owns the values and applies them to a table
///
/// This approach reuses the existing `Values` trait directly without
/// manual byte serialization.
pub struct BoxedValues {
    /// Pre-computed spec for archetype/table lookup
    spec: component::Spec,
    /// Captured apply function that owns the component data
    apply_fn: Box<dyn FnOnce(&mut Table, Row) + Send>,
}

impl BoxedValues {
    /// Create a BoxedValues from any type implementing `storage::Values`.
    ///
    /// The values are moved into a closure that will apply them later.
    pub fn new<V: storage::Values + Send>(values: V, registry: &TypeRegistry) -> Self {
        // Compute spec at creation time (needed for archetype lookup at flush)
        let spec = V::into_spec(registry);

        // Capture values in a closure - this moves ownership into the box
        let apply_fn = Box::new(move |table: &mut Table, row: Row| {
            values.apply(table, row);
        });

        Self { spec, apply_fn }
    }

    /// Get the component specification (for archetype lookup).
    pub fn spec(&self) -> &component::Spec {
        &self.spec
    }

    /// Apply the stored values to a table row (consumes self).
    pub fn apply(self, table: &mut Table, row: Row) {
        (self.apply_fn)(table, row);
    }
}
```

**Key design points:**

1. **Reuses `storage::Values` directly** - no custom serialization needed
2. **Spec computed eagerly** - available for archetype lookup before apply
3. **`FnOnce` captures ownership** - values moved into closure, applied once at flush
4. **`Send` bound** - allows commands to be sent between threads (lock-free queue)
5. **Zero-copy for small types** - values stored inline in closure, no heap allocation for small tuples

## Flush Mechanism

### Automatic Flush Points

Commands are flushed automatically between schedule phases.

```rust
impl Schedule {
    pub fn run(&mut self, phase: impl Phase, world: &mut World, executor: &Executor) {
        // Create command buffer for this phase
        let command_buffer = CommandBuffer::new();

        // Run all systems in this phase
        // Systems receive Commands parameter with:
        // - Shared reference to command buffer
        // - Shared reference to world's thread-safe allocator
        // - Shared reference to type registry
        self.run_phase_systems(phase, world, executor, &command_buffer);

        // Flush commands after phase completes (single-threaded)
        self.flush_commands(&command_buffer, world);
    }

    fn flush_commands(&self, buffer: &CommandBuffer, world: &mut World) {
        let commands = buffer.drain();

        for command in commands {
            match command {
                Command::Spawn { entity, components } => {
                    // Entity ID already allocated from world's allocator.
                    // Just create storage at that ID.
                    match world.spawn_at(entity, components) {
                        Ok(()) => {},
                        Err(e) => log::warn!("Spawn failed for {:?}: {}", entity, e),
                    }
                }
                Command::Despawn { entity } => {
                    // Despawn removes from storage and returns ID to allocator's
                    // dead pool with incremented generation.
                    if let Err(e) = world.despawn(entity) {
                        log::warn!("Despawn failed for {:?}: {}", entity, e);
                    }
                }
                Command::AddComponents { entity, components } => {
                    if let Err(e) = world.add_components(entity, components) {
                        log::warn!("AddComponents failed for {:?}: {}", entity, e);
                    }
                }
                Command::RemoveComponents { entity, spec } => {
                    if let Err(e) = world.remove_components(entity, spec) {
                        log::warn!("RemoveComponents failed for {:?}: {}", entity, e);
                    }
                }
            }
        }
    }
}
```

**Key Design Points:**

1. **No separate allocator per phase** - Commands use the world's shared `SyncAllocator` directly
2. **No sync/cleanup needed** - Allocations go directly to the authoritative allocator
3. **Despawn handles ID recycling** - `world.despawn()` returns ID to dead pool with bumped generation
4. **Generations preserved** - Reused IDs have correct generation from the allocator

### Order Preservation

Commands within a single system are naturally ordered (single producer).
Commands across systems in the same phase may interleave, but the lock-free queue preserves FIFO order per producer.

For cases where strict global ordering matters:

```rust
/// Commands with explicit ordering requirements.
pub struct OrderedCommands<'a> {
    commands: Commands<'a>,
    sequence: AtomicU64,
}

impl<'a> OrderedCommands<'a> {
    pub fn spawn<C: ComponentSet>(&mut self, components: C) -> Entity {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        // Include sequence number in command for sorting at flush
        self.commands.spawn_ordered(components, seq)
    }
}
```

## System Parameter Integration

### Parameter Trait Implementation

```rust
impl<'a> Parameter for Commands<'a> {
    type Item<'w> = Commands<'w>;

    fn extract<'w>(
        _shard: &'w mut Shard,
        context: &'w SystemContext,
    ) -> Self::Item<'w> {
        Commands {
            buffer: context.command_buffer(),
            allocator: context.entity_allocator(),  // World's thread-safe allocator
            registry: context.type_registry(),
        }
    }

    fn component_access() -> ComponentAccess {
        // Commands don't access components directly - they only write to
        // the command buffer and allocate entity IDs. Both are thread-safe.
        ComponentAccess::none()
    }
}
```

### SystemContext Extension

```rust
/// Context provided to systems during execution.
///
/// This is created by the scheduler and passed to system parameter extraction.
/// It provides access to shared infrastructure that systems need.
pub struct SystemContext<'a> {
    /// Command buffer for this phase (shared across all systems).
    command_buffer: &'a CommandBuffer,

    /// World's thread-safe entity allocator.
    /// Using SyncAllocator (mutex-wrapped) preserves generation tracking
    /// and ID reuse from the dead pool.
    entity_allocator: &'a SyncAllocator,

    /// Type registry for component registration/lookup.
    type_registry: &'a TypeRegistry,
    // ... other context (time, frame number, etc.)
}

impl<'a> SystemContext<'a> {
    pub fn new(
        command_buffer: &'a CommandBuffer,
        entity_allocator: &'a SyncAllocator,
        type_registry: &'a TypeRegistry,
    ) -> Self {
        Self {
            command_buffer,
            entity_allocator,
            type_registry,
        }
    }

    pub fn command_buffer(&self) -> &'a CommandBuffer {
        self.command_buffer
    }

    pub fn entity_allocator(&self) -> &'a SyncAllocator {
        self.entity_allocator
    }

    pub fn type_registry(&self) -> &'a TypeRegistry {
        self.type_registry
    }
}
```

## Error Handling

Errors during flush are logged as warnings:

```rust
/// Command execution errors.
#[derive(Debug)]
pub enum CommandError {
    /// Entity was despawned before command executed
    EntityNotFound(Entity),
    /// Entity already has component being added
    ComponentExists(Entity, TypeId),
    /// Entity missing component being removed
    ComponentMissing(Entity, TypeId),
    /// Reserved entity ID was invalid
    InvalidReservation(Entity),
}

// During flush:
match world.spawn_reserved(entity, components) {
    Ok(()) => {},
    Err(CommandError::InvalidReservation(e)) => {
        log::warn!("Invalid entity reservation {:?}, skipping spawn", e);
    }
    Err(e) => {
        log::warn!("Command failed: {:?}", e);
    }
}
```

## Per-Phase vs Per-System Buffers

### Recommendation: Per-Phase Buffer

Based on the concern about per-system overhead, using a single shared buffer per phase:

**Advantages:**
- Single allocation per phase
- Less memory overhead
- Simpler flush logic
- Natural ordering within phase

**Trade-offs:**
- Slight contention on lock-free queue (minimal with SegQueue)
- Commands from different systems interleave

### Alternative: Per-System Buffers (if needed later)

```rust
/// Per-system buffer option for zero contention.
pub struct SystemCommandBuffer {
    local_buffer: Vec<Command>,
}

// At phase end, collect all system buffers:
fn flush_all_system_buffers(systems: &[System], world: &mut World) {
    for system in systems {
        for command in system.buffer.drain(..) {
            execute_command(command, world);
        }
    }
}
```

## Memory Layout

```
World
├── storage: Storage
├── type_registry: TypeRegistry
└── allocator: SyncAllocator          // Thread-safe, shared across systems
    └── inner: Mutex<Allocator>
        ├── dead_pool: Vec<Entity>    // Freed entities with bumped generation
        └── next_id: u32              // Next fresh ID

CommandBuffer (per phase, temporary)
├── SegQueue<Command>
│   ├── Segment 0: [Cmd, Cmd, Cmd, ...]
│   ├── Segment 1: [Cmd, Cmd, Cmd, ...]
│   └── ...

Command
├── Spawn { entity, BoxedValues }
├── Despawn { entity }
├── AddComponents { entity, BoxedValues }
└── RemoveComponents { entity, Spec }

BoxedValues
├── spec: component::Spec       // For archetype lookup
└── apply_fn: Box<dyn FnOnce>   // Captured values + apply logic
    └── [inline closure data]   // Component values owned here

SystemContext (per phase, temporary)
├── command_buffer: &CommandBuffer
├── entity_allocator: &SyncAllocator  // Reference to world's allocator
└── type_registry: &TypeRegistry
```

### Alternative: Lock-Free Allocator (Option B)

If mutex contention becomes an issue:

```
LockFreeAllocator
├── generations: GenerationArray
│   └── chunks: RwLock<Vec<Box<[AtomicU32; 4096]>>>
├── dead_pool: SegQueue<u32>    // Just IDs, not full entities
└── next_id: AtomicU32
```

## Implementation Plan

### Phase 1: Core Infrastructure
1. Add crossbeam dependency to engine
2. Implement `BoxedValues` (wraps `storage::Values` with captured apply)
3. Implement `Command` enum
4. Implement `CommandBuffer` with `SegQueue`
5. Add `CommandError` type

### Phase 2: Thread-Safe Entity Allocator
1. Add `parking_lot` dependency for fast mutex (or use std::sync::Mutex initially)
2. Create `SyncAllocator` wrapper around existing `Allocator` (Option A - recommended)
3. Add thread-safe `alloc()`, `free()`, `alloc_many()` methods
4. Add `get_mut()` for single-threaded access during flush
5. Update World to use `SyncAllocator` instead of `Allocator`
6. Add `spawn_at()` to World (spawn entity at pre-allocated ID)
7. _(Later, if needed)_: Implement `LockFreeAllocator` (Option B) or `HybridAllocator` (Option C)

### Phase 3: Commands Parameter
1. Implement `Commands` struct with spawn/despawn/add/remove methods
2. Add `Parameter` trait implementation for system extraction
3. Create `CommandContext` to hold buffer + allocator + registry references

### Phase 4: Schedule Integration
1. Add `CommandBuffer` + `EntityIdAllocator` to phase execution context
2. Implement `flush_commands()` in scheduler
3. Add automatic flush between phases
4. Handle cleanup of unused entity reservations

### Phase 5: Testing & Benchmarks
1. Unit tests for `BoxedValues` (single component, tuples, nested)
2. Unit tests for `CommandBuffer` (push/drain)
3. Concurrent access tests (multiple producers)
4. Integration test with full schedule
5. Benchmark command throughput

## Open Questions

1. **Allocator Choice**: Start with Option A (Mutex) for simplicity. It preserves all generation/reuse semantics and `parking_lot::Mutex` is fast enough for most games. Only consider Option B (Lock-Free) or Option C (Hybrid) if benchmarks show lock contention.

2. **Flush Granularity**: Should there be explicit flush commands, or only automatic phase boundaries?

3. **Command Batching**: Should we support batched spawns for efficiency?
   ```rust
   commands.spawn_batch(positions.iter().map(|p| (p, Velocity::default())))
   ```
   With mutex allocator, batch allocation could reduce lock acquisitions.

4. **Entity References in Components**: How to handle components that reference entities spawned in the same frame?
   ```rust
   let parent = commands.spawn(Parent);
   let child = commands.spawn((Child, ParentRef(parent))); // parent not yet in storage!
   ```
   This is valid! The entity ID is immediately usable for references - the ID exists (allocated
   from world's allocator), only the storage entry is deferred. The `ParentRef(parent)` will
   work correctly after flush.

5. **World Integration**: Should `SyncAllocator` replace `Allocator` entirely, or should World hold both and switch based on context? Recommendation: Replace entirely - the mutex is only overhead during parallel phases, and `get_mut()` provides zero-cost access when exclusive.

## Dependencies

```toml
[dependencies]
crossbeam = "0.8"  # For SegQueue (lock-free queue)
log = "0.4"        # For warning on errors
```
