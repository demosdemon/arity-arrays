//! Bench/test-only compressed-trie fixture used by the `trie` benchmark and the
//! `trie_internals` integration test. Each node stores its children in a
//! representation chosen by a `ChildStore` marker, so the same recursive
//! `Clone`/`Drop` workload is timed across `GappedArray`, `PackedArray`,
//! `FixedArray`, and a `BTreeMap` baseline. Not part of the public API.

use core::fmt;
use std::collections::BTreeMap;
use std::sync::Arc;

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;

/// Selects the children-array representation. `A` comes from the trie and `V`
/// (the edge type) from the GAT, so the recursive `Trie`/`Edge` types need
/// never be named as a bare type parameter (which Rust cannot express without
/// HKT).
pub trait ChildStore<A: Arity> {
    /// Stable label for this representation, used as the trie `BenchmarkId`
    /// subject and parsed back by the chart xtask. Must be unique.
    const NAME: &'static str;
    type Map<V>: ChildMap<A, V>;
}

/// The minimal index-to-value map the builder needs. (The `trie_internals` test
/// adds its own read accessor; this trait carries only what the bench uses, so
/// the bench target stays free of dead-code on test-only methods.)
///
/// `clone_map` clones the map structure by applying a caller-supplied clone
/// function to each present value. This avoids propagating a `V: Clone` bound
/// into the store implementations and breaks the trait-solver cycle that would
/// arise from `GappedArray<Edge<A,S>, A>: Clone` ↔ `Edge<A,S>: Clone`.
pub trait ChildMap<A: Arity, V> {
    fn empty() -> Self;
    fn insert(&mut self, index: A::Index, value: V) -> Option<V>;
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self;
}

pub struct GappedStore;
pub struct PackedStore;
pub struct FixedStore;
pub struct BTreeStore;

impl<A: Arity> ChildStore<A> for GappedStore {
    const NAME: &'static str = "GappedStore";
    type Map<V> = GappedArray<V, A>;
}
impl<A: Arity> ChildStore<A> for PackedStore {
    const NAME: &'static str = "PackedStore";
    type Map<V> = PackedArray<V, A>;
}
impl<A: Arity> ChildStore<A> for FixedStore {
    const NAME: &'static str = "FixedStore";
    type Map<V> = FixedArray<Option<V>, A>;
}
impl<A: Arity> ChildStore<A> for BTreeStore {
    const NAME: &'static str = "BTreeStore";
    type Map<V> = BTreeMap<usize, V>;
}

impl<A: Arity, V> ChildMap<A, V> for GappedArray<V, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn insert(&mut self, i: A::Index, v: V) -> Option<V> {
        Self::insert(self, i, v)
    }
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self {
        let mut result = Self::new();
        for (idx, v) in self.iter_present() {
            result.insert(idx, clone_one(v));
        }
        result
    }
}
impl<A: Arity, V> ChildMap<A, V> for PackedArray<V, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn insert(&mut self, i: A::Index, v: V) -> Option<V> {
        Self::insert(self, i, v)
    }
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self {
        let mut result = Self::new();
        for (idx, v) in self.iter_present() {
            result.insert(idx, clone_one(v));
        }
        result
    }
}
impl<A: Arity, V> ChildMap<A, V> for FixedArray<Option<V>, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn insert(&mut self, i: A::Index, v: V) -> Option<V> {
        Self::replace(self, i, Some(v))
    }
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self {
        let mut result = Self::new();
        for (idx, v) in self.iter_present() {
            Self::replace(&mut result, idx, Some(clone_one(v)));
        }
        result
    }
}
impl<A: Arity, V> ChildMap<A, V> for BTreeMap<usize, V> {
    fn empty() -> Self {
        Self::new()
    }
    fn insert(&mut self, i: A::Index, v: V) -> Option<V> {
        Self::insert(self, i.as_usize(), v)
    }
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self {
        self.iter().map(|(&k, v)| (k, clone_one(v))).collect()
    }
}

