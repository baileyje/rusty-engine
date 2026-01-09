use std::marker::PhantomData;

use crate::core::ecs::{
    component,
    system::{System, param::Param},
    world,
};

/// Wraps a function to implement the System trait.
pub struct FunctionSystem<F, Params> {
    /// The component spec for this system's parameters.
    spec: component::Spec,

    /// The function to execute.
    func: F,

    /// Optional state storage (for future use).
    _marker: PhantomData<Params>,
}

impl<F, Params> FunctionSystem<F, Params> {
    pub fn new(components: &component::Registry, func: F) -> Self
    where
        F: SystemParamFunction<Params>,
    {
        Self {
            spec: F::component_spec(components),
            func,
            _marker: PhantomData,
        }
    }
}

impl<F, Params> System for FunctionSystem<F, Params>
where
    F: SystemParamFunction<Params> + Send + Sync,
    Params: Send + Sync,
{
    fn component_spec(&self) -> &component::Spec {
        &self.spec
    }

    unsafe fn run(&mut self, world: &mut world::World) {
        unsafe {
            self.func.run(world);
        }
    }
}

/// Helper trait to invoke functions with system parameters.
///
/// This trait is sealed and automatically implemented for valid function types.
pub trait SystemParamFunction<Params>: 'static {
    fn component_spec(components: &component::Registry) -> component::Spec;
    unsafe fn run(&mut self, world: &mut world::World);
}

// Implementations for functions with 0-N parameters
impl<F> SystemParamFunction<()> for F
where
    F: FnMut() + 'static,
{
    fn component_spec(_components: &component::Registry) -> component::Spec {
        component::Spec::new(vec![])
    }

    unsafe fn run(&mut self, _world: &mut world::World) {
        self();
    }
}

