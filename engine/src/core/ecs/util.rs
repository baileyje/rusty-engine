use std::convert::From;
use std::{marker::PhantomData, ptr::NonNull};

/// A type-erased pointer to an element in a column.
#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Ptr<'a>(NonNull<u8>, PhantomData<&'a u8>);

impl<'a> Ptr<'a> {
    pub fn new(inner: NonNull<u8>) -> Self {
        Self(inner, PhantomData)
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.0.as_ptr()
    }

    // Dereference the pointer into type `T`.
    //
    // # Safety
    // - The pointer must point to data of type `T`
    // - The pointer must be aligned for type `T`
    #[inline]
    pub unsafe fn deref<T>(self) -> &'a T {
        let ptr = self.as_ptr().cast::<T>();
        // SAFETY: The caller ensures the pointee is of type `T` and the pointer can be dereferenced.
        unsafe { &*ptr }
    }
}

impl<'a, T: ?Sized> From<&'a T> for Ptr<'a> {
    #[inline]
    fn from(val: &'a T) -> Self {
        Self::new(NonNull::from(val).cast())
    }
}

/// A mutable type-erased pointer to an element in a column.
#[repr(transparent)]
pub struct MutPtr<'a>(NonNull<u8>, PhantomData<&'a mut u8>);

impl<'a> MutPtr<'a> {
    pub fn new(inner: NonNull<u8>) -> Self {
        Self(inner, PhantomData)
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.0.as_ptr()
    }

    // Dereference the pointer into type `T`.
    //
    // # Safety
    // - The pointer must point to data of type `T`
    // - The pointer must be aligned for type `T`
    #[inline]
    pub unsafe fn deref_mut<T>(self) -> &'a mut T {
        let ptr = self.as_ptr().cast::<T>();
        // SAFETY: The caller ensures the pointee is of type `T` and the pointer can be dereferenced.
        unsafe { &mut *ptr }
    }
}

impl<'a, T: ?Sized> From<&'a mut T> for MutPtr<'a> {
    #[inline]
    fn from(val: &'a mut T) -> Self {
        Self::new(NonNull::from(val).cast())
    }
}
