//! Niche integer index types `U3`–`U7`, the [`Niche`] trait, and the `u8`
//! arity-256 index.

use crate::sealed::Sealed;

/// The error returned by `TryFrom<u8>` for a niche integer when the value is out
/// of range. Mirrors [`core::num::TryFromIntError`], which has no public
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TryFromIntError;

impl core::fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of range integral type conversion attempted")
    }
}

impl core::error::Error for TryFromIntError {}

/// A fixed-domain integer index whose value is always `< COUNT`.
///
/// Sealed: implemented only by `U3`–`U7` and `u8` (the arity-256 index).
pub trait Niche: Copy + Ord + Sized + Sealed {
    /// Number of valid values (`2^BITS`): 8, 16, 32, 64, 128, or 256.
    const COUNT: usize;

    /// Returns the value as a `usize`, always `< COUNT`.
    fn as_usize(self) -> usize;

    /// Constructs from a `usize`, or `None` if `i >= COUNT`.
    fn try_from_usize(i: usize) -> Option<Self>;
}

/// Placeholder index type for arity 8 — implemented in a later task.
pub enum U3 {}

/// Placeholder index type for arity 16 — implemented in a later task.
pub enum U4 {}

/// Placeholder index type for arity 32 — implemented in a later task.
pub enum U5 {}

/// Placeholder index type for arity 64 — implemented in a later task.
pub enum U6 {}

/// Placeholder index type for arity 128 — implemented in a later task.
pub enum U7 {}
