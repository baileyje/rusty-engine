use std::{
    alloc::Layout,
    any::TypeId,
    ptr::{self, NonNull},
};

use crate::ecs::component::{self};

/// A cell holds a component value for a specific table row/column intersection.
/// This is the primary access pattern for reading component data from tables.
///
/// `Cell` is `Copy` because it's semantically equivalent to a shared reference (`&T`).
/// Multiple immutable cells pointing to the same data are safe, as Rust's borrow
/// checker ensures the returned references (`&'a C`) cannot be misused.
///
/// # Design
///
/// Cells provide a type-erased pointer that can be "downcast" to the correct
/// component type when needed. This allows tables to store heterogeneous component
/// types while maintaining cache-friendly columnar storage.
///
/// # Example
///
/// ```ignore
/// let cell = column.cell(row);
/// let pos: &Position = cell.as_ref();
/// let vel: &Velocity = cell.as_ref();  // Can reuse cell for different reads
/// ```
#[derive(Debug, Copy, Clone)]
pub struct Cell<'a> {
    /// A pointer to the cell's memory.
    ptr: ptr::NonNull<u8>,

    // The info about the expected component type for this cell.
    #[cfg(debug_assertions)]
    info: &'a component::Info,

    // Ensure 'a is used even when debug_assertions is off
    #[cfg(not(debug_assertions))]
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> Cell<'a> {
    /// Construct a new cell from an existing NonNull pointer.
    pub fn new(inner: NonNull<u8>, #[allow(unused_variables)] info: &'a component::Info) -> Self {
        Self {
            ptr: inner,
            #[cfg(debug_assertions)]
            info,
            #[cfg(not(debug_assertions))]
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get a pointer to the raw data.
    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Get a reference to the cell's value as a given component type `C`.
    ///
    /// This is safe because the cell was constructed from a valid column with
    /// bounds-checked row access. Type checking is performed in debug builds.
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the cell's actual type.
    ///
    /// # Type Parameter
    /// You must explicitly specify the component type using turbofish syntax:
    /// ```ignore
    /// let pos: &Position = cell.as_ref::<Position>();
    /// ```
    #[inline]
    pub fn as_ref<C: component::Component>(&self) -> &'a C {
        #[cfg(debug_assertions)]
        ensure_type::<C>(self.info);

        let ptr = self.as_ptr().cast::<C>();

        #[cfg(debug_assertions)]
        debug_assert_eq!(
            ptr.align_offset(std::mem::align_of::<C>()),
            0,
            "Pointer is not properly aligned for type {}",
            std::any::type_name::<C>()
        );

        // SAFETY: The cell was constructed from a valid column with proper alignment
        // and the row was bounds-checked. The type is verified in debug builds.
        unsafe { &*ptr }
    }
}

/// A mutable cell holds a component value for a specific table row/column intersection.
/// This is the primary access pattern for writing component data to tables.
///
/// `CellMut` is **NOT** `Copy` or `Clone` - it's semantically equivalent to a mutable
/// reference (`&mut T`). Copying it would allow creating multiple mutable references
/// to the same memory, which is undefined behavior.
///
/// # Design
///
/// Like `Cell`, `CellMut` provides type-erased access to component data. However,
/// because it allows mutation, it must be consumed (moved) when dereferenced to
/// maintain Rust's aliasing guarantees.
///
/// # Example
///
/// ```ignore
/// let mut cell = column.cell_mut(row);
/// unsafe {
///     cell.write(Position { x: 1.0, y: 2.0 });  // For uninitialized memory
/// }
///
/// let mut cell = column.cell_mut(row);
/// let pos: &mut Position = cell.deref();  // Consumes cell, preventing aliasing
/// pos.x += 1.0;
/// ```
#[derive(Debug, Copy, Clone)]
pub struct CellMut<'a> {
    ptr: NonNull<u8>,

    // The info about the expected component type for this cell.
    #[cfg(debug_assertions)]
    info: &'a component::Info,

    // Ensure 'a is used even when debug_assertions is off
    #[cfg(not(debug_assertions))]
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> CellMut<'a> {
    /// Construct a new cell from an existing NonNull pointer.
    pub fn new(inner: NonNull<u8>, #[allow(unused_variables)] info: &'a component::Info) -> Self {
        Self {
            ptr: inner,
            #[cfg(debug_assertions)]
            info,
            #[cfg(not(debug_assertions))]
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get a pointer to the raw data.
    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Get a reference to the cell's value as a given component type `C`.
    ///
    /// This is safe because the cell was constructed from a valid column with
    /// bounds-checked row access. Type checking is performed in debug builds.
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the cell's actual type.
    #[inline]
    pub fn as_ref<C: component::Component>(&self) -> &'a C {
        #[cfg(debug_assertions)]
        ensure_type::<C>(self.info);

        let ptr = self.as_ptr().cast::<C>();

        // Ensure proper alignment in debug builds.
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            ptr.align_offset(std::mem::align_of::<C>()),
            0,
            "Pointer is not properly aligned for type {}",
            std::any::type_name::<C>()
        );

        // SAFETY: The cell was constructed from a valid column with proper alignment
        // and the row was bounds-checked. The type is verified in debug builds.
        unsafe { &*ptr }
    }

    /// Write a value to uninitialized memory in the cell.
    ///
    /// # Safety
    /// - **CRITICAL**: The memory must be UNINITIALIZED. Writing to initialized
    ///   memory will leak the old value. If the memory is already initialized,
    ///   use standard assignment (`*cell.as_mut() = value`) instead to properly
    ///   drop the old value.
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the cell's actual type.
    #[inline]
    pub unsafe fn write<C: component::Component>(&mut self, value: C) {
        #[cfg(debug_assertions)]
        ensure_type::<C>(self.info);

        unsafe { ptr::write(self.as_ptr() as *mut C, value) };
    }

    /// Get a mutable reference to the cell's value, consuming the cell.
    ///
    /// This method consumes `self` to prevent creating multiple mutable references
    /// to the same memory location, which would be undefined behavior.
    ///
    /// # Panics
    /// In debug builds, panics if type `C` doesn't match the cell's actual type.
    ///
    /// # Example
    /// ```ignore
    /// let cell = column.cell_mut(row);
    /// let pos: &mut Position = cell.into_mut();
    /// pos.x += 1.0;
    /// ```
    #[inline]
    pub fn into_mut<C: component::Component>(self) -> &'a mut C {
        #[cfg(debug_assertions)]
        ensure_type::<C>(self.info);

        let ptr = self.as_ptr().cast::<C>();
        // SAFETY: The cell was constructed from a valid column with proper alignment
        // and the row was bounds-checked. Consuming self prevents aliasing.
        unsafe { &mut *ptr }
    }
}

/// Ensure the type `C` is valid for this column.
#[cfg(debug_assertions)]
pub fn ensure_type<C: component::Component>(info: &component::Info) {
    debug_assert!(
        TypeId::of::<C>() == info.type_id(),
        "Type mismatch: attempted to use type {} with column storing components {:?}",
        std::any::type_name::<C>(),
        info
    );
    debug_assert!(
        Layout::new::<C>() == info.layout(),
        "pushed component layout does not match column layout"
    );
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn ensure_type<C: component::Component>(_info: &component::Info) {
    // No-op in release builds
}
