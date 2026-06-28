#![no_std]

//! Bounds-check-free niche integer index types.
//!
//! Each `U{n}` (`U3`–`U7`) is a newtype over a fieldless enum with `2ⁿ`
//! variants, so `Option<U{n}>` is one byte (niche optimization) and indexing a
//! `2ⁿ`-length array can elide the bounds check. The [`Niche`] trait unifies
//! the index types (including the native `u8` for arity 256); iteration over a
//! type's whole domain is via [`NicheRange`] / [`NicheRangeInclusive`].
//!
//! ```
//! # extern crate alloc;
//! use arity_index::{Niche, U4, NicheRange};
//!
//! // The whole domain, ascending:
//! let all: alloc::vec::Vec<u8> = U4::all().map(U4::as_u8).collect();
//! assert_eq!(all.len(), 16);
//!
//! // A sub-range, double-ended:
//! let mut r = NicheRange::new(U4::new_masked(1), U4::new_masked(4));
//! assert_eq!(r.next().map(U4::as_u8), Some(1));
//! assert_eq!(r.next_back().map(U4::as_u8), Some(3));
//! ```

mod niche;
mod range;

pub use niche::Niche;
pub use niche::TryFromIntError;
#[cfg(feature = "8")]
pub use niche::U3;
#[cfg(feature = "16")]
pub use niche::U4;
#[cfg(feature = "32")]
pub use niche::U5;
#[cfg(feature = "64")]
pub use niche::U6;
#[cfg(feature = "128")]
pub use niche::U7;
pub use range::NicheRange;
pub use range::NicheRangeInclusive;

/// Prevents downstream crates from implementing [`Niche`](crate::Niche).
trait Sealed {}
