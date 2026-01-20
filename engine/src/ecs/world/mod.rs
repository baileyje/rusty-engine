//! The World is the central container for all entities, components, and systems in the ECS.
//!
//! A `World` manages the lifecycle of entities and their associated component data. It provides
//! the primary API for spawning and despawning entities, as well as accessing and modifying
//! their components.
//!
//! # Architecture
//!
//! The World coordinates several subsystems:
//! - **Entity Allocator**: Manages entity ID allocation and reuse
//! - **Entity Registry**: Tracks which entities are spawned and their storage locations
//! - **Component Registry**: Maintains metadata about registered component types
//! - **Storage**: Manages the actual component data organized by archetype
//! - **Archetype Registry**: Tracks unique combinations of component types
//!
//! # Example
//!
//! ```ignore
//! use rusty_engine::ecs::world::World;
//!
//! let mut world = World::new(Id(1));
//!
//! // Spawn an entity with components
//! let entity = world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 0.0 }));
//!
//! // Access the entity
//! if let Some(entity_ref) = world.entity(entity) {
//!     let pos = entity_ref.get::<Position>().unwrap();
//! }
//!
//! // Despawn the entity
//! world.despawn(entity);
//! ```
mod access;
mod registry;
mod shard;

use std::cell::RefCell;
use std::marker::PhantomData;

use crate::ecs::{
    component::{self},
    entity,
    query::{self},
    storage::{self},
    unique,
    world::access::{ConflictError, GrantTracker},
};

/// Exported types for world access control.
pub use access::{AccessGrant, AccessRequest};
pub use registry::{TypeId, TypeInfo, TypeRegistry};
pub use shard::Shard;

/// A world identifier. This is a unique identifier for a world in the ECS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

impl Id {
    /// Create a new world identifier.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Id(id)
    }

    /// Get the raw identifier value.
    #[inline]
    pub const fn id(&self) -> u32 {
        self.0
    }
}

/// The World is the central container for all entities, components, and their relationships.
///
/// A World manages entity lifecycles, component storage, and provides the primary API for
/// interacting with the ECS. Each world is isolated from other worlds and maintains its own
/// set of entities and components.
pub struct World {
    /// The world's unique identifier.
    id: Id,

    /// The world's entity allocator.
    entity_allocator: entity::Allocator,

    /// The registry of all know resource types in the world.
    resources: TypeRegistry,

    /// The storage for components in the world.
    storage: storage::Storage,

    /// The current access grants for the world.
    active_grants: RefCell<GrantTracker>,

    /// Marker to make World !Send. World must stay on the main thread.
    _not_send: PhantomData<*mut ()>,
}

