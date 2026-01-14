//! Query data types and specifications.
//!
//! This module defines the [`Data`] trait and [`DataSpec`] struct, which represent
//! complete query specifications that can yield results.
//!
//! # Data Trait
//!
//! The [`Data`] trait is implemented by:
//! - Any single [`Parameter`] type (automatic implementation)
//! - Tuples of `Data` types (enables nested queries)
//! - The unit type `()` (empty query, returns nothing)
//!
//! # Relationship to Parameter
//!
//! - **Parameter**: Individual query elements (`&C`, `Entity`, etc.)
//! - **Data**: Complete query specifications that can be executed
//!
//! A single `Parameter` automatically implements `Data`, and tuples of `Data` types
//! (including tuples of `Parameter` types) also implement `Data`.
//!
//! [`Parameter`]: super::param::Parameter

use std::collections::HashSet;

use crate::{
    all_tuples,
    ecs::{
        component::{self},
        entity,
        query::{
            IntoQuery, Query,
            param::{Parameter, ParameterSpec},
        },
        storage, world,
    },
};

/// Types that can be used as complete query specifications.
///
/// This trait represents a complete query - a set of parameters that can be executed
/// to yield results from the ECS. Most users will use tuples of [`Parameter`] types
/// rather than implementing this trait directly.
///
/// # Implementations
///
/// - **Single Parameter**: Any type implementing [`Parameter`] automatically implements `Data`
/// - **Tuples**: Tuples of `Data` types implement `Data` (up to 26 elements)
/// - **Unit**: The unit type `()` implements `Data` (empty query)
///
/// # Methods
///
/// - [`spec`]: Generate the specification for this query
/// - [`fetch`]: Fetch data from an immutable table
/// - [`fetch_mut`]: Fetch data from a mutable table
///
/// # Examples
///
/// ```rust,ignore
/// // Single parameter (Parameter → Data)
/// Query::<&Position>::new(components)
///
/// // Tuple of parameters (tuple of Parameter → Data)
/// Query::<(&Position, &mut Velocity)>::new(components)
///
/// // Nested tuples (tuple of Data → Data)
/// Query::<(Entity, (&Position, &Velocity))>::new(components)
/// ```
///
/// [`Parameter`]: super::param::Parameter
/// [`spec`]: Data::spec
pub trait Data: Sized {
    type Data<'w>;

    /// Get the [`DataSpec`] for this query type.
    ///
    /// This method analyzes the type and produces a specification describing what
    /// data the query needs. The component registry is provided to allow registration
    /// or lookup of component types.
    ///
    /// # Parameters
    ///
    /// - `components`: The component registry for type registration and lookup
    ///
    /// # Returns
    ///
    /// A [`DataSpec`] containing the list of parameters this query needs.
    fn spec(components: &component::Registry) -> DataSpec;

    /// Fetch query data from an immutable table row.
    ///
    /// This method retrieves the data specified by this query type from a specific
    /// entity in a table. It's called internally by the query iterator for each
    /// matching entity.
    ///
    /// # Parameters
    ///
    /// - `entity`: The entity to fetch data for
    /// - `table`: The table containing the entity
    /// - `row`: The row index of the entity within the table
    ///
    /// # Returns
    ///
    /// - `Some(Self)` if all required components are present
    /// - `None` if any required component is missing or the row is invalid
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The component types match the actual types stored for their component IDs
    /// - The table and row correspond to a valid entity location
    /// - Type safety is enforced through the component registry's type-to-ID mapping
    unsafe fn fetch<'w>(
        entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self::Data<'w>>;

    /// Fetch query data from a mutable table row.
    ///
    /// This is the mutable variant of [`fetch`], allowing mutable access to
    /// components when needed. It's called internally by the query iterator when
    /// the query includes mutable component references.
    ///
    /// # Parameters
    ///
    /// - `entity`: The entity to fetch data for
    /// - `table`: The mutable table containing the entity
    /// - `row`: The row index of the entity within the table
    ///
    /// # Returns
    ///
    /// - `Some(Self)` if all required components are present
    /// - `None` if any required component is missing or the row is invalid
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The component types match the actual types stored for their component IDs
    /// - The table and row correspond to a valid entity location
    /// - No component is requested multiple times (validated at query invocation)
    /// - Type safety is enforced through the component registry's type-to-ID mapping
    ///
    /// [`fetch`]: Data::fetch
    unsafe fn fetch_mut<'w>(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Data<'w>>;
}

