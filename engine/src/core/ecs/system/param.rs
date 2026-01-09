//! System parameter extraction using Generic Associated Types.
//!
//! This module defines the [`Parameter`] trait, which enables clean system function signatures
//! without explicit lifetime parameters. The key innovation is using a Generic Associated Type
//! (GAT) to carry the world lifetime, separating the parameter's static type from its runtime value.

use crate::core::ecs::{component, query, world};

/// A type that can be passed as a parameter to a system function.
///
/// The `Parameter` trait enables extracting data from the world with clean function signatures.
/// The secret is the [`Value`](Parameter::Value) Generic Associated Type (GAT), which carries
/// the world's lifetime without appearing in the function signature.
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
/// for<'a> &'a mut Func: FnMut(Param) + FnMut(Param::Value<'a>)
/// ```
///
/// This says: "The function must accept both the parameter type AND its value form with any lifetime."
///
/// # Implementations
///
/// ## Query Results
///
/// ```rust,ignore
/// impl<D: query::Data> Parameter for query::Result<'_, D> {
///     type Value<'w> = query::Result<'w, D>;
///
///     unsafe fn get<'w>(world: &'w mut World) -> query::Result<'w, D> {
///         world.query::<D>()
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
///     type Value<'w> = &'w mut World;
///
///     unsafe fn get<'w>(world: &'w mut World) -> &'w mut World {
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
/// 3. Component specs validate this at the scheduler level
///
/// See [`super::function::WithSystemParams`] for details on safe usage.
pub trait Parameter: Sized {
    /// The runtime value type with world lifetime applied.
    ///
    /// This Generic Associated Type (GAT) allows the parameter type to be specified without
    /// a concrete lifetime in function signatures, while the runtime value has the world's lifetime.
    ///
    /// # Type Relationship
    ///
    /// For query parameters:
    /// - `Self` = `query::Result<'_, D>` (elided lifetime in function signature)
    /// - `Value<'w>` = `query::Result<'w, D>` (concrete world lifetime at runtime)
    ///
    /// For world parameters:
    /// - `Self` = `&mut World` (no lifetime in function signature)
    /// - `Value<'w>` = `&'w mut World` (concrete world lifetime at runtime)
    ///
    /// The `Value<'w>` must also be `Parameter` to allow nested extraction (future feature).
    type Value<'w>: Parameter;

    /// Get the component specification for this parameter.
    ///
    /// The component spec describes which components this parameter accesses and how
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
    /// A [`component::Spec`] describing the components accessed.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Query for &Position returns spec with Position (immutable)
    /// let spec = <query::Result<&Position> as Parameter>::component_spec(&registry);
    ///
    /// // Query for &mut Velocity returns spec with Velocity (mutable)
    /// let spec = <query::Result<&mut Velocity> as Parameter>::component_spec(&registry);
    ///
    /// // World access returns empty spec (should be "all components" in future)
    /// let spec = <&mut World as Parameter>::component_spec(&registry);
    /// ```
    ///
    /// # Future
    ///
    /// This will be generalized to a "world access request" that covers:
    /// - Component access (current)
    /// - Resource access
    /// - Full world access (exclusive)
    /// - Archetype-level access (optimization)
    fn component_spec(components: &component::Registry) -> component::Spec;

