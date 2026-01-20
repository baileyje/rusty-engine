use std::any::TypeId;

use crate::ecs::{
    component,
    entity::{self},
    storage::{
        column::Column,
        row::Row,
        value::Values,
        view::{self, View},
    },
    world,
};

/// The identifier for a table in storage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Id(u32);

impl Id {
    /// Create a new Id with the given unique identifier.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Id(id)
    }

    /// Get the index for this Id..
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

/// A table stores entities and their component data in a columnar format.
/// Each column stores all instances of a single component type for all entities in the table.
/// This provides cache-friendly iteration when processing components.
///
/// # Example Usage
///
/// ```rust,ignore
/// use rusty_engine::ecs::storage::Table;
/// use rusty_engine::ecs::component;
/// use rusty_engine::ecs::entity;
/// use rusty_engine::ecs::world;
/// use rusty_macros::Component;
///
/// // Setup Components
/// let registry = world::TypeRegistry::new();
/// #[derive(Component)]
/// struct Position { x: f32, y: f32 }
///
/// #[derive(Component)]
/// struct Velocity { dx: f32, dy: f32 }
///
/// registry.register_component::<Position>();
/// registry.register_component::<Velocity>();
///
/// // Construct the table from component info (not spec)
/// let mut table = Table::new(
///     table::Id::new(0),
///     &[
///         registry.get_info_of::<Position>().unwrap(),
///         registry.get_info_of::<Velocity>().unwrap(),
///     ],
/// );
///
/// // Create an entity
/// let mut allocator = entity::Allocator::new();
/// let entity = allocator.alloc();
///
/// // Add an entity to the table
/// table.add_entity(
///     entity,
///     (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
///     &registry,
/// );
/// ```
///
/// # Invariants
/// - `entities.len()` must equal the length of every column
pub struct Table {
    /// The unique identifier for this table.
    id: Id,

    /// The entities stored in this table (one per row).
    entities: Vec<entity::Entity>,

    /// The component columns. Each column stores all instances of one component type.
    columns: Vec<Column>,
    // TODO: Evaluate if a map or sparse set is worth it for faster lookups. Using array search for
    // now since number of components per table is expected to be small. Need benchmarks. Perhaps
    // the behavior can be configurable based on column count.
}

impl Table {
    /// Create a new table for the given component specification and component registry.
    /// Each component in the spec gets its own column.
    ///
    /// # Panics
    /// - Panics if any component in the spec is not registered in the provided registry.
    pub fn new(id: Id, components: &[component::Info]) -> Self {
        Self {
            id,
            entities: Vec::new(),
            columns: components.iter().map(|info| Column::new(*info)).collect(),
        }
    }

    /// Get the unique identifier for this table.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Add a multiple entities in a single transaction.
    ///
    /// The values must contain exactly the components specified in the table's
    /// component specification. All components are added atomically.
    ///
    /// # Safety - The caller must ensure that:
    /// - The values provided for each entity match the table's component specification.
    ///
    /// # Example
    /// ```ignore
    /// table.add_entities([
    ///     (entity, (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 })),
    ///     (entity, (Position { x: 2.0, y: 2.0 }, Velocity { dx: 0.8, dy: 1.3 })),
    /// ]);d
    /// ```
    pub fn add_entities<V: Values>(
        &mut self,
        entities: impl IntoIterator<Item = (entity::Entity, V)>,
    ) -> Vec<(Row, entity::Entity)> {
        let data_iter = entities.into_iter();

        // Capture the first entity row.
        let mut row = Row::new(self.entities.len());

        // Reserve space for all entities
        self.entities.reserve(data_iter.size_hint().0);

        // Reserve space in each column
        for column in self.columns.iter_mut() {
            column.reserve(data_iter.size_hint().0);
        }

        // Iterate through each entity and its values and apply to the table.
        let mut results = Vec::with_capacity(data_iter.size_hint().0);
        for (entity, values) in data_iter {
            // push the entity
            self.entities.push(entity);

            // Apply the values to the row
            values.apply(self, row);

            // Store the result
            results.push((row, entity));

            // Increment the row for the next entity
            row = row.increment();
        }

        // Update column lengths after batch add
        for column in self.columns.iter_mut() {
            // Safety: We have reserved and pushed data for all entities.
            unsafe { column.set_len(self.entities.len()) };
        }

        // Verify we have kept the entity/column lengths consistent.
        #[cfg(debug_assertions)]
        self.verify_invariants();

        results
    }