/// A specification describing what data a query accesses.
///
/// This struct contains the list of [`ParameterSpec`]s that make up a complete query.
/// It's used by the query system to:
/// - Validate that no component is requested multiple times (aliasing check)
/// - Determine which tables contain all required components
/// - Identify whether mutable world access is needed
///
/// # Construction
///
/// `DataSpec` is typically created by calling [`Data::spec`], not directly.
///
/// [`ParameterSpec`]: super::param::ParameterSpec
#[derive(Debug, Default, Clone)]
pub struct DataSpec {
    /// The parameters expected in the query results.
    params: Vec<ParameterSpec>,
}

impl DataSpec {
    /// An empty data specification.
    const EMPTY: DataSpec = Self::new(vec![]);

    /// Construct a new query with provided params.
    #[inline]
    pub const fn new(params: Vec<ParameterSpec>) -> Self {
        Self { params }
    }

    /// Get the parameters for this query.
    #[inline]
    pub fn params(&self) -> &[ParameterSpec] {
        &self.params
    }

    /// Convert this query specification to a [component::Spec].
    ///
    /// This extracts the component IDs from the parameter list and creates a
    /// [`component::Spec`] that can be used to find matching tables.
    ///
    /// # Filtering
    ///
    /// Only **required** (non-optional) component IDs are included in the result.
    /// Optional components don't restrict which tables match the query.
    ///
    /// # Returns
    ///
    /// A [`component::Spec`] containing only the required component IDs.
    pub fn as_component_spec(&self) -> component::Spec {
        let ids: Vec<component::Id> = self
            .params
            .iter()
            .filter_map(|t| match t {
                ParameterSpec::Component(id, _mut, optional) => match optional {
                    true => None,
                    false => Some(*id),
                },
                _ => None,
            })
            .collect();
        component::Spec::new(ids)
    }

    /// Convert this query specification to a world access request.
    ///
    /// This extracts the component IDs and mutability from the parameter list and creates a
    /// [`world::AccessRequest`] that can be used to ensure callers have access to execute this
    /// query.
    ///
    /// # Returns
    ///
    /// A [`world::AccessRequest`].
    ///
    /// # Note
    ///
    /// Optional components (`Option<&C>`, `Option<&mut C>`) are included in the access
    /// request. Even though the query doesn't require the component to exist on every
    /// entity, accessing it when present still requires the appropriate permissions.
    pub fn as_access_request(&self) -> world::AccessRequest {
        let mut immutable_ids: Vec<component::Id> = Vec::new();
        let mut mutable_ids: Vec<component::Id> = Vec::new();
        for param in self.params.iter() {
            if let ParameterSpec::Component(id, is_mut, _optional) = param {
                if *is_mut {
                    mutable_ids.push(*id);
                } else {
                    immutable_ids.push(*id);
                }
            }
        }
        world::AccessRequest::to_components(
            component::Spec::new(immutable_ids),
            component::Spec::new(mutable_ids),
        )
    }

    /// Check if this query requires mutable access to any components.
    ///
    /// Returns `true` if any parameter in the query is a mutable component reference
    /// (`&mut C` or `Option<&mut C>`).
    ///
    /// # Returns
    ///
    /// - `true` if any parameter requires mutable access
    /// - `false` if all parameters are immutable
    ///
    /// # Note
    ///
    /// Mutable queries require mutable world access and may reduce opportunities
    /// for parallelism in future multi-threaded query execution.
    pub fn is_mutable(&self) -> bool {
        self.params.iter().any(|param| match param {
            ParameterSpec::Component(_, is_mut, _) => *is_mut,
            _ => false,
        })
    }

    /// Validate that this query specification has no aliasing violations.
    ///
    /// This checks that no component ID appears more than once in the parameter list.
    /// Multiple immutable references could theoretically be allowed, but for simplicity
    /// we disallow any duplicates.
    ///
    /// # Returns
    ///
    /// - `true` if the query is valid (no duplicate component IDs)
    /// - `false` if the same component is requested multiple times
    ///
    /// # Note
    ///
    /// Rust's type system cannot prevent duplicate components at compile time
    /// (e.g., `(&Foo, &Foo)` is valid Rust), so this check happens at runtime.
    pub fn is_valid(&self) -> bool {
        let mut set = HashSet::with_capacity(self.params.len());
        for &x in &self.params {
            let ParameterSpec::Component(id, _, _) = x else {
                continue;
            };
            if !set.insert(id) {
                return false;
            }
        }
        true
    }
}

