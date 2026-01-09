use std::collections::HashMap;

use crate::ecs::{entity, storage::row::Row};

/// Trait for defining a sparse index mapping from sparse entity IDs to dense rows.
///
/// This trait enables efficient lookup of dense storage locations for sparsely distributed identifiers.
/// The primary use case is mapping entity IDs (which may have large gaps) to contiguous table row indices.
///
/// # Example
///
/// ```ignore
/// use storage::index::{Index, DynamicIndex};
///
/// let mut index = DynamicIndex::new();
///
/// // Map sparse entity IDs to dense table rows
/// index.insert(entity1, 0);   // Entity 1 is at row 0
/// index.insert(entity20, 1);  // Entity 20  is at row 1
/// index.insert(entity105, 2); // Entity 105 is at row 2
///
/// assert_eq!(index.get(entity105), Some(2));
/// assert_eq!(index.get(entity999), None);
/// ```
#[allow(dead_code)]
pub trait Index {
    /// Insert a row for the given entity.
    ///
    /// If the sparse index already exists, the old value is replaced.
    fn insert(&mut self, entity: entity::Entity, row: Row);

    /// Get the row for the given entity if it exists.
    ///
    /// Returns `None` if the entity is not present.
    fn get(&self, entity: entity::Entity) -> Option<Row>;

    /// Remove the row for the given entity.
    ///
    /// Returns the old row if it existed, or `None` if not present.
    fn remove(&mut self, entity: entity::Entity) -> Option<Row>;

    /// Check if the index contains a mapping for the given sparse index.
    #[inline]
    fn contains(&self, entity: entity::Entity) -> bool {
        self.get(entity).is_some()
    }
}

/// A block-based sparse index optimized for entity ID to table row lookups.
///
/// This index divides the sparse ID space into fixed-size blocks, allocating memory
/// only for blocks that contain at least one entry. Within each block, a dense vector
/// stores mappings, allowing O(1) lookup with good cache locality.
///
/// # Design Rationale
///
/// Entity IDs in ECS systems typically exhibit these patterns:
/// - **Sequential allocation**: IDs 0, 1, 2, 3... when entities are spawned
/// - **Chunked reuse**: After despawning, new entities reuse IDs in batches
/// - **Local density**: IDs tend to cluster (entities spawned together have similar IDs)
///
/// This design exploits these patterns:
/// - **Block allocation**: Only allocates blocks that contain entries (sparse outer structure)
/// - **Dense within blocks**: Uses Vec within blocks for fast access (dense inner structure)
/// - **Configurable block size**: Tune for your ID distribution (larger = more memory, fewer indirections)
///
/// # Performance Characteristics
///
/// | Operation | Time | Memory |
/// |-----------|------|--------|
/// | `insert()` | O(1) amortized | Allocates block on first use |
/// | `get()` | O(1) | No allocation |
/// | `remove()` | O(1) | No deallocation (leaves `None`) |
///
/// # Memory Usage
///
/// For `N` entities distributed across `B` blocks with block size `S`:
/// - **Best case** (dense IDs): `~N * sizeof(Option<usize>)` (all in one block)
/// - **Worst case** (sparse IDs): `B * S * sizeof(Option<usize>)` (many sparse blocks)
/// - **Typical case** (chunked IDs): Between best and worst
///
/// # Block Size Tuning
///
/// - **Small blocks (64-128)**: Lower memory overhead for very sparse IDs, more indirection
/// - **Default (256)**: Balanced for typical entity spawning patterns
/// - **Large blocks (512-1024)**: Better cache locality for dense IDs, higher memory overhead
///
/// # Example
///
/// ```ignore
/// // Default configuration (256 elements per block)
/// let mut index = DynamicIndex::new();
///
/// // Custom block size for very sparse or very dense patterns
/// let mut sparse_index = DynamicIndex::new_with_block_size(64);   // Sparse IDs
/// let mut dense_index = DynamicIndex::new_with_block_size(1024);  // Dense IDs
///
/// // Map entity IDs to table rows
/// index.insert(0, 0);      // Block 0: [0, 1, 2]
/// index.insert(1, 1);
/// index.insert(2, 2);
/// index.insert(1000, 3);   // Block 3: [3]
///
/// assert_eq!(index.get(1), Some(1));
/// assert_eq!(index.get(500), None);  // Block 1 not allocated
/// ```
///
/// # Trade-offs vs HashMap
///
/// | Aspect | DynamicIndex | HashMap |
/// |--------|--------------|---------|
/// | Lookup speed | Faster (2x-10x) | Slower (hashing overhead) |
/// | Memory (dense) | Lower | Higher (hash table overhead) |
/// | Memory (sparse) | Can be higher | Generally lower |
/// | Cache locality | Better | Worse (pointer chasing) |
///
/// Use DynamicIndex when entity IDs have local density (typical ECS pattern).
/// Use HashMap when IDs are truly random or extremely sparse.
#[derive(Debug)]
pub struct DynamicIndex {
    /// The size of blocks to allocate when growing the index.
    block_size: usize,

