use std::{
    alloc::{handle_alloc_error, GlobalAlloc, Layout},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

#[doc(hidden)]
pub struct LazyAtomicCell<A, T>
where
    A: 'static + GlobalAlloc,
{
    #[doc(hidden)]
    pub ptr: AtomicPtr<T>,
    #[doc(hidden)]
    pub allocator: &'static A,
}

impl<A, T> Drop for LazyAtomicCell<A, T>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        let p = self.ptr.swap(ptr::null_mut(), Ordering::SeqCst);
        if p.is_null() {
            return;
        }
        unsafe {
            ptr::drop_in_place(p);
            self.allocator.dealloc(p.cast(), Layout::new::<T>());
        }
    }
}

impl<A, T> LazyAtomicCell<A, T>
where
    A: 'static + GlobalAlloc,
{
    /// Create a new `LazyAtomicCell`.
    pub fn new(allocator: &'static A) -> Self {
        LazyAtomicCell {
            ptr: AtomicPtr::new(ptr::null_mut()),
            allocator,
        }
    }

    /// Get the value if it already exists, or create it by calling `init`.
    pub fn get_or_create(&self, init: impl FnOnce() -> T) -> &T {
        let ptr = self.ptr.load(Ordering::SeqCst);
        if !ptr.is_null() {
            return unsafe { &*ptr };
        }

        // Allocate space for our `T`.
        let layout = Layout::new::<T>();
        let new_ptr = unsafe { self.allocator.alloc(layout).cast::<T>() };
        if new_ptr.is_null() {
            handle_alloc_error(layout);
        }

        // Initialize our `T`.
        unsafe {
            ptr::write(new_ptr, init());
        }

        // Attempt to initialize `self.ptr` with our newly allocated and
        // initialized `T`. We are racing against other threads to be the first
        // to initialize `self.ptr`.
        let existing_ptr = self
            .ptr
            .compare_and_swap(ptr::null_mut(), new_ptr, Ordering::SeqCst);
        if existing_ptr.is_null() {
            // We won the race!
            unsafe { &*new_ptr }
        } else {
            // We lost the race, so we have to remember to drop and deallocate
            // our now-unnecessary `State`.
            unsafe {
                ptr::drop_in_place(new_ptr);
                self.allocator.dealloc(new_ptr.cast(), layout);
                &*existing_ptr
            }
        }
    }
}
