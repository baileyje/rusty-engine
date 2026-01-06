use crate::core::ecs::{
    component::{self, Component},
    entity::Entity,
    storage,
};

/// A reference to an entity with read-only access to its components.
/// The lifetime `'w` ensures that the reference does not outlive the storage and archetype it
/// points to. Generally this should be tied to the lifetime of the `World`.
pub struct Ref<'w> {
    /// The entity this reference points to.
    entity: Entity,
    /// The registry of known components.
    components: &'w component::Registry,
    /// The table that stores this entity's components.
    table: &'w storage::Table,
    /// The row this table occupies in the table.
    row: storage::Row,
}

impl<'w> Ref<'w> {
    /// Create a new Ref for an entity and its storage table.
    #[inline]
    pub const fn new(
        entity: Entity,
        components: &'w component::Registry,
        table: &'w storage::Table,
        row: storage::Row,
    ) -> Self {
        Self {
            entity,
            components,
            table,
            row,
        }
    }

    /// Get a reference to a component on this entity.
    /// Returns `None` if the component is not registered or not present on the entity.
    pub fn get<C: Component>(&self) -> Option<&C> {
        let component_id = self.components.get::<C>()?;
        unsafe { self.table.get(self.row, component_id) }
    }

    /// Get the entity this reference points to.
    #[inline]
    pub fn entity(&self) -> Entity {
        self.entity
    }

    /// Get the component specification for the referenced entity.
    #[inline]
    pub fn components(&self) -> &component::Spec {
        self.table.components()
    }
}

/// A mutable reference to an entity with read-only access to its components.
/// The lifetime `'w` ensures that the reference does not outlive the storage and archetype it
/// points to. Generally this should be tied to the lifetime of the `World`.
///
/// # Warning - This current holds a mutable reference to the table backing this entity. This may
/// not be ideal for most use-cases once systems are available.
///
pub struct RefMut<'w> {
    /// The entity this reference points to.
    entity: Entity,
    /// The registry of known components.
    components: &'w component::Registry,
    /// The table that stores this entity's components.
    table: &'w mut storage::Table,
    /// The row this table occupies in the table.
    row: storage::Row,
}

impl<'w> RefMut<'w> {
    /// Create a new Ref for an entity and its storage table.
    #[inline]
    pub const fn new(
        entity: Entity,
        components: &'w component::Registry,
        table: &'w mut storage::Table,
        row: storage::Row,
    ) -> Self {
        Self {
            entity,
            components,
            table,
            row,
        }
    }

    /// Get a reference to a component on this entity.
    /// Returns `None` if the component is not registered or not present on the entity.
    pub fn get<C: Component>(&self) -> Option<&C> {
        let component_id = self.components.get::<C>()?;
        unsafe { self.table.get(self.row, component_id) }
    }

    /// Get a mutable reference to a component on this entity.
    /// Returns `None` if the component is not registered or not present on the entity.
    pub fn get_mut<C: Component>(&mut self) -> Option<&mut C> {
        let component_id = self.components.get::<C>()?;
        unsafe { self.table.get_mut(self.row, component_id) }
    }

    /// Get the entity this reference points to.
    #[inline]
    pub fn entity(&self) -> Entity {
        self.entity
    }

    /// Get the component specification for the referenced entity.
    #[inline]
    pub fn components(&self) -> &component::Spec {
        self.table.components()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ecs::{
        component::{self},
        entity::{self},
        storage::{Table, table},
    };
    use rusty_macros::Component;

    #[derive(Component, Debug, Clone, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Debug, Clone, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Component, Debug, Clone, PartialEq)]
    struct Health {
        hp: i32,
    }

    #[test]
    fn ref_get_existing_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();

        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let velocity = Velocity { dx: 1.0, dy: 2.0 };

        let row = table.add_entity(entity, (position.clone(), velocity.clone()), &registry);

        // When
        let entity_ref = Ref::new(entity, &registry, &table, row);
        let retrieved_pos = entity_ref.get::<Position>();

        // Then
        assert!(retrieved_pos.is_some());
        assert_eq!(retrieved_pos.unwrap(), &position);
    }