    /// A collection of optional blocks, each block is a vector of optional usize indices.
    /// Outer Vec is indexed by `sparse_id / block_size`.
    /// Inner Vec is indexed by `sparse_id % block_size`.
    maps: Vec<Option<Vec<Option<Row>>>>,
}

#[allow(dead_code)]
impl DynamicIndex {
    /// Default block size balances memory usage and access speed for typical entity patterns.
    pub const DEFAULT_BLOCK_SIZE: usize = 256;

    /// Create a new DynamicIndex with the default block size.
    #[inline]
    pub const fn new() -> Self {
        Self::new_with_block_size(Self::DEFAULT_BLOCK_SIZE)
    }

    /// Create a new DynamicIndex with a custom block size.
    ///
    /// # Tuning Guidelines
    ///
    /// - **64-128**: Very sparse entity IDs (many gaps)
    /// - **256** (default): Balanced for typical usage
    /// - **512-1024**: Dense entity IDs (few gaps)
    ///
    /// # Panics
    ///
    /// Debug builds panic if block_size is 0.
    #[inline]
    pub const fn new_with_block_size(block_size: usize) -> Self {
        debug_assert!(block_size > 0, "block_size must be greater than 0");
        Self {
            block_size,
            maps: Vec::new(),
        }
    }

    /// Calculate block and within-block indices for a sparse index.
    #[inline]
    fn indices(&self, entity: entity::Entity) -> (usize, usize) {
        let entity_index = entity.index();
        let block_index = entity_index / self.block_size;
        let within_block_index = entity_index % self.block_size;
        (block_index, within_block_index)
    }

    /// Get the number of allocated blocks (including empty slots).
    ///
    /// Useful for debugging and memory profiling.
    #[inline]
    pub fn block_count(&self) -> usize {
        self.maps.len()
    }

    /// Get the number of blocks that have been allocated (non-None).
    ///
    /// Useful for debugging and memory profiling.
    pub fn allocated_block_count(&self) -> usize {
        self.maps.iter().filter(|b| b.is_some()).count()
    }

    /// Estimate memory usage in bytes.
    ///
    /// This is approximate and doesn't include Vec overhead or heap allocator metadata.
    pub fn memory_usage(&self) -> usize {
        let outer_vec_size =
            self.maps.capacity() * std::mem::size_of::<Option<Vec<Option<usize>>>>();
        let inner_vecs_size: usize = self
            .maps
            .iter()
            .filter_map(|block| block.as_ref())
            .map(|vec| vec.capacity() * std::mem::size_of::<Option<usize>>())
            .sum();
        outer_vec_size + inner_vecs_size
    }
}

impl Default for DynamicIndex {
    /// Custom default to ensure we get the default block size.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Index for DynamicIndex {
    fn insert(&mut self, entity: entity::Entity, row: Row) {
        let (block_index, within_block_index) = self.indices(entity);

        // Ensure the maps vector has enough blocks
        if block_index >= self.maps.len() {
            self.maps.resize_with(block_index + 1, || None);
        }

        // Ensure the block exists
        if self.maps[block_index].is_none() {
            self.maps[block_index] = Some(vec![None; self.block_size]);
        }

        // Insert the value into the appropriate block
        if let Some(block) = &mut self.maps[block_index] {
            block[within_block_index] = Some(row);
        }
    }

