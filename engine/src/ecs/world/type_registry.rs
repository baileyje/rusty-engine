//! Unified type registry for all typed world data.
//!
//! This module provides [`TypeRegistry`], a thread-safe registry that manages type IDs
//! for both components and uniques (singletons). Each type gets a single numeric ID
//! used for access control, ensuring efficient bitset-based conflict detection.
//!
//! # Design
//!
//! The registry enforces **mutual exclusion**: a type can be registered as either a
//! component OR a unique, but not both. This simplifies the access control system
//! to use a single bitset pair for all type-based access.
//!
//! # Thread Safety
//!
//! The registry uses lock-free reads via `DashMap` and minimal locking for writes.
//! Multiple worlds can share the same registry to ensure consistent IDs across threads.
//!
//! # Example
//!
//! ```rust,ignore
//! let registry = TypeRegistry::new();
//!
//! // Register a component
//! let pos_id = registry.register_component::<Position>()?;
//!
//! // Register a unique
//! let time_id = registry.register_unique::<GameTime>()?;
//!
//! // Attempting to register Position as unique would fail
//! assert!(registry.register_unique::<Position>().is_err());
//! ```

use std::{
    alloc::Layout,
    any::TypeId as StdTypeId,
    fmt,
    ptr::NonNull,
    sync::{
        RwLock,
        atomic::{AtomicU32, Ordering},
    },
};

use dashmap::DashMap;

use crate::ecs::storage::index::SparseId;

/// The kind of type registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// A component type (attached to entities, many instances).
    Component,
    /// A unique type (singleton, one instance per world).
    Unique,
}

impl fmt::Display for TypeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeKind::Component => write!(f, "component"),
            TypeKind::Unique => write!(f, "unique"),
        }
    }
}

/// A unique identifier for a registered type.
///
/// This ID is shared between components and uniques, enabling a unified access control
/// system with a single bitset pair.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId(u32);

impl TypeId {
    /// Construct a new Id from a raw u32 value.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the index of this ID for use in indexable storage (e.g., Vec, bitset).
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for TypeId {
    #[inline]
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<usize> for TypeId {
    #[inline]
    fn from(value: usize) -> Self {
        Self::new(value as u32)
    }
}

impl SparseId for TypeId {
    fn index(&self) -> usize {
        self.0 as usize
    }
}

/// Metadata about a registered type.
///
/// Contains the information needed to work with type-erased storage:
/// memory layout, drop function, and registration details.
#[derive(Debug, Clone, Copy)]
pub struct TypeInfo {
    /// The unique type ID.
    id: TypeId,

    /// What kind of type this is (component or unique).
    kind: TypeKind,

    /// The Rust TypeId for runtime type checking.
    type_id: StdTypeId,

    /// The memory layout of the type.
    layout: Layout,

    /// The drop function for the type (may be a no-op).
    drop_fn: unsafe fn(NonNull<u8>),
}

impl TypeInfo {
    /// Construct TypeInfo for type `T`.
    fn new<T: 'static>(id: TypeId, kind: TypeKind) -> Self {
        let drop_fn = if std::mem::needs_drop::<T>() {
            Self::drop_impl::<T>
        } else {
            Self::drop_noop
        };
        Self {
            id,
            kind,
            type_id: StdTypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop_fn,
        }
    }

    /// Get the type ID.
    #[inline]
    pub fn id(&self) -> TypeId {
        self.id
    }

    /// Get the kind of type (component or unique).
    #[inline]
    pub fn kind(&self) -> TypeKind {
        self.kind
    }

    /// Get the Rust TypeId.
    #[inline]
    pub fn type_id(&self) -> StdTypeId {
        self.type_id
    }

    /// Get the memory layout.
    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Check if this is a zero-sized type.
    #[inline]
    pub fn is_zero_sized(&self) -> bool {
        self.layout.size() == 0
    }

    /// Get the drop function.
    #[inline]
    pub fn drop_fn(&self) -> unsafe fn(NonNull<u8>) {
        self.drop_fn
    }

    /// Drop implementation for types that need drop.
    unsafe fn drop_impl<T>(ptr: NonNull<u8>) {
        unsafe {
            std::ptr::drop_in_place(ptr.as_ptr() as *mut T);
        }
    }

    /// No-op drop for types that don't need drop.
    unsafe fn drop_noop(_ptr: NonNull<u8>) {}
}

/// A thread-safe registry for all typed world data.
///
/// The registry manages type IDs for both components and uniques, ensuring each type
/// gets a single unique ID. This enables efficient access control using a single
/// bitset pair rather than separate sets for components and uniques.
///
/// # Dual-Use Prevention
///
/// A type cannot be registered as both a component and a unique. Attempting to do so
/// returns a [`DualUseError`]. This constraint enables the simplified access control
/// model.
pub struct TypeRegistry {
    /// Map from Rust TypeId to our Id. Lock-free reads via sharded concurrent hashmap.
    type_map: DashMap<StdTypeId, TypeId>,

