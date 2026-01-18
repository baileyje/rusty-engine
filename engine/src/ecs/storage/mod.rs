//! Type-erased columnar storage for Entity Component System (ECS).
//!
//! This module provides the foundational storage layer for the ECS, implementing efficient,
//! cache-friendly columnar storage with type erasure. It enables storing heterogeneous component
//! types in a uniform, high-performance manner while maintaining memory safety through careful
//! abstraction layers.
//!
//! # Architecture Overview
//!
//! The storage system is built on a layered architecture where each layer provides progressively
//! safer abstractions:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Application Layer                                              │
//! │  - Queries, Systems, Component Access                           │
//! └────────────────────────────┬────────────────────────────────────┘
//!                              │
//! ┌────────────────────────────▼────────────────────────────────────┐
//! │  Tables (this module)                                           │
//! │  - High-level: Multi-column entity storage (archetype pattern)  │
//! │  - Type-safe API with runtime validation                        │
//! │  - Entity → Row index mapping                                   │
//! └──────────────┬───────────────────────────┬──────────────────────┘
//!                │                           │
//!       ┌────────▼─────────┐        ┌────────▼─────────┐
//!       │  Column          │        │  Index           │
//!       │  - Type-erased   │        │  - Entity → Row  │
//!       │  - Debug checks  │        │  - O(1) lookup   │
//!       └────────┬─────────┘        └──────────────────┘
//!                │
//!       ┌────────▼─────────┐
//!       │  IndexedMemory   │
//!       │  - Raw unsafe    │
//!       │  - Zero-cost     │
//!       └──────────────────┘
//! ```
//!
//! # Core Concepts
//!
//! ## Columnar Storage (Structure of Arrays)
//!
//! Instead of storing entity data as `Vec<(Entity, ComponentA, ComponentB)>`, we use
//! **columnar storage** where each component type gets its own contiguous array:
//!
//! ```text
//!
//! Columnar (Structure of Arrays):
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Entities: [E1, E2, E3]                                      │
//! │                                                             │
//! │ Position Column: [Pos{x:1,y:2}, Pos{x:3,y:4}, Pos{x:5,y:6}] │ ← Cache-friendly!
//! │                  ▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲▲   │
//! │                  All sequential in memory                   │
//! │                                                             │
//! │ Velocity Column: [Vel{dx:0.5}, Vel{dx:-0.2}, Vel{dx:0.0}]   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! **Benefits:**
//! - **Cache efficiency**: Iterating positions reads sequential memory
//! - **SIMD potential**: Contiguous data enables vectorization
//! - **No wasted space**: No `Option<Component>` for missing components
//! - **Flexible schemas**: Easy to add/remove component types
//!
//! ## Archetype Pattern
//!
//! Entities with the **exact same set of components** are stored in the same [`Table`].
//! Each unique component combination creates a new archetype:
//!
//! ```text
//! World:
//!   Table 1: [Position, Velocity]          ← Archetype A
//!     - Entity 0: Pos, Vel
//!     - Entity 1: Pos, Vel
//!     - Entity 5: Pos, Vel
//!
//!   Table 2: [Position, Health]            ← Archetype B
//!     - Entity 2: Pos, Health
//!     - Entity 4: Pos, Health
//!
//!   Table 3: [Position, Velocity, Health]  ← Archetype C
//!     - Entity 3: Pos, Vel, Health
//! ```
//!
//! **Benefits:**
//! - Fast iteration (no sparse checks)
//! - Clear ownership (entity in exactly one table)
//! - Efficient queries (know which tables to scan)
//!
//! **Trade-off:**
//! - Adding/removing components moves entity to different table
//!
//! ## Type Erasure
//!
//! Component types are erased at runtime, allowing:
//! - Dynamic component registration
//! - Uniform storage for heterogeneous types
//! - Runtime-defined component combinations
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ Column<Position>                         │
//! │  - Knows: Layout, drop function          │
//! │  - Stores: Raw bytes (*mut u8)           │
//! │  - Type checked in debug mode            │
//! └────────────┬─────────────────────────────┘
//!              │
//!              ▼
//! ┌──────────────────────────────────────────┐
//! │ IndexedMemory                            │
//! │  - Just bytes: [u8; N * sizeof(T)]       │
//! │  - No type info at this level            │
//! └──────────────────────────────────────────┘
//! ```
//!
//! # Module Structure
//!
//! ## Public Types
//!
//! - [`Table`] - Multi-column storage for entities with the same component set
//! - [`Tables`] - Collection of tables (archetype manager)
//!
//! ## Internal Types
//!
//! - `Column` - Single-type columnar storage (type-erased)
//! - `IndexedMemory` - Low-level memory allocation (unsafe)
//! - `Cell` / `CellMut` - Type-safe component access
//! - `Row` - Type-safe row index
//! - `Index` - Entity → Row mapping trait
//! - `DynamicIndex` - Block-based entity index (default)
//! - `HashIndex` - HashMap-based entity index (fallback)
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use rusty_engine::ecs::storage::{Table, Tables};
//! use rusty_engine::ecs::{component, entity};
//! use rusty_macros::Component;
//!
//! // Define components
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Component)]
//! struct Velocity { dx: f32, dy: f32 }
//!
//! // Setup
//! let mut registry = world::TypeRegistry::new();
//! let pos_id = registry.register::<Position>();
//! let vel_id = registry.register::<Velocity>();
//!
//! // Create table for [Position, Velocity] archetype
//! let spec = component::Spec::new(vec![pos_id, vel_id]);
//! let mut table = Table::new(Id::new(0), spec, &mut registry);
//!
//! // Add entity with both components atomically
//! let mut allocator = entity::Allocator::new();
//! let entity = allocator.alloc();
//!
//! table.add_entity(
//!     entity,
//!     (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
//!     &mut registry
//! );
//!
//! // Access components via column iteration (cache-friendly!)
//! let pos_column = table.get_column(pos_id).unwrap();
//! unsafe {
//!     for pos in pos_column.iter::<Position>() {
//!         println!("Entity at ({}, {})", pos.x, pos.y);
//!     }
//! }
//!
//! // Or access specific entity components
//! unsafe {
//!     if let Some(pos) = table.get::<Position>(entity, pos_id) {
//!         println!("Entity {} at ({}, {})", entity.index(), pos.x, pos.y);
//!     }
//! }
//! ```
//!
//! # Safety Guarantees
//!
//! The storage layer maintains several critical invariants:
//!
//! ## Table Invariants
//! - **Synchronization**: `entities.len() == columns[i].len()` for all columns
//! - **Index consistency**: Entity → Row mapping always correct
//! - **Type safety**: Components match their registered types
//! - **Atomicity**: All components added/removed together
//!
//! ## Column Invariants
//! - **Initialization**: Elements [0..len) always initialized
//! - **Capacity**: `len <= capacity` at all times
//! - **Drop safety**: Removed components properly dropped
//! - **Type consistency**: All elements are the same type
//!
//! ## Memory Invariants
//! - **Valid pointers**: Non-null when capacity > 0
//! - **No double-free**: Each allocation freed exactly once
//! - **No leaks**: All elements dropped before deallocation
//! - **Layout consistency**: Matches component type layout
//!
//! # Performance Characteristics
//!
//! | Operation | Time | Notes |
//! |-----------|------|-------|
//! | Column iteration | O(n) | Cache-friendly, ~3-10ns per element |
//! | Entity lookup | O(1) | Via index, ~25-50µs typical |
//! | Add entity | O(c) | c = number of components |
//! | Remove entity | O(c) | Swap-remove, c = number of components |
//! | Get component | O(1) | Direct index, bounds-checked in debug |
//!
//! # Design Decisions
//!
//! ## Why Type Erasure?
//!
//! - **Runtime flexibility**: Components registered at runtime
//! - **Dynamic archetypes**: Unknown component combinations
//! - **Uniform storage**: Single implementation for all types
//!
//! ## Why Columnar Storage?
//!
//! - **Cache efficiency**: 80-90% of systems iterate single component types
//! - **SIMD opportunities**: Contiguous data enables vectorization
//! - **Query performance**: Common case (single-component iteration) is fastest
//!
//! ## Why Archetype Pattern?
//!
//! - **No sparse storage**: Every entity has all components in its table
//! - **Fast iteration**: No branch prediction failures from Option checks
//! - **Clear semantics**: Entity existence tied to archetype membership
//!
//! ## Trade-offs
//!
//! **Pros:**
//! - Extremely fast iteration (main ECS operation)
//! - Memory efficient (no Option overhead)
//! - Good cache locality
//!
//! **Cons:**
//! - Adding/removing components requires table migration
//! - Entity lookup is O(1) but not free (~25-50µs)
//! - More tables for diverse entity types
//!
//! # Thread Safety
//!
//! The storage types are **not** thread-safe by default:
//! - No internal synchronization
//! - Designed for single-threaded access per table
//! - Use external synchronization (e.g., RwLock) for parallel access
//!
//! # Future Work
//! - may add parallel iteration support
//! - Consider the approach used by Legion ECS to keep all component data in a single allocation and index from archetype into it.
//!
//! # Related Documentation
//!
//! For implementation details, see the source code of internal modules:
//! - `mem` - Low-level memory allocation details
//! - `column` - Type-erased column implementation  
//! - `index` - Entity-to-row index implementations
//! - `table` - Multi-column table implementation
//!

