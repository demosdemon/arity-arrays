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

/// Generates a niche integer newtype `$name` over a fieldless enum `$repr` with
/// `$count == 2^$bits` variants.
macro_rules! niche_int {
    ($name:ident, $repr:ident, $bits:literal, $count:literal) => {
        ::seq_macro::seq!(N in 0..$count {
            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
            enum $repr {
                #( V~N, )*
            }
        });

        #[doc = concat!("A ", stringify!($bits), "-bit unsigned integer index (`0..", stringify!($count), "`).")]
        ///
        /// Backed by a fieldless enum, so `Option<Self>` is one byte and indexing
        #[doc = concat!("a ", stringify!($count), "-element array can elide the bounds check.")]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            /// Number of bits in the value's domain.
            pub const BITS: u32 = $bits;
            /// Number of valid values (`2^BITS`).
            pub const COUNT: usize = $count;
            /// The smallest value (`0`).
            pub const MIN: Self = Self($repr::V0);
            /// The largest value (`COUNT - 1`).
            #[expect(
                clippy::cast_possible_truncation,
                reason = "count is 8, 16, 32, 64, or 128 — each fits in u8"
            )]
            pub const MAX: Self = {
                // SAFETY: `COUNT - 1 < COUNT`, a valid discriminant.
                unsafe { Self::new_unchecked(($count - 1) as u8) }
            };

            ::seq_macro::seq!(N in 0..$count {
                /// Constructs from a `u8`, or `None` if `v >= COUNT`.
                #[must_use]
                pub const fn try_new(v: u8) -> Option<Self> {
                    match v {
                        #( N => Some(Self($repr::V~N)), )*
                        _ => None,
                    }
                }
            });

            /// Constructs without checking that `v < COUNT`.
            ///
            /// # Safety
            ///
            /// The caller must ensure `v < COUNT`.
            #[must_use]
            pub const unsafe fn new_unchecked(v: u8) -> Self {
                debug_assert!((v as usize) < Self::COUNT);
                match Self::try_new(v) {
                    Some(x) => x,
                    // SAFETY: the caller guarantees `v < COUNT`, so `try_new` is `Some`.
                    None => unsafe { ::core::hint::unreachable_unchecked() },
                }
            }

            /// Constructs from the low `BITS` bits of `v` (ignores the rest).
            #[must_use]
            #[expect(
                clippy::cast_possible_truncation,
                reason = "count is 8, 16, 32, 64, or 128 — each fits in u8"
            )]
            pub const fn new_masked(v: u8) -> Self {
                // SAFETY: masking by `COUNT - 1` (a power of two minus one) yields a
                // value `< COUNT`.
                unsafe { Self::new_unchecked(v & (($count - 1) as u8)) }
            }

            /// Returns the value as a `u8`.
            #[must_use]
            pub const fn as_u8(self) -> u8 {
                self.0 as u8
            }

            /// Returns the value as a `usize`.
            #[must_use]
            pub const fn as_usize(self) -> usize {
                self.0 as usize
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::MIN
            }
        }

        impl Sealed for $name {}

        impl Niche for $name {
            const COUNT: usize = $count;

            fn as_usize(self) -> usize {
                self.0 as usize
            }

            fn try_from_usize(i: usize) -> Option<Self> {
                match u8::try_from(i) {
                    Ok(v) => Self::try_new(v),
                    Err(_) => None,
                }
            }
        }
    };
}

niche_int!(U3, Repr3, 3, 8);
niche_int!(U4, Repr4, 4, 16);
niche_int!(U5, Repr5, 5, 32);
niche_int!(U6, Repr6, 6, 64);
niche_int!(U7, Repr7, 7, 128);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_accessors() {
        assert_eq!(U4::COUNT, 16);
        assert_eq!(U4::BITS, 4);
        assert_eq!(U4::MIN.as_u8(), 0);
        assert_eq!(U4::MAX.as_u8(), 15);
        assert_eq!(U4::MAX.as_usize(), 15);

        assert_eq!(U4::try_new(0), Some(U4::MIN));
        assert_eq!(U4::try_new(15), Some(U4::MAX));
        assert_eq!(U4::try_new(16), None);
        assert_eq!(U4::try_new(255), None);

        // new_masked keeps the low BITS bits.
        assert_eq!(U4::new_masked(0xF3).as_u8(), 0x3);
        assert_eq!(U3::new_masked(0xFF).as_u8(), 0x7);
    }

    #[test]
    fn domain_bounds_per_type() {
        assert_eq!(U3::COUNT, 8);
        assert_eq!(U5::COUNT, 32);
        assert_eq!(U6::COUNT, 64);
        assert_eq!(U7::COUNT, 128);
        assert_eq!(U7::MAX.as_usize(), 127);
        assert_eq!(U7::try_new(128), None);
        assert_eq!(U7::try_new(127), Some(U7::MAX));
    }

    #[test]
    fn option_is_one_byte() {
        assert_eq!(core::mem::size_of::<Option<U3>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U4>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U5>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U6>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U7>>(), 1);
    }

    #[test]
    fn default_is_min() {
        assert_eq!(U4::default(), U4::MIN);
        assert_eq!(U7::default().as_u8(), 0);
    }

    #[test]
    fn niche_trait_round_trip() {
        fn round_trip<N: Niche + core::fmt::Debug + PartialEq>(count: usize) {
            assert_eq!(N::COUNT, count);
            assert!(N::try_from_usize(count).is_none());
            let last = N::try_from_usize(count - 1);
            assert!(last.is_some());
            if let Some(v) = last {
                assert_eq!(v.as_usize(), count - 1);
            }
        }
        round_trip::<U3>(8);
        round_trip::<U4>(16);
        round_trip::<U7>(128);
    }
}
