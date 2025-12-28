use crate::core::ecs::component::Id;

/// A specification for the components required for an entity or archetype.
/// This is a sorted vector of component IDs that can be used as a Hash key
/// to identify unique component combinations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    ids: Vec<Id>,
}

impl Spec {
    /// Construct a new Spec from the given component IDs.
    #[inline]
    pub fn new(ids: impl Into<Vec<Id>>) -> Self {
        let mut ids = ids.into();
        ids.sort();
        Self { ids }
    }

    /// Get the component IDs in this specification.
    #[inline]
    pub fn ids(&self) -> &[Id] {
        &self.ids
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::core::ecs::component::{Registry, Spec};

    #[test]
    fn test_component_id_order() {
        // Given
        #[derive(Component)]
        pub struct Comp1 {}
        #[derive(Component)]
        pub struct Comp2 {}
        #[derive(Component)]
        pub struct Comp3 {}

        let registry = Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();

        // When
        let ids1 = Spec::new(vec![id2, id1, id3]);
        let ids2 = Spec::new(vec![id1, id2, id3]);

        // Then
        assert_eq!(ids1, ids2);
        let mut hasher1 = DefaultHasher::new();
        ids1.hash(&mut hasher1);
        let mut hasher2 = DefaultHasher::new();
        ids2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }
}
