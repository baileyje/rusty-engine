//! Query parameter types and specifications.
//!
//! This module defines the [`Parameter`] trait and [`ParameterSpec`] enum, which represent
//! individual elements that can be used in queries.
//!
//! # Parameter vs Data
//!
//! - **Parameter**: A single query element (e.g., `&Component`, `Entity`, `Option<&mut C>`)
//! - **Data**: A complete query composed of one or more parameters (e.g., `(&C1, &mut C2)`)
//!
//! Any type implementing `Parameter` automatically implements `Data` for single-element queries.
//! Tuples of `Parameter` types implement `Data` for multi-element queries.
//!
//! # Valid Parameter Types
//!
//! - **Components**: `&C` (immutable), `&mut C` (mutable)
//! - **Optional Components**: `Option<&C>`, `Option<&mut C>`
//! - **Entity**: `Entity` (always passed by value)
//!
//! # Examples
//!
//! ```rust,ignore
//! use rusty_engine::ecs::query::param::Parameter;
//!
//! // Single parameter - queries for one component
//! let query = Query::<&Position>::new(world.components());
//!
//! // Multiple parameters - queries for multiple components
//! let query = Query::<(&Position, &mut Velocity)>::new(world.components());
//!
//! // Optional parameters - includes entities without the component
//! let query = Query::<(&Position, Option<&Velocity>)>::new(world.components());
//! ```

use crate::ecs::{component, entity, storage};

/// A single query parameter that can be fetched from the ECS.
///
/// This trait represents individual elements that can appear in a query, such as
/// component references (`&C`, `&mut C`), optional components (`Option<&C>`), or
/// entity IDs (`Entity`).
///
/// # Implementations
///
/// The following types implement `Parameter`:
///
/// | Type | Description | Mutability | Optional |
/// |------|-------------|------------|----------|
/// | `&C` | Immutable component reference | No | No |
/// | `&mut C` | Mutable component reference | Yes | No |
/// | `Option<&C>` | Optional immutable reference | No | Yes |
/// | `Option<&mut C>` | Optional mutable reference | Yes | Yes |
/// | `Entity` | Entity identifier | N/A | No |
///
/// # Relationship to Data
///
/// Any type implementing `Parameter` automatically implements [`Data`](super::data::Data),
/// allowing single-parameter queries. For multi-parameter queries, use tuples of
/// `Parameter` types.
///
/// # Examples
///
/// ```rust,ignore
/// // Single parameter queries
/// Query::<&Position>::new(world.components())
/// Query::<Entity>::new(world.components())
///
/// // Multi-parameter queries (tuples of Parameters)
/// Query::<(&Position, &mut Velocity)>::new(world.components())
/// Query::<(Entity, &Health, Option<&Shield>)>::new(world.components())
/// ```
pub trait Parameter: Sized {
    /// The value type returned when fetching this parameter.
    ///
    /// This is typically the parameter type itself with a world lifetime applied:
    /// - `&C` → `&'w C`
    /// - `Option<&mut C>` → `Option<&'w mut C>`
    /// - `Entity` → `Entity` (no lifetime needed)
    type Value<'w>;

    /// Get the query parameter specification for this type. The component registry is provided to allow a
    /// parameter type to lookup or register component information.
    fn spec(components: &component::Registry) -> ParameterSpec;

    /// Fetch a value from a specific query parameter. This will be given access to the table, row
    /// and entity combination to fetch for.
    ///
    /// Returns `None` if:
    /// - The row index is invalid (>= table.len())
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - A Component types `C` match the actual types stored for their component IDs
    ///
    /// Type safety is ensured through the component registry's type-to-ID mapping.
    /// Row validity and component existence are checked at runtime.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if a component type doesn't match the registered type.
    unsafe fn fetch<'w>(
        entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>>;

