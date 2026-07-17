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

    /// Folds the remaining set bits in one loop over the bitmap snapshot,
    /// clearing the lowest each step, instead of the default `next()`-per-item
    /// drive. `PackedArray`/`GappedArray` present iteration delegates here, so
    /// this is the shared internal-iteration primitive.
    #[inline]
    fn fold<Acc, F>(mut self, init: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let mut acc = init;
        while !self.remaining.raw_is_zero() {
            let pos = self.remaining.raw_lowest_pos();
            self.remaining = self.remaining.raw_clear_lowest();
            acc = f(
                acc,
                <B as Bitmap>::Index::try_from_usize(pos).expect("set-bit position < WIDTH"),
            );
        }
        acc
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

    /// Reverse counterpart of [`fold`](Iterator::fold): folds the remaining set
    /// bits descending, clearing the highest each step.
    #[inline]
    fn rfold<Acc, F>(mut self, init: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let mut acc = init;
        while !self.remaining.raw_is_zero() {
            let pos = self.remaining.raw_highest_pos();
            self.remaining = self.remaining.raw_clear_highest();
            acc = f(
                acc,
                <B as Bitmap>::Index::try_from_usize(pos).expect("set-bit position < WIDTH"),
            );
        }
        acc
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

    #[test]
    fn fold_and_rfold_match_next_including_partial_consumption() {
        let bm = u16::ZERO
            .with_bit(U4::new_masked(1))
            .with_bit(U4::new_masked(4))
            .with_bit(U4::new_masked(9))
            .with_bit(U4::new_masked(14));
        let push = |mut v: alloc::vec::Vec<u8>, i: U4| {
            v.push(i.as_u8());
            v
        };

        // fold ascending, rfold descending — full consumption.
        assert_eq!(bm.bits().fold(alloc::vec::Vec::new(), push), alloc::vec![
            1, 4, 9, 14
        ]);
        assert_eq!(bm.bits().rfold(alloc::vec::Vec::new(), push), alloc::vec![
            14, 9, 4, 1
        ]);

        // fold/rfold must only visit what `next`/`next_back` left behind.
        let mut it = bm.bits();
        it.next(); // 1
        it.next_back(); // 14
        assert_eq!(it.clone().fold(alloc::vec::Vec::new(), push), alloc::vec![
            4, 9
        ]);
        assert_eq!(it.rfold(alloc::vec::Vec::new(), push), alloc::vec![9, 4]);
    }
}