/// A trie node whose children live in representation `S`.
pub struct Trie<A: Arity, S: ChildStore<A>> {
    pub path: Box<[A::Index]>,
    pub value: Option<Box<[u8]>>,
    pub children: <S as ChildStore<A>>::Map<Edge<A, S>>,
}

/// An edge to a child node: an owned mutable subtree, or a frozen,
/// hash-stamped, `Arc`-shared subtree.
pub enum Edge<A: Arity, S: ChildStore<A>> {
    Mutable(Box<Trie<A, S>>),
    Frozen {
        hash: [u8; 32],
        node: Arc<Trie<A, S>>,
    },
}

// `Clone` for `Trie` and `Edge` are hand-written rather than derived because
// `derive(Clone)` would require `<S as ChildStore<A>>::Map<Edge<A, S>>: Clone`
// in the where clause, which creates an unsatisfiable trait-solver cycle when
// the map's own `Clone` impl in turn requires `Edge<A, S>: Clone`.
//
// Instead we break the cycle by implementing both `Clone` impls
// unconditionally: `Trie::clone` delegates to `ChildMap::clone_map`, which
// accepts a caller-supplied cloning function, removing the structural
// `V: Clone` requirement from the store implementations.
//
// `clone_map` *rebuilds* each children map by re-inserting its present elements
// one at a time, rather than calling the representation's own `Clone`. For the
// builder-constructed fixtures this reproduces the native clone's capacity
// (Packed exact-fit, Gapped next-power-of-two, Fixed full-width) and per-node
// cost is dominated by the recursive subtree clones, so the
// cross-representation comparison stays faithful. Caveat: `PackedArray::insert`
// reallocates to exact size on each call, so a per-node rebuild is O(count^2) —
// immaterial only because the fixtures cap fanout at 16. A future wide-fanout
// fixture would start measuring rebuild artifacts instead of clone cost and
// should clone via the representation's own `Clone`.
impl<A: Arity, S: ChildStore<A>> Clone for Trie<A, S> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            value: self.value.clone(),
            children: self.children.clone_map(Edge::clone),
        }
    }
}

impl<A: Arity, S: ChildStore<A>> Clone for Edge<A, S> {
    fn clone(&self) -> Self {
        match self {
            Self::Mutable(t) => Self::Mutable(t.clone()),
            Self::Frozen { hash, node } => Self::Frozen {
                hash: *hash,
                node: Arc::clone(node),
            },
        }
    }
}

/// Bytes in each node's `value` payload.
const VALUE_LEN: usize = 8;
/// `path` length for `Realistic` nodes (other shapes use an empty,
/// non-allocating path).
const REALISTIC_PATH_LEN: usize = 4;
/// Children per node for `Bushy`.
pub const BUSHY_FANOUT: usize = 4;

// Depths are cfg(miri)-reduced so the Miri-checked tests stay bounded; the
// benchmark always runs natively, so it uses the full sizes. These shape
// parameters are `pub` so the `trie_internals` test can compute its independent
// node-count oracle from them.
#[cfg(not(miri))]
pub const BUSHY_DEPTH: usize = 6; // (4^7 - 1) / 3 = 5461 nodes
#[cfg(miri)]
pub const BUSHY_DEPTH: usize = 3; // (4^4 - 1) / 3 = 85 nodes
#[cfg(not(miri))]
pub const REALISTIC_FANOUTS: &[usize] = &[16, 8, 4, 2, 1, 1, 1, 1]; // 5777 nodes
#[cfg(miri)]
pub const REALISTIC_FANOUTS: &[usize] = &[4, 2, 1]; // 1 + 4 + 8 + 8 = 21 nodes

/// Symbol-length of a 64-byte key in this arity's alphabet: 128 for Arity16
/// (nibbles), 64 for Arity256 (bytes). Only `Shape::Chain` uses it.
#[must_use]
pub fn key_depth<A: Arity>() -> usize {
    let log2_alphabet = A::LEN.trailing_zeros() as usize;
    // The non-zero check must precede the modulo: `512 % 0` would itself panic.
    debug_assert!(
        log2_alphabet > 0 && 512 % log2_alphabet == 0,
        "key_depth needs log2(A::LEN) to divide 512 (true for Arity16/Arity256)",
    );
    512 / log2_alphabet
}

