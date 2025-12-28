use std::collections::HashMap;

use crate::core::ecs::entity::Entity;

/// Trait for defining a sparse index mapping into a dense collection.
pub trait Index {
    /// Insert a dense index into for the given sparse index.
    fn insert(&mut self, index: usize, value: usize);

    /// Get a dense index for the given sparse index if it exists.
    fn get(&self, index: usize) -> Option<usize>;

    /// Remove the dense index for the given sparse index, and return the existing dense index if it exists.
    fn remove(&mut self, index: usize) -> Option<usize>;

    /// Check if the index contains a value for the given sparse index.
    #[inline]
    fn contains(&self, index: usize) -> bool {
        self.get(index).is_some()
    }
}

/// Struct representing storage for ECS components. This implements a sparse set that densely
/// stores components by their associated entity IDs.
///
/// TODO: Evaluate removing this since we are using tables for storage now.
#[derive(Debug, Default)]
pub struct Storage<T, I: Index = DynamicIndex> {
    /// The dense storage of component data.
    dense: Vec<T>,

    /// The sparse index mapping entity IDs to dense storage indexes.
    index: I,

    /// The list of entities corresponding to the dense storage. This must maintain the same order
    /// as the dense storage in order to aid in index updates during removals.
    entities: Vec<Entity>,
}

impl<T, I: Index> Storage<T, I> {
    /// Create a new storage with the given index.
    #[inline]
    pub const fn new_with_index(index: I) -> Self {
        Self {
            dense: Vec::new(),
            index,
            entities: Vec::new(),
        }
    }

    /// Insert a component for the given entity. If the component already exists, it will be
    /// updated.
    pub fn insert(&mut self, entity: Entity, component: T) {
        if self.index.contains(entity.index()) {
            // Update existing component
            if let Some(dense_index) = self.index.get(entity.index()) {
                self.dense[dense_index] = component;
            }
        } else {
            // Insert new component
            let dense_index = self.dense.len();
            self.dense.push(component);
            self.index.insert(entity.index(), dense_index);
            self.entities.push(entity);
        }
    }

    /// Get the component for the given entity, if it exists.
    pub fn get(&self, entity: Entity) -> Option<&T> {
        if let Some(dense_index) = self.index.get(entity.index()) {
            self.dense.get(dense_index)
        } else {
            None
        }
    }

    /// Remove the component for the given entity, if it exists.
    /// This uses a swap-remove to keep the dense storage compact.
    pub fn remove(&mut self, entity: Entity) -> Option<T> {
        if let Some(dense_index) = self.index.remove(entity.index()) {
            // Determine the last index in dense storage
            let last_dense_index = self.dense.len() - 1;
            // If its already the last item in dense, we can remove without swap or index update
            if dense_index >= last_dense_index {
                // Just remove
                let component = self.dense.remove(dense_index);
                self.entities.remove(dense_index);
                Some(component)
            } else {
                // Otherwise we need to get the entity that is going to move so we can update its
                // index
                let last_entity = self.entities[last_dense_index];
                let component = self.dense.swap_remove(dense_index);
                self.entities.swap_remove(dense_index);

                // Move index
                self.index.insert(last_entity.index(), dense_index);
                Some(component)
            }
        } else {
            None
        }
    }
}

impl<T> Storage<T, DynamicIndex> {
    /// Create a new storage with a dynamic index.
    #[inline]
    pub const fn new() -> Self {
        Self::new_with_index(DynamicIndex::new())
    }
}

/// This storage dynamic Index is used to store a sparse set of (usize) indexes to (usize) indexes from another collection.
/// This can be dynamically grown by a specific block size. When adding a dense index it will use the sparse index to
/// determine which block it belongs. This can be optimized for either storage size or speed by changing the block size.
///
/// This assumes entities of the same type are added in chunks and indexes are mostly dense within a block.
/// This uses a Vec to store the indexes within each block.  If similar entities are added randomly, this may not be the best choice.
///
/// TODO: Evaluate if this is actually better than hashmaps for sparse storage of indexes.
///
#[derive(Debug, Default)]
pub struct DynamicIndex {
    /// The size of blocks to allocate when growing the index.
    block_size: usize,

    /// A collection of optional blocks, each block is a vector of optional usize indexes.
    maps: Vec<Option<Vec<Option<usize>>>>,
}

impl DynamicIndex {
    const DEFAULT_BLOCK_SIZE: usize = 256;

    /// Create a new DynamicIndex with the default block size.
    #[inline]
    pub const fn new() -> Self {
        Self::new_with_block_size(Self::DEFAULT_BLOCK_SIZE)
    }

    /// Create a new DynamicIndex with the given block size.
    #[inline]
    pub const fn new_with_block_size(block_size: usize) -> Self {
        Self {
            block_size,
            maps: Vec::new(),
        }
    }

    /// Get the block based indexs for an index.
    #[inline]
    fn indexes(&self, index: usize) -> (usize, usize) {
        let block_index = index / self.block_size;
        let within_block_index = index % self.block_size;
        (block_index, within_block_index)
    }
}

