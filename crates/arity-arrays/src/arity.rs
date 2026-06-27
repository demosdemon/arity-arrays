//! The [`Arity`] trait and its marker types, wiring an index type, a bitmap
//! backing, and a `hybrid-array` size together for each supported width.

use arity_bitmap::Bitmap;
use arity_index::Niche;
use hybrid_array::ArraySize;
use hybrid_array::typenum::{U8, U16, U32, U64, U128, U256, Unsigned};

/// A power-of-two arity (8, 16, 32, 64, 128, or 256) that ties together a niche
/// index type, a bitmap backing, and a `hybrid-array` size.
///
/// Sealed: implemented only by the `Arity8` … `Arity256` markers in this crate.
#[expect(
    private_bounds,
    reason = "Sealed is an intentionally private supertrait that seals Arity \
              against downstream implementations"
)]
pub trait Arity: crate::Sealed {
    /// Number of slots.
    const LEN: usize;
    /// The niche index type (`U3`…`U7` or `u8`).
    type Index: Niche;
    /// The bitmap backing, whose `Index` must match `Self::Index`.
    type Bitmap: Bitmap<Index = Self::Index>;
    /// The `hybrid-array` size used by `FixedArray` (a typenum equal to `LEN`).
    type Size: ArraySize;
}

macro_rules! arity {
    ($name:ident, $len:literal, $index:ty, $bitmap:ty, $size:ty) => {
        #[doc = concat!("Arity ", stringify!($len), ".")]
        pub enum $name {}

        impl crate::Sealed for $name {}

        impl Arity for $name {
            const LEN: usize = $len;
            type Index = $index;
            type Bitmap = $bitmap;
            type Size = $size;
        }

        // Wiring invariant: index domain == slot count == bitmap width == size.
        const _: () = assert!(<$index as Niche>::COUNT == $len);
        const _: () = assert!(<$bitmap as Bitmap>::WIDTH == $len);
        const _: () = assert!(<$size as Unsigned>::USIZE == $len);
    };
}

arity!(Arity8, 8, arity_index::U3, u8, U8);
arity!(Arity16, 16, arity_index::U4, u16, U16);
arity!(Arity32, 32, arity_index::U5, u32, U32);
arity!(Arity64, 64, arity_index::U6, u64, U64);
arity!(Arity128, 128, arity_index::U7, u128, U128);
arity!(Arity256, 256, u8, arity_bitmap::U256, U256);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiring_constants() {
        assert_eq!(Arity8::LEN, 8);
        assert_eq!(Arity16::LEN, 16);
        assert_eq!(Arity256::LEN, 256);
        assert_eq!(<Arity16 as Arity>::Size::USIZE, 16);
        assert_eq!(<<Arity16 as Arity>::Index as Niche>::COUNT, 16);
        assert_eq!(<<Arity16 as Arity>::Bitmap as Bitmap>::WIDTH, 16);
        assert_eq!(<<Arity256 as Arity>::Index as Niche>::COUNT, 256);
    }
}
