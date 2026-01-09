//! View trait and implementations for accessing components of a single entity from a table.
//!
//! This module provides the [`View`] trait, which enables type-safe, ergonomic access to
//! component data for a single entity row within a table. Views are the read-side complement
//! to the [`Set`](crate::ecs::component::Set) trait used for writing components.
//!
//! # Overview
//!
//! A [`View`] represents a query for one or more component types for a single entity.
//! It provides compile-time type safety and zero-cost abstraction over direct column access.
//!
//! # Usage Examples
//!
//! ## Single Component View
//!
//! ```rust,ignore
//! use rusty_engine::ecs::storage::{Table, view::View};
//! use rusty_macros::Component;
//!
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! // Read-only access
//! let pos: Option<&Position> = unsafe {
//!     table.view(row, &registry)
//! };
//!
//! // Mutable access
//! let pos: Option<&mut Position> = unsafe {
//!     table.view_mut(row, &registry)
//! };
//! ```
//!
//! ## Multiple Component View (Tuples)
//!
//! ```rust,ignore
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Component)]
//! struct Velocity { dx: f32, dy: f32 }
//!
//! // Read multiple components
//! let view: Option<(&Position, &Velocity)> = unsafe {
//!     table.view(row, &registry)
//! };
//!
//! if let Some((pos, vel)) = view {
//!     println!("Entity at ({}, {}) moving ({}, {})", pos.x, pos.y, vel.dx, vel.dy);
//! }
//!
//! // Mixed mutability
//! let view: Option<(&Position, &mut Velocity)> = unsafe {
//!     table.view_mut(row, &registry)
//! };
//!
//! if let Some((pos, vel)) = view {
//!     vel.dx += pos.x * 0.1; // Modify velocity based on position
//! }
//! ```
//!
//! # Design
//!
//! The [`View`] trait mirrors the [`Set`](crate::ecs::component::Set) trait's design:
//! - **Single components**: `&C` and `&mut C` implement [`View`]
//! - **Tuples**: `(&A, &B, &mut C, ...)` implement [`View`] via macros
//! - **Specification**: [`View::spec()`] returns the required components
//! - **Fetching**: [`View::fetch()`] retrieves component references from a table row
//!
//! # Safety
//!
//! The [`View::fetch()`] method is `unsafe` because:
//! - Type `C` must match the actual type stored for the given component ID
//!
//! The following are **not** safety requirements, as they are checked at runtime:
//! - Row validity (checked, returns `None` if invalid)
//! - Component existence in table (checked, returns `None` if missing)
//! - Component registration (checked, returns `None` if unregistered)
//!
//! For tuple views with mutable components, the caller must also ensure the same
//! component is not requested multiple times to avoid aliasing violations.
//!
//! Debug builds validate type correctness via assertions in the underlying
//! [`Table::get()`](super::Table::get) and [`Table::get_mut()`](super::Table::get_mut) methods.
//!
//! # Performance
//!
//! Views are **zero-cost abstractions**:
//! - No allocations
//! - Compiles to direct column pointer access
//! - Same performance as manual `table.get::<C>(row, id)`
//!
//! # Comparison with Direct Access
//!
//! ```rust,ignore
//! // Manual access (verbose)
//! let pos_id = registry.get::<Position>()?;
//! let vel_id = registry.get::<Velocity>()?;
//! let pos = unsafe { table.get::<Position>(row, pos_id)? };
//! let vel = unsafe { table.get::<Velocity>(row, vel_id)? };
//!
//! // With View (ergonomic)
//! let (pos, vel): (&Position, &Velocity) = unsafe {
//!     table.view(row, &registry)?
//! };
//! ```
//!
//! # Further Considerations
//! - Should we support more complex views (e.g., optional components)?
//! - Should we require component registration upfront for better safety instead of registering?

use std::{any::TypeId, collections::HashSet};

use crate::ecs::{
    component::Component,
    entity,
    storage::{row::Row, table::Table},
};

