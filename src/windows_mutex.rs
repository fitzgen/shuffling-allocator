use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::{
    alloc::{handle_alloc_error, GlobalAlloc, Layout},
    ptr,
};
use winapi::um::synchapi::{
    AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK, SRWLOCK_INIT,
};

pub(crate) struct WindowsMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    inner: *mut SRWLOCK,
    value: UnsafeCell<T>,
    allocator: &'static A,
}

impl<A, T> Drop for WindowsMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        unsafe {
            let layout = Layout::new::<SRWLOCK>();
            self.allocator.dealloc(self.inner.cast(), layout);
        }
    }
}

impl<A, T> WindowsMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    pub fn new(allocator: &'static A, value: T) -> Self {
        let layout = Layout::new::<SRWLOCK>();
        let inner: *mut SRWLOCK = unsafe { allocator.alloc(layout).cast() };
        if inner.is_null() {
            handle_alloc_error(layout);
        }

        unsafe {
            ptr::write(inner, SRWLOCK_INIT);
        }

        WindowsMutex {
            inner,
            value: UnsafeCell::new(value),
            allocator,
        }
    }

    pub fn lock(&self) -> WindowsLockGuard<A, T> {
        unsafe {
            AcquireSRWLockExclusive(self.inner);
        }
        WindowsLockGuard { mutex: self }
    }
}

pub struct WindowsLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    mutex: &'a WindowsMutex<A, T>,
}

impl<'a, A, T> Drop for WindowsLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        unsafe {
            ReleaseSRWLockExclusive(self.mutex.inner);
        }
    }
}

impl<'a, A, T> Deref for WindowsLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<'a, A, T> DerefMut for WindowsLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}
