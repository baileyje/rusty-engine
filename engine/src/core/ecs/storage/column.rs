use std::{
    alloc::Layout,
    any::TypeId,
    ptr::{self},
};

use crate::core::ecs::{
    component::{self, Component},
    storage::{
        cell::{Cell, CellMut},
        mem::IndexedMemory,
        row::Row,
    },
};

/// A type-erased, contiguous storage for uniform-sized elements.
/// This is similar to Vec<T> but stores elements without compile-time type information.
/// Elements can be downcast when the type is known from context.
///
/// This provides cache-friendly storage for ECS component columns where all elements
/// in a column are the same type, but different columns may store different types.
///
/// # Example Usage
///
/// ```ignore
/// use crate::core::ecs::storage::Column;
/// use rusty_macros::Component;
///
/// #[derive(Component, Debug, PartialEq)]
/// struct Position { x: f32, y: f32 }
///
/// let mut registry = rusty_engine::core::ecs::component::Registry::new();
///
/// registry.register::<Position>();
///
/// // Create a type-erased column for Position components
/// let mut column = Column::new(registry.get_info::<Position>().unwrap());
///
/// // Reserve space for 3 elements
/// column.reserve(3);
///
/// // Write to reserved space (requires unsafe because type is erased)
/// unsafe {
///     column.write(0.into(), Position { x: 1.0, y: 2.0 });
///     column.write(1.into(), Position { x: 2.0, y: 3.0 });
///     column.write(2.into(), Position { x: 3.0, y: 4.0 });
///     column.set_len(3);  // Mark all values as initialized
/// }
///
/// // Iterate over all elements efficiently
/// unsafe {
///     for pos in column.iter::<Position>() {
///         println!("Position: ({}, {})", pos.x, pos.y);
///     }
/// }
///
/// // Mutate elements
/// unsafe {
///     for pos in column.iter_mut::<Position>() {
///         pos.x *= 2.0;
///     }
/// }
/// ```
///
/// # Safety
/// This structure maintains the following invariants:
/// - `ptr` points to a valid allocation if `capacity > 0`
/// - `len <= capacity`
/// - All elements from 0..len are properly initialized
/// - The layout used matches the actual element type
pub struct Column {
    /// Raw pointer to the start of the allocated memory.
    data: IndexedMemory,

    /// Number of elements currently stored.
    len: usize,

    /// Info about the column item (size, align and drop).
    info: component::Info,
}

impl Column {
    /// Create a new empty column with the given component Info.
    #[inline]
    pub fn new(info: component::Info) -> Self {
        Self {
            data: IndexedMemory::new(info.layout(), super::mem::GrowthStrategy::Multiply(2)),
            len: 0,
            info,
        }
    }

    /// Reserves capacity for at least `additional` more elements.
    ///
    /// This ensures the column has space for `self.len() + additional` elements
    /// without reallocating. Does not modify the length - use `set_len()` to
    /// mark reserved memory as initialized after writing values.
    ///
    /// # Examples
    /// ```ignore
    /// column.reserve(10);
    /// for i in 0..10 {
    ///     unsafe { column.write(Row(column.len() + i), value); }
    /// }
    /// unsafe { column.set_len(column.len() + 10); }
    /// ```
    pub fn reserve(&mut self, additional: usize) {
        self.data.reserve(additional);
        // Does NOT modify self.len - that's set_len()'s job
    }

