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
//! let query = Query::<(&Position, &mut Velocity)>::new(world.resources());
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
//! let query = Query::<(Entity, &Position, Option<&Velocity>)>::new(world.resources());
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

use crate::ecs::{component, query::source::DataSource, world};

mod data;
mod param;
mod result;
mod source;

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
/// matching entities. The iterator borrows a DataSource (World or Shard) mutably for the duration
/// of the iteration.
///
/// # Examples
///
/// ```rust,ignore
/// // Query for a single component
/// let query = Query::<&Position>::new(world.resources());
/// for pos in query.invoke(&mut world) {
///     println!("Position: {:?}", pos);
/// }
///
/// // Query for multiple components with mixed mutability
/// let query = Query::<(&Position, &mut Velocity)>::new(world.resources());
/// for (pos, vel) in query.invoke(&mut world) {
///     vel.dx += pos.x;
/// }
/// ```
///
/// [`Data`]: data::Data
/// [`Parameter`]: param::Parameter
///
///
/// # Safety
/// Its critical to ensure that queries do not violate Rust's aliasing rules. This is checked at
/// query creation time, and will panic if violations are detected. It is important no means for
/// changing the query data specification after creation exist, as that could lead to undefined
/// behavior.
pub struct Query<D> {
    /// The components accessed by this query.
    components: component::Spec,

    /// The access request for this query.
    required_access: world::AccessRequest,

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
    /// - `registry`: The resource registry used to register and look up component types
    ///
    /// # Panics
    ///
    /// Panics if the query Data specification has aliasing violations.
    ///
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.resources());
    /// ```
    #[inline]
    pub fn new(registry: &world::TypeRegistry) -> Self
    where
        D: Data,
    {
        // Generate the data specification from the Data type.
        let data_spec = D::spec(registry);
        // Assert we have not created an invalid query with potential aliasing violations.
        assert!(
            data_spec.is_valid(),
            "Query aliasing violation: same component requested multiple times in query"
        );
        Self {
            components: data_spec.as_component_spec(),
            required_access: data_spec.as_access_request(),
            phantom: PhantomData,
        }
    }

    /// Get the world access required to run this query.
    #[inline]
    pub fn required_access(&self) -> &world::AccessRequest {
        &self.required_access
    }

    /// Execute the query and return an iterator over matching entities.
    ///
    /// This method:
    /// 1. In debug builds, verifies that the data source allows the required access
    /// 2. Identifies all tables (archetypes) that contain the required components
    /// 3. Returns an iterator that yields matching entities across those tables
    ///
    /// # Parameters
    ///
    /// - `source`: Mutable reference to the data source to query
    ///
    /// # Returns
    ///
    /// An iterator that yields items of type `D` (the query data type).
    ///
    /// # Panics
    /// - In debug builds, if the data source does not support the required access for this query.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.resources());
    ///
    /// for (pos, vel) in query.invoke(&mut world) {
    ///     // Update velocity based on position
    ///     vel.dx += pos.x * 0.01;
    /// }
    /// ```
    ///
    /// ```rust,ignore
    /// let query = Query::<(&Position, &mut Velocity)>::new(world.resources());
    ///
    /// for (pos, vel) in query.invoke(&mut shard) {
    ///     // Update velocity based on position
    ///     vel.dx += pos.x * 0.01;
    /// }
    /// ```
    pub fn invoke<'w>(&self, source: &'w mut dyn DataSource) -> Result<'w, D>
    where
        D: Data,
    {
        // Ensure the data source allows the required access for this query.
        debug_assert!(
            source.allows(&self.required_access),
            "Access violation: the data source does not permit the required access for this query."
        );

        // Get the table ids that support this component spec.
        //
        // TODO: Should these be cached on the Query itself to avoid recomputing each time? This
        // would need some way to refresh the cache if new archetypes are added that match the
        // query.
        //
        let table_ids = source.table_ids_for(&self.components);

        Result::new(source, table_ids)
    }
}

/// Trait for converting various types into a `Query<D>`.
pub trait IntoQuery<D> {
    /// Convert into a `Query<D>`.
    fn into_query(registry: &world::TypeRegistry) -> Query<D>;
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
    #[should_panic(expected = "Query aliasing violation")]
    fn reject_invalid_query_data() {
        // Given
        let mut world = make_world();
        // When
        Query::<(&Comp1, &Comp1)>::new(world.resources()).invoke(&mut world);
    }

    #[test]
    fn invoke_empty_query() {
        // Given
        let mut world = make_world();
        // When
        let mut result = Query::<()>::new(world.resources()).invoke(&mut world);
        // Then
        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_simple_query() {
        // Given
        let mut world = make_world();

        // When
        let mut result = Query::<&Comp1>::new(world.resources()).invoke(&mut world);

        // Then
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
        // Given
        let mut world = make_world();
        // When
        let mut result = Query::<(&Comp1, &mut Comp2)>::new(world.resources()).invoke(&mut world);

        // Then
        assert_eq!(result.len(), 4);
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_query_with_shard() {
        // Given
        let world = make_world();
        let query = Query::<(&Comp1, &mut Comp2)>::new(world.resources());
        let access_request = query.required_access();
        let mut shard = world.shard(access_request).unwrap();

        // When
        let mut result = query.invoke(&mut shard);

        // Then
        assert_eq!(result.len(), 4);
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }
}