/// A [`Data`] implementation for any type that implements [`Parameter`].
///
/// This blanket implementation allows any valid parameter type to be used directly as a query,
/// enabling single-element queries like `Query::<&Position>` or `Query::<Entity>`.
impl<P: Parameter> Data for P {
    type Data<'w> = P::Value<'w>;

    /// Return [`DataSpec`] with a single [`ParameterSpec`] derived from parameter `P`.
    fn spec(components: &component::Registry) -> DataSpec {
        DataSpec::new(vec![P::spec(components)])
    }

    unsafe fn fetch<'w>(
        entity: entity::Entity,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Option<Self::Data<'w>> {
        unsafe { P::fetch(entity, table, row) }
    }

    unsafe fn fetch_mut<'w>(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self::Data<'w>> {
        unsafe { P::fetch_mut(entity, table, row) }
    }
}

/// A [`Data`] implementation for the unit type (empty query).
///
/// An empty query matches no entities and always returns `None` from fetch operations.
/// This is primarily useful as a base case for the type system.
impl Data for () {
    type Data<'w> = ();
    fn spec(_components: &component::Registry) -> DataSpec {
        DataSpec::EMPTY
    }

    unsafe fn fetch(
        _entity: entity::Entity,
        _table: &storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        None
    }

    unsafe fn fetch_mut(
        _entity: entity::Entity,
        _table: &mut storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        None
    }
}

/// Implement [`Data`] for tuples of [`Data`] types.
macro_rules! tuple_query {
    ($($name: ident),*) => {
        impl<$($name: Data),*> Data for ($($name,)*) {

            type Data<'w> = ($($name::Data<'w>,)*);

            fn spec(components: &component::Registry) -> DataSpec {
                let mut params = Vec::new();
                $(
                    params.extend(
                        <$name>::spec(components).params().iter().cloned()
                    );
                )*
                DataSpec::new(params)
            }

            unsafe fn fetch<'w>(
                entity: entity::Entity,
                table: &'w storage::Table,
                row: storage::Row,
            ) -> Option<Self::Data<'w>> {
                Some((
                    $(
                        unsafe { <$name>::fetch(entity, table, row )? },
                    )*
                ))
            }

            unsafe fn fetch_mut<'w>(
                entity: entity::Entity,
                table: &'w mut storage::Table,
                row: storage::Row,
            ) -> Option<Self::Data<'w>> {
                Some((
                    $(
                        unsafe { <$name>::fetch_mut(entity, &mut *(table as *mut storage::Table), row )? },
                    )*
                ))
            }
        }
    }
}

// Generate implementations for tuples up to 26 elements (A-Z)
all_tuples!(tuple_query);

/// Implement [`IntoQuery`] for any type that implements [`Data`].
impl<D: Data> IntoQuery<D> for D {
    /// Convert this data type into a [`Query`].
    fn into_query(components: &component::Registry) -> Query<D> {
        Query::<D>::new(components)
    }
}

#[cfg(test)]
mod tests {

    use rusty_macros::Component;

    use crate::ecs::{
        component, entity,
        query::{IntoQuery, data::Data, param::ParameterSpec},
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
        #[allow(dead_code)]
        value: i32,
    }

    fn test_setup() -> (world::World, storage::Table) {
        let world = world::World::new(world::Id::new(0));
        let spec = world.components().spec::<(Comp1, Comp2, Comp3)>();
        let table = storage::Table::new(storage::table::Id::new(0), spec, world.components());
        (world, table)
    }

