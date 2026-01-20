//! Type-erased columnar storage for Entity Component System (ECS).
//!
//! This module provides the foundational storage layer for the ECS, implementing efficient,
//! cache-friendly columnar storage with type erasure. It enables storing heterogeneous component
//! types in a uniform, high-performance manner while maintaining memory safety through careful
//! abstraction layers.
//!
//! # Architecture Overview
//!
//! The storage system is the central authority for entity and component data management.
//! It coordinates archetypes, tables, and entity locations in a layered architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Application Layer                                              │
//! │  - Queries, Systems, Component Access                           │
//! └────────────────────────────┬────────────────────────────────────┘
//!                              │
//! ┌────────────────────────────▼────────────────────────────────────┐
//! │  Storage (this module)                                          │
//! │  - Owns Tables, Archetypes, and Entity registry                 │
//! │  - Handles spawn/despawn and component migration                │
//! │  - Coordinates archetype → table mapping                        │
//! └────────┬───────────────────┬───────────────────┬────────────────┘
//!          │                   │                   │
//! ┌────────▼────────┐ ┌────────▼────────┐ ┌───────▼────────┐
//! │  Archetypes     │ │  Entities       │ │  Tables        │
//! │  - Spec → Table │ │  - Entity →     │ │  - Columnar    │
//! │  - Component    │ │    Location     │ │    storage     │
//! │    combinations │ │  - Generation   │ │  - Per-arch    │
//! └─────────────────┘ └─────────────────┘ └───────┬────────┘
//!                                                 │
//!                                        ┌────────▼─────────┐
//!                                        │  Column          │
//!                                        │  - Type-erased   │
//!                                        │  - Debug checks  │
//!                                        └────────┬─────────┘
//!                                                 │
//!                                        ┌────────▼─────────┐
//!                                        │  IndexedMemory   │
//!                                        │  - Raw unsafe    │
//!                                        │  - Zero-cost     │
//!                                        └──────────────────┘
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
//! - [`Storage`] - Central container owning tables, archetypes, and entity registry
//! - [`Table`] - Multi-column storage for entities with the same component set
//! - [`Location`] - Entity storage location (archetype, table, row)
//! - [`Row`] - Type-safe row index within a table
//!
//! ## Internal Types
//!
//! - `Archetypes` - Registry mapping component specs to tables
//! - `Entities` - Tracks spawned entities and their locations
//! - `Column` - Single-type columnar storage (type-erased)
//! - `IndexedMemory` - Low-level memory allocation (unsafe)
//! - `Cell` / `CellMut` - Type-safe component access
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use rusty_engine::ecs::storage::Storage;
//! use rusty_engine::ecs::{entity, world};
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
//! let mut storage = Storage::new();
//! let registry = world::TypeRegistry::new();
//! let allocator = entity::Allocator::new();
//!
//! // Spawn an entity with components
//! let entity = allocator.alloc();
//! storage.spawn_entity(
//!     entity,
//!     (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
//!     &registry
//! );
//!
//! // Access entity location
//! let location = storage.location_for(entity).unwrap();
//! let table = storage.get_table(location.table_id());
//!
//! // Iterate components via views (cache-friendly!)
//! for (entity, (pos, vel)) in unsafe { table.iter_views::<(&Position, &Velocity)>() } {
//!     println!("Entity {:?} at ({}, {})", entity, pos.x, pos.y);
//! }
//!
//! // Add a single component (migrates to new archetype)
//! #[derive(Component)]
//! struct Health { hp: i32 }
//! storage.add_components(entity, Health { hp: 100 }, &registry);
//!
//! // Add multiple components at once
//! #[derive(Component)]
//! struct Armor { defense: i32 }
//! #[derive(Component)]
//! struct Shield { block: i32 }
//! storage.add_components(entity, (Armor { defense: 50 }, Shield { block: 25 }), &registry);
//!
//! // Remove a single component (migrates to new archetype)
//! storage.remove_components::<Velocity>(entity, &registry);
//!
//! // Remove multiple components at once
//! storage.remove_components::<(Health, Armor)>(entity, &registry);
//!
//! // Despawn entity
//! storage.despawn_entity(entity);
//! ```
//!
//! # Safety Guarantees
//!
//! The storage layer maintains several critical invariants:
//!
//! ## Storage Invariants
//! - **Location consistency**: Entity locations always point to valid (table, row) pairs
//! - **Archetype uniqueness**: Each component spec maps to exactly one archetype/table
//! - **Migration atomicity**: Component add/remove fully completes or doesn't happen
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
//! ## Migration Safety
//! When an entity migrates between archetypes (via `add_components`/`remove_components`):
//! - **Shared data transfer**: Components common to both archetypes are byte-copied (no drop)
//! - **Removed data cleanup**: Components only in source archetype are properly dropped
//! - **Swap-remove handling**: When source table uses swap-remove, the moved entity's
//!   location is updated to maintain consistency
//!
//! # Performance Characteristics
//!
//! | Operation | Time | Notes |
//! |-----------|------|-------|
//! | Column iteration | O(n) | Cache-friendly, ~3-10ns per element |
//! | Entity lookup | O(1) | Via index, ~25-50ns typical |
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
//! - Entity lookup is O(1) but not free (~25-50ns)
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
//! - `archetype` - Archetype registry mapping specs to tables
//! - `entity` - Entity location tracking and generation validation
//! - `table` - Multi-column table implementation
//! - `column` - Type-erased column implementation
//! - `mem` - Low-level memory allocation details
//! - `view` - Component view trait and iterators
//!

