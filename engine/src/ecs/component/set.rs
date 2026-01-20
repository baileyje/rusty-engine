use crate::{
    all_tuples,
    ecs::{
        component::{Component, Spec},
        storage, world,
    },
};

/// Trait for component sets in an ECS (Entity Component System).
pub trait Set: 'static + Sized {
    fn as_spec(&self, types: &world::TypeRegistry) -> Spec;

    fn apply(self, target: &mut storage::Table, row: storage::Row);

    fn with<S2: Set>(self, other: S2) -> CompositeSet<Self, S2> {
        CompositeSet {
            first: self,
            second: other,
        }
    }
}

/// [`Set`] implementation for the empty tuple.
impl Set for () {
    /// Convert the empty tuple into an empty Spec.
    fn as_spec(&self, _registry: &world::TypeRegistry) -> Spec {
        Spec::EMPTY
    }

    fn apply(self, _target: &mut storage::Table, _row: storage::Row) {
        // No components to apply
    }
}

/// [`Set`] implementation for single component types.
impl<C: Component> Set for C {
    fn as_spec(&self, registry: &world::TypeRegistry) -> Spec {
        Spec::new([registry.register_component::<C>()])
    }

    fn apply(self, target: &mut storage::Table, row: storage::Row) {
        target.apply_column_write(row, self);
    }
}

/// [`Set`] implementation for tuples of component types.
macro_rules! tuple_set {
    ($($name: ident),*) => {
        impl<$($name: Set),*> Set for ($($name,)*) {
            fn as_spec(&self, registry: &world::TypeRegistry) -> Spec {
                let mut ids = Vec::new();
                #[allow(non_snake_case)]
                let ( $($name,)* ) = self;
                $(
                    ids.extend($name.as_spec(registry).ids());
                )*
                Spec::new(ids)
            }

           /// Apply each component in the tuple to the target.
            fn apply(self,  target: &mut storage::Table, row: storage::Row) {
                 #[allow(non_snake_case)]
                let ( $($name,)* ) = self;
                 #[allow(non_snake_case)]
                $(<$name as Set>::apply($name, target, row);)*

            }
        }
    }
}

pub struct CompositeSet<S1: Set, S2: Set> {
    first: S1,
    second: S2,
}

impl<S1: Set, S2: Set> Set for CompositeSet<S1, S2> {
    fn as_spec(&self, registry: &world::TypeRegistry) -> Spec {
        let mut ids = Vec::new();
        ids.extend(self.first.as_spec(registry).ids());
        ids.extend(self.second.as_spec(registry).ids());
        Spec::new(ids)
    }

    fn apply(self, target: &mut storage::Table, row: storage::Row) {
        self.first.apply(target, row);
        self.second.apply(target, row);
    }
}

type ApplyFn = Box<dyn FnOnce(&mut storage::Table, storage::Row) + Send>;

/// Type-erased container for deferred component values.
///
/// This wraps any `S: Set` by capturing:
/// 1. The component `Spec` for archetype lookup
/// 2. A boxed closure that owns the values and applies them to a table
///
/// Used by the command buffer system for deferred entity spawning and
/// component modifications.
pub struct BoxedSet {
    /// Pre-computed spec for archetype/table lookup
    spec: Spec,
    /// Captured apply function that owns the component data
    apply_fn: ApplyFn,
}

impl BoxedSet {
    /// Create a BoxedSet from any type implementing `Set`.
    ///
    /// The values are moved into a closure that will apply them later.
    pub fn new<S: Set + Send>(values: S, registry: &world::TypeRegistry) -> Self {
        // Compute spec at creation time (needed for archetype lookup at flush)
        let spec = values.as_spec(registry);

        // Capture values in a closure - this moves ownership into the box
        let apply_fn = Box::new(move |table: &mut storage::Table, row: storage::Row| {
            values.apply(table, row);
        });

        Self { spec, apply_fn }
    }
}

impl Set for BoxedSet {
    /// Get the component specification (for archetype lookup).
    fn as_spec(&self, _types: &world::TypeRegistry) -> Spec {
        self.spec.clone()
    }

    /// Apply the stored values to a table row (consumes self).
    fn apply(self, table: &mut storage::Table, row: storage::Row) {
        (self.apply_fn)(table, row);
    }
}