pub use location::Location;
pub use row::Row;
pub use table::Table;

use crate::ecs::{
    component,
    entity,
    storage::{
        change::{Change, ChangeResult},
        table::Id,
        unique::Uniques,
    },
    world,
};

pub mod change;
pub(crate) mod cell;
pub(crate) mod column;
pub(crate) mod index;
pub(crate) mod location;
pub(crate) mod mem;
pub(crate) mod row;
pub(crate) mod table;
pub(crate) mod unique;
pub mod view;

/// A collection of tables, each storing entities with a specific component layout.
pub struct Storage {
    /// The vec of know tables.
    tables: Vec<Table>,

    /// The unique resource storage for the world.
    uniques: Uniques,
}

impl Storage {
    /// Create a new empty Tables collection.
    #[inline]
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            uniques: Uniques::new(),
        }
    }

    pub fn create_table(&mut self, components: &[component::Info]) -> &mut Table {
        // Grab the index the table will be stored at.
        let id = table::Id::new(self.tables.len() as u32);
        let table = Table::new(id, components);
        // Create a new table from this archetype's components (moves components)
        self.tables.push(table);
        // Return a mutable reference
        self.get_mut(id)
    }

    /// Get an existing table by id, if it exists, otherwise panic.
    ///     
    /// # Panics
    /// - if the id is out of bounds
    pub fn get(&self, table_id: Id) -> &Table {
        assert!(
            table_id.index() < self.tables.len(),
            "table id out of bounds"
        );
        &self.tables[table_id.index()]
    }

    /// Get an existing mutable table, if it exists, otherwise panic.
    ///
    /// # Panics
    /// - if the id is out of bounds
    pub fn get_mut(&mut self, table_id: Id) -> &mut Table {
        assert!(
            table_id.index() < self.tables.len(),
            "table id out of bounds"
        );
        &mut self.tables[table_id.index()]
    }

    /// Get access to the resources.
    #[inline]
    pub fn uniques(&self) -> &Uniques {
        &self.uniques
    }

    /// Get mutable access to the resources.
    #[inline]
    pub fn uniques_mut(&mut self) -> &mut Uniques {
        &mut self.uniques
    }

    /// Execute a batch of storage changes, returning results for registry updates.
    ///
    /// This is the primary interface for modifying storage. Changes are executed
    /// in order, and results contain information needed to update entity registries.
    ///
    /// # Panics
    /// Panics if any table ID is invalid. The caller (World) must provide valid inputs.
    ///
    /// # Arguments
    /// * `changes` - Mutable slice of changes to execute (components are taken from changes)
    /// * `registry` - Type registry for component metadata
    ///
    /// # Returns
    /// A vector of results, one per change, in the same order as the input changes.
    pub fn execute(
        &mut self,
        changes: &mut [Change<'_>],
        registry: &world::TypeRegistry,
    ) -> Vec<ChangeResult> {
        changes
            .iter_mut()
            .map(|change| self.execute_change(change, registry))
            .collect()
    }

    /// Execute a single storage change (convenience wrapper).
    ///
    /// # Panics
    /// Panics if the table ID is invalid.
    pub fn execute_one(
        &mut self,
        mut change: Change<'_>,
        registry: &world::TypeRegistry,
    ) -> ChangeResult {
        self.execute_change(&mut change, registry)
    }

    /// Internal method to execute a single change.
    fn execute_change(
        &mut self,
        change: &mut Change<'_>,
        registry: &world::TypeRegistry,
    ) -> ChangeResult {
        match change {
            Change::Spawn {
                entity,
                table,
                components,
            } => {
                let table = self.get_mut(*table);
                let applicator = components
                    .take()
                    .expect("Spawn change components already consumed");
                let row = table.add_entity_dynamic(*entity, applicator, registry);
                ChangeResult::Spawned { row }
            }

            Change::Despawn { table, row, .. } => {
                let table = self.get_mut(*table);
                let moved = table.swap_remove_row(*row);
                ChangeResult::Despawned {
                    moved_entity: moved,
                }
            }

            Change::Migrate {
                entity,
                source,
                target,
                additions,
            } => self.execute_migration(*entity, source, *target, additions.take(), registry),
        }
    }

    /// Execute a migration change - move an entity from one table to another.
    ///
    /// This handles component add/remove operations by:
    /// 1. Copying shared component data from source to target
    /// 2. Applying any new components (additions) to target
    /// 3. Removing the entity from source (with proper drop handling)
    /// 4. Adding the entity to target
    fn execute_migration(
        &mut self,
        entity: entity::Entity,
        source: &change::MigrationSource,
        target_id: table::Id,
        additions: Option<Box<dyn change::ApplyOnce + '_>>,
        registry: &world::TypeRegistry,
    ) -> ChangeResult {
        // Collect source component IDs
        let source_component_ids: Vec<world::TypeId> =
            self.get(source.table).component_ids().collect();

        // Collect target component IDs
        let target_component_ids: Vec<world::TypeId> =
            self.get(target_id).component_ids().collect();

        // Find shared components (exist in both tables)
        let shared_ids: Vec<world::TypeId> = source_component_ids
            .iter()
            .copied()
            .filter(|id| target_component_ids.contains(id))
            .collect();

        // Find removed components (in source but not target) - these need to be dropped
        let removed_ids: Vec<world::TypeId> = source_component_ids
            .iter()
            .copied()
            .filter(|id| !target_component_ids.contains(id))
            .collect();

        // Step 1: Read bytes for shared components from source
        // Store as (component_id, bytes) pairs
        let mut shared_data: Vec<(world::TypeId, Vec<u8>)> = Vec::with_capacity(shared_ids.len());
        {
            let source_table = self.get(source.table);
            for &id in &shared_ids {
                if let Some(column) = source_table.get_column_by_id(id) {
                    let bytes = unsafe { column.read_bytes(source.row) };
                    shared_data.push((id, bytes.to_vec()));
                }
            }
        }

        // Step 2: Remove from source table
        // - Use swap_remove_no_drop for shared components (data is being moved)
        // - Use swap_remove (with drop) for removed components
        let source_moved: Option<entity::Entity>;
        {
            let source_table = self.get_mut(source.table);
            let row_index = source.row.index();
            let last_index = source_table.len() - 1;

            // Handle each column based on whether component is shared or removed
            for &id in &shared_ids {
                if let Some(column) = source_table.get_column_by_id_mut(id) {
                    // Shared component - don't drop (data was copied)
                    unsafe {
                        column.swap_remove_no_drop(source.row);
                    }
                }
            }

            for &id in &removed_ids {
                if let Some(column) = source_table.get_column_by_id_mut(id) {
                    // Removed component - drop it
                    unsafe {
                        column.swap_remove(source.row);
                    }
                }
            }

            // Swap-remove the entity
            source_table.entities_mut().swap_remove(row_index);

            // Determine if an entity was moved
            source_moved = if row_index != last_index {
                Some(source_table.entities()[row_index])
            } else {
                None
            };
        }

        // Step 3: Add to target table
        let new_row: Row;
        {
            let target_table = self.get_mut(target_id);
            new_row = Row::new(target_table.len());

            // Push shared component bytes to target columns
            for (id, bytes) in shared_data {
                if let Some(column) = target_table.get_column_by_id_mut(id) {
                    unsafe {
                        column.push_bytes(&bytes);
                    }
                }
            }

            // Apply additions (new components) if any
            if let Some(applicator) = additions {
                applicator.apply_once(target_table, registry);
            }

            // Add the entity to target
            target_table.entities_mut().push(entity);
        }

        ChangeResult::Migrated {
            new_row,
            source_moved,
        }
    }
}

