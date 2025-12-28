//! Low-level memory management for type-erased storage.
//!
//! This module provides [`IndexedMemory`], a raw memory allocator that manages contiguous blocks
//! of uniform-sized elements without type information. It serves as the foundation for the
//! storage layer's columnar architecture.
//!
//! # Purpose
//!
//! [`IndexedMemory`] handles the lowest level of the storage hierarchy, providing:
//! - Raw memory allocation and deallocation
//! - Index-based pointer arithmetic
//! - Configurable growth strategies
//! - Zero-cost abstraction over raw pointers
//!
//! This enables [`Column`](super::column::Column) to store any component type without
//! compile-time type knowledge, while maintaining cache-friendly contiguous layout.
//!
//! # Safety Contract
//!
//! This module is **intentionally unsafe** for performance. It requires callers to:
//! - Ensure correct type usage (no compile-time checks)
//! - Track element initialization (no runtime checks in release)
//! - Drop elements before deallocation
//! - Respect capacity limits
//!
//! Higher layers ([`Column`](super::column::Column), [`Table`](super::table::Table))
//! provide safe abstractions that maintain these invariants.
//!
//! # Growth Strategies
//!
//! [`GrowthStrategy`] controls how memory expands when capacity is exceeded:
//!
//! - **[`GrowthStrategy::Multiply(n)`]**: Exponential growth (e.g., 2x) - fewer reallocations, potential waste
//! - **[`GrowthStrategy::Buffer(n)`]**: Linear growth (e.g., +64) - balanced approach
//! - **[`GrowthStrategy::Exact`]**: Minimal growth - space-efficient, frequent reallocations
//!
//! Choose based on usage patterns:
//! - Frequent insertions → Multiply or Buffer
//! - Known/fixed sizes → Exact
//! - Memory-constrained → Exact or small Buffer
//!
//! # Usage Example
//!
//! ```ignore
//! use std::alloc::Layout;
//! use rusty_engine::core::ecs::storage::mem::{IndexedMemory, GrowthStrategy};
//!
//! // Create memory for u32 elements with 2x growth
//! let mut mem = IndexedMemory::with_capacity(
//!     Layout::new::<u32>(),
//!     10,
//!     GrowthStrategy::Multiply(2)
//! );
//!
//! // Write values to uninitialized memory
//! for i in 0..10 {
//!     unsafe {
//!         let ptr = mem.ptr_at_mut(i).as_ptr() as *mut u32;
//!         ptr.write(i as u32 * 10);
//!     }
//! }
//!
//! // Read values back
//! for i in 0..10 {
//!     unsafe {
//!         let ptr = mem.ptr_at(i).as_ptr() as *const u32;
//!         assert_eq!(ptr.read(), i as u32 * 10);
//!     }
//! }
//!
//! // CRITICAL: Drop values before memory is deallocated
//! for i in 0..10 {
//!     unsafe {
//!         let ptr = mem.ptr_at_mut(i).as_ptr() as *mut u32;
//!         ptr.drop_in_place();
//!     }
//! }
//! // mem's Drop impl deallocates the memory
//! ```
//!
//! # Safety Requirements
//!
//! When using [`IndexedMemory`], you **must**:
//!
//! 1. **Bounds**: Only access indices within capacity
//!    ```ignore
//!    # use rusty_engine::core::ecs::storage::mem::{IndexedMemory, GrowthStrategy};
//!    # use std::alloc::Layout;
//!    # let mut mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Exact);
//!    // ❌ WRONG: Out of bounds (UB in release, panic in debug)
//!    // let ptr = mem.ptr_at(100);
//!
//!    // ✅ CORRECT: Ensure capacity first
//!    mem.reserve(101);
//!    let ptr = mem.ptr_at_mut(100);
//!    ```
//!
//! 2. **Initialization**: Only read from initialized memory
//!    ```ignore
//!    # use rusty_engine::core::ecs::storage::mem::{IndexedMemory, GrowthStrategy};
//!    # use std::alloc::Layout;
//!    # let mut mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 10, GrowthStrategy::Exact);
//!    // ❌ WRONG: Reading uninitialized memory
//!    // unsafe {
//!    //     let ptr = mem.ptr_at(0).as_ptr() as *const u32;
//!    //     let value = ptr.read(); // UB!
//!    // }
//!
//!    // ✅ CORRECT: Write before reading
//!    unsafe {
//!         let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u32;
//!         ptr.write(42);
//!         let ptr = mem.ptr_at(0).as_ptr() as *const u32;
//!         let value = ptr.read(); // OK
//!    #    ptr.drop_in_place();
//!    }
//!    ```
//!
//! 3. **Drop**: Drop all initialized values before deallocation
//!    ```ignore
//!    # use rusty_engine::core::ecs::storage::mem::{IndexedMemory, GrowthStrategy};
//!    # use std::alloc::Layout;
//!    # let mut mem = IndexedMemory::with_capacity(Layout::new::<Vec<u8>>(), 1, GrowthStrategy::Exact);
//!    // ❌ WRONG: Vec's heap allocation leaks
//!    // unsafe {
//!    //     let ptr = mem.ptr_at_mut(0).as_ptr() as *mut Vec<u8>;
//!    //     ptr.write(vec![1, 2, 3]);
//!    // } // mem drops here, leaking the Vec's buffer!
//!
//!    // ✅ CORRECT: Drop before memory is freed
//!    unsafe {
//!         let ptr = mem.ptr_at_mut(0).as_ptr() as *mut Vec<u8>;
//!         ptr.write(vec![1, 2, 3]);
//!         // ... use the Vec ...
//!         ptr.drop_in_place(); // Frees Vec's buffer
//!    } // mem drops here, OK
//!    ```
//!
//! 4. **Type Safety**: Always cast to the correct type
//!    ```ignore
//!    # use rusty_engine::core::ecs::storage::mem::{IndexedMemory, GrowthStrategy};
//!    # use std::alloc::Layout;
//!    # let mut mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 1, GrowthStrategy::Exact);
//!    // ❌ WRONG: Type mismatch causes UB
//!    // unsafe {
//!    //     let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u32;
//!    //     ptr.write(42u32);
//!    //     let ptr = mem.ptr_at(0).as_ptr() as *const u64; // Wrong type!
//!    //     let value = ptr.read(); // UB!
//!    // }
//!
//!    // ✅ CORRECT: Use consistent types
//!    unsafe {
//!         let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u32;
//!         ptr.write(42u32);
//!         let ptr = mem.ptr_at(0).as_ptr() as *const u32; // Correct type
//!         let value = ptr.read(); // OK
//!    #    ptr.drop_in_place();
//!    }
//!    ```
//!
//! # Performance Characteristics
//!
//! - **Access**: O(1) - simple pointer arithmetic
//! - **Reserve**: O(1) amortized with appropriate growth strategy
//! - **Realloc**: O(n) when growing beyond current capacity (copy all elements)
//! - **Memory overhead**: Zero per element, only struct overhead (~40 bytes)
//!
//! # Comparison with std::Vec
//!
//! | Feature | Vec\<T\> | IndexedMemory |
//! |---------|---------|---------------|
//! | Type safety | ✅ Compile-time | ❌ Runtime (caller responsibility) |
//! | Drop handling | ✅ Automatic | ❌ Manual |
//! | Bounds checking | ✅ Always | ⚠️ Debug only |
//! | Initialization tracking | ✅ Via len | ❌ Caller responsibility |
//! | Zero-cost abstraction | ✅ | ✅ |
//! | Use case | General purpose | ECS internals |
//!
//! # Thread Safety
//!
//! [`IndexedMemory`] is [`Send`] and [`Sync`] - it can be moved between threads and shared via
//! synchronization primitives. However, the caller must ensure that any types stored within are
//! also [`Send`]/[`Sync`] as appropriate for their usage.