// Implement the tuple -> Spec for all tuples up to 26 elements.
all_tuples!(tuple_set);

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::ecs::{component::Spec, entity, world};

    use super::*;

    // Given
    #[derive(Component, Debug, PartialEq, Eq)]
    pub struct Comp1;
    #[derive(Component, Debug, PartialEq, Eq)]
    pub struct Comp2;
    #[derive(Component, Debug, PartialEq, Eq)]
    pub struct Comp3;

    #[test]
    fn component_set_as_spec() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();

        let comp = Comp1;

        // When
        let spec = comp.as_spec(&registry);

        // Then
        assert_eq!(spec, Spec::new([id1]));
    }

    #[test]
    fn component_set_apply() {
        // Given
        let registry = world::TypeRegistry::new();
        let id = registry.register_component::<Comp1>();

        let mut table =
            storage::Table::new(storage::TableId::new(0), &[registry.get_info(id).unwrap()]);

        let comp = Comp1;

        // When
        let row = table.add_entity(entity::Entity::new(0), comp);

        // Then
        let value = unsafe { table.get::<Comp1>(row) };
        assert!(value.is_some());
    }

    #[test]
    fn components_set_as_spec() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let tuple = (Comp1, Comp2, Comp3);

        // When
        let spec = tuple.as_spec(&registry);

        // Then
        assert_eq!(spec, Spec::new([id1, id2, id3]));
    }

    #[test]
    fn components_set_apply() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let mut table = storage::Table::new(
            storage::TableId::new(0),
            &[
                registry.get_info(id1).unwrap(),
                registry.get_info(id2).unwrap(),
                registry.get_info(id3).unwrap(),
            ],
        );

        let tuple = (Comp1, Comp2, Comp3);

        // When
        let row = table.add_entity(entity::Entity::new(0), tuple);

        // Then
        let value = unsafe { table.get::<Comp1>(row) };
        assert!(value.is_some());
        let value = unsafe { table.get::<Comp2>(row) };
        assert!(value.is_some());
        let value = unsafe { table.get::<Comp3>(row) };
        assert!(value.is_some());
    }

    #[test]
    fn composite_set_as_spec() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let tuple = (Comp1, Comp2);
        let comp = Comp3;

        let set = tuple.with(comp);

        // When
        let spec = set.as_spec(&registry);

        // Then
        assert_eq!(spec, Spec::new([id1, id2, id3]));
    }

    #[test]
    fn composite_set_apply() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let mut table = storage::Table::new(
            storage::TableId::new(0),
            &[
                registry.get_info(id1).unwrap(),
                registry.get_info(id2).unwrap(),
                registry.get_info(id3).unwrap(),
            ],
        );

        let tuple = (Comp1, Comp2);
        let comp = Comp3;

        let set = tuple.with(comp);

        // When
        let row = table.add_entity(entity::Entity::new(0), set);

        // Then
        let value = unsafe { table.get::<Comp1>(row) };
        assert!(value.is_some());
        let value = unsafe { table.get::<Comp2>(row) };
        assert!(value.is_some());
        let value = unsafe { table.get::<Comp3>(row) };
        assert!(value.is_some());
    }

    #[test]
    fn test_nested_tuple_component_set() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        // When
        let mut table = storage::Table::new(
            storage::TableId::new(0),
            &[
                registry.get_info(id1).unwrap(),
                registry.get_info(id2).unwrap(),
                registry.get_info(id3).unwrap(),
            ],
        );

        let comp1 = Comp1;
        let comp2 = Comp2;
        let comp3 = Comp3;

        table.get_column_mut::<Comp1>().unwrap().reserve(1);
        table.get_column_mut::<Comp2>().unwrap().reserve(1);
        table.get_column_mut::<Comp3>().unwrap().reserve(1);

        // When
        (comp1, (comp2, comp3)).apply(&mut table, 0.into());
        unsafe { table.get_column_mut::<Comp1>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Comp2>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Comp3>().unwrap().set_len(1) };

        // Then
        let value = unsafe { table.get::<Comp1>(0.into()) };
        assert_eq!(value, Some(&Comp1));
        let value = unsafe { table.get::<Comp2>(0.into()) };
        assert_eq!(value, Some(&Comp2));
        let value = unsafe { table.get::<Comp3>(0.into()) };
        assert_eq!(value, Some(&Comp3));
    }
}
