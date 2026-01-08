use std::any::TypeId;

use crate::core::ecs::{
    component::{self, Component, Set, SetTarget},
    entity::{self},
    storage::{column::Column, row::Row, view, view::View},
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
/// use rusty_engine::core::ecs::storage::Table;
/// use rusty_engine::core::ecs::component;
/// use rusty_engine::core::ecs::entity;
/// use rusty_macros::Component;
///
/// // Setup Components
/// let component_registry = component::Registry::new();
/// #[derive(Component)]
/// struct Comp1 {}
///
/// #[derive(Component)]
/// struct Comp2 {}
///
/// let pos_id = component_registry.register::<Comp1>();
/// let vel_id = component_registry.register::<Comp2>();
///  
/// // Construct the table from the component spec
/// let spec = component::Spec::new(vec![pos_id, vel_id]);
/// let mut table = Table::new(spec, &component_registry);
///
/// // Create an entity
/// let mut allocator = entity::Allocator::new();
/// let entity = allocator.alloc();
///
/// // Add an entity to the table
/// table.add_entity(
///   entity,
///   (Comp1 {}, Comp2 {}),
///   &component_registry
/// );
///
/// ```
///
/// # Invariants
/// - `entities.len()` must equal the length of every column
pub struct Table {
    /// The unique identifier for this table.
    id: Id,

    /// The entities stored in this table (one per row).
    entities: Vec<entity::Entity>,

    /// The component specification defining which components are in this table.
    components: component::Spec,

    /// The component columns. Each column stores all instances of one component type.
    columns: Vec<Column>,
    // TODO: Evaluate if a map or sparse set is worth it for faster lookups. Using array search for
    // now since number of components per table is expected to be small. Need benchmarks. Perhaps
    // the bahvior can be configurable based on column count.
}

impl Table {
    /// Create a new table for the given component specification and component registry.
    /// Each component in the spec gets its own column.
    ///
    /// # Panics
    /// - Panics if any component in the spec is not registered in the provided registry.
    pub fn new(id: Id, components: component::Spec, registry: &component::Registry) -> Self {
        Self {
            id,
            entities: Vec::new(),
            // For each component in this table's component spec, create a column and map its index.
            columns: components
                .ids()
                .iter()
                .map(|id| {
                    Column::new(
                        registry
                            .get_info_by_id(*id)
                            .expect("component not registered"),
                    )
                })
                .collect(),
            components,
        }
    }

    /// Get the unique identifier for this table.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Add an entity with the given set of components to the table.
    ///
    /// The set must contain exactly the components specified in the table's
    /// component specification. All components are added atomically.
    ///
    /// # Panics
    /// - Panics in debug builds if the set's component specification does not match the table's specification.
    ///
    /// # Example
    /// ```ignore
    /// table.add_entity(entity, (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }), &mut registry);
    /// ```
    pub fn add_entity<S: Set>(
        &mut self,
        entity: entity::Entity,
        set: S,
        registry: &component::Registry,
    ) -> Row {
        // Verify set matches table spec
        #[cfg(debug_assertions)]
        {
            let set_spec = S::spec(registry);
            debug_assert_eq!(
                set_spec, self.components,
                "set spec does not match table spec"
            );
        }

        // Capture the entity row.
        let row = Row::new(self.entities.len());

        // Apply the set to this table.
        set.apply(registry, self);

        // Add the entity once the components are all added.
        self.entities.push(entity);

        // Verify we have kept the entity/column lengths consistent.
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

    /// Get the component specification for this table.
    #[inline]
    pub fn components(&self) -> &component::Spec {
        &self.components
    }

    /// Get the entities stored in this table.
    #[inline]
    pub fn entities(&self) -> &[entity::Entity] {
        &self.entities
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
    pub fn get_column<C: Component>(&self) -> Option<&Column> {
        self.columns
            .iter()
            .find(|&col| col.info().type_id() == TypeId::of::<C>())
    }

    /// Get a mutable reference to a column by component type `C`.
    ///
    /// Returns `None` if:
    /// - the component column is not in this table
    #[inline]
    pub fn get_column_mut<C: Component>(&mut self) -> Option<&mut Column> {
        self.columns
            .iter_mut()
            .find(|col| col.info().type_id() == TypeId::of::<C>())
    }

    /// Get a reference to a column by component ID.
    ///
    /// Returns `None` if:
    /// - the row is not in this table
    #[inline]
    pub fn get_column_by_id(&self, component_id: component::Id) -> Option<&Column> {
        self.columns
            .iter()
            .find(|col| col.info().id() == component_id)
    }

    /// Get a mutable reference to a column by component ID.
    ///
    /// Returns `None` if:
    /// - the row is not in this table
    #[inline]
    pub fn get_column_by_id_mut(&mut self, component_id: component::Id) -> Option<&mut Column> {
        self.columns
            .iter_mut()
            .find(|col| col.info().id() == component_id)
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
    pub unsafe fn get<C: Component>(&self, row: Row) -> Option<&C> {
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
    pub unsafe fn get_mut<C: Component>(&mut self, row: Row) -> Option<&mut C> {
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

        Some(moved_entity)
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
}

impl SetTarget for Table {
    /// Apply a component value to the appropriate column in the table.
    ///
    /// # Panics
    /// - If the component ID is not valid for this table.
    /// - If the type `C` doesn't match the component ID's registered type.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - There is a valid column for the given component ID.
    /// - The type `C` matches the column's component type (validated at runtime).
    fn apply<C: 'static + Component>(&mut self, id: component::Id, value: C) {
        let column = self
            .get_column_by_id_mut(id)
            .expect("component_id not in table spec");
        // SAFETY: Type validation happens inside column.push()
        // If type C doesn't match the column's type, push() will panic
        unsafe {
            column.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use super::*;

    #[test]
    fn table_creation_with_default_index() {
        // Given
        let component_registry = component::Registry::new();
        #[derive(Component)]
        struct Comp1 {}

        #[derive(Component)]
        struct Comp2 {}

        let pos_id = component_registry.register::<Comp1>();
        let vel_id = component_registry.register::<Comp2>();

        let spec = component::Spec::new(vec![pos_id, vel_id]);

        // When
        let table = Table::new(Id::new(0), spec, &component_registry);

        // Then
        assert_eq!(table.len(), 0);
        assert_eq!(table.columns.len(), 2);
        // Columns are None until first component is added
        assert!(table.get_column_by_id(pos_id).is_some());
        assert!(table.get_column_by_id(vel_id).is_some());
    }

    #[test]
    fn table_entity_management() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Copy, Clone, Debug, PartialEq)]
        struct Health {
            value: i32,
        }

        let health_id = component_registry.register::<Health>();
        let spec = component::Spec::new(vec![health_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        // When
        let row1 = table.add_entity(entity1, Health { value: 100 }, &component_registry);
        let row2 = table.add_entity(entity2, Health { value: 75 }, &component_registry);

        // Then
        assert_eq!(row1.index(), 0);
        assert_eq!(row2.index(), 1);
        assert_eq!(table.len(), 2);
        assert_eq!(table.entities()[0], entity1);
        assert_eq!(table.entities()[1], entity2);

        let column = table.get_column_by_id(health_id).unwrap();
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
        let component_registry = component::Registry::new();

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

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();
        let health_id = component_registry.register::<Health>();

        // Create a table with three different component types
        let spec = component::Spec::new(vec![pos_id, vel_id, health_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        // When

        // Add first entity with all components atomically
        table.add_entity(
            entity1,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &component_registry,
        );

        // Add second entity with all components atomically
        table.add_entity(
            entity2,
            (
                Position { x: 3.0, y: 4.0 },
                Velocity { dx: -0.2, dy: 0.8 },
                Health { value: 75 },
            ),
            &component_registry,
        );

        // Then

        let pos_column = table.get_column_by_id(pos_id).unwrap();
        let vel_column = table.get_column_by_id(vel_id).unwrap();
        let health_column = table.get_column_by_id(health_id).unwrap();
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
        let component_registry = component::Registry::new();

        #[derive(Debug)]
        struct Score {
            points: u32,
        }
        impl component::Component for Score {}

        let score_id = component_registry.register::<Score>();
        let spec = component::Spec::new(vec![score_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();

        // Add multiple entities with the builder
        for i in 0..5 {
            table.add_entity(
                allocator.alloc(),
                Score { points: i * 10 },
                &component_registry,
            );
        }

        // When / Then

        // Iterate over the score column
        let column = table.get_column_by_id(score_id).unwrap();
        unsafe {
            let scores: Vec<u32> = column.iter::<Score>().map(|s| s.points).collect();
            assert_eq!(scores, vec![0, 10, 20, 30, 40]);
        }

        // Mutate all scores
        let column = table.get_column_by_id_mut(score_id).unwrap();
        unsafe {
            for score in column.iter_mut::<Score>() {
                score.points += 5;
            }
        }

        // Verify mutation
        let column = table.get_column_by_id(score_id).unwrap();
        unsafe {
            let scores: Vec<u32> = column.iter::<Score>().map(|s| s.points).collect();
            assert_eq!(scores, vec![5, 15, 25, 35, 45]);
        }
    }

    #[test]
    #[should_panic(expected = "set spec does not match table spec")]
    fn test_table_incomplete_row_panics() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Position {}

        #[derive(Component)]
        struct Velocity {}

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();

        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        // When Then - should panic because we don't add velocity
        table.add_entity(entity, Position {}, &component_registry);
    }

    #[test]
    fn table_swap_remove_row() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Copy, Clone, Debug, PartialEq)]
        struct Value {
            n: u32,
        }

        let value_id = component_registry.register::<Value>();
        let spec = component::Spec::new(vec![value_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let entity3 = allocator.alloc();

        table.add_entity(entity1, Value { n: 100 }, &component_registry);
        table.add_entity(entity2, Value { n: 200 }, &component_registry);
        table.add_entity(entity3, Value { n: 300 }, &component_registry);

        assert_eq!(table.len(), 3);

        // When - remove middle entity
        let moved = table.swap_remove_row(Row::new(1));

        // Then - entity2 is removed, entity3 is moved to its position
        assert_eq!(moved, Some(entity3));
        assert_eq!(table.len(), 2);
        assert_eq!(table.entities()[0], entity1);
        assert_eq!(table.entities()[1], entity3); // Swapped from end

        // Verify column data
        let column = table.get_column_by_id(value_id).unwrap();
        unsafe {
            let values: Vec<u32> = column.iter::<Value>().map(|v| v.n).collect();
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
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp {}

        let comp_id = component_registry.register::<Comp>();
        let spec = component::Spec::new(vec![comp_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        // When - try to remove from empty table
        let result = table.swap_remove_row(Row::new(0));

        // Then
        assert_eq!(result, None);

        // When - add entity and try to remove out of bounds
        let mut allocator = entity::Allocator::new();
        table.add_entity(allocator.alloc(), Comp {}, &component_registry);

        table.swap_remove_row(Row::new(10));
    }

    #[test]
    fn table_get_component_by_entity() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        #[derive(Component, Debug, PartialEq)]
        struct Velocity {
            dx: f32,
            dy: f32,
        }

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();
        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();

        let row1 = table.add_entity(
            entity1,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &component_registry,
        );
        let row2 = table.add_entity(
            entity2,
            (Position { x: 3.0, y: 4.0 }, Velocity { dx: -0.2, dy: 0.8 }),
            &component_registry,
        );

        // When/Then - get components for entity1
        unsafe {
            let pos = table.get::<Position>(row1);
            assert_eq!(pos, Some(&Position { x: 1.0, y: 2.0 }));

            let vel = table.get::<Velocity>(row1);
            assert_eq!(vel, Some(&Velocity { dx: 0.5, dy: 0.3 }));
        }

        // When/Then - get components for entity2
        unsafe {
            let pos = table.get::<Position>(row2);
            assert_eq!(pos, Some(&Position { x: 3.0, y: 4.0 }));

            let vel = table.get::<Velocity>(row2);
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
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Health {
            value: i32,
        }

        let health_id = component_registry.register::<Health>();
        let spec = component::Spec::new(vec![health_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, Health { value: 100 }, &component_registry);

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

        #[derive(Debug)]
        struct DropTracker(Arc<AtomicUsize>);

        impl Drop for DropTracker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        impl component::Component for DropTracker {}

        let component_registry = component::Registry::new();
        let tracker_id = component_registry.register::<DropTracker>();
        let spec = component::Spec::new(vec![tracker_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let counter = Arc::new(AtomicUsize::new(0));

        let mut allocator = entity::Allocator::new();

        // Add 3 entities
        table.add_entity(
            allocator.alloc(),
            DropTracker(counter.clone()),
            &component_registry,
        );
        table.add_entity(
            allocator.alloc(),
            DropTracker(counter.clone()),
            &component_registry,
        );
        table.add_entity(
            allocator.alloc(),
            DropTracker(counter.clone()),
            &component_registry,
        );

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
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Empty;

        let empty_id = component_registry.register::<Empty>();
        let spec = component::Spec::new(vec![empty_id]);
        let table = Table::new(Id::new(0), spec, &component_registry);

        // Then
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.entities().len(), 0);

        #[cfg(debug_assertions)]
        table.verify_invariants();
    }

    #[test]
    fn table_components_accessor() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp1 {}

        #[derive(Component)]
        struct Comp2 {}

        let comp1_id = component_registry.register::<Comp1>();
        let comp2_id = component_registry.register::<Comp2>();
        let spec = component::Spec::new(vec![comp1_id, comp2_id]);
        let table = Table::new(Id::new(0), spec.clone(), &component_registry);

        // When/Then
        assert_eq!(table.components(), &spec);
        assert_eq!(table.components().ids(), &[comp1_id, comp2_id]);
    }

    #[test]
    fn table_get_column_none_for_invalid_id() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp1 {}

        let comp1_id = component_registry.register::<Comp1>();
        let spec = component::Spec::new(vec![comp1_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        // When - try to get column for component not in table
        #[derive(Component)]
        struct Comp2 {}
        let comp2_id = component_registry.register::<Comp2>();

        // Then
        assert!(table.get_column_by_id(comp2_id).is_none());
        assert!(table.get_column_by_id_mut(comp2_id).is_none());
    }

    #[test]
    fn table_get_returns_none_for_entity_not_in_table() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Value {
            n: u32,
        }

        let value_id = component_registry.register::<Value>();
        let spec = component::Spec::new(vec![value_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        table.add_entity(entity, Value { n: 100 }, &component_registry);

        // When/Then - entity not in table returns None
        unsafe {
            assert_eq!(table.get::<Value>(1.into()), None);
        }
    }

    #[test]
    fn table_view_single_component() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        let pos_id = component_registry.register::<Position>();
        let spec = component::Spec::new(vec![pos_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, Position { x: 1.0, y: 2.0 }, &component_registry);

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
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Health {
            value: i32,
        }

        let health_id = component_registry.register::<Health>();
        let spec = component::Spec::new(vec![health_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, Health { value: 100 }, &component_registry);

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
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        #[derive(Component, Debug, PartialEq)]
        struct Velocity {
            dx: f32,
            dy: f32,
        }

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();
        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &component_registry,
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
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        #[derive(Component, Debug, PartialEq)]
        struct Velocity {
            dx: f32,
            dy: f32,
        }

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();
        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (Position { x: 1.0, y: 2.0 }, Velocity { dx: 0.5, dy: 0.3 }),
            &component_registry,
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
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp;

        let comp_id = component_registry.register::<Comp>();
        let spec = component::Spec::new(vec![comp_id]);
        let table = Table::new(Id::new(0), spec, &component_registry);

        // When
        let view: Option<&Comp> = unsafe { table.view(Row::new(999)) };

        // Then
        assert!(view.is_none());
    }

    #[test]
    fn table_iter_views_immutable() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq, Clone, Copy)]
        struct Value {
            n: u32,
        }

        let value_id = component_registry.register::<Value>();
        let spec = component::Spec::new(vec![value_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();

        // Add entities
        for i in 0..5 {
            table.add_entity(allocator.alloc(), Value { n: i * 10 }, &component_registry);
        }

        // When
        let values: Vec<u32> = unsafe { table.iter_views::<&Value>().map(|(_e, v)| v.n).collect() };

        // Then
        assert_eq!(values, vec![0, 10, 20, 30, 40]);
    }

    #[test]
    fn table_iter_views_mut_modify() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Counter {
            value: i32,
        }

        let counter_id = component_registry.register::<Counter>();
        let spec = component::Spec::new(vec![counter_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();

        for i in 0..3 {
            table.add_entity(allocator.alloc(), Counter { value: i }, &component_registry);
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
        let component_registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        #[derive(Component, Debug, PartialEq)]
        struct Velocity {
            dx: f32,
            dy: f32,
        }

        let pos_id = component_registry.register::<Position>();
        let vel_id = component_registry.register::<Velocity>();
        let spec = component::Spec::new(vec![pos_id, vel_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

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
                &component_registry,
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
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp;

        let comp_id = component_registry.register::<Comp>();
        let spec = component::Spec::new(vec![comp_id]);
        let table = Table::new(Id::new(0), spec, &component_registry);

        // When
        let count = unsafe { table.iter_views::<&Comp>() }.count();

        // Then
        assert_eq!(count, 0);
    }

    #[test]
    fn table_iter_views_exact_size_iterator() {
        // Given
        let component_registry = component::Registry::new();

        #[derive(Component)]
        struct Comp {
            #[allow(dead_code)]
            x: i32,
        }

        let comp_id = component_registry.register::<Comp>();
        let spec = component::Spec::new(vec![comp_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        let mut allocator = entity::Allocator::new();
        for i in 0..10 {
            table.add_entity(allocator.alloc(), Comp { x: i }, &component_registry);
        }

        // When
        let iter = unsafe { table.iter_views::<&Comp>() };

        // Then
        assert_eq!(iter.len(), 10);
        assert_eq!(iter.size_hint(), (10, Some(10)));
    }

    #[test]
    #[should_panic(expected = "Type mismatch")]
    fn table_apply_type_check_panics_in_release() {
        // This test verifies that Table::apply (via Column::push) validates types
        // in BOTH debug and release builds

        // Given
        #[derive(Component)]
        struct TypeA {
            #[allow(dead_code)]
            value: u32,
        }

        #[derive(Component)]
        struct TypeB {
            #[allow(dead_code)]
            value: u32,
        }

        let component_registry = component::Registry::new();
        let type_a_id = component_registry.register::<TypeA>();
        let _type_b_id = component_registry.register::<TypeB>();

        // Create a table with TypeA
        let spec = component::Spec::new(vec![type_a_id]);
        let mut table = Table::new(Id::new(0), spec, &component_registry);

        // When/Then - should panic when applying wrong type to component ID
        // We're using TypeA's ID but passing a TypeB value
        table.apply(type_a_id, TypeB { value: 99 });
    }
}
