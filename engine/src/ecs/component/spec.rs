use crate::{
    all_tuples,
    ecs::{component::Component, world},
};

/// A specification for the components required for an entity or archetype.
/// This is a sorted vector of component IDs that can be used as a Hash key
/// to identify unique component combinations.
///
/// It should also be used to determine if the owner (Entity, Archetype, etc.) has some specified
/// component attached to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    ids: Vec<world::TypeId>,
}

impl Spec {
    /// An empty component specification.
    pub const EMPTY: Self = Spec { ids: Vec::new() };

    /// Construct a new Spec from the given component IDs.
    #[inline]
    pub fn new(ids: impl Into<Vec<world::TypeId>>) -> Self {
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
    pub fn ids(&self) -> &[world::TypeId] {
        &self.ids
    }

    /// Determine if this specification contains the given component ID.
    #[inline]
    pub fn contains(&self, id: world::TypeId) -> bool {
        // Binary search since the IDs are sorted.
        self.ids.binary_search(&id).is_ok()
    }

    /// Determine if this specification contains all component IDs in the other specification.
    #[inline]
    pub fn contains_all(&self, other: &Spec) -> bool {
        other.ids.iter().all(|id| self.contains(*id))
    }

    /// Determine if this specification contains any component IDs in the other specification.
    #[inline]
    pub fn contains_any(&self, other: &Spec) -> bool {
        other.ids.iter().any(|id| self.contains(*id))
    }

    /// Merge this specification with another, returning a new specification containing
    /// the union of both component ID sets.
    #[inline]
    pub fn merge(&self, other: &Spec) -> Self {
        // Pre-allocate capacity to avoid reallocation
        let mut ids = Vec::with_capacity(self.ids.len() + other.ids.len());
        ids.extend_from_slice(&self.ids);
        ids.extend_from_slice(&other.ids);
        Self::new(ids)
    }

    /// Create a new spec that is the union with the othen spec.
    #[inline]
    pub fn union(&self, other: &Spec) -> Self {
        let mut ids = Vec::with_capacity(self.ids.len() + other.ids.len());
        ids.extend_from_slice(&self.ids);
        ids.extend_from_slice(&other.ids);
        Self::new(ids)
    }

    /// Get the components in self that are not in other (set difference).
    #[inline]
    pub fn difference(&self, other: &Spec) -> Self {
        let ids: Vec<_> = self
            .ids
            .iter()
            .copied()
            .filter(|id| !other.contains(*id))
            .collect();
        Self { ids } // Already sorted
    }

    /// Get the components in both self and other (set intersection).
    #[inline]
    pub fn intersection(&self, other: &Spec) -> Self {
        let ids: Vec<_> = self
            .ids
            .iter()
            .copied()
            .filter(|id| other.contains(*id))
            .collect();
        Self { ids } // Already sorted
    }

    /// Returns true if this spec is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Returns the number of component IDs in this spec.
    #[inline]
    pub fn len(&self) -> usize {
        self.ids.len()
    }
}

impl From<Vec<world::TypeId>> for Spec {
    #[inline]
    fn from(value: Vec<world::TypeId>) -> Self {
        Spec::new(value)
    }
}

/// Trait for converting a type into a component specification (`Spec`).
pub trait IntoSpec<Marker = ()> {
    /// Convert the type into a component specification using the given registry.
    fn into_spec(registry: &world::TypeRegistry) -> Spec;
}

/// [`IntoSpec`] implementation for the empty tuple.
impl IntoSpec for () {
    /// Convert the empty tuple into an empty Spec.
    fn into_spec(_registry: &world::TypeRegistry) -> Spec {
        Spec::EMPTY
    }
}

/// [`IntoSpec`] implementation for single component types.
impl<C: Component> IntoSpec for C {
    fn into_spec(registry: &world::TypeRegistry) -> Spec {
        Spec::new([registry.register_component::<C>()])
    }
}

/// [`IntoSpec`] implementation for tuples of other [`IntoSpec`] types.
macro_rules! tuple_spec {
    ($($name: ident),*) => {
        impl<$($name: IntoSpec),*> IntoSpec for ($($name,)*) {
            fn into_spec(registry: &world::TypeRegistry) -> Spec {
                let mut ids = Vec::new();
                $(
                    ids.extend(<$name>::into_spec(registry).ids());
                )*
                Spec::new(ids)
            }
        }
    }
}

// Implement the tuple -> Spec for all tuples up to 26 elements.
all_tuples!(tuple_spec);

#[cfg(test)]
mod tests {
    use rusty_macros::Component;
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::ecs::{component::Spec, world};

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
        let registry = world::TypeRegistry::new();

        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

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
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        // When
        let spec = Spec::new(vec![id2, id1, id3, id2, id1]);

