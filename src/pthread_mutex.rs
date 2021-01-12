use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::{
    alloc::{handle_alloc_error, GlobalAlloc, Layout},
    mem::MaybeUninit,
    ptr,
};

pub(crate) struct PthreadMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    inner: *mut libc::pthread_mutex_t,
    value: UnsafeCell<T>,
    allocator: &'static A,
}

impl<A, T> Drop for PthreadMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        unsafe {
            let retcode = libc::pthread_mutex_destroy(self.inner);
            assert_eq!(retcode, 0);

            let layout = Layout::new::<libc::pthread_mutex_t>();
            self.allocator.dealloc(self.inner.cast(), layout);
        }
    }
}

impl<A, T> PthreadMutex<A, T>
where
    A: 'static + GlobalAlloc,
{
    pub fn new(allocator: &'static A, value: T) -> Self {
        let layout = Layout::new::<libc::pthread_mutex_t>();
        let inner: *mut libc::pthread_mutex_t = unsafe { allocator.alloc(layout).cast() };
        if inner.is_null() {
            handle_alloc_error(layout);
        }

        unsafe {
            ptr::write(inner, libc::PTHREAD_MUTEX_INITIALIZER);
        }

        // Ensure the mutex is of type PTHREAD_MUTEX_NORMAL so that we
        // deadlock on re-entrancy, rather than trigger undefined behavior.
        //
        // For details, see the comment inside `std`:
        // https://github.com/rust-lang/rust/blob/c9b52100/library/std/src/sys/unix/mutex.rs#L30-L59
        unsafe {
            let mut attr = MaybeUninit::<libc::pthread_mutexattr_t>::uninit();
            let attr_ptr = attr.as_mut_ptr();

            let retcode = libc::pthread_mutexattr_init(attr_ptr);
            assert_eq!(retcode, 0);

            let retcode = libc::pthread_mutexattr_settype(attr_ptr, libc::PTHREAD_MUTEX_NORMAL);
            assert_eq!(retcode, 0);

            let retcode = libc::pthread_mutex_init(inner, attr_ptr);
            assert_eq!(retcode, 0);
        }

        PthreadMutex {
            inner,
            value: UnsafeCell::new(value),
            allocator,
        }
    }

    pub fn lock(&self) -> PthreadLockGuard<A, T> {
        let retcode = unsafe { libc::pthread_mutex_lock(self.inner) };
        assert_eq!(retcode, 0);
        PthreadLockGuard { mutex: self }
    }
}

pub struct PthreadLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    mutex: &'a PthreadMutex<A, T>,
}

impl<'a, A, T> Drop for PthreadLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        let retcode = unsafe { libc::pthread_mutex_unlock(self.mutex.inner) };
        assert_eq!(retcode, 0);
    }
}

impl<'a, A, T> Deref for PthreadLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<'a, A, T> DerefMut for PthreadLockGuard<'a, A, T>
where
    A: 'static + GlobalAlloc,
{
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}
