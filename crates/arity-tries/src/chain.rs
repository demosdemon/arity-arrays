//! The erased-lifetime handle chain behind lookup, iteration, and validation.
//!
//! # Invariant
//!
//! A chain holds [`EdgeStore::Shared`] handles in which each handle borrows the
//! node its predecessor dereferences to. The borrow lifetimes are erased to
//! the chain's own `'a` so the handles share one growable buffer rather than
//! costing an allocation each. This is sound
//! because `Shared` implements
//! [`StableDeref`](stable_deref_trait::StableDeref), which guarantees the
//! pointee does not move when the handle is moved (including by the backing
//! `Vec` reallocating), and because handles are released in reverse order of
//! pushing, so no handle outlives the node it borrows. `Vec` makes no
//! guarantee about the order it drops its elements in (today it is front to
//! back, the wrong order), so the only way to remove an element is `pop`, and
//! `Drop` loops `pop` until empty. That loop is guarded against a handle
//! whose destructor panics: unwinding out of it would otherwise drop the
//! remaining handles front to back, so a guard finishes the reverse release
//! during the unwind, and the buffer sits behind `ManuallyDrop` so nothing
//! but the guard ever drops it.

use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use core::mem::transmute_copy;
use core::ptr;

use arity_arrays::Arity;

use crate::Node;
use crate::children::ChildStore;
use crate::store::EdgeStore;

pub struct ChainStack<'a, St, V, A, S>
where
    St: EdgeStore<V, A, S> + 'a,
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
{
    /// Only [`Unwind`] drops this, after emptying it in reverse.
    handles: ManuallyDrop<Vec<St::Shared<'a>>>,
}

impl<'a, St, V, A, S> ChainStack<'a, St, V, A, S>
where
    St: EdgeStore<V, A, S> + 'a,
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
{
    pub const fn new() -> Self {
        Self {
            handles: ManuallyDrop::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Pushes a handle whose borrow is shorter than `'a` and returns a pointer
    /// to the node it dereferences to, valid until the handle is popped.
    ///
    /// The caller must have obtained `handle` from an edge of the node the
    /// current last handle dereferences to (or from the root when the chain
    /// is empty); that is what makes the reverse-order release below sound.
    pub fn push<'e>(&mut self, handle: St::Shared<'e>) -> *const Node<V, St::Edge, A, S>
    where
        St: 'e,
        V: 'e,
        A: 'e,
        S: 'e,
    {
        let handle = ManuallyDrop::new(handle);
        // SAFETY: `St::Shared<'e>` and `St::Shared<'a>` are the same type up
        // to the lifetime parameter, so they have the same layout, and the
        // source is not dropped (`ManuallyDrop`) so ownership moves exactly
        // once. Lengthening `'e` to `'a` is sound under the module invariant:
        // the handle is released before its predecessor, which is what keeps
        // the node it borrows alive, and `StableDeref` keeps that node in
        // place while the handle sits in the `Vec`. `mem::transmute` is not
        // used because it rejects generic types whose sizes it cannot
        // compare.
        let erased = unsafe { transmute_copy::<St::Shared<'e>, St::Shared<'a>>(&*handle) };
        self.handles.push(erased);
        let last = self.handles.last().expect("just pushed");
        ptr::from_ref(&**last)
    }

    /// Releases the last handle. Returns `false` if the chain was empty.
    pub fn pop(&mut self) -> bool {
        self.handles.pop().is_some()
    }
}

impl<'a, St, V, A, S> Drop for ChainStack<'a, St, V, A, S>
where
    St: EdgeStore<V, A, S> + 'a,
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
{
    fn drop(&mut self) {
        let guard = Unwind(&mut self.handles);
        while guard.0.pop().is_some() {}
        // `guard` drops here on the normal path with nothing left to pop, and
        // during unwinding if a destructor above panicked, where it releases
        // the remaining handles in reverse; a second panic then aborts, which
        // is the only outcome left that is not undefined behavior.
    }
}

/// Finishes releasing a chain's handles in reverse order and frees the
/// buffer, whether reached normally or by unwinding.
struct Unwind<'v, H>(&'v mut ManuallyDrop<Vec<H>>);

