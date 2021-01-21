//! A shuffling allocator.
//!
//! This crate provides the `ShufflingAllocator` type, which wraps an existing
//! allocator and shuffles the order of heap allocations it yields, effectively
//! randomizing the placement of heap allocations.
//!
//! Randomizing the locations of heap allocations is useful for testing,
//! benchmarking, and performance evaluation. It helps you separate the
//! performance effects of a given code change from accidental heap object
//! locality and the effects this may have on performance due to memory caches
//! in the CPU. This is the use case that this crate focuses on.
//!
//! While randomizing the locations of heap allocations can also be used for
//! defense-in-depth security, similar to ASLR, this crate is not written to
//! support that use case. As a result, this crate may not be the right choice
//! if your use case is the defense-in-depth security use case. Some trade offs
//! and design decisions made in this crate's implementation might not be the
//! choices you want for your use case.
//!
//! This crate is inspired by the allocator described in [*Stabilizer:
//! Statistically Sound Performance Evaluation* by Curtsinger and
//! Berger](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf ).
//!
//! # How Does It Work?
//!
//! An array of available objects for each size class is always
//! maintained. Allocating a new object involves making the allocation, choosing
//! a random index in the array, swapping the new allocation for `array[i]` and
//! returning the swapped out value. Freeing an object is similar: choose a
//! random index in the array, swap the pointer being freed with `array[i]`, and
//! then use the underlying allocator to actually free the swapped out
//! pointer. The larger the array in the shuffling layer, the closer to truly
//! randomized heap allocations we get, but also the greater the
//! overhead. Curtsinger and Berger found that arrays of size 256 gave good
//! randomization for acceptable overhead, and that is also the array size that
//! this crate uses.
//!
//! # Example
//!
//! Wrap the system allocator in a `ShufflingAllocator`, randomizing the
//! location of the system allocator's heap objects:
//!
//! ```
//! use shuffling_allocator::ShufflingAllocator;
//! use std::alloc::System;
//!
//! static SHUFFLED_SYSTEM_ALLOC: ShufflingAllocator<System> =
//!     shuffling_allocator::wrap!(&System);
//! ```

#![deny(missing_docs)]

mod lazy_atomic_cell;

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        mod pthread_mutex;
        use pthread_mutex::PthreadMutex as Mutex;
    } else if #[cfg(windows)] {
        mod windows_mutex;
        use windows_mutex::WindowsMutex as Mutex;
    } else {
        compile_error!("no mutex implementation for this platform");
    }
}

// This is only public because we can't use type parameters in `const fn` yet,
// so we can't implement `const fn ShufflingAllocator::new`. Don't use this!
#[doc(hidden)]
pub use lazy_atomic_cell::LazyAtomicCell;

