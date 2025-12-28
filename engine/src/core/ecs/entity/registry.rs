use log::warn;

use crate::core::ecs::{
    archetype,
    entity::{Entity, Generation},
    storage,
};

/// The state of an entity in the world. If the entity is Spawned, then it holds the current
/// generation.
#[derive(Debug, Default, Clone, Copy)]
enum State {
    /// A spawned entity and the current generation.
    Spawned(Generation, storage::Location),
    /// An entity that is not-spawned, but may have been in the past.
    #[default]
    Unspawned,
}

/// The central entry of an entity in the world. By default it will be unspawned, with no
/// archetype ID set.
#[derive(Debug, Default, Clone, Copy)]
struct Entry {
    /// The current state of the entity in the world.
    state: State,
}

/// The collection of all known entities in the world. This tracks whether an entity is spawned and
/// if spawned, where they are located in terms of archetype.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    entries: Vec<Entry>,
}

impl Registry {
    /// Construct a new empty entity entries collection.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Spawn an entity into the entries with a known archetype ID, ensuring capacity.
    pub fn spawn_at(&mut self, entity: Entity, location: storage::Location) {
        let index = entity.index();
        // Ensure capacity for the entity index.
        self.ensure_capacity(index);
        // Set the index to spawned with with the current generation.
        self.entries[index].state = State::Spawned(entity.generation, location);
    }

    /// Mark the given entity as despawned in the entries. Return `true` if the entity was
    /// actually despawned, `false` if the entity was not spawned.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if !self.is_spawned(entity) {
            warn!(
                "Attempted to despawn an entity that is not spawned: {:?}",
                entity
            );
            return false;
        }
        let index = entity.index();
        // Ensure capacity for the entity index.
        self.ensure_capacity(index);
        // Set the index to unspawned.
        self.entries[index].state = State::Unspawned;
        true
    }

    /// Determine if the given entity is currently spawned in the world.
    pub fn is_spawned(&self, entity: Entity) -> bool {
        self.spawned_state(entity).is_some()
    }

    /// Get the location for a spawned entity, or `None` if the entity is not spawned.
    pub fn location(&self, entity: Entity) -> Option<storage::Location> {
        self.spawned_state(entity).map(|(_, location)| location)
    }

    /// Get the archetype ID for a spawned entity, or `None` if the entity is not spawned.
    pub fn archetype(&self, entity: Entity) -> Option<archetype::Id> {
        self.location(entity).map(|loc| loc.archetype_id())
    }

    /// Set the storage location for a spawned entity. If the entity is not spawned, this will panic.
    /// Non-spawned entities can be spawned with their location using `spawn_at`.
    ///
    /// # Panics
    /// - If the entity index is not valid
    /// - If the entity is not spawned
    pub fn set_location(&mut self, entity: Entity, location: storage::Location) {
        if let Some(entry) = self.entry_mut(entity)
            && let State::Spawned(generation, _) = entry.state
        {
            entry.state = State::Spawned(generation, location);
        } else {
            panic!(
                "attempted to set archetype for an entity that is not spawned: {:?}",
                entity
            );
        }
    }

    /// Get the current state of the given entity, if it exists and the generation matches.
    fn spawned_state(&self, entity: Entity) -> Option<(Generation, storage::Location)> {
        self.state(entity).and_then(|s| match s {
            State::Spawned(generation, location) => {
                if entity.generation == generation {
                    Some((generation, location))
                } else {
                    None
                }
            }
            _ => None,
        })
    }

    /// Get the current state of the given entity, if it exists.
    fn state(&self, entity: Entity) -> Option<State> {
        self.entry(entity).map(|entry| entry.state)
    }

    /// Get a reference to the entry for the given entity, if it exists.
    fn entry(&self, entity: Entity) -> Option<&Entry> {
        let index = entity.index();
        self.entries.get(index)
    }

    /// Get a mutable reference to the entry for the given entity, if it exists.
    fn entry_mut(&mut self, entity: Entity) -> Option<&mut Entry> {
        let index = entity.index();
        self.entries.get_mut(index)
    }

    /// Ensure the entries have capacity for the given index.
    #[inline]
    fn ensure_capacity(&mut self, index: usize) {
        let capacity = index + 1;
        if capacity >= self.entries.len() {
            self.entries.resize(capacity, Entry::default());
        }
    }
}

