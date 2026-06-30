//! Unit tests for the shared bench support module. The `throughput` bench uses
//! `harness = false` (divan), so its embedded `#[test]` functions would never
//! run under `cargo test`; the bench support is `#[path]`-included here instead
//! so its correctness is verified by a normal integration test target.
#![cfg(not(miri))]

#[path = "../benches/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::collections::HashMap;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;
use support::BenchContainer;
use support::BoxArr;
use support::ChurnOp;
use support::Payload;
use support::churn_len;
use support::churn_ops;
use support::masked_index;

#[test]
fn masked_index_wraps_into_range() {
    // Arity16: low nibble.
    assert_eq!(masked_index::<Arity16>(17).as_usize(), 1);
    assert_eq!(masked_index::<Arity16>(15).as_usize(), 15);
    // Arity256: low byte.
    assert_eq!(masked_index::<Arity256>(256).as_usize(), 0);
    assert_eq!(masked_index::<Arity256>(255).as_usize(), 255);
}

#[test]
fn payload_fold_reads_whole_element() {
    // [0xAB; 32] folds to four identical chunks XORed → 0 for an even count.
    assert_eq!(<[u8; 32] as Payload>::make(0xAB).fold(), 0);
    // u64 folds to itself.
    assert_eq!(<u64 as Payload>::make(42).fold(), 42);
}

fn adapter_roundtrip<T: Payload + PartialEq + core::fmt::Debug, C: BenchContainer<T>>() {
    let mut c = C::fill(3); // slots 0,1,2 present
    assert_eq!(c.lookup(0), Some(&T::make(0)));
    assert_eq!(c.lookup(2), Some(&T::make(2)));
    assert_eq!(c.lookup(3), None);
    // set a new slot returns None; set a present slot returns the old value.
    assert_eq!(c.set(3, T::make(3)), None);
    assert_eq!(c.set(0, T::make(99)), Some(T::make(0)));
    // del a present slot returns it; del an absent slot returns None.
    assert_eq!(c.del(1), Some(T::make(1)));
    assert_eq!(c.del(1), None);
}

#[test]
fn adapters_behave_cell_a() {
    adapter_roundtrip::<[u8; 32], PackedArray<[u8; 32], Arity16>>();
    adapter_roundtrip::<[u8; 32], GappedArray<[u8; 32], Arity16>>();
    adapter_roundtrip::<[u8; 32], FixedArray<Option<[u8; 32]>, Arity16>>();
    adapter_roundtrip::<[u8; 32], BoxArr<[u8; 32], Arity16>>();
    adapter_roundtrip::<[u8; 32], BTreeMap<usize, [u8; 32]>>();
    adapter_roundtrip::<[u8; 32], HashMap<usize, [u8; 32]>>();
}

#[test]
fn adapters_behave_cell_b() {
    adapter_roundtrip::<u64, PackedArray<u64, Arity256>>();
    adapter_roundtrip::<u64, GappedArray<u64, Arity256>>();
    adapter_roundtrip::<u64, FixedArray<Option<u64>, Arity256>>();
    adapter_roundtrip::<u64, BoxArr<u64, Arity256>>();
    adapter_roundtrip::<u64, BTreeMap<usize, u64>>();
    adapter_roundtrip::<u64, HashMap<usize, u64>>();
}

#[test]
fn fold_sums_present_values() {
    // fill(2) → values make(0), make(1); XOR fold over u64 = 0 ^ 1 = 1.
    let c = <PackedArray<u64, Arity256> as BenchContainer<u64>>::fill(2);
    assert_eq!(c.fold(), 1u64); // 0 ^ 1 = 1
}

#[test]
fn churn_ops_hold_half_occupancy() {
    // Simulate the sequence against a plain occupancy bitset and confirm it
    // never no-ops: every Remove hits a present slot, every Insert an absent
    // one, and occupancy oscillates around N/2.
    let ops = churn_ops::<Arity16>();
    let n = Arity16::LEN;
    assert_eq!(ops.len(), churn_len(n));
    let mut occupied = vec![false; n];
    for slot in occupied.iter_mut().take(n / 2) {
        *slot = true;
    }
    let mut count = n / 2;
    for (op, slot) in ops {
        assert!(slot < n);
        match op {
            ChurnOp::Remove => {
                assert!(occupied[slot], "remove must target a present slot");
                occupied[slot] = false;
                count -= 1;
            }
            ChurnOp::Insert => {
                assert!(!occupied[slot], "insert must target an absent slot");
                occupied[slot] = true;
                count += 1;
            }
        }
        assert!(
            count == n / 2 || count == n / 2 - 1,
            "occupancy stays near half"
        );
    }
}

#[test]
fn bench_container_names_are_unique_and_nonempty() {
    // Every benched subject must have a distinct, non-empty NAME so the
    // criterion id path stays unambiguous. Listed per cell exactly as the
    // throughput bench sweeps them.
    let cell_a = [
        <PackedArray<[u8; 32], Arity16> as BenchContainer<[u8; 32]>>::NAME,
        <GappedArray<[u8; 32], Arity16> as BenchContainer<[u8; 32]>>::NAME,
        <FixedArray<Option<[u8; 32]>, Arity16> as BenchContainer<[u8; 32]>>::NAME,
        <BoxArr<[u8; 32], Arity16> as BenchContainer<[u8; 32]>>::NAME,
        <BTreeMap<usize, [u8; 32]> as BenchContainer<[u8; 32]>>::NAME,
        <HashMap<usize, [u8; 32]> as BenchContainer<[u8; 32]>>::NAME,
    ];
    for name in cell_a {
        assert!(!name.is_empty(), "NAME must be non-empty");
    }
    let mut deduped = cell_a.to_vec();
    deduped.sort_unstable();
    deduped.dedup();
    assert_eq!(
        deduped.len(),
        cell_a.len(),
        "NAMEs must be unique within a cell"
    );
}
