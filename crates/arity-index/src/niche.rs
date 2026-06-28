//! Niche integer index types `U3`–`U7`, the [`Niche`] trait, and the `u8`
//! arity-256 index.

use crate::Sealed;
use crate::range::NicheRangeInclusive;

/// The error returned by `TryFrom<u8>` for a niche integer when the value is
/// out of range. Mirrors [`core::num::TryFromIntError`], which has no public
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
#[expect(
    private_bounds,
    reason = "Sealed is an intentionally private supertrait that seals Niche \
              against downstream implementations"
)]
pub trait Niche: Copy + Ord + Sized + Sealed {
    /// Number of valid values (`2^BITS`): 8, 16, 32, 64, 128, or 256.
    const COUNT: usize;

    /// Returns the value as a `usize`, always `< COUNT`.
    fn as_usize(self) -> usize;

    /// Constructs from a `usize`, or `None` if `i >= COUNT`.
    fn try_from_usize(i: usize) -> Option<Self>;

    /// Iterates over all values ascending (`MIN..=MAX`) as a double-ended,
    /// exact-size iterator. `len() == COUNT`.
    ///
    /// This range iterator is deliberately the only way to enumerate the
    /// domain: there is no `ALL` constant, so nothing materializes a
    /// `COUNT`-element table (an `[U7; 128]` / `[u8; 256]` const would
    /// otherwise sit in the binary).
    #[must_use]
    fn all() -> NicheRangeInclusive<Self> {
        NicheRangeInclusive::full()
    }
}

