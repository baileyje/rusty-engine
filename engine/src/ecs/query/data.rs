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

use crate::ecs::{
    component, entity,
    query::param::{Parameter, ParameterSpec},
    storage,
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

    /// Convert this query specification to a component specification.
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

/// A query implementation for any type is is a valid [Param] type. This allows any of the valid
/// parameter types to be used directly as a query. This enables query by a single component type
/// or entity.
impl<P: Parameter> Data for P {
    type Data<'w> = P::Value<'w>;

    /// Return [DataSpec] with a single [ParamSpec] derived from [Param] `P`.
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

/// A query implementation that is empty.
///
/// Note: This is interpreted as I want nothing, and likely useless...
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

/// Implement Query for tuples of [Data] types.
macro_rules! tuple_query_impl {
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

/// Implement Query for tuples of [Param] types recursively.
macro_rules! tuple_query {
    ($head_ty:ident) => {
        tuple_query_impl!($head_ty);
    };
    ($head_ty:ident, $( $tail_ty:ident ),*) => (
        tuple_query_impl!($head_ty, $( $tail_ty ),*);
        tuple_query!($( $tail_ty ),*);
    );
}

// Generate implementations for tuples up to 26 elements (A-Z)
tuple_query! {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z
}

#[cfg(test)]
mod tests {

    use rusty_macros::Component;

    use crate::ecs::{
        component, entity,
        query::{data::Data, param::ParameterSpec},
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
}
