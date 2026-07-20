//! The [`Arity`] trait and its marker types, wiring an index type, a bitmap
//! backing, and a `hybrid-array` size together for each supported width.

use arity_bitmap::Bitmap;
use arity_index::Niche;
use hybrid_array::ArraySize;
#[cfg(feature = "8")]
use hybrid_array::typenum::U8;
#[cfg(feature = "16")]
use hybrid_array::typenum::U16;
#[cfg(feature = "32")]
use hybrid_array::typenum::U32;
#[cfg(feature = "64")]
use hybrid_array::typenum::U64;
#[cfg(feature = "128")]
use hybrid_array::typenum::U128;
#[cfg(feature = "256")]
use hybrid_array::typenum::U256;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
use hybrid_array::typenum::Unsigned;

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
    /// The `hybrid-array` size used by [`FixedArray`](crate::FixedArray) (a
    /// typenum equal to `LEN`).
    ///
    /// `hybrid-array` / `typenum` is an acknowledged sunset dependency: this
    /// associated type exists only because stable Rust cannot write
    /// `[T; A::LEN]` with `LEN` a trait associated `const`
    /// (`generic_const_exprs` is unstable). Its public exposure is kept small —
    /// [`FixedArray`](crate::FixedArray) exposes `Deref<Target = [T]>` /
    /// `AsRef<[T]>` for element access rather than naming `Array`, and its
    /// iterators are all named crate types — leaving the `ArraySize` bound on
    /// this associated type (`type Size: ArraySize`) as the one point where a
    /// `hybrid-array`/`typenum` name reaches the public surface.
    ///
    /// [`Arity`] is sealed, so no downstream impl can name `Size` to satisfy
    /// it; the residual exposure is code that names `<A as Arity>::Size`
    /// explicitly. Retiring the dependency (once `generic_const_exprs` lets the
    /// storage become `[T; A::LEN]`) is therefore low-impact, though removing
    /// this associated type would still be a breaking change.
    type Size: ArraySize;
}

#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
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

#[cfg(feature = "8")]
arity!(Arity8, 8, arity_index::U3, u8, U8);
#[cfg(feature = "16")]
arity!(Arity16, 16, arity_index::U4, u16, U16);
#[cfg(feature = "32")]
arity!(Arity32, 32, arity_index::U5, u32, U32);
#[cfg(feature = "64")]
arity!(Arity64, 64, arity_index::U6, u64, U64);
#[cfg(feature = "128")]
arity!(Arity128, 128, arity_index::U7, u128, U128);
#[cfg(feature = "256")]
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
