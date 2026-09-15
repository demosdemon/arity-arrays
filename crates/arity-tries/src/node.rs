//! The node type and the non-recursive subtree destructor.

use alloc::vec::Vec;
use core::fmt;

use arity_arrays::Arity;
use arity_arrays::bitmap::Bitmap;
use arity_arrays::index::Niche;

use crate::Path;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::children::Packed;

/// A trie node: a partial path, an optional value, and a map of edges.
///
/// A trie is `Option<Node<..>>`; the empty trie is `None`. The root may be a
/// leaf and may carry a partial path of any length. There is no leaf or
/// branch distinction: a leaf is a node with no children.
///
/// # Structural invariant
///
/// A settled trie has no valueless node with fewer than two children: a node
/// with no value has at least two children and a node with no children has a
/// value. The invariant is a property of tries the crate's own mutation
/// operations produce, not of the type: constructors do not check it and
/// [`children_mut`](Self::children_mut) is public because adopters build
/// structures those operations would not (a valueless single-child root
/// under a parallel inserter, proof nodes).
pub struct Node<V, E, A: Arity, S: ChildStore<A> = Packed> {
    partial_path: Path<A>,
    value: Option<V>,
    children: S::Map<E>,
}

impl<V, E, A: Arity, S: ChildStore<A>> Node<V, E, A, S> {
    /// A node from its parts.
    #[must_use]
    pub const fn new(partial_path: Path<A>, value: Option<V>, children: S::Map<E>) -> Self {
        Self {
            partial_path,
            value,
            children,
        }
    }

    /// A node with a value and no children.
    #[must_use]
    pub fn leaf(partial_path: Path<A>, value: V) -> Self {
        Self::new(partial_path, Some(value), S::Map::default())
    }

    /// The path from this node's parent to it, excluding the index the
    /// parent holds it under.
    #[must_use]
    pub fn partial_path(&self) -> &[A::Index] {
        &self.partial_path
    }

    /// Replaces the partial path, returning the old one.
    pub const fn set_partial_path(&mut self, partial_path: Path<A>) -> Path<A> {
        core::mem::replace(&mut self.partial_path, partial_path)
    }

    /// The value, if any.
    #[must_use]
    pub const fn value(&self) -> Option<&V> {
        self.value.as_ref()
    }

    /// The value, mutably, if any.
    #[must_use]
    pub const fn value_mut(&mut self) -> Option<&mut V> {
        self.value.as_mut()
    }

    /// Replaces the value, returning the old one.
    pub const fn set_value(&mut self, value: Option<V>) -> Option<V> {
        core::mem::replace(&mut self.value, value)
    }

    /// The children map.
    #[must_use]
    pub const fn children(&self) -> &S::Map<E> {
        &self.children
    }

    /// The children map, mutably. See the hash-validity contract on [`Node`].
    #[must_use]
    pub const fn children_mut(&mut self) -> &mut S::Map<E> {
        &mut self.children
    }

    /// `true` if the node has no children.
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Takes the node apart.
    #[must_use]
    pub fn into_parts(self) -> (Path<A>, Option<V>, S::Map<E>) {
        (self.partial_path, self.value, self.children)
    }
}

/// The placeholder node: empty path, no value, empty map.
///
/// It is what `mem::take` leaves behind when an algorithm needs to own a node
/// that sits in an inline edge. None of the three representations touches
/// the heap for it; `Fixed` additionally fills a full-width array of `None`.
impl<V, E, A: Arity, S: ChildStore<A>> Default for Node<V, E, A, S> {
    fn default() -> Self {
        Self::new(Path::new(), None, S::Map::default())
    }
}

/// Shallow: partial path, value, child count, and the present child indices.
/// It does not format the edges, so it cannot recurse through an owning
/// edge's `Debug`.
impl<V: fmt::Debug, E, A: Arity, S: ChildStore<A>> fmt::Debug for Node<V, E, A, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Present<B>(B);
        impl<B: Bitmap> fmt::Debug for Present<B> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_list()
                    .entries(self.0.bits().map(Niche::as_usize))
                    .finish()
            }
        }
        f.debug_struct("Node")
            .field("partial_path", &self.partial_path)
            .field("value", &self.value)
            .field("children", &self.children.count())
            .field("present", &Present(self.children.present()))
            .finish()
    }
}

