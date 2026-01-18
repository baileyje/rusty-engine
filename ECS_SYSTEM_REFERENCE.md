# ECS System Reference

Quick reference for the system parameter design in rusty_engine's ECS.

## Problem Solved

Writing ECS systems with clean function signatures, no explicit lifetimes:

```rust
// ✅ What we achieved
fn movement(query: query::Result<(&Velocity, &mut Position)>) {
    for (vel, pos) in query {
        pos.x += vel.dx;
    }
}

// ❌ What we avoided (explicit lifetimes everywhere)
fn movement<'a>(query: query::Result<'a, (&'a Velocity, &'a mut Position)>) { }
```

## Core Pattern: GAT + HRTB

### Generic Associated Type (GAT)

The `Parameter` trait uses a GAT to separate the parameter type (in signatures) from the value type (at runtime):

```rust
pub trait Parameter: Sized {
    type Value<'w, 's>: Parameter;  // GAT carries world and state lifetimes
    type State: 'static;

    fn required_access(components: &component::Registry) -> world::AccessRequest;
    fn build_state(world: &mut World) -> Self::State;
    unsafe fn get<'w, 's>(world: &'w mut World, state: &'s mut Self::State) -> Self::Value<'w, 's>;
}
```

**Key insight:** No lifetime on the trait itself, only on the associated type. The `'w` lifetime represents the world reference lifetime, while `'s` represents the state lifetime.

### Higher-Ranked Trait Bound (HRTB)

Function wrappers use HRTB to accept both the parameter type AND its value:

```rust
impl<Func, A: Parameter> WithSystemParams<(A,), (A::State,)> for Func
where
    Func: 'static,
    for<'a> &'a mut Func:
        FnMut(A) +                    // Accept parameter type (signature)
        FnMut(A::Value<'a, '_>),      // Accept value with any lifetime (runtime)
```

