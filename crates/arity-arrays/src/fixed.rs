//! [`FixedArray`]: full-width inline storage, one `T` per slot.

use core::ops::Deref;
use core::ops::DerefMut;
use core::ops::Index;
use core::ops::IndexMut;

use arity_index::Niche;
use hybrid_array::Array;

use crate::Arity;

/// A full-width array with one `T` per slot, indexed by `A::Index` without
/// bounds checks.
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

    /// Returns a reference to the element at `index`.
    ///
    /// Infallible (unlike [`slice::get`]): `A::Index` is a total index type,
    /// so every value of it is in bounds and there is no `None` case.
    #[must_use]
    pub fn get(&self, index: A::Index) -> &T {
        // SAFETY: `A::Index::as_usize()` is always `< Index::COUNT == A::LEN`,
        // which equals the array length, so `index` is in bounds.
        unsafe { self.0.as_slice().get_unchecked(index.as_usize()) }
    }

    /// Returns a mutable reference to the element at `index`.
    ///
    /// Infallible for the same reason as [`get`](Self::get): `A::Index` is a
    /// total index type, so every value is in bounds.
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
    type IntoIter =
        core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_slice().iter())
    }
}

impl<'a, T, A: Arity> IntoIterator for &'a mut FixedArray<T, A> {
    type Item = (A::Index, &'a mut T);
    type IntoIter =
        core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::IterMut<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_mut_slice().iter_mut())
    }
}

// Manual trait impls forwarding to the inner `Array`. These cannot be
// `#[derive]`d: derive would emit a spurious `A: Clone` (etc.) bound on the
// uninhabited `Arity` marker, even though `A` is never stored. Bounds rest on
// `T` alone.
impl<T: Clone, A: Arity> Clone for FixedArray<T, A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: PartialEq, A: Arity> PartialEq for FixedArray<T, A> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Eq, A: Arity> Eq for FixedArray<T, A> {}

impl<T: PartialOrd, A: Arity> PartialOrd for FixedArray<T, A> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T: Ord, A: Arity> Ord for FixedArray<T, A> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T: core::hash::Hash, A: Arity> core::hash::Hash for FixedArray<T, A> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: core::fmt::Debug, A: Arity> core::fmt::Debug for FixedArray<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T, A: Arity> FixedArray<Option<T>, A> {
    /// Creates a `FixedArray` with every slot `None`.
    #[must_use]
    pub fn new() -> Self {
        Self::from_fn(|_| None)
    }

    /// Returns the number of `Some` slots.
    #[must_use]
    pub fn count(&self) -> usize {
        self.iter().filter(|slot| slot.is_some()).count()
    }

    /// Sets the slot at `index` to `None`, returning the previous value.
    pub fn take(&mut self, index: A::Index) -> Option<T> {
        self.replace(index, None)
    }

    /// Iterates over the present (`Some`) slots as `(A::Index, &T)`, ascending.
    pub fn iter_present(&self) -> impl DoubleEndedIterator<Item = (A::Index, &T)> {
        self.into_iter()
            .filter_map(|(i, slot)| slot.as_ref().map(|v| (i, v)))
    }

    /// If exactly one slot is present, takes and returns it with its index;
    /// otherwise returns `None` and leaves the array unchanged.
    pub fn take_only_child(&mut self) -> Option<(A::Index, T)> {
        let mut present = self.iter_present().map(|(i, _)| i);
        let only = present.next()?;
        if present.next().is_some() {
            return None;
        }
        drop(present);
        self.take(only).map(|v| (only, v))
    }
}

impl<T, A: Arity> Default for FixedArray<Option<T>, A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use arity_index::U3;
    use arity_index::U4;

    use super::*;
    use crate::Arity8;
    use crate::Arity16;

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
        assert_eq!(pairs, alloc::vec![
            (0, 0),
            (1, 2),
            (2, 4),
            (3, 6),
            (4, 8),
            (5, 10),
            (6, 12),
            (7, 14)
        ]);
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

    #[test]
    fn option_new_count_take() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        assert_eq!(a.count(), 0);
        a[U4::new_masked(1)] = Some(10);
        a[U4::new_masked(9)] = Some(90);
        assert_eq!(a.count(), 2);
        assert_eq!(a.take(U4::new_masked(1)), Some(10));
        assert_eq!(a.count(), 1);
        assert_eq!(a.take(U4::new_masked(1)), None);
    }

    #[test]
    fn option_iter_present_ascending() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        a[U4::new_masked(3)] = Some(3);
        a[U4::new_masked(11)] = Some(11);
        let got: Vec<(u8, u8)> = a.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(got, alloc::vec![(3, 3), (11, 11)]);
    }

    #[test]
    fn option_take_only_child() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        assert_eq!(a.take_only_child(), None);
        a[U4::new_masked(5)] = Some(50);
        assert_eq!(
            a.take_only_child().map(|(i, v)| (i.as_u8(), v)),
            Some((5, 50))
        );
        assert_eq!(a.count(), 0);
        a[U4::new_masked(2)] = Some(20);
        a[U4::new_masked(6)] = Some(60);
        assert_eq!(a.take_only_child(), None); // two children → None, nothing taken
        assert_eq!(a.count(), 2);
    }

    #[test]
    fn clone_eq_and_ord() {
        let a = FixedArray::<u8, Arity8>::from_fn(U3::as_u8);
        let b = a.clone();
        assert_eq!(a, b);
        let mut c = a.clone();
        c[U3::new_masked(0)] = 100;
        assert_ne!(a, c);
        // Lexicographic ordering: c differs from a at slot 0 (100 > 0).
        assert!(c > a);
        assert!(a < c);
        assert_eq!(a.cmp(&b), core::cmp::Ordering::Equal);
    }

    #[test]
    fn debug_renders_elements() {
        let a = FixedArray::<u8, Arity8>::from_fn(U3::as_u8);
        let s = alloc::format!("{a:?}");
        // Inner `Array` Debug renders as a list of the elements.
        assert!(s.contains('0') && s.contains('7'));
    }

    #[test]
    fn hash_matches_for_equal_arrays() {
        extern crate std;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hash;
        use std::hash::Hasher;

        fn hash_of(a: &FixedArray<u8, Arity8>) -> u64 {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        }

        let a = FixedArray::<u8, Arity8>::from_fn(U3::as_u8);
        let b = a.clone();
        assert_eq!(hash_of(&a), hash_of(&b));
    }
}
