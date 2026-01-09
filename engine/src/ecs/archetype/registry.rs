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
    pub fn create(
        &mut self,
        spec: component::Spec,
        table_id: storage::table::Id,
    ) -> &mut Archetype {
        // Add a new archetype with the next valid index.
        let archetype_id = Id(self.archetypes.len() as u32);
        // Add a new archetype to storage.
        self.archetypes
            .push(Archetype::new(archetype_id, spec.clone(), table_id));

        // Add to map by components
        self.by_components.insert(spec, archetype_id);

        // Safety - We know we just added this.
        unsafe { self.get_unchecked_mut(archetype_id) }
    }

    /// Get an existing archetype matching the given component spec, or create a new one if none
    /// exists.
    pub fn get_or_create(
        &mut self,
        spec: component::Spec,
        table_id: storage::table::Id,
    ) -> &mut Archetype {
        if let Some(id) = self.by_components.get(&spec) {
            // Safety - Any mapped ID must be in the archetypes vec.
            return unsafe { self.get_unchecked_mut(*id) };
        }
        // Otherwise create it
        self.create(spec, table_id)
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

    /// Return any archetype IDs that support the provided component specification.
    pub fn supporting(&self, spec: &component::Spec) -> Vec<Id> {
        self.archetypes
            .iter()
            .filter(|a| a.supports(spec))
            .map(|a| a.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::ecs::{archetype::Registry, component, storage};

    #[test]
    fn test_component_ids() {
        // Given
        pub struct Comp1 {}
        impl component::Component for Comp1 {}
        pub struct Comp2 {}
        impl component::Component for Comp2 {}
        pub struct Comp3 {}
        impl component::Component for Comp3 {}

        let registry = component::Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();

        // When
        let ids1 = component::Spec::new(vec![id2, id1, id3]);
        let ids2 = component::Spec::new(vec![id1, id2, id3]);

        // Then
        assert_eq!(ids1, ids2);
        let mut hasher1 = DefaultHasher::new();
        ids1.hash(&mut hasher1);
        let mut hasher2 = DefaultHasher::new();
        ids2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[test]
    fn arch_registry_get_or_create_reuse() {
        // Given
        let component_registry = component::Registry::new();
        let mut registry = Registry::new();

        #[derive(Component)]
        pub struct Comp1 {}
        #[derive(Component)]
        pub struct Comp2 {}
        #[derive(Component)]
        pub struct Comp3 {}

        let id1 = component_registry.register::<Comp1>();
        let id2 = component_registry.register::<Comp2>();
        let id3 = component_registry.register::<Comp3>();

        // When
        let arch1 = registry
            .get_or_create(component::Spec::new([id1, id2]), storage::table::Id::new(0))
            .id();
        let arch2 = registry
            .get_or_create(component::Spec::new([id1, id3]), storage::table::Id::new(1))
            .id();
        let arch3 = registry
            .get_or_create(
                component::Spec::new([id1, id2, id3]),
                storage::table::Id::new(2),
            )
            .id();

        // Then
        assert_ne!(arch1, arch2);
        assert_ne!(arch1, arch3);
        assert_ne!(arch2, arch3);

        // And When
        let arch4 = registry
            .get_or_create(component::Spec::new([id1, id3]), storage::table::Id::new(1))
            .id();

        // Then
        assert_eq!(arch2, arch4);
    }
}
