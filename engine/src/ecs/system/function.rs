//! Function system wrapper and parameter extraction.
//!
//! This module provides [`Wrapper`], which converts regular functions into systems,
//! and [`WithSystemParams`], the trait that enables parameter extraction.
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
//! These functions are wrapped using [`Wrapper`] to implement the [`System`] trait:
//!
//! ```rust,ignore
//! let system = Wrapper::new(world.components(), my_system);
//! ```
//!
//! # The Parameter Extraction Process
//!
//! 1. **Function signature**: `fn(query: query::Result<&Comp>)` has elided lifetime `'_`
//! 2. **Wrapper creation**: Analyzes parameters via [`Parameter::component_spec()`]
//! 3. **System execution**: Calls [`Parameter::get()`] for each parameter
//! 4. **Function invocation**: Passes runtime values with world lifetime `'w`
//!
//! # The HRTB Magic
//!
//! The key to making this work is the Higher-Ranked Trait Bound in [`WithSystemParams`]:
//!
//! ```rust,ignore
//! for<'a> &'a mut Func: FnMut(Param) + FnMut(Param::Value<'a>)
//! ```
//!
//! This says the function must accept:
//! - The **parameter type** (with elided lifetime) - used in signature
//! - The **value type** (with any lifetime `'a`) - used at runtime
//!
//! When executed, `'a` becomes `'w` (world lifetime), bridging the gap.
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

use std::marker::PhantomData;

use crate::ecs::{
    component,
    system::{System, param::Parameter},
    world,
};

/// Wraps a function to implement the [`System`] trait.
///
/// `Wrapper` converts a function that accepts [`Parameter`] types into a system
/// that can be executed on a world. The function can have 0-26 parameters.
///
/// # Type Parameters
///
/// - `F`: The function type (implements [`WithSystemParams`])
/// - `Params`: Tuple of parameter types (e.g., `(QueryParam<&Comp>,)`)
///
/// # Examples
///
/// ```rust,ignore
/// use rusty_engine::ecs::system::function::Wrapper;
///
/// // Simple system with one parameter
/// fn movement(query: query::Result<(&Velocity, &mut Position)>) {
///     for (vel, pos) in query {
///         pos.x += vel.dx;
///     }
/// }
///
/// let system = Wrapper::new(world.components(), movement);
///
/// // System with multiple parameters
/// fn complex(
///     positions: query::Result<&Position>,
///     velocities: query::Result<&mut Velocity>,
///     world: &mut World,
/// ) {
///     // System logic
/// }
///
/// let system = Wrapper::new(world.components(), complex);
/// ```
///
/// # Component Spec
///
/// The wrapper computes a component spec when created by analyzing all parameters.
/// This spec describes which components the system accesses and how (read/write).
///
/// The scheduler will use this to:
/// - Detect conflicts between systems
/// - Determine safe execution order
/// - Enable parallel execution (future)
pub struct Wrapper<F, Params> {
    /// The world accesses required for this system's parameters.
    ///
    /// Computed by merging specs from all parameters via [`WithSystemParams::required_access()`].
    /// Used by the scheduler to validate system compatibility.
    required_access: world::AccessRequest,

    /// The function to execute.
    ///
    /// Must implement [`WithSystemParams`] to be called with world data.
    func: F,

    /// Phantom data for the parameter tuple type.
    ///
    /// Needed for type safety even though we don't store actual parameter values.
    _marker: PhantomData<Params>,
}