impl Index for DynamicIndex {
    /// Insert a value into the index at the given index.
    fn insert(&mut self, index: usize, value: usize) {
        let (block_index, within_block_index) = self.indexes(index);

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
            block[within_block_index] = Some(value);
        }
    }

    /// Get a value from the index at the given index if it exists.
    fn get(&self, index: usize) -> Option<usize> {
        let (block_index, within_block_index) = self.indexes(index);

        // Check if the block exists
        if block_index >= self.maps.len() {
            return None;
        }

        // Retrieve the value from the appropriate block
        if let Some(block) = &self.maps[block_index] {
            return block[within_block_index];
        }

        None
    }

    fn remove(&mut self, index: usize) -> Option<usize> {
        let (block_index, within_block_index) = self.indexes(index);

        // Check if the block exists
        if block_index >= self.maps.len() {
            return None;
        }

        // Remove the value from the appropriate block
        if let Some(block) = &mut self.maps[block_index] {
            let value = block[within_block_index];
            block[within_block_index] = None;
            return value;
        }

        None
    }
}

/// Possible alternative index implementation using a HashMap.
/// Initial testing shows this is at least an order of magnitude slower than the DynamicIndex.
/// This requires a lot more testing to be 100% sure the memory and speed tradeoffs are known.
pub struct HashIndex {
    map: HashMap<usize, usize>,
}

impl Index for HashIndex {
    fn insert(&mut self, index: usize, value: usize) {
        self.map.insert(index, value);
    }

    fn get(&self, index: usize) -> Option<usize> {
        self.map.get(&index).copied()
    }

    fn remove(&mut self, index: usize) -> Option<usize> {
        self.map.remove(&index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ecs::entity::{self};

    #[test]
    fn dynamic_index_single_block() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(10);

        // When
        index.insert(0, 10);
        index.insert(1, 30);
        index.insert(4, 40);
        index.insert(7, 70);
        index.insert(8, 80);

        // Then
        assert_eq!(index.maps.len(), 1);
        assert_eq!(index.get(0), Some(10));
        assert_eq!(index.get(1), Some(30));
        assert_eq!(index.get(4), Some(40));
        assert_eq!(index.get(5), None);
        assert_eq!(index.get(7), Some(70));
        assert_eq!(index.get(8), Some(80));
        assert_eq!(index.get(9), None);
    }

    #[test]
    fn dynamic_index_block_growth() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(4);

        // When - insert more than 2 blocks
        index.insert(0, 10);
        index.insert(1, 30);
        index.insert(4, 40);
        index.insert(7, 70);
        index.insert(8, 80);

        // Then - should grow to 3 blocks
        assert_eq!(index.maps.len(), 3);
    }

    #[test]
    fn dynamic_index_block_skipping() {
        // Given
        let mut index = DynamicIndex::new_with_block_size(4);

        // When
        index.insert(0, 10);
        index.insert(1, 30);
        index.insert(8, 80);

        // Then should grow to 3 blocks with the middle block being None
        assert_eq!(index.maps.len(), 3);
        assert_eq!(index.maps[1], None);
    }

    #[test]
    fn storage_insert_and_get() {
        // Given
        #[derive(Debug, PartialEq)]
        struct Position(f32, f32);

        let mut storage: Storage<Position, DynamicIndex> = Storage::new();

        let mut allocator = entity::Allocator::new();

        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let entity3 = allocator.alloc();

        // When
        storage.insert(entity1, Position(1.0, 2.0));
        storage.insert(entity2, Position(3.0, 4.0));

        // Then
        assert_eq!(storage.get(entity1), Some(&Position(1.0, 2.0)));
        assert_eq!(storage.get(entity2), Some(&Position(3.0, 4.0)));
        assert_eq!(storage.get(entity3), None);
    }

    #[test]
    fn storage_remove() {
        // Given
        #[derive(Debug, PartialEq)]
        struct Velocity(f32, f32);

        let mut storage: Storage<Velocity, DynamicIndex> = Storage::new();

        let mut allocator = entity::Allocator::new();

        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        storage.insert(entity1, Velocity(1.0, 2.0));
        storage.insert(entity2, Velocity(3.0, 4.0));

        // When
        let removed = storage.remove(entity1);

        // Then
        assert_eq!(removed, Some(Velocity(1.0, 2.0)));
        assert_eq!(storage.get(entity1), None);
        assert_eq!(storage.get(entity2), Some(&Velocity(3.0, 4.0)));
    }

    #[test]
    fn storage_update_existing() {
        // Given
        #[derive(Debug, PartialEq)]
        struct Health(u32);

        let mut storage: Storage<Health, DynamicIndex> = Storage::new();
        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        storage.insert(entity, Health(100));

        // When
        storage.insert(entity, Health(50));

        // Then
        assert_eq!(storage.get(entity), Some(&Health(50)));
    }

    #[test]
    fn storage_remove_nonexistent() {
        // Given
        #[derive(Debug, PartialEq)]
        struct Mana(u32);

        let mut storage: Storage<Mana, DynamicIndex> = Storage::new();
        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        // When
        let removed = storage.remove(entity);

        // Then
        assert_eq!(removed, None);
    }

    #[test]
    fn storage_remove_maintains_index() {
        // Given
        #[derive(Debug, PartialEq)]
        struct Score(u32);

        let mut storage: Storage<Score, DynamicIndex> = Storage::new();
        let mut allocator = entity::Allocator::new();

        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let entity3 = allocator.alloc();

        storage.insert(entity1, Score(10));
        storage.insert(entity2, Score(20));
        storage.insert(entity3, Score(30));

        // When - remove middle entity
        storage.remove(entity2);

        // Then - other entities should still be accessible
        assert_eq!(storage.get(entity1), Some(&Score(10)));
        assert_eq!(storage.get(entity2), None);
        assert_eq!(storage.get(entity3), Some(&Score(30)));
    }
}
