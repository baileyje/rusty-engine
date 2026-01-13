//! System parameter extraction using Generic Associated Types.
//!
//! This module defines the [`Parameter`] trait, which enables clean system function signatures
//! without explicit lifetime parameters.

use crate::ecs::{component, query, world};

/// A type that can be passed as a parameter to a system function.
///
/// The `Parameter` trait enables extracting data from the world with clean function signatures.
/// The [`Value`](Parameter::Value) Generic Associated Type (GAT), carries the world's lifetime
/// without appearing in the function signature.
///
/// # How It Works
///
/// When you write a system function like:
///
/// ```rust,ignore
/// fn my_system(query: query::Result<&Position>) {
///     // ...
/// }
/// ```
///
/// The type `query::Result<&Position>` is actually `query::Result<'_, &Position>` with an
/// elided lifetime. This is the **parameter type** (no concrete lifetime).
///
/// At runtime, [`Parameter::get()`] returns `query::Result<'w, &Position>` where `'w` is
/// the world's lifetime. This is the **value type** (concrete lifetime).
///
/// The [`function::WithSystemParams`](super::function::WithSystemParams) trait bridges these
/// using a Higher-Ranked Trait Bound:
///
/// ```rust,ignore
/// for<'a> &'a mut Func: FnMut(Param) + FnMut(Param::Value<'a, '_>)
/// ```
///
/// This says: "The function must accept both the parameter type AND its value form with any lifetime."
/// The `'a` is the world lifetime, while the second `'_` is the state lifetime (typically not used
/// by the function, as state is only accessed during parameter extraction).
///
/// # Implementations
///
/// ## Query Results
///
/// ```rust,ignore
/// impl<D: query::Data> Parameter for query::Result<'_, D> {
///     type Value<'w, 's> = query::Result<'w, D>;
///     type State = query::Query<D>;
///
///     fn build_state(world: &mut World) -> Self::State {
///         query::Query::new(world.components())
///     }
///
///     unsafe fn get<'w, 's>(world: &'w mut World, state: &'s mut Self::State) -> Self::Value<'w, 's> {
///         state.invoke(world)
///     }
/// }
/// ```
///
/// **Usage:**
/// ```rust,ignore
/// fn movement(query: query::Result<(&Velocity, &mut Position)>) {
///     for (vel, pos) in query {
///         pos.x += vel.dx;
///     }
/// }
/// ```
///
/// ## World Access
///
/// ```rust,ignore
/// impl Parameter for &mut world::World {
///     type Value<'w, 's> = &'w mut World;
///     type State = ();
///
///     fn build_state(_world: &mut World) -> Self::State {}
///
///     unsafe fn get<'w, 's>(world: &'w mut World, _state: &'s mut Self::State) -> Self::Value<'w, 's> {
///         world
///     }
/// }
/// ```
///
/// **Usage:**
/// ```rust,ignore
/// fn spawner(enemies: query::Result<&Enemy>, world: &mut World) {
///     if enemies.len() < 10 {
///         world.spawn(Enemy::new());
///     }
/// }
/// ```
///
/// # Future Extensions
///
/// Additional parameter types planned:
/// - **Resources**: `Res<T>`, `ResMut<T>` for global state
/// - **Commands**: Deferred entity spawning/despawning
/// - **Events**: Event readers/writers
/// - **Queries with filters**: `Query<&T, With<U>>`
///
/// # Safety
///
/// The `get` method is unsafe because:
/// 1. Multiple parameters may create aliased mutable references to the world
/// 2. The caller must ensure parameters access disjoint data
/// 3. Component access validate this at the scheduler level
///
/// See [`super::function::WithSystemParams`] for details on safe usage.
pub trait Parameter: Sized {
    /// The runtime value type with world lifetime applied.
    ///
    /// This Generic Associated Type (GAT) allows the parameter type to be specified without
    /// a concrete lifetime in function signatures, while the runtime value has the world's lifetime.
    ///
    /// # Type Parameters
    ///
    /// - `'w`: World lifetime - how long the extracted value can reference world data
    /// - `'s`: State lifetime - how long the value can reference parameter state
    ///
    /// # Type Relationship
    ///
    /// For query parameters:
    /// - `Self` = `query::Result<'_, D>` (elided lifetime in function signature)
    /// - `Value<'w, 's>` = `query::Result<'w, D>` (concrete world lifetime at runtime)
    ///
    /// For world parameters:
    /// - `Self` = `&mut World` (no lifetime in function signature)
    /// - `Value<'w, 's>` = `&'w mut World` (concrete world lifetime at runtime)
    ///
    /// The `Value<'w, 's>` must also be `Parameter` to allow nested extraction (future feature).
    type Value<'w, 's>: Parameter<State = Self::State>;

