//! The double-ended set-bit iterator.

use core::iter::FusedIterator;

use crate::Bitmap;
use crate::sealed::Raw;

/// Yields the set bits of a bitmap, ascending, as the bitmap's [`Niche`] index.
///
/// Holds a `Copy` snapshot of the bitmap and drains it from both ends.
///
/// [`Niche`]: arity_index::Niche
pub struct BitIter<B: Bitmap> {
    remaining: B,
}

impl<B: Bitmap> BitIter<B> {
    pub(crate) const fn new(remaining: B) -> Self {
        Self { remaining }
    }
}

impl<B: Raw> Iterator for BitIter<B> {
    type Item = <B as Bitmap>::Index;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let i = self.remaining.raw_lowest();
        self.remaining = self.remaining.raw_clear_lowest();
        Some(i)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining.raw_popcount() as usize;
        (n, Some(n))
    }
}

impl<B: Raw> DoubleEndedIterator for BitIter<B> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let i = self.remaining.raw_highest();
        self.remaining = self.remaining.raw_clear_highest();
        Some(i)
    }
}

impl<B: Raw> ExactSizeIterator for BitIter<B> {
    fn len(&self) -> usize {
        self.remaining.raw_popcount() as usize
    }
}

impl<B: Raw> FusedIterator for BitIter<B> {}
