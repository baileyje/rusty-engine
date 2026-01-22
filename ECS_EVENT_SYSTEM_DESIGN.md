# ECS Event System Design

## Overview

A double-buffered event system for the ECS that enables decoupled communication between systems. Writers append to an "active" buffer while readers iterate a "stable" buffer. Buffer swapping is controlled externally by the game/engine loop.

## Use Cases

- Systems communicating that something important happened without knowing who cares
- Input event handling (keyboard, mouse, gamepad)
- Game events (damage, death, collision, achievements)

## Design Decisions

### 1. Double-Buffer Model

Events use a double-buffer pattern inspired by Bevy:
- Writers append to the "active" buffer
- Readers iterate the "stable" buffer
- On swap: indices flip, new active buffer is cleared
- Events written in frame N are readable in frame N+1

**Rationale**: This allows parallel writes AND parallel reads (to different buffers), provides natural event lifetime (one frame), and solves the input event flush problem elegantly.

### 2. External Swap Control

The World exposes `swap_event_buffers()` for the engine/game loop to call:

```rust
loop {
    world.swap_event_buffers();  // Events from last frame now readable
    schedule.run(PreUpdate, &mut world);
    schedule.run(Update, &mut world);
    schedule.run(PostUpdate, &mut world);
    // Events written this frame will be readable next frame
}
```

**Rationale**: The ECS world doesn't control the game loop, so swap timing must be external. Events span all phases within a frame.

### 3. Access Model

- `Producer<T>` - exclusive access to active buffer (write)
- `Consumer<T>` - shared access to stable buffer (read)
- Producer and Consumer use **different marker TypeIds** for access tracking
- This means Producer<T> and Consumer<T> **do not conflict** (different resources)
- Multiple Producers of same type conflict (exclusive access)
- Multiple Consumers of same type don't conflict (shared access)

### 4. Stream Configuration

- Streams must be explicitly registered before use: `world.register_event::<T>()`
- Default capacity: 1024 events
- Configurable: `world.register_event_with_capacity::<T>(4096)`
- Panic on overflow (catches misconfigured systems early)

### 5. Event Trait Bounds

```rust
pub trait Event: 'static + Send + Sync + Clone + Debug {}
```

- `'static`: No borrowed data
- `Send + Sync`: Thread-safe for parallel systems
- `Clone`: Events may be read by multiple consumers
- `Debug`: For diagnostics and logging

### 6. Type Erasure

EventBroker uses `HashMap<TypeId, Box<dyn ErasedStream>>` pattern (same as Uniques):

```rust
pub struct EventBroker {
    streams: HashMap<TypeId, Box<dyn ErasedStream>>,
}
```

## File Structure

```
engine/src/ecs/
├── event/
│   ├── mod.rs       # Event trait, module re-exports
│   ├── stream.rs    # EventStream<T> double-buffer implementation
│   └── broker.rs    # EventBroker, ErasedStream trait
├── system/param/
│   └── events.rs    # Producer<T>, Consumer<T> parameters
└── world/
    └── mod.rs       # Add events field and methods
```

## Data Structures

### EventStream<T>

```rust
pub struct EventStream<E: Event> {
    /// Index of the currently active (write) buffer: 0 or 1
    active_index: usize,

    /// The two buffers - one active, one stable
    buffers: [Vec<E>; 2],

    /// Capacity limit - panic on overflow
    capacity: usize,
}

impl<E: Event> EventStream<E> {
    pub fn new(capacity: usize) -> Self;

    /// Send an event to the active buffer. Panics if over capacity.
    pub fn send(&mut self, event: E);

    /// Iterate events in the stable buffer.
    pub fn iter(&self) -> impl Iterator<Item = &E>;

    /// Number of events in stable buffer.
    pub fn len(&self) -> usize;

    /// Check if stable buffer is empty.
    pub fn is_empty(&self) -> bool;

    /// Swap buffers (called by EventBroker::swap_all).
    pub(crate) fn swap(&mut self) {
        self.active_index = 1 - self.active_index;
        self.buffers[self.active_index].clear();
    }
}
```

