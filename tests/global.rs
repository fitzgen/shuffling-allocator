use shuffling_allocator::ShufflingAllocator;
use std::alloc::System;
use std::collections::HashMap;
use std::thread;

#[global_allocator]
static A: ShufflingAllocator<System> = shuffling_allocator::wrap!(&System);

#[test]
fn foo() {
    println!("hello");
}

#[test]
fn map() {
    let mut m = HashMap::new();
    m.insert(1, 2);
    m.insert(5, 3);
    drop(m);
}

#[test]
fn strings() {
    format!("foo, bar, {}", "baz");
}

#[test]
fn threads() {
    assert!(thread::spawn(|| panic!()).join().is_err());
}

#[test]
fn test_larger_than_word_alignment() {
    use std::mem;

    // Align to 32 bytes.
    #[repr(align(32))]
    struct Align32(u8);

    assert_eq!(mem::align_of::<Align32>(), 32);

    for _ in 0..100 {
        let b = Box::new(Align32(42));

        let p = Box::into_raw(b);
        assert_eq!(p as usize % 32, 0, "{:p} should be aligned to 32", p);

        unsafe {
            let b = Box::from_raw(p);
            assert_eq!(b.0, 42);
        }
    }
}

#[test]
fn many_small_allocs() {
    let boxes = (0..1024).map(|i| Box::new(i)).collect::<Vec<_>>();
    drop(boxes);
}
