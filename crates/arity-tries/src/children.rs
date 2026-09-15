//! The children representation: a marker type selecting one of the three
//! `arity-arrays` containers, and the operation set the algorithms use on it.
//!
//! The marker exists because the edge type `E` is the application's own
//! nominal type, typically containing `Box<Node<V, E, A, S>>` or the node
//! itself, and naming the container as a bare type parameter would force the
//! application to spell out that recursion. [`ChildStore::Map`] names it once.

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::bitmap::Bitmap;

/// Selects the children container for a [`Node`](crate::Node).
pub trait ChildStore<A: Arity> {
    /// The container holding edges of type `E`, indexed by `A::Index`.
    type Map<E>: ChildMap<A, E>;
}

/// [`PackedArray`]: pointer-sized, zero heap when empty, reallocates on an
/// insert or remove that changes the child set.
pub struct Packed;

/// [`GappedArray`]: pointer-sized, with spare capacity so inserts and removes
/// rarely reallocate.
pub struct Gapped;

/// [`FixedArray`]: one inline slot per index, never reallocates, but every
/// node carries a full-width array even when it has no children.
pub struct Fixed;

impl<A: Arity> ChildStore<A> for Packed {
    type Map<E> = PackedArray<E, A>;
}

impl<A: Arity> ChildStore<A> for Gapped {
    type Map<E> = GappedArray<E, A>;
}

impl<A: Arity> ChildStore<A> for Fixed {
    type Map<E> = FixedArray<Option<E>, A>;
}

/// The index-to-edge map the algorithms use, and nothing more.
///
/// [`into_edges`](Self::into_edges) consumes the map and yields the present
/// edges ascending; it is how [`drop_subtree`](crate::drop_subtree) takes a
/// node apart. Every walk that revisits a node after descending into one of
/// its children holds the [`present`](Self::present) bitmap of the children
/// still to visit, which is `Copy`, at most 32 bytes, and costs the same to
/// advance at every fanout.
pub trait ChildMap<A: Arity, E>: Default {
    /// The edge at `index`, if present.
    fn get(&self, index: A::Index) -> Option<&E>;
    /// The edge at `index`, mutably, if present.
    fn get_mut(&mut self, index: A::Index) -> Option<&mut E>;
    /// Stores `edge` at `index`, returning the edge it displaced.
    fn insert(&mut self, index: A::Index, edge: E) -> Option<E>;
    /// Removes and returns the edge at `index`.
    fn remove(&mut self, index: A::Index) -> Option<E>;
    /// The number of present edges.
    fn count(&self) -> usize;
    /// `true` if no edge is present.
    fn is_empty(&self) -> bool {
        self.count() == 0
    }
    /// If exactly one edge is present, removes and returns it with its index.
    fn take_only_child(&mut self) -> Option<(A::Index, E)>;
    /// The present edges, ascending by index.
    fn iter_present<'a>(&'a self) -> impl DoubleEndedIterator<Item = (A::Index, &'a E)> + Clone
    where
        E: 'a;
    /// The set of present indices.
    fn present(&self) -> A::Bitmap;
    /// Consumes the map, yielding the present edges ascending.
    fn into_edges(self) -> impl Iterator<Item = (A::Index, E)>;
}

impl<A: Arity, E> ChildMap<A, E> for PackedArray<E, A> {
    fn get(&self, index: A::Index) -> Option<&E> {
        Self::get(self, index)
    }

    fn get_mut(&mut self, index: A::Index) -> Option<&mut E> {
        Self::get_mut(self, index)
    }

    fn insert(&mut self, index: A::Index, edge: E) -> Option<E> {
        Self::insert(self, index, edge)
    }

    fn remove(&mut self, index: A::Index) -> Option<E> {
        Self::remove(self, index)
    }

    fn count(&self) -> usize {
        Self::count(self)
    }

    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }

    fn take_only_child(&mut self) -> Option<(A::Index, E)> {
        Self::take_only_child(self)
    }

    fn iter_present<'a>(&'a self) -> impl DoubleEndedIterator<Item = (A::Index, &'a E)> + Clone
    where
        E: 'a,
    {
        Self::iter_present(self)
    }

    fn into_edges(self) -> impl Iterator<Item = (A::Index, E)> {
        self.into_iter()
    }

    fn present(&self) -> A::Bitmap {
        self.bitmap()
    }
}

impl<A: Arity, E> ChildMap<A, E> for GappedArray<E, A> {
    fn get(&self, index: A::Index) -> Option<&E> {
        Self::get(self, index)
    }

    fn get_mut(&mut self, index: A::Index) -> Option<&mut E> {
        Self::get_mut(self, index)
    }

    fn insert(&mut self, index: A::Index, edge: E) -> Option<E> {
        Self::insert(self, index, edge)
    }

    fn remove(&mut self, index: A::Index) -> Option<E> {
        Self::remove(self, index)
    }

