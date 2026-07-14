//! The 256-bit bitmap backing (`Bitmap::Index == u8`).
//!
//! [`U256`] is re-exported from [`ethnum`]; the crate implements the [`Bitmap`]
//! surface on it. It is nameable as `<Arity256 as Arity>::Bitmap` and, from
//! `arity-arrays`, as `arity_arrays::bitmap::U256`.

use arity_index::Niche;

use crate::Bitmap;
use crate::Raw;
use crate::Sealed;

// Wire-up invariant: the u8 index domain (256) must equal the bit width.
const _: () = assert!(<u8 as Niche>::COUNT == 256);

mod ethnum_backed {
    pub use ethnum::U256;

    use super::Bitmap;
    use super::Raw;
    use super::Sealed;

    // ethnum has no ZERO/ONE consts we can rely on; build them from words.
    const ZERO: U256 = U256::from_words(0, 0);
    const ONE: U256 = U256::from_words(0, 1);

    /// Mask of bits `[0, k)` for the ethnum `U256` (`k <= 256`).
    #[inline]
    fn low_mask(k: usize) -> U256 {
        if k >= 256 {
            !ZERO
        } else {
            let k = u32::try_from(k).expect("k < 256 fits u32");
            (ONE << k) - ONE
        }
    }

    impl Sealed for U256 {}

    impl Raw for U256 {
        #[inline]
        fn raw_is_zero(self) -> bool {
            self == ZERO
        }
        #[inline]
        fn raw_popcount(self) -> u32 {
            // `Self::count_ones` binds to ethnum's inherent method (inherent wins over the
            // `Bitmap::count_ones` being implemented); writing the bare `self.count_ones()`
            // or the trait path here would recurse.
            Self::count_ones(self)
        }
        #[inline]
        fn raw_lowest_pos(self) -> usize {
            self.trailing_zeros() as usize
        }
        #[inline]
        fn raw_highest_pos(self) -> usize {
            255 - self.leading_zeros() as usize
        }
        #[inline]
        fn raw_clear_lowest(self) -> Self {
            if self == ZERO {
                self
            } else {
                self & (self - ONE)
            }
        }
        #[inline]
        fn raw_clear_highest(self) -> Self {
            if self == ZERO {
                self
            } else {
                self & !(ONE << (255 - self.leading_zeros()))
            }
        }
        #[inline]
        fn raw_select(self, n: u32) -> Option<usize> {
            // `Self::count_ones` binds to ethnum's inherent method.
            if n >= Self::count_ones(self) {
                return None;
            }
            // Popcount-guided binary search over the full 256-bit value; `size`
            // starts at 128 so no shift reaches the 256-bit width.
            let mut n = n;
            let mut x = self;
            let mut pos = 0usize;
            let mut size: u32 = 128;
            loop {
                let lo_mask = (ONE << size) - ONE;
                let lo_count = Self::count_ones(x & lo_mask);
                if n >= lo_count {
                    n -= lo_count;
                    x >>= size;
                    pos += size as usize;
                }
                if size == 1 {
                    return Some(pos);
                }
                size /= 2;
            }
        }
        #[inline]
        fn raw_nearest_clear_at_or_below(self, from: usize) -> Option<usize> {
            debug_assert!(from < 256);
            // Bits [0, from] inclusive == low_mask(from + 1).
            let holes = !self & low_mask(from + 1);
            if holes == ZERO {
                None
            } else {
                // 255 - leading_zeros = highest set bit = greatest clear <= from.
                Some(255 - holes.leading_zeros() as usize)
            }
        }

        #[inline]
        fn raw_nearest_clear_in(self, from: usize, limit: usize) -> Option<usize> {
            debug_assert!(from <= limit && limit <= 256);
            let holes = !self & low_mask(limit) & !low_mask(from);
            if holes == ZERO {
                None
            } else {
                Some(holes.trailing_zeros() as usize)
            }
        }
    }

    impl Bitmap for U256 {
        type Index = u8;
        const WIDTH: usize = 256;
        const ZERO: Self = ZERO;