/// The three tree shapes. Each is a *complete* tree: every node at depth `d`
/// has exactly `fanout(d)` children, so there is no budget truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Chain,
    Bushy,
    Realistic,
}

impl Shape {
    /// Children of a node at `depth` (0 ⇒ leaf).
    fn fanout<A: Arity>(self, depth: usize) -> usize {
        match self {
            Self::Chain => usize::from(depth + 1 < key_depth::<A>()),
            Self::Bushy => {
                if depth < BUSHY_DEPTH {
                    BUSHY_FANOUT
                } else {
                    0
                }
            }
            Self::Realistic => REALISTIC_FANOUTS.get(depth).copied().unwrap_or(0),
        }
    }

    /// `path` length for nodes of this shape.
    const fn path_len(self) -> usize {
        match self {
            Self::Realistic => REALISTIC_PATH_LEN,
            Self::Chain | Self::Bushy => 0,
        }
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Chain => "Chain",
            Self::Bushy => "Bushy",
            Self::Realistic => "Realistic",
        };
        f.write_str(s)
    }
}

/// Evenly spread index of child `k` of `f` across `[0, A::LEN)`.
fn child_index<A: Arity>(k: usize, f: usize) -> A::Index {
    debug_assert!(0 < f && f <= A::LEN && k < f);
    <A::Index as Niche>::try_from_usize(k * (A::LEN / f)).expect("k * (LEN / f) < LEN for k < f")
}

/// A deterministic `VALUE_LEN`-byte payload tagged by depth.
fn make_value(depth: usize) -> Box<[u8]> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "depth tag wraps into a byte intentionally; only determinism matters"
    )]
    let tag = depth as u8;
    vec![tag; VALUE_LEN].into_boxed_slice()
}

/// A deterministic `len`-element path (empty ⇒ no allocation).
fn make_path<A: Arity>(len: usize) -> Box<[A::Index]> {
    (0..len)
        .map(|j| <A::Index as Niche>::try_from_usize(j % A::LEN).expect("j % LEN < LEN"))
        .collect()
}

/// Builds the deterministic complete tree for `shape`.
#[must_use]
pub fn build<A: Arity, S: ChildStore<A>>(shape: Shape) -> Trie<A, S> {
    *build_node::<A, S>(shape, 0)
}

// Allocates a childless node directly on the heap. The `Trie` literal — up to
// ~12 KiB for a `FixedStore` + `Arity256` node, whose children array is inline
// — lives only in this (non-recursive, leaf) frame before `Box::new` moves it
// to the heap, so it never accumulates across the deep `build_node` recursion.
#[expect(
    clippy::unnecessary_box_returns,
    reason = "Box is load-bearing: keeps the up-to-12 KiB node off build_node's recursive frames"
)]
fn alloc_node<A: Arity, S: ChildStore<A>>(shape: Shape, depth: usize) -> Box<Trie<A, S>> {
    Box::new(Trie {
        path: make_path::<A>(shape.path_len()),
        value: Some(make_value(depth)),
        children: ChildMap::empty(),
    })
}

#[expect(
    clippy::unnecessary_box_returns,
    reason = "Box is load-bearing: build_node's recursive frame holds only the 8-byte Box, not the up-to-12 KiB node"
)]
fn build_node<A: Arity, S: ChildStore<A>>(shape: Shape, depth: usize) -> Box<Trie<A, S>> {
    let fanout = shape.fanout::<A>(depth);
    // INVARIANT: insert children one at a time into an empty map. This is what
    // gives each Gapped/Packed node its minimal power-of-two capacity; switching
    // to batch / From-based construction would change the clone-cost profile.
    let mut node = alloc_node::<A, S>(shape, depth);
    for k in 0..fanout {
        let index = child_index::<A>(k, fanout);
        let child = build_node::<A, S>(shape, depth + 1);
        ChildMap::insert(&mut node.children, index, Edge::Mutable(child));
    }
    node
}