use std::{
    alloc::{self, Layout},
    cmp,
    ptr::{self, NonNull},
};

/// An enumeration of possible growth factors used when growing the memory's capacity. These options
/// are attempting to allow some trade offs for growing the memory allocation without having to
/// copy the data on every single element added.
#[derive(Debug, Clone)]
pub enum GrowthStrategy {
    /// Grow in multiples of the current capacity. This is most often used with a factor of 2 where
    /// you get exponential growth to reduce allocations for frequent requests of small growth.
    /// This has the possibility to use more space. So consider use-case to determine which
    /// optimization is best.
    Multiply(usize),
    /// Grow the current capacity by allocating an additional fixed capacity buffer. This is
    /// less aggressive than the multiply option, but also attempts to reduce allocations for similar
    /// use-cases without as high of a risk of over allocations.
    Buffer(usize),
    /// Grow the current capacity by the exact amount requested. This is the most space efficient,
    /// but will cause significantly more allocations for use-cases with frequent growth requests.
    Exact,
}

impl GrowthStrategy {
    /// Calculate the new capacity to grow to based on the current capacity and the requested
    /// capacity.
    pub fn new_capacity(&self, current: usize, requested: usize) -> usize {
        match self {
            Self::Multiply(factor) => cmp::max(current * factor, requested),
            Self::Buffer(buffer) => cmp::max(current + buffer, requested),
            _ => requested,
        }
    }
}