    /// Extract this parameter's value from the world.
    ///
    /// This method is called by the system executor to provide parameter values to
    /// the system function. The returned value has the world's lifetime `'w`.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// 1. **No aliasing**: Parameters access disjoint data (validated by component specs)
    /// 2. **Valid lifetime**: World reference is valid for lifetime `'w`
    /// 3. **No concurrency**: System is not executed concurrently with conflicting systems
    ///
    /// The [`super::function::WithSystemParams`] implementation upholds these by:
    /// - Using raw pointers to create aliased world references (sound due to disjoint access)
    /// - Relying on scheduler to validate component specs
    /// - Requiring `&mut self` on system execution (prevents concurrent calls)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Query extraction
    /// let query = unsafe { <query::Result<&Position> as Parameter>::get(&mut world) };
    /// for pos in query {
    ///     println!("({}, {})", pos.x, pos.y);
    /// }
    ///
    /// // World extraction
    /// let world_ref = unsafe { <&mut World as Parameter>::get(&mut world) };
    /// world_ref.spawn(Entity::new());
    /// ```
    unsafe fn get<'w>(world: &'w mut world::World) -> Self::Value<'w>;
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
/// - **Component spec**: Delegates to `D::spec()` which analyzes the query type
/// - **Extraction**: Calls `world.query::<D>()` to create the iterator
///
/// The lifetime `'_` in `query::Result<'_, D>` is elided in function signatures,
/// and becomes `'w` (world lifetime) at runtime via [`Value`](Parameter::Value).
///
/// See [`query`](crate::core::ecs::query) module for details on query composition.
impl<D: query::Data> Parameter for query::Result<'_, D> {
    type Value<'w> = query::Result<'w, D>;

    /// Get the component specification for this query parameter.
    /// This delegates to the underlying Data type to determine the required components.
    fn component_spec(components: &component::Registry) -> component::Spec {
        D::spec(components).as_component_spec()
    }

    /// Get the query results by executing a query for the specific data against the provided world
    /// instance.
    unsafe fn get<'w>(world: &'w mut world::World) -> Self::Value<'w> {
        world.query::<D>()
    }
}

/// Implementation of [`Parameter`] for direct world access.
///
/// This allows systems to access the world directly for operations that don't fit
/// the query pattern, such as spawning/despawning entities or accessing non-component data.
///
/// # Performance Implications
///
/// World access is **exclusive** - only one system can have world access at a time.
/// This limits parallelization opportunities. Prefer using queries when possible.
///
/// Once a world access request system exists (future), any system with a world parameter
/// will block all other systems from running in parallel, significantly reducing throughput.
///
/// # When to Use
///
/// Use world access when you need to:
/// - **Spawn entities**: Create new entities with components
/// - **Despawn entities**: Remove entities from the world
/// - **Access entity metadata**: Query entity existence, generation counters, etc.
/// - **Modify world structure**: Change archetypes or tables (advanced)
///
/// Don't use world access for:
/// - **Reading/writing components**: Use queries instead
/// - **Counting entities**: Use `query.len()` instead
/// - **Finding entities**: Use queries with filters (future)
///
/// # Examples
///
/// ```rust,ignore
/// // Spawning based on query results
/// fn spawner(enemies: query::Result<&Enemy>, world: &mut World) {
///     if enemies.len() < 10 {
///         world.spawn((
///             Enemy,
///             Position { x: 0.0, y: 0.0 },
///             Health { current: 100, max: 100 },
///         ));
///     }
/// }
///
/// // Despawning dead entities
/// fn reaper(
///     dead: query::Result<(entity::Entity, &Health)>,
///     world: &mut World,
/// ) {
///     let to_remove: Vec<_> = dead
///         .filter(|(_, health)| health.current <= 0)
///         .map(|(entity, _)| entity)
///         .collect();
///
///     for entity in to_remove {
///         world.despawn(entity);
///     }
/// }
///
/// // Checking entity existence
/// fn validator(player_id: entity::Entity, world: &mut World) {
///     if world.entity(player_id).is_none() {
///         println!("Player entity no longer exists!");
///     }
/// }
/// ```
///
/// # Implementation Details
///
/// - **Value type**: `&'w mut World` where `'w` is the world lifetime
/// - **Component spec**: Returns empty spec (should be "all components" in future)
/// - **Extraction**: Returns the world reference directly
///
/// # Future
///
/// The component spec will be enhanced to signal "requires exclusive world access",
/// allowing the scheduler to properly serialize world access with all other systems.
///
/// A `Commands` parameter type will be added for deferred spawning/despawning,
/// which won't require exclusive access and can run in parallel.
impl Parameter for &mut world::World {
    type Value<'w> = &'w mut world::World;

