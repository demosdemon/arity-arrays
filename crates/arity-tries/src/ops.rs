//! Lookup and mutation over a store.
//!
//! # Mutation
//!
//! `insert`, `remove`, and `remove_prefix` take `root: &mut Option<Node>` and
//! `store: &mut St`. Each descends by recording the path as a stack of node
//! pointers (see the private `frames` module for the invariant that makes the
//! pointers sound). `insert` applies its change to the node the descent stops
//! on and has nothing to unwind; `remove` and `remove_prefix` then unwind,
//! popping the frames and repairing each parent after its child frame is
//! gone. Descent materializes every edge on the path, so every node on it
//! ends up inline and unhashed. This mirrors Firewood's copy-on-write path
//! copying without allocating a fresh node at each level.
//!
//! # Error state
//!
//! [`EdgeStore::materialize`] is the only fallible step, and every mutation
//! performs all of its `materialize` calls before it changes anything. A
//! failed call leaves the edge unchanged, so on `Err` the key set and every
//! value are unchanged, the structural invariant still holds if it held
//! before, and `root` remains valid. What does change on failure is
//! representation: the nodes along the descended path up to the failure are
//! now inline, and the store has recorded the sealed edges it replaced. That
//! is the same state a successful no-op mutation along that path would leave,
//! and a later hash walk repairs it. The caller keeps its trie and may retry.
//!
//! # Memory
//!
//! Heap use is proportional to depth for every operation except
//! [`remove_prefix`], whose all-or-nothing contract holds the deleted subtree
//! resident until the whole of it is materialized. That is bounded by the
//! caller's own data, not by anything a hostile key can supply. A caller that
//! must cap the spike removes a large prefix in chunks, calling [`remove`]
//! per key.

use core::ptr;

use arity_arrays::Arity;
use arity_arrays::bitmap::Bitmap;

use crate::Node;
use crate::Path;
use crate::chain::ChainStack;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::frames::Frames;
use crate::path::common_prefix;
use crate::path::join;
use crate::store::EdgeStore;
use crate::store::NodeRef;

/// The node whose full path equals `key`, valued or not.
///
/// A loop over a handle chain, so a key of any length is looked up on a
/// bounded stack.
///
/// # Errors
///
/// The store's error from any [`read`](EdgeStore::read) along the path.
#[expect(
    clippy::type_complexity,
    reason = "the layering of the result is the API: a store error, then found or not, then the \
              node; an alias would hide it from the reader"
)]
pub fn get_node<'a, V, A, S, St>(
    root: Option<&'a Node<V, St::Edge, A, S>>,
    store: &'a St,
    key: &[A::Index],
) -> Result<Option<NodeRef<'a, V, A, S, St>>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let Some(root) = root else {
        return Ok(None);
    };
    let mut chain = ChainStack::new();
    let mut cur: *const Node<V, St::Edge, A, S> = ptr::from_ref(root);
    let mut rest = key;
    loop {
        // SAFETY: `cur` is the root, borrowed for `'a`, or the pointee of the
        // chain's last handle, which stays in place until that handle is
        // popped, and nothing pops while `cur` is in use.
        let node = unsafe { &*cur };
        let o = common_prefix(rest, node.partial_path());
        if !o.unique_b.is_empty() {
            return Ok(None);
        }
        let Some((&i, key_rest)) = o.unique_a.split_first() else {
            return Ok(Some(NodeRef::new(chain, cur)));
        };
        let Some(edge) = node.children().get(i) else {
            return Ok(None);
        };
        let handle = store.read(edge)?;
        cur = chain.push(handle);
        rest = key_rest;
    }
}

/// The value at `key`, cloned out. Uses the same handle chain as
/// [`get_node`]: each handle borrows the node its predecessor dereferences
/// to, so no predecessor can be released early.
///
/// # Errors
///
/// The store's error from any [`read`](EdgeStore::read) along the path.
pub fn get<V: Clone, A, S, St>(
    root: Option<&Node<V, St::Edge, A, S>>,
    store: &St,
    key: &[A::Index],
) -> Result<Option<V>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    Ok(get_node(root, store, key)?.and_then(|node| node.value().cloned()))
}

/// Per-frame data of a mutation: the index of the child the frame descended
/// into, `None` on the deepest frame.
type Descended<A> = Option<<A as Arity>::Index>;

