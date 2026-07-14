#![no_std]

//! Fixed-arity array storage indexed by bounds-check-free niche integers.
//!
//! [`FixedArray`] is a full-width inline array (one slot per index);
//! [`PackedArray`] is a pointer-sized, heap-packed representation that stores
//! only the present elements; [`GappedArray`] is a pointer-sized, heap-backed
//! representation with spare capacity and gaps that minimizes mutation cost.
//! All three are generic over the [`Arity`] trait, which pairs an index type
//! with a bitmap backing and a `hybrid-array` size.
//!
//! ```
//! # extern crate alloc;
//! use arity_arrays::{Arity16, FixedArray, PackedArray};
//! use arity_arrays::index::{Niche, U4};
//!
//! let mut full = FixedArray::<Option<u32>, Arity16>::new();
//! full[U4::new_masked(1)] = Some(10);
//! full[U4::new_masked(9)] = Some(90);
//!
//! // Pack: pointer-sized handle, two elements on the heap.
//! let packed = PackedArray::from(&full);
//! assert_eq!(packed.count(), 2);
//! assert_eq!(packed.get(U4::new_masked(9)), Some(&90));
//!
//! let present: alloc::vec::Vec<(u8, u32)> =
//!     packed.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
//! assert_eq!(present, alloc::vec![(1, 10), (9, 90)]);
//! ```

extern crate alloc;

pub mod arity;
pub mod fixed;
#[macro_use]
mod macros;
#[cfg(feature = "serde_with")]
mod compact;
pub mod gapped;
pub mod packed;
mod raw;

pub use arity::Arity;
#[cfg(feature = "8")]
pub use arity::Arity8;
#[cfg(feature = "16")]
pub use arity::Arity16;
#[cfg(feature = "32")]
pub use arity::Arity32;
#[cfg(feature = "64")]
pub use arity::Arity64;
#[cfg(feature = "128")]
pub use arity::Arity128;
#[cfg(feature = "256")]
pub use arity::Arity256;
#[rustfmt::skip]
pub mod bitmap {
    //! Re-exports of the [`arity_bitmap`] public API.
    //!
    //! `arity-arrays`'s semver surface includes these items: a breaking change
    //! to any of them upstream is a breaking change here. `ethnum` (the `U256`
    //! backing) is a public dependency through this module.
    pub use arity_bitmap::{BitIter, Bitmap};
    #[cfg(feature = "256")]
    pub use arity_bitmap::U256;
}

#[rustfmt::skip]
pub mod index {
    //! Re-exports of the [`arity_index`] public API.
    //!
    //! `arity-arrays`'s semver surface includes these items: a breaking change
    //! to any of them upstream is a breaking change here.
    pub use arity_index::{Niche, NicheRange, NicheRangeInclusive, TryFromIntError};
    #[cfg(feature = "8")]   pub use arity_index::U3;
    #[cfg(feature = "16")]  pub use arity_index::U4;
    #[cfg(feature = "32")]  pub use arity_index::U5;
    #[cfg(feature = "64")]  pub use arity_index::U6;
    #[cfg(feature = "128")] pub use arity_index::U7;
}
#[cfg(feature = "serde_with")]
pub use compact::Compact;
pub use fixed::FixedArray;
pub use gapped::GappedArray;
pub use packed::PackedArray;

/// Prevents downstream crates from implementing [`Arity`](crate::Arity).
trait Sealed {}

/// The crate README's usage example, compiled as a doctest so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
