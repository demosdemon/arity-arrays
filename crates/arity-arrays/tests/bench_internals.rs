//! Unit tests for the shared bench support module. The `throughput` bench uses
//! `harness = false` (divan), so its embedded `#[test]` functions would never
//! run under `cargo test`; the bench support is `#[path]`-included here instead
//! so its correctness is verified by a normal integration test target.
#![cfg(not(miri))]

#[path = "../benches/support.rs"]
mod support;

use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::index::Niche;
use support::Payload;
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
