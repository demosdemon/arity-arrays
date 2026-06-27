#![no_std]

//! Fixed-arity array storage indexed by bounds-check-free niche integers.
//!
//! [`FixedArray`] is a full-width inline array (one slot per index);
//! [`PackedArray`] is a pointer-sized, heap-packed representation that stores
//! only the present elements. Both are generic over the [`Arity`] trait, which
//! pairs an index type with a bitmap backing and a `hybrid-array` size.

extern crate alloc;

pub mod arity;
pub mod fixed;
pub mod packed;

pub use arity::{Arity, Arity8, Arity16, Arity32, Arity64, Arity128, Arity256};
pub use fixed::FixedArray;
pub use packed::PackedArray;

pub use arity_bitmap as bitmap;
pub use arity_index as index;

/// Prevents downstream crates from implementing [`Arity`](crate::Arity).
trait Sealed {}
