//! The reference in-memory store: owning edges, reference handles.

use alloc::boxed::Box;
use core::convert::Infallible;
use core::fmt;
use core::marker::PhantomData;

use arity_arrays::Arity;

use crate::Node;
use crate::children::ChildStore;
use crate::node::drop_subtree;
use crate::store::EdgeStore;

/// An owning edge: the node behind a `Box`, and its hash once sealed.
///
/// The same shape as Firewood's inline child, so the crate's own tests
/// exercise the adopter's shape by default. `Drop` tears the subtree down
/// through [`drop_subtree`], taking the node out with `mem::take` first, so a
/// chain of any depth drops on any stack.
pub struct MemEdge<V, A: Arity, S: ChildStore<A>, H> {
    node: Box<Node<V, Self, A, S>>,
    hash: Option<H>,
}

impl<V, A: Arity, S: ChildStore<A>, H> Drop for MemEdge<V, A, S, H> {
    fn drop(&mut self) {
        if !self.node.is_leaf() {
            drop_subtree(core::mem::take(&mut *self.node), |mut edge| {
                Some(core::mem::take(&mut *edge.node))
            });
        }
    }
}

/// Prints only whether the edge is sealed and its hash, never the subtree.
impl<V, A: Arity, S: ChildStore<A>, H: fmt::Debug> fmt::Debug for MemEdge<V, A, S, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemEdge")
            .field("sealed", &self.hash.is_some())
            .field("hash", &self.hash)
            .finish_non_exhaustive()
    }
}

/// The zero-sized store over [`MemEdge`]; it cannot fail and allocates only
/// in [`inline`](EdgeStore::inline), the one place a new node enters the trie.
pub struct InMemory<H>(PhantomData<H>);

impl<H> Default for InMemory<H> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<V, A: Arity, S: ChildStore<A>, H> EdgeStore<V, A, S> for InMemory<H> {
    type Edge = MemEdge<V, A, S, H>;
    type Hash = H;
    type Error = Infallible;
    type Shared<'e>
        = &'e Node<V, Self::Edge, A, S>
    where
        Self: 'e,
        V: 'e,
        A: 'e,
        S: 'e;

    fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<Self::Shared<'e>, Infallible> {
        Ok(&edge.node)
    }

    fn as_inline(edge: &mut Self::Edge) -> Option<&mut Node<V, Self::Edge, A, S>> {
        edge.hash.is_none().then_some(&mut edge.node)
    }

    fn materialize<'e>(
        &mut self,
        edge: &'e mut Self::Edge,
    ) -> Result<&'e mut Node<V, Self::Edge, A, S>, Infallible> {
        edge.hash = None;
        Ok(&mut edge.node)
    }

    fn inline(&mut self, node: Node<V, Self::Edge, A, S>) -> Self::Edge {
        MemEdge {
            node: Box::new(node),
            hash: None,
        }
    }

    fn seal(&mut self, edge: &mut Self::Edge, hash: H) {
        edge.hash = Some(hash);
    }

    fn hash(edge: &Self::Edge) -> Option<&H> {
        edge.hash.as_ref()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::format;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::store::EdgeStore;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    #[test]
    fn inline_edges_are_readable_and_unhashed() {
        let mut store = St::default();
        let mut edge = store.inline(N::leaf(Path::new(), 1));
        assert_eq!(St::hash(&edge), None);
        assert_eq!(
            St::as_inline(&mut edge).map(|n| n.value().copied()),
            Some(Some(1))
        );
        assert_eq!(store.read(&edge).expect("infallible").value(), Some(&1));
        assert_eq!(
            format!("{edge:?}"),
            "MemEdge { sealed: false, hash: None, .. }"
        );
    }

    #[test]
    fn seal_then_materialize_round_trips_the_state() {
        let mut store = St::default();
        let mut edge = store.inline(N::leaf(Path::new(), 1));
        store.seal(&mut edge, 42);
        assert_eq!(St::hash(&edge), Some(&42));
        assert!(St::as_inline(&mut edge).is_none());
        assert_eq!(store.read(&edge).expect("infallible").value(), Some(&1));
        assert_eq!(
            format!("{edge:?}"),
            "MemEdge { sealed: true, hash: Some(42), .. }"
        );
        store.seal(&mut edge, 43);
        assert_eq!(St::hash(&edge), Some(&43));
        let node = store.materialize(&mut edge).expect("infallible");
        assert_eq!(node.value(), Some(&1));
        assert_eq!(St::hash(&edge), None);
        assert!(St::as_inline(&mut edge).is_some());
    }

    fn chain(depth: usize) -> N {
        let mut store = St::default();
        let mut node = N::leaf(Path::new(), 0);
        for _ in 0..depth {
            let mut parent = N::new(Path::new(), None, PackedArray::default());
            parent
                .children_mut()
                .insert(U4::new_masked(0), store.inline(node));
            node = parent;
        }
        node
    }

    #[test]
    fn dropping_a_deep_trie_does_not_recurse() {
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| drop(chain(if cfg!(miri) { 256 } else { 100_000 })))
            .expect("spawn");
        handle.join().expect("no overflow");
    }
}