    /// List of registered type entries. Protected by RwLock for rare writes.
    types: RwLock<Vec<Option<TypeInfo>>>,

    /// Next available type identifier.
    next_id: AtomicU32,
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeRegistry {
    /// Create a new, empty type registry.
    #[inline]
    pub fn new() -> Self {
        Self {
            type_map: DashMap::new(),
            types: RwLock::new(Vec::new()),
            next_id: AtomicU32::new(0),
        }
    }

    /// Register a type as a component.
    ///
    /// Returns the type's ID, or an error if the type is already registered as a unique.
    ///
    /// If the type is already registered as a component, returns the existing ID.
    ///
    /// Panics if the type is already registered as a unique.
    pub fn register_component<T: 'static>(&self) -> TypeId {
        self.register::<T>(TypeKind::Component)
    }

    /// Register a type as a unique.
    ///
    /// Returns the type's ID, or an error if the type is already registered as a component.
    ///
    /// If the type is already registered as a unique, returns the existing ID.
    ///
    /// Panics if the type is already registered as a component.
    pub fn register_unique<T: 'static>(&self) -> TypeId {
        self.register::<T>(TypeKind::Unique)
    }

    /// Internal registration logic.
    ///
    /// Panics if the type is already registered as a different kind.
    fn register<T: 'static>(&self, kind: TypeKind) -> TypeId {
        let std_type_id = StdTypeId::of::<T>();

        // Fast path: check if already registered (lock-free read)
        if let Some(existing_id) = self.type_map.get(&std_type_id) {
            let id = *existing_id;
            // Verify the kind matches
            let types = self.types.read().unwrap();
            if let Some(Some(info)) = types.get(id.index())
                && info.kind() != kind
            {
                panic!(
                    "type '{}' is already registered as a {}, cannot register as {}",
                    std::any::type_name::<T>(),
                    info.kind(),
                    kind
                )
            }
            return id;
        }

        // Slow path: need to register
        // Use entry API to handle race conditions
        let entry = self.type_map.entry(std_type_id);

