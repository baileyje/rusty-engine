use std::collections::HashMap;

use crate::core::ecs::{
    archetype::{Archetype, Id},
    component,
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

    /// Get an existing archetype matching the given component spec, or create a new one if none
    /// exists.
    pub fn get_or_create(&mut self, spec: component::Spec) -> &Archetype {
        // If we alrady have an archetype for this spec, return it.
        if let Some(archetype_id) = self.by_components.get(&spec) {
            return &self.archetypes[archetype_id.index()];
        }

        // Add a new archetype with the next valid index.
        let archetype_id = Id(self.archetypes.len() as u32);
        // Add a new archetype to storage.
        self.archetypes
            .push(Archetype::new(archetype_id, spec.clone()));
        // Update the components -> archetype map.
        self.by_components.insert(spec, archetype_id);
        // Return the reference.
        &self.archetypes[archetype_id.index()]
    }

    /// Get an archetype by its archetypeId.
    #[inline]
    pub fn get(&self, archetype_id: Id) -> Option<&Archetype> {
        self.archetypes.get(archetype_id.index())
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::core::ecs::{archetype::Registry, component};

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
            .get_or_create(component::Spec::new([id1, id2]))
            .id();
        let arch2 = registry
            .get_or_create(component::Spec::new([id1, id3]))
            .id();
        let arch3 = registry
            .get_or_create(component::Spec::new([id1, id2, id3]))
            .id();

        // Then
        assert_ne!(arch1, arch2);
        assert_ne!(arch1, arch3);
        assert_ne!(arch2, arch3);

        // And When
        let arch4 = registry
            .get_or_create(component::Spec::new([id1, id3]))
            .id();

        // Then
        assert_eq!(arch2, arch4);
    }
}
