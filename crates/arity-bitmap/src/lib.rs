#![no_std]

//! Fixed-width bitmaps indexed by [`arity_index`] niche integers, with a
//! double-ended iterator over the set bits.
//!
//! The [`Bitmap`] trait is implemented for `u8`, `u16`, `u32`, `u64`, `u128`
//! (indexed by `U3`–`U7`) and the 256-bit [`U256`] (indexed by `u8`). The crate
//! contains no `unsafe` code: every bit position is reconstructed through the
//! statically-bounded [`arity_index::Niche`] index.

mod iter;

pub use iter::BitIter;

use arity_index::Niche;

mod sealed {
    /// Prevents downstream crates from implementing [`Bitmap`](crate::Bitmap).
    pub trait Sealed {}
}

/// Bit-scanning mechanics used by [`BitIter`]. Kept off the public [`Bitmap`]
/// surface deliberately. The `raw_lowest`/`raw_highest` methods have the
/// precondition `!self.raw_is_zero()`.
///
/// `Raw` is sealed (requires [`sealed::Sealed`]) so it cannot be implemented
/// outside this crate. It is `pub` rather than `pub(crate)` only to satisfy the
/// Rust privacy checker: [`BitIter`]`<B: Raw>` implementing the standard
/// [`Iterator`] trait requires its `Item` type to be reachable, which forces
/// `Raw` (and `Raw::Index`) to be nameable. The `#[doc(hidden)]` attribute keeps
/// it off the public documentation surface.
#[doc(hidden)]
pub trait Raw: Copy + Eq + sealed::Sealed {
    type Index: Niche;
    fn raw_is_zero(self) -> bool;
    fn raw_popcount(self) -> u32;
    fn raw_lowest(self) -> Self::Index;
    fn raw_highest(self) -> Self::Index;
    #[must_use]
    fn raw_clear_lowest(self) -> Self;
    #[must_use]
    fn raw_clear_highest(self) -> Self;
}

/// A fixed-width bitmap addressed by a [`Niche`] index type.
///
/// Sealed: implemented only by `u8`/`u16`/`u32`/`u64`/`u128` and [`U256`].
pub trait Bitmap: Copy + Eq + sealed::Sealed {
    /// The niche index type; `Index::COUNT == WIDTH`.
    type Index: Niche;
    /// The number of bits (`8`, `16`, `32`, `64`, `128`, or `256`).
    const WIDTH: usize;
    /// The empty bitmap.
    const ZERO: Self;

    /// Returns `true` if no bit is set.
    fn is_zero(self) -> bool;
    /// Returns the number of set bits.
    fn count_ones(self) -> u32;
    /// Returns `true` if the bit at `i` is set.
    fn test(self, i: Self::Index) -> bool;
    /// Returns `self` with the bit at `i` set.
    #[must_use]
    fn with_bit(self, i: Self::Index) -> Self;
    /// Returns the number of set bits strictly below `i` (the dense rank of `i`).
    fn rank(self, i: Self::Index) -> u32;
    /// Iterates over the set bits, ascending, as a double-ended iterator.
    fn bits(self) -> BitIter<Self>
    where
        Self: Raw<Index = <Self as Bitmap>::Index>,
    {
        BitIter::new(self)
    }
}