/// Trait for types that can be fetched as a view of components for a single entity row.
///
/// This trait enables type-safe retrieval of component references from a table row.
/// It is implemented for:
/// - Single component references: `&C`, `&mut C`
/// - Tuples of component references: `(&A, &B)`, `(&A, &mut B)`, etc.
///
/// # Type Parameters
///
/// * `'a` - The lifetime of the component references. Tied to the table's borrow.
///
/// # Examples
///
/// ```rust,ignore
/// use rusty_engine::ecs::storage::view::View;
///
/// // Fetch a single component
/// let health: Option<&Health> = unsafe {
///     <&Health>::fetch(table, row, &registry)
/// };
///
/// // Fetch multiple components
/// let view: Option<(&Position, &mut Velocity)> = unsafe {
///     <(&Position, &mut Velocity)>::fetch(table, row, &registry)
/// };
/// ```
pub trait View<'a>: Sized {
    /// Fetch component references for a single entity row from a table.
    ///
    /// Returns `None` if:
    /// - Any required component is missing from the table
    /// - The row index is invalid (>= table.len())
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types `C` match the actual types stored for their component IDs
    ///
    /// Type safety is ensured through the component registry's type-to-ID mapping.
    /// Row validity and component existence are checked at runtime.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if component type doesn't match the registered type.
    ///
    /// # Parameters
    ///
    /// * `table` - The table to fetch components from
    /// * `row` - The row index within the table
    unsafe fn fetch(table: &'a Table, row: Row) -> Option<Self>;

    /// Fetch component references for a single entity row from a mutable table.
    ///
    /// This is the mutable variant of [`fetch`](View::fetch), allowing mutable access
    /// to components when needed.
    ///
    /// Returns `None` if:
    /// - Any required component is missing from the table
    /// - The row index is invalid (>= table.len())
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types `C` match the actual types stored for their component IDs
    /// - When fetching tuples with mutable components, the same component is not requested multiple times
    ///
    /// Type safety is ensured through the table's type-to-ID mapping.
    /// Row validity and component existence are checked at runtime.
    unsafe fn fetch_mut(table: &'a mut Table, row: Row) -> Option<Self>;

    /// Returns the TypeIds of all components accessed mutably by this view.
    ///
    /// This is used to detect aliasing violations at runtime when creating
    /// mutable view iterators. If the same component is requested multiple times
    /// with mutable access, it would create aliased mutable references (UB).
    ///
    /// # Returns
    ///
    /// A vector containing the TypeId of each mutably-accessed component.
    /// For immutable views, this returns an empty vector.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Immutable view - no mutable components
    /// assert_eq!(<&Position>::mutable_component_ids(), vec![]);
    ///
    /// // Mutable view - one mutable component
    /// assert_eq!(<&mut Position>::mutable_component_ids(), vec![TypeId::of::<Position>()]);
    ///
    /// // Mixed tuple - only mutable components listed
    /// let ids = <(&Position, &mut Velocity)>::mutable_component_ids();
    /// assert_eq!(ids, vec![TypeId::of::<Velocity>()]);
    /// ```
    fn mutable_component_ids() -> Vec<TypeId> {
        vec![]
    }
}

/// Iterator over views of entities in a table.
///
/// This iterator yields views for each entity in the table, providing ergonomic
/// iteration over component data. Each result will have both the entity and its view.
///
/// # Invariants
///
/// Since tables enforce that all columns have the same length (archetype pattern),
/// this iterator will successfully yield a view for every row in the table, assuming
/// the view's component requirements match the table's specification.
///
/// # Type Parameters
///
/// * `'a` - The lifetime of the table borrow
/// * `V` - The view type (e.g., `&Position`, `(&Position, &Velocity)`)
///
/// # Examples
///
/// ```rust,ignore
/// // Iterate over single components
/// let iter: ViewIter<&Position> = unsafe { table.iter_views(&registry) };
/// for (entity, pos) in iter {
///     println!("Position: ({}, {}) for entity: {}", pos.x, pos.y, entity);
/// }
///
/// // Iterate over multiple components
/// let iter: ViewIter<(&Position, &Velocity)> = unsafe { table.iter_views(&registry) };
/// for (entity, (pos, vel)) in iter {
///     println!("Entity {} at ({}, {}) moving ({}, {})", entity.id(), pos.x, pos.y, vel.dx, vel.dy);
/// }
/// ```
pub struct ViewIter<'a, V: View<'a>> {
    table: &'a Table,
    current_row: usize,
    _marker: std::marker::PhantomData<V>,
}

impl<'a, V: View<'a>> ViewIter<'a, V> {
    /// Create a new ViewIter for the given table.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in V match the types registered in the registry
    /// - The table contains all components required by V (view spec ⊆ table spec)
    ///
    /// If the view requires components not in the table, all iterations will
    /// return None and the iterator will be empty.
    #[inline]
    pub unsafe fn new(table: &'a Table) -> Self {
        Self {
            table,
            current_row: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, V: View<'a>> Iterator for ViewIter<'a, V> {
    type Item = (entity::Entity, V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_row >= self.table.len() {
            return None;
        }

        let row = Row::new(self.current_row);
        self.current_row += 1;

        let entity = self.table.entity(row)?;

        // SAFETY: We're iterating within table.len() bounds, and the caller
        // ensures component types match via the unsafe constructor.
        // Due to the archetype pattern, if the first row succeeds, all rows will succeed.
        unsafe {
            let components = V::fetch(self.table, row)?;
            Some((entity, components))
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.table.len().saturating_sub(self.current_row);
        // Both bounds are exact because archetype pattern ensures all entities
        // have the same components. If the view matches, all rows yield Some.
        (remaining, Some(remaining))
    }
}

impl<'a, V: View<'a>> ExactSizeIterator for ViewIter<'a, V> {
    #[inline]
    fn len(&self) -> usize {
        self.table.len() - self.current_row
    }
}

/// Mutable iterator over views of entities in a table.
///
/// This iterator yields mutable views for each entity in the table, allowing
/// modification of component data during iteration.
///
/// # Invariants
///
/// Since tables enforce that all columns have the same length (archetype pattern),
/// this iterator will successfully yield a view for every row in the table, assuming
/// the view's component requirements match the table's specification.
///
/// # Type Parameters
///
/// * `'a` - The lifetime of the table borrow
/// * `V` - The view type (e.g., `&mut Position`, `(&Position, &mut Velocity)`)
///
/// # Examples
///
/// ```rust,ignore
/// // Iterate and modify single components
/// let iter: ViewIterMut<&mut Position> = unsafe { table.iter_views_mut(&registry) };
/// for pos in iter {
///     pos.x += 1.0;
///     pos.y += 1.0;
/// }
///
/// // Iterate over multiple components with mixed mutability
/// let iter: ViewIterMut<(&Position, &mut Velocity)> = unsafe {
///     table.iter_views_mut(&registry)
/// };
/// for (pos, vel) in iter {
///     vel.dx = pos.x * 0.1;  // Update velocity based on position
/// }
/// ```
pub struct ViewIterMut<'a, V: View<'a>> {
    table: &'a mut Table,
    current_row: usize,
    table_len: usize,
    _marker: std::marker::PhantomData<V>,
}

impl<'a, V: View<'a>> ViewIterMut<'a, V> {
    /// Create a new ViewIterMut for the given table.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - Component types in V match the types registered in the registry
    /// - The table contains all components required by V (view spec ⊆ table spec)
    ///
    /// # Panics
    ///
    /// Panics if the view requests the same mutable component multiple times,
    /// which would create aliased mutable references (undefined behavior).
    ///
    /// If the view requires components not in the table, all iterations will
    /// return None and the iterator will be empty.
    #[inline]
    pub unsafe fn new(table: &'a mut Table) -> Self {
        // Validate that we don't have duplicate mutable components (aliasing)
        let mut_ids = V::mutable_component_ids();
        let unique_ids: HashSet<_> = mut_ids.iter().collect();

        assert_eq!(
            mut_ids.len(),
            unique_ids.len(),
            "View aliasing violation: The view requests the same mutable component multiple times. \
             This would create aliased mutable references, which is undefined behavior. \
             Each mutable component can only appear once in a view."
        );

        let table_len = table.len();
        Self {
            table,
            current_row: 0,
            table_len,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a, V: View<'a>> Iterator for ViewIterMut<'a, V> {
    type Item = (entity::Entity, V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_row >= self.table_len {
            return None;
        }

        let row = Row::new(self.current_row);
        self.current_row += 1;

        let entity = self.table.entity(row)?;

        // SAFETY: We're iterating within table.len() bounds.
        // We use raw pointer casting to allow mutable iteration.
        // The caller ensures no aliasing violations via the unsafe constructor.
        // Due to the archetype pattern, if the first row succeeds, all rows will succeed.
        unsafe {
            let table_ptr = self.table as *mut Table;
            let components = V::fetch_mut(&mut *table_ptr, row)?;
            Some((entity, components))
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.table_len.saturating_sub(self.current_row);
        // Both bounds are exact because archetype pattern ensures all entities
        // have the same components. If the view matches, all rows yield Some.
        (remaining, Some(remaining))
    }
}

impl<'a, V: View<'a>> ExactSizeIterator for ViewIterMut<'a, V> {
    #[inline]
    fn len(&self) -> usize {
        self.table_len - self.current_row
    }
}

/// Implement View for single component references (&C).
///
/// Enables read-only access to a single component type.
impl<'a, C: Component> View<'a> for &'a C {
    unsafe fn fetch(table: &'a Table, row: Row) -> Option<Self> {
        // SAFETY: The caller ensures C matches an actual stored type in the table.
        unsafe { table.get::<C>(row) }
    }

    unsafe fn fetch_mut(table: &'a mut Table, row: Row) -> Option<Self> {
        // SAFETY: The caller ensures C matches the actual stored type in the table.
        unsafe { table.get::<C>(row) }
    }

    fn mutable_component_ids() -> Vec<TypeId> {
        // Immutable reference - no mutable access
        vec![]
    }
}

/// Implement View for single mutable component references (&mut C).
///
/// Enables mutable access to a single component type.
impl<'a, C: Component> View<'a> for &'a mut C {
    unsafe fn fetch(_table: &'a Table, _row: Row) -> Option<Self> {
        // Cannot fetch mutable reference from immutable table
        None
    }

    unsafe fn fetch_mut(table: &'a mut Table, row: Row) -> Option<Self> {
        // SAFETY: The caller ensures C matches an actual stored type in the table.
        unsafe { table.get_mut::<C>(row) }
    }

    fn mutable_component_ids() -> Vec<TypeId> {
        // Mutable reference - report this component
        vec![TypeId::of::<C>()]
    }
}

/// Implement View for empty tuple ().
///
/// Always succeeds, returns no components. Useful for testing or as a base case.
impl<'a> View<'a> for () {
    unsafe fn fetch(_table: &'a Table, _row: Row) -> Option<Self> {
        Some(())
    }

    unsafe fn fetch_mut(_table: &'a mut Table, _row: Row) -> Option<Self> {
        Some(())
    }

    fn mutable_component_ids() -> Vec<TypeId> {
        // Empty tuple - no components
        vec![]
    }
}

/// Macro to implement View for tuples of component references.
///
/// This generates implementations for tuples of various sizes, supporting
/// both immutable and mutable component references.
macro_rules! tuple_view_impl {
    ($($name: ident),*) => {
        impl<'a, $($name: View<'a>),*> View<'a> for ($($name,)*) {
            unsafe fn fetch(table: &'a Table, row: Row) -> Option<Self> {
                // SAFETY: Each component fetch is independent. The caller ensures component types
                // match their registered types. Row validity and component existence are checked
                // at runtime by each fetch call.
                Some((
                    $(unsafe { $name::fetch(table, row)? },)*
                ))
            }

            unsafe fn fetch_mut(table: &'a mut Table, row: Row) -> Option<Self> {
                // SAFETY: We create multiple mutable references from the same table using raw pointers,
                // but they point to different component columns, so no aliasing occurs.
                // The caller must ensure:
                // 1. Component types match their registered types
                // 2. The same component is not requested multiple times in the tuple
                //    (validated at runtime by ViewIterMut::new())
                #[allow(unused_unsafe)]
                unsafe {
                    Some((
                        $(unsafe {
                            $name::fetch_mut(
                                // SAFETY: Creating aliased mutable table pointers is safe because each
                                // fetch_mut call accesses different component columns
                                &mut *(table as *mut Table),
                                row,
                            )?
                        },)*
                    ))
                }
            }

            fn mutable_component_ids() -> Vec<TypeId> {
                // Collect mutable component IDs from all tuple elements
                let mut ids = Vec::new();
                $(
                    ids.extend($name::mutable_component_ids());
                )*
                ids
            }
        }
    }
}

/// Generate View implementations for tuples of increasing size.
macro_rules! tuple_view {
    ($head:ident) => {
        tuple_view_impl!($head);
    };
    ($head:ident, $($tail:ident),*) => {
        tuple_view_impl!($head, $($tail),*);
        tuple_view!($($tail),*);
    };
}

// Generate implementations for tuples up to 26 elements (A-Z)
tuple_view! {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use crate::ecs::{component, entity, storage::Table};

    use super::*;

    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Health {
        value: i32,
    }

    fn setup_table() -> (Table, component::Registry, entity::Allocator) {
        let registry = component::Registry::new();
        let pos_id = registry.register::<Position>();
        let vel_id = registry.register::<Velocity>();
        let health_id = registry.register::<Health>();

        let spec = component::Spec::new(vec![pos_id, vel_id, health_id]);
        let table = Table::new(super::super::table::Id::new(0), spec, &registry);
        let allocator = entity::Allocator::new();

        (table, registry, allocator)
    }

    #[test]
    fn view_single_component_immutable() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When
        let pos: Option<&Position> = unsafe { <&Position>::fetch(&table, row) };

        // Then
        assert!(pos.is_some());
        let pos = pos.unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }

    #[test]
    fn view_single_component_mutable() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When
        let health: Option<&mut Health> = unsafe { <&mut Health>::fetch_mut(&mut table, row) };

        // Then
        assert!(health.is_some());
        let health = health.unwrap();
        assert_eq!(health.value, 100);

        // Modify the value
        health.value = 75;

        // Verify mutation
        let health: Option<&Health> = unsafe { <&Health>::fetch(&table, row) };
        assert_eq!(health.unwrap().value, 75);
    }

    #[test]
    fn view_tuple_two_components() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When
        let view: Option<(&Position, &Velocity)> =
            unsafe { <(&Position, &Velocity)>::fetch(&table, row) };

        // Then
        assert!(view.is_some());
        let (pos, vel) = view.unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
        assert_eq!(vel.dx, 0.5);
        assert_eq!(vel.dy, 0.3);
    }

    #[test]
    fn view_tuple_three_components() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When
        let view: Option<(&Position, &Velocity, &Health)> =
            unsafe { <(&Position, &Velocity, &Health)>::fetch(&table, row) };

        // Then
        assert!(view.is_some());
        let (pos, vel, health) = view.unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(vel.dx, 0.5);
        assert_eq!(health.value, 100);
    }

    #[test]
    fn view_tuple_mixed_mutability() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When
        let view: Option<(&Position, &mut Velocity, &mut Health)> =
            unsafe { <(&Position, &mut Velocity, &mut Health)>::fetch_mut(&mut table, row) };

        // Then
        assert!(view.is_some());
        let (pos, vel, health) = view.unwrap();

        // Read immutable component
        assert_eq!(pos.x, 1.0);

        // Modify mutable components
        vel.dx = 1.0;
        health.value = 50;

        // Verify mutations
        let view: Option<(&Velocity, &Health)> =
            unsafe { <(&Velocity, &Health)>::fetch(&table, row) };
        let (vel, health) = view.unwrap();
        assert_eq!(vel.dx, 1.0);
        assert_eq!(health.value, 50);
    }

    #[test]
    fn view_returns_none_for_invalid_row() {
        // Given
        let (table, _registry, _allocator) = setup_table();

        // When - try to fetch from invalid row
        let view: Option<&Position> = unsafe { <&Position>::fetch(&table, Row::new(999)) };

        // Then
        assert!(view.is_none());
    }

    #[test]
    fn view_returns_none_for_unregistered_component() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        #[derive(Component)]
        struct UnregisteredComponent;

        let entity = allocator.alloc();
        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When - try to fetch unregistered component
        let view: Option<&UnregisteredComponent> =
            unsafe { <&UnregisteredComponent>::fetch(&table, row) };

        // Then
        assert!(view.is_none());
    }

    #[test]
    fn view_empty_tuple() {
        // Given
        let (table, _registry, _allocator) = setup_table();

        // When
        let view: Option<()> = unsafe { <()>::fetch(&table, Row::new(0)) };

        // Then
        assert!(view.is_some());
    }

    #[test]
    fn view_mutable_from_immutable_table_returns_none() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        let row = table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // When - try to get mutable view from immutable table
        let view: Option<&mut Position> = unsafe { <&mut Position>::fetch(&table, row) };

        // Then
        assert!(view.is_none());
    }

    #[test]
    fn view_multiple_entities() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        let entity1 = allocator.alloc();
        let entity2 = allocator.alloc();
        let entity3 = allocator.alloc();

        let row1 = table.add_entity(
            entity1,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        let row2 = table.add_entity(
            entity2,
            (
                Position { x: 3.0, y: 4.0 },
                Velocity { dx: -0.5, dy: 0.8 },
                Health { value: 75 },
            ),
            &registry,
        );

        let row3 = table.add_entity(
            entity3,
            (
                Position { x: 5.0, y: 6.0 },
                Velocity { dx: 0.0, dy: -0.2 },
                Health { value: 50 },
            ),
            &registry,
        );

        // When - fetch views for each entity
        let view1: Option<(&Position, &Health)> =
            unsafe { <(&Position, &Health)>::fetch(&table, row1) };
        let view2: Option<(&Position, &Health)> =
            unsafe { <(&Position, &Health)>::fetch(&table, row2) };
        let view3: Option<(&Position, &Health)> =
            unsafe { <(&Position, &Health)>::fetch(&table, row3) };

        // Then
        let (pos1, health1) = view1.unwrap();
        assert_eq!(pos1.x, 1.0);
        assert_eq!(health1.value, 100);

        let (pos2, health2) = view2.unwrap();
        assert_eq!(pos2.x, 3.0);
        assert_eq!(health2.value, 75);

        let (pos3, health3) = view3.unwrap();
        assert_eq!(pos3.x, 5.0);
        assert_eq!(health3.value, 50);
    }

    #[test]
    fn view_large_tuple() {
        // Given - test with 4 components
        let registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Comp1(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp2(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp3(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp4(u32);

        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();
        let id4 = registry.register::<Comp4>();

        let spec = component::Spec::new(vec![id1, id2, id3, id4]);
        let mut table = Table::new(super::super::table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, (Comp1(1), Comp2(2), Comp3(3), Comp4(4)), &registry);

        // When
        let view: Option<(&Comp1, &Comp2, &Comp3, &Comp4)> =
            unsafe { <(&Comp1, &Comp2, &Comp3, &Comp4)>::fetch(&table, row) };

        // Then
        assert!(view.is_some());
        let (c1, c2, c3, c4) = view.unwrap();
        assert_eq!(c1.0, 1);
        assert_eq!(c2.0, 2);
        assert_eq!(c3.0, 3);
        assert_eq!(c4.0, 4);
    }

    #[test]
    fn view_nested_tuple() {
        // Given - test with 4 components
        let registry = component::Registry::new();

        #[derive(Component, Debug, PartialEq)]
        struct Comp1(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp2(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp3(u32);
        #[derive(Component, Debug, PartialEq)]
        struct Comp4(u32);

        let id1 = registry.register::<Comp1>();
        let id2 = registry.register::<Comp2>();
        let id3 = registry.register::<Comp3>();
        let id4 = registry.register::<Comp4>();

        let spec = component::Spec::new(vec![id1, id2, id3, id4]);
        let mut table = Table::new(super::super::table::Id::new(0), spec, &registry);

        let mut allocator = entity::Allocator::new();
        let entity = allocator.alloc();

        let row = table.add_entity(entity, (Comp1(1), Comp2(2), Comp3(3), Comp4(4)), &registry);

        // When
        #[allow(clippy::type_complexity)]
        let view: Option<((&Comp1, &Comp2), (&Comp3, &Comp4))> =
            unsafe { <((&Comp1, &Comp2), (&Comp3, &Comp4))>::fetch(&table, row) };

        // Then
        assert!(view.is_some());
        let ((c1, c2), (c3, c4)) = view.unwrap();
        assert_eq!(c1.0, 1);
        assert_eq!(c2.0, 2);
        assert_eq!(c3.0, 3);
        assert_eq!(c4.0, 4);
    }

    #[test]
    fn view_iter_single_component() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        // Add multiple entities
        for i in 0..5 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position {
                        x: i as f32,
                        y: i as f32 * 2.0,
                    },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When
        let positions: Vec<&Position> =
            unsafe { table.iter_views::<&Position>().map(|(_e, p)| p).collect() };

        // Then
        assert_eq!(positions.len(), 5);
        for (i, pos) in positions.iter().enumerate() {
            assert_eq!(pos.x, i as f32);
            assert_eq!(pos.y, i as f32 * 2.0);
        }
    }

    #[test]
    fn view_iter_multiple_components() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        // Add multiple entities
        for i in 0..3 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position {
                        x: i as f32,
                        y: i as f32,
                    },
                    Velocity {
                        dx: i as f32 * 0.1,
                        dy: i as f32 * 0.2,
                    },
                    Health { value: 100 - i },
                ),
                &registry,
            );
        }

        // When
        let mut count = 0;
        for (i, (_e, (pos, vel, health))) in
            unsafe { table.iter_views::<(&Position, &Velocity, &Health)>() }.enumerate()
        {
            // Then
            assert_eq!(pos.x, i as f32);
            assert_eq!(vel.dx, i as f32 * 0.1);
            assert_eq!(health.value, 100 - i as i32);
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn view_iter_mut_single_component() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        for i in 0..3 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position { x: 0.0, y: 0.0 },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: i },
                ),
                &registry,
            );
        }

        // When - modify all healths
        for (_, health) in unsafe { table.iter_views_mut::<&mut Health>() } {
            health.value += 10;
        }

        // Then - verify modifications
        for (i, (entity, health)) in unsafe { table.iter_views::<&Health>() }.enumerate() {
            assert_eq!(entity, table.entity(Row::new(i)).unwrap());
            assert_eq!(health.value, i as i32 + 10);
        }
    }

    #[test]
    fn view_iter_mut_mixed_mutability() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        for i in 0..3 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position {
                        x: i as f32,
                        y: i as f32,
                    },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When - update velocity based on position
        for (_entity, (pos, vel)) in unsafe { table.iter_views_mut::<(&Position, &mut Velocity)>() }
        {
            vel.dx = pos.x * 0.5;
            vel.dy = pos.y * 0.5;
        }

        // Then - verify modifications
        for (i, (_entity, (pos, vel))) in
            unsafe { table.iter_views::<(&Position, &Velocity)>() }.enumerate()
        {
            assert_eq!(pos.x, i as f32);
            assert_eq!(vel.dx, i as f32 * 0.5);
            assert_eq!(vel.dy, i as f32 * 0.5);
        }
    }

    #[test]
    fn view_iter_empty_table() {
        // Given
        let (table, _registry, _allocator) = setup_table();

        // When
        let count = unsafe { table.iter_views::<&Position>() }.count();

        // Then
        assert_eq!(count, 0);
    }

    #[test]
    fn view_iter_size_hint() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        for _ in 0..10 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position { x: 0.0, y: 0.0 },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When
        let iter = unsafe { table.iter_views::<&Position>() };

        // Then
        assert_eq!(iter.size_hint(), (10, Some(10)));
        assert_eq!(iter.len(), 10);
    }

    #[test]
    fn view_iter_exact_size() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        for _ in 0..5 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position { x: 0.0, y: 0.0 },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When
        let mut iter = unsafe { table.iter_views::<&Position>() };

        // Then - check that ExactSizeIterator is implemented
        assert_eq!(iter.len(), 5);
        iter.next();
        assert_eq!(iter.len(), 4);
        iter.next();
        assert_eq!(iter.len(), 3);
    }

    #[test]
    fn view_iter_mut_exact_size() {
        // Given
        let (mut table, registry, mut allocator) = setup_table();

        for _ in 0..5 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position { x: 0.0, y: 0.0 },
                    Velocity { dx: 0.0, dy: 0.0 },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When
        let mut iter = unsafe { table.iter_views_mut::<&mut Position>() };

        // Then - check that ExactSizeIterator is implemented
        assert_eq!(iter.len(), 5);
        iter.next();
        assert_eq!(iter.len(), 4);
        iter.next();
        assert_eq!(iter.len(), 3);
    }

    #[test]
    fn view_iter_physics_pattern() {
        // Given - simulate a physics update pattern
        let (mut table, registry, mut allocator) = setup_table();

        for i in 0..3 {
            let entity = allocator.alloc();
            table.add_entity(
                entity,
                (
                    Position { x: 0.0, y: 0.0 },
                    Velocity {
                        dx: i as f32,
                        dy: i as f32 * 2.0,
                    },
                    Health { value: 100 },
                ),
                &registry,
            );
        }

        // When - apply velocity to position
        let dt = 1.0;
        for (_entity, (pos, vel)) in unsafe { table.iter_views_mut::<(&mut Position, &Velocity)>() }
        {
            pos.x += vel.dx * dt;
            pos.y += vel.dy * dt;
        }

        // Then - verify positions updated
        for (i, (_entity, pos)) in unsafe { table.iter_views::<&Position>() }.enumerate() {
            assert_eq!(pos.x, i as f32);
            assert_eq!(pos.y, i as f32 * 2.0);
        }
    }

    #[test]
    #[should_panic(expected = "View aliasing violation")]
    fn view_duplicate_mutable_component_panics_on_iterator_creation() {
        // This test verifies that creating an iterator with duplicate mutable
        // components is now caught and panics at runtime.

        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // Try to create an iterator with duplicate mutable Position
        // This should panic because it would create aliased mutable references
        type DuplicateView<'a> = (&'a mut Position, &'a mut Position);

        // This should panic during ViewIterMut::new()
        let _iter = unsafe { table.iter_views_mut::<DuplicateView>() };
    }

    #[test]
    fn view_mixed_immutable_and_mutable_components_ok() {
        // This test verifies that having the same component both immutable
        // and mutable is NOT allowed (also creates aliasing).

        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // This is safe - different components, one mutable, one not
        type MixedView<'a> = (&'a Position, &'a mut Velocity);

        let iter = unsafe { table.iter_views_mut::<MixedView>() };
        assert_eq!(iter.len(), 1);
    }

    #[test]
    fn view_multiple_different_mutable_components_ok() {
        // This test verifies that having multiple DIFFERENT mutable components is fine

        let (mut table, registry, mut allocator) = setup_table();
        let entity = allocator.alloc();

        table.add_entity(
            entity,
            (
                Position { x: 1.0, y: 2.0 },
                Velocity { dx: 0.5, dy: 0.3 },
                Health { value: 100 },
            ),
            &registry,
        );

        // This is safe - all different components
        type MultiMutView<'a> = (&'a mut Position, &'a mut Velocity, &'a mut Health);

        let iter = unsafe { table.iter_views_mut::<MultiMutView>() };
        assert_eq!(iter.len(), 1);
    }
}
