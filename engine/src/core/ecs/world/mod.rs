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
//! use rusty_engine::core::ecs::world::World;
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

use crate::core::ecs::{
    archetype::{self},
    component::{self},
    entity,
    query::{self},
    storage::{self},
};

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

    /// The stored entities in the world.
    entities: entity::Registry,

    /// The component registry for the world.
    components: component::Registry,

    /// The storage for components in the world.
    storage: storage::Storage,

    /// The archetype registry for the world.
    archetypes: archetype::Registry,
}
impl World {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            entity_allocator: entity::Allocator::default(),
            entities: entity::Registry::default(),
            components: component::Registry::default(),
            storage: storage::Storage::default(),
            archetypes: archetype::Registry::default(),
        }
    }

    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    #[inline]
    pub fn components(&self) -> &component::Registry {
        &self.components
    }

    #[inline]
    pub fn archetypes(&self) -> &archetype::Registry {
        &self.archetypes
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
    pub fn spawn<S: component::Set>(&mut self, set: S) -> entity::Entity {
        // Allocate a new entity.
        let entity = self.entity_allocator.alloc();

        // Construct the component specification for this set of components.
        let spec = S::spec(&self.components);

        // Get the table for the entity's archetype
        let table = self
            .storage
            .get_or_create_table(spec.clone(), &self.components);

        // Get the archetype for this set of components.
        let archetype = self.archetypes.get_or_create(spec, table.id());

        // Add the entity with all the components
        let row = table.add_entity(entity, set, &self.components);

        // Mark the entity as spawned in the world.
        self.entities.spawn_at(
            entity,
            storage::Location::new(archetype.id(), table.id(), row),
        );

        entity
    }

    /// Despawn the given entity from the world. This will remove the entity and all its components
    /// from storage.
    ///
    /// If the entity is not currently spawned, this method does nothing.
    pub fn despawn(&mut self, entity: entity::Entity) {
        // Only despawn if the entity is currently spawned
        if !self.entities.is_spawned(entity) {
            return;
        }

        // Remove the entity from its archetype table.
        if let Some((table, row)) = self.storage_for_mut(entity) {
            // Remove the entity from the table using swap-remove, which moves the last entity
            // in the table to fill the gap.
            let moved = table.swap_remove_row(row);
            // If an entity was moved into this row, update its location in the registry.
            if let Some(moved_entity) = moved
                && let Some(loc) = self.entities.location(moved_entity)
            {
                self.entities.set_location(
                    moved_entity,
                    storage::Location::new(loc.archetype_id(), loc.table_id(), row),
                );
            }
        }
        self.entities.despawn(entity);
    }

    /// Get the storage location for the given entity, if it's spawned.
    pub fn location_for(&self, entity: entity::Entity) -> Option<storage::Location> {
        self.entities.location(entity)
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
    ///
    /// This method holds a mutable reference to the entire world's storage, preventing
    /// any other access while the `RefMut` is held. For performance-critical code,
    /// consider using queriss/systems that can access multiple entities efficiently.
    pub fn entity_mut(&mut self, entity: entity::Entity) -> Option<entity::RefMut<'_>> {
        let loc = self.location_for(entity)?;
        let table = self.storage.get_mut(loc.table_id());
        Some(entity::RefMut::new(entity, table, loc.row()))
    }

    /// Get the storage table and row for a reference to the given entity, if the entity is spawned.
    pub fn storage_for(&self, entity: entity::Entity) -> Option<(&storage::Table, storage::Row)> {
        let loc = self.location_for(entity)?;
        Some((self.storage.get(loc.table_id()), loc.row()))
    }

    /// Get the storage table and row for a mutable reference to the given entity, if the entity is
    /// spawned.
    pub fn storage_for_mut(
        &mut self,
        entity: entity::Entity,
    ) -> Option<(&mut storage::Table, storage::Row)> {
        let loc = self.location_for(entity)?;
        Some((self.storage.get_mut(loc.table_id()), loc.row()))
    }

    /// Perform a world query to access all entities that match the query data `D`.
    ///
    ///
    /// Note: This holds a mutable reference to the entire world while the query result is active
    /// (use wisely).
    pub fn query<'w, D: query::Data>(&'w mut self) -> query::Result<'w, D> {
        query::Query::<D>::one_shot(self)
    }
}

#[cfg(test)]
mod test {
    use rusty_macros::Component;

    use crate::core::ecs::world::{Id, World};

    #[test]
    fn spawn_empty_entity() {
        // Given
        let mut world = World::new(Id(1));
        // When
        let entity = world.spawn(());
        // Then
        assert!(world.entities.is_spawned(entity));
    }

    #[test]
    fn spawn_entity_with_components() {
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
        assert!(world.entities.is_spawned(entity));

        let entity_ref = world.entity(entity).unwrap();

        assert_eq!(Comp1 { value: 42 }, *entity_ref.get::<Comp1>().unwrap());
        assert_eq!("Hello", entity_ref.get::<Comp2>().unwrap().value.as_str());
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
        assert!(world.entities.is_spawned(entity));

        // And When
        world.despawn(entity);

        // Then
        assert!(!world.entities.is_spawned(entity));
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
        assert_eq!(world.entities.location(entity1).unwrap().row(), 0.into());

        let entity2 = world.spawn(Comp1);
        // Confirm entity2 is at row 1
        assert_eq!(world.entities.location(entity2).unwrap().row(), 1.into());

        // And When
        world.despawn(entity1);

        // Then
        assert!(!world.entities.is_spawned(entity1));

        // Confirm entity2 is now at row 0
        assert_eq!(world.entities.location(entity2).unwrap().row(), 0.into());

        // Confirm entity2 is still spawned
        assert!(world.entities.is_spawned(entity2));

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
        assert!(!world.entities.is_spawned(entity1));
    }

    #[test]
    fn entity_ref_access() {
        let mut world = World::new(Id(1));

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

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
        assert!(world.entities.is_spawned(e1));
        assert!(world.entities.is_spawned(e2));
        assert!(world.entities.is_spawned(e3));
        assert!(world.entities.is_spawned(e4));

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
        assert!(!world.entities.is_spawned(entity1));
        assert!(world.entities.is_spawned(entity2));
    }
}