impl<H> Drop for Unwind<'_, H> {
    fn drop(&mut self) {
        while self.0.pop().is_some() {}
        // SAFETY: the vector is empty, so this only frees the buffer, and it
        // runs exactly once: the chain's `Drop` is the sole creator of this
        // guard and never touches `handles` again.
        unsafe { ManuallyDrop::drop(self.0) }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::Cell;
    use core::cell::RefCell;
    use core::convert::Infallible;
    use core::ops::Deref;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;
    use stable_deref_trait::StableDeref;

    use super::*;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::store::EdgeStore;

    type N = Node<u32, Edge, Arity16, Packed>;

    /// A reference edge whose handle logs its own drop.
    struct Edge(Rc<N>, u32);

    struct Logged<'e>(&'e N, u32, &'e RefCell<Vec<u32>>, u32);

    impl Deref for Logged<'_> {
        type Target = N;
        fn deref(&self) -> &N {
            self.0
        }
    }

    // SAFETY: the handle dereferences to a node behind a plain reference,
    // which does not move when the handle moves.
    unsafe impl StableDeref for Logged<'_> {}

    impl Drop for Logged<'_> {
        fn drop(&mut self) {
            self.2.borrow_mut().push(self.1);
            assert_ne!(
                self.1, self.3,
                "destructor of handle {} panics on purpose",
                self.1
            );
        }
    }

    struct LogStore(RefCell<Vec<u32>>, Cell<u32>);

    impl EdgeStore<u32, Arity16, Packed> for LogStore {
        type Edge = Edge;
        type Hash = ();
        type Error = Infallible;
        type Shared<'e>
            = Logged<'e>
        where
            Self: 'e;

        fn read<'e>(&'e self, edge: &'e Edge) -> Result<Logged<'e>, Infallible> {
            Ok(Logged(&edge.0, edge.1, &self.0, self.1.get()))
        }
        fn as_inline(_: &mut Edge) -> Option<&mut N> {
            None
        }
        fn materialize<'e>(&mut self, _: &'e mut Edge) -> Result<&'e mut N, Infallible> {
            unreachable!("reference edges are never materialized here")
        }
        fn inline(&mut self, _: N) -> Edge {
            unreachable!("nothing is inserted here")
        }
        fn seal(&mut self, _: &mut Edge, (): ()) {}
        fn hash(_: &Edge) -> Option<&()> {
            None
        }
    }

    fn chain_of(depth: u32) -> N {
        let mut node = N::leaf(Path::new(), 0);
        for level in (1..=depth).rev() {
            let mut parent = N::new(Path::new(), None, PackedArray::default());
            parent
                .children_mut()
                .insert(U4::new_masked(0), Edge(Rc::new(node), level));
            node = parent;
        }
        node
    }

    #[test]
    fn handles_are_released_deepest_first_on_drop() {
        let root = chain_of(3);
        let store = LogStore(RefCell::new(Vec::new()), Cell::new(0));
        {
            let mut chain = ChainStack::<LogStore, u32, Arity16, Packed>::new();
            let mut cur: &N = &root;
            for _ in 0..3 {
                let edge = cur.children().get(U4::new_masked(0)).expect("child");
                let handle = store.read(edge).expect("infallible");
                // SAFETY: the handle just pushed stays in the chain, and so
                // does its pointee, until the chain is dropped below.
                cur = unsafe { &*chain.push(handle) };
            }
            assert_eq!(chain.len(), 3);
            assert!(cur.is_leaf());
            assert!(store.0.borrow().is_empty());
        }
        assert_eq!(*store.0.borrow(), [3, 2, 1]);
    }

    #[test]
    fn pop_releases_the_last_handle_only() {
        let root = chain_of(2);
        let store = LogStore(RefCell::new(Vec::new()), Cell::new(0));
        let mut chain = ChainStack::<LogStore, u32, Arity16, Packed>::new();
        let first = store
            .read(root.children().get(U4::new_masked(0)).expect("child"))
            .expect("infallible");
        // SAFETY: `first` stays in the chain until the pops below, and `mid`
        // is not used after them.
        let mid = unsafe { &*chain.push(first) };
        let second = store
            .read(mid.children().get(U4::new_masked(0)).expect("child"))
            .expect("infallible");
        chain.push(second);
        assert!(chain.pop());
        assert_eq!(*store.0.borrow(), [2]);
        assert_eq!(chain.len(), 1);
        assert!(chain.pop());
        assert!(!chain.pop());
        assert_eq!(*store.0.borrow(), [2, 1]);
    }

    /// A handle's destructor panicking must not let the remaining handles
    /// drop front to back: an owning ancestor would be freed before a handle
    /// that borrows from it.
    #[test]
    fn a_panicking_destructor_still_releases_the_rest_deepest_first() {
        let root = chain_of(3);
        let store = LogStore(RefCell::new(Vec::new()), Cell::new(3));
        let mut chain = ChainStack::<LogStore, u32, Arity16, Packed>::new();
        let mut cur: &N = &root;
        for _ in 0..3 {
            let edge = cur.children().get(U4::new_masked(0)).expect("child");
            let handle = store.read(edge).expect("infallible");
            // SAFETY: the handle just pushed stays in the chain, and so does
            // its pointee, until the chain is dropped below.
            cur = unsafe { &*chain.push(handle) };
        }
        assert!(cur.is_leaf());
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(chain)));
        assert!(outcome.is_err(), "the deepest handle's destructor panics");
        assert_eq!(*store.0.borrow(), [3, 2, 1]);
    }
}