/// Consumes a node and every node reachable from it without recursion.
///
/// A worklist: the node's edges are pushed, one is popped, `into_node` is
/// applied (returning the owned node for an owning edge and `None` for a
/// reference edge), and that node's edges are pushed in turn. Every node is
/// taken apart before it is dropped, so no drop glue runs over more than one
/// level.
///
/// An owning edge's `Drop` takes its node out with `mem::take` and calls this,
/// and its `into_node` does the same take, so the edge that then drops
/// normally is empty. `MemEdge` is the worked example.
pub fn drop_subtree<V, E, A, S>(
    node: Node<V, E, A, S>,
    mut into_node: impl FnMut(E) -> Option<Node<V, E, A, S>>,
) where
    A: Arity,
    S: ChildStore<A>,
{
    let mut worklist: Vec<E> = Vec::new();
    let (_, _, children) = node.into_parts();
    worklist.extend(children.into_edges().map(|(_, edge)| edge));
    while let Some(edge) = worklist.pop() {
        if let Some(node) = into_node(edge) {
            let (_, _, children) = node.into_parts();
            worklist.extend(children.into_edges().map(|(_, edge)| edge));
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::boxed::Box;
    use alloc::format;
    use alloc::vec::Vec;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::children::Fixed;
    use crate::children::Packed;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    /// An owning edge with no `Drop` of its own, so dropping a deep chain
    /// through the compiler's drop glue would recurse once per level.
    struct BoxEdge(Box<Node<u32, Self, Arity16, Packed>>);

    fn path(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    #[test]
    fn leaf_has_a_value_and_no_children() {
        let leaf = Node::<u32, BoxEdge, Arity16>::leaf(path(&[1, 2]), 7);
        assert_eq!(leaf.partial_path(), &[u4(1), u4(2)]);
        assert_eq!(leaf.value(), Some(&7));
        assert!(leaf.is_leaf());
        assert_eq!(leaf.children().count(), 0);
    }

    #[test]
    fn accessors_replace_and_return_the_old_parts() {
        let mut node = Node::<u32, BoxEdge, Arity16>::new(path(&[1]), None, PackedArray::default());
        assert!(node.value().is_none());
        assert_eq!(node.set_value(Some(3)), None);
        assert_eq!(node.set_value(Some(4)), Some(3));
        *node.value_mut().expect("set") += 1;
        assert_eq!(node.value(), Some(&5));
        let old = node.set_partial_path(path(&[9, 9]));
        assert_eq!(&*old, &[u4(1)]);
        assert_eq!(node.partial_path(), &[u4(9), u4(9)]);
        node.children_mut()
            .insert(u4(2), BoxEdge(Box::new(Node::leaf(Path::new(), 1))));
        assert!(!node.is_leaf());
        let (p, v, children) = node.into_parts();
        assert_eq!(&*p, &[u4(9), u4(9)]);
        assert_eq!(v, Some(5));
        assert_eq!(children.count(), 1);
    }

    #[test]
    fn default_is_the_empty_placeholder() {
        let node = Node::<u32, BoxEdge, Arity16, Fixed>::default();
        assert_eq!(node.partial_path().len(), 0);
        assert_eq!(node.value(), None);
        assert!(node.is_leaf());
    }

    #[test]
    fn debug_is_shallow() {
        let mut node =
            Node::<u32, BoxEdge, Arity16>::new(path(&[1, 2]), Some(5), PackedArray::default());
        node.children_mut()
            .insert(u4(3), BoxEdge(Box::new(Node::leaf(Path::new(), 1))));
        node.children_mut()
            .insert(u4(11), BoxEdge(Box::new(Node::leaf(Path::new(), 2))));
        let text = format!("{node:?}");
        assert_eq!(
            text,
            "Node { partial_path: [1, 2], value: Some(5), children: 2, present: [3, 11] }"
        );
    }

    fn chain(depth: usize) -> Node<u32, BoxEdge, Arity16, Packed> {
        let mut node: Node<u32, BoxEdge, Arity16, Packed> = Node::leaf(Path::new(), 0);
        for _ in 0..depth {
            let mut parent: Node<u32, BoxEdge, Arity16, Packed> =
                Node::new(Path::new(), None, PackedArray::default());
            parent.children_mut().insert(u4(0), BoxEdge(Box::new(node)));
            node = parent;
        }
        node
    }

    #[test]
    fn drop_subtree_visits_every_node_without_recursion() {
        let depth = if cfg!(miri) { 256 } else { 100_000 };
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut visited = 0usize;
                drop_subtree(chain(depth), |edge: BoxEdge| {
                    visited += 1;
                    Some(*edge.0)
                });
                visited
            })
            .expect("spawn");
        assert_eq!(handle.join().expect("worklist thread panicked"), depth);
    }

    #[test]
    fn drop_subtree_skips_reference_edges() {
        let mut node = Node::<u32, u8, Arity16>::new(Path::new(), None, PackedArray::default());
        node.children_mut().insert(u4(1), 10);
        node.children_mut().insert(u4(2), 20);
        let mut seen = Vec::new();
        drop_subtree(node, |edge| {
            seen.push(edge);
            None
        });
        seen.sort_unstable();
        assert_eq!(seen, [10, 20]);
    }
}
