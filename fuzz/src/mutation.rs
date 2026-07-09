//! Shared harness for the `mutation_*` and `gapped_*` fuzz targets: drive
//! `PackedArray` or `GappedArray` insert/remove/get_mut against a `BTreeMap`
//! oracle, generic over both arity and container type.
//!
//! `#[path]`-included by each `fuzz_targets/mutation_*.rs` and
//! `fuzz_targets/gapped_*.rs`; the fuzz crate has no `[lib]`, so this is a
//! per-binary module include.

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use arity_arrays::Arity;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;

#[derive(Arbitrary, Debug)]
pub enum Op {
    Insert(u8, Vec<u8>),
    Remove(u8),
    GetMut(u8, Vec<u8>),
    Reserve(u8),
    ShrinkToFit,
    Clear,
}

/// Map an arbitrary byte to a valid index for arity `A`. The mask is total for
/// a power-of-two width, so `try_from_usize` always returns `Some` and the
/// `unwrap` cannot fire. For `Arity16`, `slot & 15` reproduces the old
/// `U4::new_masked(slot)` exactly, so the migrated corpus stays meaningful.
fn idx<A: Arity>(slot: u8) -> A::Index {
    <A::Index as Niche>::try_from_usize((slot as usize) & (A::LEN - 1)).unwrap()
}

/// Abstraction over `PackedArray<Vec<u8>, A>` and `GappedArray<Vec<u8>, A>`
/// that exposes the operations exercised by the mutation harness.
trait ArrayOracle<A: Arity>: Clone + Default {
    fn insert(&mut self, i: A::Index, v: Vec<u8>) -> Option<Vec<u8>>;
    fn remove(&mut self, i: A::Index) -> Option<Vec<u8>>;
    fn get_mut(&mut self, i: A::Index) -> Option<&mut Vec<u8>>;
    fn get(&self, i: A::Index) -> Option<&Vec<u8>>;
    fn count(&self) -> usize;
    // Capacity ops: Packed has no slack, so reserve/shrink are no-ops there.
    fn reserve(&mut self, _n: usize) {}
    fn shrink_to_fit(&mut self) {}
    fn clear(&mut self);
}

impl<A: Arity> ArrayOracle<A> for PackedArray<Vec<u8>, A> {
    fn insert(&mut self, i: A::Index, v: Vec<u8>) -> Option<Vec<u8>> {
        PackedArray::insert(self, i, v)
    }
    fn remove(&mut self, i: A::Index) -> Option<Vec<u8>> {
        PackedArray::remove(self, i)
    }
    fn get_mut(&mut self, i: A::Index) -> Option<&mut Vec<u8>> {
        PackedArray::get_mut(self, i)
    }
    fn get(&self, i: A::Index) -> Option<&Vec<u8>> {
        PackedArray::get(self, i)
    }
    fn count(&self) -> usize {
        PackedArray::count(self)
    }
    fn clear(&mut self) {
        *self = PackedArray::new();
    }
}

impl<A: Arity> ArrayOracle<A> for GappedArray<Vec<u8>, A> {
    fn insert(&mut self, i: A::Index, v: Vec<u8>) -> Option<Vec<u8>> {
        GappedArray::insert(self, i, v)
    }
    fn remove(&mut self, i: A::Index) -> Option<Vec<u8>> {
        GappedArray::remove(self, i)
    }
    fn get_mut(&mut self, i: A::Index) -> Option<&mut Vec<u8>> {
        GappedArray::get_mut(self, i)
    }
    fn get(&self, i: A::Index) -> Option<&Vec<u8>> {
        GappedArray::get(self, i)
    }
    fn count(&self) -> usize {
        GappedArray::count(self)
    }
    fn reserve(&mut self, n: usize) {
        GappedArray::reserve(self, n);
    }
    fn shrink_to_fit(&mut self) {
        GappedArray::shrink_to_fit(self);
    }
    fn clear(&mut self) {
        GappedArray::clear(self);
    }
}

/// Replay `ops` against a `C` container and a `BTreeMap` oracle, where `C`
/// implements `ArrayOracle<A>`.
///
/// Per-op cross-check is O(1) (count + the touched slot); the full `0..A::LEN`
/// sweep runs once at end-of-run so wide arities keep op-sequence depth under
/// the fixed fuzz budget. `Vec<u8>` values let ASAN/LSan catch drop bugs.
// `ArrayOracle` is crate-internal: callers supply concrete types (PackedArray
// or GappedArray) and never need to name the trait. The private_bounds lint
// fires because `pub fn` appears to promise something callers cannot satisfy,
// but in practice this file is #[path]-included per binary and every call site
// already passes a fully-concrete type argument.
#[expect(
    private_bounds,
    reason = "ArrayOracle is an internal seam between the two array impls; \
              callers pass a concrete type and never need to name the trait"
)]
pub fn mutation_run<A: Arity, C: ArrayOracle<A>>(ops: Vec<Op>) {
    let mut arr: C = C::default();
    let mut oracle: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

    for op in ops {
        // `touched` is `Some` only for slot-carrying ops; capacity ops skip the
        // per-slot check but still run the count check below.
        let touched: Option<A::Index> = match op {
            Op::Insert(slot, val) => {
                let i = idx::<A>(slot);
                let prev_a = arr.insert(i, val.clone());
                let prev_o = oracle.insert(i.as_usize(), val);
                assert_eq!(prev_a, prev_o);
                Some(i)
            }
            Op::Remove(slot) => {
                let i = idx::<A>(slot);
                assert_eq!(arr.remove(i), oracle.remove(&i.as_usize()));
                Some(i)
            }
            Op::GetMut(slot, val) => {
                let i = idx::<A>(slot);
                if let Some(p) = arr.get_mut(i) {
                    *p = val.clone();
                }
                if let Some(o) = oracle.get_mut(&i.as_usize()) {
                    *o = val;
                }
                Some(i)
            }
            Op::Reserve(n) => {
                arr.reserve(n as usize);
                None
            }
            Op::ShrinkToFit => {
                arr.shrink_to_fit();
                None
            }
            Op::Clear => {
                arr.clear();
                oracle.clear();
                None
            }
        };
        // Count always; the touched slot for slot-carrying ops (unchanged density
        // for Insert/Remove/GetMut).
        assert_eq!(arr.count(), oracle.len());
        if let Some(i) = touched {
            assert_eq!(arr.get(i), oracle.get(&i.as_usize()));
        }
    }

    // End-of-run: clone equivalence + full-domain sweep. Both drop after, so
    // ASAN catches any leak / double-free.
    let cloned = arr.clone();
    assert_eq!(cloned.count(), oracle.len());
    for i in <A::Index as Niche>::all() {
        let k = i.as_usize();
        assert_eq!(arr.get(i), oracle.get(&k));
        assert_eq!(cloned.get(i), oracle.get(&k));
    }
}
