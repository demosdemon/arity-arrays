//! Byte-key helpers for the arities whose indices map cleanly onto bytes.
//!
//! Feature `16`: [`nibbles`] and [`unnibble`] split a byte key into its high
//! and low nibbles and back. Feature `256`: an index is a byte, so
//! [`Path::try_from_bytes`] never fails and [`Path::as_bytes`] is its inverse;
//! no helper is needed. The other arities get no helper: their index width
//! does not divide a byte evenly, so no byte-key mapping is natural.
#![cfg_attr(not(feature = "16"), allow(unused_imports))]

use alloc::vec::Vec;

#[cfg(feature = "16")]
use arity_arrays::Arity16;
#[cfg(feature = "16")]
use arity_arrays::index::U4;

use crate::Path;

/// Splits each byte into its high nibble then its low nibble, so `[0xAB]`
/// becomes `[0xA, 0xB]`.
#[cfg(feature = "16")]
#[must_use]
pub fn nibbles(key: &[u8]) -> Path<Arity16> {
    key.iter()
        .flat_map(|b| [U4::new_masked(b >> 4), U4::new_masked(b & 0xF)])
        .collect()
}

/// The inverse of [`nibbles`]: `None` if `path` has an odd length.
#[cfg(feature = "16")]
#[must_use]
pub fn unnibble(path: &[U4]) -> Option<Vec<u8>> {
    if !path.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, _) = path.as_chunks::<2>();
    Some(
        pairs
            .iter()
            .map(|[hi, lo]| (hi.as_u8() << 4) | lo.as_u8())
            .collect(),
    )
}

#[cfg(test)]
#[cfg(feature = "16")]
mod tests {
    use alloc::vec;

    use arity_arrays::index::U4;

    use super::*;

    #[test]
    fn nibbles_splits_high_then_low() {
        let p = nibbles(&[0xAB, 0x01]);
        let expect: alloc::vec::Vec<U4> = [0xA, 0xB, 0x0, 0x1].map(U4::new_masked).into();
        assert_eq!(&*p, &expect[..]);
        assert_eq!(nibbles(&[]).len(), 0);
    }

    #[test]
    fn unnibble_inverts_nibbles() {
        let key = [0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(unnibble(&nibbles(&key)), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(unnibble(&[]), Some(vec![]));
    }

    #[test]
    fn unnibble_rejects_odd_length() {
        assert_eq!(unnibble(&[U4::new_masked(1)]), None);
        assert_eq!(unnibble(&nibbles(&[0x12])[..1]), None);
    }
}