pub use location::Location;
pub use row::Row;
pub use table::Id as TableId;
pub use table::Table;

use crate::ecs::entity::Entity;
use crate::ecs::{
    component::{self, Set},
    storage::unique::Uniques,
    world,
};

pub(crate) mod archetype;
pub(crate) mod cell;
pub(crate) mod column;
pub(crate) mod entity;
pub(crate) mod location;
pub(crate) mod mem;
pub(crate) mod row;
pub(crate) mod table;
pub(crate) mod unique;
pub mod value;
pub mod view;

/// Central storage container for the ECS, managing all entity and component data.
///
/// `Storage` is the authoritative source for:
/// - **Tables**: Columnar storage for each archetype's component data
/// - **Archetypes**: Registry mapping component specifications to tables
/// - **Entities**: Tracks which entities are spawned and their storage locations
/// - **Uniques**: Singleton resources accessible across systems
///
/// # Responsibilities
///
/// Storage handles the core data operations:
/// - Spawning entities with initial components
/// - Despawning entities and cleaning up their data
/// - Component migration (add/remove) between archetypes
/// - Location tracking for O(1) entity lookups
///
/// # Example
///
/// ```rust,ignore
/// let mut storage = Storage::new();
/// let registry = world::TypeRegistry::new();
/// let allocator = entity::Allocator::new();
///
/// // Spawn entity
/// let entity = allocator.alloc();
/// storage.spawn_entity(entity, (Position { x: 0.0, y: 0.0 },), &registry);
///
/// // Add single component (triggers migration)
/// storage.add_components(entity, Velocity { dx: 1.0, dy: 0.0 }, &registry);
///
/// // Add multiple components at once
/// storage.add_components(entity, (Health { hp: 100 }, Armor { defense: 10 }), &registry);
///
/// // Query location
/// let loc = storage.location_for(entity).unwrap();
/// let table = storage.get_table(loc.table_id());
/// ```
pub struct Storage {
    /// Collection of tables, each storing entities with a specific component layout.
    tables: Vec<Table>,

    /// Unique (singleton) resources for the world.
    uniques: Uniques,

    /// Registry of archetypes mapping component specs to tables.
    archetypes: archetype::Archetypes,

    /// Tracks spawned entities and their storage locations.
    entities: entity::Entities,
}