    /// Sets the length of the column.
    ///
    /// This will explicitly set the size of the column, without actually
    /// modifying its contents. The caller must ensure that all elements
    /// in the range `[0..new_len)` are properly initialized.
    ///
    /// # Safety
    /// - `new_len` must be less than or equal to `capacity()`
    /// - All elements in `[0..new_len)` must be properly initialized
    /// - The type of the elements must match the column's component type
    ///
    /// # Examples
    /// ```ignore
    /// column.reserve(10);
    /// for i in 0..10 {
    ///     unsafe { column.write(Row(i), values[i]); }
    /// }
    /// unsafe { column.set_len(10); }
    /// ```
    pub unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(
            new_len <= self.data.capacity(),
            "new_len ({}) exceeds capacity ({})",
            new_len,
            self.data.capacity()
        );
        self.len = new_len;
    }

    /// Write a value into the column at the given index.
    ///
    /// This writes to reserved but potentially uninitialized memory.
    /// After writing all values, call `set_len()` to mark them as initialized.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - `row.index() < self.capacity()` (index is within reserved capacity)
    /// - value C is the correct type for this column's layout
    /// - All indices `[0..new_len)` are written before calling `set_len(new_len)`
    ///
    /// # Example
    /// ```ignore
    /// column.reserve(3);
    /// unsafe {
    ///     column.write(Row(0), value1);
    ///     column.write(Row(1), value2);
    ///     column.write(Row(2), value3);
    ///     column.set_len(3);  // Now mark all as initialized
    /// }
    /// ```
    pub unsafe fn write<C: Component>(&mut self, row: Row, value: C) {
        debug_assert!(row.index() < self.data.capacity(), "index out of bounds");

        // Safety - Caller must ensure type is valid for column/cell.
        unsafe { self.cell_mut(row).write(value) };
    }

    /// Push a value onto the column.
    ///
    /// # Panics
    /// - Panics if allocation fails.
    /// - Panics if the type `C` doesn't match the column's component type.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - value C is the correct type for this column's layout
    pub unsafe fn push<C: Component>(&mut self, value: C) {
        // Validate type matches column's component type
        self.ensure_type::<C>();

        // Reserve an additional row.
        self.reserve(1);
        // SAFETY: len < capacity after reserve(1), and we're writing to self.len which is valid
        unsafe {
            self.write(Row::new(self.len), value);
        }
        // Update length to mark row initialized
        self.len += 1;
    }

    /// Get an immutable reference to the component at the given row.
    ///
    /// Returns `None` if the row index is out of bounds (>= len).
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - Component type `C` matches the column's component type
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the column's type.
    pub unsafe fn get<C: Component>(&self, row: Row) -> Option<&C> {
        if !self.is_row_valid(row) {
            return None;
        }
        Some(self.cell(row).as_ref())
    }

    /// Get a mutable reference to the component at the given row.
    ///
    /// Returns `None` if the row index is out of bounds (>= len).
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - Component type `C` matches the column's component type
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the column's type.
    pub unsafe fn get_mut<C: Component>(&mut self, row: Row) -> Option<&mut C> {
        if !self.is_row_valid(row) {
            return None;
        }
        Some(self.cell_mut(row).into_mut())
    }

    /// Remove the element at the given index using swap-remove.
    ///
    /// # Safety
    /// The caller must ensure that:
    /// - `index < self.len()`
    pub unsafe fn swap_remove(&mut self, row: Row) {
        debug_assert!(self.is_row_valid(row), "index out of bounds");

        // Start by tracking the indexes of the rows impacted
        let row_index = row.index();
        let last_index = self.len - 1;

        // Get pointers to the element to remove and the last element
        let element_ptr = self.data.ptr_at_mut(row.index());
        let last_ptr = self.data.ptr_at_mut(last_index);

        if row_index != last_index {
            // Swap with the last element
            // SAFETY: Both pointers are valid and within bounds
            unsafe {
                ptr::swap_nonoverlapping(
                    element_ptr.as_ptr(),
                    last_ptr.as_ptr(),
                    self.info.layout().size(),
                );
            }
        }
        // Drop the element now at last_index
        unsafe {
            (self.info.drop_fn())(last_ptr);
        }

        self.len -= 1;
    }

    /// Get the column info.
    #[inline]
    pub fn info(&self) -> &component::Info {
        &self.info
    }

    /// Get the number of elements in the column.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Check if the column is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the capacity of the column.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.data.capacity()
    }

    /// Get a cell for the given row.
    ///
    /// # Safety
    /// - The caller must ensure the row index is valid (< len for initialized access)
    #[inline]
    pub fn cell(&self, row: Row) -> Cell<'_> {
        debug_assert!(self.is_row_valid(row), "invalid row index");
        Cell::new(self.data.ptr_at(row.index()), &self.info)
    }

    /// Get a mutable cell for the given row.
    ///
    /// # Safety
    /// - The caller must ensure the row index is within capacity (< capacity for reserved access)
    #[inline]
    pub fn cell_mut(&mut self, row: Row) -> CellMut<'_> {
        debug_assert!(row.index() < self.capacity(), "row index exceeds capacity");
        CellMut::new(self.data.ptr_at_mut(row.index()), &self.info)
    }

    /// Clear all elements from the column, calling their destructors.
    pub fn clear(&mut self) {
        unsafe {
            for i in 0..self.len {
                let ptr = self.data.ptr_at_mut(i);
                (self.info.drop_fn())(ptr);
            }
        }
        self.len = 0;
    }

    /// Returns an iterator over references to the elements.
    ///
    /// # Safety
    /// The caller must ensure that `T` matches the type used to create this column.
    ///
    /// # Panics
    /// Panics if the generic type `C` doesn't match the column's component type.
    pub unsafe fn iter<C: Component>(&self) -> ColumnIter<'_, C> {
        self.ensure_type::<C>();

        ColumnIter {
            column: self,
            index: 0,
            _marker: std::marker::PhantomData,
        }
    }

    /// Returns an iterator over mutable references to the elements.
    ///
    /// # Safety
    /// The caller must ensure that `T` matches the type used to create this column.
    ///
    /// # Panics
    /// Panics if the generic type `C` doesn't match the column's component type.
    pub unsafe fn iter_mut<'a, C: Component>(&'a mut self) -> ColumnIterMut<'a, C> {
        self.ensure_type::<C>();
        let len = self.len;

        ColumnIterMut {
            column: self,
            len,
            index: 0,
            _marker: std::marker::PhantomData,
        }
    }

    /// Ensure the type `C` is valid for this column.
    ///
    /// This validates both TypeId and Layout to catch type mismatches at iterator creation.
    /// The cost is ~0.26ns per check (negligible overhead).
    ///
    /// # Panics
    /// Panics if:
    /// - The TypeId of `C` doesn't match the column's stored type
    /// - The Layout of `C` doesn't match the column's layout
    #[inline]
    pub fn ensure_type<C: Component>(&self) {
        assert!(
            TypeId::of::<C>() == self.info.type_id(),
            "Type mismatch: attempted to use type {} with column storing {:?}",
            std::any::type_name::<C>(),
            self.info
        );
        assert!(
            Layout::new::<C>() == self.info.layout(),
            "Layout mismatch: component layout does not match column layout"
        );
    }

    /// Determine if a row is valid for this column.
    #[inline]
    pub fn is_row_valid(&self, row: Row) -> bool {
        row.index() < self.len
    }
}