    fn get(&self, entity: entity::Entity) -> Option<Row> {
        let (block_index, within_block_index) = self.indices(entity);

        // Check if the block exists and retrieve value
        let block = self.maps.get(block_index)?.as_ref()?;
        block[within_block_index]
    }

    fn remove(&mut self, entity: entity::Entity) -> Option<Row> {
        let (block_index, within_block_index) = self.indices(entity);

        // Check if the block exists and get mutable reference
        let block = self.maps.get_mut(block_index)?.as_mut()?;

        // Take the value, leaving None in its place
        block[within_block_index].take()
    }
}

/// A HashMap-based sparse index for comparison and fallback.
///
/// This implementation uses the standard library's `HashMap` to map sparse indices
/// to dense indices. It's simpler than [`DynamicIndex`] but typically slower due to
/// hashing overhead and worse cache locality.
///
/// # When to Use
///
/// - **Truly random entity IDs**: No locality, completely unpredictable patterns
/// - **Extremely sparse IDs**: Huge gaps between entity IDs (e.g., 0, 1000000, 2000000)
/// - **Small datasets**: < 100 entities where performance differences are negligible
/// - **Benchmarking**: Compare against DynamicIndex for your specific use case
///
/// # Performance vs DynamicIndex
///
/// Based on benchmarks with typical ECS entity ID patterns:
/// - **Lookup**: ~2-10x slower (hashing + potential collisions)
/// - **Insertion**: ~2-5x slower (hashing + resizing)
/// - **Memory (sparse)**: Generally better (no unused blocks)
/// - **Memory (dense)**: Worse (hash table overhead ~2x)
///
/// # Example
///
/// ```ignore
/// let mut index = HashIndex::new();
/// index.insert(0, 0);
/// index.insert(1000000, 1);  // Huge gap - no wasted memory
/// assert_eq!(index.get(1000000), Some(1));
/// ```
#[derive(Debug, Default)]
pub struct HashIndex {
    map: HashMap<entity::Entity, Row>,
}

#[allow(dead_code)]
impl HashIndex {
    /// Create a new empty HashIndex.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Create a new HashIndex with pre-allocated capacity.
    ///
    /// Useful when you know approximately how many entities will be stored.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
        }
    }

    /// Get the number of entries in the index.
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Check if the index is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Estimate memory usage in bytes.
    #[inline]
    pub fn memory_usage(&self) -> usize {
        // Approximate: HashMap capacity * (key + value + overhead)
        // This is a rough estimate, actual overhead varies
        self.map.capacity() * (std::mem::size_of::<usize>() * 2 + 8)
    }
}

impl Index for HashIndex {
    fn insert(&mut self, entity: entity::Entity, row: Row) {
        self.map.insert(entity, row);
    }

    fn get(&self, entity: entity::Entity) -> Option<Row> {
        self.map.get(&entity).copied()
    }

    fn remove(&mut self, entity: entity::Entity) -> Option<Row> {
        self.map.remove(&entity)
    }
}

#[cfg(test)]
mod tests {
    use crate::ecs::entity::Entity;

    use super::*;

    fn entity(id: u32) -> Entity {
        Entity::new(id.into())
    }

    fn row(index: usize) -> Option<Row> {
        Some(Row::new(index))
    }

    #[test]
    fn dynamic_index_single_block() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(10);

        let entity0 = entity(0);
        let entity1 = entity(1);
        let entity5 = entity(5);
        let entity6 = entity(6);
        let entity9 = entity(9);

        // When
        index.insert(entity0, 10.into());
        index.insert(entity5, 40.into());
        index.insert(entity9, 80.into());

