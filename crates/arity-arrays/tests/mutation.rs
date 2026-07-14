//! Property and drop-safety tests for in-place `PackedArray` mutation.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::U4;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Insert(u8, u16),
    Remove(u8),
    GetMut(u8, u16),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..16, any::<u16>()).prop_map(|(s, v)| Op::Insert(s, v)),
        (0u8..16).prop_map(Op::Remove),
        (0u8..16, any::<u16>()).prop_map(|(s, v)| Op::GetMut(s, v)),
    ]
}

proptest! {
    #[test]
    fn packed_mutation_matches_btreemap(ops in proptest::collection::vec(op_strategy(), 0..200)) {
        let mut packed: PackedArray<u16, Arity16> = PackedArray::new();
        let mut oracle: BTreeMap<u8, u16> = BTreeMap::new();

        for op in ops {
            match op {
                Op::Insert(slot, val) => {
                    let i = U4::new_masked(slot);
                    let prev_p = packed.insert(i, val);
                    let prev_o = oracle.insert(i.as_u8(), val);
                    prop_assert_eq!(prev_p, prev_o);
                }
                Op::Remove(slot) => {
                    let i = U4::new_masked(slot);
                    prop_assert_eq!(packed.remove(i), oracle.remove(&i.as_u8()));
                }
                Op::GetMut(slot, val) => {
                    let i = U4::new_masked(slot);
                    if let Some(p) = packed.get_mut(i) { *p = val; }
                    if let Some(o) = oracle.get_mut(&i.as_u8()) { *o = val; }
                }
            }
            // Full-state cross-check after every op.
            prop_assert_eq!(packed.count(), oracle.len());
            for slot in 0..16u8 {
                let i = U4::new_masked(slot);
                prop_assert_eq!(packed.get(i), oracle.get(&i.as_u8()));
            }
        }
    }
}

/// A value that bumps a shared counter on drop, to detect leaks / double-drops.
struct Counted(Arc<AtomicUsize>);
impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn mutation_drops_each_element_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut p = PackedArray::<Counted, Arity16>::new();

    // Insert 4 elements (no drops yet).
    for s in [1u8, 4, 9, 14] {
        assert!(
            p.insert(U4::new_masked(s), Counted(drops.clone()))
                .is_none()
        );
    }
    assert_eq!(drops.load(Ordering::SeqCst), 0);

    // Overwrite slot 4: the old value is returned and dropped here.
    let old = p.insert(U4::new_masked(4), Counted(drops.clone()));
    assert!(old.is_some());
    drop(old);
    assert_eq!(drops.load(Ordering::SeqCst), 1);

    // Remove slot 9: returned then dropped.
    let r = p.remove(U4::new_masked(9));
    assert!(r.is_some());
    drop(r);
    assert_eq!(drops.load(Ordering::SeqCst), 2);

    // Drop the array: the remaining 3 elements (slots 1, 4, 14) drop exactly once.
    drop(p);
    assert_eq!(drops.load(Ordering::SeqCst), 5);
}

#[derive(Clone, Debug)]
enum Op256 {
    Insert(u8, u32),
    Remove(u8),
    GetMut(u8, u32),
}

fn op256_strategy() -> impl Strategy<Value = Op256> {
    prop_oneof![
        (any::<u8>(), any::<u32>()).prop_map(|(s, v)| Op256::Insert(s, v)),
        any::<u8>().prop_map(Op256::Remove),
        (any::<u8>(), any::<u32>()).prop_map(|(s, v)| Op256::GetMut(s, v)),
    ]
}

#[derive(Clone, Debug)]
enum GapOp<V> {
    Insert(u8, V),
    Remove(u8),
    GetMut(u8, V),
    Reserve(u8),
    ShrinkToFit,
    Clear,
}

// Mutation weighted heavier than capacity churn so sequences build real gap
// patterns before reshaping/clearing them.
fn gap_op16() -> impl Strategy<Value = GapOp<u16>> {
    prop_oneof![
        4 => (0u8..16, any::<u16>()).prop_map(|(s, v)| GapOp::Insert(s, v)),
        3 => (0u8..16).prop_map(GapOp::Remove),
        2 => (0u8..16, any::<u16>()).prop_map(|(s, v)| GapOp::GetMut(s, v)),
        1 => any::<u8>().prop_map(GapOp::Reserve),
        1 => Just(GapOp::ShrinkToFit),
        1 => Just(GapOp::Clear),
    ]
}

