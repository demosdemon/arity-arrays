//! [`FixedArray`]: full-width inline storage, one `T` per slot.

use core::ops::{Deref, DerefMut, Index, IndexMut};

use arity_index::Niche;
use hybrid_array::Array;

use crate::Arity;

/// A full-width array with one `T` per slot, indexed by `A::Index` without bounds
/// checks.
///
/// `hybrid_array::Array` is an implementation detail — it never appears in a
/// public signature (the type exposes `Deref<Target = [T]>` / `AsRef<[T]>`), so
/// the `typenum` backing can be retired later without a breaking change.
pub struct FixedArray<T, A: Arity>(Array<T, A::Size>);

impl<T, A: Arity> FixedArray<T, A> {
    /// Builds a `FixedArray` by calling `f` for every index in ascending order.
    pub fn from_fn(mut f: impl FnMut(A::Index) -> T) -> Self {
        Self(Array::from_fn(|i| {
            // `i` ranges over `0..A::Size::USIZE == A::LEN == Index::COUNT`.
            f(A::Index::try_from_usize(i).expect("from_fn index < LEN"))
        }))
    }

    /// Returns a reference to the element at `index` (no bounds check).
    #[must_use]
    pub fn get(&self, index: A::Index) -> &T {
        // SAFETY: `A::Index::as_usize()` is always `< Index::COUNT == A::LEN`,
        // which equals the array length, so `index` is in bounds.
        unsafe { self.0.as_slice().get_unchecked(index.as_usize()) }
    }

    /// Returns a mutable reference to the element at `index` (no bounds check).
    #[must_use]
    pub fn get_mut(&mut self, index: A::Index) -> &mut T {
        // SAFETY: as in `get` — `index.as_usize() < A::LEN == array length`.
        unsafe { self.0.as_mut_slice().get_unchecked_mut(index.as_usize()) }
    }

    /// Replaces the element at `index`, returning the previous value.
    pub fn replace(&mut self, index: A::Index, value: T) -> T {
        core::mem::replace(self.get_mut(index), value)
    }

    /// Maps each element to a new value, with its index, returning a new array.
    pub fn map<O>(self, mut f: impl FnMut(A::Index, T) -> O) -> FixedArray<O, A> {
        let mut idx = A::Index::all();
        FixedArray(self.0.map(|t| {
            let i = idx.next().expect("map cursor stays in 0..LEN");
            f(i, t)
        }))
    }
}

impl<T, A: Arity> Index<A::Index> for FixedArray<T, A> {
    type Output = T;
    fn index(&self, index: A::Index) -> &T {
        self.get(index)
    }
}

impl<T, A: Arity> IndexMut<A::Index> for FixedArray<T, A> {
    fn index_mut(&mut self, index: A::Index) -> &mut T {
        self.get_mut(index)
    }
}

impl<T, A: Arity> Deref for FixedArray<T, A> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.0.as_slice()
    }
}

impl<T, A: Arity> DerefMut for FixedArray<T, A> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.as_mut_slice()
    }
}

impl<T, A: Arity> AsRef<[T]> for FixedArray<T, A> {
    fn as_ref(&self) -> &[T] {
        self.0.as_slice()
    }
}

impl<T, A: Arity> IntoIterator for FixedArray<T, A> {
    type Item = (A::Index, T);
    // `hybrid-array` has no public `IntoIter<T, N>` name; the owned iterator type
    // is the associated `IntoIter` of `Array`'s own `IntoIterator` impl.
    type IntoIter = core::iter::Zip<
        arity_index::NicheRangeInclusive<A::Index>,
        <hybrid_array::Array<T, A::Size> as IntoIterator>::IntoIter,
    >;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0)
    }
}

impl<'a, T, A: Arity> IntoIterator for &'a FixedArray<T, A> {
    type Item = (A::Index, &'a T);
    // Route through the slice so the iterator is the std `slice::Iter` (not
    // `Array::iter`'s inherent `hybrid_array::Iter`), matching the named type.
    type IntoIter = core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_slice().iter())
    }
}

impl<'a, T, A: Arity> IntoIterator for &'a mut FixedArray<T, A> {
    type Item = (A::Index, &'a mut T);
    type IntoIter = core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::IterMut<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_mut_slice().iter_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arity16, Arity8};
    use arity_index::{U3, U4};

    extern crate alloc;
    use alloc::vec::Vec;

    #[test]
    fn from_fn_and_get() {
        let a = FixedArray::<u8, Arity16>::from_fn(U4::as_u8);
        assert_eq!(*a.get(U4::new_masked(0)), 0);
        assert_eq!(*a.get(U4::new_masked(15)), 15);
    }

    #[test]
    fn get_mut_and_replace() {
        let mut a = FixedArray::<u8, Arity8>::from_fn(|_| 0);
        *a.get_mut(U3::new_masked(2)) = 42;
        assert_eq!(*a.get(U3::new_masked(2)), 42);
        let prev = a.replace(U3::new_masked(2), 7);
        assert_eq!(prev, 42);
        assert_eq!(*a.get(U3::new_masked(2)), 7);
    }

    #[test]
    fn index_ops_and_deref() {
        let mut a = FixedArray::<u8, Arity16>::from_fn(U4::as_u8);
        assert_eq!(a[U4::new_masked(3)], 3);
        a[U4::new_masked(3)] = 99;
        assert_eq!(a[U4::new_masked(3)], 99);
        // Deref to [T]
        assert_eq!(a.len(), 16);
        assert_eq!(a.iter().copied().max(), Some(99));
    }

    #[test]
    fn into_iter_pairs_with_index() {
        let a = FixedArray::<u8, Arity8>::from_fn(|i| i.as_u8() * 2);
        let pairs: Vec<(u8, u8)> = (&a).into_iter().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(pairs, alloc::vec![(0, 0), (1, 2), (2, 4), (3, 6), (4, 8), (5, 10), (6, 12), (7, 14)]);
        // value iterator is double-ended
        let last = a.into_iter().next_back().map(|(i, v)| (i.as_u8(), v));
        assert_eq!(last, Some((7, 14)));
    }

    #[test]
    fn map_threads_index() {
        let a = FixedArray::<u8, Arity8>::from_fn(|_| 1);
        let b = a.map(|i, v| i.as_u8() + v);
        let got: Vec<u8> = (&b).into_iter().map(|(_, &v)| v).collect();
        assert_eq!(got, alloc::vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