    #[test]
    fn ref_get_nonexistent_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();

        let spec = component::Spec::new(vec![pos_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let row = table.add_entity(entity, (position,), &registry);

        // When - Try to get component not in table
        let entity_ref = Ref::new(entity, &registry, &table, row);
        let retrieved_vel = entity_ref.get::<Velocity>();

        // Then
        assert!(retrieved_vel.is_none());
    }

    #[test]
    fn ref_get_unregistered_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        // Note: Health is NOT registered

        let spec = component::Spec::new(vec![pos_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let row = table.add_entity(entity, (position,), &registry);

        // When - Try to get unregistered component
        let entity_ref = Ref::new(entity, &registry, &table, row);
        let retrieved_health = entity_ref.get::<Health>();

        // Then
        assert!(retrieved_health.is_none());
    }

    #[test]
    fn ref_get_multiple_components() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();
        let health_id = registry.register::<Health>();

        let spec = component::Spec::new(vec![pos_id, vel_id, health_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 5.0, y: 15.0 };
        let velocity = Velocity { dx: -1.0, dy: 3.0 };
        let health = Health { hp: 100 };

        let row = table.add_entity(
            entity,
            (position.clone(), velocity.clone(), health.clone()),
            &registry,
        );

        // When
        let entity_ref = Ref::new(entity, &registry, &table, row);

        // Then - Can get all components
        assert_eq!(entity_ref.get::<Position>(), Some(&position));
        assert_eq!(entity_ref.get::<Velocity>(), Some(&velocity));
        assert_eq!(entity_ref.get::<Health>(), Some(&health));
    }

    #[test]
    fn ref_for_nonexistent_entity() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();

        let spec = component::Spec::new(vec![pos_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        table.add_entity(entity1, (position,), &registry);

        // When - Create ref for entity2 which is NOT in the table
        let entity_ref = Ref::new(entity2, &registry, &table, 1.into());
        let retrieved_pos = entity_ref.get::<Position>();

        // Then
        assert!(retrieved_pos.is_none());
    }

    #[test]
    fn ref_mut_get_existing_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();

        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let velocity = Velocity { dx: 1.0, dy: 2.0 };

        let row = table.add_entity(entity, (position.clone(), velocity.clone()), &registry);

        // When
        let entity_ref = RefMut::new(entity, &registry, &mut table, row);
        let retrieved_pos = entity_ref.get::<Position>();

        // Then
        assert!(retrieved_pos.is_some());
        assert_eq!(retrieved_pos.unwrap(), &position);
    }

    #[test]
    fn ref_mut_get_nonexistent_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();

        let spec = component::Spec::new(vec![pos_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let row = table.add_entity(entity, (position,), &registry);

        // When - Try to get component not in table
        let entity_ref = RefMut::new(entity, &registry, &mut table, row);
        let retrieved_vel = entity_ref.get::<Velocity>();

        // Then
        assert!(retrieved_vel.is_none());
    }

    #[test]
    fn ref_mut_get_mut_existing_component() {
        // Given
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();

        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let position = Position { x: 10.0, y: 20.0 };
        let velocity = Velocity { dx: 1.0, dy: 2.0 };

        let row = table.add_entity(entity, (position.clone(), velocity.clone()), &registry);

        // When
        let mut entity_ref = RefMut::new(entity, &registry, &mut table, row);
        let retrieved_pos = entity_ref.get_mut::<Position>().unwrap();
        retrieved_pos.x = 9.0;
        retrieved_pos.y = 19.0;

        // Then
        let retrieved_pos = entity_ref.get::<Position>().unwrap();
        assert_eq!(retrieved_pos.x, 9.0);
        assert_eq!(retrieved_pos.y, 19.0);
    }
}
