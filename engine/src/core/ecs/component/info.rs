use std::alloc::Layout;
use std::any::TypeId;
use std::ptr::NonNull;
use std::{mem, ptr};

use crate::core::ecs::component::{Component, Id};

/// Information about a registered component.
#[derive(Debug, Clone, Copy)]
pub struct Info {
    /// The unique component ID.
    id: Id,

    /// The TypeId of the component.
    type_id: TypeId,

    /// The memory layout of the component.
    layout: Layout,

    // The drop function for the component, might be a no-op.
    drop_fn: unsafe fn(NonNull<u8>),
}

impl Info {
    /// Construct Component Info for type `C`. This will use type `C` to determine the component's
    /// memory layout and if the component needs drops a drop function.
    pub fn new<C: Component>(id: Id) -> Self {
        let drop_fn = if mem::needs_drop::<C>() {
            Self::drop_impl::<C>
        } else {
            Self::drop_noop
        };
        Self {
            id,
            type_id: TypeId::of::<C>(),
            layout: Layout::new::<C>(),
            drop_fn,
        }
    }

    /// Get the component ID for this type.
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Get the TypeId for this type.
    #[inline]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get the memory layout for this type.
    #[inline]
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Determine if this component is zero-sized type.
    #[inline]
    pub fn is_zero_sized(&self) -> bool {
        self.layout.size() == 0
    }

    #[inline]
    pub fn drop_fn(&self) -> unsafe fn(NonNull<u8>) {
        self.drop_fn
    }

    /// Drop implementation for types that need drop.
    unsafe fn drop_impl<C>(ptr: NonNull<u8>) {
        // SAFETY: Caller ensures ptr points to a valid initialized T
        unsafe {
            ptr::drop_in_place(ptr.as_ptr() as *mut C);
        }
    }

    /// No-op drop for types that don't need drop.
    unsafe fn drop_noop(_ptr: NonNull<u8>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_info_basic_properties() {
        // Given
        #[derive(Component)]
        struct TestComponent {
            #[allow(dead_code)]
            value: u32,
        }

        // When
        let info = Info::new::<TestComponent>(Id(42));

        // Then
        assert_eq!(info.id(), Id(42));
        assert_eq!(info.type_id(), TypeId::of::<TestComponent>());
        assert_eq!(info.layout(), Layout::new::<TestComponent>());
    }

    #[test]
    fn test_info_needs_drop() {
        // Given - type that needs drop
        #[derive(Component)]
        struct NeedsDrop {
            _data: Vec<u32>,
        }

        // Given - type that doesn't need drop
        #[derive(Component)]
        struct NoDrop {
            #[allow(dead_code)]
            value: u32,
        }

        // When
        let needs_drop_info = Info::new::<NeedsDrop>(Id(0));
        let no_drop_info = Info::new::<NoDrop>(Id(1));

        // Then - both should have drop functions, but different ones
        // We can't easily test the function pointers directly, but we can verify they exist
        let _ = needs_drop_info.drop_fn();
        let _ = no_drop_info.drop_fn();
    }

    #[test]
    fn test_info_drop_is_called() {
        // Given
        static DROP_CALLED: AtomicBool = AtomicBool::new(false);

        struct DropTracker {
            value: u32,
        }

        impl Drop for DropTracker {
            fn drop(&mut self) {
                DROP_CALLED.store(true, Ordering::Relaxed);
            }
        }

        impl Component for DropTracker {}

        // When
        let info = Info::new::<DropTracker>(Id(0));

        // Allocate and initialize a DropTracker
        let layout = Layout::new::<DropTracker>();
        let ptr = unsafe { std::alloc::alloc(layout) };
        assert!(!ptr.is_null());

        let ptr = NonNull::new(ptr).unwrap();
        unsafe {
            std::ptr::write(ptr.as_ptr() as *mut DropTracker, DropTracker { value: 42 });
        }

        // Call the drop function
        unsafe {
            (info.drop_fn())(ptr);
        }

        // Deallocate
        unsafe {
            std::alloc::dealloc(ptr.as_ptr(), layout);
        }

        // Then
        assert!(DROP_CALLED.load(Ordering::Relaxed));
    }

    #[test]
    fn test_info_noop_drop() {
        // Given - type that doesn't need drop
        #[derive(Component)]
        struct Simple {
            #[allow(dead_code)]
            value: u32,
        }

        // When
        let info = Info::new::<Simple>(Id(0));

        // Allocate and initialize
        let layout = Layout::new::<Simple>();
        let ptr = unsafe { std::alloc::alloc(layout) };
        assert!(!ptr.is_null());

        let ptr = NonNull::new(ptr).unwrap();
        unsafe {
            std::ptr::write(ptr.as_ptr() as *mut Simple, Simple { value: 42 });
        }

        // Call the drop function (should be no-op)
        unsafe {
            (info.drop_fn())(ptr);
        }

        // Deallocate
        unsafe {
            std::alloc::dealloc(ptr.as_ptr(), layout);
        }

        // Then - if we get here without panicking, the no-op drop worked
    }

    #[test]
    fn info_is_zero_sized() {
        // Given
        #[derive(Component)]
        struct Component;

        let info = Info::new::<Component>(Id(0));

        // Then
        assert!(info.is_zero_sized());
    }
}
