//! System parameter extraction using Generic Associated Types.
//!
//! This module defines the [`Parameter`] trait, which enables clean system function signatures
//! without explicit lifetime parameters.

mod commands;
mod query;
mod unique;
mod world;

use crate::ecs::{system::CommandBuffer, world as ecs_world};

pub use commands::Commands;
pub use query::Query;
pub use unique::{Uniq, UniqMut};

/// A type that can be passed as a parameter to a system function.
///
/// The `Parameter` trait enables extracting data from the world with clean function signatures.
/// The [`Value`](Parameter::Value) Generic Associated Type (GAT), carries the world's lifetime
/// without appearing in the function signature.
///
/// # How It Works
///
/// When you write a system function like:
///
/// ```rust,ignore
/// fn my_system(query: query::Result<&Position>) {
///     // ...
/// }
/// ```
///
/// The type `query::Result<&Position>` is actually `query::Result<'_, &Position>` with an
/// elided lifetime. This is the **parameter type** (no concrete lifetime).
///
/// At runtime, [`Parameter::get()`] returns `query::Result<'w, &Position>` where `'w` is
/// the world's lifetime. This is the **value type** (concrete lifetime).
///
/// The [`function::WithSystemParams`](super::function::WithSystemParams) trait bridges these
/// using a Higher-Ranked Trait Bound:
///
/// ```rust,ignore
/// for<'a> &'a mut Func: FnMut(Param) + FnMut(Param::Value<'a, '_>)
/// ```
///
/// This says: "The function must accept both the parameter type AND its value form with any lifetime."
/// The `'a` is the world lifetime, while the second `'_` is the state lifetime (typically not used
/// by the function, as state is only accessed during parameter extraction).
///
/// # Implementations
///
/// ## Query Results
///
/// ```rust,ignore
/// impl<D: query::Data> Parameter for query::Result<'_, D> {
///     type Value<'w, 's> = query::Result<'w, D>;
///     type State = query::Query<D>;
///
///     fn build_state(world: &mut World) -> Self::State {
///         query::Query::new(world.components())
///     }
///
///     unsafe fn get<'w, 's>(world: &'w mut World, state: &'s mut Self::State) -> Self::Value<'w, 's> {
///         state.invoke(world)
///     }
/// }
/// ```
///
/// **Usage:**
/// ```rust,ignore
/// fn movement(query: query::Result<(&Velocity, &mut Position)>) {
///     for (vel, pos) in query {
///         pos.x += vel.dx;
///     }
/// }
/// ```
///
/// ## World Access
///
/// ```rust,ignore
/// impl Parameter for &mut world::World {
///     type Value<'w, 's> = &'w mut World;
///     type State = ();
///
///     fn build_state(_world: &mut World) -> Self::State {}
///
///     unsafe fn get<'w, 's>(world: &'w mut World, _state: &'s mut Self::State) -> Self::Value<'w, 's> {
///         world
///     }
/// }
/// ```
///
/// **Usage:**
/// ```rust,ignore
/// fn spawner(enemies: query::Result<&Enemy>, world: &mut World) {
///     if enemies.len() < 10 {
///         world.spawn(Enemy::new());
///     }
/// }
/// ```
///
/// # Available Parameter Types
///
/// ## Query Results
///
/// - [`query::Result<D>`](crate::ecs::query::Result): Iterate over entities matching component criteria
///
/// ## Resources
///
/// - [`Uniq<U>`]: Immutable access to a global resource
/// - [`UniqMut<U>`]: Mutable access to a global resource
/// - `Option<Uniq<U>>`: Optional immutable resource (doesn't panic if missing)
/// - `Option<UniqMut<U>>`: Optional mutable resource (doesn't panic if missing)
///
/// ## World Access
///
/// - `&World`: Immutable world access (exclusive system)
///
/// ## Commands
///
/// - [`Commands`]: Deferred entity spawning, despawning, and component modifications
///
/// # Future Extensions
///
/// Additional parameter types planned:
/// - **Events**: Event readers/writers
/// - **Queries with filters**: `Query<&T, With<U>>`
///
/// # Safety
///
/// The `get` method is unsafe because:
/// 1. Multiple parameters may create aliased mutable references to the world
/// 2. The caller must ensure parameters access disjoint data
/// 3. Component access validate this at the scheduler level
///
/// See [`super::function::WithSystemParams`] for details on safe usage.
pub trait Parameter: Sized {
    /// The runtime value type with world lifetime applied.
    ///
    /// This Generic Associated Type (GAT) allows the parameter type to be specified without
    /// a concrete lifetime in function signatures, while the runtime value has the world's lifetime.
    ///
    /// # Type Parameters
    ///
    /// - `'w`: World lifetime - how long the extracted value can reference world data
    /// - `'s`: State lifetime - how long the value can reference parameter state
    ///
    /// # Type Relationship
    ///
    /// For query parameters:
    /// - `Self` = `query::Result<'_, D>` (elided lifetime in function signature)
    /// - `Value<'w, 's>` = `query::Result<'w, D>` (concrete world lifetime at runtime)
    ///
    /// For world parameters:
    /// - `Self` = `&mut World` (no lifetime in function signature)
    /// - `Value<'w, 's>` = `&'w mut World` (concrete world lifetime at runtime)
    ///
    /// The `Value<'w, 's>` must also be `Parameter` to allow nested extraction (future feature).
    type Value<'w, 's>: Parameter<State = Self::State>;

