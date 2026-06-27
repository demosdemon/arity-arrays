//! Regression guard: `Bitmap::bits()` (and the other methods) must be callable
//! from generic code through only the public `Bitmap` bound — this is how
//! `arity-arrays` uses it (`PackedArray<A: Arity>` over `A::Bitmap`). An
//! earlier sealed-trait design compiled for concrete receivers but failed in a
//! generic context; this test locks in the fix.

use arity_bitmap::Bitmap;
use arity_bitmap::U256;
use arity_index::Niche;
use arity_index::U4;

fn collect_indices<B: Bitmap>(bm: B) -> Vec<usize> {
    bm.bits().map(Niche::as_usize).collect()
}

fn last<B: Bitmap>(bm: B) -> Option<<B as Bitmap>::Index> {
    bm.bits().next_back()
}

#[test]
fn bits_callable_through_generic_bitmap_bound() {
    let a = u16::ZERO
        .with_bit(U4::new_masked(1))
        .with_bit(U4::new_masked(9));
    assert_eq!(collect_indices(a), vec![1, 9]);
    assert_eq!(last(a).map(Niche::as_usize), Some(9));

    let b = U256::ZERO.with_bit(0).with_bit(200);
    assert_eq!(collect_indices(b), vec![0, 200]);
    assert_eq!(last(b).map(Niche::as_usize), Some(200));
}