fn gap_op256() -> impl Strategy<Value = GapOp<u32>> {
    prop_oneof![
        4 => (any::<u8>(), any::<u32>()).prop_map(|(s, v)| GapOp::Insert(s, v)),
        3 => any::<u8>().prop_map(GapOp::Remove),
        2 => (any::<u8>(), any::<u32>()).prop_map(|(s, v)| GapOp::GetMut(s, v)),
        1 => any::<u8>().prop_map(GapOp::Reserve),
        1 => Just(GapOp::ShrinkToFit),
        1 => Just(GapOp::Clear),
    ]
}

proptest! {
    #[test]
    fn packed_mutation_matches_btreemap_arity256(
        ops in proptest::collection::vec(op256_strategy(), 0..200),
    ) {
        // Arity-256 uses `u8` directly as the index, so every byte is a valid
        // slot — no masking. Exercises the full-width U256 rank/with_bit/without_bit.
        let mut packed: PackedArray<u32, Arity256> = PackedArray::new();
        let mut oracle: BTreeMap<u8, u32> = BTreeMap::new();
        for op in ops {
            match op {
                Op256::Insert(slot, val) => {
                    prop_assert_eq!(packed.insert(slot, val), oracle.insert(slot, val));
                }
                Op256::Remove(slot) => {
                    prop_assert_eq!(packed.remove(slot), oracle.remove(&slot));
                }
                Op256::GetMut(slot, val) => {
                    if let Some(p) = packed.get_mut(slot) { *p = val; }
                    if let Some(o) = oracle.get_mut(&slot) { *o = val; }
                }
            }
            prop_assert_eq!(packed.count(), oracle.len());
            // Limb-boundary + spread spot-check after every op (cheap).
            for slot in [0u8, 1, 64, 126, 127, 128, 129, 200, 254, 255] {
                prop_assert_eq!(packed.get(slot), oracle.get(&slot));
            }
        }
        // Final full-domain sweep.
        for slot in 0..=255u8 {
            prop_assert_eq!(packed.get(slot), oracle.get(&slot));
        }
    }
}

#[derive(Clone, Debug)]
enum ZstOp {
    Insert(u8),
    Remove(u8),
}

fn zst_op_strategy() -> impl Strategy<Value = ZstOp> {
    prop_oneof![
        (0u8..16).prop_map(ZstOp::Insert),
        (0u8..16).prop_map(ZstOp::Remove),
    ]
}

proptest! {
    #[test]
    fn packed_mutation_zst_matches_btreemap(
        ops in proptest::collection::vec(zst_op_strategy(), 0..200),
    ) {
        // Zero-sized `T = ()`: the block is sized to the bitmap alone; element
        // writes/reads/copies are no-ops, but rank-select must still hold.
        // Use a `BTreeSet` oracle (not `BTreeMap<u8, ()>`) to avoid the
        // `clippy::zero_sized_map_values` lint.
        let mut packed: PackedArray<(), Arity16> = PackedArray::new();
        let mut oracle: BTreeSet<u8> = BTreeSet::new();
        for op in ops {
            match op {
                ZstOp::Insert(slot) => {
                    let i = U4::new_masked(slot);
                    let was_present = oracle.contains(&i.as_u8());
                    oracle.insert(i.as_u8());
                    let prev = packed.insert(i, ());
                    // `insert` returns `None` when the slot was absent, `Some(())`
                    // when it was already present — mirrors `BTreeSet::insert`
                    // returning `true` for new / `false` for duplicate.
                    prop_assert_eq!(prev.is_some(), was_present);
                }
                ZstOp::Remove(slot) => {
                    let i = U4::new_masked(slot);
                    let was_present = oracle.remove(&i.as_u8());
                    let removed = packed.remove(i);
                    prop_assert_eq!(removed.is_some(), was_present);
                }
            }
            prop_assert_eq!(packed.count(), oracle.len());
            for slot in 0..16u8 {
                let i = U4::new_masked(slot);
                prop_assert_eq!(packed.get(i).is_some(), oracle.contains(&i.as_u8()));
            }
        }
    }
}