    /// Fetch component references for a single entity row from a mutable table.
    ///
    /// This is the mutable variant of [`fetch`](Parameter::fetch), allowing mutable access
    /// to components when needed.
    ///
    /// Returns `None` if:
    /// - Any required component is not registered in the registry
    /// - Any required component is missing from the table
    /// - The row index is invalid (>= table.len())
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types `C` match the actual types stored for their component IDs
    /// - When fetching tuples with mutable components, the same component is not requested multiple times
    ///
    /// Type safety is ensured through the component registry's type-to-ID mapping.
    /// Row validity and component existence are checked at runtime.
    unsafe fn fetch_mut<'w>(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>>;
}

/// A single query parameter specification.
///
/// This enum describes what data a parameter needs from the ECS, allowing the query
/// system to determine which tables match and what access is required.
///
/// # Variants
///
/// - **Entity**: The parameter is an entity ID (always immutable, passed by value)
/// - **Component(id, is_mutable, is_optional)**: The parameter is a component reference
///   - `id`: The component type ID
///   - `is_mutable`: Whether mutable access is required (`&mut` vs `&`)
///   - `is_optional`: Whether the component is optional (`Option<&C>` vs `&C`)
///
/// # Optional Components
///
/// When a component is marked as optional (`is_optional = true`), the query will include
/// entities that don't have that component. Non-optional components restrict the query
/// to only entities that have all required components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterSpec {
    /// The query results should include the entity. This will always be immutable and passed by
    /// value.
    Entity,

    /// The query results should include a specific component (by ID).
    /// - The first field is the component ID
    /// - The second field indicates whether the component is mutable
    /// - The third field indicates whether the component is optional
    Component(component::Id, bool, bool),
}

/// Implement the [`Parameter`] trait for an immutable component reference.
impl<C: component::Component> Parameter for &C {
    type Value<'w> = &'w C;
    /// Return [`ParameterSpec::Component`] with mutability `false` and optional `false`.
    ///
    /// # Note
    /// This will register the component `C` type if necessary.
    fn spec(components: &component::Registry) -> ParameterSpec {
        ParameterSpec::Component(components.register::<C>(), false, false)
    }

    unsafe fn fetch<'w>(
        _entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        unsafe { table.get::<C>(row) }
    }

    unsafe fn fetch_mut<'w>(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Safety: Safe to call fetch as we are not mutating anything
        unsafe { Self::fetch(_entity, table, row) }
    }
}

/// Implement the [`Parameter`] trait for a mutable component reference.
impl<C: component::Component> Parameter for &mut C {
    type Value<'w> = &'w mut C;

    /// Return [`ParameterSpec::Component`] with mutability `true` and optional `false`.
    ///
    /// # Note
    /// This will register the [component::Component] `C` type if necessary.
    fn spec(components: &component::Registry) -> ParameterSpec {
        ParameterSpec::Component(components.register::<C>(), true, false)
    }

    unsafe fn fetch<'w>(
        _entity: entity::Entity,
        _table: &'w storage::Table,
        _row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Cannot fetch mutable reference from immutable table
        None
    }

    unsafe fn fetch_mut<'w>(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        unsafe { table.get_mut::<C>(row) }
    }
}

/// Implement the [`Parameter`] trait for an optional immutable component reference.
///
/// # Optional Component Semantics
///
/// Optional components allow querying entities that may or may not have a specific component.
/// When a component is optional:
///
/// - The query **includes** entities that don't have the component
/// - For entities without the component, the parameter value is `None`
/// - For entities with the component, the parameter value is `Some(&C)`
///
/// This differs from required components, which only match entities that have the component.
///
/// # Examples
///
/// ```rust,ignore
/// // Query entities with Position, optionally with Velocity
/// let query = Query::<(&Position, Option<&Velocity>)>::new(world.components());
///
/// for (pos, vel_opt) in query.invoke(&mut world) {
///     match vel_opt {
///         Some(vel) => println!("Moving entity at {:?}", pos),
///         None => println!("Stationary entity at {:?}", pos),
///     }
/// }
/// ```
impl<C: component::Component> Parameter for Option<&C> {
    type Value<'w> = Option<&'w C>;
    /// Return [`ParameterSpec::Component`] with mutability `false` and optional `true`.
    ///
    /// # Note
    /// This will register the component `C` type if necessary.
    fn spec(components: &component::Registry) -> ParameterSpec {
        ParameterSpec::Component(components.register::<C>(), false, true)
    }