impl World {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            entity_allocator: entity::Allocator::default(),
            resources: TypeRegistry::default(),
            storage: storage::Storage::default(),
            active_grants: RefCell::new(GrantTracker::default()),
            _not_send: PhantomData,
        }
    }

    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    #[inline]
    pub fn resources(&self) -> &TypeRegistry {
        &self.resources
    }

    #[inline]
    pub fn archetypes(&self) -> &storage::archetype::Archetypes {
        self.storage.archetypes()
    }

    #[inline]
    pub fn storage(&self) -> &storage::Storage {
        &self.storage
    }

    #[inline]
    pub fn storage_mut(&mut self) -> &mut storage::Storage {
        &mut self.storage
    }

    /// Spawn a new entity with the given set of components in the world.
    /// This will establish the entity in the appropriate archetype and storage table.
    pub fn spawn<V: storage::Values>(&mut self, set: V) -> entity::Entity {
        // Allocate a new entity.
        let entity = self.entity_allocator.alloc();

        // Spawn the entity in storage.
        self.storage.spawn_entity(entity, set, &self.resources);

        entity
    }

    /// Spawn a new entity with the given set of components in the world.
    /// This will establish the entity in the appropriate archetype and storage table.
    pub fn spawn_many<V: storage::Values>(
        &mut self,
        values: impl IntoIterator<Item = V>,
    ) -> Vec<entity::Entity> {
        // Get the component sets as a vec.
        let sets = values.into_iter();

        // Allocate a new entities based on the number of values.
        let entities = self.entity_allocator.alloc_many(sets.size_hint().0);

        // Spawn the entities in storage with the component sets.
        self.storage
            .spawn_entities(entities.iter().copied().zip(sets), &self.resources);

        entities
    }

    /// Despawn the given entity from the world. This will remove the entity and all its components
    /// from storage.
    ///
    /// If the entity is not currently spawned, this method does nothing.
    pub fn despawn(&mut self, entity: entity::Entity) {
        // Delegate to storage to despawn the entity.
        self.storage.despawn_entity(entity);
    }

    /// Add a component to an existing entity.
    ///
    /// This migrates the entity to a new archetype that includes the new component.
    /// If the entity already has this component type, this method does nothing.
    ///
    /// # Returns
    /// - `true` if the component was added
    /// - `false` if the entity doesn't exist or already has this component
    pub fn add_components<V: storage::Values>(
        &mut self,
        entity: entity::Entity,
        components: V,
    ) -> bool {
        self.storage
            .add_components(entity, components, &self.resources)
    }

    /// Remove a component from an existing entity.
    ///
    /// This migrates the entity to a new archetype that excludes the component.
    /// If the entity doesn't have this component type, this method does nothing.
    ///
    /// # Returns
    /// - `true` if the component was removed
    /// - `false` if the entity doesn't exist or doesn't have this component
    pub fn remove_components<S: component::IntoSpec>(&mut self, entity: entity::Entity) -> bool {
        self.storage.remove_components::<S>(entity, &self.resources)
    }
    /// Get a reference to the given entity, if it's spawned.
    ///
    /// Returns `None` if the entity is not currently spawned in the world.
    pub fn entity(&self, entity: entity::Entity) -> Option<entity::Ref<'_>> {
        self.storage_for(entity)
            .map(|(table, row)| entity::Ref::new(entity, table, row))
    }

    /// Get a mutable reference to the given entity, if it's spawned.
    ///
    /// Returns `None` if the entity is not currently spawned in the world.
    ///
    /// # Note
    /// This method holds a mutable reference to the entire world's storage, preventing
    /// any other access while the `RefMut` is held. For performance-critical code,
    /// consider using queries/systems that can access multiple entities efficiently.
    pub fn entity_mut(&mut self, entity: entity::Entity) -> Option<entity::RefMut<'_>> {
        let loc = self.storage.location_for(entity)?;
        let table = self.storage.get_table_mut(loc.table_id());
        Some(entity::RefMut::new(entity, table, loc.row()))
    }

    /// Get the storage table and row for a reference to the given entity, if the entity is spawned.
    pub fn storage_for(&self, entity: entity::Entity) -> Option<(&storage::Table, storage::Row)> {
        let loc = self.storage.location_for(entity)?;
        Some((self.storage.get_table(loc.table_id()), loc.row()))
    }

    /// Get the storage table and row for a mutable reference to the given entity, if the entity is
    /// spawned.
    pub fn storage_for_mut(
        &mut self,
        entity: entity::Entity,
    ) -> Option<(&mut storage::Table, storage::Row)> {
        let loc = self.storage.location_for(entity)?;
        Some((self.storage.get_table_mut(loc.table_id()), loc.row()))
    }

    /// Register a new component type in the world.
    pub fn register_component<C: component::Component>(&mut self) -> TypeId {
        self.resources.register_component::<C>()
    }

    /// Perform a world query to access all entities that match the query data `D`.
    ///
    ///
    /// Note: This holds a mutable reference to the entire world while the query result is active
    /// (use wisely).
    pub fn query<'w, D: query::Data>(&'w mut self) -> query::Result<'w, D> {
        let query = query::Query::<D>::new(&self.resources);
        query.invoke(self)
    }

    /// Register a new resource type in the world.
    pub fn register_unique<U: unique::Unique>(&mut self) -> TypeId {
        self.resources.register_unique::<U>()
    }

    /// Add a unique resource to the world.
    #[inline]
    pub fn add_unique<U: unique::Unique>(&mut self, resource: U) {
        self.storage.uniques_mut().insert::<U>(resource);
    }

    /// Get access to a unique resource stored in the world, if it exists.
    #[inline]
    pub fn get_unique<U: unique::Unique>(&self) -> Option<&U> {
        self.storage.uniques().get::<U>()
    }

    /// Get mutable access to a unique resource stored in the world, if it exists.
    #[inline]
    pub fn get_unique_mut<U: unique::Unique>(&mut self) -> Option<&mut U> {
        self.storage.uniques_mut().get_mut::<U>()
    }

    /// Remove a unique resource from the world, returning it if it existed.
    #[inline]
    pub fn remove_unique<U: unique::Unique>(&mut self) -> Option<U> {
        self.storage.uniques_mut().remove::<U>()
    }

    /// Create a shard with the requested access.
    ///
    /// Takes `&self` to allow multiple shards to coexist.
    /// Uses interior mutability to track active grants.
    pub fn shard(&self, access: &AccessRequest) -> Result<Shard<'_>, ConflictError> {
        // Check for conflicts and register grant
        let grant = self.active_grants.borrow_mut().check_and_grant(access)?;
        // Return the shard
        Ok(Shard::new(self as *const World as *mut World, grant))
    }

    /// Release a shard of this world.
    ///
    /// Must be called on the main thread (where the World lives).
    pub fn release_shard(&self, shard: Shard) {
        self.active_grants.borrow_mut().remove(&shard.into_grant());
    }

    /// Release a grant that was returned from a shard via `into_grant()`. This should consume the
    /// grant to prevent double-releasing.
    ///
    /// Note: Its generally safer to use `release_shard()` when possible.
    ///
    /// Must be called on the main thread (where the World lives).
    pub fn release_grant(&self, grant: &AccessGrant) {
        self.active_grants.borrow_mut().remove(grant);
    }
}

