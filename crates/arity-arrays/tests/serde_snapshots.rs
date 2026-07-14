//! Snapshot the wire formats (so drift is a reviewable diff) and assert the
//! Compact bitmap encoding is a canonical little-endian form, independent of
//! the in-memory representation.
#![cfg(feature = "serde_with")]

use arity_arrays::Arity16;
use arity_arrays::Compact;
use arity_arrays::PackedArray;
use arity_arrays::index::U4;
use serde::Serialize;
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize)]
struct CompactNode {
    #[serde_as(as = "Compact")]
    children: PackedArray<u16, Arity16>,
}

fn sample() -> PackedArray<u16, Arity16> {
    let mut p = PackedArray::<u16, Arity16>::new();
    p.insert(U4::new_masked(1), 11);
    p.insert(U4::new_masked(4), 44);
    p.insert(U4::new_masked(14), 14);
    p
}

#[test]
fn snapshot_logical_form() {
    let json = serde_json::to_string(&sample()).expect("ser");
    insta::assert_snapshot!("packed_logical", json);
}

#[test]
fn snapshot_compact_form() {
    let json = serde_json::to_string(&CompactNode { children: sample() }).expect("ser");
    insta::assert_snapshot!("packed_compact", json);
}