/// Iterator over column elements.
pub struct ColumnIter<'a, C: Component> {
    column: &'a Column,
    index: usize,
    _marker: std::marker::PhantomData<&'a C>,
}

impl<'a, C: Component> Iterator for ColumnIter<'a, C> {
    type Item = &'a C;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.column.len {
            let item = unsafe { self.column.get::<C>(Row::new(self.index)) };
            self.index += 1;
            item
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.column.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, C: Component> ExactSizeIterator for ColumnIter<'a, C> {}

/// Mutable iterator over column elements.
pub struct ColumnIterMut<'a, C: Component> {
    column: &'a mut Column,
    len: usize,
    index: usize,
    _marker: std::marker::PhantomData<&'a mut C>,
}

impl<'a, C: Component> Iterator for ColumnIterMut<'a, C> {
    type Item = &'a mut C;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.len {
            let ptr = self.column.data.ptr_at(self.index);
            self.index += 1;
            // SAFETY:
            // - index < len, so this is a valid element
            // - We increment index, so we never return the same element twice
            // - The lifetime 'a ensures exclusive access for the iterator's lifetime
            unsafe { Some(&mut *(ptr.as_ptr() as *mut C)) }
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, C: Component> ExactSizeIterator for ColumnIterMut<'a, C> {}

impl Drop for Column {
    fn drop(&mut self) {
        // Drop all elements - the IndexedMemory will handle memory deallocation
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use rusty_macros::Component;

    use super::*;

    #[test]
    fn column_push_and_access() {
        // Given
        #[derive(Component)]
        struct Position {
            x: f32,
            y: f32,
        }

        let registry = component::Registry::new();
        let id = registry.register::<Position>();

        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When

        unsafe {
            column.push(Position { x: 1.0, y: 2.0 });
            column.push(Position { x: 3.0, y: 4.0 });
        }

        // Then

        assert_eq!(column.len(), 2);

        unsafe {
            let mut itr = column.iter::<Position>();
            let pos0 = itr.next().unwrap();
            assert_eq!(pos0.x, 1.0);
            assert_eq!(pos0.y, 2.0);

            let pos1 = itr.next().unwrap();
            assert_eq!(pos1.x, 3.0);
            assert_eq!(pos1.y, 4.0);
        }
    }

    #[test]
    fn column_reserve_and_write() {
        // Given
        #[derive(Component)]
        struct Position {
            x: f32,
            y: f32,
        }

        let registry = component::Registry::new();
        let id = registry.register::<Position>();

        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When
        column.reserve(2);

        unsafe {
            column.write(Row::new(0), Position { x: 1.0, y: 2.0 });
            column.write(Row::new(1), Position { x: 2.0, y: 3.0 });
            column.set_len(2); // Mark as initialized after writes
        }

        // Then

        assert_eq!(column.len(), 2);

        unsafe {
            let mut itr = column.iter::<Position>();
            let pos0 = itr.next().unwrap();
            assert_eq!(pos0.x, 1.0);
            assert_eq!(pos0.y, 2.0);

            let pos1 = itr.next().unwrap();
            assert_eq!(pos1.x, 2.0);
            assert_eq!(pos1.y, 3.0);
        }
    }

    #[test]
    fn column_swap_remove_drops() {
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

        let registry = component::Registry::new();
        let id = registry.register::<DropTracker>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        let counter = Arc::new(AtomicUsize::new(0));

        // Add 3 elements
        unsafe {
            column.push(DropTracker(counter.clone()));
            column.push(DropTracker(counter.clone()));
            column.push(DropTracker(counter.clone()));
        }

        assert_eq!(column.len(), 3);
        assert_eq!(counter.load(Ordering::SeqCst), 0, "No drops yet");

        // When - remove the middle element
        unsafe {
            column.swap_remove(Row::new(1));
        }

        // Then

        assert_eq!(column.len(), 2);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "Should have dropped 1 element"
        );

        // When - remove another element
        unsafe {
            column.swap_remove(Row::new(0));
        }

        // Then

        assert_eq!(column.len(), 1);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "Should have dropped 2 elements"
        );