/// The case analysis of a descent step, with nothing borrowed from the node
/// so the node can be mutated or descended from afterwards.
struct Split<A: Arity> {
    shared_len: usize,
    /// The first unconsumed key index, if the key continues past the node.
    key_next: Option<A::Index>,
    /// The first unmatched index of the node's partial path and the rest of
    /// it, if the key ends inside or diverges from the partial path.
    above: Option<(A::Index, Path<A>)>,
}

impl<A: Arity> Split<A> {
    fn of<V, E, S: ChildStore<A>>(rest: &[A::Index], node: &Node<V, E, A, S>) -> Self {
        let o = common_prefix(rest, node.partial_path());
        Self {
            shared_len: o.shared.len(),
            key_next: o.unique_a.first().copied(),
            above: o.unique_b.split_first().map(|(&i, r)| (i, Path::from(r))),
        }
    }

    /// The next key index and the key after it, if the key continues.
    fn below<'k>(&self, rest: &'k [A::Index]) -> Option<(A::Index, &'k [A::Index])> {
        self.key_next.map(|i| (i, &rest[self.shared_len + 1..]))
    }
}

/// Materializes the child at `i` of the top node and pushes it.
fn descend<V, A, S, St>(
    frames: &mut Frames<'_, Node<V, St::Edge, A, S>, Descended<A>>,
    store: &mut St,
    i: A::Index,
) -> Result<(), St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    *frames.top_extra() = Some(i);
    frames.try_push_child(|cur| {
        let edge = cur.children_mut().get_mut(i).expect("checked present");
        Ok((store.materialize(edge)?, None))
    })
}

/// Inserts `value` at `key`, returning the value it displaced.
///
/// # Errors
///
/// The store's error from a [`materialize`](EdgeStore::materialize) on the
/// path, with the trie's contents unchanged (see the module docs).
pub fn insert<V, A, S, St>(
    root: &mut Option<Node<V, St::Edge, A, S>>,
    store: &mut St,
    key: &[A::Index],
    value: V,
) -> Result<Option<V>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let Some(root) = root.as_mut() else {
        *root = Some(Node::leaf(Path::from(key), value));
        return Ok(None);
    };
    let mut frames = Frames::new(root, None);
    let mut rest = key;
    loop {
        let (cur, _) = frames.top();
        let split = Split::of(rest, cur);
        match (split.below(rest), split.above) {
            (None, None) => return Ok(cur.set_value(Some(value))),
            (None, Some((ni, node_rest))) => {
                // The key ends inside this node's partial path: a new node at
                // the key with this one beneath it.
                let shared = Path::from(&rest[..split.shared_len]);
                let mut old = core::mem::take(cur);
                old.set_partial_path(node_rest);
                let mut children = S::Map::default();
                children.insert(ni, store.inline(old));
                *cur = Node::new(shared, Some(value), children);
                return Ok(None);
            }
            (Some((ki, key_rest)), None) => {
                if cur.children().get(ki).is_none() {
                    let leaf = Node::leaf(Path::from(key_rest), value);
                    cur.children_mut().insert(ki, store.inline(leaf));
                    return Ok(None);
                }
                descend(&mut frames, store, ki)?;
                rest = key_rest;
            }
            (Some((ki, key_rest)), Some((ni, node_rest))) => {
                // Divergence: a valueless node with the old node and a new
                // leaf beneath it.
                let shared = Path::from(&rest[..split.shared_len]);
                let mut old = core::mem::take(cur);
                old.set_partial_path(node_rest);
                let mut children = S::Map::default();
                children.insert(ni, store.inline(old));
                let leaf = Node::leaf(Path::from(key_rest), value);
                children.insert(ki, store.inline(leaf));
                *cur = Node::new(shared, None, children);
                return Ok(None);
            }
        }
    }
}

/// Descends to the node at `key`. `Ok(true)` when the top frame is that node.
fn descend_to<V, A, S, St>(
    frames: &mut Frames<'_, Node<V, St::Edge, A, S>, Descended<A>>,
    store: &mut St,
    key: &[A::Index],
) -> Result<bool, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let mut rest = key;
    loop {
        let (cur, _) = frames.top();
        let split = Split::of(rest, cur);
        if split.above.is_some() {
            return Ok(false);
        }
        let Some((i, key_rest)) = split.below(rest) else {
            return Ok(true);
        };
        if cur.children().get(i).is_none() {
            return Ok(false);
        }
        descend(frames, store, i)?;
        rest = key_rest;
    }
}

