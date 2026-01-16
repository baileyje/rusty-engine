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
        // Pre-allocate capacity to avoid reallocation
        let mut ids = Vec::with_capacity(self.ids.len() + other.ids.len());
        ids.extend_from_slice(&self.ids);
        ids.extend_from_slice(&other.ids);
        Self::new(ids)
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
}
