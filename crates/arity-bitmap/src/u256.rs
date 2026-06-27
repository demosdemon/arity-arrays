//! The 256-bit bitmap backing (`Bitmap::Index == u8`). Pure safe code.

use arity_index::Niche;

use crate::Bitmap;
use crate::sealed::Raw;
use crate::sealed::Sealed;

/// A 256-bit bitmap: bit `i` lives in `lo` for `i < 128`, else in `hi` at
/// `i - 128`. Only the [`Bitmap`] surface is implemented (no arithmetic).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct U256 {
    lo: u128,
    hi: u128,
}

// Wire-up invariant: the u8 index domain (256) must equal the bit width.
const _: () = assert!(<u8 as Niche>::COUNT == 256);

impl U256 {
    /// Splits a bit index `i` (`< 256`) into `(limb_is_hi, bit_within_limb)`.
    ///
    /// Accepts `u8` so the casts to `u32` are lossless widening conversions.
    const fn split(i: u8) -> (bool, u32) {
        if i < 128 {
            (false, i as u32)
        } else {
            (true, (i - 128) as u32)
        }
    }
}

impl Sealed for U256 {}

impl Raw for U256 {
    fn raw_is_zero(self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    fn raw_popcount(self) -> u32 {
        self.lo.count_ones() + self.hi.count_ones()
    }

    fn raw_lowest_pos(self) -> usize {
        if self.lo != 0 {
            self.lo.trailing_zeros() as usize
        } else {
            128 + self.hi.trailing_zeros() as usize
        }
    }

    fn raw_highest_pos(self) -> usize {
        if self.hi != 0 {
            128 + self.hi.ilog2() as usize
        } else {
            self.lo.ilog2() as usize
        }
    }

    fn raw_clear_lowest(self) -> Self {
        if self.lo != 0 {
            Self {
                lo: self.lo & self.lo.wrapping_sub(1),
                hi: self.hi,
            }
        } else {
            Self {
                lo: 0,
                hi: self.hi & self.hi.wrapping_sub(1),
            }
        }
    }

    fn raw_clear_highest(self) -> Self {
        if self.hi != 0 {
            Self {
                lo: self.lo,
                hi: self.hi & !(1u128 << self.hi.ilog2()),
            }
        } else if self.lo != 0 {
            Self {
                lo: self.lo & !(1u128 << self.lo.ilog2()),
                hi: 0,
            }
        } else {
            self
        }
    }
}

impl Bitmap for U256 {
    type Index = u8;
    const WIDTH: usize = 256;
    const ZERO: Self = Self { lo: 0, hi: 0 };

    fn is_zero(self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    fn count_ones(self) -> u32 {
        self.lo.count_ones() + self.hi.count_ones()
    }

    fn test(self, i: u8) -> bool {
        let (is_hi, bit) = Self::split(i);
        let limb = if is_hi { self.hi } else { self.lo };
        limb & (1u128 << bit) != 0
    }

    fn with_bit(self, i: u8) -> Self {
        let (is_hi, bit) = Self::split(i);
        if is_hi {
            Self {
                lo: self.lo,
                hi: self.hi | (1u128 << bit),
            }
        } else {
            Self {
                lo: self.lo | (1u128 << bit),
                hi: self.hi,
            }
        }
    }

    fn rank(self, i: u8) -> u32 {
        let (is_hi, bit) = Self::split(i);
        if is_hi {
            // all of lo, plus the bits of hi below `bit`
            let hi_mask = (1u128 << bit) - 1;
            self.lo.count_ones() + (self.hi & hi_mask).count_ones()
        } else {
            let lo_mask = (1u128 << bit) - 1;
            (self.lo & lo_mask).count_ones()
        }
    }
}

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

    extern crate alloc;
}