/// Merges the only child of `node` into it. The child must be inline.
fn merge_only_child<V, A, S, St>(node: &mut Node<V, St::Edge, A, S>)
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let (i, mut edge) = node.children_mut().take_only_child().expect("one child");
    let mut child = core::mem::take(St::as_inline(&mut edge).expect("materialized"));
    let path = join(node.partial_path(), i, child.partial_path());
    child.set_partial_path(path);
    *node = child;
}

/// What the unwind detached from the trie: an edge removed from a parent's
/// map, or nothing yet because the node leaving is the root, which the caller
/// takes once the frames are gone.
enum Detached<E> {
    Edge(E),
    Root,
}

/// Removes the top node from the trie and repairs its ancestors, the leaf
/// case of `remove`. The top frame's node is what leaves.
///
/// The one fallible step, materializing a two-child parent's survivor,
/// happens before any slot is removed.
fn unwind_remove<V, A, S, St>(
    mut frames: Frames<'_, Node<V, St::Edge, A, S>, Descended<A>>,
    store: &mut St,
) -> Result<Detached<St::Edge>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    loop {
        if frames.pop().is_none() {
            return Ok(Detached::Root);
        }
        let (parent, descended) = frames.top();
        let i = descended.expect("descended into a child");
        let count = parent.children().count();
        if parent.value().is_some() || count >= 3 {
            let edge = parent.children_mut().remove(i).expect("present");
            return Ok(Detached::Edge(edge));
        }
        if count == 2 {
            let (survivor, _) = parent
                .children()
                .iter_present()
                .find(|(j, _)| *j != i)
                .expect("two children");
            let edge = parent.children_mut().get_mut(survivor).expect("present");
            store.materialize(edge)?;
            let edge = parent.children_mut().remove(i).expect("present");
            merge_only_child::<V, A, S, St>(parent);
            return Ok(Detached::Edge(edge));
        }
        // A valueless parent whose only child is leaving leaves too.
    }
}

/// The value inside a detached node. A chain of valueless single-child nodes
/// is walked down iteratively; every node in it is inline because the descent
/// materialized the whole path.
fn recover<V, A, S, St>(mut node: Node<V, St::Edge, A, S>) -> Option<V>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    loop {
        let (_, value, mut children) = node.into_parts();
        if value.is_some() {
            return value;
        }
        let (_, mut edge) = children.take_only_child()?;
        node = core::mem::take(St::as_inline(&mut edge).expect("materialized"));
    }
}

/// Removes the value at `key`, returning it.
///
/// This is Firewood's `flatten_branch` applied bottom-up along the path. At
/// most one `materialize` beyond the descent happens per removal, after every
/// deeper frame has been popped and before the first structural change, so
/// the operation is all-or-nothing.
///
/// # Errors
///
/// The store's error from a [`materialize`](EdgeStore::materialize), with the
/// trie's contents unchanged (see the module docs).
pub fn remove<V, A, S, St>(
    root: &mut Option<Node<V, St::Edge, A, S>>,
    store: &mut St,
    key: &[A::Index],
) -> Result<Option<V>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let Some(node) = root.as_mut() else {
        return Ok(None);
    };
    let mut frames = Frames::new(node, None);
    if !descend_to(&mut frames, store, key)? {
        return Ok(None);
    }
    let (cur, _) = frames.top();
    if cur.value().is_none() {
        return Ok(None);
    }
    match cur.children().count() {
        0 => {}
        1 => return clear_and_merge(cur, store),
        _ => return Ok(cur.set_value(None)),
    }
    let node = match unwind_remove(frames, store)? {
        Detached::Root => root.take(),
        Detached::Edge(mut edge) => St::as_inline(&mut edge).map(core::mem::take),
    };
    Ok(node.and_then(recover::<V, A, S, St>))
}