impl<F, Params> Wrapper<F, Params> {
    /// Construct a new function system from a function.
    ///
    /// This wraps any function that takes 0-26 [`Parameter`] arguments and converts it
    /// into a [`System`] that can be executed on a world.
    ///
    /// # Parameters
    ///
    /// - `components`: Component registry for looking up component IDs
    /// - `func`: The function to wrap (must implement [`WithSystemParams`])
    ///
    /// # Returns
    ///
    /// A [`Wrapper`] containing the function and its computed component spec.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Zero parameters
    /// fn tick() {
    ///     println!("Tick!");
    /// }
    /// let sys = Wrapper::new(&registry, tick);
    ///
    /// // One parameter
    /// fn movement(query: query::Result<(&Velocity, &mut Position)>) {
    ///     for (vel, pos) in query {
    ///         pos.x += vel.dx;
    ///     }
    /// }
    /// let sys = Wrapper::new(&registry, movement);
    ///
    /// // Multiple parameters
    /// fn physics(
    ///     positions: query::Result<&Position>,
    ///     velocities: query::Result<&mut Velocity>,
    ///     world: &mut World,
    /// ) {
    ///     // System logic
    /// }
    /// let sys = Wrapper::new(&registry, physics);
    /// ```
    ///
    /// # Type Inference
    ///
    /// The `Params` type parameter is inferred from the function signature:
    /// - `fn()` → `Params = ()`
    /// - `fn(A)` → `Params = (A,)`
    /// - `fn(A, B)` → `Params = (A, B)`
    /// - etc.
    ///
    /// where A, B, etc. implement [`Parameter`].
    pub fn new(components: &component::Registry, func: F) -> Self
    where
        F: WithSystemParams<Params>,
    {
        Self {
            required_access: F::required_access(components),
            func,
            _marker: PhantomData,
        }
    }
}

/// Implement [System] for the function wrapper struct. This applies the restriction that the
/// wrapped function must take only system [Parameter] types as arguments.
impl<F, Params> System for Wrapper<F, Params>
where
    F: WithSystemParams<Params> + Send + Sync,
    Params: Send + Sync,
{
    /// Get the required world access for a wrapped function.
    fn required_access(&self) -> &world::AccessRequest {
        &self.required_access
    }

    /// Invoke the wrapped function with the world reference.
    unsafe fn run(&mut self, world: &mut world::World) {
        unsafe {
            self.func.run(world);
        }
    }
}

/// Trait enabling functions to be called with system parameters.
///
/// This trait bridges the gap between clean function signatures (with elided lifetimes)
/// and runtime execution (with world lifetime `'w`). It's the core of what makes
/// parameter extraction work.
///
/// The implementations use Higher-Ranked Trait Bounds to achieve lifetime flexibility:
///
/// ```rust,ignore
/// impl<Func, A: Parameter> WithSystemParams<(A,)> for Func
/// where
///     for<'a> &'a mut Func:
///         FnMut(A) +              // Signature: accepts parameter type
///         FnMut(A::Value<'a>),    // Runtime: accepts value with any lifetime
/// ```
///
/// This says the function must work with:
/// - **Parameter type** with elided lifetime (e.g., `query::Result<&Comp>`)
/// - **Value type** with any lifetime `'a` (e.g., `query::Result<'w, &Comp>`)
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
/// 1. Merges component specs from all parameters
/// 2. Extracts parameter values from world at runtime
/// 3. Calls the function with the extracted values
///
/// # Safety
///
/// The `run` method creates aliased mutable world pointers. This is safe because:
/// - Each parameter accesses disjoint data (different components)
/// - Component specs validate no aliasing at system registration
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
pub trait WithSystemParams<Params>: 'static {
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

    /// Execute the function with parameters extracted from the world.
    ///
    /// This method:
    /// 1. Extracts each parameter's value from the world via [`Parameter::get()`]
    /// 2. Invokes the function with all extracted values
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// - Component specs have been validated (no aliasing violations)
    /// - No other system is concurrently accessing conflicting components
    /// - World reference is valid for the call duration
    ///
    /// The implementation creates aliased mutable world pointers, which is safe
    /// only when parameters access disjoint data.
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable world reference with lifetime `'w`
    unsafe fn run(&mut self, world: &mut world::World);
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
/// let system = Wrapper::new(&components, tick_counter);
/// ```
impl<F> WithSystemParams<()> for F
where
    F: FnMut() + 'static,
{
    /// Returns an empty access request since no components are accessed.
    fn required_access(components: &component::Registry) -> world::AccessRequest {
        world::AccessRequest::NONE
    }

    /// Invokes the function without accessing the world.
    unsafe fn run(&mut self, _world: &mut world::World) {
        self();
    }
}