// World is intentionally !Send and !Sync:
// - !Send: World must stay on the main thread where it was created
// - !Sync: RefCell<GrantTracker> is !Sync, and we don't want &World shared across threads
//
// The _not_send marker ensures !Send (RefCell is Send, so we need the marker).
// RefCell naturally provides !Sync.

#[cfg(test)]
mod test {
    use rusty_macros::Component;

    use crate::ecs::world::{Id, World};

    #[derive(Component, Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Debug, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[test]
    fn spawn_empty_entity() {
        // Given
        let mut world = World::new(Id(1));
        // When
        let entity = world.spawn(());
        // Then
        assert!(world.storage.entities().is_spawned(entity));
    }

    #[test]
    fn spawn_entity_with_components() {
        // Given
        let mut world = World::new(Id(1));

        // When
        let entity = world.spawn((Position { x: 42.0, y: 67.0 }, Velocity { dx: 0.0, dy: 1.0 }));

        // Then
        assert!(world.storage.entities().is_spawned(entity));

        let entity_ref = world.entity(entity).unwrap();

        assert_eq!(
            Position { x: 42.0, y: 67.0 },
            *entity_ref.get::<Position>().unwrap()
        );
        assert_eq!(
            Velocity { dx: 0.0, dy: 1.0 },
            *entity_ref.get::<Velocity>().unwrap()
        );
    }

    #[test]
    fn spawn_many_entity_with_components() {
        // Given
        let mut world = World::new(Id(1));

        // When
        let entities = world.spawn_many([
            (Position { x: 42.0, y: 67.0 }, Velocity { dx: 0.0, dy: 1.0 }),
            (Position { x: 67.0, y: 42.0 }, Velocity { dx: 1.0, dy: 0.0 }),
        ]);

        // Then
        let entity = entities[0];
        assert!(world.storage.entities().is_spawned(entity));

        let entity_ref = world.entity(entity).unwrap();

        assert_eq!(
            Position { x: 42.0, y: 67.0 },
            *entity_ref.get::<Position>().unwrap()
        );
        assert_eq!(
            Velocity { dx: 0.0, dy: 1.0 },
            *entity_ref.get::<Velocity>().unwrap()
        );

        let entity = entities[1];
        assert!(world.storage.entities().is_spawned(entity));

        let entity_ref = world.entity(entity).unwrap();

        assert_eq!(
            Position { x: 67.0, y: 42.0 },
            *entity_ref.get::<Position>().unwrap()
        );
        assert_eq!(
            Velocity { dx: 1.0, dy: 0.0 },
            *entity_ref.get::<Velocity>().unwrap()
        );
    }