        // When - Drop the column (should drop the remaining element)
        drop(column);

        // Then
        assert_eq!(
            counter.load(Ordering::SeqCst),
            3,
            "Should have dropped all 3 elements"
        );
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn column_ensures_index_on_write_panics() {
        // Given
        #[derive(Component)]
        struct Comp1 {}

        let registry = component::Registry::new();
        let id = registry.register::<Comp1>();

        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When - Then
        unsafe {
            column.write(Row::new(0), Comp1 {}); // Should panic
        }
    }

    #[test]
    #[should_panic(expected = "Type mismatch: ")]
    fn column_ensures_type_on_write_panics() {
        // Given
        #[derive(Component)]
        struct Comp1 {}

        #[derive(Component)]
        struct Comp2 {}

        let registry = component::Registry::new();
        let id = registry.register::<Comp1>();

        let mut column = Column::new(registry.get_info_by_id(id).unwrap());
        column.reserve(1);

        // When - Then
        unsafe {
            column.set_len(1); // Set length so write() validates type against initialized memory
            column.write(Row::new(0), Comp2 {}); // Should panic
        }
    }

    #[test]
    fn column_zst_component() {
        // Given
        #[derive(Component, Debug)]
        struct Marker;

        let registry = component::Registry::new();
        let id = registry.register::<Marker>();

        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When

        unsafe {
            column.push(Marker);
            column.push(Marker);
        }

        // Then

        assert_eq!(column.data.capacity(), 2);

        assert_eq!(column.len(), 2);

        assert_eq!(column.data.as_ptr(), column.data.ptr_at(0).as_ptr());
        assert_eq!(column.data.as_ptr(), column.data.ptr_at(1).as_ptr());

        unsafe {
            let mut itr = column.iter::<Marker>();
            let marker = itr.next();
            assert!(marker.is_some());

            let marker = itr.next();
            assert!(marker.is_some());
        }
    }

