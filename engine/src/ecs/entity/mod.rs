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
//!   improves memory locality.
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
//! # Performance Considerations
//!
//! Entity IDs are reused from the dead pool when available, which provides several benefits:
//! - Reduces memory fragmentation by reusing entity slots
//! - Improves cache locality for entity-indexed data structures
//! - Prevents ID exhaustion in long-running applications
//! - Maintains a compact ID space for efficient storage indexing

mod reference;

use std::sync::{
    RwLock,
    atomic::{AtomicU32, Ordering},
};

use crossbeam::queue::SegQueue;
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

/// Growable array of atomic generations.
/// Maps from an entity id to its current generation
#[derive(Default, Debug)]
struct Generations {
    chunks: RwLock<Vec<Box<[AtomicU32; CHUNK_SIZE]>>>,
}

const CHUNK_SIZE: usize = 4096;

impl Generations {
    const fn new() -> Self {
        Self {
            chunks: RwLock::new(Vec::new()),
        }
    }

    fn get(&self, id: Id) -> Generation {
        let id = id.0;
        let chunk_idx = id as usize / CHUNK_SIZE;
        let slot_idx = id as usize % CHUNK_SIZE;

        let chunks = self.chunks.read().unwrap();
        Generation(if chunk_idx < chunks.len() {
            chunks[chunk_idx][slot_idx].load(Ordering::Acquire)
        } else {
            0 // Fresh ID, generation 0
        })
    }

    fn increment(&self, id: Id) {
        self.ensure_capacity(id);
        let id = id.0;
        let chunk_idx = id as usize / CHUNK_SIZE;
        let slot_idx = id as usize % CHUNK_SIZE;

        let chunks = self.chunks.read().unwrap();
        chunks[chunk_idx][slot_idx].fetch_add(1, Ordering::Release);
    }

    fn ensure_capacity(&self, id: Id) {
        let id = id.0;
        let chunk_idx = id as usize / CHUNK_SIZE;
        let chunks_len = self.chunks.read().unwrap().len();

        if chunk_idx >= chunks_len {
            let mut chunks = self.chunks.write().unwrap();
            while chunks.len() <= chunk_idx {
                chunks.push(Box::new(std::array::from_fn(|_| AtomicU32::new(0))));
            }
        }
    }
}

/// An allocator for entities in the ECS.
///
/// Allocates unique entity IDs and recycles freed entities to avoid ID exhaustion.
/// When an entity is freed, its generation is incremented before being placed in the
/// dead pool, invalidating any stale references.
///
/// # Design Note
///
/// This allocator requires `&mut self` for all operations and is owned by the World,
/// which is `!Send`. No atomic operations are needed since exclusive access is
/// guaranteed by the borrow checker. If a command buffer pattern is added later
/// that requires ID reservation from parallel contexts, consider batch reservation
/// rather than making allocation atomic.
#[derive(Default, Debug)]
pub struct Allocator {
    /// Generation counter for each ID slot.
    /// Indexed by entity ID, stores current generation.
    generations: Generations,

    /// Pool of IDs available for reuse (just the ID, not full Entity).
    dead_pool: SegQueue<Id>,

    /// Next fresh ID to allocate.
    next_id: AtomicU32,
}

impl Allocator {
    /// Construct a new entity allocator starting from ID 0.
    #[inline]
    pub const fn new() -> Self {
        Self {
            generations: Generations::new(),
            dead_pool: SegQueue::new(),
            next_id: AtomicU32::new(0),
        }
    }

    /// Allocate many new entities at once.
    ///
    /// Reuses entities from the dead pool first, then allocates new IDs as needed.
    pub fn alloc_many(&self, count: usize) -> Vec<Entity> {
        let mut alloced = Vec::new();
        // Allocate as many as we can from teh deap pool.
        while alloced.len() < count
            && let Some(id) = self.dead_pool.pop()
        {
            alloced.push(Entity::new_with_generation(id, self.generations.get(id)));
        }

        // Allocate remaining as new sequential IDs
        let remaining = (count - alloced.len()) as u32;
        if remaining > 0 {
            let start_id = self.next_id.fetch_add(remaining, Ordering::Relaxed);
            let last_id = start_id + remaining;
            self.generations.ensure_capacity(Id(last_id - 1));

            alloced.extend((start_id..last_id).map(|id| Entity::new(Id(id))));
        }

        alloced
    }

