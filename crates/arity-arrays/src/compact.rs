//! [`Compact`]: a `serde_with` adapter that serializes a [`PackedArray`] as a
//! fixed-width little-endian bitmap plus its dense values — a compact,
//! backing-independent wire form (the bitmap goes through
//! [`Bitmap::to_le_bytes`](arity_bitmap::Bitmap::to_le_bytes), so it is
//! identical for the custom and `ethnum` 256-bit backings).

extern crate alloc;

use alloc::vec::Vec;

use arity_bitmap::Bitmap;
use serde::Deserialize;
use serde::Serialize;
use serde_with::DeserializeAs;
use serde_with::SerializeAs;

use crate::Arity;
use crate::FixedArray;
use crate::GappedArray;
use crate::PackedArray;

/// `serde_with` adapter for the compact `PackedArray` wire form. Use as
/// `#[serde_as(as = "Compact")]` on a `PackedArray<T, A>` field.
pub struct Compact;

/// Serializes present values as a sequence without collecting them into a
/// temporary `Vec`. Holds a closure so `serialize` (which borrows `&self`) can
/// produce a fresh iterator each call.
struct PresentValues<F>(F);

impl<F, I> Serialize for PresentValues<F>
where
    F: Fn() -> I,
    I: Iterator,
    I::Item: Serialize,
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq((self.0)())
    }
}

/// Error operand for a compact bitmap of the wrong byte length.
const COMPACT_LEN_ERR: &str = "the bitmap byte length (WIDTH / 8)";
/// Error for a compact bitmap whose popcount disagrees with the value count.
const COMPACT_POPCOUNT_ERR: &str = "Compact: bitmap popcount does not match the number of values";

impl_compact_adapter!(PackedArray);
impl_compact_adapter!(GappedArray);
