#![no_main]

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use arity_arrays::index::U4;
use arity_arrays::{Arity16, PackedArray};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
enum Op {
    Insert(u8, Vec<u8>),
    Remove(u8),
    GetMut(u8, Vec<u8>),
}

fuzz_target!(|ops: Vec<Op>| {
    // Heap-owning value type (`Vec<u8>`) so the libFuzzer + AddressSanitizer /
    // LeakSanitizer run catches drop bugs in the realloc/copy/dealloc paths: a
    // forgotten drop surfaces as a leak, a double-drop as a double-free / UAF.
    // (The deterministic "dropped exactly once" check lives in tests/mutation.rs;
    // here ASAN provides the leak/double-free coverage the spec asks for.)
    let mut packed: PackedArray<Vec<u8>, Arity16> = PackedArray::new();
    let mut oracle: BTreeMap<u8, Vec<u8>> = BTreeMap::new();

    for op in ops {
        match op {
            Op::Insert(slot, val) => {
                let i = U4::new_masked(slot);
                let prev_p = packed.insert(i, val.clone());
                let prev_o = oracle.insert(i.as_u8(), val);
                assert_eq!(prev_p, prev_o);
            }
            Op::Remove(slot) => {
                let i = U4::new_masked(slot);
                assert_eq!(packed.remove(i), oracle.remove(&i.as_u8()));
            }
            Op::GetMut(slot, val) => {
                let i = U4::new_masked(slot);
                if let Some(p) = packed.get_mut(i) {
                    *p = val.clone();
                }
                if let Some(o) = oracle.get_mut(&i.as_u8()) {
                    *o = val;
                }
            }
        }
        // Full-state cross-check.
        assert_eq!(packed.count(), oracle.len());
        for slot in 0..16u8 {
            let i = U4::new_masked(slot);
            assert_eq!(packed.get(i), oracle.get(&i.as_u8()));
        }
    }

    // Clone equivalence (slot-by-slot), then both drop (ASAN catches any leak /
    // double-free).
    let cloned = packed.clone();
    assert_eq!(cloned.count(), oracle.len());
    for slot in 0..16u8 {
        let i = U4::new_masked(slot);
        assert_eq!(cloned.get(i), oracle.get(&i.as_u8()));
    }
});