impl Storage {
    /// Create a new empty Tables collection.
    #[inline]
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            uniques: Uniques::new(),
            archetypes: archetype::Archetypes::new(),
            entities: entity::Entities::new(),
        }
    }

    #[inline]
    pub fn entities(&self) -> &entity::Entities {
        &self.entities
    }

    #[inline]
    pub fn archetypes(&self) -> &archetype::Archetypes {
        &self.archetypes
    }

    pub fn create_table(&mut self, components: &[component::Info]) -> TableId {
        // Grab the index the table will be stored at.
        let id = table::Id::new(self.tables.len() as u32);
        // Create table
        self.tables.push(Table::new(id, components));
        id
    }

    /// Get an existing table by id, if it exists, otherwise panic.
    ///     
    /// # Panics
    /// - if the id is out of bounds
    pub fn get_table(&self, table_id: TableId) -> &Table {
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
    pub fn get_table_mut(&mut self, table_id: TableId) -> &mut Table {
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

    /// Spawn multiple entities sharing the same component types in a batch.
    pub fn spawn_entities<S: component::Set>(
        &mut self,
        entities: impl Iterator<Item = (Entity, S)>,
        types: &world::TypeRegistry,
    ) {
        // If there is nothing to add, return early.
        if entities.size_hint().0 == 0 {
            return;
        }
        // Convert to peekable Iterator to grab spec from first item.
        let mut peekable = entities.peekable();
        // Get the spec for the values.
        let spec = peekable.peek().map(|(_, v)| v.as_spec(types)).unwrap();
        // Get the archetype and table for this component spec, creating them if they don't exist.
        let (archetype_id, table_id) = self.get_storage_target(&spec, types);
        let table = self.get_table_mut(table_id);
        // Add rows to the table and collect their locations for entity registration.
        table
            .add_entities(peekable)
            .into_iter()
            .for_each(|(row, entity)| {
                // Mark the entity as spawned in the world.
                self.entities
                    .spawn_at(entity, Location::new(archetype_id, table_id, row));
            });
    }

    /// Spawn a new entity with the given set of components.
    pub fn spawn_entity<S: component::Set>(
        &mut self,
        entity: Entity,
        values: S,
        types: &world::TypeRegistry,
    ) {
        // Get the spec for this values.
        let spec = values.as_spec(types);
        // Get the archetype and table for this component spec, creating them if they don't exist.
        let (archetype_id, table_id) = self.get_storage_target(&spec, types);
        let table = self.get_table_mut(table_id);
        // Add rows to the table and collect their locations for entity registration.
        let row = table.add_entity(entity, values);
        // Mark the entity as spawned in the world.
        self.entities
            .spawn_at(entity, Location::new(archetype_id, table_id, row));
    }

    pub fn despawn_entity(&mut self, entity: Entity) {
        if let Some(location) = self.entities.location(entity) {
            let table = self.get_table_mut(location.table_id());

            // Remove the row from the table, swapping in the last row.
            let moved = table.swap_remove_row(location.row());

            // If another entity was moved, update its location.
            if let Some(moved) = moved {
                self.entities.set_location(moved, location);
            }
            // Despawn the entity.
            self.entities.despawn(entity);
        }
    }

    /// Add one or more components to an existing entity.
    ///
    /// This migrates the entity to a new archetype that includes the new components.
    /// Accepts either a single component or a tuple of components.
    ///
    /// # Returns
    /// - `true` if the components were added
    /// - `false` if:
    ///   - The entity doesn't exist
    ///   - The entity already has ANY of the specified components
    ///   - The component set is empty
    ///
    /// # Examples
    /// ```rust,ignore
    /// // Add single component
    /// storage.add_components(entity, Velocity { dx: 1.0, dy: 0.0 }, &registry);
    ///
    /// // Add multiple components at once
    /// storage.add_components(entity, (Velocity { dx: 1.0, dy: 0.0 }, Health { hp: 100 }), &registry);
    /// ```
    pub fn add_components<S: component::Set>(
        &mut self,
        entity: Entity,
        components: S,
        types: &world::TypeRegistry,
    ) -> bool {
        // Check if entity is spawned
        let source = match self.entities.location(entity) {
            Some(loc) => loc,
            None => return false,
        };

        // Get the specification for columns to add
        let add_spec = components.as_spec(types);

        // No reason to process an empty add
        if add_spec.is_empty() {
            return false;
        }

        // Get the current archetype's spec
        let source_archetype = self
            .archetypes()
            .get(source.archetype_id())
            .expect("entity location references valid archetype");

        // Check if entity already has these components
        if source_archetype.components().contains_any(&add_spec) {
            return false;
        }

        // Execute the row migration between the two tables.
        self.execute_migration(
            entity,
            source,
            source_archetype.components().union(&add_spec),
            components,
            types,
        );

        true
    }

    /// Remove one or more components from an existing entity.
    ///
    /// This migrates the entity to a new archetype that excludes the specified components.
    /// Accepts either a single component type or a tuple of component types.
    ///
    /// # Returns
    /// - `true` if the components were removed
    /// - `false` if:
    ///   - The entity doesn't exist
    ///   - The entity doesn't have ALL of the specified components
    ///   - The component set is empty
    ///
    /// # Examples
    /// ```rust,ignore
    /// // Remove single component
    /// storage.remove_components::<Velocity>(entity, &registry);
    ///
    /// // Remove multiple components at once
    /// storage.remove_components::<(Velocity, Health)>(entity, &registry);
    /// ```
    pub fn remove_components<S: component::IntoSpec>(
        &mut self,
        entity: Entity,
        types: &world::TypeRegistry,
    ) -> bool {
        // Check if entity is spawned
        let source = match self.entities.location(entity) {
            Some(loc) => loc,
            None => return false,
        };

        // Get the specification for columns to remove.
        let remove_spec = S::into_spec(types);

        // No reason to process an empty remove
        if remove_spec.is_empty() {
            return false;
        }

        // Get the current archetype's spec
        let source_archetype = self
            .archetypes()
            .get(source.archetype_id())
            .expect("entity location references valid archetype");

        // Check if entity has this component
        if !source_archetype.components().contains_all(&remove_spec) {
            return false;
        }

        // Execute the row migration between the two tables.
        self.execute_migration(
            entity,
            source,
            source_archetype.components().difference(&remove_spec),
            (),
            types,
        );

        true
    }

    /// Remove one or more components from an existing entity by spec
    ///
    /// This migrates the entity to a new archetype that excludes the specified components.
    /// Accepts either a single component type or a tuple of component types.
    ///
    /// # Returns
    /// - `true` if the components were removed
    /// - `false` if:
    ///   - The entity doesn't exist
    ///   - The entity doesn't have ALL of the specified components
    ///   - The component set is empty
    ///
    /// # Examples
    /// ```rust,ignore
    /// // Remove single component
    /// storage.remove_components::<Velocity>(entity, &registry);
    ///
    /// // Remove multiple components at once
    /// storage.remove_components::<(Velocity, Health)>(entity, &registry);
    /// ```
    pub fn remove_components_dynamic(
        &mut self,
        entity: Entity,
        spec: &component::Spec,
        types: &world::TypeRegistry,
    ) -> bool {
        // Check if entity is spawned
        let source = match self.entities.location(entity) {
            Some(loc) => loc,
            None => return false,
        };

        // No reason to process an empty remove
        if spec.is_empty() {
            return false;
        }

        // Get the current archetype's spec
        let source_archetype = self
            .archetypes()
            .get(source.archetype_id())
            .expect("entity location references valid archetype");

        // Check if entity has this component
        if !source_archetype.components().contains_all(spec) {
            return false;
        }

        // Execute the row migration between the two tables.
        self.execute_migration(
            entity,
            source,
            source_archetype.components().difference(spec),
            (),
            types,
        );

        true
    }

    /// Execute a migration - move an entity from one archetype/table to another.
    ///
    /// This is the core operation for `add_components` and `remove_components`. It safely
    /// transfers an entity between tables while preserving shared component data.
    ///
    /// # Process
    /// 1. Get or create the target archetype/table for the new component spec
    /// 2. Identify shared components (exist in both source and target)
    /// 3. Extract shared component data from source (byte-copy, no drop)
    /// 4. Remove entity from source table via swap-remove
    /// 5. Update the swapped entity's location if one was moved
    /// 6. Add entity to target table with extracted data + new components
    /// 7. Update the migrated entity's location
    ///
    /// # Safety
    /// - Shared components are byte-copied and not dropped in source
    /// - Removed components (in source but not target) are properly dropped
    /// - Swap-remove in source table may move another entity; its location is updated
    /// - The migrated entity's location is updated to point to target table
    fn execute_migration<S: component::Set>(
        &mut self,
        entity: Entity,
        source: Location,
        target: component::Spec,
        additions: S,
        types: &world::TypeRegistry,
    ) {
        // Debug: Verify that shared components + additions = target spec
        // This ensures add_entity_from_extract will write to all columns
        #[cfg(debug_assertions)]
        {
            let additions_spec = additions.as_spec(types);
            let source_spec = self
                .archetypes
                .get(source.archetype_id())
                .expect("valid source archetype")
                .components();
            let shared_spec = source_spec.intersection(&target);
            let combined = shared_spec.union(&additions_spec);
            debug_assert_eq!(
                combined, target,
                "Migration invariant violated: shared components ({:?}) + additions ({:?}) != target ({:?})",
                shared_spec, additions_spec, target
            );
        }

        // Get or create the target archetype/table
        let (target_archetype_id, target_table_id) = self.get_storage_target(&target, types);

        // Get the component specs for source archetypes
        let source_spec = self
            .archetypes
            .get(source.archetype_id())
            .expect("valid source archetype")
            .components();

        // Find shared components (exist in both tables)
        let shared_spec = source_spec.intersection(&target);

        // Extract shared component data from source and remove the entity
        // Store as (component_id, bytes) pairs
        let (extract, moved) = self
            .get_table_mut(source.table_id())
            .extract_and_swap_row(source.row(), &shared_spec);

        // Update the moved entity's location in source, if one was moved.
        if let Some(moved) = moved {
            self.entities.set_location(moved, source);
        }

        // Step 2: Add to target table
        let new_row = self
            .get_table_mut(target_table_id)
            .add_entity(entity, extract.with(additions));

        // Update the migrated entity's location
        self.entities.set_location(
            entity,
            Location::new(target_archetype_id, target_table_id, new_row),
        );
    }

    /// Get the storage location for the given entity, if it's spawned.
    pub fn location_for(&self, entity: Entity) -> Option<Location> {
        self.entities.location(entity)
    }

    /// Get or create the archetype and table for a given component specification.
    ///
    /// This is the core lookup/creation method for finding where entities with a
    /// particular set of components should be stored. If an archetype already exists
    /// for the spec, returns its IDs. Otherwise, creates a new archetype and table.
    ///
    /// # Returns
    /// A tuple of `(archetype::Id, TableId)` for the matching or newly created storage.
    pub fn get_storage_target(
        &mut self,
        spec: &component::Spec,
        resources: &world::TypeRegistry,
    ) -> (archetype::Id, TableId) {
        match self.archetypes.get_by_spec(spec) {
            Some(archetype) => (archetype.id(), archetype.table_id()),
            None => {
                let table_id = self.create_table(&resources.info_for_spec(spec));
                let archetype_id = self.archetypes.create(spec.clone(), table_id);
                (archetype_id, table_id)
            }
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
        let table_id = storage.create_table(&registry.info_for_spec(&spec));
        let table = storage.get_table(table_id);
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
        let table_id = TableId::new(999);

        // When
        storage.get_table(table_id);
    }

    #[test]
    fn get_returns_existing_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec));

        // When
        let table = storage.get_table(table_id);

        // Then
        assert_eq!(table.len(), 0);
    }

    #[test]
    #[should_panic(expected = "table id out of bounds")]
    fn get_mut_panics_for_nonexistent_table_id() {
        // Given
        let mut storage = Storage::new();

        // When
        storage.get_table_mut(TableId::new(999));
    }

    #[test]
    fn get_mut_returns_existing_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let spec = <Position>::into_spec(&registry);
        let table_id = storage.create_table(&registry.info_for_spec(&spec));

        // When
        let table = storage.get_table_mut(table_id);

        // Then
        assert_eq!(table.len(), 0);
    }

    // ==================== Spawn/Despawn Tests ====================

    #[test]
    fn spawn_entity_creates_archetype_and_table() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();

        // When
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // Then
        assert!(storage.entities().is_spawned(entity));
        assert_eq!(storage.tables.len(), 1);
        assert_eq!(
            storage
                .archetypes()
                .table_ids_for(&<Position>::into_spec(&registry))
                .len(),
            1
        );
    }

    #[test]
    fn spawn_multiple_entities_same_archetype() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();

        // When
        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        let e3 = allocator.alloc();
        storage.spawn_entity(e1, Position { x: 1.0, y: 1.0 }, &registry);
        storage.spawn_entity(e2, Position { x: 2.0, y: 2.0 }, &registry);
        storage.spawn_entity(e3, Position { x: 3.0, y: 3.0 }, &registry);

        // Then - all in same table
        assert_eq!(storage.tables.len(), 1);
        assert_eq!(storage.get_table(TableId::new(0)).len(), 3);

        // Verify locations
        assert_eq!(storage.location_for(e1).unwrap().row(), 0.into());
        assert_eq!(storage.location_for(e2).unwrap().row(), 1.into());
        assert_eq!(storage.location_for(e3).unwrap().row(), 2.into());
    }

    #[test]
    fn spawn_multiple_entities_in_batch() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();

        // When
        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        let e3 = allocator.alloc();
        storage.spawn_entities(
            vec![
                (e1, Position { x: 1.0, y: 1.0 }),
                (e2, Position { x: 2.0, y: 2.0 }),
                (e3, Position { x: 3.0, y: 3.0 }),
            ]
            .into_iter(),
            &registry,
        );

        // Then - all in same table
        assert_eq!(storage.tables.len(), 1);
        assert_eq!(storage.get_table(TableId::new(0)).len(), 3);

        // Verify locations
        assert_eq!(storage.location_for(e1).unwrap().row(), 0.into());
        assert_eq!(storage.location_for(e2).unwrap().row(), 1.into());
        assert_eq!(storage.location_for(e3).unwrap().row(), 2.into());
    }

    #[test]
    fn despawn_entity_removes_from_storage() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When
        storage.despawn_entity(entity);

        // Then
        assert!(!storage.entities().is_spawned(entity));
        assert_eq!(storage.get_table(TableId::new(0)).len(), 0);
    }

    #[test]
    fn despawn_entity_updates_swapped_entity_location() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();

        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        let e3 = allocator.alloc();
        storage.spawn_entity(e1, Position { x: 1.0, y: 1.0 }, &registry);
        storage.spawn_entity(e2, Position { x: 2.0, y: 2.0 }, &registry);
        storage.spawn_entity(e3, Position { x: 3.0, y: 3.0 }, &registry);

        // e1 at row 0, e2 at row 1, e3 at row 2
        assert_eq!(storage.location_for(e1).unwrap().row(), 0.into());
        assert_eq!(storage.location_for(e3).unwrap().row(), 2.into());

        // When - despawn e1 (e3 should swap into row 0)
        storage.despawn_entity(e1);

        // Then
        assert!(!storage.entities().is_spawned(e1));
        assert_eq!(storage.location_for(e2).unwrap().row(), 1.into()); // unchanged
        assert_eq!(storage.location_for(e3).unwrap().row(), 0.into()); // moved from 2 to 0
    }

    #[test]
    fn despawn_nonexistent_entity_is_noop() {
        // Given
        let mut storage = Storage::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();

        // When - despawn entity that was never spawned
        storage.despawn_entity(entity);

        // Then - no panic, no effect
        assert!(!storage.entities().is_spawned(entity));
    }

    // ==================== Component Migration Tests ====================

    #[test]
    fn add_component_migrates_entity() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When
        let added = storage.add_components(entity, Velocity { dx: 0.5, dy: 0.3 }, &registry);

        // Then
        assert!(added);
        assert_eq!(storage.tables.len(), 2); // new archetype created

        // Verify component data preserved
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            let vel = table.get::<Velocity>(loc.row()).unwrap();
            assert_eq!(pos.x, 1.0);
            assert_eq!(pos.y, 2.0);
            assert_eq!(vel.dx, 0.5);
            assert_eq!(vel.dy, 0.3);
        }
    }

    #[test]
    fn add_component_returns_false_if_already_exists() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - try to add Position again
        let added = storage.add_components(entity, Position { x: 5.0, y: 6.0 }, &registry);

        // Then
        assert!(!added);
        // Original data unchanged
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            assert_eq!(pos.x, 1.0);
        }
    }

    #[test]
    fn add_component_returns_false_for_nonexistent_entity() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();

        // When
        let added = storage.add_components(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // Then
        assert!(!added);
    }

    #[test]
    fn remove_component_migrates_entity() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &registry,
        );

        // When
        let removed = storage.remove_components::<Velocity>(entity, &registry);

        // Then
        assert!(removed);
        assert_eq!(storage.tables.len(), 2); // new archetype created

        // Verify Position preserved, Velocity gone
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            assert_eq!(pos.x, 1.0);
            assert_eq!(pos.y, 2.0);
            assert!(table.get::<Velocity>(loc.row()).is_none());
        }
    }

    #[test]
    fn remove_component_returns_false_if_not_present() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When
        let removed = storage.remove_components::<Velocity>(entity, &registry);

        // Then
        assert!(!removed);
    }

    #[test]
    fn migration_updates_swapped_entity_location() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();

        // Spawn two entities with same archetype
        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        storage.spawn_entity(e1, Position { x: 1.0, y: 1.0 }, &registry);
        storage.spawn_entity(e2, Position { x: 2.0, y: 2.0 }, &registry);

        // e1 at row 0, e2 at row 1
        assert_eq!(storage.location_for(e1).unwrap().row(), 0.into());
        assert_eq!(storage.location_for(e2).unwrap().row(), 1.into());

        // When - migrate e1 (e2 should swap into row 0)
        storage.add_components(e1, Velocity { dx: 0.5, dy: 0.3 }, &registry);

        // Then - e2's location should be updated
        assert_eq!(storage.location_for(e2).unwrap().row(), 0.into());

        // Verify e2's data still accessible
        let loc = storage.location_for(e2).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            assert_eq!(pos.x, 2.0);
        }
    }

    #[test]
    fn migration_single_entity_no_swap() {
        // Given - only one entity in source table
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - migrate (no other entity to swap)
        storage.add_components(entity, Velocity { dx: 0.5, dy: 0.3 }, &registry);

        // Then - should succeed without issues
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert_eq!(table.get::<Position>(loc.row()).unwrap().x, 1.0);
            assert_eq!(table.get::<Velocity>(loc.row()).unwrap().dx, 0.5);
        }
    }

    #[test]
    fn sequential_migrations() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - multiple migrations
        storage.add_components(entity, Velocity { dx: 0.5, dy: 0.3 }, &registry);
        storage.add_components(entity, Health { hp: 100 }, &registry);
        storage.remove_components::<Velocity>(entity, &registry);

        // Then - final state: Position + Health
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert!(table.get::<Position>(loc.row()).is_some());
            assert!(table.get::<Health>(loc.row()).is_some());
            assert!(table.get::<Velocity>(loc.row()).is_none());
        }
    }

    #[test]
    fn migration_with_drop_tracking() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, Component)]
        struct DropTracker(Arc<AtomicUsize>);

        impl Drop for DropTracker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        let counter = Arc::new(AtomicUsize::new(0));

        storage.spawn_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, DropTracker(counter.clone())),
            &registry,
        );
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // When - remove DropTracker component
        storage.remove_components::<DropTracker>(entity, &registry);

        // Then - DropTracker should have been dropped exactly once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn migration_preserves_shared_components_no_extra_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, Component)]
        struct DropCounter {
            counter: Arc<AtomicUsize>,
        }

        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.counter.fetch_add(1, Ordering::SeqCst);
            }
        }

        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        let counter = Arc::new(AtomicUsize::new(0));

        storage.spawn_entity(
            entity,
            DropCounter {
                counter: counter.clone(),
            },
            &registry,
        );

        // When - add component (DropCounter should NOT be dropped during migration)
        storage.add_components(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // Then - DropCounter not dropped (shared component, byte-copied)
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Cleanup - despawn should drop it
        storage.despawn_entity(entity);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // ==================== Multi-Component Add Tests ====================

    #[test]
    fn add_multiple_components_at_once() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - add two components at once
        let added = storage.add_components(
            entity,
            (Velocity { dx: 0.5, dy: 0.3 }, Health { hp: 100 }),
            &registry,
        );

        // Then
        assert!(added);
        assert_eq!(storage.tables.len(), 2); // one migration

        // Verify all component data
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            let vel = table.get::<Velocity>(loc.row()).unwrap();
            let health = table.get::<Health>(loc.row()).unwrap();
            assert_eq!(pos.x, 1.0);
            assert_eq!(pos.y, 2.0);
            assert_eq!(vel.dx, 0.5);
            assert_eq!(vel.dy, 0.3);
            assert_eq!(health.hp, 100);
        }
    }

    #[test]
    fn add_multiple_components_returns_false_if_any_exists() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        // Entity already has Velocity
        storage.spawn_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.1, dy: 0.1 }),
            &registry,
        );

        // When - try to add (Velocity, Health) - Velocity already exists
        let added = storage.add_components(
            entity,
            (Velocity { dx: 0.5, dy: 0.3 }, Health { hp: 100 }),
            &registry,
        );

        // Then - should fail because Velocity already exists
        assert!(!added);

        // Verify original Velocity unchanged, Health not added
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let vel = table.get::<Velocity>(loc.row()).unwrap();
            assert_eq!(vel.dx, 0.1); // original value
            assert!(table.get::<Health>(loc.row()).is_none());
        }
    }

    #[test]
    fn add_multiple_components_empty_tuple_returns_false() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - try to add empty tuple
        let added = storage.add_components(entity, (), &registry);

        // Then
        assert!(!added);
    }

    #[test]
    fn add_three_components_at_once() {
        // Given
        #[derive(Component, Debug, PartialEq)]
        struct Tag;

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - add three components at once
        let added = storage.add_components(
            entity,
            (Velocity { dx: 0.5, dy: 0.3 }, Health { hp: 100 }, Tag),
            &registry,
        );

        // Then
        assert!(added);

        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert!(table.get::<Position>(loc.row()).is_some());
            assert!(table.get::<Velocity>(loc.row()).is_some());
            assert!(table.get::<Health>(loc.row()).is_some());
            assert!(table.get::<Tag>(loc.row()).is_some());
        }
    }

    // ==================== Multi-Component Remove Tests ====================

    #[test]
    fn remove_multiple_components_at_once() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { hp: 100 },
            ),
            &registry,
        );

        // When - remove two components at once
        let removed = storage.remove_components::<(Velocity, Health)>(entity, &registry);

        // Then
        assert!(removed);

        // Verify only Position remains
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            assert_eq!(pos.x, 1.0);
            assert!(table.get::<Velocity>(loc.row()).is_none());
            assert!(table.get::<Health>(loc.row()).is_none());
        }
    }

    #[test]
    fn remove_multiple_components_returns_false_if_any_missing() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        // Entity has Position and Velocity, but NOT Health
        storage.spawn_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &registry,
        );

        // When - try to remove (Velocity, Health) - Health doesn't exist
        let removed = storage.remove_components::<(Velocity, Health)>(entity, &registry);

        // Then - should fail because entity doesn't have ALL components
        assert!(!removed);

        // Verify Velocity still exists (nothing was removed)
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert!(table.get::<Velocity>(loc.row()).is_some());
        }
    }

    #[test]
    fn remove_multiple_components_empty_tuple_returns_false() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - try to remove empty tuple
        let removed = storage.remove_components::<()>(entity, &registry);

        // Then
        assert!(!removed);
    }

    #[test]
    fn remove_three_components_at_once() {
        // Given
        #[derive(Component)]
        struct Tag;

        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { hp: 100 },
                Tag,
            ),
            &registry,
        );

        // When - remove three components at once
        let removed = storage.remove_components::<(Velocity, Health, Tag)>(entity, &registry);

        // Then
        assert!(removed);

        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert!(table.get::<Position>(loc.row()).is_some());
            assert!(table.get::<Velocity>(loc.row()).is_none());
            assert!(table.get::<Health>(loc.row()).is_none());
            assert!(table.get::<Tag>(loc.row()).is_none());
        }
    }

    #[test]
    fn multi_component_add_then_remove() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        storage.spawn_entity(entity, Position { x: 1.0, y: 2.0 }, &registry);

        // When - add multiple, then remove multiple
        storage.add_components(
            entity,
            (Velocity { dx: 0.5, dy: 0.3 }, Health { hp: 100 }),
            &registry,
        );
        storage.remove_components::<(Velocity, Health)>(entity, &registry);

        // Then - back to just Position
        let loc = storage.location_for(entity).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            assert!(table.get::<Position>(loc.row()).is_some());
            assert!(table.get::<Velocity>(loc.row()).is_none());
            assert!(table.get::<Health>(loc.row()).is_none());
        }
    }

    #[test]
    fn multi_component_migration_with_drop_tracking() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, Component)]
        struct DropTracker1(Arc<AtomicUsize>);
        #[derive(Debug, Component)]
        struct DropTracker2(Arc<AtomicUsize>);

        impl Drop for DropTracker1 {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Drop for DropTracker2 {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        storage.spawn_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                DropTracker1(counter1.clone()),
                DropTracker2(counter2.clone()),
            ),
            &registry,
        );
        assert_eq!(counter1.load(Ordering::SeqCst), 0);
        assert_eq!(counter2.load(Ordering::SeqCst), 0);

        // When - remove both trackers at once
        storage.remove_components::<(DropTracker1, DropTracker2)>(entity, &registry);

        // Then - both should be dropped exactly once
        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn multi_component_add_preserves_existing_no_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, Component)]
        struct DropCounter {
            counter: Arc<AtomicUsize>,
        }

        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.counter.fetch_add(1, Ordering::SeqCst);
            }
        }

        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();
        let entity = allocator.alloc();
        let counter = Arc::new(AtomicUsize::new(0));

        storage.spawn_entity(
            entity,
            DropCounter {
                counter: counter.clone(),
            },
            &registry,
        );

        // When - add multiple components (DropCounter should NOT be dropped)
        storage.add_components(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &registry,
        );

        // Then - DropCounter not dropped (shared component, byte-copied)
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Cleanup
        storage.despawn_entity(entity);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn multi_component_updates_swapped_entity_location() {
        // Given
        let mut storage = Storage::new();
        let registry = world::TypeRegistry::new();
        let allocator = crate::ecs::entity::Allocator::new();

        // Spawn two entities with same archetype
        let e1 = allocator.alloc();
        let e2 = allocator.alloc();
        storage.spawn_entity(e1, Position { x: 1.0, y: 1.0 }, &registry);
        storage.spawn_entity(e2, Position { x: 2.0, y: 2.0 }, &registry);

        // e1 at row 0, e2 at row 1
        assert_eq!(storage.location_for(e1).unwrap().row(), 0.into());
        assert_eq!(storage.location_for(e2).unwrap().row(), 1.into());

        // When - add multiple components to e1 (e2 should swap into row 0)
        storage.add_components(
            e1,
            (Velocity { dx: 0.5, dy: 0.3 }, Health { hp: 100 }),
            &registry,
        );

        // Then - e2's location should be updated
        assert_eq!(storage.location_for(e2).unwrap().row(), 0.into());

        // Verify e2's data still accessible
        let loc = storage.location_for(e2).unwrap();
        let table = storage.get_table(loc.table_id());
        unsafe {
            let pos = table.get::<Position>(loc.row()).unwrap();
            assert_eq!(pos.x, 2.0);
        }
    }
}
