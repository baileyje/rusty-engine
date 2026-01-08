use std::marker::PhantomData;

use crate::core::ecs::{
    component,
    query::{
        data::{Data, DataSpec},
        result::Result,
    },
    world,
};

mod data;
mod param;
mod result;

pub struct Query<'w, D: Data<'w>> {
    data_spec: DataSpec,

    phantom: PhantomData<&'w D>,
}

impl<'w, D: Data<'w>> Query<'w, D> {
    #[inline]
    pub fn new(components: &component::Registry) -> Self {
        Self {
            data_spec: D::query_data_spec(components),
            phantom: PhantomData,
        }
    }

    pub fn invoke(&self, world: &'w mut world::World) -> Result<'w, D> {
        // Create a query spec from the `D` type.
        println!("Got data spec: {:?}", self.data_spec);

        // Create a component spec for the query.
        let comp_spec = self.data_spec.as_component_spec();
        println!("Got comp spec: {:?}", comp_spec);

        // Get the archetype ids for that support this component spec.
        let table_ids = world.storage().supporting(&comp_spec);
        println!("Got archetypes: {:?}", table_ids);

        Result::new(world, self.data_spec.clone(), table_ids)
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::core::ecs::{query::Query, world};

    #[derive(Component)]
    struct Comp1;

    #[derive(Component)]
    struct Comp2;

    #[derive(Component)]
    struct Comp3;

    fn make_world() -> world::World {
        let mut world = world::World::new(world::Id::new(0));

        world.spawn((Comp1, Comp2));
        world.spawn(Comp1);
        world.spawn((Comp1, Comp2));
        world.spawn((Comp1, Comp2, Comp3));
        world.spawn(Comp2);
        world.spawn(Comp3);
        world.spawn((Comp1, Comp3));
        world.spawn((Comp3, Comp2, Comp1));
        world.spawn((Comp2, Comp3));
        world
    }

    #[test]
    fn invoke_empty_query() {
        let mut world = make_world();
        let mut result = Query::<()>::new(world.components()).invoke(&mut world);

        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_simple_query() {
        let mut world = make_world();
        let mut result = Query::<&Comp1>::new(world.components()).invoke(&mut world);

        println!("Result Len: {:?}", result.len());

        assert_eq!(result.len(), 6);

        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }

    #[test]
    fn invoke_mix_mut_query() {
        let mut world = make_world();
        let mut result = Query::<(&Comp1, &mut Comp2)>::new(world.components()).invoke(&mut world);

        println!("Result Len: {:?}", result.len());

        assert_eq!(result.len(), 4);

        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_some());
        assert!(result.next().is_none());
    }
}
