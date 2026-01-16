use super::Parameter;
use crate::ecs::{query, world};

/// System parameter for a world Query.
///
/// This allows systems to declare queries they want to run on the world, enabling
/// iteration over entities that match specific component criteria.
///
/// # Type Parameter
///
/// `D` defines the data being queried and implements [`query::Data`]. Common patterns:
///
/// - **Single components**: `&Component`, `&mut Component`
/// - **Tuples**: `(&Component1, &mut Component2)` for multiple components
/// - **Entity IDs**: `entity::Entity` to get entity handles
/// - **Optional**: `Option<&Component>` for components that may not exist
/// - **Mixed**: `(entity::Entity, &Comp1, Option<&mut Comp2>)`
///
/// # Examples
///
/// ```rust,ignore
/// // Read-only query
/// fn print_positions(query: Query<&Position>) {
///     for pos in query {
///         println!("({}, {})", pos.x, pos.y);
///     }
/// }
///
/// // Mutable query
/// fn apply_gravity(query: Query<&mut Velocity>) {
///     for vel in query {
///         vel.dy -= 9.8;
///     }
/// }
///
/// // Mixed access
/// fn movement(query: Query<(&Velocity, &mut Position)>) {
///     for (vel, pos) in query {
///         pos.x += vel.dx;
///     }
/// }
///
/// // With optional components
/// //
/// // Optional components in queries allow matching entities with required components
/// // that may also have additional components. The archetype storage doesn't store
/// // optionals - this is resolved at query time by matching multiple archetypes.
/// fn heal(query: Query<(&Player, Option<&mut Health>)>) {
///     for (player, health) in query {
///         if let Some(h) = health {
///             h.current += 1;
///         }
///     }
/// }
/// ```
pub struct Query<'w, D: query::Data> {
    inner: query::Result<'w, D>,
}

impl<'w, D: query::Data> Query<'w, D> {
    /// Create a new query parameter from the inner query result.
    pub fn new(inner: query::Result<'w, D>) -> Self {
        Self { inner }
    }
}

/// Implementation of iterator for world Query.
///
/// Delegates to the inner [`query::Result`] iterator.
impl<'w, D: query::Data> Iterator for Query<'w, D> {
    type Item = D::Data<'w>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'w, D: query::Data> ExactSizeIterator for Query<'w, D> {}

/// Implementation of [`Parameter`] for world Query .
///
/// # Implementation Details
///
/// - **Value type**: `Query<'w, D>` where `'w` is the world lifetime
/// - **Access request**: Delegates to `D::spec()` which analyzes the query type
/// - **Extraction**: Calls `world.query::<D>()` to create the iterator
///
/// The lifetime `'_` in `Query<'_, D>` is elided in function signatures,
/// and becomes `'w` (world lifetime) at runtime via [`Value`](Parameter::Value).
///
///
/// See [`query`](crate::ecs::query) module for details on query composition.
impl<D: query::Data + 'static> Parameter for Query<'_, D> {
    /// The value type is the query result with world lifetime.
    type Value<'w, 's> = Query<'w, D>;

    /// The state type is the query instance for this data.
    type State = query::Query<D>;

    /// Build the query state for this parameter.
    fn build_state(world: &mut world::World) -> Self::State {
        query::Query::new(world.resources())
    }

    /// Get the world access request for this query parameter.
    /// This delegates to the underlying Data type to determine the required components.
    fn required_access(world: &world::World) -> world::AccessRequest {
        D::spec(world.resources()).as_access_request()
    }

    /// Get the query results by executing a query for the specific data against the shard.
    ///
    /// The shard provides safe access to components and storage according to its grant,
    /// ensuring that the query only accesses data permitted by the system's access request.
    ///
    /// Safety: Caller ensures disjoint access via access requests
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        //Safety: Caller ensures disjoint access via access requests - The query invoke
        // method is safe, but its unclear it should be since the table access is only checked at
        // debug. That being said, the world manages the grants for shards so its probably ok.
        Query::new(state.invoke(shard))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::ecs::{component, entity, world};
    use rusty_macros::Component;

    #[derive(Component)]
    struct Comp1 {
        value: i32,
    }