/// Clears the value of a node with exactly one child and merges the child
/// into it. The child's path, and therefore its hash, change, so it is
/// materialized first: the one fallible step, before anything changes.
fn clear_and_merge<V, A, S, St>(
    cur: &mut Node<V, St::Edge, A, S>,
    store: &mut St,
) -> Result<Option<V>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let (i, _) = cur.children().iter_present().next().expect("one child");
    let edge = cur.children_mut().get_mut(i).expect("present");
    store.materialize(edge)?;
    let old = cur.set_value(None);
    merge_only_child::<V, A, S, St>(cur);
    Ok(old)
}

/// Removes every key that starts with `prefix`.
///
/// The subtree under the prefix is materialized in full before it is detached,
/// so the store records every replacement and a failure leaves the contents
/// unchanged; the cost is the whole subtree resident at once (see the module
/// docs). The detached subtree is freed through the removed edge's `Drop`,
/// or, when the whole trie leaves, by dropping the root node.
///
/// # Errors
///
/// The store's error from a [`materialize`](EdgeStore::materialize), with the
/// trie's contents unchanged (see the module docs).
pub fn remove_prefix<V, A, S, St>(
    root: &mut Option<Node<V, St::Edge, A, S>>,
    store: &mut St,
    prefix: &[A::Index],
) -> Result<(), St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let Some(node) = root.as_mut() else {
        return Ok(());
    };
    let mut frames = Frames::new(node, None);
    let mut rest = prefix;
    loop {
        let (cur, _) = frames.top();
        let split = Split::of(rest, cur);
        let Some((i, prefix_rest)) = split.below(rest) else {
            // `cur`'s full path starts with the prefix: it leaves.
            materialize_subtree(cur, store)?;
            match unwind_remove(frames, store)? {
                Detached::Root => *root = None,
                Detached::Edge(edge) => drop(edge),
            }
            return Ok(());
        };
        if split.above.is_some() || cur.children().get(i).is_none() {
            return Ok(());
        }
        descend(&mut frames, store, i)?;
        rest = prefix_rest;
    }
}

/// Turns every edge beneath `node` inline, so the next hash walk rehashes the
/// whole subtree and the store records every replacement.
///
/// This is how a caller makes a subtree safe to move under another parent, or
/// a trie safe to hash under a different hasher (see the hash-validity
/// contract on [`Node`]). Nodes stay attached throughout, so a failure needs
/// no repair: the nodes materialized so far are inline and the contents are
/// unchanged.
///
/// # Errors
///
/// The store's error from a [`materialize`](EdgeStore::materialize).
pub fn materialize_subtree<V, A, S, St>(
    node: &mut Node<V, St::Edge, A, S>,
    store: &mut St,
) -> Result<(), St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    // Depth-first over (node, children still to visit). A worklist that
    // materialized all of a node's children in one visit would hold pointers
    // into siblings while reborrowing the parent's map, which the frame
    // invariant forbids.
    let remaining = node.children().present();
    let mut frames = Frames::new(node, remaining);
    loop {
        let (_, remaining) = frames.top();
        let Some(i) = remaining.select(0) else {
            if frames.pop().is_none() {
                return Ok(());
            }
            continue;
        };
        *remaining = remaining.without_bit(i);
        materialize_child(&mut frames, store, i)?;
    }
}

