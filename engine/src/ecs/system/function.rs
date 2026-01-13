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
//! fn my_system(query: query::Result<(&Velocity, &mut Position)>) {
//!     for (vel, pos) in query {
//!         pos.x += vel.dx;
//!     }
//! }
//! ```
//!
//! These functions are turned into systems via the [`IntoSystem`] trait, which is implemented
//!
//! ```rust,ignore
//! let system = IntoSystem::into_system(my_system, world);
//! ```
//!
//! # The Parameter Extraction Process
//!
//! 1. **Function signature**: `fn(query: query::Result<&Comp>)` has elided lifetime `'_`
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
        component,
        system::{IntoSystem, System, param::Parameter},
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
/// - **Parameter type** with elided lifetime (e.g., `query::Result<&Comp>`)
/// - **Value type** with any world lifetime `'a` (e.g., `query::Result<'a, &Comp>`)
/// - The state lifetime `'_` is handled separately during extraction
///
/// When executed, `'a` becomes `'w` (the world's lifetime), bridging the gap.
///
/// # Why This Works
///
/// 1. You write: `fn my_system(query: query::Result<&Position>)`
/// 2. Elided lifetime: Actually `query::Result<'_, &Position>`
/// 3. HRTB `for<'a>`: Matches any lifetime, including `'_`
/// 4. Runtime: Passes `query::Result<'w, &Position>` where `'w` is world lifetime
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
/// fn movement(query: query::Result<&Position>) { }
/// // Implements: WithSystemParams<(query::Result<'_, &Position>,)>
///
/// // Multiple parameters
/// fn physics(
///     positions: query::Result<&Position>,
///     velocities: query::Result<&mut Velocity>,
/// ) { }
/// // Implements: WithSystemParams<(
/// //     query::Result<'_, &Position>,
/// //     query::Result<'_, &mut Velocity>,
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
    /// - `components`: Component registry for looking up component IDs
    ///
    /// # Returns
    ///
    /// A [`world::AccessRequest`] describing read/write access to all world resources
    /// needed by the system's parameters.
    fn required_access(components: &component::Registry) -> world::AccessRequest;

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
    unsafe fn run(&mut self, shard: &mut world::Shard<'_>, state: &mut State);

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
/// let system = IntoSystem::into_system(&world, tick_counter);
/// ```
impl<Func> WithSystemParams<(), ()> for Func
where
    Func: FnMut() + 'static,
{
    /// Returns an empty access request since no components are accessed.
    fn required_access(_components: &component::Registry) -> world::AccessRequest {
        world::AccessRequest::NONE
    }

    /// Invokes the function without accessing the shard.
    unsafe fn run(&mut self, _shard: &mut world::Shard<'_>, _state: &mut ()) {
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
            fn required_access(components: &component::Registry) -> world::AccessRequest {
                // Merge component specs from all parameters
                let mut access = world::AccessRequest::NONE;
                $(
                    access = access.merge(&$param::required_access(components));
                )*
                access
            }

            fn build_state(world: &mut world::World) -> ($(<$param as Parameter>::State,)*) {
                (
                    $(
                        $param::build_state(world),
                    )*
                )
            }

            unsafe fn run(&mut self, shard: &mut world::Shard<'_>, state:  &mut ($($param::State,)*)) {
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
                    let $param = unsafe { $param::get(&mut *(shard as *mut world::Shard<'_>), $param) };
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
    fn into_system(mut instance: Self, _world: &mut world::World) -> System {
        System::exclusive(world::AccessRequest::to_world(true), move |world| {
            instance(world)
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
    fn into_system(mut instance: Self, world: &mut world::World) -> System {
        let access = Func::required_access(world.components());
        let mut state = Func::build_state(world);
        System::parallel(access, move |shard| unsafe {
            instance.run(shard, &mut state);
        })
    }
}

#[cfg(test)]
mod tests {

    use crate::ecs::{
        component, query,
        system::{IntoSystem, System},
        world,
    };

    use rusty_macros::Component;

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }

    fn into_system<M>(world: &mut world::World, sys: impl IntoSystem<M>) -> System {
        IntoSystem::into_system(sys, world)
    }

    #[test]
    fn no_param_function_system() {
        // Given

        fn my_system() {
            // No-op
        }

        let mut world = world::World::new(world::Id::new(0));

        // When
        let mut system = into_system(&mut world, my_system);

        // Then
        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn world_mut_param_function_system() {
        fn my_system(world: &mut world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }

        let mut world = world::World::new(world::Id::new(0));
        let mut system = into_system(&mut world, my_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn world_param_function_system() {
        fn my_system(world: &world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }

        let mut world = world::World::new(world::Id::new(0));
        let mut system = into_system(&mut world, my_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn single_query_handle_function_system() {
        fn my_system(query: query::Result<&Comp1>) {
            for comp in query {
                assert_eq!(comp.value, 42);
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 42 });

        let mut system = into_system(&mut world, my_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn mutable_query_system() {
        fn increment_system(query: query::Result<&mut Comp1>) {
            for comp in query {
                comp.value += 1;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 5 });
        world.spawn(Comp1 { value: 10 });

        let mut system = into_system(&mut world, increment_system);

        unsafe {
            system.run(&mut world);
        }

        // Verify values were incremented
        let mut values: Vec<i32> = world.query::<&Comp1>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![6, 11]);
    }

    #[test]
    fn multiple_entities_query_system() {
        fn count_system(query: query::Result<(&Comp1, &Comp2)>) {
            let count = query.count();
            assert_eq!(count, 2);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
        world.spawn(Comp1 { value: 3 }); // Only Comp1, won't match

        let mut system = into_system(&mut world, count_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn mixed_mutability_query_system() {
        fn physics_system(query: query::Result<(&Comp1, &mut Comp2)>) {
            for (c1, c2) in query {
                c2.value = c1.value * 2;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 5 }, Comp2 { value: 0 }));
        world.spawn((Comp1 { value: 10 }, Comp2 { value: 0 }));

        let mut system = into_system(&mut world, physics_system);

        unsafe {
            system.run(&mut world);
        }

        // Verify Comp2 values were updated
        let mut values: Vec<i32> = world.query::<&Comp2>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![10, 20]);
    }

    #[test]
    fn multiple_query_parameters_system() {
        fn two_query_system(query1: query::Result<&Comp1>, query2: query::Result<&Comp2>) {
            let count1 = query1.count();
            let count2 = query2.count();
            assert_eq!(count1, 3);
            assert_eq!(count2, 2);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
        world.spawn(Comp1 { value: 3 }); // Only Comp1

        let mut system = into_system(&mut world, two_query_system);

        unsafe {
            system.run(&mut world);
        }
    }

    // NOTE: Tests for systems with both queries and &mut World have been removed.
    // Such systems are no longer supported - use WorldSystem for exclusive world access,
    // or Wrapper for query-based systems. This separation ensures that systems with
    // exclusive world access cannot accidentally run in parallel.

    #[test]
    fn access_single_query() {
        // Given
        fn my_system(_query: query::Result<&Comp1>) {}

        let mut world = world::World::new(world::Id::new(0));
        world.components().register::<Comp1>();

        let system = into_system(&mut world, my_system);
        let access = system.required_access();

        // Should have Comp1 in the access request
        assert_eq!(
            *access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![world.components().get::<Comp1>().unwrap()]),
                component::Spec::EMPTY
            )
        );
    }

    #[test]
    fn access_multiple_queries() {
        fn my_system(_query1: query::Result<&Comp1>, _query2: query::Result<&Comp2>) {}

        let mut world = world::World::new(world::Id::new(0));
        let id1 = world.components().register::<Comp1>();
        let id2 = world.components().register::<Comp2>();

        let system = into_system(&mut world, my_system);
        let access = system.required_access();

        // Should have both components in the merged spec
        assert_eq!(
            *access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![id1, id2]),
                component::Spec::EMPTY
            )
        )
    }

    #[test]
    fn component_spec_mixed_query() {
        fn my_system(_query: query::Result<(&Comp1, &Comp2)>) {}

        let mut world = world::World::new(world::Id::new(0));
        let id1 = world.components().register::<Comp1>();
        let id2 = world.components().register::<Comp2>();

        let system = into_system(&mut world, my_system);
        let access = system.required_access();

        // Should have both components
        assert_eq!(
            *access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![id1, id2]),
                component::Spec::EMPTY
            )
        )
    }

    #[test]
    fn entity_id_query_system() {
        use crate::ecs::entity;

        fn entity_system(query: query::Result<(entity::Entity, &Comp1)>) {
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

        let mut system = into_system(&mut world, entity_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn empty_query_system() {
        fn empty_system(query: query::Result<&Comp1>) {
            let count = query.count();
            assert_eq!(count, 0);
        }

        let mut world = world::World::new(world::Id::new(0));
        // Don't spawn any entities

        let mut system = into_system(&mut world, empty_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn two_query_parameter_system() {
        fn two_query_system(query1: query::Result<&Comp1>, query2: query::Result<&Comp2>) {
            assert_eq!(query1.count(), 2);
            assert_eq!(query2.count(), 1);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 1 });
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 10 }));

        let mut system = into_system(&mut world, two_query_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn system_can_be_run_multiple_times() {
        fn increment_system(query: query::Result<&mut Comp1>) {
            for comp in query {
                comp.value += 1;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 0 });

        let mut system = into_system(&mut world, increment_system);

        // Run system 3 times
        unsafe {
            system.run(&mut world);
            system.run(&mut world);
            system.run(&mut world);
        }

        // Verify value was incremented 3 times
        let value = world.query::<&Comp1>().next().unwrap().value;
        assert_eq!(value, 3);
    }

    #[test]
    fn mutable_query_with_multiple_entities() {
        fn multiply_system(query: query::Result<(&Comp1, &mut Comp2)>) {
            for (c1, c2) in query {
                c2.value *= c1.value;
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 3 }));
        world.spawn((Comp1 { value: 4 }, Comp2 { value: 5 }));
        world.spawn((Comp1 { value: 10 }, Comp2 { value: 10 }));

        let mut system = into_system(&mut world, multiply_system);

        unsafe {
            system.run(&mut world);
        }

        // Verify values were multiplied
        let mut values: Vec<i32> = world.query::<&Comp2>().map(|c| c.value).collect();
        values.sort();
        assert_eq!(values, vec![6, 20, 100]);
    }

    #[test]
    fn component_spec_empty_system() {
        fn my_system() {}

        let mut world = world::World::new(world::Id::new(0));
        let system = into_system(&mut world, my_system);
        let access = system.required_access();

        // Should have empty spec
        assert!(access.is_none());
    }

    #[test]
    fn component_spec_world_only_system() {
        // Given
        fn my_system(_world: &mut world::World) {}

        let mut world = world::World::new(world::Id::new(0));

        let system = into_system(&mut world, my_system);

        // When
        let access = system.required_access();

        // Then
        assert!(access.world_mut());
    }
}
