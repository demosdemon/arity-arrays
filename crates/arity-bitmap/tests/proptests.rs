//! Property tests: each `Bitmap` backing must agree with a `BTreeSet<usize>`
//! reference model for membership, rank, popcount, and ordered iteration.

use std::collections::BTreeSet;

use arity_bitmap::Bitmap;
use arity_index::Niche;
use proptest::prelude::*;

proptest! {
    #[test]
    fn u16_matches_model(indices in proptest::collection::vec(0usize..16, 0..16)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = u16::ZERO;
        for &i in &model {
            let idx = arity_index::U4::try_from_usize(i).expect("i < 16");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..16 {
            let idx = arity_index::U4::try_from_usize(i).expect("i < 16");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(arity_index::U4::as_usize).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(&fwd, &expected);
        let mut back: Vec<usize> = bm.bits().rev().map(arity_index::U4::as_usize).collect();
        back.reverse();
        prop_assert_eq!(&back, &expected);
    }

    #[test]
    fn u128_matches_model(indices in proptest::collection::vec(0usize..128, 0..128)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = u128::ZERO;
        for &i in &model {
            let idx = arity_index::U7::try_from_usize(i).expect("i < 128");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..128 {
            let idx = arity_index::U7::try_from_usize(i).expect("i < 128");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(arity_index::U7::as_usize).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(&fwd, &expected);
        let mut back: Vec<usize> = bm.bits().rev().map(arity_index::U7::as_usize).collect();
        back.reverse();
        prop_assert_eq!(&back, &expected);
    }

    #[test]
    fn u256_matches_model(indices in proptest::collection::vec(0usize..256, 0..256)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = arity_bitmap::U256::ZERO;
        for &i in &model {
            let idx = u8::try_from_usize(i).expect("i < 256");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..256 {
            let idx = u8::try_from_usize(i).expect("i < 256");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(Niche::as_usize).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(&fwd, &expected);
        let mut back: Vec<usize> = bm.bits().rev().map(Niche::as_usize).collect();
        back.reverse();
        prop_assert_eq!(&back, &expected);
    }
}