/// Macro implementing [`WithSystemParams`] for functions with N parameters.
///
/// This macro generates implementations for functions with specific parameter counts.
/// For example, `system_param_function_impl!(A, B)` implements `WithSystemParams<(A, B)>`
/// for any function `Fn(A, B)` where `A` and `B` implement [`Parameter`].
///
/// # Generated Implementation
///
/// - **component_spec**: Merges specs from all parameters
/// - **run**: Extracts each parameter from world, calls function
///
/// # The HRTB Constraint
///
/// ```ignore
/// for<'a> &'a mut Func: FnMut($($param),*) + FnMut($($param::Value<'a>),*)
/// ```
///
/// This requires the function to accept both:
/// 1. Parameters with elided lifetimes (signature)
/// 2. Parameter values with any lifetime (runtime)
///
/// # Safety
///
/// The `run` implementation creates aliased mutable world pointers by casting
/// `&mut World` to `*mut World` for each parameter. This is safe because:
/// - Each `Parameter::get()` accesses disjoint components
/// - Component specs are validated before execution
/// - The scheduler prevents concurrent conflicting access
macro_rules! system_param_function_impl {
    ($($param:ident),*) => {
        impl<Func, $($param: Parameter),*> WithSystemParams<($($param,)*)> for Func
        where
            Func: 'static,
            // HRTB: Function must work with both elided lifetimes (signature)
            // and any specific lifetime 'a (runtime with world lifetime)
            for<'a> &'a mut Func: FnMut($($param),*) + FnMut($($param::Value<'a>),*),
        {
            fn required_access(components: &component::Registry) -> world::AccessRequest {
                // Merge component specs from all parameters
                let mut access = world::AccessRequest::NONE;
                $(
                    access = access.merge(&$param::required_access(components));
                )*
                access
            }

            unsafe fn run(&mut self, world: &mut world::World) {
                // Helper function to call with extracted parameters
                // Needed because we can't directly call self($($param),*) due to macro hygiene
                #[allow(clippy::too_many_arguments, non_snake_case)]
                fn call_it<$($param),*>(mut func: impl FnMut($($param),*), $($param: $param),*) {
                    func($($param),*);
                }

                // Extract each parameter from the world
                $(
                    // SAFETY: Creating aliased mutable world pointers is safe because:
                    // 1. Each Parameter::get() accesses different components (disjoint data)
                    // 2. Component specs validated this at system registration
                    // 3. Scheduler ensures no concurrent conflicting access
                    #[allow(non_snake_case)]
                    let $param = unsafe { $param::get(&mut *(world as *mut world::World)) };
                )*

                // Call the function with all extracted parameters
                call_it(self, $($param),*);
            }
        }
    };
}

/// Recursive macro to generate [`WithSystemParams`] for all parameter counts.
///
/// Given a list of type parameters like `A, B, C`, this macro generates implementations for:
/// - `(A, B, C)` - 3 parameters
/// - `(B, C)` - 2 parameters
/// - `(C)` - 1 parameter
///
/// This allows the same invocation to cover all parameter counts from N down to 1.
///
/// # Example
///
/// ```ignore
/// system_param_function!(A, B, C);
/// ```
///
/// Generates:
/// - `impl WithSystemParams<(A, B, C)> for Func where ...`
/// - `impl WithSystemParams<(B, C)> for Func where ...`
/// - `impl WithSystemParams<(C,)> for Func where ...`
macro_rules! system_param_function {
    // Base case: single parameter
    ($head_ty:ident) => {
        system_param_function_impl!($head_ty);
    };
    // Recursive case: head + tail
    ($head_ty:ident, $( $tail_ty:ident ),*) => (
        // Generate implementation for full parameter list
        system_param_function_impl!($head_ty, $( $tail_ty ),*);
        // Recurse with tail (one fewer parameter)
        system_param_function!($( $tail_ty ),*);
    );
}

// Generate WithSystemParams implementations for functions with 1-26 parameters.
// This covers the vast majority of real-world systems. If you need more than 26 parameters,
// consider breaking your system into smaller systems or using a resource to pass shared data.
system_param_function! {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z
}

#[cfg(test)]
mod tests {
    use crate::ecs::{
        component, query,
        system::{System, function::Wrapper},
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

    #[test]
    fn no_param_function_system() {
        fn my_system() {
            // No-op
        }

        let components = component::Registry::new();
        let mut system = Wrapper::new(&components, my_system);

        let mut world = world::World::new(world::Id::new(0));
        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn world_param_function_system() {
        fn my_system(world: &mut world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }

        let components = component::Registry::new();
        let mut system = Wrapper::new(&components, my_system);

        let mut world = world::World::new(world::Id::new(0));
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

        let components = world.components();
        let mut system = Wrapper::new(components, my_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, increment_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, count_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, physics_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, two_query_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn query_and_world_parameters_system() {
        fn spawner_system(query: query::Result<&Comp1>, world: &mut world::World) {
            let count = query.count();
            if count < 5 {
                world.spawn(Comp1 { value: 100 });
            }
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 1 });

        let components = world.components();
        let mut system = Wrapper::new(components, spawner_system);

        unsafe {
            system.run(&mut world);
        }

        // Verify entity was spawned
        let count = world.query::<&Comp1>().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn access_single_query() {
        fn my_system(_query: query::Result<&Comp1>) {}

        let components = component::Registry::new();
        components.register::<Comp1>();

        let system = Wrapper::new(&components, my_system);
        let access = system.required_access();

        // Should have Comp1 in the access request
        assert_eq!(
            *access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![components.get::<Comp1>().unwrap()]),
                component::Spec::EMPTY
            )
        );
    }

    #[test]
    fn access_multiple_queries() {
        fn my_system(_query1: query::Result<&Comp1>, _query2: query::Result<&Comp2>) {}

        let components = component::Registry::new();
        let id1 = components.register::<Comp1>();
        let id2 = components.register::<Comp2>();

        let system = Wrapper::new(&components, my_system);
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

        let components = component::Registry::new();
        let id1 = components.register::<Comp1>();
        let id2 = components.register::<Comp2>();

        let system = Wrapper::new(&components, my_system);
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

        let components = world.components();
        let mut system = Wrapper::new(components, entity_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, empty_system);

        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn three_parameter_system() {
        fn three_param_system(
            query1: query::Result<&Comp1>,
            query2: query::Result<&Comp2>,
            _world: &mut world::World,
        ) {
            assert_eq!(query1.count(), 2);
            assert_eq!(query2.count(), 1);
        }

        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Comp1 { value: 1 });
        world.spawn((Comp1 { value: 2 }, Comp2 { value: 10 }));

        let components = world.components();
        let mut system = Wrapper::new(components, three_param_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, increment_system);

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

        let components = world.components();
        let mut system = Wrapper::new(components, multiply_system);

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

        let components = component::Registry::new();
        let system = Wrapper::new(&components, my_system);
        let access = system.required_access();

        // Should have empty spec
        assert!(access.is_none());
    }

    #[test]
    fn component_spec_world_only_system() {
        // Given
        fn my_system(_world: &mut world::World) {}

        let components = component::Registry::new();
        let system = Wrapper::new(&components, my_system);

        // When
        let access = system.required_access();

        // Then
        assert!(access.world_mut());
    }
}
