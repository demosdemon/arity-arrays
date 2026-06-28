//! Shared bench support: payload trait and index helper, used by both the
//! `throughput` benchmark and the `bench_internals` integration test (each
//! `#[path]`-includes this file as a module). It depends only on the crate's
//! public API, so the integration test can exercise it directly.

use arity_arrays::Arity;
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
