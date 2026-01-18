# Storage Change/Migration System Design

## Overview

Design a unified system for modeling storage operations as "changes" that can be applied to storage while maintaining table invariants. This enables entity spawn, despawn, and component add/remove operations through a consistent interface.

## Current Architecture Summary

```
World
├── entity_allocator: Allocator      // Entity ID lifecycle
├── entities: Registry               // Spawned state + Location(archetype, table, row)
├── storage: Storage                 // Vec<Table> for columnar data
├── archetypes: Registry             // Spec → Archetype → Table mapping
└── resources: TypeRegistry          // TypeId → Info mapping
```

**Key patterns:**
- `Set` trait: Write components to a target (tuples supported via macros)
- `Spec`: Sorted Vec<TypeId> identifying component combinations
- `Info`: Storage metadata (layout, drop_fn) for type-erased columns
- `Location`: (archetype_id, table_id, row) for O(1) entity lookup
- Swap-remove: O(1) removal with entity relocation tracking

## Design Goals

1. **Uniform change model**: All storage mutations expressed as `Change` variants
2. **Exclusive access**: Changes require `&mut Storage`
3. **Spec-driven**: Use component specs to determine involved tables
4. **World orchestration**: World provides context (specs, tables), Storage executes
5. **Invariant preservation**: Changes maintain table synchronization invariants

## Proposed Change Types

```rust
pub enum Change<'a> {
    /// Spawn: Add entity to a single table
    Spawn {
        entity: Entity,
        table: table::Id,
        /// Option allows .take() during batch execution
        components: Option<Box<dyn ApplyOnce + 'a>>,
    },

    /// Despawn: Remove entity from a single table
    Despawn {
        entity: Entity,
        table: table::Id,
        row: Row,
    },

    /// Migrate: Move entity between tables (component add/remove)
    Migrate {
        entity: Entity,
        source: MigrationSource,
        target: table::Id,
        /// New components to add (None for component removal)
        additions: Option<Box<dyn ApplyOnce + 'a>>,
    },
}

pub struct MigrationSource {
    pub table: table::Id,
    pub row: Row,
}

impl<'a> Change<'a> {
    pub fn spawn<S: Set + 'a>(entity: Entity, table: table::Id, components: S) -> Self {
        Change::Spawn {
            entity,
            table,
            components: Some(Box::new(components)),
        }
    }

    pub fn despawn(entity: Entity, table: table::Id, row: Row) -> Self {
        Change::Despawn { entity, table, row }
    }

    pub fn migrate(
        entity: Entity,
        source: MigrationSource,
        target: table::Id,
    ) -> Self {
        Change::Migrate { entity, source, target, additions: None }
    }

    pub fn migrate_with<S: Set + 'a>(
        entity: Entity,
        source: MigrationSource,
        target: table::Id,
        additions: S,
    ) -> Self {
        Change::Migrate {
            entity,
            source,
            target,
            additions: Some(Box::new(additions)),
        }
    }
}
```

## Key Design: ApplyOnce Trait

Extend the `Set` pattern for one-shot application (ownership transfer):

```rust
/// Like Set, but takes ownership and can only be applied once.
/// Used for type-erased component application in changes.
pub trait ApplyOnce {
    fn apply_once(self: Box<Self>, target: &mut dyn SetTarget);
}

impl<S: Set> ApplyOnce for S {
    fn apply_once(self: Box<Self>, target: &mut dyn SetTarget) {
        (*self).apply(target);
    }
}
```

## Storage API Extension

