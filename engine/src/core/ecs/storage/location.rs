use crate::core::ecs::{
    archetype,
    storage::{Row, table},
};

/// The location an entity is stored in the ECS. This si made of of the entity's table and row.
/// This is intended to create constant time lookups for entities within the world's storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// The archetype id for this entity.
    archetype_id: archetype::Id,

    /// The table the entity belongs to.
    table_id: table::Id,

    /// The table row the entity is stored at.
    row: Row,
}

impl Location {
    /// Create a new Location with the given archetype,table and row.
    #[inline]
    pub const fn new(archetype_id: archetype::Id, table_id: table::Id, row: Row) -> Self {
        Self {
            archetype_id,
            table_id,
            row,
        }
    }

    /// Get the archetype ID for this location.
    #[inline]
    pub fn archetype_id(&self) -> archetype::Id {
        self.archetype_id
    }

    /// Get the table ID for this location.
    #[inline]
    pub fn table_id(&self) -> table::Id {
        self.table_id
    }

    /// Get the table row for this location.
    #[inline]
    pub fn row(&self) -> Row {
        self.row
    }
}
