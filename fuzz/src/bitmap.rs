//! Shared harness for the `bitmap_roundtrip` fuzz target: byte<->bitmap
//! totality plus accessor self-consistency for `U256`.
//!
//! `#[path]`-included by `fuzz_targets/bitmap_roundtrip.rs`.

use arity_arrays::bitmap::Bitmap;
use arity_arrays::index::Niche;

pub fn bitmap_roundtrip_run<B: Bitmap>(bytes: &[u8])
where
    <B as Bitmap>::Index: std::fmt::Debug,
{
    // `try_from_bytes` rejects any length != BYTES; the thin wrapper always
    // passes 32 bytes, so this guards only against generic-fn misuse.
    let Some(b) = B::try_from_bytes(bytes) else {
        return;
    };

    // Byte round-trip is total: every pattern is a valid width-WIDTH bitmap.
    let out = b.to_bytes();
    assert_eq!(out.as_ref(), bytes);

    // bits() enumerates exactly the set positions.
    assert_eq!(b.bits().count() as u32, b.count_ones());

    // rank/select are inverse on present bits (documented contract).
    for i in b.bits() {
        assert!(b.test(i));
        assert_eq!(b.select(b.rank(i)), Some(i));
    }

    // with_bit/without_bit behave at every position, incl. the limb boundary
    // at bit 128.
    for k in 0..B::WIDTH {
        let i = <B::Index as Niche>::try_from_usize(k).unwrap();
        assert!(b.with_bit(i).test(i));
        assert!(!b.without_bit(i).test(i));
    }
}