    /// Fetch an optional component reference from a table row.
    ///
    /// Always returns `Some(inner_option)` where:
    /// - `inner_option` is `Some(&C)` if the entity has the component
    /// - `inner_option` is `None` if the entity lacks the component
    ///
    /// This ensures the query continues iterating even when the component is missing.
    unsafe fn fetch<'w>(
        _entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Always return Some(inner_option) - never fail the query for missing optional components
        Some(unsafe { table.get::<C>(row) })
    }

    unsafe fn fetch_mut<'w>(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Safety: Safe to call fetch as we are not mutating anything
        unsafe { Self::fetch(_entity, table, row) }
    }
}

/// Implement the [`Parameter`] trait for an optional mutable component reference.
///
/// # Optional Mutable Component Semantics
///
/// This works identically to `Option<&C>`, but provides mutable access to the component
/// when it exists. The query still includes entities without the component.
///
/// # Examples
///
/// ```rust,ignore
/// // Query entities with Position, optionally mutating Velocity
/// let query = Query::<(&Position, Option<&mut Velocity>)>::new(world.components());
///
/// for (pos, vel_opt) in query.invoke(&mut world) {
///     if let Some(vel) = vel_opt {
///         // Only entities with Velocity reach here
///         vel.dx += pos.x * 0.1;
///     }
/// }
/// ```
impl<C: component::Component> Parameter for Option<&mut C> {
    type Value<'w> = Option<&'w mut C>;
    /// Return [`ParameterSpec::Component`] with mutability `true` and optional `true`.
    ///
    /// # Note
    /// This will register the component `C` type if necessary.
    fn spec(components: &component::Registry) -> ParameterSpec {
        ParameterSpec::Component(components.register::<C>(), true, true)
    }

    unsafe fn fetch<'w>(
        _entity: entity::Entity,
        _table: &'w storage::Table,
        _row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Cannot fetch mutable reference from immutable table
        None
    }

    /// Fetch an optional mutable component reference from a table row.
    ///
    /// Always returns `Some(inner_option)` where:
    /// - `inner_option` is `Some(&mut C)` if the entity has the component
    /// - `inner_option` is `None` if the entity lacks the component
    ///
    /// This ensures the query continues iterating even when the component is missing.
    unsafe fn fetch_mut<'w>(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Value<'w>> {
        // Always return Some(inner_option) - never fail the query for missing optional components
        Some(unsafe { table.get_mut::<C>(row) })
    }
}

/// Implement the [`Parameter`] trait for [`entity::Entity`].
impl Parameter for entity::Entity {
    type Value<'w> = entity::Entity;

    /// Return [`ParameterSpec::Entity`].
    fn spec(_components: &component::Registry) -> ParameterSpec {
        ParameterSpec::Entity
    }

    unsafe fn fetch(
        entity: entity::Entity,
        _table: &storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        Some(entity)
    }

    unsafe fn fetch_mut(
        entity: entity::Entity,
        _table: &mut storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        // Safety: Safe to call fetch as we are not mutating anything
        unsafe { Self::fetch(entity, _table, _row) }
    }
}

#[cfg(test)]
mod tests {

    use rusty_macros::Component;

    use crate::ecs::{
        component, entity,
        query::param::{Parameter, ParameterSpec},
        storage, world,
    };

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp3 {
        value: i32,
    }

    #[derive(Component)]
    #[allow(dead_code)]
    struct Comp4 {
        value: i32,
    }

