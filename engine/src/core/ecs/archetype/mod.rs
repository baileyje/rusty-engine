use std::hash::Hash;

use crate::core::ecs::{component, storage};

mod registry;

pub use registry::Registry;

/// A unique identifier for an Archetype in the ECS (Entity Component System).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(u32);

impl Id {
    /// Create a new Id with the given unique identifier.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Id(id)
    }

    /// Get the unique identifier of the Id.
    #[inline]
    pub fn id(&self) -> u32 {
        self.0
    }

    /// Get the index of the Id as a usize to be used in collections.
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

/// An Archetype represents a collection of entities with a unique combination of components.
pub struct Archetype {
    /// The archetype's unique identifier.
    id: Id,

    /// The id for the table that contains storage for this archetype.
    table_id: storage::table::Id,

    /// The components that make up this archetype.
    components: component::Spec,
}

impl Archetype {
    /// Create a new Archetype with the given archetype ID
    #[inline]
    pub const fn new(id: Id, components: component::Spec, table_id: storage::table::Id) -> Self {
        Self {
            id,
            table_id,
            components,
        }
    }

    /// Get the Id of this archetype.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Get the storage table identifier for this archetype.
    #[inline]
    pub fn table_id(&self) -> storage::table::Id {
        self.table_id
    }

    /// Get the component specification of this archetype.
    #[inline]
    pub fn components(&self) -> &component::Spec {
        &self.components
    }

    /// Determines whether this archetype supports the provided component specification.
    #[inline]
    pub fn supports(&self, spec: &component::Spec) -> bool {
        self.components.contains_all(spec)
    }
}