    #[test]
    fn despawn_entity_with_components() {
        // Given
        let mut world = World::new(Id(1));

        #[derive(Component, Debug, PartialEq)]
        struct Comp1 {
            value: u32,
        }

        #[derive(Component, Debug, PartialEq)]
        struct Comp2 {
            value: String,
        }

        // When
        let entity = world.spawn((
            Comp1 { value: 42 },
            Comp2 {
                value: "Hello".to_string(),
            },
        ));

        // Then
        assert!(world.storage.entities().is_spawned(entity));

        // And When
        world.despawn(entity);

        // Then
        assert!(!world.storage.entities().is_spawned(entity));
        assert!(world.entity(entity).is_none());
    }

    #[test]
    fn despawn_entity_swaps_and_updates_location() {
        // Given
        let mut world = World::new(Id(1));

        #[derive(Component, Debug, PartialEq)]
        struct Comp1;

        let entity1 = world.spawn(Comp1);
        // Confirm entity1 is at row 0
        assert_eq!(
            world.storage.entities().location(entity1).unwrap().row(),
            0.into()
        );

        let entity2 = world.spawn(Comp1);
        // Confirm entity2 is at row 1
        assert_eq!(
            world.storage.entities().location(entity2).unwrap().row(),
            1.into()
        );

        // And When
        world.despawn(entity1);

        // Then
        assert!(!world.storage.entities().is_spawned(entity1));

        // Confirm entity2 is now at row 0
        assert_eq!(
            world.storage.entities().location(entity2).unwrap().row(),
            0.into()
        );

        // Confirm entity2 is still spawned
        assert!(world.storage.entities().is_spawned(entity2));

        // Confirm we can still get its components
        assert!(world.entity(entity2).unwrap().get::<Comp1>().is_some());
    }

    #[test]
    fn world_id() {
        let world = World::new(Id(42));
        assert_eq!(world.id(), Id(42));
        assert_eq!(world.id().id(), 42);
    }

    #[test]
    fn despawn_non_existent_entity_is_noop() {
        let mut world = World::new(Id(1));

        #[derive(Component)]
        struct TestComp;

        let entity1 = world.spawn(TestComp);
        world.despawn(entity1);

        // Despawn again - should be a no-op
        world.despawn(entity1);

        // Entity should still be despawned
        assert!(!world.storage.entities().is_spawned(entity1));
    }