#[cfg(test)]
mod test {

    use crate::core::ecs::{
        archetype,
        entity::{Allocator, Entity, Id, Registry},
        storage::{self},
    };

    #[test]
    fn entity_spawn_check() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype_id = archetype::Id::new(42);

        // When - just allocated and not spawned
        let entity = allocator.alloc();

        // Then
        assert!(!entities.is_spawned(entity));

        // When - spawned
        entities.spawn_at(entity, storage::Location::new(archetype_id, 0.into()));

        // Then
        assert!(entities.is_spawned(entity));

        // When - despawned
        entities.despawn(entity);

        // Then
        assert!(!entities.is_spawned(entity));
    }

    #[test]
    fn get_location() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype_id = archetype::Id::new(42);
        let entity = allocator.alloc();
        let location = storage::Location::new(archetype_id, 0.into());

        // When
        entities.spawn_at(entity, location);

        // Then
        assert_eq!(entities.location(entity), Some(location));
    }

    #[test]
    fn get_archetype() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype_id = archetype::Id::new(42);
        let entity = allocator.alloc();
        let location = storage::Location::new(archetype_id, 0.into());

        // When
        entities.spawn_at(entity, location);

        // Then
        assert_eq!(entities.archetype(entity), Some(archetype_id));
    }

    #[test]
    fn set_location() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype1 = archetype::Id::new(1);
        let archetype2 = archetype::Id::new(2);
        let entity = allocator.alloc();
        let location1 = storage::Location::new(archetype1, 0.into());
        let location2 = storage::Location::new(archetype2, 1.into());
        entities.spawn_at(entity, location1);

        // When
        entities.set_location(entity, location2);

        // Then
        assert_eq!(entities.location(entity), Some(location2));
    }

    #[test]
    #[should_panic(
        expected = "attempted to set archetype for an entity that is not spawned: Entity { id: Id(0), generation: Generation(0) }"
    )]
    fn set_archetype_panics_with_unspawned() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let entity = allocator.alloc();
        let location = storage::Location::new(archetype::Id::new(1), 0.into());

        // When
        entities.set_location(entity, location);
    }

    #[test]
    #[should_panic(
        expected = "attempted to set archetype for an entity that is not spawned: Entity { id: Id(0), generation: Generation(0) }"
    )]
    fn set_archetype_panics_with_despawned() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let entity = allocator.alloc();
        let location = storage::Location::new(archetype::Id::new(1), 0.into());

        entities.spawn_at(entity, location);
        entities.despawn(entity);

        // When
        entities.set_location(entity, location);
    }

    #[test]
    fn storage_handles_sparse_entity_ids() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype = archetype::Id::new(1);

        // When - Create entities with gaps (allocate but don't spawn all)
        let e0 = allocator.alloc(); // Id 0
        let e1 = allocator.alloc(); // Id 1
        let e2 = allocator.alloc(); // Id 2

        // Only spawn e0 and e2 (skip e1)
        entities.spawn_at(e0, storage::Location::new(archetype, 0.into()));
        entities.spawn_at(e2, storage::Location::new(archetype, 1.into()));

        // Then
        assert!(entities.is_spawned(e0));
        assert!(!entities.is_spawned(e1));
        assert!(entities.is_spawned(e2));
    }

    #[test]
    fn storage_capacity_grows() {
        // Given
        let mut entities = Registry::default();

        // When - Spawn entity with high ID
        let high_id_entity = Entity::new(Id(999));
        entities.spawn_at(
            high_id_entity,
            storage::Location::new(archetype::Id::new(1), 0.into()),
        );

        // Then - Storage should have grown to accommodate
        assert!(entities.entries.len() >= 1000);
        assert!(entities.is_spawned(high_id_entity));
    }

    #[test]
    fn despawn_returns_false_for_already_despawned() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let entity = allocator.alloc();
        entities.spawn_at(
            entity,
            storage::Location::new(archetype::Id::new(1), 0.into()),
        );

        // When - Despawn once
        let result1 = entities.despawn(entity);

        // Then - Should succeed
        assert!(result1);

        // When - Try to despawn again
        let result2 = entities.despawn(entity);

        // Then - Should fail
        assert!(!result2);
    }

    #[test]
    fn despawn_returns_false_for_never_spawned() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let entity = allocator.alloc();

        // When - Try to despawn without spawning
        let result = entities.despawn(entity);

        // Then
        assert!(!result);
    }

    #[test]
    fn archetype_of_returns_none_for_wrong_generation() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let location = storage::Location::new(archetype::Id::new(1), 0.into());
        let entity = allocator.alloc();
        entities.spawn_at(entity, location);

        // When - Despawn and free the entity
        entities.despawn(entity);
        allocator.free(entity);

        // When - Reallocate (same ID, incremented generation) and spawn
        let new_entity = allocator.alloc(); // Same ID, different generation
        entities.spawn_at(new_entity, location);

        // Then - Old entity reference should return None (wrong generation)
        assert_eq!(entities.archetype(entity), None);

        // But new entity with correct generation should work
        assert_eq!(entities.location(new_entity), Some(location));
        assert!(entities.is_spawned(new_entity));
        assert!(!entities.is_spawned(entity)); // Old generation not valid
    }

    #[test]
    fn storage_multiple_archetypes() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype1 = archetype::Id::new(1);
        let archetype2 = archetype::Id::new(2);
        let archetype3 = archetype::Id::new(3);

        // When - Spawn entities with different archetypes
        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        let e3 = allocator.alloc();

        entities.spawn_at(e1, storage::Location::new(archetype1, 0.into()));
        entities.spawn_at(e2, storage::Location::new(archetype2, 0.into()));
        entities.spawn_at(e3, storage::Location::new(archetype3, 0.into()));

        // Then
        assert_eq!(entities.archetype(e1), Some(archetype1));
        assert_eq!(entities.archetype(e2), Some(archetype2));
        assert_eq!(entities.archetype(e3), Some(archetype3));
    }

    #[test]
    fn storage_respawn_after_despawn() {
        // Given
        let mut entities = Registry::default();
        let mut allocator = Allocator::default();
        let archetype1 = archetype::Id::new(1);
        let archetype2 = archetype::Id::new(2);
        let entity = allocator.alloc();

        // When - Spawn, despawn, and respawn with different archetype
        entities.spawn_at(entity, storage::Location::new(archetype1, 0.into()));
        assert!(entities.is_spawned(entity));

        entities.despawn(entity);
        assert!(!entities.is_spawned(entity));

        entities.spawn_at(entity, storage::Location::new(archetype2, 0.into()));

        // Then - Should be spawned with new archetype
        assert!(entities.is_spawned(entity));
        assert_eq!(entities.archetype(entity), Some(archetype2));
    }

    #[test]
    fn is_spawned_returns_false_for_nonexistent_entity() {
        // Given
        let entities = Registry::default();
        let entity = Entity::new(Id(999));

        // When/Then
        assert!(!entities.is_spawned(entity));
    }

    #[test]
    fn archetype_of_returns_none_for_nonexistent_entity() {
        // Given
        let entities = Registry::default();
        let entity = Entity::new(Id(999));

        // When/Then
        assert_eq!(entities.archetype(entity), None);
    }
}
