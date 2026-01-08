use crate::core::ecs::{component, entity, storage};

/// A trait used to identify types that can be used as parameters to queries. Each type used in a
/// query must produce a single [ParamSpec] instance that will be used to drive the queries.
///
/// Generally these parameters will be [entity::Entity] or [component::Component] related.
/// - Component params be read-only or mutable depending on the type used in the query.
/// - Entity params are always passed by value.
/// - [entity::Ref] and [entity::RefMut] are valid params if additional entity access is needed.
///
/// Example:
/// ```rust, ignore
/// use crate::core::ecs::{
///     query::param::{Param, ParamType},
///     component,
/// };
///
/// #[derive(Component)]
/// struct Comp;
///
/// let components = component::Registry::new();
/// components.register::<Comp>();
///
/// let spec = <&Comp>::query_param_spec(&components);
///
/// assert_eq!(
///    spec.param_type(),
///    ParamType::Component(components.get::<Comp1>().unwrap())
///);
///assert!(!spec.is_mut());
/// ````
pub trait Param<'w>: Sized {
    /// Get the query parameter for this type. The component registry is provided to allow a
    /// parameter type to lookup or register component information.
    fn query_param_spec(components: &component::Registry) -> ParamSpec;

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
    unsafe fn fetch_value(
        entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self>;

    /// Fetch component references for a single entity row from a mutable table.
    ///
    /// This is the mutable variant of [`fetch`](Param::fetch), allowing mutable access
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
    unsafe fn fetch_value_mut(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self>;
}

/// An enumeration of possible query parameter types.
/// This enumeration limits, but normalizes the possible types used in a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// The query results should include the entity.
    Entity,
    /// The query results should include an entity reference.
    EntityRef,
    /// The query results should include a specific component (by ID).
    Component(component::Id),
}

/// A structure containing the specification for a single query parameter.
/// The `param_type` fields gives the query engine the understanding of what information is needed in
/// the results. This will also provide information on whether the query results should include
/// mutable references.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    /// The type of parameter this is.
    param_type: ParamType,
    /// Whether the results should contain mutable references to the value
    is_mut: bool,
}

impl ParamSpec {
    /// Construct a new instance with type and mutability.
    #[inline]
    pub const fn new(param_type: ParamType, is_mut: bool) -> Self {
        Self { param_type, is_mut }
    }

    /// Get the type for this parameter.
    #[inline]
    pub fn param_type(&self) -> ParamType {
        self.param_type
    }

    /// Get the mutability flag.
    #[inline]
    pub fn is_mut(&self) -> bool {
        self.is_mut
    }
}

/// Implement the [Param] trait for a [component::Component] reference.
impl<'w, C: component::Component> Param<'w> for &'w C {
    /// Return [ParamSpec] with the [ParamType::Component] type and mutability `false`.
    ///
    /// # Note
    /// This will register the component `C` type if necessary.
    fn query_param_spec(components: &component::Registry) -> ParamSpec {
        ParamSpec::new(ParamType::Component(components.register::<C>()), false)
    }

    unsafe fn fetch_value(
        _entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        unsafe { table.get::<C>(row) }
    }

    unsafe fn fetch_value_mut(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        unsafe { table.get::<C>(row) }
    }
}

/// Implement the [Param] trait for a mutable [component::Component] reference.
impl<'w, C: component::Component> Param<'w> for &'w mut C {
    /// Return [ParamSpec] with the [ParamType::Component] type and mutability `true`.
    ///
    /// # Note
    /// This will register the [component::Component] `C` type if necessary.
    fn query_param_spec(components: &component::Registry) -> ParamSpec {
        ParamSpec::new(ParamType::Component(components.register::<C>()), true)
    }

    unsafe fn fetch_value(
        _entity: entity::Entity,
        _table: &'w storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        // Cannot fetch mutable reference from immutable table
        None
    }

    unsafe fn fetch_value_mut(
        _entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        unsafe { table.get_mut::<C>(row) }
    }
}

/// Implement the [Param] trait for [entity::Entity].
impl<'w> Param<'w> for entity::Entity {
    /// Return [ParamSpec] with the [ParamType::Entity] and mutability always `false`.
    fn query_param_spec(_components: &component::Registry) -> ParamSpec {
        ParamSpec::new(ParamType::Entity, false)
    }

    unsafe fn fetch_value(
        entity: entity::Entity,
        _table: &'w storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        // Cannot fetch mutable reference from immutable table
        Some(entity)
    }

    unsafe fn fetch_value_mut(
        entity: entity::Entity,
        _table: &'w mut storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        Some(entity)
    }
}

/// Implement the [Param] trait for [entity::Ref].
impl<'w> Param<'w> for entity::Ref<'w> {
    /// Return [ParamSpec] with the [ParamType::EntityRef] and mutability always `false`.
    fn query_param_spec(_components: &component::Registry) -> ParamSpec {
        ParamSpec::new(ParamType::EntityRef, false)
    }

    unsafe fn fetch_value(
        entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        Some(entity::Ref::new(entity, table, row))
    }

    unsafe fn fetch_value_mut(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        Some(entity::Ref::new(entity, table, row))
    }
}

