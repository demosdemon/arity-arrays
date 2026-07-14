//! [`Compact`]: a `serde_with` adapter that serializes a [`PackedArray`] or
//! [`GappedArray`] as a fixed-width little-endian bitmap plus its dense values
//! — a compact, canonical wire form (the bitmap goes through
//! [`Bitmap::to_bytes`](arity_bitmap::Bitmap::to_bytes)).

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

/// `serde_with` adapter for the compact wire form of [`PackedArray`] and
/// [`GappedArray`]. Use as `#[serde_as(as = "Compact")]` on a
/// `PackedArray<T, A>` or `GappedArray<T, A>` field.
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

/// The `Expected` descriptor passed to `serde::de::Error::invalid_length` when
/// the bitmap byte slice has the wrong length; the message reads
/// `"invalid length N, expected the bitmap byte length (WIDTH / 8)"`.
const COMPACT_LEN_ERR: &str = "the bitmap byte length (WIDTH / 8)";
/// The complete message passed to `serde::de::Error::custom` when the bitmap
/// popcount disagrees with the number of decoded values.
const COMPACT_POPCOUNT_ERR: &str = "Compact: bitmap popcount does not match the number of values";

impl_compact_adapter!(PackedArray);
impl_compact_adapter!(GappedArray);