### ErasedStream Trait

```rust
pub(crate) trait ErasedStream: Send + Sync {
    fn swap(&mut self);
    fn stable_len(&self) -> usize;
}

impl<E: Event> ErasedStream for EventStream<E> {
    fn swap(&mut self) { EventStream::swap(self); }
    fn stable_len(&self) -> usize { self.len() }
}
```

### EventBroker

```rust
pub struct EventBroker {
    streams: HashMap<TypeId, Box<dyn ErasedStream>>,
}

impl EventBroker {
    pub fn new() -> Self;

    /// Register with default capacity (1024). Panics if already registered.
    pub fn register<E: Event>(&mut self);

    /// Register with custom capacity. Panics if already registered.
    pub fn register_with_capacity<E: Event>(&mut self, capacity: usize);

    pub fn is_registered<E: Event>(&self) -> bool;

    /// Get stream for reading. Panics if not registered.
    pub fn stream<E: Event>(&self) -> &EventStream<E>;

    /// Get stream for writing. Panics if not registered.
    pub fn stream_mut<E: Event>(&mut self) -> &mut EventStream<E>;

    /// Swap all event buffers.
    pub fn swap_all(&mut self) {
        for stream in self.streams.values_mut() {
            stream.swap();
        }
    }
}
```

### Producer<T> Parameter

```rust
/// Marker type for access tracking (not stored, only for TypeId discrimination)
struct EventActiveMarker<E: Event>(PhantomData<E>);

pub struct Producer<'w, E: Event> {
    stream: &'w mut EventStream<E>,
}

impl<'w, E: Event> Producer<'w, E> {
    pub fn send(&mut self, event: E) {
        self.stream.send(event);
    }
}

impl<E: Event> Parameter for Producer<'_, E> {
    type Value<'w, 's> = Producer<'w, E>;
    type State = ();

    fn build_state(_world: &mut World) -> Self::State {}

    fn required_access(world: &World) -> AccessRequest {
        // Mutable access to active buffer marker
        AccessRequest::to_resources(
            &[],
            &[world.resources().register_unique::<EventActiveMarker<E>>()]
        )
    }

    unsafe fn extract<'w, 's>(
        shard: &'w mut Shard<'_>,
        _state: &'s mut Self::State,
        _command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's> {
        Producer {
            stream: shard.events_mut().stream_mut::<E>(),
        }
    }
}
```

### Consumer<T> Parameter

```rust
/// Marker type for access tracking
struct EventStableMarker<E: Event>(PhantomData<E>);

pub struct Consumer<'w, E: Event> {
    stream: &'w EventStream<E>,
}

impl<'w, E: Event> Consumer<'w, E> {
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.stream.iter()
    }

    pub fn len(&self) -> usize {
        self.stream.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stream.is_empty()
    }
}

impl<E: Event> Parameter for Consumer<'_, E> {
    type Value<'w, 's> = Consumer<'w, E>;
    type State = ();

    fn build_state(_world: &mut World) -> Self::State {}

    fn required_access(world: &World) -> AccessRequest {
        // Immutable access to stable buffer marker
        AccessRequest::to_resources(
            &[world.resources().register_unique::<EventStableMarker<E>>()],
            &[]
        )
    }

    unsafe fn extract<'w, 's>(
        shard: &'w mut Shard<'_>,
        _state: &'s mut Self::State,
        _command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's> {
        Consumer {
            stream: shard.events().stream::<E>(),
        }
    }
}
```

## World API Changes

### World Struct

```rust
pub struct World {
    // ... existing fields ...

    /// Event broker for double-buffered event streams.
    events: event::EventBroker,
}
```

### World Methods

```rust
impl World {
    /// Register a new event type with default capacity (1024).
    pub fn register_event<E: event::Event>(&mut self) {
        self.events.register::<E>();
    }

    /// Register a new event type with specified capacity.
    pub fn register_event_with_capacity<E: event::Event>(&mut self, capacity: usize) {
        self.events.register_with_capacity::<E>(capacity);
    }

    /// Swap all event buffers. Call once per frame.
    pub fn swap_event_buffers(&mut self) {
        self.events.swap_all();
    }

    pub fn events(&self) -> &event::EventBroker {
        &self.events
    }

    pub fn events_mut(&mut self) -> &mut event::EventBroker {
        &mut self.events
    }
}
```

