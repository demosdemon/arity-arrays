//! `Bitmap`/`Raw` impls for the native unsigned integers `u8`..`u128`.

#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use arity_index::Niche;
#[cfg(feature = "8")]
use arity_index::U3;
#[cfg(feature = "16")]
use arity_index::U4;
#[cfg(feature = "32")]
use arity_index::U5;
#[cfg(feature = "64")]
use arity_index::U6;
#[cfg(feature = "128")]
use arity_index::U7;

#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Bitmap;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Raw;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Sealed;

/// Implements `Sealed` + `Raw` + `Bitmap` for a native unsigned integer `$ty`
/// (width `$width`) indexed by niche type `$idx` (with `$idx::COUNT ==
/// $width`).
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
macro_rules! impl_native_bitmap {
    ($ty:ty, $idx:ty, $width:literal) => {
        // Wire-up invariant: the index domain must equal the bit width.
        const _: () = assert!(<$idx as Niche>::COUNT == $width);

        impl Sealed for $ty {}

        #[expect(
            unsafe_code,
            reason = "unsafe impl asserts the Raw bit-position contract; the \
                      impl body performs no unsafe operations"
        )]
        // SAFETY: raw_popcount returns a count <= WIDTH; raw_select/
        // raw_lowest_pos/raw_highest_pos return positions < WIDTH;
        // raw_clear_lowest/raw_clear_highest clear exactly the named bit. The
        // Raw contract holds for every native integer width.
        unsafe impl Raw for $ty {
            #[inline(always)]
            fn raw_is_zero(self) -> bool {
                self == 0
            }

            #[inline(always)]
            fn raw_popcount(self) -> u32 {
                self.count_ones()
            }

            #[inline(always)]
            fn raw_lowest_pos(self) -> usize {
                self.trailing_zeros() as usize
            }

            #[inline]
            fn raw_highest_pos(self) -> usize {
                self.ilog2() as usize
            }

            #[inline]
            fn raw_clear_lowest(self) -> Self {
                self & self.wrapping_sub(1)
            }

            #[inline]
            fn raw_clear_highest(self) -> Self {
                if self == 0 {
                    0
                } else {
                    self & !(1 << self.ilog2())
                }
            }

            #[inline]
            fn raw_select(self, n: u32) -> Option<usize> {
                if n >= self.raw_popcount() {
                    return None;
                }
                // Popcount-guided binary search over the limb: at each step,
                // compare `n` against the popcount of the low `size` bits; if it
                // lies above, skip them and accumulate `size` into `pos`. `size`
                // starts at WIDTH/2 so no shift ever reaches the type width.
                let mut n = n;
                let mut x = self;
                let mut pos = 0usize;
                let mut size: u32 = ($width / 2) as u32;
                loop {
                    let lo_mask = ((1 as $ty) << size).wrapping_sub(1);
                    let lo_count = (x & lo_mask).count_ones();
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
                debug_assert!(from < $width);
                // Complement, masked to bits [0, from] inclusive. `from + 1`
                // can equal WIDTH, which would overflow the shift, so saturate.
                let mask: $ty = if from + 1 >= $width {
                    !0
                } else {
                    ((1 as $ty) << (from + 1)).wrapping_sub(1)
                };
                let holes = !self & mask;
                if holes == 0 {
                    None
                } else {
                    // Highest set bit of `holes` = greatest clear bit <= from.
                    Some(holes.ilog2() as usize)
                }
            }

            #[inline]
            fn raw_nearest_clear_in(self, from: usize, limit: usize) -> Option<usize> {
                debug_assert!(from <= limit && limit <= $width);
                // Bits [from, limit): the low-`limit` mask minus the low-`from`
                // mask. Either bound can equal WIDTH; saturate to avoid the
                // overflowing shift.
                let low_limit: $ty = if limit >= $width {
                    !0
                } else {
                    ((1 as $ty) << limit).wrapping_sub(1)
                };
                let low_from: $ty = if from >= $width {
                    !0
                } else {
                    ((1 as $ty) << from).wrapping_sub(1)
                };
                let holes = !self & low_limit & !low_from;
                if holes == 0 {
                    None
                } else {
                    // Lowest set bit of `holes` = least clear bit >= from.
                    Some(holes.trailing_zeros() as usize)
                }
            }
        }

        impl Bitmap for $ty {
            type Index = $idx;
            const WIDTH: usize = $width;
            const ZERO: Self = 0;

            #[inline(always)]
            fn is_zero(self) -> bool {
                self == 0
            }

            #[inline(always)]
            fn count_ones(self) -> u32 {
                <$ty>::count_ones(self)
            }

            #[inline]
            fn test(self, i: $idx) -> bool {
                self & (1 << i.as_usize()) != 0
            }

            #[inline]
            fn with_bit(self, i: $idx) -> Self {
                self | (1 << i.as_usize())
            }

            #[inline]
            fn rank(self, i: $idx) -> u32 {
                let below = (1 << i.as_usize()) - 1;
                (self & below).count_ones()
            }

            #[inline]
            fn without_bit(self, i: $idx) -> Self {
                self & !(1 << i.as_usize())
            }

            type Bytes = [u8; $width / 8];

            #[inline]
            fn to_bytes(self) -> Self::Bytes {
                <$ty>::to_le_bytes(self)
            }

            #[inline]
            fn from_bytes(bytes: Self::Bytes) -> Self {
                <$ty>::from_le_bytes(bytes)
            }
        }
    };
}

