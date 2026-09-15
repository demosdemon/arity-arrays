//! The pluggable hash scheme and the Merkle hash walk.
//!
//! # One hasher per trie
//!
//! A sealed hash records nothing about the hasher that computed it, and
//! [`hash`] admits any hasher whose `Hash` type matches, so a trie sealed under
//! one scheme, seed, or domain separator carries hashes a second hasher would
//! trust and build its root over, giving a value that is correct under
//! neither. Every sealed edge in a trie therefore belongs to one hasher,
//! algorithm and configuration included, for as long as it is sealed. To
//! change hasher, call [`materialize_subtree`](crate::materialize_subtree) on
//! the root, which turns every edge inline and lets the store record the
//! replacements; then hash under the new one.

use alloc::vec::Vec;

use arity_arrays::Arity;
use arity_arrays::bitmap::Bitmap;

use crate::Node;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::frames::Frames;
use crate::store::EdgeStore;

/// Everything a scheme needs to hash one node.
///
/// `leading_path` is the full path from the root to the node's slot in its
/// parent, so a position-dependent scheme (Firewood's `MerkleDB`) can hash the
/// full path. `children` yields the present children ascending, as an iterator
/// rather than a full-width array so a scheme that hashes absent slots
/// (Ethereum) fills them in itself; a hasher that needs the child count
/// clones the iterator and counts. `siblings` is the number of present
/// children of the node's parent, counting the node itself, and `0` for the
/// root.
pub struct HashInput<'a, V, A: Arity, C> {
    /// The full path from the root to the node's slot in its parent.
    pub leading_path: &'a [A::Index],
    /// The node's own partial path.
    pub partial_path: &'a [A::Index],
    /// The node's value, if any. Digesting it (Firewood hashes a value once it
    /// reaches 32 bytes) is the hasher's business.
    pub value: Option<&'a V>,
    /// Present children of the parent, counting this node; `0` at the root.
    pub siblings: usize,
    /// The present children's indices and hashes, ascending.
    pub children: C,
}

/// A hash scheme over a trie.
///
/// This trait, [`EdgeStore`], [`ChildMap`], and the shape of [`Node`] are the
/// crate's hard-to-reverse surface. See the module docs for the one-hasher
/// contract.
pub trait TrieHasher<V, A: Arity> {
    /// The hash type; the store's `Hash` must equal it.
    type Hash;

    /// Hashes one node from its input.
    fn hash_node<'a, C>(&self, input: HashInput<'a, V, A, C>) -> Self::Hash
    where
        C: Iterator<Item = (A::Index, &'a Self::Hash)> + Clone,
        Self::Hash: 'a;

    /// `true` if the children of the node at this position hash differently
    /// depending on whether they have siblings.
    ///
    /// Under Firewood's Ethereum scheme a node at the account level (a full
    /// path of 64 nibbles) with exactly one child hashes that child as the
    /// root of a standalone storage trie, so adding or removing a sibling
    /// changes a sealed child's correct hash without touching the child. The
    /// supported dependency is exactly "only child or not": a child's hash
    /// may take one form when `siblings == 1` and another when `siblings >
    /// 1`, never depend on the exact count. The walk recovers from that
    /// dependency by rehashing a lone sealed child of such a parent; see
    /// [`hash`]. The default is `false`.
    fn sibling_sensitive(&self, leading_path: &[A::Index], partial_path: &[A::Index]) -> bool {
        let _ = (leading_path, partial_path);
        false
    }

    /// Rewrites a node's stored value from its children's hashes before the
    /// node is hashed.
    ///
    /// The walk calls this on every inline node with a value that it is about
    /// to hash, the root included when it has one, and only then builds the
    /// [`HashInput`] from the value as rewritten; a valueless node has nothing
    /// to rewrite. The default does nothing. It carries Firewood's Ethereum
    /// account storage root, which is spliced into the stored account RLP at
    /// hash time. A sealed node is never rewritten: the lone-sealed-child
    /// rehash re-derives that child's hash without calling this, which is why
    /// there is no `siblings` argument, and a value written at such a level
    /// reads back as written until the next [`hash`].
    fn update_value<'a, C>(
        &self,
        _leading_path: &[A::Index],
        _partial_path: &[A::Index],
        _children: C,
        _value: &mut V,
    ) where
        C: Iterator<Item = (A::Index, &'a Self::Hash)> + Clone,
        Self::Hash: 'a,
    {
    }
}