    /// Get the component specification for this world parameter.
    /// Since we don't currently have any way to signify this should block all component access,
    /// we are just bailing with no components.
    ///
    /// Note: Replace with full world access request.
    fn component_spec(_components: &component::Registry) -> component::Spec {
        // TODO: Require All Components.....
        component::Spec::EMPTY
    }

    /// Get the world.....
    unsafe fn get<'w>(world: &'w mut world::World) -> Self::Value<'w> {
        world
    }
}

#[cfg(test)]
mod tests {

    use crate::core::ecs::{component, entity, query, system::Parameter, world};
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
        let spec = <&mut world::World as Parameter>::component_spec(world.components());

        // Then - world param should have empty component spec (or all components in future)
        assert_eq!(spec.ids().len(), 0);
    }

    #[test]
    fn world_param_get() {
        // Given
        let (mut world, _) = test_setup();

        // When
        let world_ref = unsafe { <&mut world::World as Parameter>::get(&mut world) };

        // Then
        assert_eq!(world_ref.id(), world.id());
    }

    #[test]
    fn comp_param_component_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let spec = <query::Result<&Comp1> as Parameter>::component_spec(world.components());

        // Then
        assert_eq!(spec.ids().len(), 1);
        assert_eq!(spec.ids()[0], world.components().get::<Comp1>().unwrap());
    }

    #[test]
    fn comp_param_component_get() {
        // Given
        let (mut world, _) = test_setup();

        // When
        let mut result = unsafe { <query::Result<&Comp1> as Parameter>::get(&mut world) };

        // Then
        assert_eq!(result.len(), 2);
        let row = result.next().unwrap();
        assert_eq!(row.value, 10);
        let row = result.next().unwrap();
        assert_eq!(row.value, 30);
    }

    #[test]
    fn comp_param_component_get_mut() {
        // Given
        let (mut world, _) = test_setup();

        // When
        let mut result = unsafe { <query::Result<&mut Comp1> as Parameter>::get(&mut world) };

        // Then
        assert_eq!(result.len(), 2);
        let comp = result.next().unwrap();
        comp.value += 1;
        let comp = result.next().unwrap();
        comp.value += 2;

        // And When
        let mut result = unsafe { <query::Result<&Comp1> as Parameter>::get(&mut world) };

        // Then
        assert_eq!(result.len(), 2);
        let comp = result.next().unwrap();
        assert_eq!(comp.value, 11);
        let comp = result.next().unwrap();
        assert_eq!(comp.value, 32);
    }

    #[test]
    fn comp_param_entity_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let spec = <query::Result<entity::Entity> as Parameter>::component_spec(world.components());

        // Then
        assert_eq!(spec.ids().len(), 0);
    }

    #[test]
    fn comp_param_entity_get() {
        // Given
        let (mut world, (entity1, entity2, entity3)) = test_setup();

        // When
        let mut result = unsafe { <query::Result<entity::Entity> as Parameter>::get(&mut world) };

        // Then
        assert_eq!(result.len(), 3);

        let entity = result.next().unwrap();
        assert_eq!(entity, entity1);
        let entity = result.next().unwrap();
        assert_eq!(entity, entity2);
        let entity = result.next().unwrap();
        assert_eq!(entity, entity3);
    }

    #[test]
    fn comp_param_entity_comps_spec() {
        // Given
        let (world, _) = test_setup();

        // When
        let spec =
            <query::Result<(entity::Entity, &Comp1, &mut Comp2)> as Parameter>::component_spec(
                world.components(),
            );

        // Then
        assert_eq!(spec.ids().len(), 2);
        assert_eq!(spec.ids()[0], world.components().get::<Comp1>().unwrap());
        assert_eq!(spec.ids()[1], world.components().get::<Comp2>().unwrap());
    }

    #[test]
    fn comp_param_entity_comps_get() {
        // Given
        let (mut world, (entity1, _, entity3)) = test_setup();

        // When
        let mut result = unsafe {
            <query::Result<(entity::Entity, &Comp1, Option<&mut Comp2>)> as Parameter>::get(
                &mut world,
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
    }
}
