use std::collections::HashMap;

use crate::ecs::{
    archetype::{Archetype, Id},
    component, storage,
};

/// Central registry of archetypes.
#[derive(Default)]
pub struct Registry {
    /// The archetypes stored by their unique identifier
    archetypes: Vec<Archetype>,

    /// The archetypes indexed by their component specifications
    by_components: HashMap<component::Spec, Id>,
}

impl Registry {
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
    pub fn create(&mut self, spec: component::Spec, table_id: storage::table::Id) -> Id {
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
    pub fn table_ids_for(&self, spec: &component::Spec) -> Vec<storage::table::Id> {
        self.archetypes
            .iter()
            .filter(|a| a.supports(spec))
            .map(|a| a.table_id())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::ecs::{archetype::Registry, component, storage, world};

    #[test]
    fn arch_registry_create() {
        // Given
        let type_registry = world::TypeRegistry::new();
        let mut registry = Registry::new();

        #[derive(Component)]
        pub struct Comp1 {}
        #[derive(Component)]
        pub struct Comp2 {}
        #[derive(Component)]
        pub struct Comp3 {}

        let id1 = type_registry.register_component::<Comp1>();
        let id2 = type_registry.register_component::<Comp2>();
        let id3 = type_registry.register_component::<Comp3>();

        // When
        let arch1 = registry
            .create(component::Spec::new([id1, id2]), storage::table::Id::new(0))
            .id();
        let arch2 = registry
            .create(component::Spec::new([id1, id3]), storage::table::Id::new(1))
            .id();
        let arch3 = registry
            .create(
                component::Spec::new([id1, id2, id3]),
                storage::table::Id::new(2),
            )
            .id();

        // Then
        assert_ne!(arch1, arch2);
        assert_ne!(arch1, arch3);
        assert_ne!(arch2, arch3);
    }
}