/// Per-frame data of the hash walk.
struct Frame<A: Arity> {
    /// The node's index in its parent; `None` at the root.
    index_in_parent: Option<A::Index>,
    /// The length of the node's leading path in the shared buffer.
    leading_len: usize,
    /// What the node's own input carries: its parent's `count`, `0` at the
    /// root. Copied at push time so the parent is never re-read while this
    /// frame exists.
    siblings: usize,
    /// The node's present child count, taken in the scan that fills
    /// `pending`; `Fixed`'s count is a full scan of the array, so this also
    /// avoids one per child.
    count: usize,
    /// The inline children still to hash.
    pending: A::Bitmap,
}

/// The sealed hashes of `node`'s children, ascending. Every child must be
/// sealed.
fn sealed_hashes<'a, V, A, S, St>(
    node: &'a Node<V, St::Edge, A, S>,
) -> impl Iterator<Item = (A::Index, &'a St::Hash)> + Clone
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
    St::Hash: 'a,
{
    node.children()
        .iter_present()
        .map(|(i, edge)| (i, St::hash(edge).expect("every child is sealed")))
}

/// Prepares the frame for `node`, whose leading path is `buf[..leading_len]`,
/// entirely through the node's own reference and before any child frame
/// exists: one scan of the children, and the rehash of a lone sealed child
/// under a sibling-sensitive parent.
fn prepare<V, A, S, St, Hs>(
    node: &mut Node<V, St::Edge, A, S>,
    store: &mut St,
    hasher: &Hs,
    buf: &mut Vec<A::Index>,
    index_in_parent: Option<A::Index>,
    leading_len: usize,
    siblings: usize,
) -> Result<Frame<A>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = Hs::Hash>,
    Hs: TrieHasher<V, A>,
{
    let mut pending = A::Bitmap::ZERO;
    let mut count = 0;
    let mut sealed = 0;
    let mut last_sealed = None;
    for (i, edge) in node.children().iter_present() {
        count += 1;
        if St::hash(edge).is_none() {
            pending = pending.with_bit(i);
        } else {
            sealed += 1;
            last_sealed = Some(i);
        }
    }
    if sealed == 1 && hasher.sibling_sensitive(&buf[..leading_len], node.partial_path()) {
        // Exactly one sealed child: nothing records whether it was sealed in
        // lone or peers form, so recompute it. With two or more, each was
        // sealed by a walk that first applied this rule to whichever was then
        // the lone one, so all are in peers form and valid.
        let i = last_sealed.expect("one sealed child");
        buf.truncate(leading_len);
        buf.extend_from_slice(node.partial_path());
        buf.push(i);
        let rehashed = {
            let edge = node.children().get(i).expect("present");
            let handle = store.read(edge)?;
            hash_sealed_node::<V, A, S, St, Hs>(&handle, buf, count, hasher)
        };
        buf.truncate(leading_len);
        let edge = node.children_mut().get_mut(i).expect("present");
        if let Some(h) = rehashed {
            store.seal(edge, h);
        } else {
            // The sealed child has inline descendants, which only a
            // hand-built trie produces: hash it like any inline child.
            store.materialize(edge)?;
            pending = pending.with_bit(i);
        }
    }
    Ok(Frame {
        index_in_parent,
        leading_len,
        siblings,
        count,
        pending,
    })
}

/// Hashes the trie under `root`, sealing every inline edge with its node's
/// hash on the way up, and returns the root's hash.
///
/// Post-order over a frame stack, so a trie of any depth hashes on a bounded
/// stack. Already sealed children are never descended into and no map
/// changes shape; the walk is linear in the number of inline nodes plus their
/// fanouts, plus one [`read`](EdgeStore::read) per sibling-sensitive parent
/// with a lone sealed child. The root stays inline, since it is not behind an
/// edge. The empty trie's hash is scheme-defined and not this function's
/// concern.
///
/// The walk trusts every sealed edge it does not rehash: see the
/// hash-validity contract on [`Node`] and the one-hasher contract in the
/// module docs.
///
/// # Errors
///
/// The store's error from a [`read`](EdgeStore::read) or
/// [`materialize`](EdgeStore::materialize), both of which happen only when a
/// sibling-sensitive parent must rehash a sealed child. Under a hasher that
/// never returns `true` from [`TrieHasher::sibling_sensitive`] the walk calls
/// neither and never fails. A failure leaves the trie consistent: every edge
/// sealed so far carries the hash of its current content and position, the
/// failed call changed nothing, and calling `hash` again finishes the work.
pub fn hash<V, A, S, St, Hs>(
    root: &mut Node<V, St::Edge, A, S>,
    store: &mut St,
    hasher: &Hs,
) -> Result<Hs::Hash, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = Hs::Hash>,
    Hs: TrieHasher<V, A>,
{
    let mut buf: Vec<A::Index> = Vec::new();
    let root_frame = prepare(root, store, hasher, &mut buf, None, 0, 0)?;
    let mut frames = Frames::new(root, root_frame);
    loop {
        let (node, frame) = frames.top();
        if let Some(i) = frame.pending.select(0) {
            frame.pending = frame.pending.without_bit(i);
            buf.truncate(frame.leading_len);
            buf.extend_from_slice(node.partial_path());
            buf.push(i);
            let siblings = frame.count;
            // The parent is read above, before the child pointer is derived,
            // never after.
            push_pending_child(&mut frames, store, hasher, &mut buf, i, siblings)?;
            continue;
        }
        // Every child is sealed: hash this node. Everything the input borrows
        // comes from the popped node and its frame, so no parent is touched
        // while the child's pointer is in use.
        let (h, index_in_parent) = frames.pop_with(|node, frame| {
            buf.truncate(frame.leading_len);
            (
                finish_node::<V, A, S, St, Hs>(node, hasher, &buf, frame.siblings),
                frame.index_in_parent,
            )
        });
        let Some(i) = index_in_parent else {
            return Ok(h);
        };
        let (parent, _) = frames.top();
        seal_child::<V, A, S, St>(parent, store, i, h);
    }
}

/// Pushes the inline child at `i` of the top node, whose leading path is the
/// whole of `buf`.
fn push_pending_child<V, A, S, St, Hs>(
    frames: &mut Frames<'_, Node<V, St::Edge, A, S>, Frame<A>>,
    store: &mut St,
    hasher: &Hs,
    buf: &mut Vec<A::Index>,
    i: A::Index,
    siblings: usize,
) -> Result<(), St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = Hs::Hash>,
    Hs: TrieHasher<V, A>,
{
    let leading_len = buf.len();
    frames.try_push_child(|parent| {
        let edge = parent.children_mut().get_mut(i).expect("present");
        let child = St::as_inline(edge).expect("pending children are inline");
        let frame = prepare(child, store, hasher, buf, Some(i), leading_len, siblings)?;
        Ok((child, frame))
    })
}

/// Rewrites the value of a node whose children are all sealed, then hashes
/// it.
fn finish_node<V, A, S, St, Hs>(
    node: &mut Node<V, St::Edge, A, S>,
    hasher: &Hs,
    leading_path: &[A::Index],
    siblings: usize,
) -> Hs::Hash
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = Hs::Hash>,
    Hs: TrieHasher<V, A>,
{
    let (partial_path, value, children) = node.parts_mut();
    if let Some(value) = value.as_mut() {
        let child_hashes = children
            .iter_present()
            .map(|(i, edge)| (i, St::hash(edge).expect("every child is sealed")));
        hasher.update_value(leading_path, partial_path, child_hashes, value);
    }
    hasher.hash_node(HashInput {
        leading_path,
        partial_path: node.partial_path(),
        value: node.value(),
        siblings,
        children: sealed_hashes::<V, A, S, St>(node),
    })
}

/// Seals the child at `i` of `parent` with `h`.
fn seal_child<V, A, S, St>(
    parent: &mut Node<V, St::Edge, A, S>,
    store: &mut St,
    i: A::Index,
    h: St::Hash,
) where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let edge = parent.children_mut().get_mut(i).expect("present");
    store.seal(edge, h);
}

