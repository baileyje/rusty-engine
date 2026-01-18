//! Storage change/migration types for the ECS.
//!
//! This module provides a unified abstraction for storage mutations:
//! - `Spawn`: Add an entity with components to a table
//! - `Despawn`: Remove an entity from a table
//! - `Migrate`: Move an entity between tables (component add/remove)

use crate::ecs::{
    component::Set,
    entity,
    storage::{row::Row, table, Table},
    world,
};

/// Trait for applying components to a table, used for type-erased component sets.
///
/// This enables `Change::Spawn` and `Change::Migrate` to work with any `Set` type
/// while being stored as a trait object.
pub trait ApplyOnce: Send {
    /// Apply the components to the target table.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - The table has the correct columns for the components being applied
    /// - The table has reserved space for the new row
    fn apply_once(self: Box<Self>, target: &mut Table, registry: &world::TypeRegistry);
}

/// Blanket implementation of `ApplyOnce` for any `Set` type.
impl<S: Set + Send> ApplyOnce for S {
    fn apply_once(self: Box<Self>, target: &mut Table, _registry: &world::TypeRegistry) {
        // Use the existing Set::apply mechanism
        // Table implements SetTarget (Target), so this works directly
        (*self).apply(target);
    }
}

/// Source location for a migration operation.
#[derive(Debug, Clone, Copy)]
pub struct MigrationSource {
    /// The source table ID.
    pub table: table::Id,
    /// The row in the source table.
    pub row: Row,
}

impl MigrationSource {
    /// Create a new migration source.
    pub fn new(table: table::Id, row: Row) -> Self {
        Self { table, row }
    }
}

/// A storage change operation.
///
/// Changes are created by the World and executed by Storage.
/// Each change represents a single atomic modification to the storage layer.
pub enum Change<'a> {
    /// Spawn: Add an entity with components to a single table.
    Spawn {
        /// The entity being spawned.
        entity: entity::Entity,
        /// The target table ID.
        table: table::Id,
        /// The components to add (Option allows .take() during batch execution).
        components: Option<Box<dyn ApplyOnce + 'a>>,
    },

    /// Despawn: Remove an entity from a single table.
    Despawn {
        /// The entity being despawned.
        entity: entity::Entity,
        /// The table containing the entity.
        table: table::Id,
        /// The row in the table.
        row: Row,
    },

    /// Migrate: Move an entity between tables (for component add/remove).
    Migrate {
        /// The entity being migrated.
        entity: entity::Entity,
        /// The source table and row.
        source: MigrationSource,
        /// The target table ID.
        target: table::Id,
        /// New components to add (None for component removal).
        additions: Option<Box<dyn ApplyOnce + 'a>>,
    },
}

impl<'a> Change<'a> {
    /// Create a spawn change.
    pub fn spawn<S: Set + Send + 'a>(
        entity: entity::Entity,
        table: table::Id,
        components: S,
    ) -> Self {
        Change::Spawn {
            entity,
            table,
            components: Some(Box::new(components)),
        }
    }

    /// Create a despawn change.
    pub fn despawn(entity: entity::Entity, table: table::Id, row: Row) -> Self {
        Change::Despawn { entity, table, row }
    }

    /// Create a migration change (for component removal).
    pub fn migrate(entity: entity::Entity, source: MigrationSource, target: table::Id) -> Self {
        Change::Migrate {
            entity,
            source,
            target,
            additions: None,
        }
    }

    /// Create a migration change with new components (for component addition).
    pub fn migrate_with<S: Set + Send + 'a>(
        entity: entity::Entity,
        source: MigrationSource,
        target: table::Id,
        additions: S,
    ) -> Self {
        Change::Migrate {
            entity,
            source,
            target,
            additions: Some(Box::new(additions)),
        }
    }
}

/// Result of executing a storage change.
///
/// Contains information needed to update entity registries and handle
/// entities that were relocated during swap-remove operations.
#[derive(Debug)]
pub enum ChangeResult {
    /// Result of a spawn operation.
    Spawned {
        /// The row where the entity was placed.
        row: Row,
    },

    /// Result of a despawn operation.
    Despawned {
        /// Entity that was moved to fill the gap (if any).
        /// This entity's location needs to be updated in the entity registry.
        moved_entity: Option<entity::Entity>,
    },

    /// Result of a migration operation.
    Migrated {
        /// The row in the target table where the entity was placed.
        new_row: Row,
        /// Entity that was moved in the source table to fill the gap (if any).
        source_moved: Option<entity::Entity>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_macros::Component;

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
    fn change_spawn_creation() {
        let entity = entity::Entity::new(1.into());
        let table_id = table::Id::new(0);
        let components = (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 });

        let change = Change::spawn(entity, table_id, components);

        match change {
            Change::Spawn {
                entity: e,
                table: t,
                components: c,
            } => {
                assert_eq!(e, entity);
                assert_eq!(t, table_id);
                assert!(c.is_some());
            }
            _ => panic!("Expected Spawn variant"),
        }
    }

    #[test]
    fn change_despawn_creation() {
        let entity = entity::Entity::new(1.into());
        let table_id = table::Id::new(0);
        let row = Row::new(5);

        let change = Change::despawn(entity, table_id, row);

        match change {
            Change::Despawn {
                entity: e,
                table: t,
                row: r,
            } => {
                assert_eq!(e, entity);
                assert_eq!(t, table_id);
                assert_eq!(r, row);
            }
            _ => panic!("Expected Despawn variant"),
        }
    }

    #[test]
    fn change_migrate_creation() {
        let entity = entity::Entity::new(1.into());
        let source = MigrationSource::new(table::Id::new(0), Row::new(3));
        let target = table::Id::new(1);

        let change = Change::migrate(entity, source, target);

        match change {
            Change::Migrate {
                entity: e,
                source: s,
                target: t,
                additions: a,
            } => {
                assert_eq!(e, entity);
                assert_eq!(s.table, table::Id::new(0));
                assert_eq!(s.row, Row::new(3));
                assert_eq!(t, target);
                assert!(a.is_none());
            }
            _ => panic!("Expected Migrate variant"),
        }
    }

    #[test]
    fn change_migrate_with_creation() {
        let entity = entity::Entity::new(1.into());
        let source = MigrationSource::new(table::Id::new(0), Row::new(3));
        let target = table::Id::new(1);
        let additions = Velocity { dx: 1.0, dy: 2.0 };

        let change = Change::migrate_with(entity, source, target, additions);

        match change {
            Change::Migrate {
                entity: e,
                source: s,
                target: t,
                additions: a,
            } => {
                assert_eq!(e, entity);
                assert_eq!(s.table, table::Id::new(0));
                assert_eq!(s.row, Row::new(3));
                assert_eq!(t, target);
                assert!(a.is_some());
            }
            _ => panic!("Expected Migrate variant"),
        }
    }
}
