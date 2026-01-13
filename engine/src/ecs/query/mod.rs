//! High-level query API for accessing entities and components across archetypes.
//!
//! This module provides a type-safe query system that allows you to iterate over
//! entities and their components across multiple tables (archetypes) in the ECS.
//!
//! # Architecture
//!
//! The query system is built on three main concepts:
//!
//! - **[Parameter]**: Individual query elements like `&Component`, `&mut Component`,
//!   `Option<&Component>`, or `Entity`. Each parameter represents a single piece of
//!   data you want to access.
//!
//! - **[Data]**: Complete query specifications composed of one or more parameters.
//!   Tuples of parameters automatically implement `Data`, allowing queries like
//!   `(Entity, &Position, &mut Velocity)`.
//!
//! - **[Result]**: The iterator returned by invoking a query, which yields matching
//!   entities across all relevant tables.
//!
//! # Usage
//!
//! ```rust,ignore
//! use rusty_engine::ecs::{query::Query, world::World};
//! use rusty_macros::Component;
//!
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Component)]
//! struct Velocity { dx: f32, dy: f32 }
//!
//! let mut world = World::new(/* ... */);
//!
//! // Create a query for entities with Position and mutable Velocity
//! let query = Query::<(&Position, &mut Velocity)>::new(world.components());
//!
//! // Iterate over matching entities
//! for (pos, vel) in query.invoke(&mut world) {
//!     vel.dx += pos.x * 0.1;
//!     vel.dy += pos.y * 0.1;
//! }
//! ```
//!
//! # Optional Components
//!
//! You can query for components that may or may not exist using `Option<T>`:
//!
//! ```rust,ignore
//! let query = Query::<(Entity, &Position, Option<&Velocity>)>::new(world.components());
//!
//! for (entity, pos, vel) in query.invoke(&mut world) {
//!     match vel {
//!         Some(v) => println!("Entity {:?} has velocity", entity),
//!         None => println!("Entity {:?} is stationary", entity),
//!     }
//! }
//! ```
//!
//! # Safety and Validation
//!
//! The query system performs runtime validation to prevent aliasing violations:
//!
//! - Requesting the same component multiple times (e.g., `(&Foo, &Foo)`) will panic
//! - Requesting conflicting mutability (e.g., `(&Foo, &mut Foo)`) will panic
//!
//! This validation happens at query invocation time, not construction time.
//!
//! [Parameter]: param::Parameter
//! [Data]: data::Data
//! [Result]: result::Result

use std::marker::PhantomData;

use crate::ecs::{component, query::data::DataSpec, world};

mod data;
mod param;
mod result;

/// Publicly re-exported query data trait.
pub use data::Data;

/// Publicly re-exported query result iterator.
pub use result::Result;

/// A query for accessing entities and components across multiple tables.
///
/// `Query<D>` is parameterized by a [`Data`] type that specifies what components
/// and entities should be accessed. The type parameter `D` is typically a tuple
/// of [`Parameter`] types.
///
/// # Type Parameters
///
/// - `D`: The query data specification (e.g., `&Component`, `(&C1, &mut C2)`)
///
/// # Construction
///
/// Queries are created using [`Query::new`], which takes a component registry
/// to register and look up component types.
///
/// # Invocation
///
/// Queries are executed using [`Query::invoke`], which returns an iterator over
/// matching entities. The iterator borrows the world mutably for the duration
/// of the iteration.
///
/// # Examples
///
/// ```rust,ignore
/// // Query for a single component
/// let query = Query::<&Position>::new(world.components());
/// for pos in query.invoke(&mut world) {
///     println!("Position: {:?}", pos);
/// }
///
/// // Query for multiple components with mixed mutability
/// let query = Query::<(&Position, &mut Velocity)>::new(world.components());
/// for (pos, vel) in query.invoke(&mut world) {
///     vel.dx += pos.x;
/// }
/// ```
///
/// [`Data`]: data::Data
/// [`Parameter`]: param::Parameter
pub struct Query<D> {
    /// The specification describing what data this query accesses.
    data_spec: DataSpec,

    /// Phantom data to tie the Data type to the struct.
    phantom: PhantomData<D>,
}

