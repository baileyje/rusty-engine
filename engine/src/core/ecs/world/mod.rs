use crate::core::ecs::{
    archetype,
    component::{self},
    entity,
    storage::{self},
};

/// A world  identifier. This is a non-zero unique identifier for a world in the ECS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

#[derive()]
pub struct World {
    /// The world's unique identifier.
    id: Id,

    /// The world's entity allocator.
    entity_allocator: entity::Allocator,

    /// The storage entities in the world.
    entities: entity::Registry,

    /// The component registry for the world.
    componnents: component::Registry,

    /// The storage for components in the world.
    storage: storage::Storage,

    // Teh archtype registry for the world.
    archetypes: archetype::Registry,
}
impl World {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            entity_allocator: entity::Allocator::default(),
            entities: entity::Registry::default(),
            componnents: component::Registry::default(),
            storage: storage::Storage::default(),
            archetypes: archetype::Registry::default(),
        }
    }

    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Spawn a new entity with the given set of components in the world.
    /// This will establish the entity in the appropriate archetype and storage table.
    pub fn spawn<S: component::Set>(&mut self, set: S) -> entity::Entity {
        // Allocate a new entity.
        let entity = self.entity_allocator.alloc();

        // Construct the component specification for this set of components.
        let spec = S::spec(&mut self.componnents);

        // Get the archetype for this set of components.
        let archetype = self.archetypes.get_or_create(spec.clone());

        // Get the table for the entities archetype
        let table = self
            .storage
            .get_or_create_table(archetype, &mut self.componnents);

        // Add the enitty with all the componnents
        let row = table.add_entity(entity, set, &mut self.componnents);

        // Mark the entity as spawned in the world.
        self.entities
            .spawn_at(entity, storage::Location::new(archetype.id(), row));

        entity
    }

    /// Despawn the given entity from the world. This will remove the entity and all its components
    /// from storage.
    pub fn despawn(&mut self, entity: entity::Entity) {
        // Remove the entity from its archetype table.
        if let Some((table, row)) = self.storage_for_mut(entity) {
            // TODO: We need to update
            // its location in the entity storage.
            let _moved = table.swap_remove_row(row);
        }
        self.entities.despawn(entity);
    }

    /// Get a reference to the given entity.
    pub fn entity(&self, entity: entity::Entity) -> Option<entity::Ref<'_>> {
        self.storage_for(entity)
            .map(|(table, row)| entity::Ref::new(entity, &self.componnents, table, row))
    }

    /// Get the storage table and row for a reference to the given entity.
    fn storage_for(&self, entity: entity::Entity) -> Option<(&storage::Table, storage::Row)> {
        self.entities.location(entity).and_then(|loc| {
            self.storage
                .get(loc.archetype_id())
                .map(|table| (table, loc.row()))
        })
    }

    /// Get the storage table and row for a mutable reference to the given entity.
    fn storage_for_mut(
        &mut self,
        entity: entity::Entity,
    ) -> Option<(&mut storage::Table, storage::Row)> {
        self.entities.location(entity).and_then(|loc| {
            self.storage
                .get_mut(loc.archetype_id())
                .map(|table| (table, loc.row()))
        })
    }
}

#[cfg(test)]
mod test {
    use rusty_macros::Component;

    use crate::core::ecs::world::{Id, World};

    #[test]
    fn spwan_empty_entity() {
        // Given
        let mut world = World::new(Id(0));
        // When
        let entity = world.spawn(());
        // Then
        assert!(world.entities.is_spawned(entity));
    }

    #[test]
    fn spawn_entity_with_components() {
        // Given
        let mut world = World::new(Id(0));

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

        // TODO: Verify components are stored correctly.
    }

    #[test]
    fn despawn_entity_with_components() {
        // Given
        let mut world = World::new(Id(0));

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
    }
}