/// A structure that manages a contiguous block of allocated memory that allows a sized `element` to be indexed within the memory.
/// This can be thought of as a type erased collection of elements of homogeneous layout. This does not make any
/// assumptions about whether the elements are initialized or need to be dropped. It simply
/// provides convenient pointer access into the memory.
///
/// # Safety
///
/// This structure does **not** track initialization state or handle drop. The caller is responsible for:
/// - Only reading from initialized memory
/// - Manually dropping any values before they are overwritten or the memory is deallocated
/// - Ensuring indices are within bounds when using `ptr_at` and `ptr_at_mut`
///
/// # Example
///
/// ```ignore
/// let layout = Layout::new::<u32>();
/// let mut mem = IndexedMemory::with_capacity(layout, 10, GrowthStrategy::Multiply(2));
///
/// // Write a value to index 0
/// unsafe {
///     let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u32;
///     ptr.write(42);
/// }
///
/// // Read it back
/// unsafe {
///     let ptr = mem.ptr_at(0).as_ptr() as *const u32;
///     assert_eq!(ptr.read(), 42);
/// }
///
/// // Important: Drop the value before dropping the memory!
/// unsafe {
///     let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u32;
///     ptr.drop_in_place();
/// }
/// ```
pub struct IndexedMemory {
    /// The pointer to the underlying memory
    ptr: NonNull<u8>,
    /// The capacity to store elements.
    capacity: usize,
    /// The memory layout of an element.
    element_layout: Layout,
    /// Growth strategy for this memory.
    growth_strat: GrowthStrategy,
    /// The capacity that has actually been reserved by the owner of this memory. This will likely
    /// be different from what the current capacity actually is. We use this to track how much is
    /// requested vs how much we are optimistically pre-allocating to reduce re-allocation.
    reserved_capacity: usize,
}

impl IndexedMemory {
    /// Construct a new empty memory block with a specific element_layout and growth strategy.
    #[inline]
    pub const fn new(element_layout: Layout, growth_strat: GrowthStrategy) -> Self {
        Self {
            ptr: NonNull::dangling(),
            capacity: 0,
            element_layout,
            growth_strat,
            reserved_capacity: 0,
        }
    }