    /// The runtime state associated with this parameter.
    ///
    /// This Generic Associated Type (GAT) allows parameters to maintain state
    /// with the world's lifetime.
    type State: 'static;

    /// Build the state for this parameter from the world.
    ///
    /// This method is called once per system execution to create any
    /// necessary state for the parameter.
    fn build_state(world: &mut world::World) -> Self::State;

    /// Get the world access required for this parameter.
    ///
    /// The access request describes which world resources this parameter accesses and how
    /// (immutable vs mutable). The scheduler uses this to:
    /// - Detect conflicts between system parameters
    /// - Validate no aliasing violations occur
    /// - Determine safe execution order for parallel systems (future)
    ///
    /// # Parameters
    ///
    /// - `components`: The component registry for looking up component IDs
    ///
    /// # Returns
    ///
    /// A [`world::AccessRequest`] describing world access required.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Query for &Position returns access with Position (immutable)
    /// let access = <query::Result<&Position> as Parameter>::access(&registry);
    ///
    /// // Query for &mut Velocity returns access with Velocity (mutable)
    /// let access = <query::Result<&mut Velocity> as Parameter>::access(&registry);
    ///
    /// // World access returns immutable world access.
    /// let access = <&World as Parameter>::access(&registry);
    ///
    /// // World access returns mutable world access.
    /// let access = <&mut World as Parameter>::access(&registry);
    /// ```
    fn required_access(components: &component::Registry) -> world::AccessRequest;

    /// Extract this parameter's value from a shard.
    ///
    /// This method is called by the system executor to provide parameter values to
    /// the system function. The returned value has the shard's lifetime `'w`.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// 1. **No aliasing**: Parameters access disjoint data (validated by access requests)
    /// 2. **Valid lifetime**: Shard reference is valid for lifetime `'w`
    /// 3. **No concurrency**: System is not executed concurrently with conflicting systems
    ///
    /// The [`super::function::WithSystemParams`] implementation upholds these by:
    /// - Using raw pointers to create aliased shard references (sound due to disjoint access)
    /// - Relying on scheduler to validate access requests before execution
    /// - Requiring `&mut self` on system execution (prevents concurrent calls)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let state = <query::Result<&Position> as Parameter>::build_state(&mut world);
    /// let shard = world.shard(&access_request)?;
    ///
    /// // Query extraction
    /// let query = unsafe { <query::Result<&Position> as Parameter>::get(&mut shard, &mut state) };
    /// for pos in query {
    ///     println!("({}, {})", pos.x, pos.y);
    /// }
    /// ```
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        state: &'s mut Self::State,
    ) -> Self::Value<'w, 's>;
}

