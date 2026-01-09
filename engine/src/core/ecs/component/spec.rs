use crate::core::ecs::component::Id;

/// A specification for the components required for an entity or archetype.
/// This is a sorted vector of component IDs that can be used as a Hash key
/// to identify unique component combinations.
///
/// It should also be used to determine if the owner (Entity, Archetype, etc.) has some specified
/// component attached to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    ids: Vec<Id>,
}

impl Spec {
    /// Construct a new Spec from the given component IDs.
    #[inline]
    pub fn new(ids: impl Into<Vec<Id>>) -> Self {
        let mut ids = ids.into();
        // Ensure the IDs are sorted.
        ids.sort();
        // Remove any duplicates. Perhaps this should be an error instead?
        ids.dedup();
        // Trim any excess capacity.
        ids.shrink_to_fit();

        Self { ids }
    }

    /// Get the component IDs in this specification.
    #[inline]
    pub fn ids(&self) -> &[Id] {
        &self.ids
    }

    /// Determine if this specification contains the given component ID.
    #[inline]
    pub fn contains(&self, id: Id) -> bool {
        // Binary search since the IDs are sorted.
        self.ids.binary_search(&id).is_ok()
    }

    /// Determine if this specification contains all component IDs in the other specification.
    #[inline]
    pub fn contains_all(&self, other: &Spec) -> bool {
        for id in &other.ids {
            if !self.contains(*id) {
                return false;
            }
        }
        true
    }

    /// Merge this specification with another, returning a new specification containing
    /// the union of both component ID sets.
    #[inline]
    pub fn merge(&self, other: &Spec) -> Self {
        let mut ids = self.ids.clone();
        ids.extend_from_slice(&other.ids);
        Self::new(ids)
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::core::ecs::component::{Registry, Spec};

    // Given
    #[derive(Component)]
    pub struct Comp1;
    #[derive(Component)]
    pub struct Comp2;
    #[derive(Component)]
    pub struct Comp3;
    #[derive(Component)]
    pub struct Comp4;

    #[test]
    fn component_id_order() {
        // Given
        let registry = Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();

        // When
        let spec1 = Spec::new(vec![id2, id1, id3]);
        let spec2 = Spec::new(vec![id1, id2, id3]);

        // Then
        assert_eq!(spec1, spec2);
        let mut hasher1 = DefaultHasher::new();
        spec1.hash(&mut hasher1);
        let mut hasher2 = DefaultHasher::new();
        spec2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[test]
    fn component_id_dedupe() {
        // Given
        let registry = Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();

        // When
        let spec = Spec::new(vec![id2, id1, id3, id2, id1]);

        // Then
        assert_eq!(spec.ids(), &[id1, id2, id3]);
    }

    #[test]
    fn contains() {
        // Given
        let registry = Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();

        let spec = Spec::new(vec![id2, id1]);

        // Then
        assert!(spec.contains(id1));
        assert!(spec.contains(id2));
        assert!(!spec.contains(id3));
    }

    #[test]
    fn contains_all() {
        // Given
        let registry = Registry::new();
        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();
        let id4 = registry.register::<Comp4>();

        let spec1 = Spec::new(vec![id1, id2, id3]);
        let spec2 = Spec::new(vec![id1, id2]);
        let spec3 = Spec::new(vec![id1, id4]);

        // Then
        assert!(spec1.contains_all(&spec2));
        assert!(spec1.contains_all(&spec1));
        assert!(!spec1.contains_all(&spec3));
    }
}