use mem::MaybeUninit;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::{
    alloc::{handle_alloc_error, GlobalAlloc, Layout},
    mem, ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

const SHUFFLING_ARRAY_SIZE: usize = 256;

struct ShufflingArray<A>
where
    A: 'static + GlobalAlloc,
{
    elems: [AtomicPtr<u8>; SHUFFLING_ARRAY_SIZE],
    size_class: usize,
    allocator: &'static A,
}

impl<A> Drop for ShufflingArray<A>
where
    A: 'static + GlobalAlloc,
{
    fn drop(&mut self) {
        let layout =
            unsafe { Layout::from_size_align_unchecked(self.size_class, mem::align_of::<usize>()) };
        for el in &self.elems {
            let p = el.swap(ptr::null_mut(), Ordering::SeqCst);
            if !p.is_null() {
                unsafe {
                    self.allocator.dealloc(p, layout);
                }
            }
        }
    }
}

impl<A> ShufflingArray<A>
where
    A: 'static + GlobalAlloc,
{
    fn new(size_class: usize, allocator: &'static A) -> Self {
        let elems = unsafe {
            let mut elems = MaybeUninit::<[AtomicPtr<u8>; 256]>::uninit();
            let elems_ptr: *mut [AtomicPtr<u8>; 256] = elems.as_mut_ptr();
            let elems_ptr: *mut AtomicPtr<u8> = elems_ptr.cast();
            let layout = Layout::from_size_align_unchecked(size_class, mem::align_of::<usize>());
            for i in 0..256 {
                let p = allocator.alloc(layout);
                if p.is_null() {
                    handle_alloc_error(layout);
                }
                ptr::write(elems_ptr.offset(i), AtomicPtr::new(p));
            }
            elems.assume_init()
        };
        ShufflingArray {
            elems,
            size_class,
            allocator,
        }
    }

    /// Get the layout for this size class, aka the layout for elements within
    /// this shuffing array.
    fn elem_layout(&self) -> Layout {
        unsafe {
            debug_assert!(
                Layout::from_size_align(self.size_class, mem::align_of::<usize>()).is_ok()
            );
            Layout::from_size_align_unchecked(self.size_class, mem::align_of::<usize>())
        }
    }
}

struct SizeClasses<A>([LazyAtomicCell<A, ShufflingArray<A>>; NUM_SIZE_CLASSES])
where
    A: 'static + GlobalAlloc;

struct SizeClassInfo {
    index: usize,
    size_class: usize,
}

#[rustfmt::skip]
#[inline]
fn size_class_info(size: usize) -> Option<SizeClassInfo> {
    let mut size_class = mem::size_of::<usize>();
    let mut stride = mem::size_of::<usize>();

    if size <= size_class {
        return Some(SizeClassInfo { index: 0, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 1, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 2, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 3, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 4, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 5, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 6, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 7, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 8, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 9, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 10, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 11, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 12, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 13, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 14, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 15, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 16, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 17, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 18, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 19, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 20, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 21, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 22, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 23, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 24, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 25, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 26, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 27, size_class });
    }
    size_class += stride;

    stride = stride * 2;

    if size <= size_class {
        return Some(SizeClassInfo { index: 28, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 29, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 30, size_class });
    }
    size_class += stride;
    if size <= size_class {
        return Some(SizeClassInfo { index: 31, size_class });
    }

    None
}

const NUM_SIZE_CLASSES: usize = 32;

/// A shuffling allocator.
///
/// Wraps an existing allocator and shuffles the order of heap allocations
/// yielded.
///
/// See [the crate-level documentation](./index.html) for more details.
///
/// # Example
///
/// ```
/// use shuffling_allocator::ShufflingAllocator;
/// use std::alloc::System;
///
/// static SHUFFLED_SYSTEM_ALLOC: ShufflingAllocator<System> = shuffling_allocator::wrap!(&System);
/// ```
pub struct ShufflingAllocator<A>
where
    A: 'static + GlobalAlloc,
{
    // XXX: You shouldn't touch these fields! They need to be `pub` so that
    // `wrap!` works, but as soon as `const fn`s can have type parameters, these
    // will be private.
    #[doc(hidden)]
    pub inner: &'static A,
    #[doc(hidden)]
    pub state: LazyAtomicCell<A, State<A>>,
}

#[doc(hidden)]
pub struct State<A>
where
    A: 'static + GlobalAlloc,
{
    rng: Mutex<A, StdRng>,
    size_classes: LazyAtomicCell<A, SizeClasses<A>>,
}

/// Wrap shuffling around an existing global allocator.
///
/// # Example
///
/// ```
/// use shuffling_allocator::ShufflingAllocator;
/// use std::alloc::System;
///
/// static SHUFFLED_SYSTEM_ALLOC: ShufflingAllocator<System> = shuffling_allocator::wrap!(&System);
/// ```
#[macro_export]
macro_rules! wrap {
    ($inner:expr) => {
        $crate::ShufflingAllocator {
            inner: $inner,
            state: $crate::LazyAtomicCell {
                ptr: ::std::sync::atomic::AtomicPtr::new(::std::ptr::null_mut()),
                allocator: $inner,
            },
        }
    };
}

impl<A> ShufflingAllocator<A>
where
    A: 'static + GlobalAlloc,
{
    // XXX: this is disabled until we can have `const fn`s with type parameters.
    //
    // pub const fn new(inner: &'static A) -> Self {
    //     ShufflingAllocator {
    //         inner,
    //         state: LazyAtomicCell::new(inner),
    //     }
    // }

    #[inline]
    fn state(&self) -> &State<A> {
        self.state.get_or_create(|| State {
            rng: Mutex::new(&self.inner, StdRng::from_entropy()),
            size_classes: LazyAtomicCell::new(self.inner),
        })
    }

    #[inline]
    fn random_index(&self) -> usize {
        let mut rng = self.state().rng.lock();
        rng.gen_range(0..SHUFFLING_ARRAY_SIZE)
    }

    #[inline]
    fn size_classes(&self) -> &SizeClasses<A> {
        self.state().size_classes.get_or_create(|| {
            let mut classes =
                MaybeUninit::<[LazyAtomicCell<A, ShufflingArray<A>>; NUM_SIZE_CLASSES]>::uninit();
            unsafe {
                for i in 0..NUM_SIZE_CLASSES {
                    ptr::write(
                        classes
                            .as_mut_ptr()
                            .cast::<LazyAtomicCell<A, ShufflingArray<A>>>()
                            .offset(i as _),
                        LazyAtomicCell::new(self.inner),
                    );
                }
                SizeClasses(classes.assume_init())
            }
        })
    }

    #[inline]
    fn shuffling_array(&self, size: usize) -> Option<&ShufflingArray<A>> {
        let SizeClassInfo { index, size_class } = size_class_info(size)?;
        let size_classes = self.size_classes();
        Some(size_classes.0[index].get_or_create(|| ShufflingArray::new(size_class, self.inner)))
    }
}

unsafe impl<A> GlobalAlloc for ShufflingAllocator<A>
where
    A: GlobalAlloc,
{
    #[inline]
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // We only support shuffling reasonably aligned allocations.
        if layout.align() > mem::align_of::<usize>() {
            return self.inner.alloc(layout);
        }

        match self.shuffling_array(layout.size()) {
            // We don't have a shuffling array for this size (it must be fairly
            // big) so just use the inner allocator.
            None => self.inner.alloc(layout),

            // Choose a random entry from the shuffle array to return, refilling
            // the entry with a new pointer from the inner allocator.
            Some(array) => {
                let replacement_ptr = self.inner.alloc(array.elem_layout());
                if replacement_ptr.is_null() {
                    return ptr::null_mut();
                }

                let index = self.random_index();
                array.elems[index].swap(replacement_ptr, Ordering::SeqCst)
            }
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        if ptr.is_null() {
            return;
        }

        if layout.align() > mem::align_of::<usize>() {
            self.inner.dealloc(ptr, layout);
            return;
        }

        match self.shuffling_array(layout.size()) {
            // No size class for this layout, use the inner allocator directly.
            None => self.inner.dealloc(ptr, layout),

            // Choose a random entry in the shuffle array to swap this pointer
            // with, and then deallocate the old entry.
            Some(array) => {
                let index = self.random_index();
                let old_ptr = array.elems[index].swap(ptr, Ordering::SeqCst);
                self.inner.dealloc(old_ptr, array.elem_layout());
            }
        }
    }
}
