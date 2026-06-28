//! Property and drop-safety tests for in-place `PackedArray` mutation.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use arity_arrays::index::U4;
use arity_arrays::{Arity16, PackedArray};
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
        assert!(p.insert(U4::new_masked(s), Counted(drops.clone())).is_none());
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
