//! Logical serde round-trip + adversarial-decode tests.
#![cfg(feature = "serde")]

use arity_arrays::index::U4;
use arity_arrays::{Arity16, Arity256, FixedArray, PackedArray};

#[test]
fn fixed_round_trip() {
    let mut a = FixedArray::<u8, Arity16>::from_fn(U4::as_u8);
    a[U4::new_masked(3)] = 200;
    let json = serde_json::to_string(&a).expect("ser");
    let back: FixedArray<u8, Arity16> = serde_json::from_str(&json).expect("de");
    assert_eq!(a, back);
    // Wrong length is rejected.
    assert!(serde_json::from_str::<FixedArray<u8, Arity16>>("[1,2,3]").is_err());
}

#[test]
fn packed_logical_round_trip_and_validation() {
    let mut p = PackedArray::<u16, Arity16>::new();
    p.insert(U4::new_masked(2), 20);
    p.insert(U4::new_masked(9), 90);
    let json = serde_json::to_string(&p).expect("ser");
    assert_eq!(json, "[[2,20],[9,90]]"); // ascending (index, value) pairs
    let back: PackedArray<u16, Arity16> = serde_json::from_str(&json).expect("de");
    assert_eq!(p, back);

    // Non-ascending indices are rejected.
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[9,90],[2,20]]").is_err());
    // Duplicate indices are rejected.
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[2,20],[2,21]]").is_err());
    // Out-of-range index is rejected (16 invalid for Arity16 / U4).
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[16,1]]").is_err());
}

#[test]
fn packed_arity256_round_trip() {
    let mut p = PackedArray::<u32, Arity256>::new();
    p.insert(0, 1);
    p.insert(255, 2);
    let json = serde_json::to_string(&p).expect("ser");
    let back: PackedArray<u32, Arity256> = serde_json::from_str(&json).expect("de");
    assert_eq!(p, back);
}
