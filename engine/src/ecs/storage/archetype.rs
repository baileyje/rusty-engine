use std::{collections::HashMap, hash::Hash};

use crate::ecs::{component, storage};

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
    table_id: storage::TableId,

    /// The components that make up this archetype.
    components: component::Spec,
}

impl Archetype {
    /// Create a new Archetype with the given archetype ID
    #[inline]
    pub const fn new(id: Id, components: component::Spec, table_id: storage::TableId) -> Self {
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
    pub fn table_id(&self) -> storage::TableId {
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

/// Central registry of archetypes.
#[derive(Default)]
pub struct Archetypes {
    /// The archetypes stored by their unique identifier
    archetypes: Vec<Archetype>,

    /// The archetypes indexed by their component specifications
    by_components: HashMap<component::Spec, Id>,
}

impl Archetypes {
    /// Create an empty archetype registry.
    #[inline]
    pub fn new() -> Self {
        Self {
            archetypes: Vec::new(),
            by_components: HashMap::new(),
        }
    }

    /// Create a new archetype with the given spec.
    ///
    /// Node, calling create does not check for existing archetypes with the same spec as it the
    /// table Id may differ. The caller is responsible for ensuring archetype uniqueness and this
    /// is currently handled in teh world when creating new archetypes.
    ///
    /// TODO: Consider adding a check to prevent duplicate archetypes with the same spec, but
    /// determoining how to handle the error secenario.
    pub fn create(&mut self, spec: component::Spec, table_id: storage::TableId) -> Id {
        // Add a new archetype with the next valid index.
        let archetype_id = Id(self.archetypes.len() as u32);
        // Add to map by components (requires one clone for HashMap key)
        self.by_components.insert(spec.clone(), archetype_id);
        // Add a new archetype to storage (moves spec)
        self.archetypes
            .push(Archetype::new(archetype_id, spec, table_id));
        archetype_id
    }

    /// Get an archetype by its component specification, if it exists.
    #[inline]
    pub fn get_by_spec(&self, spec: &component::Spec) -> Option<&Archetype> {
        self.by_components.get(spec).and_then(|id| self.get(*id))
    }

    /// Get an archetype by its archetype Id.
    #[inline]
    pub fn get(&self, archetype_id: Id) -> Option<&Archetype> {
        self.archetypes.get(archetype_id.index())
    }

    /// Get a mutable archetype by its archetype Id, if it exists.
    #[inline]
    pub fn get_mut(&mut self, archetype_id: Id) -> Option<&mut Archetype> {
        self.archetypes.get_mut(archetype_id.index())
    }

    /// Get an archetype by its archetype Id without an existence check.
    ///
    /// # Safety
    /// - Caller must ensure the provided archetype_id exists in the registry.
    #[inline]
    pub unsafe fn get_unchecked(&self, archetype_id: Id) -> &Archetype {
        unsafe { self.archetypes.get_unchecked(archetype_id.index()) }
    }

    /// Get a mutable archetype by its archetype Id without an existence check.
    ///
    /// # Safety
    /// - Caller must ensure the provided archetype_id exists in the registry.
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, archetype_id: Id) -> &mut Archetype {
        unsafe { self.archetypes.get_unchecked_mut(archetype_id.index()) }
    }

    /// Get the table IDs for archetypes that support the provided component specification.
    ///     
    /// This does not have to be an exact match; any archetype that contains all components in the
    /// spec `supports` the spec. This is useful for querying archetypes that can fulfill a set of
    /// query parameters.
    pub fn table_ids_for(&self, spec: &component::Spec) -> Vec<storage::TableId> {
        self.archetypes
            .iter()
            .filter(|a| a.supports(spec))
            .map(|a| a.table_id())
            .collect()
    }
}
