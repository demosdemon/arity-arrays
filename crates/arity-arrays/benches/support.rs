//! Shared bench support: payload trait and index helper, used by both the
//! `throughput` benchmark and the `bench_internals` integration test (each
//! `#[path]`-includes this file as a module). It depends only on the crate's
//! public API, so the integration test can exercise it directly.

use core::marker::PhantomData;
use std::collections::BTreeMap;
use std::collections::HashMap;

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
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
        for c in self.as_chunks::<8>().0 {
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
/// `usize` slot. Named to avoid clashing with the inherent
/// `get`/`insert`/`remove` methods on the concrete types. Bounded-width
/// representations mask the slot into `[0, A::LEN)` — the typed containers
/// (`PackedArray`, `GappedArray`, `FixedArray`) via [`masked_index`], `BoxArr`
/// inline — while map-backed implementations use the raw value directly. A new
/// representation implements this trait and adds its concrete types to each
/// `single_op_benches!` and `workload_benches!` invocation (two cells × two
/// macro families = four call sites); to also appear in the conversion table
/// (`mod convert` in `throughput.rs`) or the memory-report table
/// (`memory_table` in `tests/memory_report.rs`), it must be wired into those
/// call sites too.
pub trait BenchContainer<T: Payload> {
    /// Stable label for this representation, used as the criterion
    /// `BenchmarkId` subject and parsed back by the chart xtask. Must be
    /// unique per cell.
    const NAME: &'static str;
    /// An empty container.
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
    const NAME: &'static str = "PackedArray";
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

impl<T: Payload, A: Arity> BenchContainer<T> for GappedArray<T, A> {
    const NAME: &'static str = "GappedArray";
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
    const NAME: &'static str = "FixedArray";
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
    const NAME: &'static str = "BoxArr";
    fn empty() -> Self {
        let mut v: Vec<Option<T>> = Vec::with_capacity(A::LEN);
        v.resize_with(A::LEN, || None);
        Self(v.into_boxed_slice(), PhantomData)
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
    const NAME: &'static str = "BTreeMap";
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
    const NAME: &'static str = "HashMap";
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

/// A churn step: which mutation, and the slot it targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChurnOp {
    Remove,
    Insert,
}

/// Deterministic xorshift64 seed for the churn sequence. Fixed so the workload
/// — and therefore the committed baseline — is reproducible across runs.
const CHURN_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

const fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Churn sequence length for arity width `n`: `max(256, 8 × n)`. A single
/// definition so `churn_ops` and its test cannot drift — the length is part of
/// the before/after baseline contract for the capacity-tracking follow-up.
pub const fn churn_len(n: usize) -> usize {
    let scaled = n.saturating_mul(8);
    if scaled > 256 { scaled } else { 256 }
}

/// Deterministic seed for the randomized-index get benches. Distinct from
/// `CHURN_SEED` so the two access patterns do not correlate. Fixed so the
/// sequence — and any committed baseline — is reproducible across runs.
const RAND_SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// Number of slots in a randomized-access index sequence. Fixed so the access
/// pattern (and the committed baseline) is reproducible; a power of two so the
/// bench's `cursor & (RAND_SEQ_LEN - 1)` wrap is a cheap mask.
pub const RAND_SEQ_LEN: usize = 256;

/// A reproducible sequence of [`RAND_SEQ_LEN`] pseudo-random slots drawn from
/// the half-open range `[lo, hi)`, used by the randomized-index get benches so
/// each iteration reads an unpredictable slot (defeating the branch-predictor
/// and L1-cache bias of a fixed target). Seeded from a fixed constant, so the
/// draw — and the committed baseline — is reproducible.
///
/// Returns an array rather than a `Vec` so the length is a compile-time
/// constant. Paired with the bench's `cursor & (RAND_SEQ_LEN - 1)` index, that
/// lets the compiler prove every access in bounds and drop the check — and,
/// because the elements are inline rather than behind a `Vec`'s pointer, it
/// also removes a dependent load from the timed region. Both would otherwise
/// be charged to every iteration of a benchmark that exists to measure exactly
/// that class of cost.
///
/// # Panics
///
/// Panics if `hi <= lo` (the range must be non-empty).
pub fn rand_slots(lo: usize, hi: usize) -> [usize; RAND_SEQ_LEN] {
    assert!(hi > lo, "rand_slots needs a non-empty [lo, hi) range");
    let span = u64::try_from(hi - lo).expect("range width < usize::MAX fits u64");
    let mut state = RAND_SEED;
    let mut slots = [0usize; RAND_SEQ_LEN];
    for slot in &mut slots {
        let draw = usize::try_from(xorshift64(&mut state) % span)
            .expect("draw < span <= usize::MAX fits usize");
        *slot = lo + draw;
    }
    slots
}

/// Build the churn sequence for arity `A`: start from slots `0..N/2` present,
/// then alternate Remove(present)/Insert(absent) so occupancy oscillates ±1
/// around `N/2` and never reaches a boundary where a step would no-op. Length
/// is `max(256, 8 * N)`, a named constant of the before/after baseline.
pub fn churn_ops<A: Arity>() -> Vec<(ChurnOp, usize)> {
    let n = A::LEN;
    let len = churn_len(n);
    let mut occupied = vec![false; n];
    for slot in occupied.iter_mut().take(n / 2) {
        *slot = true;
    }
    let mut state = CHURN_SEED;
    let mut ops = Vec::with_capacity(len);
    let mut want_remove = true;
    while ops.len() < len {
        let want = want_remove;
        // Draw masked slots until one matches the required present/absent state.
        let slot = loop {
            let candidate = usize::try_from(xorshift64(&mut state) & ((n as u64) - 1))
                .expect("masked value is < n <= 256, fits usize");
            if occupied[candidate] == want {
                break candidate;
            }
        };
        if want {
            occupied[slot] = false;
            ops.push((ChurnOp::Remove, slot));
        } else {
            occupied[slot] = true;
            ops.push((ChurnOp::Insert, slot));
        }
        want_remove = !want_remove;
    }
    ops
}