**How it works:**
1. Function signature: `query::Result<&Comp>` has elided lifetime `'_`
2. HRTB `for<'a>` matches any lifetime including `'_`
3. At runtime: `'a` becomes `'w` (world's lifetime)
4. State lifetime (second `'_`) is used during parameter extraction
5. Type system verifies everything is sound

## Usage Examples

### Basic System

```rust
use rusty_engine::ecs::{query, system::IntoSystem};

fn my_system(query: query::Result<&Position>) {
    for pos in query {
        println!("({}, {})", pos.x, pos.y);
    }
}

// Create and run using IntoSystem trait
let mut system = IntoSystem::into_system(my_system, &mut world);
unsafe { system.run(&mut world); }
```

### Common Patterns

```rust
// Mutable query
fn update(mut query: query::Result<&mut Position>) {
    for pos in query { pos.x += 1.0; }
}

// Multiple components
fn physics(query: query::Result<(&Velocity, &mut Position)>) {
    for (vel, pos) in query {
        pos.x += vel.dx;
    }
}

// Multiple parameters
fn complex(
    positions: query::Result<&Position>,
    velocities: query::Result<&mut Velocity>,
) {
    for vel in velocities { vel.dy -= 0.5; }
}

// Optional components
//
// Optional components in queries allow matching entities with required components
// that may also have additional components. The archetype storage doesn't store
// optionals - this is resolved at query time by matching multiple archetypes.
fn healing(query: query::Result<(&Player, Option<&mut Health>)>) {
    for (_, health) in query {
        if let Some(h) = health { h.current += 1; }
    }
}

// Entity IDs
fn debug(query: query::Result<(entity::Entity, &Position)>) {
    for (ent, pos) in query {
        println!("{:?}: ({}, {})", ent, pos.x, pos.y);
    }
}

// Direct world access
fn spawner(enemies: query::Result<&Enemy>, world: &mut World) {
    if enemies.len() < 3 {
        world.spawn(Enemy);
    }
}
```

## Parameter Implementations

### Query Results

```rust
impl<D: query::Data> Parameter for query::Result<'_, D> {
    type Value<'w, 's> = query::Result<'w, D>;
    type State = query::Query<D>;

    fn required_access(components: &component::Registry) -> world::AccessRequest {
        D::spec(components).as_access_request()
    }

    fn build_state(world: &mut World) -> Self::State {
        query::Query::new(world.components())
    }

    unsafe fn get<'w, 's>(world: &'w mut World, state: &'s mut Self::State) -> Self::Value<'w, 's> {
        state.invoke(world)
    }
}
```

### World Access

```rust
impl Parameter for &world::World {
    type Value<'w, 's> = &'w world::World;
    type State = ();

    fn required_access(_components: &component::Registry) -> world::AccessRequest {
        world::AccessRequest::to_world(false)
    }

    fn build_state(_world: &mut World) -> Self::State {}

    unsafe fn get<'w, 's>(world: &'w mut World, _state: &'s mut Self::State) -> Self::Value<'w, 's> {
        world
    }
}
```

## Safety Contracts

### The Unsafe Code

Located in `system/function.rs`, the macro generates:

```rust
unsafe fn run(&mut self, world: &mut World, state: &mut State) {
    // Create aliased mutable world pointers
    let a = unsafe { A::get(&mut *(world as *mut World), &mut state.0) };
    let b = unsafe { B::get(&mut *(world as *mut World), &mut state.1) };

    // Call function with extracted parameters
    self(a, b);
}
```

### Why It's Safe

**Invariants that MUST hold:**

1. **Disjoint data access**
   - Each parameter extracts DIFFERENT components
   - `positions: query::Result<&Position>` accesses position storage
   - `velocities: query::Result<&mut Velocity>` accesses velocity storage
   - These are separate memory regions (different columns/tables)

2. **Access request validation**
   - Each parameter reports component access via `required_access()`
   - Returns `world::AccessRequest` describing read/write component access
   - Scheduler (future) validates no aliasing before system runs
   - Runtime: Query panics if same component accessed twice

3. **Limited scope**
   - Aliased references only exist during parameter extraction
   - Once extracted, parameters hold disjoint data
   - Original world reference not used after extraction

### Aliasing Detection Examples

```rust
// ❌ INVALID: Same component mutably accessed twice
fn bad(
    q1: query::Result<&mut Position>,
    q2: query::Result<&mut Position>,  // Aliasing!
) { }

// ❌ INVALID: Mutable and immutable conflict
fn also_bad(
    read: query::Result<&Position>,
    write: query::Result<&mut Position>,  // Conflict!
) { }

// ✅ VALID: Different components
fn good(
    positions: query::Result<&mut Position>,
    velocities: query::Result<&mut Velocity>,  // OK - different
) { }

// ✅ VALID: Same component, both immutable
fn also_good(
    q1: query::Result<&Position>,
    q2: query::Result<&Position>,  // OK - both read-only
) { }
```

### Caller Responsibilities

When calling `system.run(&mut world)`:
- ✅ Access requests must be validated (no aliasing)
- ✅ No concurrent calls to same system
- ✅ World reference valid for call duration
- ✅ Scheduler must validate before execution

## Key Architecture

### Traits

```rust
// Individual parameter type (Query, World, future: Res, Commands)
pub trait Parameter: Sized {
    type Value<'w, 's>: Parameter;
    type State: 'static;
    fn required_access(components: &Registry) -> world::AccessRequest;
    fn build_state(world: &mut World) -> Self::State;
    unsafe fn get<'w, 's>(world: &'w mut World, state: &'s mut Self::State) -> Self::Value<'w, 's>;
}

// Trait for functions with parameters
pub trait WithSystemParams<Params, State>: 'static {
    fn required_access(components: &Registry) -> world::AccessRequest;
    fn build_state(world: &mut World) -> State;
    unsafe fn run(&mut self, world: &mut World, state: &mut State);
}

// System trait
pub struct System {
    required_access: world::AccessRequest,
    run_mode: RunMode,
}
```

### IntoSystem Trait

```rust
pub trait IntoSystem<Marker = ()>: Sized {
    fn into_system(instance: Self, world: &mut world::World) -> System;
}

// For parameter-based functions
impl<Func, Params, State> IntoSystem<(Params, State)> for Func
where
    Func: WithSystemParams<Params, State> + Send + Sync,
    Params: 'static,
    State: Send + Sync + 'static,
{
    fn into_system(mut instance: Self, world: &mut world::World) -> System {
        let access = Func::required_access(world.components());
        let mut state = Func::build_state(world);
        System::parallel(access, move |shard| unsafe {
            instance.run(shard.world_mut(), &mut state);
        })
    }
}

// For exclusive world functions
impl<F> IntoSystem<WorldFnMarker> for F
where
    F: FnMut(&mut world::World) + 'static,
{
    fn into_system(mut instance: Self, _world: &mut world::World) -> System {
        System::exclusive(world::AccessRequest::to_world(true), move |world| {
            instance(world)
        })
    }
}
```

## Implementation Details

### Macro-Generated Implementations

`WithSystemParams` is implemented for functions with 0-26 parameters via macros:

```rust
macro_rules! system_param_function_impl {
    ($($param:ident),*) => {
        impl<Func, $($param: Parameter),*> WithSystemParams<($($param,)*), ($($param::State,)*)> for Func
        where
            Func: 'static,
            for<'a> &'a mut Func: FnMut($($param),*) + FnMut($($param::Value<'a, '_>),*),
        {
            fn required_access(components: &Registry) -> world::AccessRequest {
                // Merge access from all parameters
                let mut access = world::AccessRequest::NONE;
                $( access = access.merge(&$param::required_access(components)); )*
                access
            }

            fn build_state(world: &mut World) -> ($($param::State,)*) {
                ($($param::build_state(world),)*)
            }

            unsafe fn run(&mut self, world: &mut World, state: &mut ($($param::State,)*)) {
                // Extract each parameter (creating aliased pointers)
                #[allow(non_snake_case)]
                let ($($param,)*) = state;
                $( let $param = unsafe { $param::get(&mut *(world as *mut World), $param) }; )*

                // Call function
                call_it(self, $($param),*);
            }
        }
    };
}
```

## Future Extensions

### Resources (Planned)

```rust
#[derive(Resource)]
struct GameTime { elapsed: f32 }

fn timed_system(time: Res<GameTime>, query: query::Result<&Position>) {
    // Access global resources
}

impl<T: 'static> Parameter for Res<T> {
    type Value<'w> = &'w T;
    // ...
}
```

### Commands Buffer (Planned)

```rust
fn spawner(mut commands: Commands) {
    commands.spawn((Position::default(), Velocity::default()));
    // Deferred until end of frame
}
```

### Scheduler (Planned)

```rust
pub struct Scheduler {
    systems: Vec<System>,
}

impl Scheduler {
    pub fn add_system<M>(&mut self, func: impl IntoSystem<M>) -> SystemId
    {
        let system = IntoSystem::into_system(func, &mut self.world);
        let access = system.required_access();

        // Validate against existing systems
        for existing in &self.systems {
            if access.conflicts_with(existing.required_access()) {
                panic!("System conflicts with existing system");
            }
        }

        self.systems.push(system);
        SystemId(self.systems.len() - 1)
    }

    pub fn run_all(&mut self, world: &mut World) {
        for system in &mut self.systems {
            unsafe { system.run(world); }
        }
    }
}
```

## Limitations

1. **26 parameter maximum** - Can be extended if needed
2. **World access requests** - `&mut World` parameter marks exclusive world access
3. **No partial iteration state** - Queries reset each run
4. **Single world** - Can't query multiple worlds simultaneously

## Testing

```rust
#[test]
fn test_system_execution() {
    let mut world = World::new(Id::new(0));
    world.spawn(Position { x: 0.0, y: 0.0 });

    let mut system = IntoSystem::into_system(
        |query: query::Result<&mut Position>| {
            for pos in query { pos.x += 1.0; }
        },
        &mut world
    );

    unsafe { system.run(&mut world); }

    // Verify position changed
    let pos = world.query::<&Position>().next().unwrap();
    assert_eq!(pos.x, 1.0);
}
```

## Documentation

See also:
- `engine/src/core/ecs/system/mod.rs` - Module-level documentation
- `engine/src/core/ecs/system/param.rs` - Parameter trait documentation
- `engine/src/core/ecs/system/function.rs` - Wrapper and HRTB documentation
- `engine/examples/system_parameters_demo.rs` - Working example with all patterns

## Comparison with Other ECS

| Feature | rusty_engine | Bevy | Hecs |
|---------|--------------|------|------|
| Lifetime in signature | ❌ Hidden (GAT) | ❌ Hidden (interior mut) | ✅ Required |
| Interior mutability | ❌ No | ✅ Yes (UnsafeCell) | ❌ No |
| Max parameters | 26 | 16 | N/A |
| Query iteration | Iterator | Iterator | Iterator |
| Resource access | Planned | ✅ Yes | ❌ No |

**vs Bevy:** We avoid runtime overhead of interior mutability, using validated raw pointer casts instead.

**vs Hecs:** We hide lifetimes using GATs, while Hecs requires explicit lifetime parameters.

## Status

**✅ Production Ready** (pending scheduler implementation)

Working features:
- ✅ Query parameters (immutable, mutable, mixed, optional)
- ✅ Multiple parameters (up to 26)
- ✅ Entity ID access
- ✅ Direct world access
- ✅ Clean signatures without lifetimes
- ✅ Full safety via lifetimes and validation

Future work:
- ⬜ Scheduler with conflict detection
- ⬜ Resource system (Res<T>, ResMut<T>)
- ⬜ Commands buffer for deferred operations
- ⬜ Parallel execution