proptest! {
    #[test]
    fn gapped_mutation_matches_btreemap(ops in proptest::collection::vec(gap_op16(), 0..200)) {
        let mut g: GappedArray<u16, Arity16> = GappedArray::new();
        let mut oracle: BTreeMap<u8, u16> = BTreeMap::new();
        for op in ops {
            match op {
                GapOp::Insert(slot, val) => {
                    let i = U4::new_masked(slot);
                    prop_assert_eq!(g.insert(i, val), oracle.insert(i.as_u8(), val));
                }
                GapOp::Remove(slot) => {
                    let i = U4::new_masked(slot);
                    // delete-never-moves: capture a surviving element's address.
                    let survivor = (0..16u8)
                        .map(U4::new_masked)
                        .find(|&j| j.as_u8() != slot && g.get(j).is_some());
                    let before = survivor.and_then(|j| g.get(j)).map(|r| std::ptr::from_ref(r) as usize);
                    prop_assert_eq!(g.remove(i), oracle.remove(&i.as_u8()));
                    if let (Some(j), Some(addr)) = (survivor, before)
                        && let Some(r) = g.get(j)
                    {
                        prop_assert_eq!(std::ptr::from_ref(r) as usize, addr, "remove moved a survivor");
                    }
                }
                GapOp::GetMut(slot, val) => {
                    let i = U4::new_masked(slot);
                    if let Some(p) = g.get_mut(i) { *p = val; }
                    if let Some(o) = oracle.get_mut(&i.as_u8()) { *o = val; }
                }
                GapOp::Reserve(n) => {
                    g.reserve(n as usize); // reserve preserves logical content
                }
                GapOp::ShrinkToFit => {
                    g.shrink_to_fit();
                }
                GapOp::Clear => {
                    g.clear();
                    oracle.clear();
                }
            }
            // Structural invariants.
            prop_assert_eq!(g.count(), oracle.len());
            let cap = g.capacity();
            if cap > 0 {
                prop_assert!(cap.is_power_of_two() && cap <= 16 && cap >= g.count().max(1));
            }
            for slot in 0..16u8 {
                let i = U4::new_masked(slot);
                prop_assert_eq!(g.get(i), oracle.get(&i.as_u8()));
            }
        }
    }
}

proptest! {
    #[test]
    fn gapped_mutation_matches_btreemap_arity256(
        ops in proptest::collection::vec(gap_op256(), 0..200),
    ) {
        let mut g: GappedArray<u32, Arity256> = GappedArray::new();
        let mut oracle: BTreeMap<u8, u32> = BTreeMap::new();
        for op in ops {
            match op {
                GapOp::Insert(slot, val) => prop_assert_eq!(g.insert(slot, val), oracle.insert(slot, val)),
                GapOp::Remove(slot) => prop_assert_eq!(g.remove(slot), oracle.remove(&slot)),
                GapOp::GetMut(slot, val) => {
                    if let Some(p) = g.get_mut(slot) { *p = val; }
                    if let Some(o) = oracle.get_mut(&slot) { *o = val; }
                }
                GapOp::Reserve(n) => g.reserve(n as usize),
                GapOp::ShrinkToFit => g.shrink_to_fit(),
                GapOp::Clear => { g.clear(); oracle.clear(); }
            }
            prop_assert_eq!(g.count(), oracle.len());
            let cap = g.capacity();
            if cap > 0 { prop_assert!(cap.is_power_of_two() && cap <= 256); }
            for slot in [0u8, 1, 64, 127, 128, 129, 255] {
                prop_assert_eq!(g.get(slot), oracle.get(&slot));
            }
        }
        for slot in 0..=255u8 { prop_assert_eq!(g.get(slot), oracle.get(&slot)); }
    }
}

proptest! {
    #[test]
    fn gapped_mutation_zst(ops in proptest::collection::vec(zst_op_strategy(), 0..200)) {
        let mut g: GappedArray<(), Arity16> = GappedArray::new();
        let mut oracle: BTreeSet<u8> = BTreeSet::new();
        for op in ops {
            match op {
                ZstOp::Insert(slot) => {
                    let i = U4::new_masked(slot);
                    let was = oracle.contains(&i.as_u8());
                    oracle.insert(i.as_u8());
                    prop_assert_eq!(g.insert(i, ()).is_some(), was);
                }
                ZstOp::Remove(slot) => {
                    let i = U4::new_masked(slot);
                    let was = oracle.remove(&i.as_u8());
                    prop_assert_eq!(g.remove(i).is_some(), was);
                }
            }
            prop_assert_eq!(g.count(), oracle.len());
            for slot in 0..16u8 {
                prop_assert_eq!(g.get(U4::new_masked(slot)).is_some(), oracle.contains(&slot));
            }
        }
    }
}