    /// Add a single entity with components. This is a shortcut to `add_entities` with a single
    /// item.
    ///
    /// See `add_entities` for details.
    ///
    /// # Example
    /// ```ignore
    /// table.add_entity(
    ///     entity, (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
    /// );
    /// ```
    pub fn add_entity<V: Values>(&mut self, entity: entity::Entity, values: V) -> Row {
        self.add_entities([(entity, values)]).first().unwrap().0
    }

    /// Add an entity using a type-erased component data from an extraction and possible additional
    /// data.
    ///
    /// This is used by the storage migration system where component types
    /// are not known at compile time.
    ///
    /// # Arguments
    /// * `entity` - The entity to add
    /// * `extract` - A vector of (TypeId, bytes) tuples representing the extracted components to add
    /// * `additions` - A set of additional components to add
    ///
    /// # Safety Contract
    /// The caller must ensure that `extract` + `additions` together cover ALL columns in this table.
    /// This is verified at the Storage layer via spec validation in `execute_migration`.
    ///
    /// # Returns
    /// The row where the entity was placed.
    pub(crate) fn add_entity_from_extract<V: Values>(
        &mut self,
        entity: entity::Entity,
        extract: Vec<(world::TypeId, Vec<u8>)>,
        additions: V,
    ) -> Row {
        // Capture the entity row
        let row = Row::new(self.entities.len());

        // Reserve space in each column
        for column in self.columns.iter_mut() {
            column.reserve(1);
        }

        // Push extracted component bytes to target columns
        for (id, bytes) in extract {
            if let Some(column) = self.get_column_by_id_mut(id) {
                unsafe {
                    column.write_bytes(row, &bytes);
                }
            } else {
                // Extract contains a component ID not in this table - this is a bug
                debug_assert!(
                    false,
                    "extract contains component ID {:?} not present in table",
                    id
                );
            }
        }

        // Apply additions (new components) if any
        // Note: additions.apply() will panic if a component doesn't exist in the table
        additions.apply(self, row);

        // Add the entity once the components are all added
        self.entities.push(entity);

        // Update the length after new row.
        for column in self.columns.iter_mut() {
            // Safety - The row was reserved above.
            unsafe { column.set_len(self.entities.len()) };
        }
        // Verify we have kept the entity/column lengths consistent
        #[cfg(debug_assertions)]
        self.verify_invariants();

        row
    }

    /// Get the number of entities (rows) in the table.
    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Check if the table is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Get the entities stored in this table.
    #[inline]
    pub fn entities(&self) -> &[entity::Entity] {
        &self.entities
    }

    /// Get mutable access to the entities vector.
    ///
    /// This is primarily used by the migration system for direct manipulation.
    #[inline]
    pub fn entities_mut(&mut self) -> &mut Vec<entity::Entity> {
        &mut self.entities
    }

    /// Get an entity stored at a specific row.
    ///
    /// Returns `None` if:
    /// - the row is not in this table
    #[inline]
    pub fn entity(&self, row: Row) -> Option<entity::Entity> {
        self.entities.get(row.index()).copied()
    }

    /// Get a reference to a column by component type `C`.
    ///
    /// Returns `None` if:
    /// - the component column is not in this table
    #[inline]
    pub fn get_column<C: component::Component>(&self) -> Option<&Column> {
        self.columns
            .iter()
            .find(|&col| col.info().type_id() == TypeId::of::<C>())
    }

    /// Get a mutable reference to a column by component type `C`.
    ///
    /// Returns `None` if:
    /// - the component column is not in this table
    #[inline]
    pub fn get_column_mut<C: component::Component>(&mut self) -> Option<&mut Column> {
        self.columns
            .iter_mut()
            .find(|col| col.info().type_id() == TypeId::of::<C>())
    }

    /// Get a reference to a column by component ID.
    ///
    /// Returns `None` if the component is not in this table.
    #[inline]
    pub fn get_column_by_id(&self, id: world::TypeId) -> Option<&Column> {
        self.columns.iter().find(|col| col.info().id() == id)
    }

    /// Get a mutable reference to a column by component ID.
    ///
    /// Returns `None` if the component is not in this table.
    #[inline]
    pub fn get_column_by_id_mut(&mut self, id: world::TypeId) -> Option<&mut Column> {
        self.columns.iter_mut().find(|col| col.info().id() == id)
    }