/// Implementation of [`Parameter`] for query results.
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
/// fn print_positions(query: query::Result<&Position>) {
///     for pos in query {
///         println!("({}, {})", pos.x, pos.y);
///     }
/// }
///
/// // Mutable query
/// fn apply_gravity(query: query::Result<&mut Velocity>) {
///     for vel in query {
///         vel.dy -= 9.8;
///     }
/// }
///
/// // Mixed access
/// fn movement(query: query::Result<(&Velocity, &mut Position)>) {
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
/// fn heal(query: query::Result<(&Player, Option<&mut Health>)>) {
///     for (player, health) in query {
///         if let Some(h) = health {
///             h.current += 1;
///         }
///     }
/// }
/// ```
///
/// # Implementation Details
///
/// - **Value type**: `query::Result<'w, D>` where `'w` is the world lifetime
/// - **Access request**: Delegates to `D::spec()` which analyzes the query type
/// - **Extraction**: Calls `world.query::<D>()` to create the iterator
///
/// The lifetime `'_` in `query::Result<'_, D>` is elided in function signatures,
/// and becomes `'w` (world lifetime) at runtime via [`Value`](Parameter::Value).
///
/// See [`query`](crate::ecs::query) module for details on query composition.
impl<D: query::Data + 'static> Parameter for query::Result<'_, D> {
    /// The value type is the query result with world lifetime.
    type Value<'w, 's> = query::Result<'w, D>;

    /// The state type is the query instance for this data.
    type State = query::Query<D>;

    /// Build the query state for this parameter.
    fn build_state(world: &mut world::World) -> Self::State {
        query::Query::new(world.components())
    }

    /// Get the world access request for this query parameter.
    /// This delegates to the underlying Data type to determine the required components.
    fn required_access(components: &component::Registry) -> world::AccessRequest {
        D::spec(components).as_access_request()
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
        // TODO: Safety: Caller ensures disjoint access via access requests - The query invoke
        // method is safe, but its unclear it should be since the table access is only checked at
        // debug. That being said, the world manages the grants for shards so its probably ok.
        state.invoke(shard)
    }
}

/// Implementation of [`Parameter`] for direct immutable world access.
///
/// This allows systems to read world structure (entity metadata, etc.) directly.
/// Note that this parameter type can only be used in exclusive systems (those taking
/// `&mut World` directly), not in parallel systems with multiple parameters.
///
/// # Scheduling Implications
///
/// Immutable world access indicates the system needs read-only access to the world structure
/// itself (not components). A scheduler should treat this as exclusive access for safety.
///
/// # When to Use
///
/// Use immutable world access when you need to:
/// - **Access entity metadata**: Query entity existence, generation counters, etc.
/// - **Read world configuration**: World ID or other world-level data
///
/// Don't use world access for:
/// - **Reading components**: Use queries instead for better performance
/// - **Parallel systems**: World parameters bypass shard grants
///
/// # Examples
///
/// ```rust,ignore
/// // Exclusive system with world access
/// fn validator(world: &World) {
///     for entity in some_entity_list {
///         if world.entity(entity).is_none() {
///             println!("Entity no longer exists!");
///         }
///     }
/// }
/// ```
///
/// # Implementation Details
///
/// - **Value type**: `&'w World` where `'w` is the shard lifetime
/// - **Access request**: Returns access request for immutable world access
/// - **Extraction**: Extracts world from shard unsafely (bypassing grant)
///
impl Parameter for &world::World {
    /// The value type is an immutable world reference with shard lifetime.
    type Value<'w, 's> = &'w world::World;

    /// The state type is empty since no state is needed for immutable world access.
    type State = ();

    /// Build empty state for this parameter.
    fn build_state(_world: &mut world::World) -> Self::State {}

    /// Get the world access request for this world parameter.
    fn required_access(_components: &component::Registry) -> world::AccessRequest {
        world::AccessRequest::to_world(false)
    }

    /// Get immutable access to the world from the shard.
    ///
    /// # Safety
    ///
    /// This bypasses the shard's grant checking and accesses the world directly.
    /// It should only be used in systems that have been validated to require
    /// world-level access. Typically, world parameters indicate the system should
    /// be exclusive rather than parallel.
    unsafe fn get<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
    ) -> Self::Value<'w, 's> {
        // SAFETY: Caller ensures this system has exclusive world access rights
        unsafe { shard.world() }
    }
}

#[cfg(test)]
mod tests {