```rust
impl Storage {
    /// Execute a batch of changes, returning results for registry updates.
    ///
    /// # Panics
    /// Panics if any table ID is invalid (World must provide valid inputs).
    pub fn execute(
        &mut self,
        changes: &mut [Change<'_>],
        registry: &TypeRegistry,
    ) -> Vec<ChangeResult> {
        changes.iter_mut().map(|change| {
            match change {
                Change::Spawn { entity, table, components } => {
                    let table = self.get_mut(*table);
                    let row = table.add_entity_dynamic(*entity, components.take(), registry);
                    ChangeResult::Spawned { row }
                }

                Change::Despawn { table, row, .. } => {
                    let table = self.get_mut(*table);
                    let moved = table.swap_remove_row(*row);
                    ChangeResult::Despawned { moved_entity: moved }
                }

                Change::Migrate { entity, source, target, additions } => {
                    self.execute_migration(*entity, source, *target, additions.take(), registry)
                }
            }
        }).collect()
    }

    /// Execute a single change (convenience wrapper).
    pub fn execute_one(&mut self, change: Change<'_>, registry: &TypeRegistry) -> ChangeResult {
        self.execute(&mut [change], registry).pop().unwrap()
    }
}

pub enum ChangeResult {
    Spawned { row: Row },
    Despawned { moved_entity: Option<Entity> },
    Migrated {
        new_row: Row,
        source_moved: Option<Entity>,
    },
}
```

## Migration Implementation Strategy

### Challenge: Reading Components Type-Erased

When migrating, we need to copy component data from source table to target table without knowing the concrete types. Options:

**Option A: Byte-level copy (Recommended)**
- Use `Info.layout()` to memcpy component bytes
- Use `Info.drop_fn()` to clean up source without dropping (since data moved)
- Requires `Column::read_bytes(row) -> &[u8]` and `Column::push_bytes(&[u8])`

**Option B: View + Set pattern**
- Create a "migration set" that reads from source and writes to target
- More type-safe but requires knowing component types at compile time

**Option C: Component extraction API**
- `Table::extract_row(row) -> ExtractedComponents`
- `ExtractedComponents` implements `Set` for reinsertion
- Clean abstraction but may have performance overhead

### Proposed: Byte-level Migration

```rust
impl Storage {
    fn execute_migration(
        &mut self,
        entity: Entity,
        source: MigrationSource,
        target_id: table::Id,
        additions: Option<Box<dyn ApplyOnce + '_>>,
        registry: &TypeRegistry,
    ) -> ChangeResult {
        // Safety: We have &mut self, so exclusive access to all tables
        let source_table = &mut self.tables[source.table.index()];
        let target_table = &mut self.tables[target_id.index()];

        // 1. For each column in source that exists in target:
        //    - Read bytes at source.row
        //    - Push bytes to target column

        // 2. Apply additions (new components) to target

        // 3. Swap-remove from source (handles entity vector too)

        // 4. Add entity to target's entity vector

        ChangeResult::Migrated { ... }
    }
}
```

## World Orchestration

World remains the coordinator, using Specs to determine table IDs:

```rust
impl World {
    pub fn add_component<C: Component>(&mut self, entity: Entity, component: C) {
        let location = self.entities.location(entity).unwrap();
        let current_spec = self.archetypes.get(location.archetype_id()).spec();

        // Compute new spec
        let component_id = self.resources.id::<C>();
        let new_spec = current_spec.with(component_id);

        // Get or create target archetype/table
        let (target_archetype, target_table) = self.ensure_archetype(&new_spec);

        // Build and execute change
        let change = Change::Migrate {
            entity,
            source: MigrationSource {
                table: location.table_id(),
                row: location.row(),
            },
            target: target_table,
            additions: Some(Box::new(component)),
        };

        let result = self.storage.execute(change, &self.resources);

        // Update entity location
        if let ChangeResult::Migrated { new_row, source_moved } = result {
            self.entities.set_location(entity, Location::new(
                target_archetype,
                target_table,
                new_row,
            ));

            // Handle entity that was moved during swap-remove
            if let Some(moved) = source_moved {
                self.update_moved_entity_location(moved, location);
            }
        }
    }

    pub fn remove_component<C: Component>(&mut self, entity: Entity) {
        // Similar pattern but new_spec = current_spec.without(component_id)
        // No additions in the change
    }
}
```

## Spec Extensions Needed