    /// Allocate a new entity, either by reusing a freed entity from the dead pool
    /// or by allocating a new unique ID.
    pub fn alloc(&self) -> Entity {
        // Try to reuse from dead pool first
        if let Some(id) = self.dead_pool.pop() {
            return Entity::new_with_generation(id, self.generations.get(id));
        }

        // Allocate fresh ID
        let id = Id(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.generations.ensure_capacity(id);
        Entity::new(id)
    }

    /// Free an entity for reuse (lock-free).
    pub fn free(&self, entity: Entity) {
        let id = entity.id();
        // Bump generation atomically
        self.generations.increment(id);
        // Return ID to pool
        self.dead_pool.push(id);
    }
}

#[test]
fn allocator_uniqueness() {
    // Given
    let allocator = Allocator::default();

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
    let allocator = Allocator::default();

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
fn allocator_free_and_reuse_cycle() {
    // Given
    let allocator = Allocator::default();

    // When - Allocate 5 entities
    let mut entities = Vec::new();
    for _ in 0..5 {
        entities.push(allocator.alloc());
    }

    // Then - Pool should be empty
    assert_eq!(allocator.dead_pool.len(), 0);

    // When - Free all entities
    for e in entities.drain(..) {
        allocator.free(e);
    }

    // Then - Pool should have 5 entities
    assert_eq!(allocator.dead_pool.len(), 5);

    // When - Allocate 6 (more than pool size)
    let mut new_entities = Vec::new();
    for _ in 0..6 {
        new_entities.push(allocator.alloc());
    }

    // Then - Pool should be empty and we got one new ID
    assert_eq!(allocator.dead_pool.len(), 0);
    // 5 reused (gen 1) + 1 new (gen 0)
    let new_count = new_entities.iter().filter(|e| e.generation.0 == 0).count();
    let reused_count = new_entities.iter().filter(|e| e.generation.0 == 1).count();
    assert_eq!(new_count, 1);
    assert_eq!(reused_count, 5);
}

#[test]
fn allocator_empty_pool_allocates_new() {
    // Given
    let allocator = Allocator::default();

    // When - Allocate without any freed entities
    let e1 = allocator.alloc();
    let e2 = allocator.alloc();

    // Then - Should allocate new sequential IDs
    assert_eq!(e1.id.0, 0);
    assert_eq!(e2.id.0, 1);
    assert_eq!(e1.generation.0, 0);
    assert_eq!(e2.generation.0, 0);

    // When - Free one entity
    allocator.free(e1);

    // Then - Dead pool should have one entity with incremented generation
    assert_eq!(allocator.dead_pool.len(), 1);
    assert_eq!(allocator.generations.get(Id(0)), Generation(1));

    // When - Allocate again (should reuse from pool)
    let e1_reused = allocator.alloc();

    // Then - Should get the freed entity with new generation
    assert_eq!(e1_reused.id, e1.id);
    assert_eq!(e1_reused.generation.0, 1);
    assert_eq!(allocator.dead_pool.len(), 0);

    // When - Allocate again (pool empty)
    let e3 = allocator.alloc();

    // Then - Should allocate new ID
    assert_eq!(e3.id.0, 2);
    assert_eq!(e3.generation.0, 0);
}

#[test]
fn allocator_large_scale_reuse() {
    // Given
    let allocator = Allocator::default();

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
    let allocator = Allocator::default();
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
fn allocator_alloc_many_from_empty() {
    // Given
    let allocator = Allocator::default();

    // When - Allocate many from empty allocator
    let entities = allocator.alloc_many(5);

    // Then - Should get sequential new IDs
    assert_eq!(entities.len(), 5);
    for (i, e) in entities.iter().enumerate() {
        assert_eq!(e.id.0, i as u32);
        assert_eq!(e.generation.0, 0);
    }
    assert_eq!(allocator.next_id.load(Ordering::Relaxed), 5);
}

#[test]
fn allocator_alloc_many_from_pool() {
    // Given
    let allocator = Allocator::default();
    // Create and free 5 entities to populate the pool
    for e in allocator.alloc_many(5) {
        allocator.free(e);
    }
    assert_eq!(allocator.dead_pool.len(), 5);

    // When - Allocate 3 (less than pool size)
    let entities = allocator.alloc_many(3);

    // Then - Should reuse from pool
    assert_eq!(entities.len(), 3);
    for e in &entities {
        assert_eq!(e.generation.0, 1); // Reused entities have generation 1
    }
    assert_eq!(allocator.dead_pool.len(), 2); // 2 left in pool
}

#[test]
fn allocator_alloc_many_mixed() {
    // Given
    let allocator = Allocator::default();
    // Create and free 3 entities to populate the pool
    for e in allocator.alloc_many(3) {
        allocator.free(e);
    }
    assert_eq!(allocator.dead_pool.len(), 3);
    assert_eq!(allocator.next_id.load(Ordering::Relaxed), 3);

    // When - Allocate 5 (more than pool size)
    let entities = allocator.alloc_many(5);

    // Then - Should get 3 reused + 2 new
    assert_eq!(entities.len(), 5);
    let reused: Vec<_> = entities.iter().filter(|e| e.generation.0 == 1).collect();
    let new: Vec<_> = entities.iter().filter(|e| e.generation.0 == 0).collect();
    assert_eq!(reused.len(), 3);
    assert_eq!(new.len(), 2);

    // New entities should have IDs 3 and 4
    let mut new_ids: Vec<_> = new.iter().map(|e| e.id.0).collect();
    new_ids.sort();
    assert_eq!(new_ids, vec![3, 4]);

    assert_eq!(allocator.dead_pool.len(), 0);
    assert_eq!(allocator.next_id.load(Ordering::Relaxed), 5);
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