/// Implement the [Param] trait for [entity::RefMut].
impl<'w> Param<'w> for entity::RefMut<'w> {
    /// Return [ParamSpec] with the [ParamType::EntityRef] and mutability always `true`.
    fn query_param_spec(_components: &component::Registry) -> ParamSpec {
        ParamSpec::new(ParamType::EntityRef, true)
    }

    unsafe fn fetch_value(
        _entity: entity::Entity,
        _table: &'w storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        // Cannot fetch mutable reference from immutable table
        None
    }

    unsafe fn fetch_value_mut(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        Some(entity::RefMut::new(entity, table, row))
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::core::ecs::{
        component, entity,
        query::param::{Param, ParamType},
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
        let spec = <&Comp1>::query_param_spec(world.components());

        // Then
        assert_eq!(
            spec.param_type(),
            ParamType::Component(world.components().get::<Comp1>().unwrap())
        );
        assert!(!spec.is_mut());
    }

    #[test]
    fn spec_for_component_ref_mut() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <&mut Comp1>::query_param_spec(world.components());

        // Then
        assert_eq!(
            spec.param_type(),
            ParamType::Component(world.components().get::<Comp1>().unwrap())
        );
        assert!(spec.is_mut());
    }

    #[test]
    fn spec_for_entity() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = entity::Entity::query_param_spec(world.components());

        // Then
        assert_eq!(spec.param_type(), ParamType::Entity);
        assert!(!spec.is_mut());
    }

    #[test]
    fn spec_for_entity_ref() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = entity::Ref::query_param_spec(world.components());

        // Then
        assert_eq!(spec.param_type(), ParamType::EntityRef);
        assert!(!spec.is_mut());
    }

    #[test]
    fn spec_for_entity_ref_mut() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = entity::RefMut::query_param_spec(world.components());

        // Then
        assert_eq!(spec.param_type(), ParamType::EntityRef);
        assert!(spec.is_mut());
    }

    #[test]
    fn fetch_for_component_ref() {
        // Given
        let (_world, table) = test_setup();

        // When
        let value1 =
            unsafe { <&Comp1>::fetch_value(entity::Entity::new(0.into()), &table, 0.into()) };
        let value2 =
            unsafe { <&Comp2>::fetch_value(entity::Entity::new(1.into()), &table, 1.into()) };

        let value3 =
            unsafe { <&Comp3>::fetch_value(entity::Entity::new(2.into()), &table, 2.into()) };

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
            <&Comp1>::fetch_value_mut(
                entity::Entity::new(0.into()),
                &mut *(&mut table as *mut storage::Table),
                0.into(),
            )
        };
        let value2 = unsafe {
            <&Comp2>::fetch_value_mut(
                entity::Entity::new(1.into()),
                &mut *(&mut table as *mut storage::Table),
                1.into(),
            )
        };

        let value3 = unsafe {
            <&Comp3>::fetch_value_mut(
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
            <&mut Comp1>::fetch_value(
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
            <&mut Comp1>::fetch_value_mut(
                entity::Entity::new(0.into()),
                &mut *(&mut table as *mut storage::Table), // Nasty multi-borrow workaround.
                0.into(),
            )
        };
        let value2 = unsafe {
            <&mut Comp2>::fetch_value_mut(
                entity::Entity::new(1.into()),
                &mut *(&mut table as *mut storage::Table),
                1.into(),
            )
        };

        let value3 = unsafe {
            <&mut Comp3>::fetch_value_mut(
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
    fn fetch_for_entity() {
        // Given
        let (_world, table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::Entity>::fetch_value(entity, &table, 0.into()) };

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
        let value = unsafe { <entity::Entity>::fetch_value_mut(entity, &mut table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value, entity);
    }

    #[test]
    fn fetch_for_entity_ref() {
        // Given
        let (_world, table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::Ref>::fetch_value(entity, &table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.entity(), entity);
    }

    #[test]
    fn fetch_mut_for_entity_ref() {
        // Given
        let (_world, mut table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::Ref>::fetch_value_mut(entity, &mut table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.entity(), entity);
    }

    #[test]
    fn fetch_for_entity_ref_mut() {
        // Given
        let (_world, table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::RefMut>::fetch_value(entity, &table, 0.into()) };

        // Then
        assert!(value.is_none());
    }

    #[test]
    fn fetch_mut_for_entity_ref_mut() {
        // Given
        let (_world, mut table) = test_setup();

        let entity = entity::Entity::new(22.into());

        // When
        let value = unsafe { <entity::RefMut>::fetch_value_mut(entity, &mut table, 0.into()) };

        // Then
        assert!(value.is_some());
        let value = value.unwrap();
        assert_eq!(value.entity(), entity);
    }
}
