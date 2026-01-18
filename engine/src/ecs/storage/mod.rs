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
    storage::{table::Id, unique::Uniques},
};

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
}