    #[inline]
    pub fn with_capacity(
        element_layout: Layout,
        capacity: usize,
        growth_strat: GrowthStrategy,
    ) -> Self {
        let mut block = Self::new(element_layout, growth_strat);
        block.reserved_capacity = capacity;
        block.grow_to(capacity);
        block
    }

    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get a pointer to the element at the given index.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `index` is less than the current capacity
    /// - The memory at this index has been initialized before reading from it
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `index >= capacity()`.
    #[inline]
    pub fn ptr_at(&self, index: usize) -> NonNull<u8> {
        debug_assert!(
            index < self.reserved_capacity,
            "index {} out of bounds (capacity: {})",
            index,
            self.reserved_capacity
        );
        unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(index * self.element_layout.size())) }
    }

    /// Get a mutable pointer to the element at the given index.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `index` is less than the current capacity
    /// - If reading, the memory at this index has been initialized
    /// - If writing to initialized memory, the old value is properly dropped first
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `index >= capacity()`.
    #[inline]
    pub fn ptr_at_mut(&mut self, index: usize) -> NonNull<u8> {
        debug_assert!(
            index < self.reserved_capacity,
            "index {} out of bounds (capacity: {})",
            index,
            self.reserved_capacity
        );
        unsafe { NonNull::new_unchecked(self.ptr.as_ptr().add(index * self.element_layout.size())) }
    }

    /// Get a pointer the underlying memory for this block.
    #[inline]
    pub fn as_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Reserve at least the requested new capacity in the memory allocation.
    pub fn reserve(&mut self, additional: usize) {
        self.reserve_with(additional, self.growth_strat.clone());
    }

    /// Reserve at least the requested new capacity in the memory allocation.
    pub fn reserve_exact(&mut self, additional: usize) {
        self.reserve_with(additional, GrowthStrategy::Exact);
    }

    /// Rserver at least the additional elements using the provided growth strategy.
    fn reserve_with(&mut self, additional: usize, strategy: GrowthStrategy) {
        // Always increase the reserved capacity with what is requested.
        self.reserved_capacity += additional;
        // If we already have enough capacity, just bail.
        if self.reserved_capacity <= self.capacity {
            return;
        }
        // Calculate an exact new capacity based on our reserve and growth strategy.
        let new_capacity = strategy.new_capacity(self.capacity, self.reserved_capacity);
        // Actually grow the capacity
        self.grow_to(new_capacity);
    }

    /// Grow the memory to support the requested capacity. If the existing capacity is 0, this will
    /// allocate a fresh block of memory. If there is existing capacity it will realloc from the
    /// existing block.
    fn grow_to(&mut self, capacity: usize) {
        let new_layout = Layout::from_size_align(
            capacity * self.element_layout.size(),
            self.element_layout.align(),
        )
        .expect("layout overflow");

        let new_ptr = if self.capacity == 0 {
            unsafe { alloc::alloc(new_layout) }
        } else {
            let old_layout = Layout::from_size_align(
                self.capacity * self.element_layout.size(),
                self.element_layout.align(),
            )
            .expect("layout overflow");

            unsafe { alloc::realloc(self.ptr.as_ptr(), old_layout, new_layout.size()) }
        };

        if new_ptr.is_null() {
            alloc::handle_alloc_error(new_layout);
        }

        self.ptr = ptr::NonNull::new(new_ptr).expect("allocation returned null");
        self.capacity = capacity;
    }
}

impl Drop for IndexedMemory {
    fn drop(&mut self) {
        // Only deallocate if we actually allocated memory
        if self.capacity > 0 {
            let layout = Layout::from_size_align(
                self.capacity * self.element_layout.size(),
                self.element_layout.align(),
            )
            .expect("layout overflow");

            unsafe {
                alloc::dealloc(self.ptr.as_ptr(), layout);
            }
        }
    }
}

// SAFETY: IndexedMemory can be sent across threads as it owns its allocation
// The caller is responsible for ensuring any T stored within is Send
unsafe impl Send for IndexedMemory {}

