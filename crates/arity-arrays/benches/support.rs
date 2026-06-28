//! Shared bench support: payload trait and index helper, used by both the
//! `throughput` benchmark and the `bench_internals` integration test (each
//! `#[path]`-includes this file as a module). It depends only on the crate's
//! public API, so the integration test can exercise it directly.

use core::marker::PhantomData;
use std::collections::BTreeMap;
use std::collections::HashMap;

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;

/// Bench payload: a `Copy`, `Drop`-free element with a deterministic
/// constructor and a whole-element `u64` projection used as a dead-code
/// elimination barrier when iterating.
pub trait Payload: Copy {
    /// Deterministic value for fill index `i`.
    fn make(i: usize) -> Self;
    /// Fold the *entire* element into a `u64`. Reading only a prefix would let
    /// the compiler elide the rest, defeating the iteration benchmark.
    fn fold(&self) -> u64;
}

impl Payload for u64 {
    fn make(i: usize) -> Self {
        i as Self
    }
    fn fold(&self) -> u64 {
        *self
    }
}

impl Payload for [u8; 32] {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bench fill index is always < A::LEN <= 256, fitting u8"
    )]
    fn make(i: usize) -> Self {
        [i as u8; 32]
    }
    fn fold(&self) -> u64 {
        // XOR all four 8-byte chunks so every byte is observed.
        let mut acc = 0u64;
        let mut chunk = [0u8; 8];
        for c in self.chunks_exact(8) {
            chunk.copy_from_slice(c);
            acc ^= u64::from_ne_bytes(chunk);
        }
        acc
    }
}

/// Map a raw `usize` to a valid index for arity `A`. The mask is total for a
/// power-of-two width, so `try_from_usize` is always `Some`.
pub fn masked_index<A: Arity>(i: usize) -> A::Index {
    <A::Index as Niche>::try_from_usize(i & (A::LEN - 1))
        .expect("masked index is always < LEN for a power-of-two arity")
}

/// The operations every benched representation supports, keyed by a raw
/// `usize` slot the bench body computes via [`masked_index`]. Named to avoid
/// clashing with the inherent `get`/`insert`/`remove` methods on the concrete
/// types. The future capacity-tracking container implements this trait and is
/// appended to each bench's `types = [...]` list — the entire integration cost.
pub trait BenchContainer<T: Payload> {
    /// An empty container. (divan labels each generic instantiation by type, so
    /// no per-impl name constant is needed.)
    fn empty() -> Self;
    /// Slots `0..occupancy` present, each holding `T::make(slot)`.
    fn fill(occupancy: usize) -> Self
    where
        Self: Sized,
    {
        let mut c = Self::empty();
        for i in 0..occupancy {
            c.set(i, T::make(i));
        }
        c
    }
    /// Reference to the element at `index`, or `None`.
    fn lookup(&self, index: usize) -> Option<&T>;
    /// Insert/overwrite at `index`, returning the previous value if present.
    fn set(&mut self, index: usize, value: T) -> Option<T>;
    /// Remove at `index`, returning the previous value if present.
    fn del(&mut self, index: usize) -> Option<T>;
    /// XOR-fold every present element's whole-element projection.
    fn fold(&self) -> u64;
}

impl<T: Payload, A: Arity> BenchContainer<T> for PackedArray<T, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn lookup(&self, index: usize) -> Option<&T> {
        self.get(masked_index::<A>(index))
    }
    fn set(&mut self, index: usize, value: T) -> Option<T> {
        self.insert(masked_index::<A>(index), value)
    }
    fn del(&mut self, index: usize) -> Option<T> {
        self.remove(masked_index::<A>(index))
    }
    fn fold(&self) -> u64 {
        self.iter_present().fold(0, |acc, (_, v)| acc ^ v.fold())
    }
}

impl<T: Payload, A: Arity> BenchContainer<T> for FixedArray<Option<T>, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn lookup(&self, index: usize) -> Option<&T> {
        self.get(masked_index::<A>(index)).as_ref()
    }
    fn set(&mut self, index: usize, value: T) -> Option<T> {
        self.replace(masked_index::<A>(index), Some(value))
    }
    fn del(&mut self, index: usize) -> Option<T> {
        self.take(masked_index::<A>(index))
    }
    fn fold(&self) -> u64 {
        self.iter_present().fold(0, |acc, (_, v)| acc ^ v.fold())
    }
}

/// Full-width boxed slice baseline. Wraps `Box<[Option<T>]>` so the arity (and
/// thus the slice length `A::LEN`) is available to `empty`.
pub struct BoxArr<T, A: Arity>(Box<[Option<T>]>, PhantomData<A>);

impl<T: Payload, A: Arity> BenchContainer<T> for BoxArr<T, A> {
    fn empty() -> Self {
        let mut v: Vec<Option<T>> = Vec::with_capacity(A::LEN);
        v.resize_with(A::LEN, || None);
        Self(v.into_boxed_slice(), core::marker::PhantomData)
    }
    fn lookup(&self, index: usize) -> Option<&T> {
        self.0[index & (A::LEN - 1)].as_ref()
    }
    fn set(&mut self, index: usize, value: T) -> Option<T> {
        self.0[index & (A::LEN - 1)].replace(value)
    }
    fn del(&mut self, index: usize) -> Option<T> {
        self.0[index & (A::LEN - 1)].take()
    }
    fn fold(&self) -> u64 {
        self.0.iter().flatten().fold(0, |acc, v| acc ^ v.fold())
    }
}

impl<T: Payload> BenchContainer<T> for BTreeMap<usize, T> {
    fn empty() -> Self {
        Self::new()
    }
    fn lookup(&self, index: usize) -> Option<&T> {
        self.get(&index)
    }
    fn set(&mut self, index: usize, value: T) -> Option<T> {
        self.insert(index, value)
    }
    fn del(&mut self, index: usize) -> Option<T> {
        self.remove(&index)
    }
    fn fold(&self) -> u64 {
        self.values().fold(0, |acc, v| acc ^ v.fold())
    }
}

impl<T: Payload> BenchContainer<T> for HashMap<usize, T> {
    fn empty() -> Self {
        Self::new()
    }
    fn lookup(&self, index: usize) -> Option<&T> {
        self.get(&index)
    }
    fn set(&mut self, index: usize, value: T) -> Option<T> {
        self.insert(index, value)
    }
    fn del(&mut self, index: usize) -> Option<T> {
        self.remove(&index)
    }
    fn fold(&self) -> u64 {
        self.values().fold(0, |acc, v| acc ^ v.fold())
    }
}