    #[test]
    fn column_get_and_get_mut() {
        // Given
        #[derive(Component, Debug, PartialEq)]
        struct Position {
            x: f32,
            y: f32,
        }

        let registry = component::Registry::new();
        let id = registry.register::<Position>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        unsafe {
            column.push(Position { x: 1.0, y: 2.0 });
            column.push(Position { x: 3.0, y: 4.0 });
        }

        // When/Then - get() returns Some for valid indices
        unsafe {
            let pos = column.get::<Position>(Row::new(0));
            assert!(pos.is_some());
            assert_eq!(pos.unwrap().x, 1.0);

            let pos = column.get::<Position>(Row::new(1));
            assert!(pos.is_some());
            assert_eq!(pos.unwrap().y, 4.0);
        }

        // When/Then - get() returns None for invalid indices
        unsafe {
            let pos = column.get::<Position>(Row::new(10));
            assert!(pos.is_none());
        }

        // When/Then - get_mut() returns Some and allows mutation
        unsafe {
            let pos = column.get_mut::<Position>(Row::new(0));
            assert!(pos.is_some());
            pos.unwrap().x = 100.0;

            let pos = column.get::<Position>(Row::new(0));
            assert_eq!(pos.unwrap().x, 100.0);
        }

        // When/Then - get_mut() returns None for invalid indices
        unsafe {
            let pos = column.get_mut::<Position>(Row::new(10));
            assert!(pos.is_none());
        }
    }

    #[test]
    fn column_iter_mut() {
        // Given
        #[derive(Component)]
        struct Position {
            x: f32,
            y: f32,
        }

        let registry = component::Registry::new();
        let id = registry.register::<Position>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        unsafe {
            column.push(Position { x: 1.0, y: 2.0 });
            column.push(Position { x: 3.0, y: 4.0 });
            column.push(Position { x: 5.0, y: 6.0 });
        }

        // When - mutate all elements
        unsafe {
            for pos in column.iter_mut::<Position>() {
                pos.x *= 2.0;
                pos.y *= 2.0;
            }
        }

        // Then - verify mutations
        unsafe {
            let mut iter = column.iter::<Position>();
            let pos = iter.next().unwrap();
            assert_eq!(pos.x, 2.0);
            assert_eq!(pos.y, 4.0);

            let pos = iter.next().unwrap();
            assert_eq!(pos.x, 6.0);
            assert_eq!(pos.y, 8.0);

            let pos = iter.next().unwrap();
            assert_eq!(pos.x, 10.0);
            assert_eq!(pos.y, 12.0);

            assert!(iter.next().is_none());
        }
    }

    #[test]
    fn column_clear() {
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

        let registry = component::Registry::new();
        let id = registry.register::<DropTracker>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        let counter = Arc::new(AtomicUsize::new(0));

        unsafe {
            column.push(DropTracker(counter.clone()));
            column.push(DropTracker(counter.clone()));
            column.push(DropTracker(counter.clone()));
        }

        assert_eq!(column.len(), 3);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // When - clear the column
        column.clear();

        // Then
        assert_eq!(column.len(), 0);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            3,
            "All elements should be dropped"
        );
    }

    #[test]
    fn column_capacity_growth() {
        // Given
        #[derive(Component)]
        struct Position {
            #[allow(dead_code)]
            x: f32,
        }

        let registry = component::Registry::new();
        let id = registry.register::<Position>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        assert_eq!(column.capacity(), 0);

        // When - push first element
        unsafe {
            column.push(Position { x: 1.0 });
        }

        // Then - capacity should grow (2x strategy)
        let first_capacity = column.capacity();
        assert!(first_capacity >= 1);

        // When - push more elements up to capacity
        unsafe {
            while column.len() < first_capacity {
                column.push(Position {
                    x: column.len() as f32,
                });
            }
        }

        assert_eq!(column.len(), first_capacity);

        // When - push one more to trigger growth
        unsafe {
            column.push(Position { x: 999.0 });
        }

        // Then - capacity should have grown
        assert!(
            column.capacity() > first_capacity,
            "Capacity should grow when full"
        );
        assert_eq!(column.len(), first_capacity + 1);
    }

    #[test]
    fn column_reserve_exact_amount() {
        // Given
        #[derive(Component)]
        struct Data(u32);

        let registry = component::Registry::new();
        let id = registry.register::<Data>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When - reserve exact amount
        column.reserve(5);

        // Then
        assert!(column.capacity() >= 5);
        assert_eq!(column.len(), 0); // Length unchanged

        // When - write to reserved space
        unsafe {
            for i in 0..5 {
                column.write(Row::new(i), Data(i as u32));
            }
            column.set_len(5);
        }

        // Then - verify all values
        unsafe {
            for (i, data) in column.iter::<Data>().enumerate() {
                assert_eq!(data.0, i as u32);
            }
        }
    }

