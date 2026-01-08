use crate::core::ecs::{
    component, entity,
    query::param::{Param, ParamSpec, ParamType},
    storage,
};

/// A trait used to identify types that can be used to generate query specifications.
/// Any type that implements this trait can be used as a query for entities and components in the
/// ECS. Generally this will be a mix of [entity::Entity] and [component::Component] references.
///
/// Example:
/// ```rust, ignore
/// use rusty_engine::core::ecs::{ component, entity, query::Query};
/// use rust_macros::Component;
///
/// #[derive(Component)]
/// struct Comp1;
///
/// #[derive(Component)]
/// struct Comp2;
///
/// let components = component::Registry::new();
/// components.register::<Comp1>();
/// components.register::<Comp2>();
///
/// let spec = <Entity, &Comp1, &mut Comp2>::query_data_spec(&components);
///
/// assert_eq!(spec.params().len(), 3);
///
/// ```
pub trait Data<'w>: Sized {
    /// Get the [QuerySpec] for a type.
    /// The component registry is provided to allow a
    /// parameter type to lookup or register component information.
    fn query_data_spec(components: &component::Registry) -> DataSpec;

    unsafe fn fetch(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self>;
}

/// A structure that contains the specification an ECS query data. This will at a minimum return the
/// required parameters ([ParamSpec]) for the query.
#[derive(Debug, Default, Clone)]
pub struct DataSpec {
    /// The parameters expected in the query results.
    params: Vec<ParamSpec>,
}

impl DataSpec {
    /// Construct a new query with provided params.
    #[inline]
    pub fn new(params: Vec<ParamSpec>) -> Self {
        Self { params }
    }

    /// Get the parameters for this query.
    #[inline]
    pub fn params(&self) -> &[ParamSpec] {
        &self.params
    }

    /// Get a component specification that includes any components ids that were
    /// referenced by this query.
    pub fn as_component_spec(&self) -> component::Spec {
        let ids: Vec<component::Id> = self
            .params
            .iter()
            .map(|p| p.param_type())
            .filter_map(|t| match t {
                ParamType::Component(id) => Some(id),
                _ => None,
            })
            .collect();
        component::Spec::new(ids)
    }
}

/// A query implementation for any type is is a valid [Param] type. This allows any of the valid
/// parameter types to be used directly as a query. This enables query by a single component type
/// or entity.
impl<'w, P: Param<'w>> Data<'w> for P {
    /// Return [DataSpec] with a single [ParamSpec] derived from [Param] `P`.
    fn query_data_spec(components: &component::Registry) -> DataSpec {
        DataSpec::new(vec![P::query_param_spec(components)])
    }

    unsafe fn fetch(
        entity: entity::Entity,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Option<Self> {
        unsafe { P::fetch_value(entity, table, row) }
    }
}

/// A query implementation that is empty.
///
/// Note: This is interpreted as I want nothing, and likely useless...
impl<'w> Data<'w> for () {
    fn query_data_spec(_components: &component::Registry) -> DataSpec {
        DataSpec::new(vec![])
    }

    unsafe fn fetch(
        _entity: entity::Entity,
        _table: &'w mut storage::Table,
        _row: storage::Row,
    ) -> Option<Self> {
        None
    }
}

/// Implement Query for tuples of [Param] types.
macro_rules! tuple_query_impl {
    ($(($name: ident, $alias: ident)),*) => {
        impl<'w, $($name: Param<'w>),*> Data<'w> for ($($name,)*) {
            fn query_data_spec(components: &component::Registry) -> DataSpec {
                DataSpec::new(vec![
                    $(
                        <$name as Param>::query_param_spec(components),
                    )*
                ])
            }

            unsafe fn fetch(
                entity: entity::Entity,
                table: &'w mut storage::Table,
                row: storage::Row,
            ) -> Option<Self> {
                Some((
                    $(
                        unsafe { <$name as Param>::fetch_value_mut(entity, &mut *(table as *mut storage::Table), row)? },
                    )*
                ))
            }
        }
    }
}
/// Implement Query for tuples of [Param] types recursively.
macro_rules! tuple_query {
    (($head_ty:ident, $head_alias: ident)) => {
        tuple_query_impl!(($head_ty, $head_alias));
    };
    (($head_ty:ident, $head_alias: ident), $( ($tail_ty:ident, $tail_alias: ident) ),*) => (
        tuple_query_impl!(($head_ty, $head_alias), $(( $tail_ty, $tail_alias) ),*);
        tuple_query!($( ($tail_ty, $tail_alias) ),*);
    );
}
// This can't be the best way to do this, but it works for now.
tuple_query! {
    (A, a), (B, b), (C, c), (D, d), (E, e), (F, f),
    (G, g), (H, h), (I, i), (J, j), (K, k), (L, l),
    (M, m), (N, n), (O, o), (P, p), (Q, q), (R, r),
    (S, s), (T, t), (U, u), (V, v), (W, w), (X, x),
    (Y, y), (Z, z)
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::core::ecs::{
        component, entity,
        query::{data::Data, param::ParamType},
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
    fn component_as_query() {
        // Given
        let (world, _table) = test_setup();

        // When
        let spec = <&Comp1>::query_data_spec(world.components());

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 1);
        let param = params[0];
        assert_eq!(
            param.param_type(),
            ParamType::Component(world.components().get::<Comp1>().unwrap())
        );
        assert!(!param.is_mut());
    }

    #[test]
    fn entity_as_query() {
        // Given
        let registry = component::Registry::new();

        // When
        let spec = entity::Entity::query_data_spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 1);
        let param = params[0];
        assert_eq!(param.param_type(), ParamType::Entity);
        assert!(!param.is_mut());
    }

    #[test]
    fn entity_and_comp_as_query() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();

        // When
        let spec = <(entity::Entity, &mut Comp1)>::query_data_spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 2);
        let param = params[0];
        assert_eq!(param.param_type(), ParamType::Entity);
        assert!(!param.is_mut());

        let param = params[1];
        assert_eq!(
            param.param_type(),
            ParamType::Component(registry.get::<Comp1>().unwrap())
        );
        assert!(param.is_mut());
    }

    #[test]
    fn entity_and_comps_mixed_as_query() {
        // Given
        let registry = component::Registry::new();
        registry.register::<Comp1>();
        registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::query_data_spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 3);
        let param = params[0];
        assert_eq!(param.param_type(), ParamType::Entity);
        assert!(!param.is_mut());

        let param = params[1];
        assert_eq!(
            param.param_type(),
            ParamType::Component(registry.get::<Comp1>().unwrap())
        );
        assert!(param.is_mut());
        let param = params[2];
        assert_eq!(
            param.param_type(),
            ParamType::Component(registry.get::<Comp2>().unwrap())
        );
        assert!(!param.is_mut());
    }

    #[test]
    fn entity_and_comps_component_spec() {
        // Given
        let registry = component::Registry::new();
        let comp1_id = registry.register::<Comp1>();
        let comp2_id = registry.register::<Comp2>();

        // When
        let spec = <(entity::Entity, &mut Comp1, &Comp2)>::query_data_spec(&registry);

        // Then
        let params = spec.params();
        assert_eq!(params.len(), 3);

        let comp_spec = spec.as_component_spec();
        assert_eq!(comp_spec.ids(), vec![comp1_id, comp2_id]);
    }
}