```rust
impl Spec {
    /// Create a new spec with an additional component
    pub fn with(&self, id: TypeId) -> Spec {
        let mut ids = self.ids.clone();
        if !ids.contains(&id) {
            ids.push(id);
            ids.sort();
        }
        Spec { ids }
    }

    /// Create a new spec without a component
    pub fn without(&self, id: TypeId) -> Spec {
        let ids: Vec<_> = self.ids.iter()
            .copied()
            .filter(|&i| i != id)
            .collect();
        Spec { ids }
    }

    /// Get the components in self that are not in other
    pub fn difference(&self, other: &Spec) -> Spec {
        let ids: Vec<_> = self.ids.iter()
            .copied()
            .filter(|id| !other.ids.contains(id))
            .collect();
        Spec { ids }
    }

    /// Get the components in both self and other
    pub fn intersection(&self, other: &Spec) -> Spec {
        let ids: Vec<_> = self.ids.iter()
            .copied()
            .filter(|id| other.ids.contains(id))
            .collect();
        Spec { ids }
    }
}
```

## Column Extensions for Byte-Level Operations

```rust
impl Column {
    /// Read raw bytes for a component at given row
    pub unsafe fn read_bytes(&self, row: Row) -> &[u8] {
        let offset = row.index() * self.info.layout().size();
        let ptr = self.data.as_ptr().add(offset);
        std::slice::from_raw_parts(ptr, self.info.layout().size())
    }

    /// Push raw bytes as a new component (caller ensures type correctness)
    pub unsafe fn push_bytes(&mut self, bytes: &[u8]) {
        debug_assert_eq!(bytes.len(), self.info.layout().size());
        self.data.reserve(1, self.info.layout());
        let offset = self.len * self.info.layout().size();
        let ptr = self.data.as_mut_ptr().add(offset);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        self.len += 1;
    }

    /// Remove at row without calling drop (for moves)
    pub unsafe fn swap_remove_no_drop(&mut self, row: Row) {
        // Swap bytes with last row, decrement len
        // Don't call drop_fn - data is being moved, not destroyed
    }
}
```

## Files to Modify

1. **`engine/src/ecs/storage/mod.rs`**
   - Add `Change` enum and `ChangeResult`
   - Add `Storage::execute()` method
   - Add migration implementation

2. **`engine/src/ecs/storage/column.rs`**
   - Add `read_bytes()`, `push_bytes()`, `swap_remove_no_drop()`

3. **`engine/src/ecs/component/spec.rs`**
   - Add `with()`, `without()`, `difference()`, `intersection()`

4. **`engine/src/ecs/component/set.rs`**
   - Add `ApplyOnce` trait (or integrate into existing Set)

5. **`engine/src/ecs/world/mod.rs`**
   - Add `add_component()`, `remove_component()` methods
   - Refactor `spawn()` to use `Change::Spawn`
   - Refactor `despawn()` to use `Change::Despawn`

6. **`engine/src/ecs/storage/table.rs`**
   - Add `add_entity_dynamic()` for type-erased spawn
   - Add helpers for migration (column iteration, etc.)

## Design Decisions

1. **Batch changes**: Yes - `Storage::execute()` accepts `&mut [Change]` for batching
2. **Error handling**: Trust World - panic on invalid inputs (debug assertions)
3. **Change validation**: Storage trusts World to provide valid table IDs
4. **ZST handling**: Zero-sized types need special handling in byte operations
5. **Deferred execution**: Immediate only for now (can add command buffer later)

## Implementation Order

1. Add `Spec::with()`, `without()` - foundation for migration
2. Add `Column` byte-level operations - enables type-erased data movement
3. Create `Change` enum and `ChangeResult` - the abstraction
4. Implement `Storage::execute()` for Spawn/Despawn - validate pattern
5. Implement migration - the core new functionality
6. Add `World::add_component()`, `remove_component()` - user-facing API
7. Refactor existing spawn/despawn to use Change pattern (optional)