    #[test]
    fn component_as_query_data() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <&Comp1>::spec(world.components());

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 1);
        let param = params[0];
        assert_eq!(
            param,
            ParameterSpec::Component(world.components().get::<Comp1>().unwrap(), false, false)
        );
    }

    #[test]
    fn entity_as_query_data() {
        // Given
        let registry = component::Registry::new();

        // When
        let spec = entity::Entity::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 1);
        let param = params[0];
        assert_eq!(param, ParameterSpec::Entity);
    }

    #[test]
    fn entity_and_comp_as_query_data() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();

        // When
        let spec = <(entity::Entity, &mut Comp1)>::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 2);
        let param = params[0];
        assert_eq!(param, ParameterSpec::Entity);

        let param = params[1];
        assert_eq!(
            param,
            ParameterSpec::Component(registry.get::<Comp1>().unwrap(), true, false)
        );
    }

    #[test]
    fn entity_and_comps_mixed_as_query_data() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 3);
        let param = params[0];
        assert_eq!(param, ParameterSpec::Entity);

        let param = params[1];
        assert_eq!(
            param,
            ParameterSpec::Component(registry.get::<Comp1>().unwrap(), true, false)
        );
        let param = params[2];
        assert_eq!(
            param,
            ParameterSpec::Component(registry.get::<Comp2>().unwrap(), false, false)
        );
    }

    #[test]
    fn entity_and_comps_component_spec() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();
        let comp2_id = registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 3);

        let comp_spec = spec.as_component_spec();
        assert_eq!(comp_spec.ids(), vec![comp1_id, comp2_id]);
    }

    #[test]
    fn comps_access() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();
        let comp2_id = registry.register::<Comp2>();
        let comp3_id = registry.register::<Comp3>();

        // When
        let spec = <(&mut Comp1, &Comp2, &Comp3)>::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 3);

        let request = spec.as_access_request();
        assert_eq!(
            request,
            world::AccessRequest::to_components(
                component::Spec::new(vec![comp2_id, comp3_id]),
                component::Spec::new(vec![comp1_id]),
            )
        );
    }

    #[test]
    fn optional_comps_included_in_access() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();
        let comp2_id = registry.register::<Comp2>();
        let comp3_id = registry.register::<Comp3>();

        // When - query with optional components
        let spec = <(&Comp1, Option<&Comp2>, Option<&mut Comp3>)>::spec(&registry);

        // Then - optional components should be included in access request
        let request = spec.as_access_request();

        // Should grant access to all components (including optional ones)
        assert_eq!(
            request,
            world::AccessRequest::to_components(
                component::Spec::new(vec![comp1_id, comp2_id]),
                component::Spec::new(vec![comp3_id]),
            )
        );

        // Should not grant access to components not in query
        // Use a component ID that's definitely not in our query
        let fake_id = component::Id::new(99);
        assert_ne!(
            request,
            world::AccessRequest::to_components(
                component::Spec::new(vec![fake_id]),
                component::Spec::EMPTY,
            )
        );
    }

    #[test]
    fn entity_not_in_access_request() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();

        // When - query includes Entity
        let spec = <(entity::Entity, &Comp1)>::spec(&registry);

        // Then - Entity should not affect the access request
        let request = spec.as_access_request();

        // Should only have component access, not world access
        assert!(!request.world());
        assert!(!request.world_mut());

        // Should grant the component access
        assert_eq!(
            request,
            world::AccessRequest::to_components(
                component::Spec::new(vec![comp1_id]),
                component::Spec::EMPTY,
            )
        );
    }

    #[test]
    fn empty_query_access_is_none() {
        // Given
        let registry = component::Registry::new();

        // When - empty query
        let spec = <()>::spec(&registry);

        // Then - should return no access
        let request = spec.as_access_request();
        assert!(request.is_none());
    }

    #[test]
    fn access_request_conflicts_correctly() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // Query A: reads Comp1, writes Comp2
        let spec_a = <(&Comp1, &mut Comp2)>::spec(&registry);
        let access_a = spec_a.as_access_request();

        // Query B: writes Comp1 (conflicts with A's read)
        let spec_b = <&mut Comp1>::spec(&registry);
        let access_b = spec_b.as_access_request();

        // Query C: reads Comp1 (no conflict with A)
        let spec_c = <&Comp1>::spec(&registry);
        let access_c = spec_c.as_access_request();

        // Query D: reads Comp2 (conflicts with A's write)
        let spec_d = <&Comp2>::spec(&registry);
        let access_d = spec_d.as_access_request();

        // Then
        // A and B conflict (A reads Comp1, B writes Comp1)
        assert!(access_a.conflicts_with(&access_b));

        // A and C don't conflict (both read Comp1, A writes Comp2 but C doesn't touch it)
        assert!(!access_a.conflicts_with(&access_c));

        // A and D conflict (A writes Comp2, D reads Comp2)
        assert!(access_a.conflicts_with(&access_d));
    }

    #[test]
    fn optional_comps_component_spec() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(
            entity::Entity,
            &mut Comp1,
            Option<&Comp2>,
            Option<&mut Comp3>,
        )>::spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 4);

        let comp_spec = spec.as_component_spec();
        assert_eq!(comp_spec.ids(), vec![comp1_id]);
    }

    #[test]
    fn components_not_mutable() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &Comp1, &Comp2)>::spec(&registry);

        // Then
        assert!(!spec.is_mutable());
    }

    #[test]
    fn components_mutable() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::spec(&registry);

        // Then
        assert!(spec.is_mutable());
    }

    #[test]
    fn components_valid() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::spec(&registry);

        // Then
        assert!(spec.is_valid());
    }

    #[test]
    fn components_invalid() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp1)>::spec(&registry);

        // Then
        assert!(!spec.is_valid());
    }

    #[test]
    fn components_invalid_nesting() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, (&Comp1, &Comp2))>::spec(&registry);

        // Then
        assert!(!spec.is_valid());
    }

    fn spawn_entities(
        world: &mut world::World,
    ) -> (entity::Entity, entity::Entity, entity::Entity) {
        (
            world.spawn(Comp1 { value: 10 }),
            world.spawn((Comp1 { value: 10 }, Comp2 { value: 20 })),
            world.spawn((
                Comp1 { value: 10 },
                Comp2 { value: 20 },
                Comp3 { value: 30 },
            )),
        )
    }

    #[test]
    fn fetch_empty_query_data() {
        // Given
        let (mut world, _table) = test_setup();
        let (entity, _, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for(entity).unwrap();

        // When
        let result = unsafe { <()>::fetch(entity, table, row) };

        // Then
        assert!(result.is_none());
    }

    #[test]
    fn fetch_single_comp_query_data() {
        // Given
        let (mut world, _table) = test_setup();
        let (entity, _, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for(entity).unwrap();

        // When
        let result = unsafe { <&Comp1>::fetch(entity, table, row) };

        // Then
        assert!(result.is_some());
        let result = result.unwrap();

        assert_eq!(result.value, 10);
    }

    #[test]
    fn fetch_mut_single_comp_query_data() {
        // Given
        let (mut world, _table) = test_setup();
        let (entity, _, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for_mut(entity).unwrap();

        // When
        let result = unsafe { <&mut Comp1>::fetch_mut(entity, table, row) };

        // Then
        assert!(result.is_some());
        let result = result.unwrap();

        assert_eq!(result.value, 10);
    }

    #[test]
    fn fetch_entity_and_comps_query_data() {
        // Given
        let (mut world, _table) = test_setup();
        let (_, entity, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for_mut(entity).unwrap();

        // When
        let result = unsafe { <(entity::Entity, &Comp1, &Comp2)>::fetch(entity, table, row) };

        // Then
        assert!(result.is_some());
        let (entity_res, comp1, comp2) = result.unwrap();
        assert_eq!(entity_res, entity);
        assert_eq!(comp1.value, 10);
        assert_eq!(comp2.value, 20);
    }

    #[test]
    fn fetch_entity_and_comps_mut_query_data() {
        // Given
        let (mut world, _table) = test_setup();
        let (_, entity, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for_mut(entity).unwrap();

        // When
        let result =
            unsafe { <(entity::Entity, &Comp1, &mut Comp2)>::fetch_mut(entity, table, row) };

        // Then
        assert!(result.is_some());
        let (entity_res, comp1, comp2) = result.unwrap();
        assert_eq!(entity_res, entity);
        assert_eq!(comp1.value, 10);
        assert_eq!(comp2.value, 20);
        comp2.value = 120;
        let (_, _, comp2) =
            unsafe { <(entity::Entity, &Comp1, &mut Comp2)>::fetch_mut(entity, table, row) }
                .unwrap();
        assert_eq!(comp2.value, 120);
    }

    #[test]
    fn fetch_entity_and_comps_nested() {
        // Given
        let (mut world, _table) = test_setup();
        let (_, entity, _) = spawn_entities(&mut world);
        let (table, row) = world.storage_for_mut(entity).unwrap();

        // When
        let result =
            unsafe { <(entity::Entity, (&Comp1, &mut Comp2))>::fetch_mut(entity, table, row) };

        // Then
        assert!(result.is_some());
        let (entity_res, (comp1, comp2)) = result.unwrap();
        assert_eq!(entity_res, entity);
        assert_eq!(comp1.value, 10);
        assert_eq!(comp2.value, 20);
        comp2.value = 120;
        let (_, _, comp2) =
            unsafe { <(entity::Entity, &Comp1, &mut Comp2)>::fetch_mut(entity, table, row) }
                .unwrap();
        assert_eq!(comp2.value, 120);
    }

    #[test]
    fn data_into_query() {
        // Given
        let (world, _table) = test_setup();
        let comp1_id = world.components().get::<Comp1>().unwrap();
        let comp2_id = world.components().get::<Comp2>().unwrap();

        // When
        let query = <(entity::Entity, &Comp1, &mut Comp2)>::into_query(world.components());

        // Then
        assert_eq!(
            *query.required_access(),
            world::AccessRequest::to_components(
                component::Spec::new(vec![comp1_id]),
                component::Spec::new(vec![comp2_id]),
            )
        );
    }
}
