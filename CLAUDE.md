# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rusty Engine is a multi-threaded game engine built in Rust with a custom Entity-Component-System (ECS) architecture. The engine is designed to leverage multi-threaded architectures for simulations, games, and scientific applications.

**Core Architecture:**
- **Dual-thread design**: Primary logic loop on one thread, dedicated render loop on second thread
- **Frame-based updates**: Logic loop runs as fast as possible while maintaining frame timing
- **Fixed update cycle**: Reliable time-delta sensitive operations (e.g., physics)
- **State passing**: Logic thread produces state data consumed by render thread

## Repository Structure

This is a Cargo workspace with three main crates:
- `engine/` - Core engine library (rusty_engine)
  - `src/core/` - Engine architecture (context, control, runner, tasks, time, etc.)
  - `src/ecs/` - Entity-Component-System implementation
- `app/` - Application/game using the engine (rusty_app)
- `cli/` - CLI utilities (rusty_cli)

## Common Commands

### Building
```bash
# Build entire workspace
cargo build

# Build specific package
cargo build -p rusty_engine
cargo build -p rusty_app
cargo build -p rusty_cli

# Release build
cargo build --release
```

### Testing
```bash
# Run all tests in workspace
cargo test

# Run tests for specific package
cargo test -p rusty_engine

# Run specific test by name
cargo test <test_name>

# Run tests in a specific module (e.g., storage tests)
cargo test -p rusty_engine storage

# Run tests with output
cargo test -- --nocapture

# Don't run, just check compilation
cargo test --no-run
```

### Examples
```bash
# Run the simple_world example
cargo run --example simple_world
```

## ECS Architecture

The engine implements a **custom archetype-based ECS** with columnar storage for cache efficiency.

### Key Subsystems

1. **World** (`ecs/world/`) - Central container managing all entities, components, and systems
   - Coordinates entity allocator, registries, storage, and archetypes
   - `TypeRegistry` - Centralized type registration, maps TypeId → Info
   - Primary API: `world.spawn()`, `world.despawn()`, `world.entity()`

2. **Component System** (`ecs/component/`)
   - `Spec` - Identifies component combinations for archetype lookup (used as HashMap keys)
   - `Info` - Component metadata (layout, drop fn, TypeId) used by storage layer
   - `Set` trait - Write-side API for adding components (supports tuples)
   - `#[derive(Component)]` macro - Auto-implements component traits
   - Note: Spec is for semantic identification; Info is for storage mechanics

3. **Entity System** (`ecs/entity/`)
   - `Allocator` - Manages entity ID allocation and reuse
   - `Registry` - Tracks spawned entities and their storage locations
   - `Reference` - Safe handle to entities with generation checking

4. **Storage Layer** (`ecs/storage/`)
   - **Archetype pattern**: Entities with identical component sets share a `Table`
   - **Columnar layout**: Each component type stored in contiguous `Column` (cache-friendly)
   - **Type erasure**: Uniform storage via `IndexedMemory` with runtime type checking
   - **View trait**: Read-side API for accessing components (single or tuples)
   - **Info-based creation**: Tables created with `&[component::Info]`, not Spec
   - Key types:
     - `Table` - Multi-column storage for one archetype (created from Info array)
     - `Storage` - Collection of all tables
     - `Column` - Type-erased single-component column (stores its own Info)
     - `Index` - Entity → Row mapping (DynamicIndex or HashIndex)

5. **Query System** (`ecs/query/`)
   - `Query<D>` - Type-safe iteration over entities matching component specs
   - Supports tuples: `Query<(&Position, &mut Velocity)>`
   - Returns `Result` iterator over matching tables

6. **System Module** (`ecs/system/`)
   - `Parameter` trait - GAT-based parameter extraction
   - `Wrapper` - Converts functions to systems
   - Supports clean signatures without explicit lifetimes
   - Component spec generation for scheduler

7. **Archetype Registry** (`ecs/archetype/`)
   - Tracks unique component combinations
   - Maps specs to storage tables

### Storage Design Patterns

**Archetype Invariant:**
All entities in a Table have exactly the same components. This means:
- No `Option<Component>` overhead
- Fast iteration without sparse checks
- All columns have identical length
- Exact size hints for iterators

