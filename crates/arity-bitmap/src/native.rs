//! `Bitmap`/`Raw` impls for the native unsigned integers `u8`..`u128`.

use arity_index::Niche;
use arity_index::U3;
use arity_index::U4;
use arity_index::U5;
use arity_index::U6;
use arity_index::U7;

use crate::Bitmap;
use crate::sealed::Raw;
use crate::sealed::Sealed;

/// Implements `Sealed` + `Raw` + `Bitmap` for a native unsigned integer `$ty`
/// (width `$width`) indexed by niche type `$idx` (with `$idx::COUNT ==
/// $width`).
macro_rules! impl_native_bitmap {
    ($ty:ty, $idx:ty, $width:literal) => {
        // Wire-up invariant: the index domain must equal the bit width.
        const _: () = assert!(<$idx as Niche>::COUNT == $width);

        impl Sealed for $ty {}

        impl Raw for $ty {
            fn raw_is_zero(self) -> bool {
                self == 0
            }

            fn raw_popcount(self) -> u32 {
                self.count_ones()
            }

            fn raw_lowest_pos(self) -> usize {
                self.trailing_zeros() as usize
            }

            fn raw_highest_pos(self) -> usize {
                self.ilog2() as usize
            }

            fn raw_clear_lowest(self) -> Self {
                self & self.wrapping_sub(1)
            }

            fn raw_clear_highest(self) -> Self {
                if self == 0 {
                    0
                } else {
                    self & !(1 << self.ilog2())
                }
            }
        }

        impl Bitmap for $ty {
            type Index = $idx;
            const WIDTH: usize = $width;
            const ZERO: Self = 0;

            fn is_zero(self) -> bool {
                self == 0
            }

            fn count_ones(self) -> u32 {
                <$ty>::count_ones(self)
            }

            fn test(self, i: $idx) -> bool {
                self & (1 << i.as_usize()) != 0
            }

            fn with_bit(self, i: $idx) -> Self {
                self | (1 << i.as_usize())
            }

            fn rank(self, i: $idx) -> u32 {
                let below = (1 << i.as_usize()) - 1;
                (self & below).count_ones()
            }
        }
    };
}

impl_native_bitmap!(u8, U3, 8);
impl_native_bitmap!(u16, U4, 16);
impl_native_bitmap!(u32, U5, 32);
impl_native_bitmap!(u64, U6, 64);
impl_native_bitmap!(u128, U7, 128);

#[cfg(test)]
mod tests {
    use super::*;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn width_and_zero() {
        assert_eq!(<u16 as Bitmap>::WIDTH, 16);
        assert_eq!(<u8 as Bitmap>::WIDTH, 8);
        assert_eq!(<u128 as Bitmap>::WIDTH, 128);
        assert_eq!(<u16 as Bitmap>::ZERO, 0u16);
        assert!(<u16 as Bitmap>::ZERO.is_zero());
        assert_eq!(<u16 as Bitmap>::ZERO.count_ones(), 0);
    }

    #[test]
    fn test_with_bit_and_count() {
        let bm = u16::ZERO.with_bit(u4(0)).with_bit(u4(7)).with_bit(u4(15));
        assert!(bm.test(u4(0)));
        assert!(bm.test(u4(7)));
        assert!(bm.test(u4(15)));
        assert!(!bm.test(u4(1)));
        assert!(!bm.test(u4(8)));
        assert_eq!(bm.count_ones(), 3);
        assert!(!bm.is_zero());
        assert_eq!(bm, 0b1000_0000_1000_0001u16);
    }

    #[test]
    fn rank_is_dense_index() {
        // bits 0, 7, 15 set: rank(0)=0, rank(7)=1, rank(15)=2.
        let bm = u16::ZERO.with_bit(u4(0)).with_bit(u4(7)).with_bit(u4(15));
        assert_eq!(bm.rank(u4(0)), 0);
        assert_eq!(bm.rank(u4(7)), 1);
        assert_eq!(bm.rank(u4(15)), 2);
        // rank counts bits strictly below i, regardless of whether i is set.
        assert_eq!(bm.rank(u4(8)), 2);
        assert_eq!(bm.rank(u4(1)), 1);
    }

    #[test]
    fn bits_forward_and_back() {
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(14));
        let fwd: alloc::vec::Vec<u8> = bm.bits().map(U4::as_u8).collect();
        assert_eq!(fwd, alloc::vec![1, 4, 14]);
        let back: alloc::vec::Vec<u8> = bm.bits().rev().map(U4::as_u8).collect();
        assert_eq!(back, alloc::vec![14, 4, 1]);
        assert_eq!(bm.bits().len(), 3);
        // meet in the middle
        let mut it = bm.bits();
        assert_eq!(it.next().map(U4::as_u8), Some(1));
        assert_eq!(it.next_back().map(U4::as_u8), Some(14));
        assert_eq!(it.next().map(U4::as_u8), Some(4));
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
    }

    #[test]
    fn edge_widths_u8_and_u128() {
        let b8 = u8::ZERO.with_bit(U3::MIN).with_bit(U3::MAX);
        assert_eq!(b8.count_ones(), 2);
        assert_eq!(b8.rank(U3::MAX), 1);
        assert_eq!(b8, 0b1000_0001u8);

        let b128 = u128::ZERO.with_bit(U7::MAX); // bit 127
        assert_eq!(b128.count_ones(), 1);
        assert_eq!(b128.rank(U7::MAX), 0);
        assert_eq!(b128, 1u128 << 127);
        let only: alloc::vec::Vec<u8> = b128.bits().map(U7::as_u8).collect();
        assert_eq!(only, alloc::vec![127]);
    }

    extern crate alloc;
}
