//! Function system wrapper and parameter extraction.
//!
//! This module provides [`WithSystemParams`], the trait that enables parameter extraction. This
//! trait combined with [`IntoSystem`] allows any functions that can be expressed with system
//! parameters to be added as systems.
//!
//! # Overview
//!
//! The function system design allows you to write game logic as regular Rust functions:
//!
//! ```rust,ignore
//! fn my_system(query: Query<(&Velocity, &mut Position)>) {
//!     for (vel, pos) in query {
//!         pos.x += vel.dx;
//!     }
//! }
//! ```
//!
//! These functions are turned into systems via the [`IntoSystem`] trait, which is implemented
//!
//! ```rust,ignore
//! let system = my_system.into_system(, world);
//! ```
//!
//! # The Parameter Extraction Process
//!
//! 1. **Function signature**: `fn(query: Query<&Comp>)` has elided lifetime `'_`
//! 2. **Wrapper creation**: Analyzes parameters via [`Parameter::required_access()`]
//! 3. **System execution**: Calls [`Parameter::get()`] for each parameter
//! 4. **Function invocation**: Passes runtime values with world lifetime `'w`
//!
//! # Safety
//!
//! The implementation uses raw pointers to create aliased mutable world references.
//! This is safe because:
//! - Each parameter accesses disjoint data (different components)
//! - Component specs validate no aliasing occurs
//! - The scheduler ensures systems don't conflict
//!
//! See [`WithSystemParams`] for detailed safety documentation.

use crate::{
    all_tuples,
    ecs::{
        system::{CommandBuffer, IntoSystem, System, param::Parameter},
        world,
    },
};

/// Trait enabling functions to be called with system parameters.
///
/// This trait bridges the gap between clean function signatures (with elided lifetimes)
/// and runtime execution (with world lifetime `'w`). It's the core of what makes
/// parameter extraction work.
///
/// The implementations use Higher-Ranked Trait Bounds to achieve lifetime flexibility:
///
/// ```rust,ignore
/// impl<Func, A: Parameter> WithSystemParams<(A,), (A::State,)> for Func
/// where
///     for<'a> &'a mut Func:
///         FnMut(A) +                    // Signature: accepts parameter type
///         FnMut(A::Value<'a, '_>),      // Runtime: accepts value with any lifetime
/// ```
///
/// This says the function must work with:
/// - **Parameter type** with elided lifetime (e.g., `Query<&Comp>`)
/// - **Value type** with any world lifetime `'a` (e.g., `Query<'a, &Comp>`)
/// - The state lifetime `'_` is handled separately during extraction
///
/// When executed, `'a` becomes `'w` (the world's lifetime), bridging the gap.
///
/// # Why This Works
///
/// 1. You write: `fn my_system(query: Query<&Position>)`
/// 2. Elided lifetime: Actually `Query<'_, &Position>`
/// 3. HRTB `for<'a>`: Matches any lifetime, including `'_`
/// 4. Runtime: Passes `Query<'w, &Position>` where `'w` is world lifetime
/// 5. Type system: Verifies everything is sound
///
/// # Implementation
///
/// You don't implement this trait manually. It's implemented by macros for:
/// - Functions with 0 parameters: `fn()`
/// - Functions with 1-26 parameters: `fn(A)`, `fn(A, B)`, ..., `fn(A, B, ..., Z)`
///
/// Each implementation:
/// 1. Merges required_access from all parameters
/// 2. Extracts parameter values from world at runtime
/// 3. Calls the function with the extracted values
///
/// # Safety
///
/// The `run` method is executed with an access restricted world [`Shard`]. This is safe because:
/// - Each parameter accesses disjoint data (different components)
/// - AccessGrant validate no aliasing at system registration
/// - The scheduler ensures no conflicting systems run concurrently
///
/// # Examples
///
/// ```rust,ignore
/// // Zero parameters
/// fn tick() { }
/// // Implements: WithSystemParams<()>
///
/// // One parameter
/// fn movement(query: Query<&Position>) { }
/// // Implements: WithSystemParams<(Query<'_, &Position>,)>
///
/// // Multiple parameters
/// fn physics(
///     positions: Query<&Position>,
///     velocities: Query<&mut Velocity>,
/// ) { }
/// // Implements: WithSystemParams<(
/// //     Query<'_, &Position>,
/// //     Query<'_, &mut Velocity>,
/// // )>
/// ```
pub trait WithSystemParams<Params, State>: 'static {
    /// Compute the combined required world access for all parameters.
    ///
    /// This merges the required_access from each parameter to create a unified
    /// spec describing all components this system accesses and how.
    ///
    /// # Parameters
    ///
    /// - `world`: The world to look up component or resource info from
    ///
    /// # Returns
    ///
    /// A [`world::AccessRequest`] describing read/write access to all world resources
    /// needed by the system's parameters.
    fn required_access(world: &world::World) -> world::AccessRequest;

    /// Execute the function with parameters extracted from the shard.
    ///
    /// This method:
    /// 1. Extracts each parameter's value from the shard via [`Parameter::get()`]
    /// 2. Invokes the function with all extracted values
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// - Access requests have been validated (no aliasing violations)
    /// - No other system is concurrently accessing conflicting components
    /// - Shard has appropriate grant for the required access
    ///
    /// The implementation creates aliased mutable shard pointers, which is safe
    /// only when parameters access disjoint data.
    ///
    /// # Parameters
    ///
    /// - `shard`: Mutable shard reference with validated grant
    /// - `state`: Mutable reference to parameter state
    /// - `command_buffer`: reference to the command buffer
    unsafe fn run(
        &mut self,
        shard: &mut world::Shard<'_>,
        state: &mut State,
        command_buffer: &CommandBuffer,
    );

    /// Build the state needed for parameter extraction.
    ///
    /// This method is given mutable access to the world to gain access to any key resorurces or
    /// setup any resources needed for this parameter state.
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable world reference to build state from
    ///
    /// # Returns
    ///
    /// A [`State`] instance to be stored and passed to `run` during execution.
    fn build_state(world: &mut world::World) -> State;
}