    fn test_setup() -> (world::World, storage::Table) {
        let world = world::World::new(world::Id::new(0));
        let spec = component::Spec::new(vec![
            world.components().register::<Comp1>(),
            world.components().register::<Comp2>(),
            world.components().register::<Comp3>(),
        ]);
        let mut table = storage::Table::new(storage::table::Id::new(0), spec, world.components());

        table.add_entity(
            entity::Entity::new(0.into()),
            (
                Comp1 { value: 10 },
                Comp2 { value: 20 },
                Comp3 { value: 30 },
            ),
            world.components(),
        );
        table.add_entity(
            entity::Entity::new(1.into()),
            (
                Comp1 { value: 20 },
                Comp2 { value: 30 },
                Comp3 { value: 40 },
            ),
            world.components(),
        );
        table.add_entity(
            entity::Entity::new(2.into()),
            (
                Comp1 { value: 30 },
                Comp2 { value: 40 },
                Comp3 { value: 50 },
            ),
            world.components(),
        );

        (world, table)
    }

    #[test]
    fn spec_for_component_ref() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <&Comp1>::spec(world.components());

        // Then
        assert_eq!(
            spec,
            ParameterSpec::Component(world.components().get::<Comp1>().unwrap(), false, false)
        );
    }

    #[test]
    fn spec_for_component_ref_mut() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <&mut Comp1>::spec(world.components());

        // Then
        assert_eq!(
            spec,
            ParameterSpec::Component(world.components().get::<Comp1>().unwrap(), true, false)
        );
    }

    #[test]
    fn spec_for_optional_component_ref() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <Option<&Comp1>>::spec(world.components());

        // Then
        assert_eq!(
            spec,
            ParameterSpec::Component(world.components().get::<Comp1>().unwrap(), false, true)
        );
    }

    #[test]
    fn spec_for_optional_mut_component_ref() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <Option<&mut Comp1>>::spec(world.components());

        // Then
        assert_eq!(
            spec,
            ParameterSpec::Component(world.components().get::<Comp1>().unwrap(), true, true)
        );
    }

    #[test]
    fn spec_for_entity() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = entity::Entity::spec(world.components());

        // Then
        assert_eq!(spec, ParameterSpec::Entity);
    }

    #[test]
    fn fetch_for_component_ref() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value1 = unsafe { <&Comp1>::fetch(entity::Entity::new(0.into()), &table, 0.into()) };
        let value2 = unsafe { <&Comp2>::fetch(entity::Entity::new(1.into()), &table, 1.into()) };

        let value3 = unsafe { <&Comp3>::fetch(entity::Entity::new(2.into()), &table, 2.into()) };

        // Then
        assert!(value1.is_some());
        let value1 = value1.unwrap();
        assert_eq!(value1.value, 10);

        assert!(value2.is_some());
        let value2 = value2.unwrap();
        assert_eq!(value2.value, 30);

        assert!(value3.is_some());
        let value3 = value3.unwrap();
        assert_eq!(value3.value, 50);
    }

    #[test]
    fn fetch_mut_for_component_ref() {
        // Given
        let (_world, mut table) = test_setup();

        // When
        let value1 = unsafe {
            <&Comp1>::fetch_mut(
                entity::Entity::new(0.into()),
                &mut *(&mut table as *mut storage::Table),
                0.into(),
            )
        };
        let value2 = unsafe {
            <&Comp2>::fetch_mut(
                entity::Entity::new(1.into()),
                &mut *(&mut table as *mut storage::Table),
                1.into(),
            )
        };

        let value3 = unsafe {
            <&Comp3>::fetch_mut(
                entity::Entity::new(2.into()),
                &mut *(&mut table as *mut storage::Table),
                2.into(),
            )
        };

        // Then
        assert!(value1.is_some());
        let value1 = value1.unwrap();
        assert_eq!(value1.value, 10);

        assert!(value2.is_some());
        let value2 = value2.unwrap();
        assert_eq!(value2.value, 30);

        assert!(value3.is_some());
        let value3 = value3.unwrap();
        assert_eq!(value3.value, 50);
    }

    #[test]
    fn fetch_for_component_ref_mut() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value = unsafe {
            <&mut Comp1>::fetch(
                entity::Entity::new(0.into()),
                &table, // Nasty multi-borrow workaround.
                0.into(),
            )
        };

        // Then
        assert!(value.is_none());
    }

    #[test]
    fn fetch_mut_for_component_ref_mut() {
        // Given
        let (_world, mut table) = test_setup();

        // When
        let value1 = unsafe {
            <&mut Comp1>::fetch_mut(
                entity::Entity::new(0.into()),
                &mut *(&mut table as *mut storage::Table), // Nasty multi-borrow workaround.
                0.into(),
            )
        };
        let value2 = unsafe {
            <&mut Comp2>::fetch_mut(
                entity::Entity::new(1.into()),
                &mut *(&mut table as *mut storage::Table),
                1.into(),
            )
        };

        let value3 = unsafe {
            <&mut Comp3>::fetch_mut(
                entity::Entity::new(2.into()),
                &mut *(&mut table as *mut storage::Table),
                2.into(),
            )
        };

        // Then
        assert!(value1.is_some());
        let value1 = value1.unwrap();
        assert_eq!(value1.value, 10);
        value1.value += 100;
        assert_eq!(value1.value, 110);

        assert!(value2.is_some());
        let value2 = value2.unwrap();
        assert_eq!(value2.value, 30);
        value2.value += 100;
        assert_eq!(value2.value, 130);

        assert!(value3.is_some());
        let value3 = value3.unwrap();
        assert_eq!(value3.value, 50);
        value3.value += 100;
        assert_eq!(value3.value, 150);
    }

    #[test]
    fn fetch_for_optional_component_ref() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value =
            unsafe { <Option<&Comp1>>::fetch(entity::Entity::new(0.into()), &table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.value, 10);
    }

    #[test]
    fn fetch_mut_for_optional_component_ref() {
        // Given
        let (_world, mut table) = test_setup();

        // When
        let value = unsafe {
            <Option<&Comp1>>::fetch_mut(entity::Entity::new(0.into()), &mut table, 0.into())
        };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.value, 10);
    }

    #[test]
    fn fetch_for_optional_mut_component_ref() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value =
            unsafe { <Option<&mut Comp1>>::fetch(entity::Entity::new(0.into()), &table, 0.into()) };

        // Then
        assert!(value.is_none());
    }

    #[test]
    fn fetch_mut_for_optional_mut_component_ref() {
        // Given
        let (_world, mut table) = test_setup();

        // When
        let value = unsafe {
            <Option<&mut Comp1>>::fetch_mut(entity::Entity::new(0.into()), &mut table, 0.into())
        };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.value, 10);
    }

    #[test]
    fn fetch_for_optional_component_ref_not_in_table() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value =
            unsafe { <Option<&Comp4>>::fetch(entity::Entity::new(0.into()), &table, 0.into()) };

        // Then - Optional components return Some(None) when component is missing
        assert!(value.is_some());
        assert!(value.unwrap().is_none());
    }

    #[test]
    fn fetch_for_optional_mut_component_ref_not_in_table() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value =
            unsafe { <Option<&mut Comp4>>::fetch(entity::Entity::new(0.into()), &table, 0.into()) };

        // Then
        assert!(value.is_none());
    }

    #[test]
    fn fetch_for_entity() {
        // Given
        let (_world, table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::Entity>::fetch(entity, &table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value, entity);
    }

    #[test]
    fn fetch_mut_for_entity() {
        // Given
        let (_world, mut table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::Entity>::fetch_mut(entity, &mut table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value, entity);
    }
}