    /// The runtime state associated with this parameter.
    ///
    /// This Generic Associated Type (GAT) allows parameters to maintain state
    /// with the world's lifetime.
    type State: 'static;

    /// Build the state for this parameter from the world.
    ///
    /// This method is called once per system execution to create any
    /// necessary state for the parameter.
    fn build_state(world: &mut ecs_world::World) -> Self::State;

    /// Get the world access required for this parameter.
    ///
    /// The access request describes which world resources this parameter accesses and how
    /// (immutable vs mutable). The scheduler uses this to:
    /// - Detect conflicts between system parameters
    /// - Validate no aliasing violations occur
    /// - Determine safe execution order for parallel systems (future)
    ///
    /// # Parameters
    ///
    /// - `world`: The world to look up component or resource info from
    ///
    /// # Returns
    ///
    /// A [`world::AccessRequest`] describing world access required.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Query for &Position returns access with Position (immutable)
    /// let access = <query::Result<&Position> as Parameter>::required_access(&registry);
    ///
    /// // Query for &mut Velocity returns access with Velocity (mutable)
    /// let access = <query::Result<&mut Velocity> as Parameter>::required_access(&registry);
    ///
    /// // World access returns immutable world access.
    /// let access = <&World as Parameter>::required_access(&world);
    ///
    /// // World access returns mutable world access.
    /// let access = <&mut World as Parameter>::required_access(&world);
    /// ```
    fn required_access(world: &ecs_world::World) -> ecs_world::AccessRequest;

    /// Extract this parameter's value from a shard.
    ///
    /// This method is called by the system executor to provide parameter values to
    /// the system function. The returned value has the shard's lifetime `'w`.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// 1. **No aliasing**: Parameters access disjoint data (validated by access requests)
    /// 2. **Valid lifetime**: Shard reference is valid for lifetime `'w`
    /// 3. **No concurrency**: System is not executed concurrently with conflicting systems
    ///
    /// The [`super::function::WithSystemParams`] implementation upholds these by:
    /// - Using raw pointers to create aliased shard references (sound due to disjoint access)
    /// - Relying on scheduler to validate access requests before execution
    /// - Requiring `&mut self` on system execution (prevents concurrent calls)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let state = <query::Result<&Position> as Parameter>::build_state(&mut world);
    /// let shard = world.shard(&access_request)?;
    ///
    /// // Query extraction
    /// let query = unsafe { <query::Result<&Position> as Parameter>::get(&mut shard, &mut state, &command_buffer) };
    /// for pos in query {
    ///     println!("({}, {})", pos.x, pos.y);
    /// }
    /// ```
    unsafe fn extract<'w, 's>(
        shard: &'w mut ecs_world::Shard<'_>,
        state: &'s mut Self::State,
        command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's>;
}
