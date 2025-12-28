use crate::core::ecs::{archetype, storage::row::Row};

/// The location an entity is stored in the ECS. This is made of of the entity's archetype and
/// and table row. This is intended to create constant time lookups for entities within the world's
/// storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// The archetype the entity belongs to.
    archetype_id: archetype::Id,

    /// The table row the entity is stored at.
    row: Row,
}

impl Location {
    /// Create a new Location with the given archetype ID and table row.
    #[inline]
    pub const fn new(archetype_id: archetype::Id, row: Row) -> Self {
        Self { archetype_id, row }
    }

    /// Get the archetype ID for this location.
    #[inline]
    pub fn archetype_id(&self) -> archetype::Id {
        self.archetype_id
    }

    /// Get the table row for this location.
    #[inline]
    pub fn row(&self) -> Row {
        self.row
    }
}