/// Implementation for functions with zero parameters.
///
/// Allows systems that don't need any world data, useful for:
/// - Logging or telemetry
/// - Time-based ticking
/// - State machine transitions
/// - Any logic that doesn't query entities or components
///
/// # Example
///
/// ```rust,ignore
/// fn tick_counter() {
///     println!("Tick!");
/// }
///
/// let system = ticke_counter.into_system(&world);
/// ```
impl<Func> WithSystemParams<(), ()> for Func
where
    Func: FnMut() + 'static,
{
    /// Returns an empty access request since no components are accessed.
    fn required_access(_world: &world::World) -> world::AccessRequest {
        world::AccessRequest::NONE
    }

    /// Invokes the function without accessing the shard.
    unsafe fn run(
        &mut self,
        _shard: &mut world::Shard<'_>,
        _state: &mut (),
        _command_buffer: &CommandBuffer,
    ) {
        self();
    }

    // Build empty state
    fn build_state(_world: &mut world::World) {}
}

/// Macro implementing [`WithSystemParams`] for functions with N parameters.
///
/// This macro generates implementations for functions with specific parameter counts.
/// For example, `system_param_function_impl!(A, B)` implements `WithSystemParams<(A, B)>`
/// for any function `Fn(A, B)` where `A` and `B` implement [`Parameter`].
///
/// # Generated Implementation
///
/// - **required_access**: Merges access requests from all parameters
/// - **run**: Extracts each parameter from world, calls function
///
/// # The HRTB Constraint
///
/// ```ignore
/// for<'a> &'a mut Func: FnMut($($param),*) + FnMut($($param::Value<'a, '_>),*)
/// ```
///
/// This requires the function to accept both:
/// 1. Parameters with elided lifetimes (signature)
/// 2. Parameter values with any world lifetime (runtime)
/// 3. State lifetime is handled separately during extraction (the second `'_`)
///
/// # Safety
///
/// The `run` implementation creates aliased mutable world pointers by casting
/// `&mut Shard` to `*mut Shard` for each parameter. This is safe because:
/// - Each `Parameter::get()` accesses disjoint components
/// - Component specs are validated before execution
/// - The scheduler prevents concurrent conflicting access
macro_rules! system_param_function {
    ($($param:ident),*) => {
        impl<Func, $($param: Parameter, )*> WithSystemParams<($($param, )*), ($($param::State,)*)> for Func
        where
            Func: 'static,
            // HRTB: Function must work with both elided lifetimes (signature)
            // and any specific lifetime 'w (runtime with world lifetime)
            for<'w> &'w mut Func: FnMut($($param),*) + FnMut($($param::Value<'w, '_>),*),
        {
            /// Determine the world access this set of parameters requires. This will merge all the
            /// individual parameter access requests into a single request.
            ///
            /// # Panics
            /// If any of the parameters have conflicting access (e.g., two mutable accesses to the
            /// same component type),
            fn required_access(world: &world::World) -> world::AccessRequest {
                // Merge component specs from all parameters, but always ensure no conflicts
                let mut access = world::AccessRequest::NONE;
                $(
                    let required = $param::required_access(world);
                    assert!(!access.conflicts_with(&required), "Conflicting access in system parameters");
                    access = access.merge(&required);
                )*
                access
            }

            /// Build the parameter state for this set of parameters. The resulting state will be a
            /// tuple of individual parameter states.
            fn build_state(world: &mut world::World) -> ($(<$param as Parameter>::State,)*) {
                (
                    $(
                        $param::build_state(world),
                    )*
                )
            }

            /// Run the parameter extraction and call the function with extracted parameters.
            ///
            /// # Safety
            /// Caller must ensure:
            /// - Access requests have been validated (no aliasing violations)
            /// - No other system is concurrently accessing conflicting components
            unsafe fn run(&mut self, shard: &mut world::Shard<'_>, state:  &mut ($($param::State,)*), command_buffer: &CommandBuffer) {
                // Helper function to call with extracted parameters
                // Needed because we can't directly call self($($param),*) due to macro hygiene
                #[allow(clippy::too_many_arguments, non_snake_case)]
                fn call_it<$($param),*>(mut func: impl FnMut($($param),*), $($param: $param),*) {
                    func($($param),*);
                }

                #[allow(non_snake_case)]
                let ($($param,)*) = state;

                // Extract each parameter from the shard
                $(
                    // SAFETY: Creating aliased mutable shard pointers is safe because:
                    // 1. Each Parameter::get() accesses different components (disjoint data)
                    // 2. Access requests validated this at system registration
                    // 3. Scheduler ensures no concurrent conflicting access
                    // 4. Shard has grant covering all required access
                    #[allow(non_snake_case)]
                    let $param = unsafe { $param::extract(&mut *(shard as *mut world::Shard<'_>), $param, command_buffer)};
                )*

                // Call the function with all extracted parameters
                call_it(self, $($param),*);
            }
        }
    };
}

// Generate WithSystemParams implementations for functions with 1-26 parameters.
// This covers the vast majority of real-world systems. If you need more than 26 parameters,
// consider breaking your system into smaller systems or using a resource to pass shared data.
all_tuples!(system_param_function);

/// Marker type for `fn(&mut World)` to `WorldSystem` conversion.
///
/// This distinguishes world functions from parameter-based functions in the
/// `IntoSystem` trait implementation.
pub struct WorldFnMarker;

/// Implement [`IntoSystem`] for functions that take only `&mut World`.
///
/// This creates an exclusive system that requires main-thread execution with full
/// mutable world access. The [`WorldFnMarker`] distinguishes this from parameter-based systems.
impl<F> IntoSystem<WorldFnMarker> for F
where
    F: FnMut(&mut world::World) + 'static,
{
    fn into_system(mut self, _world: &mut world::World) -> System {
        System::exclusive(world::AccessRequest::to_world(true), move |world| {
            self(world)
        })
    }
}

/// Implement [`IntoSystem`] for functions with parameters.
///
/// This creates a parallel-capable system when the function and its state are `Send + Sync`.
/// The marker type `(Params, State)` distinguishes this implementation from the
/// exclusive world function implementation.
impl<Func, Params, State> IntoSystem<(Params, State)> for Func
where
    Func: WithSystemParams<Params, State> + Send + Sync + 'static,
    Params: 'static,
    State: Send + Sync + 'static,
{
    fn into_system(mut self, world: &mut world::World) -> System {
        let access = Func::required_access(world);
        let mut state = Func::build_state(world);
        System::parallel(access, move |shard, command_buffer| unsafe {
            self.run(shard, &mut state, command_buffer);
        })
    }
}

#[cfg(test)]
mod tests {

    use crate::ecs::{
        entity,
        system::{CommandBuffer, Commands, Consumer, IntoSystem, Producer, param::Query},
        world,
    };

    use rusty_macros::{Component, Event};

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }
    fn run_exclusive<M>(system: impl IntoSystem<M>, world: &mut world::World) {
        let mut system = system.into_system(world);
        unsafe {
            system.run_exclusive(world);
        }
    }

    fn run_parallel<M>(system: impl IntoSystem<M>, world: &mut world::World) {
        let mut system = system.into_system(world);
        let command_buffer = CommandBuffer::new();
        {
            let mut shard = world.shard(system.required_access()).unwrap();
            unsafe {
                system.run_parallel(&mut shard, &command_buffer);
            }
        }
        command_buffer.flush(world);
    }

    #[test]
    fn no_param_function_system() {
        // Given
        fn my_system() {
            // No-op
        }
        let mut world = world::World::new(world::Id::new(0));

        // Then - Should not panic
        run_parallel(my_system, &mut world);
    }

    #[test]
    fn world_mut_param_function_system() {
        // Given
        fn my_system(world: &mut world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }
        let mut world = world::World::new(world::Id::new(0));

        // Then - Should not panic
        run_exclusive(my_system, &mut world);
    }

    #[test]
    fn world_param_function_system() {
        // Given
        fn my_system(world: &world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }
        let mut world = world::World::new(world::Id::new(0));

        // Then - Should not panic
        run_parallel(my_system, &mut world);
    }

    #[test]
    fn single_query_handle_function_system() {
        // Given
        fn my_system(query: Query<&Comp1>) {
            for comp in query {
                assert_eq!(comp.value, 42);
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 42 });

        // Then - Should not panic
        run_parallel(my_system, &mut world);
    }

    #[test]
    fn mutable_query_system() {
        // Given
        fn increment_system(query: Query<&mut Comp1>) {
            for comp in query {
                comp.value += 1;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 5 });
        world.spawn(Comp1 { value: 10 });

        // When
        run_parallel(increment_system, &mut world);

        // Then
        let mut values: Vec<i32> = world.query::<&Comp1>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![6, 11]);
    }

    #[test]
    fn multiple_entities_query_system() {
        // Given
        fn count_system(query: Query<(&Comp1, &Comp2)>) {
            let count = query.count();
            assert_eq!(count, 2);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
        world.spawn(Comp1 { value: 3 }); // Only Comp1, won't match

        // Then - Should not panic
        run_parallel(count_system, &mut world);
    }

    #[test]
    fn mixed_mutability_query_system() {
        // Given
        fn physics_system(query: Query<(&Comp1, &mut Comp2)>) {
            for (c1, c2) in query {
                c2.value = c1.value * 2;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 5 }, Comp2 { value: 0 }));
        world.spawn((Comp1 { value: 10 }, Comp2 { value: 0 }));

        // When
        run_parallel(physics_system, &mut world);

        // Then
        let mut values: Vec<i32> = world.query::<&Comp2>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![10, 20]);
    }

    #[test]
    fn multiple_query_parameters_system() {
        // Given
        fn two_query_system(query1: Query<&Comp1>, query2: Query<&Comp2>) {
            let count1 = query1.count();
            let count2 = query2.count();
            assert_eq!(count1, 3);
            assert_eq!(count2, 2);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
        world.spawn(Comp1 { value: 3 }); // Only Comp1

        // THen - Should not panic
        run_parallel(two_query_system, &mut world);
    }

    #[test]
    fn access_single_query() {
        // Given
        fn my_system(_query: Query<&Comp1>) {}

        let mut world = world::World::new(world::Id::new(0));
        let system = my_system.into_system(&mut world);
        let access = system.required_access();

        // Should have Comp1 in the access request
        assert_eq!(
            *access,
            world::AccessRequest::to_resources(&[world.resources().get::<Comp1>().unwrap()], &[])
        );
    }

    #[test]
    fn access_multiple_queries() {
        // Given
        fn my_system(_query1: Query<&Comp1>, _query2: Query<&Comp2>) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);
        let access = system.required_access();

        // Should have both components in the merged spec
        assert_eq!(
            *access,
            world::AccessRequest::to_resources(
                &[
                    world.resources().get::<Comp1>().unwrap(),
                    world.resources().get::<Comp2>().unwrap()
                ],
                &[]
            )
        )
    }

    #[test]
    fn required_access_mixed_query() {
        // Given
        fn my_system(_query: Query<(&Comp1, &Comp2)>) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);
        let access = system.required_access();

        // Should have both components
        assert_eq!(
            *access,
            world::AccessRequest::to_resources(
                &[
                    world.resources().get::<Comp1>().unwrap(),
                    world.resources().get::<Comp2>().unwrap()
                ],
                &[]
            )
        )
    }

    #[test]
    fn entity_id_query_system() {
        // Given
        fn entity_system(query: Query<(entity::Entity, &Comp1)>) {
            let mut count = 0;
            for (_entity, comp) in query {
                // Just verify we get entity along with component
                assert!(comp.value > 0);
                count += 1;
            }
            assert_eq!(count, 2);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 5 });
        world.spawn(Comp1 { value: 10 });

        // Then - Should not panic
        run_parallel(entity_system, &mut world);
    }

    #[test]
    fn empty_query_system() {
        // Given
        fn empty_system(query: Query<&Comp1>) {
            let count = query.count();
            assert_eq!(count, 0);
        }

        let mut world = world::World::new(world::Id::new(0));
        // Don't spawn any entities

        // Then - Should not panic
        run_parallel(empty_system, &mut world);
    }

    #[test]
    fn two_query_parameter_system() {
        // Given
        fn two_query_system(query1: Query<&Comp1>, query2: Query<&Comp2>) {
            assert_eq!(query1.count(), 2);
            assert_eq!(query2.count(), 1);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 1 });
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 10 }));

        // Then - Should not panic
        run_parallel(two_query_system, &mut world);
    }

    #[test]
    fn system_can_be_run_multiple_times() {
        // Given
        fn increment_system(query: Query<&mut Comp1>) {
            for comp in query {
                comp.value += 1;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 0 });

        // When
        run_parallel(increment_system, &mut world);
        run_parallel(increment_system, &mut world);
        run_parallel(increment_system, &mut world);

        // Then
        let value = world.query::<&Comp1>().next().unwrap().value;
        assert_eq!(value, 3);
    }

    #[test]
    fn mutable_query_with_multiple_entities() {
        // Given
        fn multiply_system(query: Query<(&Comp1, &mut Comp2)>) {
            for (c1, c2) in query {
                c2.value *= c1.value;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 3 }));
        world.spawn((Comp1 { value: 4 }, Comp2 { value: 5 }));
        world.spawn((Comp1 { value: 10 }, Comp2 { value: 10 }));

        // When
        run_parallel(multiply_system, &mut world);

        // Then
        let mut values: Vec<i32> = world.query::<&Comp2>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![6, 20, 100]);
    }

    #[test]
    fn required_access_empty_system() {
        // Given
        fn my_system() {}

        let mut world = world::World::new(world::Id::new(0));
        let system = my_system.into_system(&mut world);
        let access = system.required_access();

        // Should have empty spec
        assert!(access.is_none());
    }

    #[test]
    fn required_access_world_only_system() {
        // Given
        fn my_system(_world: &mut world::World) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);

        // When
        let access = system.required_access();

        // Then
        assert!(access.world_mut());
    }

    #[test]
    fn required_access_commmands_system() {
        // Given
        fn my_system(_commands: Commands) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);

        // When
        let access = system.required_access();

        // Then
        assert!(access.is_none());
    }

    #[test]
    fn parallel_system_run_as_exclusive() {
        // Given
        fn my_system(query: Query<&Comp1>) {
            for comp in query {
                assert_eq!(comp.value, 42);
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 42 });

        // Then - Should not panic
        run_exclusive(my_system, &mut world);
    }

    #[test]
    fn parallel_system_run_as_exclusive_with_commands() {
        // Given
        fn my_system(query: Query<&Comp1>, commands: Commands) {
            for comp in query {
                assert_eq!(comp.value, 42);
            }
            commands.spawn(Comp1 { value: 100 });
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 42 });

        // When
        run_exclusive(my_system, &mut world);

        // Then
        let comps: Vec<&Comp1> = world.query::<&Comp1>().collect();
        assert_eq!(comps.len(), 2);
    }

    #[derive(Event, Debug, Clone)]
    struct TestEvent;

    #[test]
    fn required_access_event_consumer_system() {
        // Given
        fn my_system(_events: Consumer<TestEvent>) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);

        // When
        let access = system.required_access();

        // Then
        assert_eq!(
            *access,
            world::AccessRequest::to_resources(
                &[world.resources().get_event::<TestEvent>().unwrap().1],
                &[]
            )
        );
    }

    #[test]
    fn required_access_event_producer_system() {
        // Given
        fn my_system(_events: Producer<TestEvent>) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = my_system.into_system(&mut world);

        // When
        let access = system.required_access();

        // Then
        assert_eq!(
            *access,
            world::AccessRequest::to_resources(
                &[],
                &[world.resources().get_event::<TestEvent>().unwrap().0],
            )
        );
    }
    //
    // #[test]
    // fn event_producer_consumer_system() {
    //     // Given
    //     fn producer_system(_events: Producer<TestEvent>) {}
    //     fn consumer_system(_events: Consumer<TestEvent>) {}
    //
    //     let mut world = world::World::new(world::Id::new(0));
    //     world.register_event::<TestEvent>();
    //
    //     let producer_system = producer_system.into_system(&mut world);
    //     let consumer_system = consumer_system.into_system(&mut world);
    //
    //     // When
    //     run_parallel();
    // }
}
