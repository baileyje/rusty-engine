//! Entity management for the ECS (Entity Component System).
//!
//! This module provides the core entity types and allocation mechanisms for managing
//! unique entity identifiers within the ECS. Entities serve as lightweight handles
//! that tie together components and enable the ECS to track and manage game objects.
//!
//! # Architecture
//!
//! The entity system is built around several key types:
//!
//! - **[`Entity`]**: A unique identifier combining an [`Id`] and [`Generation`]. The ID
//!   identifies the entity slot, while the generation tracks how many times that slot
//!   has been reused. This allows the system to detect stale entity references.
//!
//! - **[`Allocator`]**: Manages entity ID allocation and recycling. When entities are
//!   freed, they are placed in a dead pool for reuse. This prevents ID exhaustion and
//!   improves memory locality. The allocator uses atomic operations to enable thread-safe
//!   access to the dead pool count.
//!
//! - **[`Storage`]**: Tracks active entities and their generations, enabling validation
//!   of entity references. This ensures that operations on deleted entities fail gracefully.
//!
//! - **[`Ref`]**: A lightweight reference to an entity that can be validated against the
//!   current state of the world. This prevents use-after-free bugs at the entity level.
//!
//! # Generation Tracking
//!
//! The generation system is crucial for memory safety. When an entity is freed, its
//! generation is incremented before being placed in the dead pool. Any references to
//! the old entity will have a mismatched generation, allowing the system to detect
//! that the entity is no longer valid:
//!
//! ```rust,ignore
//! let entity = allocator.alloc(); // Entity { id: 0, generation: 0 }
//! allocator.free(entity);
//! let reused = allocator.alloc();  // Entity { id: 0, generation: 1 }
//! // Original entity reference is now invalid due to generation mismatch
//! ```
//!
//! # Thread Safety
//!
//! The allocator uses atomic operations for the dead pool count and next ID, allowing
//! concurrent allocations. The dead pool resynchronization logic in `free()` ensures
//! consistency when the pool is modified. While `alloc()` and `free()` require `&mut self`,
//! the atomic fields enable these operations to be safely interleaved when the allocator
//! is accessed through interior mutability patterns (e.g., `Mutex` or `RwLock`).
//!
//! # Performance Considerations
//!
//! Entity IDs are reused from the dead pool when available, which provides several benefits:
//! - Reduces memory fragmentation by reusing entity slots
//! - Improves cache locality for entity-indexed data structures
//! - Prevents ID exhaustion in long-running applications
//! - Maintains a compact ID space for efficient storage indexing

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

mod reference;

/// Export the reference module for entity references.
pub use reference::{Ref, RefMut};

/// The generation of an entity, used to track whether an entity is the active entity in a world.
/// The generation starts at `FIRST` and is incremented each time an entity with the same `id` spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u32);

impl Generation {
    /// The first generation of an entity.
    const FIRST: Self = Self(0);

    /// Get the next generation from the current.
    #[inline]
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// An entity identifier. This is a non-zero unique identifier for an entity in the ECS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

impl From<u32> for Id {
    /// Get a row from an id value.
    fn from(value: u32) -> Self {
        Self(value)
    }
}

/// An entity in the ECS (Entity Component System).
/// This struct uniquely identifies an entity using its `id` and `generation`.
/// World should contain at most one `active` entity for a given `id`. The `generation` is used to
/// track whether an entity for for this reference is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Entity {
    /// The unique identifier of the entity.
    id: Id,

    /// The generation of the entity.
    generation: Generation,
}

impl Entity {
    /// Construct a new entity with just an id. This will default to the first generation.
    ///
    /// This is primarily used for testing.
    #[inline]
    pub(crate) fn new(id: impl Into<Id>) -> Self {
        Self::new_with_generation(id.into(), Generation::FIRST)
    }

    /// Construct a new entity with an id and known generations.
    #[inline]
    pub(crate) const fn new_with_generation(id: Id, generation: Generation) -> Self {
        Self { id, generation }
    }

    /// Get the id of this entity.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Get the generation of this entity.
    #[inline]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Get the index of this entity if it were to live in indexable storage (e.g. Vec)
    #[inline]
    pub fn index(&self) -> usize {
        self.id.0 as usize
    }

    /// Get a new entity with the same id but the next generation.
    #[inline]
    pub fn genned(&self) -> Self {
        Self::new_with_generation(self.id, self.generation.next())
    }
}

/// Implement ordering for Entity based on id and generation.
impl PartialOrd for Entity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Implement ordering for Entity based on id and generation.
impl Ord for Entity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.id.cmp(&other.id) {
            std::cmp::Ordering::Equal => self.generation.cmp(&other.generation),
            ord => ord,
        }
    }
}

/// An allocator for entities in the ECS. This will allocate unique entity IDs and
/// recycle freed entities to avoid ID exhaustion. The `next_id` and `freed_count` are
/// atomic to allow for concurrent allocations and deallocations.
#[derive(Default, Debug)]
pub struct Allocator {
    /// The pool of freed entities available for reuse.
    dead_pool: Vec<Entity>,
    /// The count of freed entities in the pool.
    dead_count: AtomicUsize,
    /// The next unique ID to allocate.
    next_id: AtomicU32,
}

