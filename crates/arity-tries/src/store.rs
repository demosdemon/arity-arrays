//! The edge store trait and the lookup result type.

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

use arity_arrays::Arity;
use stable_deref_trait::StableDeref;

use crate::Node;
use crate::chain::ChainStack;
use crate::children::ChildStore;

/// The application's node store: how edges resolve to nodes, and how nodes
/// move between the inline and sealed states.
///
/// This trait, [`ChildMap`](crate::ChildMap), and the shape of [`Node`] are
/// the crate's hard-to-reverse surface: while the crate is at `0.x`, any
/// change to them bumps the minor version.
///
/// # Edge states
///
/// An edge is *inline* when [`hash`](Self::hash) is `None` and *sealed* when
/// it is `Some`. [`as_inline`](Self::as_inline) returns `Some` exactly for
/// inline edges; [`materialize`](Self::materialize) turns a sealed edge
/// inline and [`seal`](Self::seal) turns an inline edge sealed. The mutation
/// operations in [`ops`](crate::ops) leave every node on a mutated path
/// inline.
///
/// # Why `materialize` and `as_inline` are safe to implement
///
/// The mutation walks derive raw pointers from the `&mut Node` these two
/// return. The property those pointers rely on, that the reference is the
/// only live one to that node for as long as the edge borrow lasts, is what
/// the type system guarantees of every safe implementation: a safe store
/// cannot return an `&'e mut Node` that aliases anything else reachable while
/// `'e` is live, and cannot return one that lives in its own state, because
/// the lifetime is the edge's, not the store's. A store that produces two
/// live exclusive references to one node has written `unsafe` of its own and
/// owns that bug. [`StableDeref`] is different: no safe signature can say
/// that a handle's pointee stays put when the handle moves, so that trait is
/// `unsafe` and an adopter whose handle is not a plain reference or a
/// standard smart pointer writes `unsafe impl` for its
/// [`Shared`](Self::Shared).
pub trait EdgeStore<V, A: Arity, S: ChildStore<A>> {
    /// The edge type held in a node's children map.
    type Edge;
    /// The hash a sealed edge carries.
    type Hash;
    /// The store's failure type; `Infallible` for a store that cannot fail.
    type Error;
    /// A read handle: a borrowed node (a store whose inline edge holds the
    /// node) or an owned one (an `Arc` read from disk). `StableDeref` is what
    /// lets the lookup chain hold a handle whose successor borrows from it.
    type Shared<'e>: StableDeref<Target = Node<V, Self::Edge, A, S>>
    where
        Self: 'e,
        V: 'e,
        A: 'e,
        S: 'e;

    /// Resolves any edge, inline or sealed, to a handle.
    ///
    /// # Errors
    ///
    /// Whatever resolving the edge can fail with; I/O, for a persistent store.
    fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<Self::Shared<'e>, Self::Error>;

    /// The node behind an inline edge, without the store and without I/O;
    /// `None` for a sealed edge.
    fn as_inline(edge: &mut Self::Edge) -> Option<&mut Node<V, Self::Edge, A, S>>;

    /// Turns a sealed edge into an inline one in place and returns its node.
    ///
    /// It is the store's one chance to record that the edge, and any
    /// persistent node behind it, is being replaced. On an inline edge it
    /// returns the node and records nothing. On `Err` the edge is unchanged
    /// and a later call on it behaves as if the failed one never happened, so
    /// a store that records replacements must record only after the node is
    /// in hand. The returned borrow is tied to the edge, not the store.
    ///
    /// # Errors
    ///
    /// Whatever reading the node behind a sealed edge can fail with. The edge
    /// is unchanged on `Err`.
    fn materialize<'e>(
        &mut self,
        edge: &'e mut Self::Edge,
    ) -> Result<&'e mut Node<V, Self::Edge, A, S>, Self::Error>;

    /// A fresh inline edge around a node the algorithms created.
    fn inline(&mut self, node: Node<V, Self::Edge, A, S>) -> Self::Edge;

    /// Marks an inline edge sealed with its hash, in place; on a sealed edge,
    /// replaces the cached hash.
    fn seal(&mut self, edge: &mut Self::Edge, hash: Self::Hash);

    /// The cached hash of a sealed edge, `None` for an inline one. Takes no
    /// store because the hash lives on the edge.
    fn hash(edge: &Self::Edge) -> Option<&Self::Hash>;
}

/// The result of a lookup: the found node, kept alive by the chain of read
/// handles from the root down to it.
///
/// It borrows the root and the store for `'a`. For a store whose handles are
/// plain references the chain is a `Vec` of references: one growable buffer
/// per lookup rather than an allocation per level.
pub struct NodeRef<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    _chain: ChainStack<'a, St, V, A, S>,
    node: *const Node<V, St::Edge, A, S>,
    _borrow: PhantomData<&'a Node<V, St::Edge, A, S>>,
}

impl<'a, V, A, S, St> NodeRef<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    pub(crate) const fn new(
        chain: ChainStack<'a, St, V, A, S>,
        node: *const Node<V, St::Edge, A, S>,
    ) -> Self {
        Self {
            _chain: chain,
            node,
            _borrow: PhantomData,
        }
    }
}

impl<'a, V, A, S, St> Deref for NodeRef<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    type Target = Node<V, St::Edge, A, S>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `node` is either the root, borrowed for `'a`, or the pointee
        // of the last handle in `_chain`, which `StableDeref` keeps in place
        // for as long as the handle lives, and the handle lives as long as
        // `self` because `_chain` is only dropped with it.
        unsafe { &*self.node }
    }
}

impl<'a, V, A, S, St> fmt::Debug for NodeRef<'a, V, A, S, St>
where
    V: fmt::Debug + 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

// SAFETY: the raw pointer stands in for a `&'a Node` and the chain is a `Vec`
// of `St::Shared<'a>` handles, so the bounds are those the safe equivalent
// would get for free: `Sync` on the node and `Send` on the handles. No store
// reference is held, so nothing is required of the store itself; a handle
// that borrows the store carries that requirement in its own `Send`. The
// `_chain` field is dropped in reverse order on any thread, which needs no
// more than `Send` on the handles.
unsafe impl<'a, V, A, S, St> Send for NodeRef<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
    St::Shared<'a>: Send,
    Node<V, St::Edge, A, S>: Sync,
{
}

// SAFETY: sharing a `NodeRef` shares a `&Node` and a `&St::Shared<'a>` per
// level, which is sound exactly when both are `Sync`.
unsafe impl<'a, V, A, S, St> Sync for NodeRef<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
    St::Shared<'a>: Sync,
    Node<V, St::Edge, A, S>: Sync,
{
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use arity_arrays::Arity16;

    use crate::InMemory;
    use crate::Node;
    use crate::Path;
    use crate::get_node;

    #[test]
    fn node_ref_derefs_and_formats_like_its_node() {
        let key = Path::<Arity16>::try_from_bytes(&[1]).expect("ok");
        let root = Node::<u32, _, Arity16>::leaf(key.clone(), 9);
        let store = InMemory::<u64>::default();
        let found = get_node(Some(&root), &store, &key)
            .expect("infallible")
            .expect("found");
        assert_eq!(found.value(), Some(&9));
        assert_eq!(format!("{found:?}"), format!("{root:?}"));
    }
}
