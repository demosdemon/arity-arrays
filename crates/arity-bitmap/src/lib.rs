#![no_std]
#![forbid(unsafe_code)]

//! Fixed-width bitmaps indexed by [`arity_index`] niche integers, with a
//! double-ended iterator over the set bits.
//!
//! The [`Bitmap`] trait is implemented for `u8`, `u16`, `u32`, `u64`, `u128`
//! (indexed by `U3`–`U7`) and the 256-bit [`U256`] (indexed by `u8`). The crate
//! contains no `unsafe` code: every bit position is reconstructed through the
//! statically-bounded [`arity_index::Niche`] index.
//!
//! ```
//! # extern crate alloc;
//! use arity_bitmap::Bitmap;
//! use arity_index::{Niche, U4};
//!
//! let bm = u16::ZERO
//!     .with_bit(U4::new_masked(1))
//!     .with_bit(U4::new_masked(4))
//!     .with_bit(U4::new_masked(9));
//!
//! assert_eq!(bm.count_ones(), 3);
//! assert!(bm.test(U4::new_masked(4)));
//! assert_eq!(bm.rank(U4::new_masked(4)), 1); // one set bit below index 4
//!
//! let set: alloc::vec::Vec<u8> = bm.bits().map(U4::as_u8).collect();
//! assert_eq!(set, alloc::vec![1, 4, 9]);
//! ```

mod iter;
mod native;
#[cfg(feature = "256")]
mod u256;

use arity_index::Niche;
pub use iter::BitIter;
#[cfg(feature = "256")]
#[doc(hidden)]
pub use u256::U256;

/// Panic message shared by every `Bitmap::{from,to}_le_bytes` implementation
/// when the byte buffer length does not equal [`Bitmap::BYTES`]. Owning the
/// message — rather than leaning on `copy_from_slice`'s internal wording —
/// keeps it uniform across backings (the two-limb `U256` would otherwise report
/// a slice-range panic for a buffer shorter than one limb) and stable for the
/// byte-length contract tests.
///
/// Gated to the same set of width features as the `Bitmap` impls that reference
/// it: with no width feature selected the crate implements `Bitmap` for
/// nothing, so the constant would otherwise be dead code.
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
pub(crate) const BYTE_LEN_PANIC_MSG: &str = "byte buffer length must equal Bitmap::BYTES";

/// Seals [`Bitmap`](crate::Bitmap) against downstream implementations.
trait Sealed {}

/// Crate-internal bit-scanning mechanics used by
/// [`BitIter`](crate::BitIter).
///
/// Declared in this private module so it is unnameable/uncallable outside
/// the crate. It is a *supertrait* of [`Bitmap`](crate::Bitmap), so
/// every `Bitmap` implies these mechanics — which is what lets
/// `Bitmap::bits()` be called from generic downstream code. It returns
/// raw `usize` bit positions (not the index type) to avoid a
/// `Raw`/`Bitmap` cycle; `BitIter` reconstructs the typed index.
///
/// `raw_lowest_pos`/`raw_highest_pos` have the precondition
/// `!self.raw_is_zero()` and return a position `< WIDTH`.
/// `raw_clear_lowest`/`raw_clear_highest` are total: a zero bitmap is
/// returned unchanged (zero), so they need no precondition.
trait Raw: Sealed + Copy + Eq {
    fn raw_is_zero(self) -> bool;
    fn raw_popcount(self) -> u32;
    fn raw_lowest_pos(self) -> usize;
    fn raw_highest_pos(self) -> usize;
    #[must_use]
    fn raw_clear_lowest(self) -> Self;
    #[must_use]
    fn raw_clear_highest(self) -> Self;
    /// Returns the bit position (`< WIDTH`) of the `n`-th set bit (0-based), or
    /// `None` if `n >= raw_popcount()`. Runs in `O(log WIDTH)` per limb (a
    /// popcount-guided binary search). Every backend implements this directly;
    /// there is no `O(n)` fallback.
    fn raw_select(self, n: u32) -> Option<usize>;
}

/// A fixed-width bitmap addressed by a [`Niche`] index type.
///
/// Sealed: implemented only by `u8`/`u16`/`u32`/`u64`/`u128` and [`U256`].
///
/// [`Niche`]: arity_index::Niche
#[expect(
    private_bounds,
    reason = "Raw and Sealed are intentionally private supertraits: they seal \
              Bitmap against downstream impls and keep the bit-scanning mechanics \
              off the public API, while still being implied by `B: Bitmap` so \
              `bits()` is callable from generic downstream code"
)]
pub trait Bitmap: Copy + Eq + Raw {
    /// The niche index type; `Index::COUNT == WIDTH`.
    type Index: Niche;
    /// The number of bits (`8`, `16`, `32`, `64`, `128`, or `256`).
    const WIDTH: usize;
    /// The number of bytes in the little-endian byte form (`WIDTH / 8`).
    const BYTES: usize = Self::WIDTH / 8;
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
    /// Returns the number of set bits strictly below `i` (the dense rank of
    /// `i`).
    fn rank(self, i: Self::Index) -> u32;
    /// Returns `self` with the bit at `i` cleared (the inverse of
    /// [`with_bit`](Bitmap::with_bit)). Clearing an unset bit is a no-op.
    #[must_use]
    fn without_bit(self, i: Self::Index) -> Self;
    /// Returns the index of the `n`-th set bit (0-based), or `None` if
    /// `n >= count_ones()`. The inverse of [`rank`](Bitmap::rank):
    /// `select(rank(i)) == Some(i)` for every set `i`.
    ///
    /// Runs in `O(log WIDTH)` per limb.
    fn select(self, n: u32) -> Option<Self::Index> {
        let pos = self.raw_select(n)?;
        // `raw_select` yields `pos < WIDTH == Self::Index::COUNT`, so the
        // reconstruction is always `Some`.
        Some(
            <Self::Index as Niche>::try_from_usize(pos)
                .expect("raw_select yields a position < WIDTH == Index::COUNT"),
        )
    }
    /// Writes the bitmap as `BYTES` little-endian bytes into `buf`.
    ///
    /// `buf.len()` must equal [`BYTES`](Bitmap::BYTES); a wrong length panics.
    /// The byte form is backing-independent — it does not depend on the limb
    /// layout of any particular backing.
    fn to_le_bytes(self, buf: &mut [u8]);
    /// Reads a bitmap from `BYTES` little-endian bytes.
    ///
    /// `buf.len()` must equal [`BYTES`](Bitmap::BYTES); a wrong length panics.
    fn from_le_bytes(buf: &[u8]) -> Self;
    /// Iterates over the set bits, ascending, as a double-ended iterator.
    fn bits(self) -> BitIter<Self> {
        BitIter::new(self)
    }
}

/// The crate README's usage example, compiled as a doctest so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