    #[derive(Component)]
    struct Comp2 {
        value: i32,
    }

    fn test_setup() -> (
        world::World,
        (entity::Entity, entity::Entity, entity::Entity),
    ) {
        let mut world = world::World::new(world::Id::new(0));
        component::Spec::new(vec![
            world.resources().register_component::<Comp1>(),
            world.resources().register_component::<Comp2>(),
        ]);
        let entities = (
            world.spawn(Comp1 { value: 10 }),
            world.spawn(Comp2 { value: 20 }),
            world.spawn((Comp1 { value: 30 }, Comp2 { value: 40 })),
        );
        (world, entities)
    }

    #[test]
    fn comp_param_component_access() {
        // Given
        let (world, _) = test_setup();

        // When
        let access = <Query<(&Comp1, &Comp2)>>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(
                &[
                    world.resources().get::<Comp1>().unwrap(),
                    world.resources().get::<Comp2>().unwrap()
                ],
                &[],
            )
        )
    }

    #[test]
    fn comp_param_component_get() {
        // Given
        let (mut world, _) = test_setup();
        let mut state = <Query<&Comp1>>::build_state(&mut world);
        let access = <Query<&Comp1>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe { <Query<&Comp1>>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.len(), 2);
        let row = result.next().unwrap();
        assert_eq!(row.value, 10);
        let row = result.next().unwrap();
        assert_eq!(row.value, 30);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn comp_param_component_get_mut() {
        // Given
        let (mut world, _) = test_setup();
        let mut state = <Query<&mut Comp1>>::build_state(&mut world);
        let access = <Query<&mut Comp1>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe { <Query<&mut Comp1>>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.len(), 2);
        let comp = result.next().unwrap();
        comp.value += 1;
        let comp = result.next().unwrap();
        comp.value += 2;

        // Release shard
        world.release_shard(shard);

        // And When
        let mut state = <Query<&Comp1>>::build_state(&mut world);
        let access = <Query<&Comp1>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");
        let mut result = unsafe { <Query<&Comp1>>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.len(), 2);
        let comp = result.next().unwrap();
        assert_eq!(comp.value, 11);
        let comp = result.next().unwrap();
        assert_eq!(comp.value, 32);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn comp_param_entity_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let access = <Query<entity::Entity>>::required_access(&world);

        // Then
        assert!(access.is_none());
    }

    #[test]
    fn comp_param_entity_get() {
        // Given
        let (mut world, (entity1, entity2, entity3)) = test_setup();
        let mut state = <Query<entity::Entity>>::build_state(&mut world);
        let access = <Query<entity::Entity>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe { <Query<entity::Entity>>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.len(), 3);

        let entity = result.next().unwrap();
        assert_eq!(entity, entity1);
        let entity = result.next().unwrap();
        assert_eq!(entity, entity2);
        let entity = result.next().unwrap();
        assert_eq!(entity, entity3);

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn comp_param_entity_comps_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let access = <Query<(entity::Entity, &Comp1, &mut Comp2)>>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(
                &[world.resources().get::<Comp1>().unwrap()],
                &[world.resources().get::<Comp2>().unwrap()],
            )
        )
    }

    #[test]
    fn comp_param_entity_comps_get() {
        // Given
        let (mut world, (entity1, _, entity3)) = test_setup();
        let mut state =
            <Query<(entity::Entity, &Comp1, Option<&mut Comp2>)>>::build_state(&mut world);
        let access = <Query<(entity::Entity, &Comp1, Option<&mut Comp2>)>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe {
            <Query<(entity::Entity, &Comp1, Option<&mut Comp2>)>>::get(&mut shard, &mut state)
        };

        // Then
        assert_eq!(result.len(), 2);
        let (entity, comp1, comp2) = result.next().unwrap();
        assert_eq!(entity, entity1);
        assert_eq!(comp1.value, 10);
        assert!(comp2.is_none());

        let (entity, comp1, comp2) = result.next().unwrap();
        assert_eq!(entity, entity3);
        assert_eq!(comp1.value, 30);
        assert!(comp2.is_some());
        assert_eq!(comp2.unwrap().value, 40);

        // Release shard
        world.release_shard(shard);
    }
}