        #[inline]
        fn is_zero(self) -> bool {
            self == ZERO
        }
        #[inline]
        fn count_ones(self) -> u32 {
            // `Self::count_ones` binds to ethnum's inherent method (inherent wins over the
            // `Bitmap::count_ones` being implemented); writing the bare `self.count_ones()`
            // or the trait path here would recurse.
            Self::count_ones(self)
        }
        #[inline]
        fn test(self, i: u8) -> bool {
            (self >> u32::from(i)) & ONE != ZERO
        }
        #[inline]
        fn with_bit(self, i: u8) -> Self {
            self | (ONE << u32::from(i))
        }
        #[inline]
        fn rank(self, i: u8) -> u32 {
            if i == 0 {
                0
            } else {
                // `Self::count_ones` binds to ethnum's inherent method (inherent wins over the
                // `Bitmap::count_ones` being implemented); writing the bare `self.count_ones()`
                // or the trait path here would recurse.
                Self::count_ones(self & ((ONE << u32::from(i)) - ONE))
            }
        }
        #[inline]
        fn without_bit(self, i: u8) -> Self {
            self & !(ONE << u32::from(i))
        }
        type Bytes = [u8; 32];
        #[inline]
        fn to_bytes(self) -> Self::Bytes {
            // ethnum's inherent method returns [u8; 32].
            Self::to_le_bytes(self)
        }
        #[inline]
        fn from_bytes(bytes: Self::Bytes) -> Self {
            Self::from_le_bytes(bytes)
        }
    }
}
pub use ethnum_backed::U256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_zero_and_basic_bits() {
        assert_eq!(<U256 as Bitmap>::WIDTH, 256);
        assert!(U256::ZERO.is_zero());
        assert_eq!(U256::ZERO.count_ones(), 0);

        // bits spanning both limbs: 0 (lo), 127 (lo top), 128 (hi bottom), 255 (hi top)
        let bm = U256::ZERO
            .with_bit(0)
            .with_bit(127)
            .with_bit(128)
            .with_bit(255);
        assert_eq!(bm.count_ones(), 4);
        assert!(bm.test(0));
        assert!(bm.test(127));
        assert!(bm.test(128));
        assert!(bm.test(255));
        assert!(!bm.test(1));
        assert!(!bm.test(129));
    }

    #[test]
    fn rank_across_the_limb_boundary() {
        let bm = U256::ZERO
            .with_bit(0)
            .with_bit(127)
            .with_bit(128)
            .with_bit(255);
        assert_eq!(bm.rank(0), 0);
        assert_eq!(bm.rank(127), 1);
        assert_eq!(bm.rank(128), 2); // bits 0 and 127 are below 128
        assert_eq!(bm.rank(255), 3);
        assert_eq!(bm.rank(200), 3); // 0,127,128 below 200
    }

    #[test]
    fn bits_forward_and_back_span_limbs() {
        let bm = U256::ZERO
            .with_bit(3)
            .with_bit(127)
            .with_bit(128)
            .with_bit(254);
        let fwd: alloc::vec::Vec<u8> = bm.bits().collect();
        assert_eq!(fwd, alloc::vec![3u8, 127, 128, 254]);
        let back: alloc::vec::Vec<u8> = bm.bits().rev().collect();
        assert_eq!(back, alloc::vec![254u8, 128, 127, 3]);

        let mut it = bm.bits();
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next_back(), Some(254));
        assert_eq!(it.next(), Some(127));
        assert_eq!(it.next_back(), Some(128));
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
    }

    #[test]
    fn without_bit_across_limbs() {
        let bm = U256::ZERO
            .with_bit(3)
            .with_bit(127)
            .with_bit(128)
            .with_bit(254);
        let cleared = bm.without_bit(128);
        assert!(!cleared.test(128));
        assert!(cleared.test(127));
        assert!(cleared.test(254));
        assert_eq!(cleared.count_ones(), 3);
        assert_eq!(bm.without_bit(200), bm); // unset bit: no-op
    }

    #[test]
    fn select_spans_limbs() {
        let bm = U256::ZERO
            .with_bit(3)
            .with_bit(127)
            .with_bit(128)
            .with_bit(254);
        assert_eq!(bm.select(0), Some(3));
        assert_eq!(bm.select(1), Some(127));
        assert_eq!(bm.select(2), Some(128));
        assert_eq!(bm.select(3), Some(254));
        assert_eq!(bm.select(4), None);
    }

    #[test]
    fn le_bytes_round_trip_u256() {
        assert_eq!(<U256 as Bitmap>::BYTES, 32);
        let bm = U256::ZERO
            .with_bit(3)
            .with_bit(127)
            .with_bit(128)
            .with_bit(254);
        let bytes = <U256 as Bitmap>::to_bytes(bm);
        // bit 128 is the lowest bit of the high limb -> first byte of the second half.
        assert_eq!(bytes[16], 0b0000_0001);
        assert_eq!(<U256 as Bitmap>::from_bytes(bytes), bm);
    }

    #[test]
    fn try_from_bytes_checks_length_u256() {
        let bm = U256::ZERO.with_bit(3).with_bit(128).with_bit(254);
        // Exact length round-trips; every other length is rejected (U256 wants 32).
        assert_eq!(
            <U256 as Bitmap>::try_from_bytes(bm.to_bytes().as_ref()),
            Some(bm)
        );
        assert_eq!(<U256 as Bitmap>::try_from_bytes(&[0u8; 8]), None);
        assert_eq!(<U256 as Bitmap>::try_from_bytes(&[0u8; 31]), None);
    }

    #[test]
    fn select_spans_limbs_inverse_of_rank() {
        let bm = U256::ZERO
            .with_bit(0)
            .with_bit(127)
            .with_bit(128)
            .with_bit(255);
        assert_eq!(bm.select(0), Some(0));
        assert_eq!(bm.select(1), Some(127));
        assert_eq!(bm.select(2), Some(128));
        assert_eq!(bm.select(3), Some(255));
        assert_eq!(bm.select(4), None);
        for i in bm.bits() {
            assert_eq!(bm.select(bm.rank(i)), Some(i));
        }
    }

    #[test]
    fn nearest_clear_queries_u256() {
        // Dense run 126,127,128,129 set (spans the limb boundary), rest clear.
        let bm = U256::ZERO
            .with_bit(126)
            .with_bit(127)
            .with_bit(128)
            .with_bit(129);
        assert_eq!(
            bm.nearest_clear_at_or_below(129).map(Niche::as_usize),
            Some(125)
        );
        assert_eq!(
            bm.nearest_clear_at_or_below(128).map(Niche::as_usize),
            Some(125)
        );
        assert_eq!(
            bm.nearest_clear_in(126, 200).map(Niche::as_usize),
            Some(130)
        );
        assert_eq!(bm.nearest_clear_in(126, 130), None); // [126,130) fully set

        // Top-index (bit 255) boundary: bits 0..=254 set, 255 clear.
        let mut top_clear = U256::ZERO;
        for i in 0u8..=254 {
            top_clear = top_clear.with_bit(i);
        }
        assert_eq!(
            top_clear
                .nearest_clear_at_or_below(255)
                .map(Niche::as_usize),
            Some(255)
        );
        assert_eq!(
            top_clear.nearest_clear_in(0, 256).map(Niche::as_usize),
            Some(255)
        );

        // Fully set: no clear bit anywhere.
        let mut all = U256::ZERO;
        for i in 0u8..=255 {
            all = all.with_bit(i);
        }
        assert_eq!(all.nearest_clear_at_or_below(255), None);
        assert_eq!(all.nearest_clear_in(0, 256), None);
    }

    #[test]
    fn u256_is_hash() {
        use core::hash::Hash;
        use core::hash::Hasher;

        // Minimal no_std hasher: XOR-folds written bytes.
        #[derive(Default)]
        struct XorHasher(u64);
        impl Hasher for XorHasher {
            fn finish(&self) -> u64 {
                self.0
            }
            fn write(&mut self, bytes: &[u8]) {
                for &b in bytes {
                    self.0 = self.0.rotate_left(8) ^ u64::from(b);
                }
            }
        }

        fn hash_of(v: U256) -> u64 {
            let mut h = XorHasher::default();
            v.hash(&mut h);
            h.finish()
        }

        let a = U256::ZERO.with_bit(3).with_bit(200);
        let b = U256::ZERO.with_bit(3).with_bit(200);
        let c = U256::ZERO.with_bit(4);
        assert_eq!(hash_of(a), hash_of(b)); // equal values hash equally
        assert_ne!(hash_of(a), hash_of(c)); // different values differ here
    }

    extern crate alloc;
}
