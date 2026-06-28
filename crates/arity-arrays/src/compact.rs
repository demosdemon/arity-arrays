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
use crate::PackedArray;

/// `serde_with` adapter for the compact `PackedArray` wire form. Use as
/// `#[serde_as(as = "Compact")]` on a `PackedArray<T, A>` field.
pub struct Compact;

impl<T: Serialize, A: Arity> SerializeAs<PackedArray<T, A>> for Compact {
    fn serialize_as<S: serde::Serializer>(
        source: &PackedArray<T, A>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut buf = alloc::vec![0u8; <A::Bitmap as Bitmap>::BYTES];
        source.bitmap().to_le_bytes(&mut buf);
        let values: Vec<&T> = source.iter_present().map(|(_, v)| v).collect();
        (buf, values).serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, A: Arity> DeserializeAs<'de, PackedArray<T, A>> for Compact {
    fn deserialize_as<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PackedArray<T, A>, D::Error> {
        let (buf, values): (Vec<u8>, Vec<T>) = Deserialize::deserialize(deserializer)?;
        if buf.len() != <A::Bitmap as Bitmap>::BYTES {
            return Err(serde::de::Error::invalid_length(
                buf.len(),
                &"the bitmap byte length (WIDTH / 8)",
            ));
        }
        let bitmap = <A::Bitmap as Bitmap>::from_le_bytes(&buf);
        if bitmap.count_ones() as usize != values.len() {
            return Err(serde::de::Error::custom(
                "Compact: bitmap popcount does not match the number of values",
            ));
        }
        let mut out = FixedArray::<Option<T>, A>::new();
        for (index, value) in bitmap.bits().zip(values) {
            out[index] = Some(value);
        }
        Ok(PackedArray::from(out))
    }
}
