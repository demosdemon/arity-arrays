//! Shared harness for the `mutation_*` fuzz targets: drive `PackedArray`
//! insert/remove/get_mut against a `BTreeMap` oracle, generic over arity.
//!
//! `#[path]`-included by each `fuzz_targets/mutation_*.rs`; the fuzz crate has
//! no `[lib]`, so this is a per-binary module include.

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use arity_arrays::index::Niche;
use arity_arrays::{Arity, PackedArray};

#[derive(Arbitrary, Debug)]
pub enum Op {
    Insert(u8, Vec<u8>),
    Remove(u8),
    GetMut(u8, Vec<u8>),
}

/// Map an arbitrary byte to a valid index for arity `A`. The mask is total for
/// a power-of-two width, so `try_from_usize` always returns `Some` and the
/// `unwrap` cannot fire. For `Arity16`, `slot & 15` reproduces the old
/// `U4::new_masked(slot)` exactly, so the migrated corpus stays meaningful.
fn idx<A: Arity>(slot: u8) -> A::Index {
    <A::Index as Niche>::try_from_usize((slot as usize) & (A::LEN - 1)).unwrap()
}

/// Replay `ops` against a `PackedArray<Vec<u8>, A>` and a `BTreeMap` oracle.
/// Per-op cross-check is O(1) (count + the touched slot); the full `0..A::LEN`
/// sweep runs once at end-of-run so wide arities keep op-sequence depth under
/// the fixed fuzz budget. `Vec<u8>` values let ASAN/LSan catch drop bugs.
pub fn mutation_run<A: Arity>(ops: Vec<Op>) {
    let mut packed: PackedArray<Vec<u8>, A> = PackedArray::new();
    let mut oracle: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

    for op in ops {
        let i = match &op {
            Op::Insert(slot, _) | Op::Remove(slot) | Op::GetMut(slot, _) => idx::<A>(*slot),
        };
        match op {
            Op::Insert(_, val) => {
                let prev_p = packed.insert(i, val.clone());
                let prev_o = oracle.insert(i.as_usize(), val);
                assert_eq!(prev_p, prev_o);
            }
            Op::Remove(_) => {
                assert_eq!(packed.remove(i), oracle.remove(&i.as_usize()));
            }
            Op::GetMut(_, val) => {
                if let Some(p) = packed.get_mut(i) {
                    *p = val.clone();
                }
                if let Some(o) = oracle.get_mut(&i.as_usize()) {
                    *o = val;
                }
            }
        }
        // O(1) post-op check: count plus the single touched slot.
        assert_eq!(packed.count(), oracle.len());
        assert_eq!(packed.get(i), oracle.get(&i.as_usize()));
    }

    // End-of-run: clone equivalence + full-domain sweep. Both drop after, so
    // ASAN catches any leak / double-free.
    let cloned = packed.clone();
    assert_eq!(cloned.count(), oracle.len());
    for i in <A::Index as Niche>::all() {
        let k = i.as_usize();
        assert_eq!(packed.get(i), oracle.get(&k));
        assert_eq!(cloned.get(i), oracle.get(&k));
    }
}
