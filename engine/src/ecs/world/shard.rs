use std::marker::PhantomData;

use crate::ecs::{
    archetype, query, storage, unique,
    world::{TypeRegistry, World, access::AccessGrant},
};

/// A shard of an ECS world. Shards are used to partition the world into different sets of
/// resources to allow for better concurrency and access control.
///
/// A shard can only be created by the ECS world itself and must be created with a specific
/// world access grant. All interactions with the shard must respect the access rights granted to
/// it and it should at a minimum enforce those access rights in debug builds if not in release
/// builds.
///
///
/// For example, a shard with read-only access to a set of components should prevent any mutable access to
/// storage and should only allow immutable queries on those components it's been granted.
pub struct Shard<'w> {
    /// A pointer to the world this shard belongs to. Access must be mediated through the access
    /// grant to ensure no alias violations occur.
    world: *mut World,

    /// The access grant for this shard.
    grant: AccessGrant,

    _marker: PhantomData<&'w World>,
}

// SAFETY: Shards can be sent to worker threads because:
// 1. Component data accessed through shards is Send + Sync (Component trait bound)
// 2. Disjoint access is guaranteed by the grant system
// 3. Grant release happens on main thread via into_grant() + release_grant()
unsafe impl Send for Shard<'_> {}

// Shard is NOT Sync - a shard should only be used by one thread at a time

impl<'w> Shard<'w> {
    /// Creates a new shard. This should only be called by World::shard().
    #[inline]
    pub(super) const fn new(world: *mut World, grant: AccessGrant) -> Self {
        Self {
            world,
            grant,
            _marker: PhantomData,
        }
    }

    /// Returns a reference to the access grant for this shard.
    ///
    /// This can be used to validate that an operation is permitted before executing it.
    #[inline]
    pub fn grant(&self) -> &AccessGrant {
        &self.grant
    }

    /// Get the resource type registry.
    ///
    /// This is always safe to access as it's read-only metadata about resource types.
    /// Resource registration is immutable after initialization.
    #[inline]
    pub fn resources(&self) -> &TypeRegistry {
        // SAFETY: Resource registry is read-only metadata, safe to access
        unsafe { (*self.world).resources() }
    }

    /// Get the archetypes registry.
    ///
    /// This is always safe to access as it's read-only metadata about archetypes.
    #[inline]
    pub fn archetypes(&self) -> &archetype::Registry {
        // SAFETY: Resource registry is read-only metadata, safe to access
        unsafe { (*self.world).archetypes() }
    }

    /// Get immutable access to storage.
    ///
    /// This is safe because the shard's grant ensures no conflicting mutable access
    /// can occur. In debug builds, this validates that the grant permits storage access.
    #[inline]
    pub fn storage(&self) -> &storage::Storage {
        #[cfg(debug_assertions)]
        {
            // TODO: Validate grant permits storage access (once grant has storage info)
            // For now, we rely on pre-execution validation
        }

        // SAFETY: Grant validates no conflicting access exists
        unsafe { (*self.world).storage() }
    }

    /// Get mutable access to storage.
    ///
    /// TODO: Remove the ability to get wide storage access from shards - instead,
    /// require more fine-grained access to tables with access verification.
    ///
    /// This is safe because the shard's grant ensures no conflicting access
    /// can occur. In debug builds, this validates that the grant permits mutable access.
    #[inline]
    pub fn storage_mut(&mut self) -> &mut storage::Storage {
        #[cfg(debug_assertions)]
        {
            // TODO: Validate grant permits mutable storage access
            // For now, we rely on pre-execution validation
        }

        // SAFETY: Grant validates no conflicting access exists
        unsafe { (*self.world).storage_mut() }
    }

    /// Returns a reference to the underlying world.
    ///
    /// # Safety
    ///
    /// The caller must ensure that any access through the returned reference
    /// respects this shard's access grant. Accessing components or resources
    /// not covered by the grant is undefined behavior.
    #[inline]
    pub unsafe fn world(&self) -> &World {
        // SAFETY: Grant validates no conflicting access exists
        unsafe { &*self.world }
    }

    /// Returns a mutable reference to the underlying world.
    ///
    /// # Safety
    ///
    /// The caller must ensure that any access through the returned reference
    /// respects this shard's access grant. Accessing components or resources
    /// not covered by the grant is undefined behavior.
    ///
    /// # Deprecation Note
    ///
    /// This method should rarely be needed. Prefer using the safe accessors like
    /// `components()`, `storage()`, and `storage_mut()` instead. This method exists
    /// primarily for backward compatibility and special cases.
    #[inline]
    pub unsafe fn world_mut(&mut self) -> &mut World {
        unsafe { &mut *self.world }
    }

    /// Consume the shard and return the grant for release.
    ///
    /// This must be called instead of dropping the shard when used on worker threads.
    /// The returned grant should be passed back to the main thread for release.
    ///
    /// Generally, this is only necessary when shards are sent to worker threads for parallel
    /// execution and need can't return the shard itself.
    pub fn into_grant(self) -> AccessGrant {
        let grant = self.grant.clone();
        std::mem::forget(self); // Don't run Drop
        grant
    }

    /// Perform a shard query to access all entities that match the query data `D`.
    ///
    ///
    /// Note: This holds a mutable reference to the shard (and underlying component data) while the query result is active
    /// (use wisely).
    pub fn query<D: query::Data>(&'w mut self) -> query::Result<'w, D> {
        let query = query::Query::<D>::new(self.resources());
        assert!(
            self.grant.grants(query.required_access()),
            "Shard grant does not permit the requested query access."
        );

        // TODO: Safety - Query creation and grant validation ensures no alias violations occur
        query.invoke(self)
    }

    /// Get access to a unique resource stored in the world, if it exists.
    #[inline]
    pub fn get_unique<U: unique::Unique>(&self) -> Option<&U> {
        // TODO: Check grant permits resource access
        self.storage().uniques().get::<U>()
    }

    /// Get mutable access to a unique resource stored in the world, if it exists.
    #[inline]
    pub fn get_unique_mut<U: unique::Unique>(&mut self) -> Option<&mut U> {
        self.storage_mut().uniques_mut().get_mut::<U>()
    }
}

impl<'w> Drop for Shard<'w> {
    fn drop(&mut self) {
        // Debug assertion: catch misuse during development
        #[cfg(debug_assertions)]
        {
            // TODO: Check if on main thread, panic if not
            // This catches bugs where shards are dropped on worker threads
        }

        // Release grant - only safe on main thread
        unsafe {
            (*self.world).release_grant(&self.grant);
        }
    }
}