impl<F, P1> SystemParamFunction<(P1,)> for F
where
    F: FnMut(P1::Item<'_>) + 'static,
    P1: Param,
{
    fn component_spec(components: &component::Registry) -> component::Spec {
        P1::component_spec(components)
    }

    unsafe fn run(&mut self, world: &mut world::World) {
        let p1 = unsafe { P1::get(world) };
        self(p1);
    }
}

// Macro to implement for tuples of 2-12 parameters
macro_rules! impl_system_param_function {
    ($($param:ident),*) => {
        impl<F, $($param),*> SystemParamFunction<($($param,)*)> for F
        where
            F: FnMut($($param::Item<'_>),*) + 'static,
            $($param: Param),*
        {
            fn component_spec(components: &component::Registry) -> component::Spec {
                let mut spec = component::Spec::new(vec![]);
                $(
                    spec = spec.merge(&$param::component_spec(components));
                )*
                spec
            }

            unsafe fn run(&mut self, world: &mut world::World) {
                $(
                    // SAFETY: Creating aliased mutable world pointers is safe because each
                    // Param::get call accesses different data (validated by scheduler via component specs)
                    #[allow(non_snake_case)]
                    let $param = unsafe { $param::get(&mut *(world as *mut world::World)) };
                )*
                self($($param),*);
            }
        }
    };
}
// impl_system_param_function!(P1);
impl_system_param_function!(P1, P2);
impl_system_param_function!(P1, P2, P3);
impl_system_param_function!(P1, P2, P3, P4);
impl_system_param_function!(P1, P2, P3, P4, P5);
impl_system_param_function!(P1, P2, P3, P4, P5, P6);

#[cfg(test)]
mod tests {
    use super::super::System;
    use super::*;
    use crate::core::ecs::{component, world};

    use rusty_macros::Component;

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }

    #[test]
    fn no_param_function_system() {
        fn my_system() {
            // No-op
        }

        let components = component::Registry::new();
        let mut system = FunctionSystem::new(&components, my_system);

        let mut world = world::World::new(world::Id::new(0));
        unsafe {
            system.run(&mut world);
        }
    }

    #[test]
    fn world_param_function_system() {
        fn my_system(world: &mut world::World) {
            // Verify we can access the world
            assert_eq!(world.id(), world::Id::new(0));
        }

        let components = component::Registry::new();
        let mut system: FunctionSystem<_, (&mut world::World,)> =
            FunctionSystem::new(&components, my_system);

        let mut world = world::World::new(world::Id::new(0));
        unsafe {
            system.run(&mut world);
        }
    }

    // #[test]
    // fn single_query_handle_function_system() {
    //     fn my_system<'a>(query: QueryParam<'a, &'a Comp1>) {
    //         let mut count = 0;
    //
    //         assert_eq!(count, 1);
    //     }
    //
    //     let mut world = world::World::new(world::Id::new(0));
    //     world.spawn(Comp1 { value: 42 });
    //
    //     let components = world.components();
    //     let mut system: FunctionSystem<_, (QueryHandle<&Comp1>,)> =
    //         FunctionSystem::new(components, my_system);
    //
    //     unsafe {
    //         system.run(&mut world);
    //     }
    // }
    //
    // #[test]
    // fn query_handle_multiple_entities() {
    //     fn count_system(query: QueryHandle<(&Comp1, &Comp2)>) {
    //         let mut count = 0;
    //         query.for_each(|(_c1, _c2)| {
    //             count += 1;
    //         });
    //         assert_eq!(count, 2);
    //     }
    //
    //     let mut world = world::World::new(world::Id::new(0));
    //
    //     // Spawn entities
    //     world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
    //     world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
    //     world.spawn(Comp1 { value: 3 }); // Only Comp1, won't match query
    //
    //     let components = world.components();
    //     let mut system: FunctionSystem<_, (QueryHandle<(&Comp1, &Comp2)>,)> =
    //         FunctionSystem::new(components, count_system);
    //
    //     unsafe {
    //         system.run(&mut world);
    //     }
    // }
    //
    // #[test]
    // fn multiple_query_handles_function_system() {
    //     fn two_query_system(query1: QueryHandle<&Comp1>, query2: QueryHandle<&Comp2>) {
    //         // Count Comp1 entities
    //         let count1 = query1.count();
    //         assert_eq!(count1, 3);
    //
    //         // Count Comp2 entities
    //         let count2 = query2.count();
    //         assert_eq!(count2, 2);
    //     }
    //
    //     let mut world = world::World::new(world::Id::new(0));
    //
    //     world.spawn((Comp1 { value: 1 }, Comp2 { value: 10 }));
    //     world.spawn((Comp1 { value: 2 }, Comp2 { value: 20 }));
    //     world.spawn(Comp1 { value: 3 }); // Only Comp1
    //
    //     let components = world.components();
    //     let mut system: FunctionSystem<_, (QueryHandle<&Comp1>, QueryHandle<&Comp2>)> =
    //         FunctionSystem::new(components, two_query_system);
    //
    //     unsafe {
    //         system.run(&mut world);
    //     }
    // }
    //
    // #[test]
    // fn query_handle_with_mutable_components() {
    //     fn increment_system(mut query: QueryHandle<&mut Comp1>) {
    //         query.for_each_mut(|comp| {
    //             comp.value += 1;
    //         });
    //     }
    //
    //     let mut world = world::World::new(world::Id::new(0));
    //     world.spawn(Comp1 { value: 5 });
    //     world.spawn(Comp1 { value: 10 });
    //
    //     let components = world.components();
    //     let mut system: FunctionSystem<_, (QueryHandle<&mut Comp1>,)> =
    //         FunctionSystem::new(components, increment_system);
    //
    //     // Run the system
    //     unsafe {
    //         system.run(&mut world);
    //     }
    //
    //     // Verify the values were incremented
    //     use crate::core::ecs::query::Query;
    //     let mut query = Query::<&Comp1>::new(world.components()).invoke(&mut world);
    //
    //     let values: Vec<i32> = query.map(|c| c.value).collect();
    //     assert!(values.contains(&6));
    //     assert!(values.contains(&11));
    // }
}
