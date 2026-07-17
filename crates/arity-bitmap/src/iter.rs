//! The double-ended set-bit iterator.

use core::iter::FusedIterator;

use arity_index::Niche;

use crate::Bitmap;

/// Yields the set bits of a bitmap, ascending, as the bitmap's [`Niche`] index.
///
/// Holds a `Copy` snapshot of the bitmap and drains it from both ends.
///
/// [`Niche`]: arity_index::Niche
#[derive(Clone, Debug)]
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct BitIter<B: Bitmap> {
    remaining: B,
}

impl<B: Bitmap> BitIter<B> {
    #[inline]
    pub(crate) const fn new(remaining: B) -> Self {
        Self { remaining }
    }
}

impl<B: Bitmap> Iterator for BitIter<B> {
    type Item = <B as Bitmap>::Index;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let pos = self.remaining.raw_lowest_pos();
        self.remaining = self.remaining.raw_clear_lowest();
        // `pos < WIDTH == Index::COUNT`, so this never returns `None`.
        Some(<B as Bitmap>::Index::try_from_usize(pos).expect("set-bit position < WIDTH"))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining.raw_popcount() as usize;
        (n, Some(n))
    }
}

impl<B: Bitmap> DoubleEndedIterator for BitIter<B> {
    #[inline]
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
    #[inline]
    fn len(&self) -> usize {
        self.remaining.raw_popcount() as usize
    }
}

impl<B: Bitmap> FusedIterator for BitIter<B> {}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use arity_index::U4;

    use crate::Bitmap;

    #[test]
    fn bit_iter_is_clone_and_debug() {
        let it = u16::ZERO
            .with_bit(U4::new_masked(1))
            .with_bit(U4::new_masked(4))
            .bits();
        let cloned = it.clone();
        assert!(alloc::format!("{it:?}").contains("BitIter"));
        assert_eq!(it.count(), cloned.count());
    }
}