/// Generates a niche integer newtype `$name` over a fieldless enum `$repr` with
/// `$count == 2^$bits` variants.
///
/// The fieldless enum gives the compiler a layout with `2^$bits` valid
/// discriminants and the rest as niches, which is what earns both payoffs:
/// `Option<$name>` reuses an unused discriminant for `None` (stays one byte),
/// and a value is statically `< 2^$bits` so array indexing can elide the bounds
/// check. The largest type (`U7`) needs 128 variants — too many to hand-write —
/// so [`seq_macro`](https://docs.rs/seq-macro) generates the variants and match
/// arms. Compile-time expansion keeps a single source of truth (no committed
/// generated `.rs` to drift) at the cost of one build dependency.
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
macro_rules! niche_int {
    ($name:ident, $repr:ident, $bits:literal, $count:literal) => {
        ::seq_macro::seq!(N in 0..$count {
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            #[repr(u8)]
            enum $repr {
                #( V~N, )*
            }
        });

        #[doc = concat!("A ", stringify!($bits), "-bit unsigned integer index (`0..", stringify!($count), "`).")]
        ///
        /// Backed by a fieldless enum, so `Option<Self>` is one byte and indexing
        #[doc = concat!("a ", stringify!($count), "-element array can elide the bounds check.")]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        // The whole point of the type: `Option<Self>` must fit in one byte
        // (niche optimization). Enforced at compile time for every profile.
        const _: () = assert!(::core::mem::size_of::<::core::option::Option<$name>>() == 1);

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

        // `Debug` forwards to the integer value (prints `15`, not `U4(V15)`).
        // The `$repr` enum is deliberately not `Debug`.
        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Debug::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::LowerHex for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::LowerHex::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::UpperHex for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::UpperHex::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::Binary for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Binary::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::convert::TryFrom<u8> for $name {
            type Error = crate::TryFromIntError;

            fn try_from(v: u8) -> ::core::result::Result<Self, Self::Error> {
                Self::try_new(v).ok_or(crate::TryFromIntError)
            }
        }

        impl ::core::convert::From<$name> for u8 {
            fn from(v: $name) -> u8 {
                v.as_u8()
            }
        }

        impl ::core::convert::From<$name> for usize {
            fn from(v: $name) -> usize {
                v.as_usize()
            }
        }

        #[cfg(feature = "serde")]
        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_u8(self.as_u8())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::core::result::Result<Self, D::Error> {
                let v = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::try_new(v).ok_or_else(|| {
                    ::serde::de::Error::invalid_value(
                        ::serde::de::Unexpected::Unsigned(::core::primitive::u64::from(v)),
                        &concat!("an integer in 0..", stringify!($count)),
                    )
                })
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

#[cfg(feature = "8")]
niche_int!(U3, Repr3, 3, 8);
#[cfg(feature = "16")]
niche_int!(U4, Repr4, 4, 16);
#[cfg(feature = "32")]
niche_int!(U5, Repr5, 5, 32);
#[cfg(feature = "64")]
niche_int!(U6, Repr6, 6, 64);
#[cfg(feature = "128")]
niche_int!(U7, Repr7, 7, 128);

#[cfg(feature = "256")]
impl Sealed for u8 {}

#[cfg(feature = "256")]
impl Niche for u8 {
    const COUNT: usize = 256;

    fn as_usize(self) -> usize {
        usize::from(self)
    }

    fn try_from_usize(i: usize) -> Option<Self> {
        // `Self::try_from` succeeds iff `i <= 255`, i.e. `i < COUNT`. No cast.
        Self::try_from(i).ok()
    }
}

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

    #[test]
    fn formatting_and_tryfrom() {
        extern crate alloc;
        use alloc::format;

        assert_eq!(format!("{:?}", U4::MAX), "15");
        assert_eq!(format!("{}", U4::MAX), "15");
        assert_eq!(format!("{:x}", U4::new_masked(10)), "a");
        assert_eq!(format!("{:X}", U4::new_masked(10)), "A");
        assert_eq!(format!("{:b}", U4::new_masked(5)), "101");

        assert_eq!(U4::try_from(7u8), Ok(U4::new_masked(7)));
        assert_eq!(U4::try_from(16u8), Err(TryFromIntError));
    }

    #[test]
    fn u8_is_arity_256_index() {
        assert_eq!(<u8 as Niche>::COUNT, 256);
        assert_eq!(Niche::as_usize(255u8), 255);
        assert_eq!(<u8 as Niche>::try_from_usize(0), Some(0u8));
        assert_eq!(<u8 as Niche>::try_from_usize(255), Some(255u8));
        assert_eq!(<u8 as Niche>::try_from_usize(256), None);
    }

    #[test]
    fn all_covers_domain_double_ended() {
        fn check<N: Niche>(count: usize) {
            // Forward length and order.
            let fwd = N::all();
            assert_eq!(fwd.len(), count);
            let collected: usize = N::all().count();
            assert_eq!(collected, count);

            // First and last via both ends.
            let mut it = N::all();
            assert_eq!(it.next().map(Niche::as_usize), Some(0));
            assert_eq!(it.next_back().map(Niche::as_usize), Some(count - 1));

            // Ascending and exact.
            let mut prev: Option<usize> = None;
            let mut seen = 0usize;
            for v in N::all() {
                let cur = v.as_usize();
                if let Some(p) = prev {
                    assert!(cur == p + 1, "not ascending by 1");
                }
                prev = Some(cur);
                seen += 1;
            }
            assert_eq!(seen, count);
        }
        check::<U3>(8);
        check::<U4>(16);
        check::<U5>(32);
        check::<U6>(64);
        check::<U7>(128);
        check::<u8>(256);
    }

    #[test]
    fn into_u8_and_usize() {
        fn take(i: impl Into<usize>) -> usize {
            i.into()
        }

        // Infallible widening conversions for generic `Into` bounds.
        assert_eq!(u8::from(U4::MAX), 15u8);
        assert_eq!(usize::from(U4::MAX), 15usize);
        assert_eq!(u8::from(U7::MAX), 127u8);
        assert_eq!(usize::from(U3::MIN), 0usize);

        // Usable through a generic `Into<usize>` bound.
        assert_eq!(take(U5::new_masked(5)), 5usize);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_and_range_validation() {
        // Round-trip through JSON for a couple of values.
        let v = U4::new_masked(9);
        let json = serde_json::to_string(&v).expect("serialize U4");
        assert_eq!(json, "9");
        let back: U4 = serde_json::from_str(&json).expect("deserialize U4");
        assert_eq!(back, v);

        // Out-of-range integers are rejected (16 is not a valid U4).
        let err = serde_json::from_str::<U4>("16");
        assert!(err.is_err());
        // In-range boundary is accepted.
        assert_eq!(serde_json::from_str::<U4>("15").expect("15"), U4::MAX);
    }
}
