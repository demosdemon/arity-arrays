//! Double-ended range iterators over [`Niche`](crate::Niche) values.
//!
//! `std`'s `Range`/`RangeInclusive` cannot iterate a niche integer (that needs
//! the unstable `Step` trait), so these custom iterators provide it. They store
//! bounds as `usize` and reconstruct each value at yield time; the cursor is
//! always `< COUNT` by construction, so the reconstruction is sound.

use crate::Niche;
use core::iter::FusedIterator;
use core::marker::PhantomData;

/// A half-open range `[start, end)` over the values of a [`Niche`] type.
#[derive(Clone, Debug)]
pub struct NicheRange<N: Niche> {
    lo: usize,
    hi: usize,
    _marker: PhantomData<N>,
}

impl<N: Niche> NicheRange<N> {
    /// Creates the half-open range `[start, end)`. Empty if `start >= end`.
    #[must_use]
    pub fn new(start: N, end: N) -> Self {
        Self {
            lo: start.as_usize(),
            hi: end.as_usize(),
            _marker: PhantomData,
        }
    }
}

impl<N: Niche> Iterator for NicheRange<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        if self.lo >= self.hi {
            return None;
        }
        // SAFETY: `lo < hi <= COUNT`, so `lo < COUNT` and `try_from_usize` is `Some`.
        let v = unsafe { N::try_from_usize(self.lo).unwrap_unchecked() };
        self.lo += 1;
        Some(v)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.len();
        (n, Some(n))
    }
}

impl<N: Niche> DoubleEndedIterator for NicheRange<N> {
    fn next_back(&mut self) -> Option<N> {
        if self.lo >= self.hi {
            return None;
        }
        self.hi -= 1;
        // SAFETY: `hi < COUNT` (it was `<= COUNT` and just decremented).
        Some(unsafe { N::try_from_usize(self.hi).unwrap_unchecked() })
    }
}

impl<N: Niche> ExactSizeIterator for NicheRange<N> {
    fn len(&self) -> usize {
        self.hi.saturating_sub(self.lo)
    }
}

impl<N: Niche> FusedIterator for NicheRange<N> {}

/// A closed range `[start, end]` over the values of a [`Niche`] type.
#[derive(Clone, Debug)]
pub struct NicheRangeInclusive<N: Niche> {
    lo: usize,
    hi: usize,
    done: bool,
    _marker: PhantomData<N>,
}

impl<N: Niche> NicheRangeInclusive<N> {
    /// Creates the closed range `[start, end]`. Empty if `start > end`.
    #[must_use]
    pub fn new(start: N, end: N) -> Self {
        let (lo, hi) = (start.as_usize(), end.as_usize());
        Self {
            lo,
            hi,
            done: lo > hi,
            _marker: PhantomData,
        }
    }

    /// The whole domain `[0, COUNT - 1]`. Backs [`Niche::all`](crate::Niche::all).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "consumed by Niche::all(), added in a later task")
    )]
    pub(crate) const fn full() -> Self {
        Self {
            lo: 0,
            hi: N::COUNT - 1,
            done: false,
            _marker: PhantomData,
        }
    }
}

impl<N: Niche> Iterator for NicheRangeInclusive<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        if self.done {
            return None;
        }
        // SAFETY: `lo <= hi <= COUNT - 1 < COUNT`.
        let v = unsafe { N::try_from_usize(self.lo).unwrap_unchecked() };
        if self.lo == self.hi {
            self.done = true;
        } else {
            self.lo += 1;
        }
        Some(v)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.len();
        (n, Some(n))
    }
}

impl<N: Niche> DoubleEndedIterator for NicheRangeInclusive<N> {
    fn next_back(&mut self) -> Option<N> {
        if self.done {
            return None;
        }
        // SAFETY: `lo <= hi <= COUNT - 1 < COUNT`.
        let v = unsafe { N::try_from_usize(self.hi).unwrap_unchecked() };
        if self.lo == self.hi {
            self.done = true;
        } else {
            self.hi -= 1;
        }
        Some(v)
    }
}

impl<N: Niche> ExactSizeIterator for NicheRangeInclusive<N> {
    fn len(&self) -> usize {
        if self.done {
            0
        } else {
            self.hi - self.lo + 1
        }
    }
}

impl<N: Niche> FusedIterator for NicheRangeInclusive<N> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{U3, U4};

    extern crate alloc;
    use alloc::vec::Vec;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn half_open_forward_and_len() {
        let r = NicheRange::new(u4(2), u4(6));
        assert_eq!(r.len(), 4);
        let got: Vec<u8> = r.map(U4::as_u8).collect();
        assert_eq!(got, alloc::vec![2, 3, 4, 5]);
    }

    #[test]
    fn half_open_empty() {
        let r = NicheRange::new(u4(6), u4(2));
        assert_eq!(r.len(), 0);
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn half_open_double_ended_meets_in_middle() {
        let mut r = NicheRange::new(u4(0), u4(4));
        assert_eq!(r.next().map(U4::as_u8), Some(0));
        assert_eq!(r.next_back().map(U4::as_u8), Some(3));
        assert_eq!(r.next().map(U4::as_u8), Some(1));
        assert_eq!(r.next_back().map(U4::as_u8), Some(2));
        assert_eq!(r.next(), None);
        assert_eq!(r.next_back(), None);
    }

    #[test]
    fn inclusive_forward_and_len() {
        let r = NicheRangeInclusive::new(u4(2), u4(5));
        assert_eq!(r.len(), 4);
        let got: Vec<u8> = r.map(U4::as_u8).collect();
        assert_eq!(got, alloc::vec![2, 3, 4, 5]);
    }

    #[test]
    fn inclusive_single_element() {
        let mut r = NicheRangeInclusive::new(u4(7), u4(7));
        assert_eq!(r.len(), 1);
        assert_eq!(r.next().map(U4::as_u8), Some(7));
        assert_eq!(r.next(), None);
    }

    #[test]
    fn inclusive_double_ended_meets_in_middle() {
        let mut r = NicheRangeInclusive::new(u4(0), u4(3));
        assert_eq!(r.len(), 4);
        assert_eq!(r.next().map(U4::as_u8), Some(0));
        assert_eq!(r.next_back().map(U4::as_u8), Some(3));
        assert_eq!(r.next().map(U4::as_u8), Some(1));
        assert_eq!(r.next_back().map(U4::as_u8), Some(2));
        assert_eq!(r.next(), None);
        assert_eq!(r.next_back(), None);
    }

    #[test]
    fn inclusive_full_covers_domain() {
        let r = NicheRangeInclusive::<U3>::full();
        assert_eq!(r.len(), 8);
        let got: Vec<u8> = r.map(U3::as_u8).collect();
        assert_eq!(got, alloc::vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