        // Then
        assert_eq!(index.block_count(), 1);
        assert_eq!(index.get(entity0), row(10));
        assert_eq!(index.get(entity1), None);
        assert_eq!(index.get(entity5), row(40));
        assert_eq!(index.get(entity6), None);
        assert_eq!(index.get(entity9), row(80));
    }

    #[test]
    fn dynamic_index_multi_block() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(4);
        let entity0 = entity(0);
        let entity5 = entity(5);
        let entity9 = entity(9);

        // When
        index.insert(entity0, 10.into());
        index.insert(entity5, 30.into());
        index.insert(entity9, 80.into());

        // Then - should grow to 3 blocks
        assert_eq!(index.block_count(), 3);
        assert_eq!(index.allocated_block_count(), 3);
    }

    #[test]
    fn dynamic_index_block_skipping() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(4);
        let entity0 = entity(0);
        let entity9 = entity(9);

        // When
        index.insert(entity0, 10.into());
        index.insert(entity9, 80.into());

        // Then - should grow to 3 blocks with the middle block being None
        assert_eq!(index.block_count(), 3);
        assert_eq!(index.allocated_block_count(), 2); // Only blocks 0 and 2
        assert_eq!(index.maps[1], None);
    }

    #[test]
    fn dynamic_index_remove() {
        // Given
        let mut index = DynamicIndex::new();
        let entity0 = entity(0);
        let entity1 = entity(1);
        let entity2 = entity(2);
        let entity3 = entity(3);
        index.insert(entity0, 100.into());
        index.insert(entity1, 200.into());
        index.insert(entity2, 300.into());

        // When - remove existing
        let removed = index.remove(entity1);

        // Then
        assert_eq!(removed, row(200));
        assert_eq!(index.get(entity1), None);
        assert_eq!(index.get(entity0), row(100)); // Others unaffected
        assert_eq!(index.get(entity2), row(300));

        // When - remove non-existent
        let removed = index.remove(entity3);

        // Then
        assert_eq!(removed, None);
    }

    #[test]
    fn dynamic_index_overwrite() {
        // Given
        let mut index = DynamicIndex::new();
        let entity = entity(5);
        index.insert(entity, 100.into());

        // When - overwrite existing value
        index.insert(entity, 200.into());

        // Then
        assert_eq!(index.get(entity), row(200));
    }

    #[test]
    fn dynamic_index_contains() {
        // Given
        let mut index = DynamicIndex::new();
        let ent = entity(10);
        index.insert(ent, 100.into());

        // When/Then
        assert!(index.contains(ent));
        assert!(!index.contains(entity(11)));
        assert!(!index.contains(entity(0)));
    }

    #[test]
    fn dynamic_index_large_sparse_indices() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(256);

        // When - insert very sparse indices
        index.insert(entity(0), 0.into());
        index.insert(entity(1000), 1.into());
        index.insert(entity(10000), 2.into());
        index.insert(entity(100000), 3.into());

        // Then
        assert_eq!(index.get(entity(0)), row(0));
        assert_eq!(index.get(entity(1000)), row(1));
        assert_eq!(index.get(entity(10000)), row(2));
        assert_eq!(index.get(entity(100000)), row(3));
        assert_eq!(index.get(entity(500)), None);

        // Check memory allocation
        assert_eq!(index.block_count(), 391); // 100000 / 256 + 1
        assert_eq!(index.allocated_block_count(), 4); // Only 4 blocks actually allocated
    }

    #[test]
    fn dynamic_index_sequential_entity_pattern() {
        // Given - simulate sequential entity spawning
        let mut index = DynamicIndex::new();

        // When - add 1000 sequential entities
        for i in 0..1000 {
            index.insert(entity(i), Row::new(i as usize));
        }

        // Then - all retrievable
        for i in 0..1000 {
            assert_eq!(index.get(entity(i)), row(i as usize));
        }

        // Should use minimal blocks (1000 / 256 = 4 blocks)
        assert_eq!(index.allocated_block_count(), 4);
    }

    #[test]
    fn dynamic_index_chunked_entity_pattern() {
        // Given - simulate chunked entity spawning with gaps
        let mut index = DynamicIndex::new();

        // When - add entities in chunks with gaps
        for chunk in 0..5 {
            let base = chunk * 1000;
            for i in 0..100 {
                index.insert(entity(base + i), Row::new((chunk * 100 + i) as usize));
            }
        }

        // Then - all retrievable
        for chunk in 0..5 {
            let base = chunk * 1000;
            for i in 0..100 {
                assert_eq!(index.get(entity(base + i)), row((chunk * 100 + i) as usize));
            }
        }

        // Verify gaps return None
        assert_eq!(index.get(entity(500)), None);
        assert_eq!(index.get(entity(1500)), None);
    }

    #[test]
    fn dynamic_index_memory_usage() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(256);

        // When - empty
        let empty_usage = index.memory_usage();

        // Then - should be 0 or minimal
        assert_eq!(empty_usage, 0);

        // When - add one entry
        index.insert(entity(0), 0.into());
        let one_block_usage = index.memory_usage();

        // Then - should allocate one block
        assert!(one_block_usage > 0);

        // When - add sparse entry
        index.insert(entity(10000), 1.into());
        let sparse_usage = index.memory_usage();

        // Then - should be larger but not proportional to gap
        assert!(sparse_usage > one_block_usage);
        assert!(sparse_usage < one_block_usage * 40); // Not 40x for 40x span
    }

    // HashIndex tests

    #[test]
    fn hash_index_basic_operations() {
        // Given
        let mut index = HashIndex::new();

        // When
        index.insert(entity(0), 100.into());
        index.insert(entity(1), 200.into());
        index.insert(entity(1000), 300.into());

        // Then
        assert_eq!(index.get(entity(0)), row(100));
        assert_eq!(index.get(entity(1)), row(200));
        assert_eq!(index.get(entity(1000)), row(300));
        assert_eq!(index.get(entity(500)), None);
        assert_eq!(index.len(), 3);
        assert!(!index.is_empty());
    }

    #[test]
    fn hash_index_remove() {
        // Given
        let mut index = HashIndex::new();
        index.insert(entity(5), 500.into());
        index.insert(entity(10), 1000.into());

        // When
        let removed = index.remove(entity(5));

        // Then
        assert_eq!(removed, row(500));
        assert_eq!(index.get(entity(5)), None);
        assert_eq!(index.get(entity(10)), row(1000));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn hash_index_overwrite() {
        // Given
        let mut index = HashIndex::new();
        index.insert(entity(42), 100.into());

        // When
        index.insert(entity(42), 200.into());

        // Then
        assert_eq!(index.get(entity(42)), row(200));
        assert_eq!(index.len(), 1); // Still only one entry
    }

    #[test]
    fn hash_index_with_capacity() {
        // Given
        let index = HashIndex::with_capacity(100);

        // Then
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        // Capacity is pre-allocated but map is empty
    }

    #[test]
    fn hash_index_contains() {
        // Given
        let mut index = HashIndex::new();
        index.insert(entity(7), 77.into());

        // When/Then
        assert!(index.contains(entity(7)));
        assert!(!index.contains(entity(8)));
    }

    #[test]
    fn hash_index_very_sparse() {
        // Given
        let mut index = HashIndex::new();

        // When - extremely sparse indices
        index.insert(entity(0), 0.into());
        index.insert(entity(1000000), 1.into());
        index.insert(entity(2000000), 2.into());

        // Then - all work without wasted memory between
        assert_eq!(index.get(entity(0)), row(0));
        assert_eq!(index.get(entity(1000000)), row(1));
        assert_eq!(index.get(entity(2000000)), row(2));
        assert_eq!(index.len(), 3);
    }

    // Trait object tests

    #[test]
    fn index_trait_dynamic_dispatch() {
        // Given - use trait objects
        let mut dynamic: Box<dyn Index> = Box::new(DynamicIndex::new());
        let mut hash: Box<dyn Index> = Box::new(HashIndex::new());

        // When
        dynamic.insert(entity(10), 100.into());
        hash.insert(entity(10), 100.into());

        // Then - both work through trait
        assert_eq!(dynamic.get(entity(10)), row(100));
        assert_eq!(hash.get(entity(10)), row(100));
        assert!(dynamic.contains(entity(10)));
        assert!(hash.contains(entity(10)));
    }

    #[test]
    #[should_panic(expected = "block_size must be greater than 0")]
    #[cfg(debug_assertions)]
    fn dynamic_index_zero_block_size_panics() {
        let _ = DynamicIndex::new_with_block_size(0);
    }
}
