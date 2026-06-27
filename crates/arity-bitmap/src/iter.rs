//! The double-ended set-bit iterator.

use core::iter::FusedIterator;

use arity_index::Niche;

use crate::Bitmap;

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

impl<B: Bitmap> Iterator for BitIter<B> {
    type Item = <B as Bitmap>::Index;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let pos = self.remaining.raw_lowest_pos();
        self.remaining = self.remaining.raw_clear_lowest();
        // `pos < WIDTH == Index::COUNT`, so this never returns `None`.
        Some(<B as Bitmap>::Index::try_from_usize(pos).expect("set-bit position < WIDTH"))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining.raw_popcount() as usize;
        (n, Some(n))
    }
}

impl<B: Bitmap> DoubleEndedIterator for BitIter<B> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let pos = self.remaining.raw_highest_pos();
        self.remaining = self.remaining.raw_clear_highest();
        Some(<B as Bitmap>::Index::try_from_usize(pos).expect("set-bit position < WIDTH"))
    }
}

impl<B: Bitmap> ExactSizeIterator for BitIter<B> {
    fn len(&self) -> usize {
        self.remaining.raw_popcount() as usize
    }
}

impl<B: Bitmap> FusedIterator for BitIter<B> {}