    /// Get the component IDs for all columns in this table.
    #[inline]
    pub fn component_ids(&self) -> impl Iterator<Item = world::TypeId> + '_ {
        self.columns.iter().map(|col| col.info().id())
    }

    /// Get a component reference for a specific row.
    ///
    /// Returns `None` if:
    /// - the row is not in this table
    /// - The component ID is not part of this table's specification
    ///
    /// # Performance
    /// This method performs O(1) index lookup to find the entity's row.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - Component type `C` matches the type registered for `component_id`
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the component type.
    #[inline]
    pub unsafe fn get<C: component::Component>(&self, row: Row) -> Option<&C> {
        unsafe { self.get_column::<C>().and_then(|c| c.get(row)) }
    }

    /// Get a mutable component reference for a specific row.
    ///
    /// Returns `None` if:
    /// - The entity is not in this table
    /// - The component ID is not part of this table's specification
    ///
    /// # Performance
    /// This method performs O(1) index lookup to find the entity's row.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - Component type `C` matches the type registered for `component_id`
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the component type.
    #[inline]
    pub unsafe fn get_mut<C: component::Component>(&mut self, row: Row) -> Option<&mut C> {
        unsafe { self.get_column_mut::<C>().and_then(|c| c.get_mut(row)) }
    }

    /// Remove an entity and its components at the given row using swap-remove.
    /// Returns the entity moved into the row, or None if the row is the last. This differs from
    /// Vec as it returns the moved entity for updating its location.
    ///
    /// This maintains the invariant that all columns stay synchronized.
    /// The removed components are properly dropped.
    ///
    /// # Panics
    /// In debug builds, panics if the row is out of bounds.
    pub fn swap_remove_row(&mut self, row: Row) -> Option<entity::Entity> {
        let index = row.index();
        debug_assert!(index < self.entities.len(), "row index out of bounds");

        // Capture the last index for fixing moved entity later
        let last_index = self.entities.len() - 1;

        // Swap-remove the entity in the list
        self.entities.swap_remove(index);

        // Remove from all columns
        for column in self.columns.iter_mut() {
            // SAFETY: row < entities.len() == columns.len() (by invariant)
            unsafe {
                column.swap_remove(row);
            }
        }

        if last_index == index {
            // Removed the last entity, nothing was moved
            return None;
        }
        // Get the entity that was moved into the removed row
        let moved_entity = self.entities[index];

        // Verify we have kept the entity/column lengths consistent
        #[cfg(debug_assertions)]
        self.verify_invariants();

        Some(moved_entity)
    }

    /// Extract raw component data for the specified row and extraction spec. The retunred data
    /// will be a type erased Vec of (TypeId, bytes) tuples for each extracted component. This will
    /// remove the entity from the table using swap-remove. Any components not part of the
    /// extraction will be dropped. If an entity was moved as part of this operation, it is
    /// returned.
    ///
    /// # Panics
    /// In debug builds, panics if the row is out of bounds.
    pub(crate) fn extract_and_swap_row(
        &mut self,
        row: Row,
        to_extract: &component::Spec,
    ) -> (Vec<(world::TypeId, Vec<u8>)>, Option<entity::Entity>) {
        debug_assert!(row.index() < self.entities.len(), "row index out of bounds");

        // Capture the last index for fixing moved entity later
        let last_index = self.entities.len() - 1;
        let row_index = row.index();

        // Swap-remove the entity in the list
        self.entities.swap_remove(row_index);

        // Create the full table spec for comparison
        let table_spec = component::Spec::new(
            self.columns
                .iter()
                .map(|col| col.info().id())
                .collect::<Vec<_>>(),
        );

        let mut shared_data: Vec<(world::TypeId, Vec<u8>)> = Vec::with_capacity(to_extract.len());
        // Iterate through all fields in the spec to extract the value and remove it from the column without drop.
        for &id in to_extract.ids() {
            if let Some(column) = self.get_column_by_id_mut(id) {
                // SAFETY: row < entities.len() == columns.len() (by invariant)
                let bytes = unsafe { column.read_bytes(row) };
                shared_data.push((id, bytes.to_vec()));
                // Shared component - don't drop (data was copied)
                unsafe {
                    column.swap_remove_no_drop(row);
                }
            }
            // TODO: Should we panic if no colmn is found? Unlikely as the components come from the
            // the storage module migration logic.
        }

        // Determine any columns that are not part of the extract and remove with drop.
        let removed_spec = table_spec.difference(to_extract);
        for &id in removed_spec.ids() {
            if let Some(column) = self.get_column_by_id_mut(id) {
                // Removed component - drop it
                unsafe {
                    column.swap_remove(row);
                }
            }
        }

        // Special case for last row removal
        if last_index == row_index {
            // Removed the last entity, nothing was moved
            return (shared_data, None);
        }
        // Get the entity that was moved into the removed row
        let moved_entity = self.entities[row_index];

        // Verify we have kept the entity/column lengths consistent
        #[cfg(debug_assertions)]
        self.verify_invariants();

        (shared_data, Some(moved_entity))
    }

    /// Verify that all columns have the same length as entities.
    /// This is useful for debugging and testing.
    ///
    /// # Panics
    /// Panics if any column length doesn't match the entity count.
    #[cfg(debug_assertions)]
    pub fn verify_invariants(&self) {
        let expected_len = self.entities.len();
        for (i, col) in self.columns.iter().enumerate() {
            assert_eq!(
                col.len(),
                expected_len,
                "Column {} length {} doesn't match entity count {}",
                i,
                col.len(),
                expected_len
            );
        }
    }

    /// Get a view of components for a specific row.
    ///
    /// This provides a type-safe, ergonomic way to access one or more components
    /// for a single entity. The view type determines which components are fetched.
    ///
    /// # Type Parameters
    ///
    /// * `V` - The view type, typically a tuple of component references like `(&A, &B, &mut C)`
    ///
    /// # Returns
    ///
    /// * `Some(view)` - If all requested components exist in this table at the given row
    /// * `None` - If any component is missing or the row is invalid
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in the view match the types registered in the component registry
    /// - The row index is valid (< table.len())
    /// - For immutable views only: component types are correctly specified
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Single component
    /// let pos: Option<&Position> = unsafe { table.view(row, &registry) };
    ///
    /// // Multiple components
    /// let view: Option<(&Position, &Velocity)> = unsafe {
    ///     table.view(row, &registry)
    /// };
    ///
    /// if let Some((pos, vel)) = view {
    ///     println!("Entity at ({}, {})", pos.x, pos.y);
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// In debug builds, panics if:
    /// - Component type doesn't match the registered type
    /// - Row index is out of bounds
    pub unsafe fn view<'a, V: View<'a>>(&'a self, row: Row) -> Option<V> {
        unsafe { V::fetch(self, row) }
    }

    /// Get a mutable view of components for a specific row.
    ///
    /// This is the mutable variant of [`view`](Table::view), allowing modification
    /// of component data. Supports mixed mutability (some `&C`, some `&mut C`).
    ///
    /// # Type Parameters
    ///
    /// * `V` - The view type, typically a tuple of component references like `(&A, &mut B, &mut C)`
    ///
    /// # Returns
    ///
    /// * `Some(view)` - If all requested components exist in this table at the given row
    /// * `None` - If any component is missing or the row is invalid
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in the view match the types registered in the component registry
    /// - The row index is valid (< table.len())
    /// - No aliasing violations occur (don't request the same component multiple times)
    /// - Rust's borrowing rules are upheld for mutable references
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Mutable single component
    /// let health: Option<&mut Health> = unsafe {
    ///     table.view_mut(row, &registry)
    /// };
    ///
    /// if let Some(health) = health {
    ///     health.value -= 10;
    /// }
    ///
    /// // Mixed mutability
    /// let view: Option<(&Position, &mut Velocity)> = unsafe {
    ///     table.view_mut(row, &registry)
    /// };
    ///
    /// if let Some((pos, vel)) = view {
    ///     vel.dx += pos.x * 0.1; // Modify velocity based on position
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// In debug builds, panics if:
    /// - Component type doesn't match the registered type
    /// - Row index is out of bounds
    pub unsafe fn view_mut<'a, V: View<'a>>(&'a mut self, row: Row) -> Option<V> {
        unsafe { V::fetch_mut(self, row) }
    }

    /// Create an iterator over views for all entities in this table.
    ///
    /// This provides an ergonomic way to iterate over component data for all entities
    /// in the table using the specified view type.
    ///
    /// # Type Parameters
    ///
    /// * `V` - The view type (e.g., `&Position`, `(&Position, &Velocity)`)
    ///
    /// # Returns
    ///
    /// An iterator that yields views for each entity in the table. Due to the archetype
    /// pattern, if the view's components match the table, all rows will yield successfully.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in V match the types registered in the registry
    /// - The table contains all components required by V
    ///
    /// If components are missing, the iterator will yield `None` for all rows.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Iterate over single component
    /// let iter: view::ViewIter<&Position> = unsafe {
    ///     table.iter_views(&registry)
    /// };
    /// for pos in iter {
    ///     println!("Position: ({}, {})", pos.x, pos.y);
    /// }
    ///
    /// // Iterate over multiple components
    /// let iter: view::ViewIter<(&Position, &Velocity)> = unsafe {
    ///     table.iter_views(&registry)
    /// };
    /// for (pos, vel) in iter {
    ///     println!("At ({}, {}) moving ({}, {})", pos.x, pos.y, vel.dx, vel.dy);
    /// }
    ///
    /// // Type inference often works
    /// for (pos, vel) in unsafe { table.iter_views::<(&Position, &Velocity)>(&registry) } {
    ///     println!("Entity position: ({}, {})", pos.x, pos.y);
    /// }
    /// ```
    pub unsafe fn iter_views<'a, V: View<'a>>(&'a self) -> view::ViewIter<'a, V> {
        unsafe { view::ViewIter::new(self) }
    }

    /// Create a mutable iterator over views for all entities in this table.
    ///
    /// This provides an ergonomic way to iterate and modify component data for all
    /// entities in the table using the specified view type.
    ///
    /// # Type Parameters
    ///
    /// * `V` - The view type (e.g., `&mut Position`, `(&Position, &mut Velocity)`)
    ///
    /// # Returns
    ///
    /// A mutable iterator that yields views for each entity in the table. Due to the
    /// archetype pattern, if the view's components match the table, all rows will yield
    /// successfully.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in V match the types registered in the registry
    /// - The table contains all components required by V
    /// - No aliasing violations occur (V doesn't request same component multiple times)
    ///
    /// If components are missing, the iterator will yield `None` for all rows.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Iterate and modify single component
    /// let iter: view::ViewIterMut<&mut Position> = unsafe {
    ///     table.iter_views_mut(&registry)
    /// };
    /// for pos in iter {
    ///     pos.x += 1.0;
    ///     pos.y += 1.0;
    /// }
    ///
    /// // Mixed mutability - read position, modify velocity
    /// let iter: view::ViewIterMut<(&Position, &mut Velocity)> = unsafe {
    ///     table.iter_views_mut(&registry)
    /// };
    /// for (pos, vel) in iter {
    ///     vel.dx = -pos.x * 0.1;  // Update velocity based on position
    ///     vel.dy = -pos.y * 0.1;
    /// }
    ///
    /// // Physics update pattern
    /// let dt = 0.016; // 60 FPS
    /// for (pos, vel) in unsafe { table.iter_views_mut::<(&mut Position, &Velocity)>(&registry) } {
    ///     pos.x += vel.dx * dt;
    ///     pos.y += vel.dy * dt;
    /// }
    /// ```
    pub unsafe fn iter_views_mut<'a, V: View<'a>>(&'a mut self) -> view::ViewIterMut<'a, V> {
        unsafe { view::ViewIterMut::new(self) }
    }

    /// Apply a component value to the appropriate column in the table.
    ///
    /// # Panics
    /// - If the component ID is not valid for this table.
    /// - If the type `C` doesn't match the component ID's registered type.
    /// - In debug builds, panics if the row is out of bounds.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - There is a valid column for the given component ID.
    /// - The type `C` matches the column's component type (validated at runtime).
    /// - Row is valid for this table.
    pub fn apply_value<C: component::Component>(&mut self, row: Row, value: C) {
        let column = self.get_column_mut::<C>().expect("component not in table");
        // SAFETY: Write provides validation of type and row bounds in debugg. Caller must ensure
        // all safety conditions are met for this method.
        unsafe {
            column.write(row, value);
        }
    }
}

#[cfg(test)]
mod tests {

    use rusty_macros::Component;

    use crate::ecs::world;

    use super::*;

    #[derive(Component, Copy, Clone, Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Copy, Clone, Debug, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Component, Copy, Clone, Debug, PartialEq)]
    struct Health {
        value: i32,
    }

    #[derive(Component, Copy, Clone, Debug, PartialEq)]
    struct Score {
        points: u32,
    }

    #[test]
    fn table_creation() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();

        // When
        let table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
            ],
        );

        // Then
        assert_eq!(table.len(), 0);
        assert_eq!(table.columns.len(), 2);
        // Columns are None until first component is added
        assert!(table.get_column::<Position>().is_some());
        assert!(table.get_column::<Velocity>().is_some());
    }

    #[test]
    fn table_entity_management() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        // When
        table.add_entities([
            (entity1, Health { value: 100 }),
            (entity2, Health { value: 75 }),
        ]);

        // Then
        assert_eq!(table.len(), 2);
        assert_eq!(table.entities()[0], entity1);
        assert_eq!(table.entities()[1], entity2);

        let column = table.get_column::<Health>().unwrap();
        unsafe {
            let mut itr = column.iter::<Health>();
            assert_eq!(itr.next().unwrap().value, 100);
        }

        #[cfg(debug_assertions)]
        table.verify_invariants();
    }

    #[test]
    fn table_with_multiple_component_types() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();
        registry.register_component::<Health>();

        // Create a table with three different component types
        let mut table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
                registry.get_info_of::<Health>().unwrap(),
            ],
        );

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        // When

        // Add first entity with all components atomically
        table.add_entities([
            (
                entity1,
                (
                    Position { x: 1.0, y: 2.0 },
                    Velocity { dx: 0.5, dy: 0.3 },
                    Health { value: 100 },
                ),
            ),
            (
                entity2,
                (
                    Position { x: 3.0, y: 4.0 },
                    Velocity { dx: -0.2, dy: 0.8 },
                    Health { value: 75 },
                ),
            ),
        ]);

        // Then

        let pos_column = table.get_column::<Position>().unwrap();
        let vel_column = table.get_column::<Velocity>().unwrap();
        let health_column = table.get_column::<Health>().unwrap();
        unsafe {
            let posistions: Vec<Position> = pos_column.iter::<Position>().copied().collect();
            assert_eq!(
                posistions,
                vec![Position { x: 1.0, y: 2.0 }, Position { x: 3.0, y: 4.0 }]
            );

            let velocities: Vec<Velocity> = vel_column.iter::<Velocity>().copied().collect();
            assert_eq!(
                velocities,
                vec![
                    Velocity { dx: 0.5, dy: 0.3 },
                    Velocity { dx: -0.2, dy: 0.8 }
                ]
            );

            let healths: Vec<Health> = health_column.iter::<Health>().copied().collect();
            assert_eq!(healths, vec![Health { value: 100 }, Health { value: 75 },]);
        }

        assert_eq!(table.len(), 2);

        #[cfg(debug_assertions)]
        table.verify_invariants();
    }

    #[test]
    fn table_column_iteration() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Score>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Score>().unwrap()]);

        let mut allocator = entity::Allocator::new();

        // Add multiple entities with the builder
        for i in 0..5 {
            table.add_entities([(allocator.alloc(), Score { points: i * 10 })]);
        }

        // When / Then

        // Iterate over the score column
        let column = table.get_column::<Score>().unwrap();
        unsafe {
            let scores: Vec<u32> = column.iter::<Score>().map(|s| s.points).collect();
            assert_eq!(scores, vec![0, 10, 20, 30, 40]);
        }

        // Mutate all scores
        let column = table.get_column_mut::<Score>().unwrap();
        unsafe {
            for score in column.iter_mut::<Score>() {
                score.points += 5;
            }
        }

        // Verify mutation
        let column = table.get_column::<Score>().unwrap();
        unsafe {
            let scores: Vec<u32> = column.iter::<Score>().map(|s| s.points).collect();
            assert_eq!(scores, vec![5, 15, 25, 35, 45]);
        }
    }

    #[test]
    fn table_swap_remove_row() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();
        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let entity3 = allocator.alloc();

        table.add_entities([
            (entity1, Health { value: 100 }),
            (entity2, Health { value: 200 }),
            (entity3, Health { value: 300 }),
        ]);

        assert_eq!(table.len(), 3);

        // When - remove middle entity
        let moved = table.swap_remove_row(Row::new(1));

        // Then - entity2 is removed, entity3 is moved to its position
        assert_eq!(moved, Some(entity3));
        assert_eq!(table.len(), 2);
        assert_eq!(table.entities()[0], entity1);
        assert_eq!(table.entities()[1], entity3); // Swapped from end

        // Verify column data
        let column = table.get_column::<Health>().unwrap();
        unsafe {
            let values: Vec<i32> = column.iter::<Health>().map(|v| v.value).collect();
            assert_eq!(values, vec![100, 300]);
        }

        #[cfg(debug_assertions)]
        table.verify_invariants();

        // When - remove last entity
        let moved = table.swap_remove_row(Row::new(1));

        // Then
        assert_eq!(moved, None);
        assert_eq!(table.len(), 1);
        assert_eq!(table.entities()[0], entity1);

        #[cfg(debug_assertions)]
        table.verify_invariants();
    }

    #[test]
    #[should_panic(expected = "row index out of bounds")]
    fn table_swap_remove_row_out_of_bounds() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        // When - try to remove from empty table
        let result = table.swap_remove_row(Row::new(0));

        // Then
        assert_eq!(result, None);

        // When - add entity and try to remove out of bounds
        let mut allocator = entity::Allocator::new();
        table.add_entities([(allocator.alloc(), Position { x: 0.0, y: 0.0 })]);

        table.swap_remove_row(Row::new(10));
    }

    #[test]
    fn table_get_component_by_entity() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();

        let mut table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
            ],
        );

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let rows = table
            .add_entities([
                (
                    entity1,
                    (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
                ),
                (
                    entity2,
                    (Position { x: 3.0, y: 4.0 }, Velocity { dx: -0.2, dy: 0.8 }),
                ),
            ])
            .into_iter()
            .map(|(row, _)| row)
            .collect::<Vec<_>>();

        // When/Then - get components for entity1
        unsafe {
            let pos = table.get::<Position>(rows[0]);
            assert_eq!(pos, Some(&Position { x: 1.0, y: 2.0 }));

            let vel = table.get::<Velocity>(rows[0]);
            assert_eq!(vel, Some(&Velocity { dx: 0.5, dy: 0.3 }));
        }

        // When/Then - get components for entity2
        unsafe {
            let pos = table.get::<Position>(rows[1]);
            assert_eq!(pos, Some(&Position { x: 3.0, y: 4.0 }));

            let vel = table.get::<Velocity>(rows[1]);
            assert_eq!(vel, Some(&Velocity { dx: -0.2, dy: 0.8 }));
        }

        // When/Then - get non-existent entity
        unsafe {
            let pos = table.get::<Position>(Row::new(3));
            assert_eq!(pos, None);
        }
    }

    #[test]
    fn table_get_mut_component_by_row() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table
            .add_entities([(entity, Health { value: 100 })])
            .first()
            .unwrap()
            .0;

        // When - mutate the health
        unsafe {
            let health = table.get_mut::<Health>(row);
            assert!(health.is_some());
            health.unwrap().value = 75;
        }

        // Then - verify mutation
        unsafe {
            let health = table.get::<Health>(row);
            assert_eq!(health, Some(&Health { value: 75 }));
        }
    }

    #[test]
    fn table_drop_components() {
        // Given
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, Component)]
        struct DropTracker(Arc<AtomicUsize>);

        impl Drop for DropTracker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let registry = world::TypeRegistry::new();
        registry.register_component::<DropTracker>();

        let mut table = Table::new(
            Id::new(0),
            &[registry.get_info_of::<DropTracker>().unwrap()],
        );

        let counter = Arc::new(AtomicUsize::new(0));

        let mut allocator = entity::Allocator::new();

        // Add 3 entities
        table.add_entities([
            (allocator.alloc(), DropTracker(counter.clone())),
            (allocator.alloc(), DropTracker(counter.clone())),
            (allocator.alloc(), DropTracker(counter.clone())),
        ]);

        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // When - swap remove one
        table.swap_remove_row(Row::new(1));

        // Then - one drop
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(table.len(), 2);

        // When - drop table
        drop(table);

        // Then - all remaining dropped
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn table_empty_operations() {
        // Given

        #[derive(Component)]
        struct Empty;

        let registry = world::TypeRegistry::new();
        registry.register_component::<Empty>();

        let table = Table::new(Id::new(0), &[registry.get_info_of::<Empty>().unwrap()]);

        // Then
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.entities().len(), 0);

        #[cfg(debug_assertions)]
        table.verify_invariants();
    }

    #[test]
    fn table_get_column_none_for_invalid_id() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        // When - try to get column for component not in table
        #[derive(Component)]
        struct Comp2 {}
        registry.register_component::<Comp2>();

        // Then
        assert!(table.get_column::<Comp2>().is_none());
        assert!(table.get_column_mut::<Comp2>().is_none());
    }

    #[test]
    fn table_get_returns_none_for_entity_not_in_table() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        table.add_entities([(entity, Health { value: 100 })]);

        // When/Then - entity not in table returns None
        unsafe {
            assert_eq!(table.get::<Health>(1.into()), None);
        }
    }

    #[test]
    fn table_view_single_component() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, Position { x: 1.0, y: 2.0 });

        // When
        let pos: Option<&Position> = unsafe { table.view(row) };

        // Then
        assert!(pos.is_some());
        let pos = pos.unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }

    #[test]
    fn table_view_mut_single_component() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, Health { value: 100 });

        // When
        let health: Option<&mut Health> = unsafe { table.view_mut(row) };

        // Then
        assert!(health.is_some());
        let health = health.unwrap();
        health.value = 75;

        // Verify mutation
        let health: Option<&Health> = unsafe { table.view(row) };
        assert_eq!(health.unwrap().value, 75);
    }

    #[test]
    fn table_view_multiple_components() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();

        let mut table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
            ],
        );

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
        );

        // When
        let view: Option<(&Position, &Velocity)> = unsafe { table.view(row) };

        // Then
        assert!(view.is_some());
        let (pos, vel) = view.unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert_eq!(vel.dx, 0.5);
        assert_eq!(vel.dy, 0.3);
    }

    #[test]
    fn table_view_mut_mixed_mutability() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();

        let mut table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
            ],
        );

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
        );

        // When - mixed mutability view
        let view: Option<(&Position, &mut Velocity)> = unsafe { table.view_mut(row) };

        // Then
        assert!(view.is_some());
        let (pos, vel) = view.unwrap();

        // Read immutable
        assert_eq!(pos.x, 1.0);

        // Modify mutable
        vel.dx = 1.5;

        // Verify mutation
        let view: Option<(&Velocity,)> = unsafe { table.view(row) };
        assert_eq!(view.unwrap().0.dx, 1.5);
    }

    #[test]
    fn table_view_returns_none_for_invalid_row() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        let table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        // When
        let view: Option<&Position> = unsafe { table.view(Row::new(999)) };

        // Then
        assert!(view.is_none());
    }

    #[test]
    fn table_iter_views_immutable() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();

        // Add entities
        for i in 0..5 {
            table.add_entity(allocator.alloc(), Health { value: i * 10 });
        }

        // When
        let values: Vec<i32> = unsafe {
            table
                .iter_views::<&Health>()
                .map(|(_e, v)| v.value)
                .collect()
        };

        // Then
        assert_eq!(values, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn table_iter_views_mut_modify() {
        // Given
        #[derive(Component, Debug, PartialEq)]
        struct Counter {
            value: i32,
        }
        let registry = world::TypeRegistry::new();
        registry.register_component::<Counter>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Counter>().unwrap()]);

        let mut allocator = entity::Allocator::new();

        for i in 0..3 {
            table.add_entity(allocator.alloc(), Counter { value: i });
        }

        // When - increment all counters
        for (_entity, counter) in unsafe { table.iter_views_mut::<&mut Counter>() } {
            counter.value += 100;
        }

        // Then - verify all incremented
        let values: Vec<i32> = unsafe {
            table
                .iter_views::<&Counter>()
                .map(|(_e, c)| c.value)
                .collect()
        };
        assert_eq!(values, vec![100, 101, 102]);
    }

    #[test]
    fn table_iter_views_multiple_components() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();
        registry.register_component::<Velocity>();

        let mut table = Table::new(
            Id::new(0),
            &[
                registry.get_info_of::<Position>().unwrap(),
                registry.get_info_of::<Velocity>().unwrap(),
            ],
        );

        let mut allocator = entity::Allocator::new();

        for i in 0..3 {
            table.add_entity(
                allocator.alloc(),
                (
                    Position {
                        x: i as f32,
                        y: i as f32 * 2.0,
                    },
                    Velocity {
                        dx: i as f32 * 0.1,
                        dy: i as f32 * 0.2,
                    },
                ),
            );
        }

        // When
        let mut count = 0;
        for (_, (pos, vel)) in unsafe { table.iter_views::<(&Position, &Velocity)>() } {
            // Then
            assert_eq!(pos.x * 2.0, pos.y);
            assert_eq!(vel.dx * 2.0, vel.dy);
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn table_iter_views_empty_table() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        let table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        // When
        let count = unsafe { table.iter_views::<&Position>() }.count();

        // Then
        assert_eq!(count, 0);
    }

    #[test]
    fn table_iter_views_exact_size_iterator() {
        // Given
        let registry = world::TypeRegistry::new();
        registry.register_component::<Health>();

        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Health>().unwrap()]);

        let mut allocator = entity::Allocator::new();
        for i in 0..10 {
            table.add_entity(allocator.alloc(), Health { value: i });
        }

        // When
        let iter = unsafe { table.iter_views::<&Health>() };

        // Then
        assert_eq!(iter.len(), 10);
        assert_eq!(iter.size_hint(), (10, Some(10)));
    }

    #[test]
    #[should_panic(expected = "component not in table")]
    fn table_apply_type_check_panics_in_release() {
        // This test verifies that Table::apply (via Column::push) validates types
        // in BOTH debug and release builds

        let registry = world::TypeRegistry::new();
        registry.register_component::<Position>();

        // Create a table with Type
        let mut table = Table::new(Id::new(0), &[registry.get_info_of::<Position>().unwrap()]);

        // When/Then - should panic when applying wrong type to component ID
        // We're using TypeA's ID but passing a TypeB value
        table.apply_value(0.into(), Health { value: 99 });
    }
}