**Set vs View Symmetry:**
- `Set` trait: Write components when spawning entities (tuples supported)
- `View` trait: Read components from existing entities (tuples supported)
- Both use macro-generated implementations for tuples (1-26 components)

**Safety Model:**
- Storage uses `unsafe` with clear contracts and debug assertions
- Type correctness validated via TypeId matching
- Bounds checking in debug, stripped in release for zero-cost abstraction

### Working with Components

**Define a component:**
```rust
use rusty_macros::Component;

#[derive(Component)]
struct Position { x: f32, y: f32 }
```

**Spawn an entity:**
```rust
// Single component
let entity = world.spawn(Position { x: 0.0, y: 0.0 });

// Multiple components (tuples)
let entity = world.spawn((
    Position { x: 1.0, y: 2.0 },
    Velocity { dx: 0.5, dy: 0.3 }
));
```

**Query entities:**
```rust
let query = Query::<(&Position, &Velocity)>::new(world.components());
for (pos, vel) in query.invoke(&mut world) {
    // Process each entity with Position + Velocity
}
```

**Table-level access (lower-level):**
```rust
// View single entity components
let (pos, vel): (&Position, &Velocity) = unsafe {
    table.view(row).unwrap()
};

// Iterate over all entities in a table (returns entity + view tuple)
for (entity, (pos, vel)) in unsafe {
    table.iter_views::<(&mut Position, &Velocity)>()
} {
    pos.x += vel.dx * dt;
    pos.y += vel.dy * dt;
}
```

## Proc Macros

The `rusty_macros` crate (located in `engine/src/ecs/macros/`) provides:
- `#[derive(Component)]` - Implements component registration traits

When modifying macro code, changes require rebuilding dependents.

## Important Design Decisions

1. **Columnar Storage**: Components stored in separate arrays (Structure of Arrays) for cache efficiency
2. **Type Erasure**: Components use runtime type registration for flexibility
3. **Archetype Pattern**: Component addition/removal moves entities between tables (trade-off for fast iteration)
4. **No Thread Safety**: Storage types require external synchronization for parallel access
5. **Zero-Cost Abstractions**: Debug-only safety checks, raw pointer access in release builds

## Unsafe Code Justification

The ECS uses `unsafe` in two key places, both justified by external guarantees:

### System Parameter Extraction (`System::run`)
Systems with multiple parameters (e.g., `Query<&Position>, Query<&mut Velocity>`) create aliased mutable references to a `Shard`. This is safe because:
- Each parameter accesses **disjoint component data** (validated at system registration)
- The scheduler ensures **no concurrent conflicting access** across systems
- This is the same pattern as `slice::split_at_mut()` - aliased refs to disjoint data

### Parallel System Execution (`Phase::run_group`)
Worker threads access different systems from `Vec<System>` via raw pointers. This is safe because:
- Scheduler guarantees **disjoint system indices** per unit
- Scoped execution ensures systems **aren't dropped** while workers run
- `&mut` references aren't `Send`, but wrapped raw pointers are

See `ECS_SHARD_DESIGN.md` for the full parallel execution pattern.

## Testing Strategy

Tests are co-located with implementation:
- Unit tests in `#[cfg(test)] mod tests` blocks within each module
- Storage layer has comprehensive coverage (107+ tests)
- Test component migration when adding/removing components
- Validate archetype invariants in table operations

**Key test patterns:**
```rust
#[derive(Component)]
struct TestComponent { /* fields */ }

#[test]
fn test_name() {
    let registry = component::Registry::new();
    let id = registry.register::<TestComponent>();
    // ... test logic
}
```

## Documentation

Key documentation files:
- `CLAUDE.md` - This file - project overview and guide for Claude Code sessions
- `ECS_STORAGE_NOTES.md` - Storage layer patterns, safety mechanisms, and View system
- `ECS_SYSTEM_REFERENCE.md` - System parameter design (GAT + HRTB pattern)
- `ECS_SHARD_DESIGN.md` - Shard pattern for parallel execution, thread safety model
- `ECS_SCHEDULER_ALGORITHMS.md` - Graph coloring algorithms for system scheduling
- `ECS_STORAGE_CHANGE_DESIGN.md` - Design for storage change/migration system (WIP)

Engine README describes the dual-thread architecture and core loop design.

## Rust Edition

This project uses Rust edition 2024. Ensure compatibility when adding dependencies or using language features.
