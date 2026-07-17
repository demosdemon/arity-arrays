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
    ///
    /// # Safety-critical
    /// `arity-arrays` uses the result (`< COUNT`) for `get_unchecked`. A value
    /// `>= COUNT` would be undefined behavior there; treat edits to this
    /// contract as safety-load-bearing.
    fn as_usize(self) -> usize;

    /// Constructs from a `usize`, or `None` if `i >= COUNT`.
    fn try_from_usize(i: usize) -> Option<Self>;

    /// Reinterprets a byte slice as `&[Self]`, or returns `None` if any byte is
    /// out of range (`>= COUNT`).
    ///
    /// The scan is `O(n)`; the conversion itself is free. The result borrows
    /// the original bytes in place — nothing is copied.
    ///
    /// ```
    /// use arity_index::{Niche, U4};
    ///
    /// fn parse<N: Niche>(bytes: &[u8]) -> Option<&[N]> {
    ///     N::try_from_slice(bytes)
    /// }
    ///
    /// assert!(parse::<U4>(&[0, 15]).is_some());
    /// assert!(parse::<U4>(&[0, 16]).is_none());
    /// // Every byte is a valid arity-256 index, so this never fails.
    /// assert!(parse::<u8>(&[0, 255]).is_some());
    /// ```
    fn try_from_slice(slice: &[u8]) -> Option<&[Self]>;

    /// Reinterprets a byte slice as `&[Self]` without scanning it.
    ///
    /// Prefer [`try_from_slice`](Self::try_from_slice) unless the scan is
    /// measurably too costly and the range is already established.
    ///
    /// # Safety
    ///
    /// Every byte of `slice` must be `< COUNT`. Otherwise the returned slice
    /// contains an invalid value, which is undefined behavior even if it is
    /// never read.
    ///
    /// # Panics
    ///
    /// Panics if a byte is `>= COUNT` and `debug_assertions` are enabled. This
    /// is a debugging aid, not a guarantee: in release builds the same call is
    /// undefined behavior with no diagnostic.
    #[must_use]
    unsafe fn from_slice_unchecked(slice: &[u8]) -> &[Self];

    /// Reinterprets a slice of `Self` as the underlying bytes.
    ///
    /// Free and infallible — every `Self` is a valid `u8`. The result borrows
    /// in place; nothing is copied.
    ///
    /// There is deliberately no `&mut [Self] -> &mut [u8]` counterpart: it
    /// would let a caller store an out-of-range byte and leave an invalid
    /// `Self` behind.
    #[must_use]
    fn as_u8_slice(slice: &[Self]) -> &[u8];

    /// Iterates over all values ascending (`MIN..=MAX`) as a double-ended,
    /// exact-size iterator. `len() == COUNT`.
    ///
    /// This range iterator is deliberately the only way to enumerate the
    /// domain: there is no `ALL` constant, so nothing materializes a
    /// `COUNT`-element table (an `[U7; 128]` / `[u8; 256]` const would
    /// otherwise sit in the binary).
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
/// generated `.rs` to drift) at the cost of one proc-macro dependency.
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
        ///
        /// `#[repr(transparent)]` over that enum, so this type has the size and
        /// alignment of `u8` and a slice of it can be reinterpreted as bytes in
        /// place — see [`try_from_slice`](Self::try_from_slice) and
        /// [`as_u8_slice`](Self::as_u8_slice).
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name($repr);

        // The whole point of the type: `Option<Self>` must fit in one byte
        // (niche optimization). Enforced at compile time for every profile.
        const _: () = assert!(::core::mem::size_of::<::core::option::Option<$name>>() == 1);

        // The layout premise behind the slice reinterpretations: `repr(transparent)`
        // over a `repr(u8)` fieldless enum gives `u8`'s size and alignment, so a
        // `&[u8]` and a `&[Self]` over the same bytes agree on both element count
        // and address. Asserted rather than assumed — the `unsafe` in
        // `transmute_slice` / `as_u8_slice` is unsound without it.
        const _: () = assert!(::core::mem::size_of::<$name>() == ::core::mem::size_of::<u8>());
        const _: () = assert!(::core::mem::align_of::<$name>() == ::core::mem::align_of::<u8>());

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
                #[inline]
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
            #[inline]
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
            #[inline]
            pub const fn new_masked(v: u8) -> Self {
                // SAFETY: masking by `COUNT - 1` (a power of two minus one) yields a
                // value `< COUNT`.
                unsafe { Self::new_unchecked(v & (($count - 1) as u8)) }
            }

            /// Returns the value as a `u8`.
            #[must_use]
            #[inline(always)]
            pub const fn as_u8(self) -> u8 {
                self.0 as u8
            }

            /// Returns the value as a `usize`.
            #[must_use]
            #[inline(always)]
            pub const fn as_usize(self) -> usize {
                self.0 as usize
            }

            /// Returns whether every byte is in range (`< COUNT`), i.e. whether
            /// the slice is a valid `&[Self]`.
            #[inline]
            const fn bytes_in_range(slice: &[u8]) -> bool {
                let mut i = 0;
                while i < slice.len() {
                    if slice[i] >= $count {
                        return false;
                    }
                    i += 1;
                }
                true
            }

            /// Reinterprets `slice` as `&[Self]`, checking nothing.
            ///
            /// # Safety
            ///
            /// Every byte of `slice` must be `< COUNT`.
            #[inline]
            const unsafe fn transmute_slice(slice: &[u8]) -> &[Self] {
                // SAFETY: `Self` is `repr(transparent)` over a `repr(u8)` fieldless
                // enum, so it shares `u8`'s size and alignment (asserted above) and
                // the element count carries over unchanged. The caller guarantees
                // every byte is `< COUNT`, so every element is a valid discriminant.
                // The output borrows `slice`, so the lifetime and provenance are
                // inherited rather than invented.
                unsafe { ::core::slice::from_raw_parts(slice.as_ptr().cast::<Self>(), slice.len()) }
            }

            /// Reinterprets a byte slice as `&[Self]`, or returns `None` if any
            /// byte is out of range (`>= COUNT`).
            ///
            /// The scan is `O(n)`; the conversion itself is free. The result
            /// borrows the original bytes in place — nothing is copied.
            ///
            /// ```
            #[doc = concat!("# use arity_index::", stringify!($name), ";")]
            #[doc = concat!("let bytes = &[0u8, 1, ", stringify!($count), " - 1];")]
            #[doc = concat!("let idx = ", stringify!($name), "::try_from_slice(bytes).expect(\"all in range\");")]
            #[doc = concat!("assert_eq!(idx.len(), 3);")]
            #[doc = concat!("assert_eq!(idx[2].as_u8(), ", stringify!($count), " - 1);")]
            ///
            /// // One out-of-range byte rejects the whole slice.
            #[doc = concat!("assert!(", stringify!($name), "::try_from_slice(&[0, ", stringify!($count), "]).is_none());")]
            /// ```
            #[must_use]
            #[inline]
            pub const fn try_from_slice(slice: &[u8]) -> Option<&[Self]> {
                if Self::bytes_in_range(slice) {
                    // SAFETY: `bytes_in_range` just proved every byte is `< COUNT`.
                    Some(unsafe { Self::transmute_slice(slice) })
                } else {
                    None
                }
            }

            /// Reinterprets a byte slice as `&[Self]` without scanning it.
            ///
            /// Prefer [`try_from_slice`](Self::try_from_slice) unless the scan is
            /// measurably too costly and the range is already established.
            ///
            /// # Safety
            ///
            /// Every byte of `slice` must be `< COUNT`. Otherwise the returned
            /// slice contains an invalid discriminant, which is undefined
            /// behavior even if the value is never read.
            ///
            /// # Panics
            ///
            /// Panics if a byte is `>= COUNT` and `debug_assertions` are enabled.
            /// This is a debugging aid, not a guarantee: in release builds the
            /// same call is undefined behavior with no diagnostic.
            #[must_use]
            #[inline]
            pub const unsafe fn from_slice_unchecked(slice: &[u8]) -> &[Self] {
                debug_assert!(Self::bytes_in_range(slice));
                // SAFETY: the caller guarantees every byte is `< COUNT`.
                unsafe { Self::transmute_slice(slice) }
            }

            /// Reinterprets a slice of `Self` as the underlying bytes.
            ///
            /// Free and infallible — every `Self` is a valid `u8`. The result
            /// borrows in place; nothing is copied.
            ///
            /// There is deliberately no `&mut [Self] -> &mut [u8]` counterpart:
            /// it would let a caller store an out-of-range byte and leave an
            /// invalid `Self` behind.
            ///
            /// ```
            #[doc = concat!("# use arity_index::", stringify!($name), ";")]
            #[doc = concat!("let idx = ", stringify!($name), "::try_from_slice(&[1, 2, 3]).expect(\"in range\");")]
            #[doc = concat!("assert_eq!(", stringify!($name), "::as_u8_slice(idx), &[1, 2, 3]);")]
            /// ```
            #[must_use]
            #[inline]
            pub const fn as_u8_slice(slice: &[Self]) -> &[u8] {
                // SAFETY: `Self` is `repr(transparent)` over a `repr(u8)` fieldless
                // enum, so it shares `u8`'s size and alignment (asserted above) and
                // the element count carries over unchanged. Every discriminant is a
                // valid `u8`, so no validity check is needed in this direction. The
                // output borrows `slice`, inheriting its lifetime and provenance.
                unsafe { ::core::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), slice.len()) }
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

            #[inline]
            fn try_from(v: u8) -> ::core::result::Result<Self, Self::Error> {
                Self::try_new(v).ok_or(crate::TryFromIntError)
            }
        }

        impl ::core::convert::From<$name> for u8 {
            #[inline]
            fn from(v: $name) -> u8 {
                v.as_u8()
            }
        }

        impl ::core::convert::From<$name> for usize {
            #[inline]
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

            #[inline(always)]
            fn as_usize(self) -> usize {
                self.0 as usize
            }

            #[inline]
            fn try_from_usize(i: usize) -> Option<Self> {
                match u8::try_from(i) {
                    Ok(v) => Self::try_new(v),
                    Err(_) => None,
                }
            }

            // These forward to the inherent associated functions of the same name:
            // an inherent item takes precedence over a trait one, so `Self::` here
            // resolves to the inherent version, not back into this impl.
            #[inline]
            fn try_from_slice(slice: &[u8]) -> Option<&[Self]> {
                Self::try_from_slice(slice)
            }

            #[inline]
            unsafe fn from_slice_unchecked(slice: &[u8]) -> &[Self] {
                // SAFETY: the caller of this trait method guarantees every byte is
                // `< COUNT`, which is exactly the inherent function's precondition.
                unsafe { Self::from_slice_unchecked(slice) }
            }

            #[inline]
            fn as_u8_slice(slice: &[Self]) -> &[u8] {
                Self::as_u8_slice(slice)
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

    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "single-instruction cast feeding get_unchecked on the \
                  indexing fast path; forcing inlining avoids a call across \
                  the crate boundary without LTO"
    )]
    fn as_usize(self) -> usize {
        usize::from(self)
    }

    #[inline]
    fn try_from_usize(i: usize) -> Option<Self> {
        // `Self::try_from` succeeds iff `i <= 255`, i.e. `i < COUNT`. No cast.
        Self::try_from(i).ok()
    }

    // `Self` is `u8`, so all three conversions are the identity: every byte is
    // already a valid arity-256 index. No scan, no transmute, and `try_from_slice`
    // cannot fail.
    #[inline]
    fn try_from_slice(slice: &[u8]) -> Option<&[Self]> {
        Some(slice)
    }

    #[inline]
    unsafe fn from_slice_unchecked(slice: &[u8]) -> &[Self] {
        slice
    }

    #[inline]
    fn as_u8_slice(slice: &[Self]) -> &[u8] {
        slice
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

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

    #[test]
    fn try_from_slice_accepts_in_range_and_rejects_out_of_range() {
        // Boundary: COUNT - 1 is accepted, COUNT is not.
        let ok = U4::try_from_slice(&[0, 7, 15]).expect("all bytes < 16");
        assert_eq!(ok.len(), 3);
        assert_eq!(ok[0], U4::MIN);
        assert_eq!(ok[2], U4::MAX);
        assert!(U4::try_from_slice(&[16]).is_none());
        assert!(U4::try_from_slice(&[0, 1, 255]).is_none());

        // A single bad byte anywhere rejects the whole slice.
        assert!(U4::try_from_slice(&[0, 1, 2, 16, 4]).is_none());

        // Empty slices are vacuously valid.
        assert_eq!(U4::try_from_slice(&[]).expect("empty is valid").len(), 0);

        // Per-type boundaries.
        assert!(U3::try_from_slice(&[7]).is_some());
        assert!(U3::try_from_slice(&[8]).is_none());
        assert!(U7::try_from_slice(&[127]).is_some());
        assert!(U7::try_from_slice(&[128]).is_none());
    }

    #[test]
    fn slice_round_trips_and_borrows_in_place() {
        let bytes: &[u8] = &[0, 5, 9, 15];
        let idx = U4::try_from_slice(bytes).expect("in range");
        assert_eq!(U4::as_u8_slice(idx), bytes);

        // The conversion is a reinterpretation, not a copy: same address, same
        // length. This is the property the `repr(transparent)` change buys.
        assert_eq!(U4::as_u8_slice(idx).as_ptr(), bytes.as_ptr());
        assert_eq!(idx.len(), bytes.len());

        // Load every element, so Miri validates each discriminant (it checks an
        // enum tag on load, not when the slice reference is formed).
        for (v, &b) in idx.iter().zip(bytes) {
            assert_eq!(v.as_u8(), b);
        }

        // The full domain of each type survives the round trip.
        let full: alloc::vec::Vec<u8> = (0..=255).collect();
        assert!(U7::try_from_slice(&full).is_none());
        let in_range: alloc::vec::Vec<u8> = (0..128).collect();
        let idx7 = U7::try_from_slice(&in_range).expect("0..128 are all valid U7");
        for (v, &b) in idx7.iter().zip(&in_range) {
            assert_eq!(v.as_u8(), b);
        }
    }

    #[test]
    fn from_slice_unchecked_matches_checked() {
        let bytes: &[u8] = &[0, 5, 9, 15];
        // SAFETY: every byte is < 16.
        let unchecked = unsafe { U4::from_slice_unchecked(bytes) };
        let checked = U4::try_from_slice(bytes).expect("in range");
        assert_eq!(unchecked, checked);
    }

    #[test]
    fn slice_conversions_work_generically() {
        // Exercises the trait forwarding impls. If `Self::try_from_slice` in the
        // macro resolved to the trait method rather than the inherent one, these
        // would recurse until the stack overflowed.
        fn round_trip<N: Niche + core::fmt::Debug>(bytes: &[u8], expect_ok: bool) {
            match N::try_from_slice(bytes) {
                Some(idx) => {
                    assert!(expect_ok, "expected rejection, got {idx:?}");
                    assert_eq!(N::as_u8_slice(idx), bytes);
                    // SAFETY: `try_from_slice` just proved every byte is < COUNT.
                    let unchecked = unsafe { N::from_slice_unchecked(bytes) };
                    assert_eq!(N::as_u8_slice(unchecked), bytes);
                    // Load every element by value. Miri validates an enum tag on
                    // load, not when the slice reference is formed, so a test that
                    // only inspects lengths or converts back to bytes would let an
                    // invalid discriminant slip past it.
                    for (v, &b) in idx.iter().zip(bytes) {
                        assert_eq!(v.as_usize(), usize::from(b));
                    }
                }
                None => assert!(!expect_ok, "expected acceptance, got None"),
            }
        }

        round_trip::<U3>(&[0, 7], true);
        round_trip::<U3>(&[8], false);
        round_trip::<U4>(&[0, 15], true);
        round_trip::<U4>(&[16], false);
        round_trip::<U5>(&[31], true);
        round_trip::<U5>(&[32], false);
        round_trip::<U6>(&[63], true);
        round_trip::<U6>(&[64], false);
        round_trip::<U7>(&[127], true);
        round_trip::<U7>(&[128], false);
        // Every byte is a valid arity-256 index, so nothing is rejected.
        round_trip::<u8>(&[0, 128, 255], true);
    }

    // Only compiled with debug assertions on. In a release build the call below
    // is undefined behavior rather than a panic, so the test must not exist there.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "bytes_in_range")]
    fn from_slice_unchecked_debug_asserts_on_out_of_range() {
        // SAFETY: none — this deliberately violates the precondition to prove the
        // debug assertion catches it. It panics before reaching the transmute, so
        // no invalid `U4` is ever created.
        let _ = unsafe { U4::from_slice_unchecked(&[0, 1, 16]) };
    }

    #[test]
    fn try_from_slice_is_const() {
        // The inherent form is usable in a const context; this fails to compile
        // if it ever stops being `const fn`.
        const BYTES: &[u8] = &[0, 1, 2];
        const IDX: Option<&[U4]> = U4::try_from_slice(BYTES);
        assert!(IDX.is_some());
    }

    #[test]
    fn niche_types_have_u8_layout() {
        // The premise behind the slice reinterpretations.
        assert_eq!(core::mem::size_of::<U3>(), 1);
        assert_eq!(core::mem::size_of::<U7>(), 1);
        assert_eq!(core::mem::align_of::<U3>(), 1);
        assert_eq!(core::mem::align_of::<U7>(), 1);
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
