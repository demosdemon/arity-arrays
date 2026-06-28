//! `serde_with::Compact` round-trip + adversarial-decode tests.
#![cfg(feature = "serde_with")]

use arity_arrays::Arity16;
use arity_arrays::Compact;
use arity_arrays::PackedArray;
use arity_arrays::index::U4;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Node {
    #[serde_as(as = "Compact")]
    children: PackedArray<u16, Arity16>,
}

#[test]
fn compact_round_trip() {
    let mut children = PackedArray::<u16, Arity16>::new();
    children.insert(U4::new_masked(2), 20);
    children.insert(U4::new_masked(9), 90);
    let node = Node { children };

    let json = serde_json::to_string(&node).expect("ser");
    // bitmap = bits 2 and 9 set = 0x0204, little-endian bytes [4, 2]; values [20,
    // 90].
    assert_eq!(json, r#"{"children":[[4,2],[20,90]]}"#);
    let back: Node = serde_json::from_str(&json).expect("de");
    assert_eq!(node, back);
}

#[test]
fn compact_rejects_popcount_mismatch() {
    // bitmap [4,2] has popcount 2, but only one value is supplied.
    let bad = r#"{"children":[[4,2],[20]]}"#;
    assert!(serde_json::from_str::<Node>(bad).is_err());
    // wrong-length bitmap (BYTES must be 2 for Arity16).
    let bad_len = r#"{"children":[[4,2,0],[20,90]]}"#;
    assert!(serde_json::from_str::<Node>(bad_len).is_err());
}

#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Node256 {
    #[serde_as(as = "Compact")]
    children: arity_arrays::PackedArray<u32, arity_arrays::Arity256>,
}

#[test]
fn compact_arity256_round_trip_stable_bytes() {
    let mut children = arity_arrays::PackedArray::<u32, arity_arrays::Arity256>::new();
    children.insert(0, 1);
    children.insert(128, 2); // limb boundary
    children.insert(255, 3);
    let node = Node256 { children };
    let json = serde_json::to_string(&node).expect("ser");
    // 32-byte LE bitmap with bits 0,128,255 set, then values [1,2,3].
    // Byte 0 bit0 -> 1; byte 16 bit0 (bit 128) -> 1; byte 31 bit7 (bit 255) -> 128.
    assert!(json.contains("[1,2,3]"));
    let back: Node256 = serde_json::from_str(&json).expect("de");
    assert_eq!(node, back);
}