#[cfg(feature = "8")]
impl_native_bitmap!(u8, U3, 8);
#[cfg(feature = "16")]
impl_native_bitmap!(u16, U4, 16);
#[cfg(feature = "32")]
impl_native_bitmap!(u32, U5, 32);
#[cfg(feature = "64")]
impl_native_bitmap!(u64, U6, 64);
#[cfg(feature = "128")]
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

    #[test]
    fn without_bit_clears_one_bit() {
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(9));
        let cleared = bm.without_bit(u4(4));
        assert!(!cleared.test(u4(4)));
        assert!(cleared.test(u4(1)));
        assert!(cleared.test(u4(9)));
        assert_eq!(cleared.count_ones(), 2);
        // Clearing an unset bit is a no-op.
        assert_eq!(bm.without_bit(u4(2)), bm);
    }

    #[test]
    fn select_in_word_all_widths() {
        // Exhaustive single-limb check: for every set bit, select(rank(i)) == i,
        // and select past the popcount is None.
        let bm = u8::ZERO
            .with_bit(U3::new_masked(0))
            .with_bit(U3::new_masked(3))
            .with_bit(U3::new_masked(7));
        assert_eq!(bm.select(0).map(U3::as_u8), Some(0));
        assert_eq!(bm.select(1).map(U3::as_u8), Some(3));
        assert_eq!(bm.select(2).map(U3::as_u8), Some(7));
        assert_eq!(bm.select(3), None);

        // u128: bits at both ends and the middle.
        let b = u128::ZERO
            .with_bit(U7::new_masked(0))
            .with_bit(U7::new_masked(64))
            .with_bit(U7::new_masked(127));
        assert_eq!(b.select(0).map(U7::as_u8), Some(0));
        assert_eq!(b.select(1).map(U7::as_u8), Some(64));
        assert_eq!(b.select(2).map(U7::as_u8), Some(127));
        assert_eq!(b.select(3), None);
        // select is the inverse of rank for every set bit.
        for i in b.bits() {
            assert_eq!(b.select(b.rank(i)), Some(i));
        }
    }

    #[test]
    fn select_is_inverse_of_rank() {
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(9));
        assert_eq!(bm.select(0).map(U4::as_u8), Some(1));
        assert_eq!(bm.select(1).map(U4::as_u8), Some(4));
        assert_eq!(bm.select(2).map(U4::as_u8), Some(9));
        assert_eq!(bm.select(3), None);
        // select(rank(i)) == i for every set bit i.
        for i in bm.bits() {
            assert_eq!(bm.select(bm.rank(i)), Some(i));
        }
    }

    #[test]
    fn nearest_clear_queries_native() {
        // Bits 1,2,3 set (a dense run), the rest clear.
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(2)).with_bit(u4(3));

        // at_or_below: greatest clear index <= from.
        assert_eq!(bm.nearest_clear_at_or_below(3).map(U4::as_usize), Some(0));
        assert_eq!(bm.nearest_clear_at_or_below(2).map(U4::as_usize), Some(0));
        assert_eq!(bm.nearest_clear_at_or_below(0).map(U4::as_usize), Some(0));

        // in [from, limit): least clear index >= from and < limit.
        assert_eq!(bm.nearest_clear_in(1, 6).map(U4::as_usize), Some(4));
        assert_eq!(bm.nearest_clear_in(4, 6).map(U4::as_usize), Some(4));
        assert_eq!(bm.nearest_clear_in(1, 4), None); // 1,2,3 set → [1,4) full
        assert_eq!(bm.nearest_clear_in(3, 3), None); // empty range

        // Fully set: no clear bit anywhere.
        assert_eq!((!0u16).nearest_clear_at_or_below(15), None);
        assert_eq!((!0u16).nearest_clear_in(0, 16), None);

        // Top-index (from + 1 == WIDTH) boundary: only bit 15 clear.
        let top_clear = !(1u16 << 15);
        assert_eq!(
            top_clear.nearest_clear_at_or_below(15).map(U4::as_usize),
            Some(15)
        );
        assert_eq!(
            top_clear.nearest_clear_in(0, 16).map(U4::as_usize),
            Some(15)
        );

        // u8 top-index boundary.
        let b = !(1u8 << 7);
        assert_eq!(b.nearest_clear_at_or_below(7).map(U3::as_usize), Some(7));
    }

    #[test]
    fn le_bytes_round_trip_native() {
        assert_eq!(<u16 as Bitmap>::BYTES, 2);
        assert_eq!(<u8 as Bitmap>::BYTES, 1);
        assert_eq!(<u128 as Bitmap>::BYTES, 16);

        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(9));
        let bytes = <u16 as Bitmap>::to_bytes(bm);
        assert_eq!(bytes, 0b0000_0010_0000_0010u16.to_le_bytes());
        assert_eq!(<u16 as Bitmap>::from_bytes(bytes), bm);
    }

    #[test]
    fn try_from_bytes_checks_length_native() {
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(9));
        // Exact length round-trips; every other length is rejected (u16 wants 2).
        assert_eq!(<u16 as Bitmap>::try_from_bytes(&bm.to_bytes()), Some(bm));
        assert_eq!(<u16 as Bitmap>::try_from_bytes(&[0u8; 1]), None);
        assert_eq!(<u16 as Bitmap>::try_from_bytes(&[0u8; 3]), None);
    }

    extern crate alloc;
}