        // Then
        assert_eq!(spec.ids(), &[id1, id2, id3]);
    }

    #[test]
    fn test_component_hash() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

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
    fn contains() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let spec = Spec::new(vec![id2, id1]);

        // Then
        assert!(spec.contains(id1));
        assert!(spec.contains(id2));
        assert!(!spec.contains(id3));
    }

    #[test]
    fn contains_all() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();
        let id4 = registry.register_component::<Comp4>();

        let spec1 = Spec::new(vec![id1, id2, id3]);
        let spec2 = Spec::new(vec![id1, id2]);
        let spec3 = Spec::new(vec![id1, id4]);

        // Then
        assert!(spec1.contains_all(&spec2));
        assert!(spec1.contains_all(&spec1));
        assert!(!spec1.contains_all(&spec3));
    }

    #[test]
    fn with_adds_new_component() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();

        let spec = Spec::new(vec![id1, id2]);
        let other = Spec::new(vec![id3]);

        // When
        let new_spec = spec.union(&other);

        // Then
        assert_eq!(new_spec.ids().len(), 3);
        assert!(new_spec.contains(id1));
        assert!(new_spec.contains(id2));
        assert!(new_spec.contains(id3));
        // Original unchanged
        assert_eq!(spec.ids().len(), 2);
    }

    #[test]
    fn with_existing_component_returns_same() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();

        let spec = Spec::new(vec![id1, id2]);
        let other = Spec::new(vec![id1, id2]);

        // When
        let new_spec = spec.union(&other);

        // Then
        assert_eq!(new_spec, spec);
    }

    #[test]
    fn difference_returns_components_not_in_other() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();
        let id4 = registry.register_component::<Comp4>();

        let spec1 = Spec::new(vec![id1, id2, id3]);
        let spec2 = Spec::new(vec![id2, id4]);

        // When
        let diff = spec1.difference(&spec2);

        // Then - id1 and id3 are in spec1 but not spec2
        assert_eq!(diff.ids().len(), 2);
        assert!(diff.contains(id1));
        assert!(diff.contains(id3));
        assert!(!diff.contains(id2));
        assert!(!diff.contains(id4));
    }

    #[test]
    fn intersection_returns_common_components() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();
        let id2 = registry.register_component::<Comp2>();
        let id3 = registry.register_component::<Comp3>();
        let id4 = registry.register_component::<Comp4>();

        let spec1 = Spec::new(vec![id1, id2, id3]);
        let spec2 = Spec::new(vec![id2, id3, id4]);

        // When
        let inter = spec1.intersection(&spec2);

        // Then - id2 and id3 are common
        assert_eq!(inter.ids().len(), 2);
        assert!(inter.contains(id2));
        assert!(inter.contains(id3));
        assert!(!inter.contains(id1));
        assert!(!inter.contains(id4));
    }

    #[test]
    fn is_empty_and_len() {
        // Given
        let registry = world::TypeRegistry::new();
        let id1 = registry.register_component::<Comp1>();

        // Then
        assert!(Spec::EMPTY.is_empty());
        assert_eq!(Spec::EMPTY.len(), 0);

        let spec = Spec::new(vec![id1]);
        assert!(!spec.is_empty());
        assert_eq!(spec.len(), 1);
    }
}
