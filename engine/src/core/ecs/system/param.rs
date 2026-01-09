use crate::core::ecs::{component, world};

/// A type that can be passed as a parameter to a system function.
///
/// This trait is implemented by types that can be extracted from the world,
/// such as queries, resources, or other world state.
///
/// # Implementations
///
/// - `Query<D>` - For querying entities and components
/// - Future: `Res<T>`, `ResMut<T>` for resource access
/// - Future: `Commands` for deferred operations
pub trait Param: Sized {
    /// The type that this parameter extracts from the world.
    /// Uses a GAT to handle lifetime properly.
    type Item<'w>;

    /// Get the component specification for this parameter.
    /// Used by the scheduler to determine system dependencies and detect aliasing.
    fn component_spec(components: &component::Registry) -> component::Spec;

    /// Extract this parameter from the world.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// - No aliasing violations occur (scheduler responsibility)
    /// - The world reference is valid for the lifetime 'w
    unsafe fn get<'w>(world: &'w mut world::World) -> Self::Item<'w>;
}

// // Marker type to hold query data specification
// pub struct QueryParam<'w, D>(std::marker::PhantomData<D>);
//
// impl<'w, D: query::Data<'w>> Param for QueryParam<'w, D> {
//     type Item = query::Result<'w, D>;
//
//     fn component_spec(components: &component::Registry) -> component::Spec {
//         // We can use any lifetime here since the spec doesn't depend on it
//         let data_spec = D::spec(components);
//         data_spec.as_component_spec()
//     }
//
//     unsafe fn get(world: &'w mut world::World) -> Self::Item<'w> {
//         query::Query::<D>::one_shot(world)
//     }
// }
//
// impl<D> Param for QueryHandle<'_, D>
// where
//     for<'w> D: query::Data<'w>,
// {
//     type Item<'w> = QueryHandle<'w, D>;
//
//     fn component_spec(components: &component::Registry) -> component::Spec {
//         let data_spec = D::spec(components);
//         data_spec.as_component_spec()
//     }
//
//     unsafe fn get<'w>(world: &'w mut world::World) -> Self::Item<'w> {
//         let spec = D::spec(world.components());
//         QueryHandle::new(world, spec)
//     }
// }

impl Param for &mut world::World {
    type Item<'w> = &'w mut world::World;

    fn component_spec(_components: &component::Registry) -> component::Spec {
        // TODO: Require All Components.....
        component::Spec::new(vec![])
    }

    unsafe fn get<'w>(world: &'w mut world::World) -> Self::Item<'w> {
        world
    }
}

#[cfg(test)]
mod tests {

    use crate::core::ecs::{component, system::Param, world};
    use rusty_macros::Component;

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }

    fn test_setup() -> world::World {
        let world = world::World::new(world::Id::new(0));
        component::Spec::new(vec![
            world.components().register::<Comp1>(),
            world.components().register::<Comp2>(),
        ]);
        world
    }

    #[test]
    fn world_param_component_spec() {
        let world = test_setup();

        let spec = <&mut world::World as Param>::component_spec(world.components());

        // World param should have empty component spec (or all components in future)
        assert_eq!(spec.ids().len(), 0);
    }

    #[test]
    fn world_param_get() {
        let mut world = test_setup();

        unsafe {
            let world_ref = <&mut world::World as Param>::get(&mut world);

            // Should get back a valid world reference
            assert_eq!(world_ref.id(), world.id());
        }
    }

    // Note: Query as Param is tested via integration tests in function.rs
    // Direct testing here causes lifetime issues with the higher-ranked trait bounds
}
