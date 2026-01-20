use crate::{
    all_tuples,
    ecs::{
        component::{self, Component},
        storage::{Row, Table},
    },
};

/// A trait describing component values to be applied to an entity (table).
/// The goal of this trait is to allow multiple ways to apply component values.
///
/// Examples of value include: a single component type, a tuple of component types, or something
/// hand created.
pub trait Values: component::IntoSpec + 'static {
    /// Apply the component values in this set to the given target. This takes ownership of self.
    fn apply(self, target: &mut Table, row: Row);
}

/// Implement Set for single component types.
impl<C: Component> Values for C {
    fn apply(self, target: &mut Table, row: Row) {
        target.apply_value::<C>(row, self);
    }
}

impl Values for () {
    fn apply(self, _target: &mut Table, _row: Row) {
        // No components to apply.
    }
}

/// Implement Set for tuples of component types.
macro_rules! tuple_set {
    ($($name: ident),*) => {
        impl<$($name: Values),*> Values for ($($name,)*) {

            /// Apply each component in the tuple to the target.
            fn apply(self,  target: &mut Table, row: Row) {
                 #[allow(non_snake_case)]
                let ( $($name,)* ) = self;
                 #[allow(non_snake_case)]
                $(<$name as Values>::apply($name, target, row);)*
            }
        }
    }
}

// Implement the tuple Set for all tuples up to 26 elements.
all_tuples!(tuple_set);

#[cfg(test)]
mod tests {

    #[cfg(test)]
    use crate::ecs::{storage::table, world};
    use rusty_macros::Component;

    use super::*;

    #[derive(Component, Debug, PartialEq)]
    struct Component1 {
        value: u32,
    }

    #[derive(Component, Debug, PartialEq)]
    struct Component2 {
        value: u32,
    }

    #[derive(Component, Debug, PartialEq)]
    struct Component3 {
        value: u32,
    }

    #[test]
    fn test_single_component_set() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Component1>();

        // When
        let mut table = Table::new(
            table::Id::new(0),
            &[registry.get_info_of::<Component1>().unwrap()],
        );

        let comp = Component1 { value: 42 };
        table.get_column_mut::<Component1>().unwrap().reserve(1);

        // When
        comp.apply(&mut table, 0.into());
        unsafe { table.get_column_mut::<Component1>().unwrap().set_len(1) };

        // Then
        let value = unsafe { table.get::<Component1>(0.into()) };
        assert_eq!(value, Some(&Component1 { value: 42 }));
    }

    #[test]
    fn test_tuple_component_set() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Component1>();
        registry.register_component::<Component2>();
        registry.register_component::<Component3>();

        // When
        let mut table = Table::new(
            table::Id::new(0),
            &[
                registry.get_info_of::<Component1>().unwrap(),
                registry.get_info_of::<Component2>().unwrap(),
                registry.get_info_of::<Component3>().unwrap(),
            ],
        );

        let comp1 = Component1 { value: 42 };
        let comp2 = Component2 { value: 67 };
        let comp3 = Component3 { value: 99 };

        table.get_column_mut::<Component1>().unwrap().reserve(1);
        table.get_column_mut::<Component2>().unwrap().reserve(1);
        table.get_column_mut::<Component3>().unwrap().reserve(1);

        // When
        (comp1, comp2, comp3).apply(&mut table, 0.into());
        unsafe { table.get_column_mut::<Component1>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Component2>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Component3>().unwrap().set_len(1) };

        // Then
        let value = unsafe { table.get::<Component1>(0.into()) };
        assert_eq!(value, Some(&Component1 { value: 42 }));
        let value = unsafe { table.get::<Component2>(0.into()) };
        assert_eq!(value, Some(&Component2 { value: 67 }));
        let value = unsafe { table.get::<Component3>(0.into()) };
        assert_eq!(value, Some(&Component3 { value: 99 }));
    }

    #[test]
    fn test_nested_tuple_component_set() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Component1>();
        registry.register_component::<Component2>();
        registry.register_component::<Component3>();

        // When
        let mut table = Table::new(
            table::Id::new(0),
            &[
                registry.get_info_of::<Component1>().unwrap(),
                registry.get_info_of::<Component2>().unwrap(),
                registry.get_info_of::<Component3>().unwrap(),
            ],
        );

        let comp1 = Component1 { value: 42 };
        let comp2 = Component2 { value: 67 };
        let comp3 = Component3 { value: 99 };

        table.get_column_mut::<Component1>().unwrap().reserve(1);
        table.get_column_mut::<Component2>().unwrap().reserve(1);
        table.get_column_mut::<Component3>().unwrap().reserve(1);

        // When
        (comp1, (comp2, comp3)).apply(&mut table, 0.into());
        unsafe { table.get_column_mut::<Component1>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Component2>().unwrap().set_len(1) };
        unsafe { table.get_column_mut::<Component3>().unwrap().set_len(1) };

        // Then
        let value = unsafe { table.get::<Component1>(0.into()) };
        assert_eq!(value, Some(&Component1 { value: 42 }));
        let value = unsafe { table.get::<Component2>(0.into()) };
        assert_eq!(value, Some(&Component2 { value: 67 }));
        let value = unsafe { table.get::<Component3>(0.into()) };
        assert_eq!(value, Some(&Component3 { value: 99 }));
    }
}