        match entry {
            dashmap::Entry::Occupied(occupied) => {
                // Another thread registered it first - verify kind matches
                let id = *occupied.get();
                let types = self.types.read().unwrap();
                if let Some(Some(info)) = types.get(id.index())
                    && info.kind() != kind
                {
                    panic!(
                        "type '{}' is already registered as a {}, cannot register as {}",
                        std::any::type_name::<T>(),
                        info.kind(),
                        kind
                    )
                }
                id
            }
            dashmap::Entry::Vacant(vacant) => {
                // We get to register it
                let id_value = self.next_id.fetch_add(1, Ordering::Relaxed);
                let id = TypeId(id_value);

                // Add entry to the types list
                let mut types = self.types.write().unwrap();
                let index = id_value as usize;

                // Expand if necessary
                if index >= types.len() {
                    types.resize(index + 1, None);
                }

                types[index] = Some(TypeInfo::new::<T>(id, kind));
                vacant.insert(id);

                id
            }
        }
    }

    /// Get the ID for a type, if registered.
    #[inline]
    pub fn get<T: 'static>(&self) -> Option<TypeId> {
        self.type_map
            .get(&StdTypeId::of::<T>())
            .map(|entry| *entry.value())
    }

    /// Get the ID for a type if registered as a component.
    ///
    /// Returns `None` if not registered or registered as a unique.
    #[inline]
    pub fn get_component<T: 'static>(&self) -> Option<TypeId> {
        self.get_if_kind::<T>(TypeKind::Component)
    }

    /// Get the ID for a type if registered as a unique.
    ///
    /// Returns `None` if not registered or registered as a component.
    #[inline]
    pub fn get_unique<T: 'static>(&self) -> Option<TypeId> {
        self.get_if_kind::<T>(TypeKind::Unique)
    }

    /// Get the ID for a type if it matches the specified kind.
    fn get_if_kind<T: 'static>(&self, expected_kind: TypeKind) -> Option<TypeId> {
        let id = self.get::<T>()?;
        let types = self.types.read().unwrap();
        types
            .get(id.index())
            .and_then(|opt| opt.as_ref())
            .filter(|info| info.kind() == expected_kind)
            .map(|info| info.id())
    }

    /// Get type info by ID.
    #[inline]
    pub fn get_info(&self, id: TypeId) -> Option<TypeInfo> {
        let types = self.types.read().unwrap();
        types.get(id.index()).and_then(|opt| *opt)
    }

    /// Get type info for a type, if registered.
    #[inline]
    pub fn get_info_of<T: 'static>(&self) -> Option<TypeInfo> {
        let id = self.get::<T>()?;
        self.get_info(id)
    }

    /// Get the kind of a registered type by ID.
    #[inline]
    pub fn kind(&self, id: TypeId) -> Option<TypeKind> {
        self.get_info(id).map(|info| info.kind())
    }

    /// Get the number of registered types.
    #[inline]
    pub fn len(&self) -> usize {
        self.next_id.load(Ordering::Relaxed) as usize
    }

    /// Check if the registry is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    struct Position {
        #[allow(dead_code)]
        x: f32,
        #[allow(dead_code)]
        y: f32,
    }

    struct Velocity {
        #[allow(dead_code)]
        dx: f32,
        #[allow(dead_code)]
        dy: f32,
    }

    struct GameTime {
        #[allow(dead_code)]
        elapsed: f32,
    }

    // ==================== Basic Registration ====================

    #[test]
    fn register_component() {
        // Given
        let registry = TypeRegistry::new();

        // When
        let id = registry.register_component::<Position>();

        // Then
        assert_eq!(registry.get::<Position>(), Some(id));
        assert_eq!(registry.kind(id), Some(TypeKind::Component));
    }

    #[test]
    fn register_unique() {
        // Given
        let registry = TypeRegistry::new();

        // When
        let id = registry.register_unique::<GameTime>();

        // Then
        assert_eq!(registry.get::<GameTime>(), Some(id));
        assert_eq!(registry.kind(id), Some(TypeKind::Unique));
    }

    #[test]
    fn register_same_component_twice_returns_same_id() {
        // Given
        let registry = TypeRegistry::new();
        // When
        let id1 = registry.register_component::<Position>();
        let id2 = registry.register_component::<Position>();
        // then
        assert_eq!(id1, id2);
    }

    #[test]
    fn register_same_unique_twice_returns_same_id() {
        // Given
        let registry = TypeRegistry::new();

        // When
        let id1 = registry.register_unique::<GameTime>();
        let id2 = registry.register_unique::<GameTime>();

        // Then
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_types_get_different_ids() {
        // Given
        let registry = TypeRegistry::new();

        // When
        let pos_id = registry.register_component::<Position>();
        let vel_id = registry.register_component::<Velocity>();
        let time_id = registry.register_unique::<GameTime>();

        // Then
        assert_ne!(pos_id, vel_id);
        assert_ne!(pos_id, time_id);
        assert_ne!(vel_id, time_id);
    }

    // ==================== Dual-Use Prevention ====================

    #[test]
    #[should_panic(
        expected = "Position' is already registered as a component, cannot register as unique"
    )]
    fn dual_use_component_then_unique_fails() {
        // Given
        let registry = TypeRegistry::new();
        // When
        registry.register_component::<Position>();
        registry.register_unique::<Position>();
    }

    #[test]
    #[should_panic(
        expected = "GameTime' is already registered as a unique, cannot register as component"
    )]
    fn dual_use_unique_then_component_fails() {
        // Given
        let registry = TypeRegistry::new();

        // When
        registry.register_unique::<GameTime>();
        registry.register_component::<GameTime>();
    }

    // ==================== Type Info ====================

    #[test]
    fn type_info_available_after_registration() {
        // Given
        let registry = TypeRegistry::new();
        let id = registry.register_component::<Position>();

        // When
        let info = registry.get_info(id).unwrap();

        // Then
        assert_eq!(info.id(), id);
        assert_eq!(info.kind(), TypeKind::Component);
        assert_eq!(info.type_id(), StdTypeId::of::<Position>());
        assert_eq!(info.layout(), Layout::new::<Position>());
    }

    #[test]
    fn get_info_of_type() {
        // Given
        let registry = TypeRegistry::new();
        registry.register_unique::<GameTime>();

        // When
        let info = registry.get_info_of::<GameTime>().unwrap();

        // Then
        assert_eq!(info.kind(), TypeKind::Unique);
    }

    #[test]
    fn get_component_returns_none_for_unique() {
        // Given
        let registry = TypeRegistry::new();
        // When
        registry.register_unique::<GameTime>();

        // Then
        assert!(registry.get_component::<GameTime>().is_none());
    }

    #[test]
    fn get_unique_returns_none_for_component() {
        // Given
        let registry = TypeRegistry::new();
        // When
        registry.register_component::<Position>();
        // Then
        assert!(registry.get_unique::<Position>().is_none());
    }

    // ==================== Concurrent Registration ====================

    #[test]
    fn concurrent_registration_same_type() {
        // Given
        let registry = Arc::new(TypeRegistry::new());

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let registry = Arc::clone(&registry);
                thread::spawn(move || registry.register_component::<Position>())
            })
            .collect();

        // When
        let ids = handles
            .into_iter()
            .map(|h| h.join())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(ids.iter().all(|&id| id == ids[0]));
    }

    #[test]
    fn concurrent_registration_different_types() {
        // Given
        let registry = Arc::new(TypeRegistry::new());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let registry = Arc::clone(&registry);
                thread::spawn(move || {
                    if i % 3 == 0 {
                        registry.register_component::<Position>()
                    } else if i % 3 == 1 {
                        registry.register_component::<Velocity>()
                    } else {
                        registry.register_unique::<GameTime>()
                    }
                })
            })
            .collect();

        // When
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Then
        // Each type should have consistent ID
        let pos_ids: Vec<_> = results.iter().step_by(3).collect();
        let vel_ids: Vec<_> = results.iter().skip(1).step_by(3).collect();
        let time_ids: Vec<_> = results.iter().skip(2).step_by(3).collect();

        assert!(pos_ids.iter().all(|r| *r == pos_ids[0]));
        assert!(vel_ids.iter().all(|r| *r == vel_ids[0]));
        assert!(time_ids.iter().all(|r| *r == time_ids[0]));

        // All three types should have different IDs
        assert_ne!(pos_ids[0], vel_ids[0]);
        assert_ne!(pos_ids[0], time_ids[0]);
    }

    // ==================== Drop Function ====================

    #[test]
    fn drop_function_is_called() {
        // Given
        use std::sync::atomic::{AtomicBool, Ordering};

        static DROP_CALLED: AtomicBool = AtomicBool::new(false);

        struct DropTracker;

        impl Drop for DropTracker {
            fn drop(&mut self) {
                DROP_CALLED.store(true, Ordering::Relaxed);
            }
        }

        let registry = TypeRegistry::new();
        let id = registry.register_component::<DropTracker>();
        let info = registry.get_info(id).unwrap();

        // Allocate and initialize
        let layout = Layout::new::<DropTracker>();
        let ptr = unsafe { std::alloc::alloc(layout) };
        assert!(!ptr.is_null());

        let ptr = NonNull::new(ptr).unwrap();
        unsafe {
            std::ptr::write(ptr.as_ptr() as *mut DropTracker, DropTracker);
        }

        // When
        // Call the drop function
        unsafe {
            (info.drop_fn())(ptr);
        }

        // Deallocate
        unsafe {
            std::alloc::dealloc(ptr.as_ptr(), layout);
        }

        // Then
        assert!(DROP_CALLED.load(Ordering::Relaxed));
    }

    // ==================== Utility Methods ====================

    #[test]
    fn len_and_is_empty() {
        // Given
        let registry = TypeRegistry::new();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        // When
        registry.register_component::<Position>();
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);

        // Then
        registry.register_unique::<GameTime>();
        assert_eq!(registry.len(), 2);
    }
}