/// Hashes one node whose children are all sealed; `None` if any child is
/// inline.
///
/// For proof-side code that holds a node's child hashes without the child
/// nodes, and for the walk's rehash of a lone sealed child. It takes `&Node`
/// and so never calls [`TrieHasher::update_value`], and `hasher` must be the
/// one that sealed the children.
#[must_use]
pub fn hash_sealed_node<V, A, S, St, Hs>(
    node: &Node<V, St::Edge, A, S>,
    leading_path: &[A::Index],
    siblings: usize,
    hasher: &Hs,
) -> Option<Hs::Hash>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = Hs::Hash>,
    Hs: TrieHasher<V, A>,
{
    if node
        .children()
        .iter_present()
        .any(|(_, edge)| St::hash(edge).is_none())
    {
        return None;
    }
    Some(hasher.hash_node(HashInput {
        leading_path,
        partial_path: node.partial_path(),
        value: node.value(),
        siblings,
        children: sealed_hashes::<V, A, S, St>(node),
    }))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::vec::Vec;

    use arity_arrays::Arity16;
    use arity_arrays::PackedArray;
    use arity_arrays::index::U4;

    use super::*;
    use crate::InMemory;
    use crate::MemEdge;
    use crate::Node;
    use crate::Packed;
    use crate::Path;
    use crate::insert;
    use crate::materialize_subtree;
    use crate::remove;
    use crate::store::EdgeStore;
    use crate::testing::Failing;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    fn p(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    /// A 64-bit FNV-1a hasher over everything in the input. With
    /// `sensitive_depth`, children of a node at that full-path length hash in
    /// a distinct lone form when they have no siblings. With `rewrite_depth`,
    /// a node at that full-path length has its value replaced by a digest of
    /// its children's hashes before hashing, the shape of Firewood's account
    /// storage root.
    struct Fnv {
        seed: u64,
        sensitive_depth: Option<usize>,
        rewrite_depth: Option<usize>,
    }

    impl Fnv {
        const PLAIN: Self = Self {
            seed: 0xcbf2_9ce4_8422_2325,
            sensitive_depth: None,
            rewrite_depth: None,
        };

        fn feed(h: &mut u64, bytes: &[u8]) {
            for b in bytes {
                *h ^= u64::from(*b);
                *h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }

        fn digest<'a>(children: impl Iterator<Item = (U4, &'a u64)>) -> u64 {
            let mut h = 0x9e37_79b9_7f4a_7c15;
            for (i, child) in children {
                Self::feed(&mut h, &[i.as_u8()]);
                Self::feed(&mut h, &child.to_le_bytes());
            }
            h
        }
    }

    impl TrieHasher<u32, Arity16> for Fnv {
        type Hash = u64;

        fn hash_node<'a, C>(&self, input: HashInput<'a, u32, Arity16, C>) -> u64
        where
            C: Iterator<Item = (U4, &'a u64)> + Clone,
        {
            let mut h = self.seed;
            Self::feed(&mut h, U4::as_u8_slice(input.leading_path));
            Self::feed(&mut h, &[0xff]);
            Self::feed(&mut h, U4::as_u8_slice(input.partial_path));
            Self::feed(&mut h, &[0xff]);
            match input.value {
                Some(v) => Self::feed(&mut h, &v.to_le_bytes()),
                None => Self::feed(&mut h, &[0xfe]),
            }
            let lone = self.sensitive_depth == Some(input.leading_path.len().wrapping_sub(1))
                && input.siblings == 1;
            Self::feed(&mut h, &[u8::from(lone)]);
            Self::feed(&mut h, &Self::digest(input.children).to_le_bytes());
            h
        }

        fn sibling_sensitive(&self, leading_path: &[U4], partial_path: &[U4]) -> bool {
            self.sensitive_depth == Some(leading_path.len() + partial_path.len())
        }

        fn update_value<'a, C>(
            &self,
            leading_path: &[U4],
            partial_path: &[U4],
            children: C,
            value: &mut u32,
        ) where
            C: Iterator<Item = (U4, &'a u64)> + Clone,
        {
            if self.rewrite_depth == Some(leading_path.len() + partial_path.len()) {
                *value =
                    u32::try_from(Self::digest(children) & u64::from(u32::MAX)).expect("masked");
            }
        }
    }

    fn build(keys: &[(&[u8], u32)]) -> (Option<N>, St) {
        let (mut root, mut store) = (None, St::default());
        for (key, value) in keys {
            insert(&mut root, &mut store, &p(key), *value).expect("infallible");
        }
        (root, store)
    }

    fn fresh_hash(keys: &[(&[u8], u32)], hasher: &Fnv) -> u64 {
        let (mut root, mut store) = build(keys);
        hash(root.as_mut().expect("non-empty"), &mut store, hasher).expect("infallible")
    }

    fn sealed_edges(node: &N, store: &St) -> (usize, usize) {
        let mut stack = alloc::vec![node];
        let (mut sealed, mut inline) = (0, 0);
        while let Some(n) = stack.pop() {
            for (_, edge) in n.children().iter_present() {
                if St::hash(edge).is_some() {
                    sealed += 1;
                } else {
                    inline += 1;
                }
                stack.push(store.read(edge).expect("infallible"));
            }
        }
        (sealed, inline)
    }

    const KEYS: &[(&[u8], u32)] = &[
        (&[1, 2, 3], 3),
        (&[1, 2, 4], 4),
        (&[1, 2, 4, 5], 5),
        (&[1, 7], 7),
        (&[8], 8),
    ];

    #[test]
    fn hash_seals_every_edge_and_is_stable() {
        let (mut root, mut store) = build(KEYS);
        let node = root.as_mut().expect("root");
        let first = hash(node, &mut store, &Fnv::PLAIN).expect("infallible");
        assert_eq!(sealed_edges(node, &store), (7, 0));
        assert_eq!(
            hash(node, &mut store, &Fnv::PLAIN).expect("infallible"),
            first
        );
    }

    #[test]
    fn hash_after_interleaved_mutation_equals_a_fresh_build() {
        let (mut root, mut store) = build(&KEYS[..2]);
        hash(root.as_mut().expect("root"), &mut store, &Fnv::PLAIN).expect("infallible");
        for (key, value) in &KEYS[2..] {
            insert(&mut root, &mut store, &p(key), *value).expect("infallible");
        }
        let node = root.as_mut().expect("root");
        let (sealed, inline) = sealed_edges(node, &store);
        assert!(sealed > 0 && inline > 0, "a mix of sealed and inline edges");
        assert_eq!(
            hash(node, &mut store, &Fnv::PLAIN).expect("infallible"),
            fresh_hash(KEYS, &Fnv::PLAIN)
        );
        remove(&mut root, &mut store, &p(&[1, 2, 4])).expect("infallible");
        let rest: Vec<(&[u8], u32)> = KEYS
            .iter()
            .copied()
            .filter(|(k, _)| *k != [1, 2, 4])
            .collect();
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut store, &Fnv::PLAIN).expect("infallible"),
            fresh_hash(&rest, &Fnv::PLAIN)
        );
    }

    #[test]
    fn hash_of_a_root_leaf_covers_its_path_and_value() {
        let a = fresh_hash(&[(&[1, 2], 5)], &Fnv::PLAIN);
        let b = fresh_hash(&[(&[1, 3], 5)], &Fnv::PLAIN);
        let c = fresh_hash(&[(&[1, 2], 6)], &Fnv::PLAIN);
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn different_seeds_give_different_hashes() {
        let other = Fnv {
            seed: 1,
            ..Fnv::PLAIN
        };
        assert_ne!(fresh_hash(KEYS, &Fnv::PLAIN), fresh_hash(KEYS, &other));
    }

    #[test]
    fn switching_hashers_after_materialize_subtree_matches_a_fresh_build() {
        let other = Fnv {
            seed: 1,
            ..Fnv::PLAIN
        };
        let (mut root, mut store) = build(KEYS);
        let node = root.as_mut().expect("root");
        hash(node, &mut store, &Fnv::PLAIN).expect("infallible");
        assert_ne!(
            hash(node, &mut store, &other).expect("infallible"),
            fresh_hash(KEYS, &other)
        );
        materialize_subtree(node, &mut store).expect("infallible");
        assert_eq!(
            hash(node, &mut store, &other).expect("infallible"),
            fresh_hash(KEYS, &other)
        );
    }

    const SENSITIVE: Fnv = Fnv {
        sensitive_depth: Some(1),
        ..Fnv::PLAIN
    };

    #[test]
    fn lone_and_peer_forms_differ_under_a_sensitive_hasher() {
        let lone = fresh_hash(&[(&[5], 1), (&[5, 1], 2)], &SENSITIVE);
        let plain = fresh_hash(&[(&[5], 1), (&[5, 1], 2)], &Fnv::PLAIN);
        assert_ne!(lone, plain);
        let peers = fresh_hash(&[(&[5], 1), (&[5, 1], 2), (&[5, 2], 3)], &SENSITIVE);
        let peers_plain = fresh_hash(&[(&[5], 1), (&[5, 1], 2), (&[5, 2], 3)], &Fnv::PLAIN);
        assert_eq!(peers, peers_plain);
    }

    #[test]
    fn a_sealed_lone_child_is_rehashed_when_it_gains_or_loses_a_sibling() {
        let (mut root, mut store) = build(&[(&[5], 1), (&[5, 1], 2)]);
        hash(root.as_mut().expect("root"), &mut store, &SENSITIVE).expect("infallible");
        insert(&mut root, &mut store, &p(&[5, 2]), 3).expect("infallible");
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut store, &SENSITIVE).expect("infallible"),
            fresh_hash(&[(&[5], 1), (&[5, 1], 2), (&[5, 2], 3)], &SENSITIVE)
        );
        remove(&mut root, &mut store, &p(&[5, 2])).expect("infallible");
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut store, &SENSITIVE).expect("infallible"),
            fresh_hash(&[(&[5], 1), (&[5, 1], 2)], &SENSITIVE)
        );
        insert(&mut root, &mut store, &p(&[5, 3]), 4).expect("infallible");
        insert(&mut root, &mut store, &p(&[5, 4]), 5).expect("infallible");
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut store, &SENSITIVE).expect("infallible"),
            fresh_hash(
                &[(&[5], 1), (&[5, 1], 2), (&[5, 3], 4), (&[5, 4], 5)],
                &SENSITIVE
            )
        );
    }

    #[test]
    fn hash_sealed_node_matches_the_walk_and_rejects_inline_children() {
        let (mut root, mut store) = build(KEYS);
        let node = root.as_mut().expect("root");
        assert!(hash_sealed_node::<_, _, _, St, _>(node, &[], 0, &Fnv::PLAIN).is_none());
        let walked = hash(node, &mut store, &Fnv::PLAIN).expect("infallible");
        assert_eq!(
            hash_sealed_node::<_, _, _, St, _>(node, &[], 0, &Fnv::PLAIN),
            Some(walked)
        );
        let leaf = N::leaf(p(&[9]), 9);
        assert_eq!(
            hash_sealed_node::<_, _, _, St, _>(&leaf, &[u4(1)], 2, &Fnv::PLAIN),
            Some(Fnv::PLAIN.hash_node(HashInput {
                leading_path: &[u4(1)],
                partial_path: &[u4(9)],
                value: Some(&9),
                siblings: 2,
                children: core::iter::empty(),
            }))
        );
    }

    const REWRITING: Fnv = Fnv {
        rewrite_depth: Some(1),
        ..Fnv::PLAIN
    };

    #[test]
    fn update_value_rewrites_the_stored_value_at_hash_time() {
        let keys: &[(&[u8], u32)] = &[(&[5], 1), (&[5, 1], 2), (&[5, 2], 3)];
        let (mut root, mut store) = build(keys);
        assert_eq!(
            crate::get(root.as_ref(), &store, &p(&[5])).expect("infallible"),
            Some(1)
        );
        let h = hash(root.as_mut().expect("root"), &mut store, &REWRITING).expect("infallible");
        let rewritten = crate::get(root.as_ref(), &store, &p(&[5])).expect("infallible");
        assert_ne!(rewritten, Some(1));
        assert_ne!(h, fresh_hash(keys, &Fnv::PLAIN));

        // The rewrite is a function of the children: a fresh build with the
        // rewritten value stored up front hashes and reads the same.
        let (mut again, mut store2) = build(&[
            (&[5], rewritten.expect("value")),
            (&[5, 1], 2),
            (&[5, 2], 3),
        ]);
        assert_eq!(
            hash(again.as_mut().expect("root"), &mut store2, &REWRITING).expect("infallible"),
            h
        );
        assert_eq!(
            crate::get(again.as_ref(), &store2, &p(&[5])).expect("infallible"),
            rewritten
        );

        // Changing the storage beneath changes the value on the next hash.
        insert(&mut root, &mut store, &p(&[5, 3]), 4).expect("infallible");
        hash(root.as_mut().expect("root"), &mut store, &REWRITING).expect("infallible");
        assert_ne!(
            crate::get(root.as_ref(), &store, &p(&[5])).expect("infallible"),
            rewritten
        );
    }

    #[test]
    fn a_failed_read_during_a_rehash_is_recoverable() {
        let (mut root, mut store) = build(&[(&[5], 1), (&[5, 1], 2)]);
        hash(root.as_mut().expect("root"), &mut store, &SENSITIVE).expect("infallible");
        insert(&mut root, &mut store, &p(&[5, 2]), 3).expect("infallible");
        let mut failing = Failing::new(store, 1, 0);
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut failing, &SENSITIVE),
            Err("read")
        );
        assert_eq!(
            hash(root.as_mut().expect("root"), &mut failing.inner, &SENSITIVE).expect("infallible"),
            fresh_hash(&[(&[5], 1), (&[5, 1], 2), (&[5, 2], 3)], &SENSITIVE)
        );
    }

    #[test]
    fn a_sealed_child_with_inline_descendants_is_materialized_and_rehashed() {
        // Hand-built: root [5]=1 with one child 1 -> node []=2 sealed, whose
        // own child 7 -> leaf []=9 is inline. Only children_mut
        // produces this.
        let mut store = St::default();
        let mut mid = N::new(Path::new(), Some(2), PackedArray::default());
        mid.children_mut()
            .insert(u4(7), store.inline(N::leaf(Path::new(), 9)));
        let mut edge = store.inline(mid);
        store.seal(&mut edge, 0xdead);
        let mut root = N::new(p(&[5]), Some(1), PackedArray::default());
        root.children_mut().insert(u4(1), edge);

        let mut failing = Failing::new(store, 0, 1);
        assert_eq!(
            hash(&mut root, &mut failing, &SENSITIVE),
            Err("materialize")
        );
        assert_eq!(
            hash(&mut root, &mut failing.inner, &SENSITIVE).expect("infallible"),
            fresh_hash(&[(&[5], 1), (&[5, 1], 2), (&[5, 1, 7], 9)], &SENSITIVE)
        );
    }

    #[test]
    fn hashing_a_deep_chain_does_not_recurse() {
        // The test hasher feeds the whole leading path at every level, so a
        // chain hashes in quadratic time; ten thousand levels still overflow
        // a 64 KiB stack if any step recurses.
        let depth = if cfg!(miri) { 256 } else { 10_000 };
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
                let h = hash(&mut node, &mut store, &Fnv::PLAIN).expect("infallible");
                drop(node);
                h
            })
            .expect("spawn");
        handle.join().expect("no overflow");
    }
}