    fn count(&self) -> usize {
        Self::count(self)
    }

    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }

    fn take_only_child(&mut self) -> Option<(A::Index, E)> {
        Self::take_only_child(self)
    }

    fn iter_present<'a>(&'a self) -> impl DoubleEndedIterator<Item = (A::Index, &'a E)> + Clone
    where
        E: 'a,
    {
        Self::iter_present(self)
    }

    fn into_edges(self) -> impl Iterator<Item = (A::Index, E)> {
        self.into_iter()
    }

    fn present(&self) -> A::Bitmap {
        self.bitmap()
    }
}

impl<A: Arity, E> ChildMap<A, E> for FixedArray<Option<E>, A> {
    fn get(&self, index: A::Index) -> Option<&E> {
        Self::get(self, index).as_ref()
    }

    fn get_mut(&mut self, index: A::Index) -> Option<&mut E> {
        Self::get_mut(self, index).as_mut()
    }

    fn insert(&mut self, index: A::Index, edge: E) -> Option<E> {
        self.replace(index, Some(edge))
    }

    fn remove(&mut self, index: A::Index) -> Option<E> {
        self.take(index)
    }

    fn count(&self) -> usize {
        Self::count(self)
    }

    fn take_only_child(&mut self) -> Option<(A::Index, E)> {
        Self::take_only_child(self)
    }

    fn iter_present<'a>(&'a self) -> impl DoubleEndedIterator<Item = (A::Index, &'a E)> + Clone
    where
        E: 'a,
    {
        self.into_iter()
            .filter_map(|(i, slot)| slot.as_ref().map(|edge| (i, edge)))
    }

    fn into_edges(self) -> impl Iterator<Item = (A::Index, E)> {
        self.into_iter()
            .filter_map(|(i, slot)| slot.map(|edge| (i, edge)))
    }

    /// One scan of the array; the other two representations keep the bitmap.
    fn present(&self) -> A::Bitmap {
        self.into_iter()
            .filter(|(_, slot)| slot.is_some())
            .fold(A::Bitmap::ZERO, |bits, (i, _)| bits.with_bit(i))
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use arity_arrays::Arity16;
    use arity_arrays::Arity256;
    use arity_arrays::bitmap::Bitmap;
    use arity_arrays::index::U4;

    use super::*;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    fn exercise<M: ChildMap<Arity16, u32>>() {
        let mut m = M::default();
        assert!(m.is_empty());
        assert_eq!(m.count(), 0);
        assert!(m.present().is_zero());
        assert_eq!(m.take_only_child(), None);

        assert_eq!(m.insert(u4(3), 30), None);
        assert_eq!(m.insert(u4(9), 90), None);
        assert_eq!(m.insert(u4(3), 31), Some(30));
        assert_eq!(m.count(), 2);
        assert!(!m.is_empty());
        assert_eq!(m.get(u4(3)), Some(&31));
        assert_eq!(m.get(u4(4)), None);
        *m.get_mut(u4(9)).expect("present") += 1;
        assert_eq!(m.get(u4(9)), Some(&91));

        let present: Vec<u8> = m.present().bits().map(U4::as_u8).collect();
        assert_eq!(present, [3, 9]);
        let pairs: Vec<(u8, u32)> = m.iter_present().map(|(i, v)| (i.as_u8(), *v)).collect();
        assert_eq!(pairs, [(3, 31), (9, 91)]);
        let cloned = m.iter_present().clone().next_back().map(|(i, _)| i.as_u8());
        assert_eq!(cloned, Some(9));

        assert_eq!(m.take_only_child(), None);
        assert_eq!(m.remove(u4(3)), Some(31));
        assert_eq!(m.remove(u4(3)), None);
        assert_eq!(m.take_only_child(), Some((u4(9), 91)));
        assert!(m.is_empty());

        m.insert(u4(0), 1);
        m.insert(u4(15), 2);
        let drained: Vec<(u8, u32)> = m.into_edges().map(|(i, v)| (i.as_u8(), v)).collect();
        assert_eq!(drained, [(0, 1), (15, 2)]);
    }

    #[test]
    fn packed_map_supports_the_operation_set() {
        exercise::<<Packed as ChildStore<Arity16>>::Map<u32>>();
    }

    #[test]
    fn gapped_map_supports_the_operation_set() {
        exercise::<<Gapped as ChildStore<Arity16>>::Map<u32>>();
    }

    #[test]
    fn fixed_map_supports_the_operation_set() {
        exercise::<<Fixed as ChildStore<Arity16>>::Map<u32>>();
    }

    #[test]
    fn present_matches_iteration_at_arity_256() {
        let mut m = <Fixed as ChildStore<Arity256>>::Map::<u8>::default();
        for i in [0u8, 7, 128, 255] {
            m.insert(i, i);
        }
        let from_bits: Vec<u8> = m.present().bits().collect();
        let from_iter: Vec<u8> = m.iter_present().map(|(i, _)| i).collect();
        assert_eq!(from_bits, from_iter);
        assert_eq!(m.present().count_ones(), 4);
    }
}
