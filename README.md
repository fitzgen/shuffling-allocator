# `shuffling-allocator`

A shuffling allocator.

This crate provides the `ShufflingAllocator` type, which wraps an existing
allocator and shuffles the order of heap allocations it yields, effectively
randomizing the placement of heap allocations.

Randomizing the locations of heap allocations is useful for testing,
benchmarking, and performance evaluation. It helps you separate the
performance effects of a given code change from accidental heap object
locality and the effects this may have on performance due to memory caches
in the CPU. This is the use case that this crate focuses on.

While randomizing the locations of heap allocations can also be used for
defense-in-depth security, similar to ASLR, this crate is not written to
support that use case. As a result, this crate may not be the right choice
if your use case is the defense-in-depth security use case. Some trade offs
and design decisions made in this crate's implementation might not be the
choices you want for your use case.

This crate is inspired by the allocator described in [*Stabilizer:
Statistically Sound Performance Evaluation* by Curtsinger and
Berger](https://people.cs.umass.edu/~emery/pubs/stabilizer-asplos13.pdf ).

## How Does It Work?

An array of available objects for each size class is always
maintained. Allocating a new object involves making the allocation, choosing
a random index in the array, swapping the new allocation for `array[i]` and
returning the swapped out value. Freeing an object is similar: choose a
random index in the array, swap the pointer being freed with `array[i]`, and
then use the underlying allocator to actually free the swapped out
pointer. The larger the array in the shuffling layer, the closer to truly
randomized heap allocations we get, but also the greater the
overhead. Curtsinger and Berger found that arrays of size 256 gave good
randomization for acceptable overhead, and that is also the array size that
this crate uses.

## Example

Wrap the system allocator in a `ShufflingAllocator`, randomizing the
location of the system allocator's heap objects:

```rust
use shuffling_allocator::ShufflingAllocator;
use std::alloc::System;

static SHUFFLED_SYSTEM_ALLOC: ShufflingAllocator<System> =
    shuffling_allocator::wrap!(&System);
```
