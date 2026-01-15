use std::{
    any::TypeId,
    sync::RwLock,
    sync::atomic::{AtomicU32, Ordering},
};

use dashmap::DashMap;

use crate::ecs::component::{Component, Id, Info, IntoSpec, Spec};

/// A thread-safe component registry. This is responsible for managing component types and their
/// identifiers within the ECS.
///
/// The registry uses lock-free reads for TypeId→ComponentId lookups via `DashMap`, making the
/// common read path highly performant. Component registration uses minimal locking - only a
/// single shard of the DashMap and a write lock for the component info vector.
///
/// Why thread-safe?
/// - Most things in the ECS are not thread-safe, but different worlds may be created in their own threads, and all worlds need to agree on component IDs.
pub struct Registry {
    /// Map from TypeId to component Id. Lock-free reads via sharded concurrent hashmap.
    type_map: DashMap<TypeId, Id>,

    /// List of registered component entries. Protected by RwLock for rare writes.
    components: RwLock<Vec<Option<Info>>>,

    /// Next available component identifier.
    next_id: AtomicU32,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Create a new component registry.
    #[inline]
    pub fn new() -> Self {
        Self {
            type_map: DashMap::new(),
            components: RwLock::new(Vec::new()),
            next_id: AtomicU32::new(0),
        }
    }

    /// Register a new component type and get its unique identifier.
    ///
    /// This method is thread-safe and can be called concurrently. If the component type is
    /// already registered, returns the existing ID. Otherwise, allocates a new ID and stores
    /// the component info.
    pub fn register<C: Component + 'static>(&self) -> Id {
        let type_id = TypeId::of::<C>();

        // Fast path: check if already registered (lock-free read)
        if let Some(id) = self.type_map.get(&type_id) {
            return *id;
        }

        // Slow path: need to register
        // Use entry API to avoid race condition where two threads both miss the cache
        *self
            .type_map
            .entry(type_id)
            .or_insert_with(|| {
                // Generate a new unique identifier
                let id_value = self.next_id.fetch_add(1, Ordering::Relaxed);
                let comp_id = Id(id_value);

                // Add entry to the components list
                let mut components = self.components.write().unwrap();
                let index = id_value as usize;

                // Expand if necessary
                if index >= components.len() {
                    components.resize(index + 1, None);
                }

                components[index] = Some(Info::new::<C>(comp_id));

                comp_id
            })
            .value()
    }

    /// Get the component ID for a provided type `C`, if registered.
    ///
    /// Performance:
    /// - Uses lock-free read to get ID from TypeId.
    #[inline]
    pub fn get<C: Component + 'static>(&self) -> Option<Id> {
        let type_id = TypeId::of::<C>();
        self.type_map.get(&type_id).map(|entry| *entry.value())
    }

    /// Get the component info for a provided type `C`, if registered.
    ///
    /// Performance:
    /// - Uses lock-free read to get ID from TypeId.
    /// - Uses read lock to access component info vector.
    #[inline]
    pub fn get_info<C: Component + 'static>(&self) -> Option<Info> {
        let id = self.get::<C>()?;
        self.get_info_by_id(id)
    }

    /// Get component info by ID.
    ///
    /// Performance:
    /// - Uses read lock to access component info vector.
    #[inline]
    pub fn get_info_by_id(&self, id: Id) -> Option<Info> {
        let components = self.components.read().unwrap();
        components.get(id.index()).and_then(|i| *i)
    }

    /// Get a component specification for a generic type `IS` which implements [`IntoSpec`].
    #[inline]
    pub fn spec<IS: IntoSpec>(&self) -> Spec {
        IS::into_spec(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn component_registration() {
        // Given
        #[derive(Component, Debug)]
        struct Position();

        #[derive(Component, Debug)]
        struct Velocity();

        let registry = Registry::new();

        // When
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();

        // Then
        assert_ne!(pos_id, vel_id);
        assert_eq!(registry.components.read().unwrap().len(), 2);
        assert_eq!(
            *registry.type_map.get(&TypeId::of::<Position>()).unwrap(),
            pos_id
        );
        assert_eq!(
            *registry.type_map.get(&TypeId::of::<Velocity>()).unwrap(),
            vel_id
        );

        // Then - Registering the same type again should result in the same id
        assert_eq!(registry.register::<Position>(), pos_id);
    }

    #[test]
    fn component_id_retrieval() {
        // Given
        #[derive(Component, Debug)]
        struct Health();

        let registry = Registry::new();
        let health_id = registry.register::<Health>();

        // When
        let retrieved = registry.get::<Health>().unwrap();

        // Then
        assert_eq!(health_id, retrieved);

        // When - Retrieving a non-registered component
        #[derive(Component, Debug)]
        struct Mana();
        let non_existent_id = registry.get::<Mana>();

        // Then
        assert!(non_existent_id.is_none());
    }

    #[test]
    fn component_info_retrieval() {
        // Given
        #[derive(Component, Debug)]
        struct Health();

        let registry = Registry::new();
        let health_id = registry.register::<Health>();

        // When
        let retrieved = registry.get_info::<Health>().unwrap();

        // Then
        assert_eq!(health_id, retrieved.id());

        // When - Retrieving a non-registered component
        #[derive(Component, Debug)]
        struct Mana();
        let non_existent_id = registry.get_info::<Mana>();

        // Then
        assert!(non_existent_id.is_none());
    }

    #[test]
    fn concurrent_registration() {
        // Given
        #[derive(Component, Debug)]
        struct Position();

        #[derive(Component, Debug)]
        struct Velocity();

        #[derive(Component, Debug)]
        struct Health();

        let registry = Arc::new(Registry::new());

        // When - Multiple threads register components concurrently
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let registry = Arc::clone(&registry);
                thread::spawn(move || {
                    if i % 3 == 0 {
                        registry.register::<Position>()
                    } else if i % 3 == 1 {
                        registry.register::<Velocity>()
                    } else {
                        registry.register::<Health>()
                    }
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Then - All threads that registered the same type should get the same ID
        let pos_ids: Vec<_> = results.iter().step_by(3).copied().collect();
        let vel_ids: Vec<_> = results.iter().skip(1).step_by(3).copied().collect();
        let health_ids: Vec<_> = results.iter().skip(2).step_by(3).copied().collect();

        assert!(pos_ids.iter().all(|&id| id == pos_ids[0]));
        assert!(vel_ids.iter().all(|&id| id == vel_ids[0]));
        assert!(health_ids.iter().all(|&id| id == health_ids[0]));

        // And all three types have different IDs
        assert_ne!(pos_ids[0], vel_ids[0]);
        assert_ne!(pos_ids[0], health_ids[0]);
        assert_ne!(vel_ids[0], health_ids[0]);
    }

    #[test]
    fn concurrent_read_after_write() {
        // Given
        #[derive(Component, Debug)]
        struct Position();

        let registry = Arc::new(Registry::new());
        let id = registry.register::<Position>();

        // When - Multiple threads read concurrently
        let handles: Vec<_> = (0..100)
            .map(|_| {
                let registry = Arc::clone(&registry);
                thread::spawn(move || registry.get::<Position>())
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Then - All reads should return the same ID
        assert!(results.iter().all(|&r| r == Some(id)));
    }
}