    #[test]
    fn entity_ref_access() {
        let mut world = World::new(Id(1));

        let entity = world.spawn(Position { x: 10.0, y: 20.0 });

        // Test entity() method
        let entity_ref = world.entity(entity).unwrap();
        let pos = entity_ref.get::<Position>().unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 20.0);
    }

    #[test]
    fn entity_mut_access() {
        let mut world = World::new(Id(1));

        #[derive(Component, Debug, PartialEq)]
        struct Counter {
            value: u32,
        }

        let entity = world.spawn(Counter { value: 0 });

        // Modify via entity_mut
        {
            let mut entity_mut = world.entity_mut(entity).unwrap();
            let counter = entity_mut.get_mut::<Counter>().unwrap();
            counter.value = 100;
        }

        // Verify the change
        let entity_ref = world.entity(entity).unwrap();
        assert_eq!(entity_ref.get::<Counter>().unwrap().value, 100);
    }

    #[test]
    fn multiple_archetypes() {
        let mut world = World::new(Id(1));

        #[derive(Component)]
        struct A;

        #[derive(Component)]
        struct B;

        #[derive(Component)]
        struct C;

        // Spawn entities with different component combinations
        let e1 = world.spawn((A, B));
        let e2 = world.spawn((A, C));
        let e3 = world.spawn((B, C));
        let e4 = world.spawn((A, B, C));

        // All should be spawned
        assert!(world.storage.entities().is_spawned(e1));
        assert!(world.storage.entities().is_spawned(e2));
        assert!(world.storage.entities().is_spawned(e3));
        assert!(world.storage.entities().is_spawned(e4));

        // Verify we can access them
        assert!(world.entity(e1).is_some());
        assert!(world.entity(e2).is_some());
        assert!(world.entity(e3).is_some());
        assert!(world.entity(e4).is_some());
    }

    #[test]
    fn entity_reuse_after_despawn() {
        let mut world = World::new(Id(1));

        #[derive(Component)]
        struct TestComp;

        let entity1 = world.spawn(TestComp);
        let entity1_id = entity1.id();

        world.despawn(entity1);

        // Spawn another entity - it may reuse the ID
        let entity2 = world.spawn(TestComp);

        // The generation should be different even if ID is reused
        if entity2.id() == entity1_id {
            assert_ne!(entity1.generation(), entity2.generation());
        }

        // Original entity should not be accessible
        assert!(!world.storage.entities().is_spawned(entity1));
        assert!(world.storage.entities().is_spawned(entity2));
    }

    #[test]
    fn add_component_to_entity() {
        let mut world = World::new(Id(1));

        // Spawn entity with just Position
        let entity = world.spawn(Position { x: 1.0, y: 2.0 });

        // Verify only has Position
        {
            let entity_ref = world.entity(entity).unwrap();
            assert!(entity_ref.get::<Position>().is_some());
            assert!(entity_ref.get::<Velocity>().is_none());
        }

        // Add Velocity component
        let added = world.add_components(entity, Velocity { dx: 0.5, dy: 0.3 });
        assert!(added);

        // Verify now has both components
        let entity_ref = world.entity(entity).unwrap();
        let pos = entity_ref.get::<Position>().unwrap();
        let vel = entity_ref.get::<Velocity>().unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert_eq!(vel.dx, 0.5);
        assert_eq!(vel.dy, 0.3);
    }

    #[test]
    fn add_component_already_exists_returns_false() {
        let mut world = World::new(Id(1));

        let entity = world.spawn(Position { x: 1.0, y: 2.0 });

        // Try to add Position again
        let added = world.add_components(entity, Position { x: 5.0, y: 6.0 });
        assert!(!added);

        // Original values should be unchanged
        let entity_ref = world.entity(entity).unwrap();
        let pos = entity_ref.get::<Position>().unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }

    #[test]
    fn add_component_to_nonexistent_entity_returns_false() {
        let mut world = World::new(Id(1));

        #[derive(Component)]
        struct TestComp;

        // Create and despawn an entity
        let entity = world.spawn(TestComp);
        world.despawn(entity);

        // Try to add component to despawned entity
        let added = world.add_components(entity, TestComp);
        assert!(!added);
    }

    #[test]
    fn remove_component_from_entity() {
        let mut world = World::new(Id(1));

        // Spawn entity with Position and Velocity
        let entity = world.spawn((Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }));

        // Verify has both components
        {
            let entity_ref = world.entity(entity).unwrap();
            assert!(entity_ref.get::<Position>().is_some());
            assert!(entity_ref.get::<Velocity>().is_some());
        }

        // Remove Velocity component
        let removed = world.remove_components::<Velocity>(entity);
        assert!(removed);

        // Verify only has Position now
        let entity_ref = world.entity(entity).unwrap();
        let pos = entity_ref.get::<Position>().unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert!(entity_ref.get::<Velocity>().is_none());
    }

    #[test]
    fn remove_component_not_present_returns_false() {
        let mut world = World::new(Id(1));

        let entity = world.spawn(Position { x: 1.0, y: 2.0 });

        // Try to remove Velocity which doesn't exist
        let removed = world.remove_components::<Velocity>(entity);
        assert!(!removed);
    }

    #[test]
    fn remove_component_from_nonexistent_entity_returns_false() {
        let mut world = World::new(Id(1));

        #[derive(Component)]
        struct TestComp;

        let entity = world.spawn(TestComp);
        world.despawn(entity);

        // Try to remove component from despawned entity
        let removed = world.remove_components::<TestComp>(entity);
        assert!(!removed);
    }

    #[test]
    fn add_component_updates_other_entity_location() {
        // Test that swap-remove during migration properly updates other entities
        let mut world = World::new(Id(1));

        // Spawn two entities with same archetype
        let entity1 = world.spawn(Position { x: 1.0, y: 1.0 });
        let entity2 = world.spawn(Position { x: 2.0, y: 2.0 });

        // entity1 at row 0, entity2 at row 1
        assert_eq!(world.storage.location_for(entity1).unwrap().row(), 0.into());
        assert_eq!(world.storage.location_for(entity2).unwrap().row(), 1.into());

        // Migrate entity1 to new archetype (Position + Velocity)
        world.add_components(entity1, Velocity { dx: 0.5, dy: 0.3 });

        // entity2 should now be at row 0 (was swapped during entity1's migration)
        assert_eq!(world.storage.location_for(entity2).unwrap().row(), 0.into());

        // Both entities should still be accessible with correct data
        let e1_ref = world.entity(entity1).unwrap();
        assert_eq!(e1_ref.get::<Position>().unwrap().x, 1.0);
        assert_eq!(e1_ref.get::<Velocity>().unwrap().dx, 0.5);

        let e2_ref = world.entity(entity2).unwrap();
        assert_eq!(e2_ref.get::<Position>().unwrap().x, 2.0);
        assert!(e2_ref.get::<Velocity>().is_none());
    }

    #[test]
    fn remove_component_updates_other_entity_location() {
        let mut world = World::new(Id(1));

        // Spawn two entities with same archetype (Position + Velocity)
        let entity1 = world.spawn((Position { x: 1.0, y: 1.0 }, Velocity { dx: 0.5, dy: 0.5 }));
        let entity2 = world.spawn((Position { x: 2.0, y: 2.0 }, Velocity { dx: 1.0, dy: 1.0 }));

        // entity1 at row 0, entity2 at row 1
        assert_eq!(world.storage.location_for(entity1).unwrap().row(), 0.into());
        assert_eq!(world.storage.location_for(entity2).unwrap().row(), 1.into());

        // Remove Velocity from entity1
        world.remove_components::<Velocity>(entity1);

        // entity2 should now be at row 0
        assert_eq!(world.storage.location_for(entity2).unwrap().row(), 0.into());

        // Both entities should still be accessible with correct data
        let e1_ref = world.entity(entity1).unwrap();
        assert_eq!(e1_ref.get::<Position>().unwrap().x, 1.0);
        assert!(e1_ref.get::<Velocity>().is_none());

        let e2_ref = world.entity(entity2).unwrap();
        assert_eq!(e2_ref.get::<Position>().unwrap().x, 2.0);
        assert_eq!(e2_ref.get::<Velocity>().unwrap().dx, 1.0);
    }

    #[test]
    fn add_then_remove_component() {
        let mut world = World::new(Id(1));

        #[derive(Component, Debug, PartialEq)]
        struct Tag;

        let entity = world.spawn(Position { x: 1.0, y: 2.0 });

        // Add Tag
        assert!(world.add_components(entity, Tag));
        assert!(world.entity(entity).unwrap().get::<Tag>().is_some());

        // Remove Tag
        assert!(world.remove_components::<Tag>(entity));
        assert!(world.entity(entity).unwrap().get::<Tag>().is_none());

        // Position should still be there
        let entity_ref = world.entity(entity).unwrap();
        let pos = entity_ref.get::<Position>().unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }
}