impl<D> Query<D> {
    /// Construct a new query for the given data type.
    ///
    /// This creates the query specification by analyzing the type `D` and registering
    /// any necessary component types in the component registry.
    ///
    /// # Parameters
    ///
    /// - `components`: The component registry used to register and look up component types
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.components());
    /// ```
    #[inline]
    pub fn new(components: &component::Registry) -> Self
    where
        D: Data,
    {
        Self {
            data_spec: D::spec(components),
            phantom: PhantomData,
        }
    }

    /// One-shot query execution. Shorthand for creating and invoking a query immediately that
    /// can't be cached for re-use.
    ///
    /// This will create a query and invoke it immediately on the given world.
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable reference to the world to query
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let result = Query::<(&Position, &mut Velocity)>::one_shot(&mut world);
    /// ```
    #[inline]
    pub fn one_shot<'w>(world: &'w mut world::World) -> Result<'w, D>
    where
        D: Data,
    {
        Self::new(world.components()).invoke(world)
    }

    /// Execute the query and return an iterator over matching entities.
    ///
    /// This method:
    /// 1. Validates the query specification to prevent aliasing violations
    /// 2. Identifies all tables (archetypes) that contain the required components
    /// 3. Returns an iterator that yields matching entities across those tables
    ///
    /// # Parameters
    ///
    /// - `world`: Mutable reference to the world to query
    ///
    /// # Returns
    ///
    /// An iterator that yields items of type `D` (the query data type).
    ///
    /// # Panics
    ///
    /// Panics if the query contains aliasing violations, such as:
    /// - Requesting the same component multiple times (e.g., `(&Foo, &Foo)`)
    /// - Requesting conflicting mutability (e.g., `(&Foo, &mut Foo)`)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.components());
    ///
    /// for (pos, vel) in query.invoke(&mut world) {
    ///     // Update velocity based on position
    ///     vel.dx += pos.x * 0.01;
    /// }
    /// ```
    pub fn invoke<'w>(&self, world: &'w mut world::World) -> Result<'w, D>
    where
        D: Data,
    {
        // Runtime check to ensure no aliasing violations in component data types.
        assert!(
            self.data_spec.is_valid(),
            "Query aliasing violation: same component requested multiple times"
        );

        // Create a component spec for the query.
        let comp_spec = self.data_spec.as_component_spec();

        // Get the table ids that support this component spec.
        let table_ids = world.storage().supporting(&comp_spec);

        Result::new(world, table_ids)
    }

    /// Invoke the query on a shard to get an iterator over matching entities.
    ///
    /// This is similar to [`invoke`](Self::invoke) but works with a [`world::Shard`]
    /// instead of a full `World`. The shard's grant should cover the components
    /// accessed by this query.
    ///
    /// # Parameters
    ///
    /// - `shard`: Mutable reference to the shard to query
    ///
    /// # Returns
    ///
    /// An iterator that yields items of type `D` (the query data type).
    ///
    /// # Panics
    ///
    /// Panics if the query has aliasing violations, same as [`invoke`](Self::invoke).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.components());
    /// let shard = world.shard(&access_request)?;
    ///
    /// for (pos, vel) in query.invoke_shard(&mut shard) {
    ///     vel.dx += pos.x * 0.01;
    /// }
    /// ```
    pub fn invoke_shard<'w>(&self, shard: &'w mut world::Shard<'_>) -> Result<'w, D>
    where
        D: Data,
    {
        // Runtime check to ensure no aliasing violations in component data types.
        assert!(
            self.data_spec.is_valid(),
            "Query aliasing violation: same component requested multiple times"
        );

        // Create a component spec for the query.
        let comp_spec = self.data_spec.as_component_spec();

        // Get the table ids that support this component spec.
        let table_ids = shard.storage().supporting(&comp_spec);

        // SAFETY: Shard has grant covering required access, validated at system execution
        Result::new(unsafe { shard.world_mut() }, table_ids)
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::ecs::{query::Query, world};

    #[derive(Component)]
    struct Comp1;

    #[derive(Component)]
    struct Comp2;

    #[derive(Component)]
    struct Comp3;

    fn make_world() -> world::World {
        let mut world = world::World::new(world::Id::new(0));

        world.spawn((Comp1, Comp2));
        world.spawn(Comp1);
        world.spawn((Comp1, Comp2));
        world.spawn((Comp1, Comp2, Comp3));
        world.spawn(Comp2);
        world.spawn(Comp3);
        world.spawn((Comp1, Comp3));
        world.spawn((Comp3, Comp2, Comp1));
        world.spawn((Comp2, Comp3));
        world
    }

    #[test]
    fn invoke_empty_query() {
        let mut world = make_world();
        let mut result = Query::<()>::new(world.components()).invoke(&mut world);

        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_simple_query() {
        let mut world = make_world();
        let mut result = Query::<&Comp1>::new(world.components()).invoke(&mut world);

        assert_eq!(result.len(), 6);

        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_mix_mut_query() {
        let mut world = make_world();
        let mut result = Query::<(&Comp1, &mut Comp2)>::new(world.components()).invoke(&mut world);

        assert_eq!(result.len(), 4);

        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }
}
