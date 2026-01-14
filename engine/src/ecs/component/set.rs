use crate::{
    all_tuples,
    ecs::component::{Component, Id, IntoSpec, Registry},
};

/// Trait describing a target that can have component values applied to it from a `Set`.
pub trait Target {
    fn apply<C: Component>(&mut self, id: Id, value: C);
}

/// A trait describing a set of component values owned by an entity.
/// The goal of this trait is to allow multiple ways to apply component values for an entity while
/// providing a way to iterate over the component types and values in a type-erased manner.
///
/// Examples of sets include: a single component type, a tuple of component types, or something
/// hand created.
pub trait Set: IntoSpec + Sized + 'static {
    /// Apply the component values in this set to the given target. This takes ownership of self.
    fn apply<T: Target>(self, registry: &Registry, target: &mut T);
}

/// Implement Set for single component types.
impl<C: Component> Set for C {
    fn apply<T: Target>(self, registry: &Registry, target: &mut T) {
        target.apply::<C>(registry.register::<C>(), self);
    }
}

impl Set for () {
    fn apply<T: Target>(self, _registry: &Registry, _target: &mut T) {
        // No components to apply.
    }
}

/// Implement Set for tuples of component types.
macro_rules! tuple_set {
    ($($name: ident),*) => {
        impl<$($name: Set),*> Set for ($($name,)*) {

            /// Apply each component in the tuple to the target.
            fn apply<CT: Target>(self, registry: &Registry, target: &mut CT) {
                 #[allow(non_snake_case)]
                let ( $($name,)* ) = self;
                 #[allow(non_snake_case)]
                $(<$name as Set>::apply($name, registry, target);)*
            }
        }
    }
}

// Implement the tuple Set for all tuples up to 26 elements.
all_tuples!(tuple_set);

#[cfg(test)]
mod tests {

    use std::any::Any;

    #[cfg(test)]
    use crate::ecs::component::Id;
    use crate::ecs::component::Spec;

    use super::*;

    struct MockTarget {
        ids: Vec<Id>,
        vals: Vec<Box<dyn Any>>,
    }

    impl Target for MockTarget {
        fn apply<C: Component>(&mut self, id: Id, value: C) {
            self.ids.push(id);
            self.vals.push(Box::new(value));
        }
    }

    fn test_set<S: Set>(set: S, registry: &mut Registry) -> (Spec, Vec<Id>, Vec<Box<dyn Any>>) {
        let mut target = MockTarget {
            ids: Vec::new(),
            vals: Vec::new(),
        };

        set.apply(registry, &mut target);

        (registry.spec::<S>(), target.ids, target.vals)
    }

    #[test]
    fn test_single_component_set() {
        // Given
        #[derive(Component, Debug, PartialEq)]
        struct TestComponent {
            value: u32,
        }

        let mut registry = Registry::new();

        let comp = TestComponent { value: 42 };

        // When
        let (spec, ids, vals) = test_set(comp, &mut registry);

        // Then
        assert_eq!(spec.ids(), &[Id(0)]);

        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], Id(0));

        assert_eq!(vals.len(), 1);
        assert_eq!(
            vals[0].downcast_ref::<TestComponent>(),
            Some(&TestComponent { value: 42 })
        );
    }

    #[test]
    fn test_tuple_component_set() {
        // Given
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

        let mut registry = Registry::new();

        let comp1 = Component1 { value: 42 };
        let comp2 = Component2 { value: 67 };
        let comp3 = Component3 { value: 99 };

        // When
        let (spec, ids, vals) = test_set((comp1, comp2, comp3), &mut registry);

        // Then
        assert_eq!(spec.ids(), &[Id(0), Id(1), Id(2)]);

        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], Id(0));
        assert_eq!(ids[1], Id(1));
        assert_eq!(ids[2], Id(2));

        assert_eq!(vals.len(), 3);
        assert_eq!(
            vals[0].downcast_ref::<Component1>(),
            Some(&Component1 { value: 42 })
        );
        assert_eq!(
            vals[1].downcast_ref::<Component2>(),
            Some(&Component2 { value: 67 })
        );
        assert_eq!(
            vals[2].downcast_ref::<Component3>(),
            Some(&Component3 { value: 99 })
        );
    }

    #[test]
    fn test_nested_tuple_component_set() {
        // Given
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

        let mut registry = Registry::new();

        let comp1 = Component1 { value: 42 };
        let comp2 = Component2 { value: 67 };
        let comp3 = Component3 { value: 99 };

        // When
        let (spec, ids, vals) = test_set((comp1, (comp2, comp3)), &mut registry);

        // Then
        assert_eq!(spec.ids(), &[Id(0), Id(1), Id(2)]);

        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], Id(0));
        assert_eq!(ids[1], Id(1));
        assert_eq!(ids[2], Id(2));

        assert_eq!(vals.len(), 3);
        assert_eq!(
            vals[0].downcast_ref::<Component1>(),
            Some(&Component1 { value: 42 })
        );
        assert_eq!(
            vals[1].downcast_ref::<Component2>(),
            Some(&Component2 { value: 67 })
        );
        assert_eq!(
            vals[2].downcast_ref::<Component3>(),
            Some(&Component3 { value: 99 })
        );
    }
}