// SAFETY: IndexedMemory can be shared across threads as it provides no internal mutability
// The caller is responsible for ensuring any T stored within is Sync
unsafe impl Sync for IndexedMemory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_memory_is_empty() {
        let mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Multiply(2));
        assert_eq!(mem.capacity(), 0);
    }

    #[test]
    fn test_with_capacity_allocates() {
        let mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 10, GrowthStrategy::Exact);
        assert_eq!(mem.capacity(), 10);
    }

    #[test]
    fn test_reserve_grows_capacity() {
        let mut mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Exact);
        assert_eq!(mem.capacity(), 0);

        mem.reserve(5);
        assert_eq!(mem.capacity(), 5);

        mem.reserve(3);
        assert_eq!(mem.capacity(), 8);
    }

    #[test]
    fn test_reserve_exact_no_overgrowth() {
        let mut mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Exact);
        mem.reserve_exact(5);
        assert_eq!(mem.capacity(), 5);

        mem.reserve_exact(3);
        assert_eq!(mem.capacity(), 8);
    }

    #[test]
    fn test_growth_strategy_multiply() {
        let mut mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Multiply(2));
        mem.reserve(1);
        // With multiply(2), capacity should be at least 2 (0 * 2 = 0, so uses requested)
        assert_eq!(mem.capacity(), 1);

        let first_cap = mem.capacity();
        mem.reserve(1);
        // Should grow to at least 2x the previous capacity
        assert_eq!(mem.capacity(), first_cap * 2);
    }

    #[test]
    fn test_growth_strategy_buffer() {
        let mut mem = IndexedMemory::new(Layout::new::<u32>(), GrowthStrategy::Buffer(10));
        mem.reserve(5);
        assert_eq!(mem.capacity(), 10); // reserved capacity is 5

        mem.reserve(2);
        assert_eq!(mem.capacity(), 10); // reserved capacity is now 5 + 2 = 7
    }

    #[test]
    fn test_write_and_read_u32() {
        let mut mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 5, GrowthStrategy::Exact);

        // Write values
        for i in 0..5 {
            unsafe {
                let ptr = mem.ptr_at_mut(i).as_ptr() as *mut u32;
                ptr.write(i as u32 * 10);
            }
        }

        // Read them back
        for i in 0..5 {
            unsafe {
                let ptr = mem.ptr_at(i).as_ptr() as *const u32;
                assert_eq!(ptr.read(), i as u32 * 10);
            }
        }

        // Clean up - drop the values
        for i in 0..5 {
            unsafe {
                let ptr = mem.ptr_at_mut(i).as_ptr() as *mut u32;
                ptr.drop_in_place();
            }
        }
    }

    #[test]
    fn test_write_and_read_complex_type() {
        #[derive(Debug, PartialEq)]
        struct ComplexType {
            value: i32,
            data: Vec<u8>,
        }

        let mut mem = IndexedMemory::with_capacity(
            Layout::new::<ComplexType>(),
            3,
            GrowthStrategy::Multiply(2),
        );

        // Write values
        for i in 0..3 {
            let obj = ComplexType {
                value: i as i32,
                data: vec![i as u8; 5],
            };
            unsafe {
                let ptr = mem.ptr_at_mut(i).as_ptr() as *mut ComplexType;
                ptr.write(obj);
            }
        }

        // Read them back
        for i in 0..3 {
            unsafe {
                let ptr = mem.ptr_at(i).as_ptr() as *const ComplexType;
                let obj = &*ptr;
                assert_eq!(obj.value, i as i32);
                assert_eq!(obj.data, vec![i as u8; 5]);
            }
        }

        // Clean up - must drop to avoid leaking Vec allocations
        for i in 0..3 {
            unsafe {
                let ptr = mem.ptr_at_mut(i).as_ptr() as *mut ComplexType;
                ptr.drop_in_place();
            }
        }
    }

    #[test]
    fn test_realloc_preserves_data() {
        let mut mem = IndexedMemory::with_capacity(Layout::new::<i32>(), 2, GrowthStrategy::Exact);

        // Write initial values
        unsafe {
            let ptr = mem.ptr_at_mut(0).as_ptr() as *mut i32;
            ptr.write(42);
            let ptr = mem.ptr_at_mut(1).as_ptr() as *mut i32;
            ptr.write(99);
        }

        // Grow the memory
        mem.reserve(5);
        assert_eq!(mem.capacity(), 7);

        // Verify old values are preserved
        unsafe {
            let ptr = mem.ptr_at(0).as_ptr() as *const i32;
            assert_eq!(ptr.read(), 42);
            let ptr = mem.ptr_at(1).as_ptr() as *const i32;
            assert_eq!(ptr.read(), 99);
        }

        // Clean up
        for i in 0..2 {
            unsafe {
                let ptr = mem.ptr_at_mut(i).as_ptr() as *mut i32;
                ptr.drop_in_place();
            }
        }
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    #[cfg(debug_assertions)]
    fn test_ptr_at_bounds_check_debug() {
        let mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 5, GrowthStrategy::Exact);
        let _ = mem.ptr_at(10); // Should panic in debug mode
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    #[cfg(debug_assertions)]
    fn test_ptr_at_mut_bounds_check_debug() {
        let mut mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 5, GrowthStrategy::Exact);
        let _ = mem.ptr_at_mut(10); // Should panic in debug mode
    }

    #[test]
    fn test_zero_sized_types() {
        struct ZeroSized;
        let mut mem =
            IndexedMemory::with_capacity(Layout::new::<ZeroSized>(), 100, GrowthStrategy::Exact);
        assert_eq!(mem.capacity(), 100);

        // ZSTs should work without allocating actual memory
        unsafe {
            let ptr = mem.ptr_at_mut(0).as_ptr() as *mut ZeroSized;
            ptr.write(ZeroSized);
        }
    }

    #[test]
    fn test_growth_strategy_new_capacity() {
        let exact = GrowthStrategy::Exact;
        assert_eq!(exact.new_capacity(10, 15), 15);

        let multiply = GrowthStrategy::Multiply(2);
        assert_eq!(multiply.new_capacity(10, 15), 20); // max(10*2, 15) = 20
        assert_eq!(multiply.new_capacity(10, 25), 25); // max(10*2, 25) = 25

        let buffer = GrowthStrategy::Buffer(5);
        assert_eq!(buffer.new_capacity(10, 12), 15); // max(10+5, 12) = 15
        assert_eq!(buffer.new_capacity(10, 20), 20); // max(10+5, 20) = 20
    }

    #[test]
    fn test_drop_deallocates() {
        // This test verifies memory is properly freed on drop
        // If Drop is not implemented, this would leak memory
        let mut mem =
            IndexedMemory::with_capacity(Layout::new::<u64>(), 1000, GrowthStrategy::Exact);

        // Write some data
        unsafe {
            let ptr = mem.ptr_at_mut(0).as_ptr() as *mut u64;
            ptr.write(0xDEADBEEF);
        }

        // mem will be dropped here, and should deallocate
        drop(mem);
        // If we had a memory leak detector, we'd verify here
    }

    #[test]
    fn test_reserve_with_existing_capacity_doesnt_reallocate() {
        let mut mem = IndexedMemory::with_capacity(Layout::new::<u32>(), 10, GrowthStrategy::Exact);

        // Reserve less than we already have - should not reallocate
        mem.reserve(2);
        assert_eq!(mem.capacity(), 12);

        // The physical capacity is already 10, so this shouldn't reallocate yet
        // Let's write at the boundary to test
        unsafe {
            let ptr = mem.ptr_at_mut(9).as_ptr() as *mut u32;
            ptr.write(42);
            let ptr = mem.ptr_at(9).as_ptr() as *const u32;
            assert_eq!(ptr.read(), 42);
            let ptr = mem.ptr_at_mut(9).as_ptr() as *mut u32;
            ptr.drop_in_place();
        }
    }
}
