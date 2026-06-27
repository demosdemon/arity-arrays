#![no_std]

//! Bounds-check-free niche integer index types.
//!
//! Each `U{n}` (`U3`–`U7`) is a newtype over a fieldless enum with `2ⁿ`
//! variants, so `Option<U{n}>` is one byte (niche optimization) and indexing a
//! `2ⁿ`-length array can elide the bounds check. The [`Niche`] trait unifies the
//! index types (including the native `u8` for arity 256); iteration over a
//! type's whole domain is via [`NicheRange`] / [`NicheRangeInclusive`].

mod niche;
mod range;

pub use niche::{Niche, TryFromIntError, U3, U4, U5, U6, U7};
pub use range::{NicheRange, NicheRangeInclusive};

mod sealed {
    /// Prevents downstream crates from implementing [`Niche`](crate::Niche).
    pub trait Sealed {}
}