impl Default for Storage {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::ecs::{component::IntoSpec, world};

    use super::*;

    #[derive(Component)]
    #[allow(dead_code)]
    struct Position {
        x: f32,
        y: f32,
    }
    #[derive(Component)]
    #[allow(dead_code)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Component)]
    #[allow(dead_code)]
    struct Health {
        hp: i32,
    }

    #[test]
    fn storage_new_is_empty() {
        let storage = Storage::new();
        assert_eq!(storage.tables.len(), 0);
    }

    #[test]
    fn storage_default_is_empty() {
        let storage = Storage::new();
        assert_eq!(storage.tables.len(), 0);
    }

    #[test]
    fn create_table_creates_new_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        let spec = <Position>::into_spec(&registry);

        // When
        let table = storage.create_table(&registry.info_for_spec(&spec));
        let table_len = table.len();

        // Then
        assert_eq!(storage.tables.len(), 1);
        assert_eq!(table_len, 0);
    }

    #[test]
    fn create_table_creates_multiple_tables() {
        // Given

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        let spec1 = <Position>::into_spec(&registry);

        let spec2 = <(Position, Velocity)>::into_spec(&registry);

        // When
        let _ = storage.create_table(&registry.info_for_spec(&spec1));
        let _ = storage.create_table(&registry.info_for_spec(&spec2));

        // Then
        assert_eq!(storage.tables.len(), 2);
    }

    #[test]
    #[should_panic(expected = "table id out of bounds")]
    fn get_returns_none_for_nonexistent_table_id() {
        // Given
        let storage = Storage::new();
        let table_id = Id::new(999);

        // When
        storage.get(table_id);
    }

    #[test]
    fn get_returns_existing_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        // When
        let table = storage.get(table_id);

        // Then
        assert_eq!(table.len(), 0);
    }

    #[test]
    #[should_panic(expected = "table id out of bounds")]
    fn get_mut_panics_for_nonexistent_table_id() {
        // Given
        let mut storage = Storage::new();

        // When
        storage.get_mut(Id::new(999));
    }

    #[test]
    fn get_mut_returns_existing_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        // When
        let table = storage.get_mut(table_id);

        // Then
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn execute_spawn_adds_entity_to_table() {
        // Given
        use crate::ecs::{entity, storage::change::Change};

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <(Position, Velocity)>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        let entity = entity::Entity::new(1.into());
        let components = (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 });

        let mut changes = [Change::spawn(entity, table_id, components)];

        // When
        let results = storage.execute(&mut changes, &registry);

        // Then
        assert_eq!(results.len(), 1);
        match &results[0] {
            super::change::ChangeResult::Spawned { row } => {
                assert_eq!(row.index(), 0);
            }
            _ => panic!("Expected Spawned result"),
        }

        let table = storage.get(table_id);
        assert_eq!(table.len(), 1);
        assert_eq!(table.entity(0.into()), Some(entity));
    }

    #[test]
    fn execute_despawn_removes_entity_from_table() {
        // Given
        use crate::ecs::{entity, storage::change::Change};

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        // Add two entities manually
        let entity1 = entity::Entity::new(1.into());
        let entity2 = entity::Entity::new(2.into());

        let table = storage.get_mut(table_id);
        table.add_entity(entity1, Position { x: 1.0, y: 2.0 }, &registry);
        table.add_entity(entity2, Position { x: 3.0, y: 4.0 }, &registry);

        assert_eq!(storage.get(table_id).len(), 2);

        // When - despawn entity1 at row 0
        let mut changes = [Change::despawn(entity1, table_id, 0.into())];
        let results = storage.execute(&mut changes, &registry);

        // Then
        assert_eq!(results.len(), 1);
        match &results[0] {
            super::change::ChangeResult::Despawned { moved_entity } => {
                // entity2 should have been moved from row 1 to row 0
                assert_eq!(*moved_entity, Some(entity2));
            }
            _ => panic!("Expected Despawned result"),
        }

        let table = storage.get(table_id);
        assert_eq!(table.len(), 1);
        // entity2 should now be at row 0
        assert_eq!(table.entity(0.into()), Some(entity2));
    }

    #[test]
    fn execute_despawn_last_entity_returns_none() {
        // Given
        use crate::ecs::{entity, storage::change::Change};

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        let entity1 = entity::Entity::new(1.into());
        let table = storage.get_mut(table_id);
        table.add_entity(entity1, Position { x: 1.0, y: 2.0 }, &registry);

        // When - despawn the only entity
        let mut changes = [Change::despawn(entity1, table_id, 0.into())];
        let results = storage.execute(&mut changes, &registry);

        // Then
        match &results[0] {
            super::change::ChangeResult::Despawned { moved_entity } => {
                // No entity moved since we removed the last one
                assert_eq!(*moved_entity, None);
            }
            _ => panic!("Expected Despawned result"),
        }

        assert_eq!(storage.get(table_id).len(), 0);
    }

    #[test]
    fn execute_one_works() {
        // Given
        use crate::ecs::{entity, storage::change::Change};

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        let entity = entity::Entity::new(1.into());
        let change = Change::spawn(entity, table_id, Position { x: 5.0, y: 6.0 });

        // When
        let result = storage.execute_one(change, &registry);

        // Then
        match result {
            super::change::ChangeResult::Spawned { row } => {
                assert_eq!(row.index(), 0);
            }
            _ => panic!("Expected Spawned result"),
        }
        assert_eq!(storage.get(table_id).len(), 1);
    }

    #[test]
    fn execute_batch_multiple_changes() {
        // Given
        use crate::ecs::{entity, storage::change::Change};

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec)).id();

        let entity1 = entity::Entity::new(1.into());
        let entity2 = entity::Entity::new(2.into());
        let entity3 = entity::Entity::new(3.into());

        let mut changes = [
            Change::spawn(entity1, table_id, Position { x: 1.0, y: 1.0 }),
            Change::spawn(entity2, table_id, Position { x: 2.0, y: 2.0 }),
            Change::spawn(entity3, table_id, Position { x: 3.0, y: 3.0 }),
        ];

        // When
        let results = storage.execute(&mut changes, &registry);

        // Then
        assert_eq!(results.len(), 3);
        assert_eq!(storage.get(table_id).len(), 3);

        // Verify rows are assigned sequentially
        for (i, result) in results.iter().enumerate() {
            match result {
                super::change::ChangeResult::Spawned { row } => {
                    assert_eq!(row.index(), i);
                }
                _ => panic!("Expected Spawned result"),
            }
        }
    }

    #[test]
    fn migrate_add_component() {
        // Given - entity with Position, add Velocity
        use crate::ecs::{
            entity,
            storage::change::{Change, MigrationSource},
        };

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        // Create source table with Position only
        let source_spec = <Position>::into_spec(&registry);
        let source_table_id = storage
            .create_table(&registry.info_for_spec(&source_spec))
            .id();

        // Create target table with Position + Velocity
        let target_spec = <(Position, Velocity)>::into_spec(&registry);
        let target_table_id = storage
            .create_table(&registry.info_for_spec(&target_spec))
            .id();

        // Add entity to source table
        let entity1 = entity::Entity::new(1.into());
        storage
            .get_mut(source_table_id)
            .add_entity(entity1, Position { x: 1.0, y: 2.0 }, &registry);

        assert_eq!(storage.get(source_table_id).len(), 1);
        assert_eq!(storage.get(target_table_id).len(), 0);

        // When - migrate entity, adding Velocity
        let source = MigrationSource::new(source_table_id, 0.into());
        let change = Change::migrate_with(
            entity1,
            source,
            target_table_id,
            Velocity { dx: 0.5, dy: 0.3 },
        );
        let result = storage.execute_one(change, &registry);

        // Then
        assert_eq!(storage.get(source_table_id).len(), 0);
        assert_eq!(storage.get(target_table_id).len(), 1);

        match result {
            super::change::ChangeResult::Migrated {
                new_row,
                source_moved,
            } => {
                assert_eq!(new_row.index(), 0);
                assert!(source_moved.is_none()); // Only entity in source
            }
            _ => panic!("Expected Migrated result"),
        }

        // Verify component data was preserved
        let target_table = storage.get(target_table_id);
        unsafe {
            let pos = target_table.get::<Position>(0.into()).unwrap();
            assert_eq!(pos.x, 1.0);
            assert_eq!(pos.y, 2.0);

            let vel = target_table.get::<Velocity>(0.into()).unwrap();
            assert_eq!(vel.dx, 0.5);
            assert_eq!(vel.dy, 0.3);
        }
    }

    #[test]
    fn migrate_remove_component() {
        // Given - entity with Position + Velocity, remove Velocity
        use crate::ecs::{
            entity,
            storage::change::{Change, MigrationSource},
        };

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        // Create source table with Position + Velocity
        let source_spec = <(Position, Velocity)>::into_spec(&registry);
        let source_table_id = storage
            .create_table(&registry.info_for_spec(&source_spec))
            .id();

        // Create target table with Position only
        let target_spec = <Position>::into_spec(&registry);
        let target_table_id = storage
            .create_table(&registry.info_for_spec(&target_spec))
            .id();

        // Add entity to source table
        let entity1 = entity::Entity::new(1.into());
        storage.get_mut(source_table_id).add_entity(
            entity1,
            (Position { x: 3.0, y: 4.0 }, Velocity { dx: 1.0, dy: 2.0 }),
            &registry,
        );

        // When - migrate entity, removing Velocity (no additions)
        let source = MigrationSource::new(source_table_id, 0.into());
        let change = Change::migrate(entity1, source, target_table_id);
        let result = storage.execute_one(change, &registry);

        // Then
        assert_eq!(storage.get(source_table_id).len(), 0);
        assert_eq!(storage.get(target_table_id).len(), 1);

        match result {
            super::change::ChangeResult::Migrated { new_row, .. } => {
                assert_eq!(new_row.index(), 0);
            }
            _ => panic!("Expected Migrated result"),
        }

        // Verify Position was preserved
        let target_table = storage.get(target_table_id);
        unsafe {
            let pos = target_table.get::<Position>(0.into()).unwrap();
            assert_eq!(pos.x, 3.0);
            assert_eq!(pos.y, 4.0);
        }
    }

    #[test]
    fn migrate_with_entity_swap() {
        // Given - two entities in source, migrate first one
        use crate::ecs::{
            entity,
            storage::change::{Change, MigrationSource},
        };

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        // Create source table with Position
        let source_spec = <Position>::into_spec(&registry);
        let source_table_id = storage
            .create_table(&registry.info_for_spec(&source_spec))
            .id();

        // Create target table with Position + Velocity
        let target_spec = <(Position, Velocity)>::into_spec(&registry);
        let target_table_id = storage
            .create_table(&registry.info_for_spec(&target_spec))
            .id();

        // Add two entities to source table
        let entity1 = entity::Entity::new(1.into());
        let entity2 = entity::Entity::new(2.into());
        storage
            .get_mut(source_table_id)
            .add_entity(entity1, Position { x: 1.0, y: 1.0 }, &registry);
        storage
            .get_mut(source_table_id)
            .add_entity(entity2, Position { x: 2.0, y: 2.0 }, &registry);

        assert_eq!(storage.get(source_table_id).len(), 2);

        // When - migrate entity1 (at row 0)
        let source = MigrationSource::new(source_table_id, 0.into());
        let change = Change::migrate_with(
            entity1,
            source,
            target_table_id,
            Velocity { dx: 0.0, dy: 0.0 },
        );
        let result = storage.execute_one(change, &registry);

        // Then
        assert_eq!(storage.get(source_table_id).len(), 1);
        assert_eq!(storage.get(target_table_id).len(), 1);

        match result {
            super::change::ChangeResult::Migrated {
                new_row,
                source_moved,
            } => {
                assert_eq!(new_row.index(), 0);
                // entity2 should have been moved to row 0 in source
                assert_eq!(source_moved, Some(entity2));
            }
            _ => panic!("Expected Migrated result"),
        }

        // Verify entity2 is now at row 0 in source
        assert_eq!(
            storage.get(source_table_id).entity(0.into()),
            Some(entity2)
        );
    }

    #[test]
    fn migrate_preserves_multiple_shared_components() {
        // Given - entity with A, B, C - migrate to table with A, B, D
        use crate::ecs::{
            entity,
            storage::change::{Change, MigrationSource},
        };

        #[derive(Component, Debug, PartialEq)]
        struct CompA(u32);
        #[derive(Component, Debug, PartialEq)]
        struct CompB(u64);
        #[derive(Component, Debug, PartialEq)]
        struct CompC(f32);
        #[derive(Component, Debug, PartialEq)]
        struct CompD(f64);

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();

        // Source: A, B, C
        let source_spec = <(CompA, CompB, CompC)>::into_spec(&registry);
        let source_table_id = storage
            .create_table(&registry.info_for_spec(&source_spec))
            .id();

        // Target: A, B, D
        let target_spec = <(CompA, CompB, CompD)>::into_spec(&registry);
        let target_table_id = storage
            .create_table(&registry.info_for_spec(&target_spec))
            .id();

        // Add entity with A=1, B=2, C=3.0
        let entity1 = entity::Entity::new(1.into());
        storage.get_mut(source_table_id).add_entity(
            entity1,
            (CompA(1), CompB(2), CompC(3.0)),
            &registry,
        );

        // When - migrate, adding D=4.0 (C gets removed)
        let source = MigrationSource::new(source_table_id, 0.into());
        let change = Change::migrate_with(entity1, source, target_table_id, CompD(4.0));
        storage.execute_one(change, &registry);

        // Then - verify A and B preserved, D added
        let target_table = storage.get(target_table_id);
        unsafe {
            assert_eq!(target_table.get::<CompA>(0.into()), Some(&CompA(1)));
            assert_eq!(target_table.get::<CompB>(0.into()), Some(&CompB(2)));
            assert_eq!(target_table.get::<CompD>(0.into()), Some(&CompD(4.0)));
        }
    }
}