/// Materializes the child at `i` of the top node and pushes it with its own
/// set of children to visit.
fn materialize_child<V, A, S, St>(
    frames: &mut Frames<'_, Node<V, St::Edge, A, S>, A::Bitmap>,
    store: &mut St,
    i: A::Index,
) -> Result<(), St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    frames.try_push_child(|cur| {
        let edge = cur.children_mut().get_mut(i).expect("present");
        let child = store.materialize(edge)?;
        let remaining = child.children().present();
        Ok((child, remaining))
    })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::vec::Vec;
    use core::cell::Cell;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::InMemory;
    use crate::MemEdge;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::store::EdgeStore;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    fn p(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    /// root [1,2] (no value) -> 3: leaf [4] = 34, 5: node [] = 5 -> 6: leaf []
    /// = 56
    fn sample() -> (N, St) {
        let mut store = St::default();
        let mut mid = N::new(Path::new(), Some(5), PackedArray::default());
        mid.children_mut()
            .insert(u4(6), store.inline(N::leaf(Path::new(), 56)));
        let mut root = N::new(p(&[1, 2]), None, PackedArray::default());
        root.children_mut()
            .insert(u4(3), store.inline(N::leaf(p(&[4]), 34)));
        root.children_mut().insert(u4(5), store.inline(mid));
        (root, store)
    }

    /// `(found, value)`.
    fn lookup(root: &N, store: &St, key: &[u8]) -> (bool, Option<u32>) {
        get_node(Some(root), store, &p(key))
            .expect("infallible")
            .map_or((false, None), |n| (true, n.value().copied()))
    }

    /// `(partial path, value, child count)` of the node at `key`.
    fn shape(root: Option<&N>, store: &St, key: &[u8]) -> (Vec<u8>, Option<u32>, usize) {
        let node = get_node(root, store, &p(key))
            .expect("infallible")
            .expect("present");
        (
            node.partial_path().iter().map(|i| i.as_u8()).collect(),
            node.value().copied(),
            node.children().count(),
        )
    }

    #[test]
    fn get_node_finds_valued_and_valueless_nodes() {
        let (root, store) = sample();
        assert_eq!(lookup(&root, &store, &[1, 2]), (true, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3, 4]), (true, Some(34)));
        assert_eq!(lookup(&root, &store, &[1, 2, 5]), (true, Some(5)));
        assert_eq!(lookup(&root, &store, &[1, 2, 5, 6]), (true, Some(56)));
    }

    #[test]
    fn get_node_misses_inside_a_partial_path_and_past_a_leaf() {
        let (root, store) = sample();
        assert_eq!(lookup(&root, &store, &[1]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 9]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 3, 4, 0]), (false, None));
        assert_eq!(lookup(&root, &store, &[1, 2, 7]), (false, None));
        assert_eq!(lookup(&root, &store, &[]), (false, None));
    }

    #[test]
    fn get_on_an_empty_trie_is_none() {
        let store = St::default();
        assert_eq!(get(None::<&N>, &store, &p(&[1])).expect("infallible"), None);
    }

    #[test]
    fn get_clones_the_value() {
        let (root, store) = sample();
        assert_eq!(
            get(Some(&root), &store, &p(&[1, 2, 5, 6])).expect("infallible"),
            Some(56)
        );
        assert_eq!(
            get(Some(&root), &store, &p(&[1, 2])).expect("infallible"),
            None
        );
    }

    #[test]
    fn lookup_of_a_deep_chain_does_not_recurse() {
        let depth = if cfg!(miri) { 256 } else { 100_000 };
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut store = St::default();
                let mut node = N::leaf(Path::new(), 7);
                for _ in 0..depth {
                    let mut parent = N::new(Path::new(), None, PackedArray::default());
                    parent.children_mut().insert(u4(0), store.inline(node));
                    node = parent;
                }
                let key = alloc::vec![u4(0); depth];
                let found = get(Some(&node), &store, &key).expect("infallible");
                drop(node);
                found
            })
            .expect("spawn");
        assert_eq!(handle.join().expect("no overflow"), Some(7));
    }

    fn ins(root: &mut Option<N>, store: &mut St, key: &[u8], value: u32) -> Option<u32> {
        insert(root, store, &p(key), value).expect("infallible")
    }

    fn rem(root: &mut Option<N>, store: &mut St, key: &[u8]) -> Option<u32> {
        remove(root, store, &p(key)).expect("infallible")
    }

    fn val(root: Option<&N>, store: &St, key: &[u8]) -> Option<u32> {
        get(root, store, &p(key)).expect("infallible")
    }

    #[test]
    fn insert_into_an_empty_trie_makes_a_root_leaf() {
        let (mut root, mut store) = (None, St::default());
        assert_eq!(ins(&mut root, &mut store, &[1, 2, 3], 9), None);
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![1, 2, 3], Some(9), 0)
        );
        assert_eq!(ins(&mut root, &mut store, &[1, 2, 3], 10), Some(9));
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 3]), Some(10));
    }

    #[test]
    fn insert_above_a_node_puts_it_beneath_the_new_one() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2, 3], 9);
        assert_eq!(ins(&mut root, &mut store, &[1], 1), None);
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], Some(1), 1)
        );
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![3], Some(9), 0)
        );
    }

    #[test]
    fn insert_below_a_leaf_adds_a_child() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        ins(&mut root, &mut store, &[1, 2, 3], 9);
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], Some(1), 1)
        );
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![3], Some(9), 0)
        );
        ins(&mut root, &mut store, &[1, 2, 3, 4], 4);
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![3], Some(9), 1)
        );
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 3, 4]), Some(4));
    }

    #[test]
    fn insert_at_a_divergence_makes_a_valueless_branch() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2, 3], 9);
        ins(&mut root, &mut store, &[1, 2, 7, 8], 8);
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2]),
            (alloc::vec![1, 2], None, 2)
        );
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![], Some(9), 0)
        );
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 7, 8]),
            (alloc::vec![8], Some(8), 0)
        );
    }

    #[test]
    fn insert_with_an_empty_key_values_the_root() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        assert_eq!(ins(&mut root, &mut store, &[], 0), None);
        assert_eq!(
            shape(root.as_ref(), &store, &[]),
            (alloc::vec![], Some(0), 1)
        );
        assert_eq!(val(root.as_ref(), &store, &[1]), Some(1));
    }

    #[test]
    fn remove_of_an_absent_key_changes_nothing() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2, 3], 9);
        ins(&mut root, &mut store, &[1, 2, 7], 7);
        for key in [&[][..], &[1], &[1, 2], &[1, 2, 3, 4], &[1, 2, 5], &[9]] {
            assert_eq!(rem(&mut root, &mut store, key), None);
        }
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2]),
            (alloc::vec![1, 2], None, 2)
        );
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 3]), Some(9));
        assert_eq!(rem(&mut None, &mut store, &[1]), None);
    }

    #[test]
    fn remove_of_a_valued_branch_with_two_children_clears_the_value() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        ins(&mut root, &mut store, &[1, 2], 2);
        ins(&mut root, &mut store, &[1, 3], 3);
        assert_eq!(rem(&mut root, &mut store, &[1]), Some(1));
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], None, 2)
        );
    }

    #[test]
    fn remove_of_a_valued_node_with_one_child_merges_the_child_into_it() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        ins(&mut root, &mut store, &[1, 2, 3], 3);
        assert_eq!(rem(&mut root, &mut store, &[1]), Some(1));
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 3]),
            (alloc::vec![1, 2, 3], Some(3), 0)
        );
        assert_eq!(val(root.as_ref(), &store, &[1]), None);
    }

    #[test]
    fn remove_of_a_leaf_under_a_valued_parent_drops_the_slot() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        ins(&mut root, &mut store, &[1, 2, 3], 3);
        assert_eq!(rem(&mut root, &mut store, &[1, 2, 3]), Some(3));
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], Some(1), 0)
        );
        assert_eq!(
            lookup(root.as_ref().expect("root"), &store, &[1, 2, 3]),
            (false, None)
        );
    }

    #[test]
    fn remove_of_a_leaf_under_a_valueless_two_child_parent_merges_the_survivor() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2, 3], 3);
        ins(&mut root, &mut store, &[1, 2, 7, 8], 8);
        assert_eq!(rem(&mut root, &mut store, &[1, 2, 3]), Some(3));
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 7, 8]),
            (alloc::vec![1, 2, 7, 8], Some(8), 0)
        );
    }

    #[test]
    fn remove_of_a_leaf_under_a_three_child_parent_keeps_the_parent() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2], 2);
        ins(&mut root, &mut store, &[1, 3], 3);
        ins(&mut root, &mut store, &[1, 4], 4);
        assert_eq!(rem(&mut root, &mut store, &[1, 3]), Some(3));
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], None, 2)
        );
    }

    #[test]
    fn remove_of_the_root_leaf_empties_the_trie() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1, 2], 2);
        assert_eq!(rem(&mut root, &mut store, &[1, 2]), Some(2));
        assert!(root.is_none());
    }

    #[test]
    fn remove_turning_a_valued_node_childless_keeps_it() {
        let (mut root, mut store) = (None, St::default());
        ins(&mut root, &mut store, &[1], 1);
        ins(&mut root, &mut store, &[1, 2], 2);
        assert_eq!(rem(&mut root, &mut store, &[1, 2]), Some(2));
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], Some(1), 0)
        );
    }

    /// A chain of `levels` valueless single-child nodes ending at a leaf.
    fn chain(store: &mut St, levels: usize, leaf: u32) -> N {
        let mut node = N::leaf(Path::new(), leaf);
        for _ in 0..levels {
            let mut parent = N::new(Path::new(), None, PackedArray::default());
            parent.children_mut().insert(u4(0), store.inline(node));
            node = parent;
        }
        node
    }

    #[test]
    fn remove_recovers_the_value_through_a_chain_only_root() {
        let mut store = St::default();
        let mut root = Some(chain(&mut store, 3, 42));
        assert_eq!(rem(&mut root, &mut store, &[0, 0, 0]), Some(42));
        assert!(root.is_none());
    }

    #[test]
    fn remove_recovers_the_value_through_a_chain_under_a_two_child_parent() {
        let mut store = St::default();
        let mut parent = N::new(p(&[5]), None, PackedArray::default());
        let chain_edge = {
            let chain = chain(&mut store, 3, 42);
            store.inline(chain)
        };
        parent.children_mut().insert(u4(0), chain_edge);
        parent
            .children_mut()
            .insert(u4(9), store.inline(N::leaf(p(&[9]), 99)));
        let mut root = Some(parent);
        assert_eq!(rem(&mut root, &mut store, &[5, 0, 0, 0, 0]), Some(42));
        assert_eq!(
            shape(root.as_ref(), &store, &[5, 9, 9]),
            (alloc::vec![5, 9, 9], Some(99), 0)
        );
    }

    fn rp(root: &mut Option<N>, store: &mut St, prefix: &[u8]) {
        remove_prefix(root, store, &p(prefix)).expect("infallible");
    }

    fn populated() -> (Option<N>, St) {
        let (mut root, mut store) = (None, St::default());
        for (key, v) in [
            (&[1, 2, 3][..], 3),
            (&[1, 2, 4], 4),
            (&[1, 2, 4, 5], 5),
            (&[1, 7], 7),
            (&[8], 8),
        ] {
            ins(&mut root, &mut store, key, v);
        }
        (root, store)
    }

    #[test]
    fn remove_prefix_at_the_root_empties_the_trie() {
        let (mut root, mut store) = populated();
        rp(&mut root, &mut store, &[]);
        assert!(root.is_none());
    }

    #[test]
    fn remove_prefix_mid_trie_removes_the_subtree_and_collapses() {
        let (mut root, mut store) = populated();
        rp(&mut root, &mut store, &[1, 2]);
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 3]), None);
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 4, 5]), None);
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 7]),
            (alloc::vec![7], Some(7), 0)
        );
        assert_eq!(shape(root.as_ref(), &store, &[]), (alloc::vec![], None, 2));
    }

    #[test]
    fn remove_prefix_ending_inside_a_partial_path_removes_that_node() {
        let (mut root, mut store) = populated();
        rp(&mut root, &mut store, &[1, 2, 4, 5]);
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 4, 5]), None);
        assert_eq!(
            shape(root.as_ref(), &store, &[1, 2, 4]),
            (alloc::vec![], Some(4), 0)
        );
        rp(&mut root, &mut store, &[1]);
        assert_eq!(
            shape(root.as_ref(), &store, &[8]),
            (alloc::vec![8], Some(8), 0)
        );
    }

    #[test]
    fn remove_prefix_on_a_leaf_and_on_a_missing_prefix() {
        let (mut root, mut store) = populated();
        rp(&mut root, &mut store, &[8]);
        assert_eq!(val(root.as_ref(), &store, &[8]), None);
        rp(&mut root, &mut store, &[1, 2, 9]);
        rp(&mut root, &mut store, &[1, 3]);
        assert_eq!(
            shape(root.as_ref(), &store, &[1]),
            (alloc::vec![1], None, 2)
        );
        assert_eq!(val(root.as_ref(), &store, &[1, 2, 3]), Some(3));
    }

    fn sealed_count(node: &N, store: &St) -> usize {
        let mut stack = alloc::vec![node];
        let mut sealed = 0;
        while let Some(n) = stack.pop() {
            for (_, edge) in n.children().iter_present() {
                if St::hash(edge).is_some() {
                    sealed += 1;
                }
                stack.push(store.read(edge).expect("infallible"));
            }
        }
        sealed
    }

    #[test]
    fn materialize_subtree_turns_every_edge_inline() {
        let (mut root, mut store) = populated();
        let node = root.as_mut().expect("root");
        seal_all(node, &mut store);
        assert_eq!(sealed_count(node, &store), 7);
        materialize_subtree(node, &mut store).expect("infallible");
        assert_eq!(sealed_count(node, &store), 0);
    }

    /// Fails `materialize` on the nth call (1-based).
    struct FailNth {
        inner: St,
        calls: Cell<usize>,
        fail_at: usize,
    }

    impl EdgeStore<u32, Arity16, Packed> for FailNth {
        type Edge = MemEdge<u32, Arity16, Packed, u64>;
        type Hash = u64;
        type Error = usize;
        type Shared<'e>
            = &'e N
        where
            Self: 'e;

        fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<&'e N, usize> {
            self.inner.read(edge).map_err(|e| match e {})
        }
        fn as_inline(edge: &mut Self::Edge) -> Option<&mut N> {
            St::as_inline(edge)
        }
        fn materialize<'e>(&mut self, edge: &'e mut Self::Edge) -> Result<&'e mut N, usize> {
            let n = self.calls.get() + 1;
            self.calls.set(n);
            if n == self.fail_at {
                return Err(n);
            }
            self.inner.materialize(edge).map_err(|e| match e {})
        }
        fn inline(&mut self, node: N) -> Self::Edge {
            self.inner.inline(node)
        }
        fn seal(&mut self, edge: &mut Self::Edge, hash: u64) {
            self.inner.seal(edge, hash);
        }
        fn hash(edge: &Self::Edge) -> Option<&u64> {
            St::hash(edge)
        }
    }

    fn seal_all(node: &mut N, store: &mut St) {
        let mut stack = alloc::vec![core::ptr::from_mut(node)];
        while let Some(n) = stack.pop() {
            // SAFETY: a plain pre-order walk over owning edges with no
            // aliasing: each node is visited once, after its parent's map
            // has been fully read.
            let n = unsafe { &mut *n };
            for (_, edge) in n.children_mut().iter_present_mut() {
                stack.push(core::ptr::from_mut(St::as_inline(edge).expect("inline")));
                store.seal(edge, 1);
            }
        }
    }

    fn snapshot(root: Option<&N>, store: &St) -> Vec<(Vec<u8>, Option<u32>, usize)> {
        [
            &[][..],
            &[1],
            &[1, 2],
            &[1, 2, 3],
            &[1, 2, 4],
            &[1, 2, 4, 5],
            &[1, 7],
            &[8],
        ]
        .into_iter()
        .filter_map(|key| {
            get_node(root, store, &p(key))
                .expect("infallible")
                .map(|n| {
                    (
                        n.partial_path().iter().map(|i| i.as_u8()).collect(),
                        n.value().copied(),
                        n.children().count(),
                    )
                })
        })
        .collect()
    }

    fn atomic(op: impl Fn(&mut Option<N>, &mut FailNth) -> Result<(), usize>) {
        let mut fired = 0;
        for fail_at in 1..=6 {
            let (mut root, mut store) = populated();
            seal_all(root.as_mut().expect("root"), &mut store);
            let before = snapshot(root.as_ref(), &store);
            let mut failing = FailNth {
                inner: store,
                calls: Cell::new(0),
                fail_at,
            };
            let result = op(&mut root, &mut failing);
            if result.is_err() {
                fired += 1;
                assert_eq!(
                    before,
                    snapshot(root.as_ref(), &failing.inner),
                    "fail_at {fail_at}"
                );
            }
        }
        assert!(fired >= 1, "the operation never materialized");
    }

    #[test]
    fn a_failed_materialize_leaves_the_contents_unchanged() {
        atomic(|root, st| insert(root, st, &p(&[1, 2, 4, 5]), 50).map(|_| ()));
        atomic(|root, st| insert(root, st, &p(&[1, 2, 4, 6]), 60).map(|_| ()));
        atomic(|root, st| remove(root, st, &p(&[1, 2, 4, 5])).map(|_| ()));
        atomic(|root, st| remove(root, st, &p(&[1, 2, 3])).map(|_| ()));
        atomic(|root, st| remove(root, st, &p(&[1, 7])).map(|_| ()));
        atomic(|root, st| remove_prefix(root, st, &p(&[1, 2])));
        atomic(|root, st| remove_prefix(root, st, &p(&[1])));
    }
}
