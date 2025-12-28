use std::slice;

use crate::core::ecs::component::{Component, Id, Registry, Spec};

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
pub trait Set: Sized + 'static {
    /// Get the component specification for this set.
    fn spec(registry: &mut Registry) -> Spec;

    /// Apply the component values in this set to the given target. This takes ownership of self.
    fn apply<T: Target>(self, registry: &mut Registry, target: &mut T);
}

/// Implement Set for single component types.
impl<C: Component> Set for C {
    /// Get the component specification for this set. Always a single component.
    fn spec(registry: &mut Registry) -> Spec {
        let id = registry.register::<C>();
        Spec::new(slice::from_ref(&id))
    }

    fn apply<T: Target>(self, registry: &mut Registry, target: &mut T) {
        target.apply::<C>(registry.register::<C>(), self);
    }
}

impl Set for () {
    fn spec(_registry: &mut Registry) -> Spec {
        Spec::new(Vec::new())
    }

    fn apply<T: Target>(self, _registry: &mut Registry, _target: &mut T) {
        // No components to apply.
    }
}

/// Implement Set for tuples of component types.
macro_rules! tuple_set_impl {
    ($(($name: ident, $alias: ident)),*) => {
        impl<$($name: Set),*> Set for ($($name,)*) {
            fn spec(registry: &mut Registry) -> Spec {
                let mut ids = Vec::new();
                $(ids.extend(<$name as Set>::spec(registry).ids());)*
                Spec::new(ids)
            }

            fn apply<CT: Target>(self, registry: &mut Registry, target: &mut CT) {
                let ( $($alias,)* ) = self;
                $(<$name as Set>::apply($alias, registry, target);)*
            }

        }
    }
}

/// Implement Set for tuples of component types recursively.
macro_rules! tuple_set {
    (($head_ty:ident, $head_alias: ident)) => {
        tuple_set_impl!(($head_ty, $head_alias));
    };
    (($head_ty:ident, $head_alias: ident), $( ($tail_ty:ident, $tail_alias: ident) ),*) => (
        tuple_set_impl!(($head_ty, $head_alias), $(( $tail_ty, $tail_alias) ),*);
        tuple_set!($( ($tail_ty, $tail_alias) ),*);
    );
}

// This can't be the best way to do this, but it works for now.
tuple_set! {
    (A, a), (B, b), (C, c), (D, d), (E, e), (F, f),
    (G, g), (H, h), (I, i), (J, j), (K, k), (L, l),
    (M, m), (N, n), (O, o), (P, p), (Q, q), (R, r),
    (S, s), (T, t), (U, u), (V, v), (W, w), (X, x),
    (Y, y), (Z, z)
}

#[cfg(test)]
mod tests {

    use std::any::Any;

    #[cfg(test)]
    use crate::core::ecs::component::Id;

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

        (S::spec(registry), target.ids, target.vals)
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