### Shard Methods

```rust
impl Shard<'_> {
    pub fn events(&self) -> &event::EventBroker {
        unsafe { (*self.world).events() }
    }

    pub fn events_mut(&mut self) -> &mut event::EventBroker {
        unsafe { (*self.world).events_mut() }
    }
}
```

## Access Conflict Matrix

| Parameter A | Parameter B | Conflict? | Reason |
|-------------|-------------|-----------|--------|
| Producer<T> | Producer<T> | Yes | Same mutable marker |
| Consumer<T> | Consumer<T> | No | Shared immutable access |
| Producer<T> | Consumer<T> | No | Different marker types |
| Producer<T> | Consumer<U> | No | Different event types |

## Usage Example

```rust
use rusty_engine::ecs::{Event, Producer, Consumer};

#[derive(Clone, Debug)]
struct DamageEvent {
    target: Entity,
    amount: u32,
    source: Entity,
}
impl Event for DamageEvent {}

#[derive(Clone, Debug)]
struct DeathEvent {
    entity: Entity,
}
impl Event for DeathEvent {}

// Setup
fn setup(world: &mut World) {
    world.register_event::<DamageEvent>();
    world.register_event_with_capacity::<DeathEvent>(256);
}

// Producer system
fn combat_system(
    query: Query<(Entity, &Attack, &Target)>,
    mut damage: Producer<DamageEvent>,
) {
    for (attacker, attack, target) in query {
        damage.send(DamageEvent {
            target: target.entity,
            amount: attack.damage,
            source: attacker,
        });
    }
}

// Consumer system that also produces
fn damage_system(
    damage_events: Consumer<DamageEvent>,
    mut death: Producer<DeathEvent>,
    mut query: Query<(Entity, &mut Health)>,
) {
    for event in damage_events.iter() {
        if let Some((entity, mut health)) = query.get_mut(event.target) {
            health.current = health.current.saturating_sub(event.amount);
            if health.current == 0 {
                death.send(DeathEvent { entity });
            }
        }
    }
}

// Game loop
fn game_loop(world: &mut World, schedule: &mut Schedule) {
    loop {
        // Start of frame: swap buffers
        // Events written last frame become readable
        world.swap_event_buffers();

        // Run all phases
        schedule.run(PreUpdate, world);
        schedule.run(Update, world);
        schedule.run(PostUpdate, world);

        // Events written this frame will be readable next frame
    }
}
```

## Test Strategy

### Unit Tests (event/stream.rs)

1. **Basic operations**: new, send, iter, len, is_empty
2. **Swap behavior**: events move from active to stable, old stable cleared
3. **Capacity enforcement**: panic on overflow
4. **Multiple swaps**: cycle correctly

### Unit Tests (event/broker.rs)

1. **Registration**: success, duplicate panics, is_registered
2. **Type safety**: different types get different streams
3. **swap_all**: propagates to all streams

### Unit Tests (system/param/events.rs)

1. **Access requests**: Producer mutable, Consumer immutable
2. **Conflict detection**: Producer+Producer conflicts, Consumer+Consumer doesn't, Producer+Consumer doesn't
3. **Extraction**: correctly accesses broker

### Integration Tests

1. **Frame lifecycle**: write in frame N, read in frame N+1
2. **System integration**: Producer writes, Consumer reads after swap
3. **Parallel scheduling**: Producer and Consumer scheduled without conflict

```bash
cargo test -p rusty_engine event
cargo test -p rusty_engine producer
cargo test -p rusty_engine consumer
```

## Future Considerations

1. **Derive macro**: `#[derive(Event)]` to auto-implement the trait
2. **Event filtering**: Consumer with predicate
3. **Event priority**: Ordered event delivery within a frame
4. **Metrics**: Track event throughput, buffer utilization
5. **Overflow strategies**: Drop oldest, block, or configurable behavior