    use crate::ecs::{component, entity, query, system::Parameter, world};
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
            world.components().register::<Comp1>(),
            world.components().register::<Comp2>(),
        ]);
        let entities = (
            world.spawn(Comp1 { value: 10 }),
            world.spawn(Comp2 { value: 20 }),
            world.spawn((Comp1 { value: 30 }, Comp2 { value: 40 })),
        );
        (world, entities)
    }

    #[test]
    fn world_param_component_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let access = <&world::World as Parameter>::required_access(world.components());

        // Then
        assert!(access.world());
    }

    #[test]
    fn world_param_get() {
        // Given
        let (mut world, _) = test_setup();
        #[allow(clippy::let_unit_value)]
        let mut state = <&world::World as Parameter>::build_state(&mut world);
        let access = <&world::World as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let world_ref = unsafe { <&world::World as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(world_ref.id(), world.id());

        // Release shard
        world.release_shard(shard);
    }

    #[test]
    fn comp_param_component_access() {
        // Given
        let (world, _) = test_setup();

        // When
        let access =
            <query::Result<(&Comp1, &Comp2)> as Parameter>::required_access(world.components());

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![
                    world.components().get::<Comp1>().unwrap(),
                    world.components().get::<Comp2>().unwrap()
                ]),
                component::Spec::EMPTY,
            )
        )
    }

    #[test]
    fn comp_param_component_get() {
        // Given
        let (mut world, _) = test_setup();
        let mut state = <query::Result<&Comp1> as Parameter>::build_state(&mut world);
        let access = <query::Result<&Comp1> as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result =
            unsafe { <query::Result<&Comp1> as Parameter>::get(&mut shard, &mut state) };

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
        let mut state = <query::Result<&mut Comp1> as Parameter>::build_state(&mut world);
        let access = <query::Result<&mut Comp1> as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result =
            unsafe { <query::Result<&mut Comp1> as Parameter>::get(&mut shard, &mut state) };

        // Then
        assert_eq!(result.len(), 2);
        let comp = result.next().unwrap();
        comp.value += 1;
        let comp = result.next().unwrap();
        comp.value += 2;

        // Release shard
        world.release_shard(shard);

        // And When
        let mut state = <query::Result<&Comp1> as Parameter>::build_state(&mut world);
        let access = <query::Result<&Comp1> as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");
        let mut result =
            unsafe { <query::Result<&Comp1> as Parameter>::get(&mut shard, &mut state) };

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
        let access =
            <query::Result<entity::Entity> as Parameter>::required_access(world.components());

        // Then
        assert!(access.is_none());
    }

    #[test]
    fn comp_param_entity_get() {
        // Given
        let (mut world, (entity1, entity2, entity3)) = test_setup();
        let mut state = <query::Result<entity::Entity> as Parameter>::build_state(&mut world);
        let access =
            <query::Result<entity::Entity> as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result =
            unsafe { <query::Result<entity::Entity> as Parameter>::get(&mut shard, &mut state) };

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
        let access =
            <query::Result<(entity::Entity, &Comp1, &mut Comp2)> as Parameter>::required_access(
                world.components(),
            );

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_components(
                component::Spec::new(vec![world.components().get::<Comp1>().unwrap()]),
                component::Spec::new(vec![world.components().get::<Comp2>().unwrap()]),
            )
        )
    }

    #[test]
    fn comp_param_entity_comps_get() {
        // Given
        let (mut world, (entity1, _, entity3)) = test_setup();
        let mut state =
            <query::Result<(entity::Entity, &Comp1, Option<&mut Comp2>)> as Parameter>::build_state(
                &mut world,
            );
        let access = <query::Result<(entity::Entity, &Comp1, Option<&mut Comp2>)> as Parameter>::required_access(world.components());
        let mut shard = world.shard(&access).expect("Failed to create shard");

        // When
        let mut result = unsafe {
            <query::Result<(entity::Entity, &Comp1, Option<&mut Comp2>)> as Parameter>::get(
                &mut shard, &mut state,
            )
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