    #[test]
    fn column_empty_operations() {
        // Given
        #[derive(Component)]
        struct Empty;

        let registry = component::Registry::new();
        let id = registry.register::<Empty>();
        let column = Column::new(registry.get_info_by_id(id).unwrap());

        // Then
        assert!(column.is_empty());
        assert_eq!(column.len(), 0);

        // When - try to get from empty column
        unsafe {
            let result = column.get::<Empty>(Row::new(0));
            assert!(result.is_none());
        }
    }

    #[test]
    fn column_swap_remove_order() {
        // Given
        #[derive(Component, Debug, PartialEq)]
        struct Value(u32);

        let registry = component::Registry::new();
        let id = registry.register::<Value>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        unsafe {
            column.push(Value(0));
            column.push(Value(1));
            column.push(Value(2));
            column.push(Value(3));
        }

        assert_eq!(column.len(), 4);

        // When - swap remove index 1 (value 1)
        unsafe {
            column.swap_remove(Row::new(1));
        }

        // Then - last element (3) should now be at index 1
        assert_eq!(column.len(), 3);
        unsafe {
            assert_eq!(column.get::<Value>(Row::new(0)).unwrap().0, 0);
            assert_eq!(column.get::<Value>(Row::new(1)).unwrap().0, 3); // Swapped from end
            assert_eq!(column.get::<Value>(Row::new(2)).unwrap().0, 2);
        }

        // When - swap remove the last element
        unsafe {
            column.swap_remove(Row::new(2));
        }

        // Then - no swap occurs, just removes last
        assert_eq!(column.len(), 2);
        unsafe {
            assert_eq!(column.get::<Value>(Row::new(0)).unwrap().0, 0);
            assert_eq!(column.get::<Value>(Row::new(1)).unwrap().0, 3);
        }
    }

    #[test]
    fn column_iterator_exact_size() {
        // Given
        #[derive(Component)]
        #[allow(dead_code)]
        struct Value(u32);

        let registry = component::Registry::new();
        let id = registry.register::<Value>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        unsafe {
            for i in 0..10 {
                column.push(Value(i));
            }
        }

        // When/Then - iterator reports correct size
        unsafe {
            let iter = column.iter::<Value>();
            assert_eq!(iter.len(), 10);
            assert_eq!(iter.size_hint(), (10, Some(10)));

            let mut iter_mut = column.iter_mut::<Value>();
            assert_eq!(iter_mut.len(), 10);
            assert_eq!(iter_mut.size_hint(), (10, Some(10)));

            // Advance and check size updates
            iter_mut.next();
            iter_mut.next();
            assert_eq!(iter_mut.len(), 8);
            assert_eq!(iter_mut.size_hint(), (8, Some(8)));
        }
    }

    #[test]
    #[should_panic(expected = "Type mismatch")]
    fn column_iter_type_check_panics_in_release() {
        // This test verifies that type checking happens in BOTH debug and release builds
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

        let registry = component::Registry::new();
        let id = registry.register::<TypeA>();
        let column = Column::new(registry.get_info_by_id(id).unwrap());

        // When/Then - should panic even in release builds
        unsafe {
            let _ = column.iter::<TypeB>(); // Wrong type!
        }
    }

    #[test]
    #[should_panic(expected = "Type mismatch")]
    fn column_iter_mut_type_check_panics_in_release() {
        // This test verifies that type checking happens in BOTH debug and release builds
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

        let registry = component::Registry::new();
        let id = registry.register::<TypeA>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // When/Then - should panic even in release builds
        unsafe {
            let _ = column.iter_mut::<TypeB>(); // Wrong type!
        }
    }

    #[test]
    #[should_panic(expected = "Type mismatch")]
    fn column_push_type_check_panics_in_release() {
        // This test verifies that Column::push validates types in BOTH debug and release builds
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

        let registry = component::Registry::new();
        let id = registry.register::<TypeA>();
        let mut column = Column::new(registry.get_info_by_id(id).unwrap());

        // Add a valid value first
        unsafe {
            column.push(TypeA { value: 42 });
        }

        // When/Then - should panic when pushing wrong type, even in release builds
        unsafe {
            column.push(TypeB { value: 99 }); // Wrong type!
        }
    }
}