impl Allocator {
    /// Construct a new entity allocator starting from ID 0.
    #[inline]
    pub const fn new() -> Self {
        Self {
            dead_pool: Vec::new(),
            dead_count: AtomicUsize::new(0),
            next_id: AtomicU32::new(0),
        }
    }

    /// Allocate a new entity, either by reusing a freed entity from the dead pool
    /// or by allocating a new unique ID.
    ///
    /// This uses atomics to allow thread-safe access to the dead pool count. When the pool
    /// is empty (dead_count == 0), fetch_sub will underflow to usize::MAX, and the subsequent
    /// wrapping_sub(1) produces usize::MAX - 1, which is out of bounds for the dead_pool Vec,
    /// causing get() to return None and triggering allocation of a new entity ID.
    pub fn alloc(&mut self) -> Entity {
        // Atomically decrement dead_count and get the previous value.
        // If dead_count was 0, this underflows to usize::MAX.
        // wrapping_sub(1) then gives us usize::MAX - 1, which is out of bounds.
        let free_index = self
            .dead_count
            .fetch_sub(1, Ordering::Relaxed)
            .wrapping_sub(1);

        // Attempt to get a recycled entity from the dead pool.
        // If free_index is out of bounds (pool empty or underflowed), allocate a new ID.
        self.dead_pool.get(free_index).copied().unwrap_or_else(|| {
            let id = Id(self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            Entity {
                id,
                generation: Generation(0),
            }
        })
    }

    /// Free an entity, making it available for reuse. The generation is incremented to
    /// invalidate any existing references to this entity.
    ///
    /// This method synchronizes the atomic dead_count with the actual dead_pool Vec.
    /// If dead_count has underflowed (become very large due to more allocs than frees),
    /// the pool is cleared and rebuilt. Otherwise, excess entries are truncated to match
    /// the atomic count.
    pub fn free(&mut self, entity: Entity) {
        let dead_count = self.dead_count.get_mut();

        // If dead_count > dead_pool.len(), it means alloc() caused underflow.
        // This happens when more allocations occurred than entities in the pool.
        // Clear the pool and start fresh.
        if *dead_count > self.dead_pool.len() {
            self.dead_pool.clear();
        } else {
            // Truncate to match the atomic count (handles case where dead_count < pool length).
            self.dead_pool.truncate(*dead_count);
        }

        // Add the freed entity to the pool with incremented generation.
        self.dead_pool.push(entity.genned());

        // Update atomic count to match the new pool size.
        *dead_count = self.dead_pool.len();
    }
}

#[test]
fn allocator_uniqueness() {
    // Given
    let mut allocator = Allocator::default();

    // When
    let mut entities = Vec::new();
    for _ in 0..200 {
        entities.push(allocator.alloc());
    }

    // Then - No dupes generated
    let pre_len = entities.len();
    entities.sort();
    entities.dedup();
    assert_eq!(pre_len, entities.len());
}

#[test]
fn allocator_reuse() {
    // Given
    let mut allocator = Allocator::default();

    // When
    let mut entities = Vec::new();
    for _ in 0..10 {
        entities.push(allocator.alloc());
    }

    for e in entities.drain(..) {
        allocator.free(e);
    }

    let mut reused_entities = Vec::new();
    for _ in 0..10 {
        reused_entities.push(allocator.alloc());
    }

    // Then - Entities should be reused with incremented generation
    reused_entities.sort();
    for (i, e) in reused_entities.iter().enumerate() {
        assert_eq!(e.id.0, i as u32);
        assert_eq!(e.generation.0, 1); // Generation should be incremented
    }
}

#[test]
fn allocator_exhaustion() {
    // Given
    let mut allocator = Allocator::default();

    // When
    let mut entities = Vec::new();
    for _ in 0..5 {
        entities.push(allocator.alloc());
    }

    for (i, e) in entities.drain(..).enumerate() {
        allocator.free(e);
        assert_eq!(allocator.dead_count.load(Ordering::Relaxed), i + 1)
    }

    // Then - Free counts match
    assert_eq!(
        allocator.dead_pool.len(),
        allocator.dead_count.load(Ordering::Relaxed)
    );

    // And When pushed past dead count
    for _ in 0..6 {
        allocator.alloc();
    }

    // Then
    assert!(allocator.dead_count.load(Ordering::Relaxed) > allocator.dead_pool.len());
}

#[test]
fn allocator_underflow_handling() {
    // Given
    let mut allocator = Allocator::default();

    // When - Allocate without any freed entities (dead_count is 0)
    let e1 = allocator.alloc();
    let e2 = allocator.alloc();

    // Then - Should allocate new IDs, not crash from underflow
    assert_eq!(e1.id.0, 0);
    assert_eq!(e2.id.0, 1);
    assert_eq!(e1.generation.0, 0);
    assert_eq!(e2.generation.0, 0);

    // When - Free one entity
    allocator.free(e1);

    // Then - Dead pool should have one entity with incremented generation
    assert_eq!(allocator.dead_pool.len(), 1);
    assert_eq!(allocator.dead_count.load(Ordering::Relaxed), 1);
    assert_eq!(allocator.dead_pool[0].generation.0, 1);

    // When - Allocate again (should reuse from pool)
    let e1_reused = allocator.alloc();

    // Then - Should get the freed entity with new generation
    assert_eq!(e1_reused.id, e1.id);
    assert_eq!(e1_reused.generation.0, 1);

    // When - Allocate again (pool now empty, should trigger underflow)
    let e3 = allocator.alloc();

    // Then - Should allocate new ID without crashing
    assert_eq!(e3.id.0, 2);
    assert_eq!(e3.generation.0, 0);

    // Verify dead_count underflowed (very large number)
    // The actual value depends on the wrapping_sub implementation
    assert!(allocator.dead_count.load(Ordering::Relaxed) > 1000);
}

#[test]
fn allocator_pool_resync_after_underflow() {
    // Given
    let mut allocator = Allocator::default();

    // When - Free entity to populate pool
    let e1 = allocator.alloc();
    allocator.free(e1);

    // When - Allocate twice (empties pool and causes underflow)
    let _ = allocator.alloc();
    let _ = allocator.alloc();

    // Then - dead_count should be very large (underflowed)
    let dead_count = allocator.dead_count.load(Ordering::Relaxed);
    assert!(dead_count > allocator.dead_pool.len());

    // When - Free another entity (should resync pool)
    let e2 = allocator.alloc();
    allocator.free(e2);

    // Then - Pool should be cleared and resynced
    assert_eq!(allocator.dead_pool.len(), 1);
    assert_eq!(allocator.dead_count.load(Ordering::Relaxed), 1);

    // And - Next alloc should work correctly
    let reused = allocator.alloc();
    assert_eq!(reused.id, e2.id);
}

#[test]
fn allocator_large_scale_reuse() {
    // Given
    let mut allocator = Allocator::default();

    // When - Allocate 1000 entities
    let mut entities = Vec::new();
    for _ in 0..1000 {
        entities.push(allocator.alloc());
    }

    // Then - All should be unique
    let mut sorted = entities.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(entities.len(), sorted.len());

    // When - Free half of them
    for e in entities.drain(0..500) {
        allocator.free(e);
    }

    // When - Allocate 500 more (should reuse)
    let mut reused = Vec::new();
    for _ in 0..500 {
        reused.push(allocator.alloc());
    }

    // Then - Reused entities should have generation 1
    for e in &reused {
        assert_eq!(e.generation.0, 1);
    }

    // Then - IDs should be from the freed range (0..500)
    for e in &reused {
        assert!(e.id.0 < 500);
    }
}

#[test]
fn allocator_multiple_generations() {
    // Given
    let mut allocator = Allocator::default();
    let entity = allocator.alloc();
    let original_id = entity.id;

    // When - Free and reallocate multiple times
    allocator.free(entity);
    let gen1 = allocator.alloc();

    allocator.free(gen1);
    let gen2 = allocator.alloc();

    allocator.free(gen2);
    let gen3 = allocator.alloc();

    // Then - Same ID, incrementing generations
    assert_eq!(gen1.id, original_id);
    assert_eq!(gen1.generation.0, 1);

    assert_eq!(gen2.id, original_id);
    assert_eq!(gen2.generation.0, 2);

    assert_eq!(gen3.id, original_id);
    assert_eq!(gen3.generation.0, 3);
}

#[test]
fn entity_ordering() {
    // Given
    let e1 = Entity::new(Id(1));
    let e2 = Entity::new(Id(2));
    let e1_gen1 = e1.genned();

    // Then - Ordered by ID first, then generation
    assert!(e1 < e2);
    assert!(e1 < e1_gen1);
    assert!(e1_gen1 < e2);
}

#[test]
fn entity_equality() {
    // Given
    let e1 = Entity::new(Id(42));
    let e2 = Entity::new(Id(42));
    let e3 = Entity::new(Id(43));
    let e1_gen1 = e1.genned();

    // Then
    assert_eq!(e1, e2);
    assert_ne!(e1, e3);
    assert_ne!(e1, e1_gen1); // Different generation
}

#[test]
fn entity_index() {
    // Given
    let e1 = Entity::new(Id(0));
    let e2 = Entity::new(Id(42));
    let e3 = Entity::new(Id(1000));

    // Then
    assert_eq!(e1.index(), 0);
    assert_eq!(e2.index(), 42);
    assert_eq!(e3.index(), 1000);
}

#[test]
fn generation_next() {
    // Given
    let gen0 = Generation::FIRST;

    // When
    let gen1 = gen0.next();
    let gen2 = gen1.next();

    // Then
    assert_eq!(gen0.0, 0);
    assert_eq!(gen1.0, 1);
    assert_eq!(gen2.0, 2);
}

#[test]
fn id_from_u32() {
    // Given
    let id1 = Id::from(42);
    let id2 = Id::from(1000);

    // Then
    assert_eq!(id1.0, 42);
    assert_eq!(id2.0, 1000);
}
