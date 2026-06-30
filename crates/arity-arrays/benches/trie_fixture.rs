//! Bench/test-only compressed-trie fixture used by the `trie` benchmark and the
//! `trie_internals` integration test. Each node stores its children in a
//! representation chosen by a `ChildStore` marker, so the same recursive
//! `Clone`/`Drop` workload is timed across `GappedArray`, `PackedArray`,
//! `FixedArray`, and a `BTreeMap` baseline. Not part of the public API.

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
    type Map<V>: ChildMap<A, V>;
}

/// The minimal index-to-value map the builder and the test walker need.
///
/// `clone_map` clones the map structure by applying a caller-supplied clone
/// function to each present value. This avoids propagating a `V: Clone` bound
/// into the store implementations and breaks the trait-solver cycle that would
/// arise from `GappedArray<Edge<A,S>, A>: Clone` ↔ `Edge<A,S>: Clone`.
pub trait ChildMap<A: Arity, V> {
    fn empty() -> Self;
    fn insert(&mut self, index: A::Index, value: V) -> Option<V>;
    fn get(&self, index: A::Index) -> Option<&V>;
    fn clone_map(&self, clone_one: impl Fn(&V) -> V) -> Self;
}

pub struct GappedStore;
pub struct PackedStore;
pub struct FixedStore;
pub struct BTreeStore;

impl<A: Arity> ChildStore<A> for GappedStore {
    type Map<V> = GappedArray<V, A>;
}
impl<A: Arity> ChildStore<A> for PackedStore {
    type Map<V> = PackedArray<V, A>;
}
impl<A: Arity> ChildStore<A> for FixedStore {
    type Map<V> = FixedArray<Option<V>, A>;
}
impl<A: Arity> ChildStore<A> for BTreeStore {
    type Map<V> = BTreeMap<usize, V>;
}

impl<A: Arity, V> ChildMap<A, V> for GappedArray<V, A> {
    fn empty() -> Self {
        Self::new()
    }
    fn insert(&mut self, i: A::Index, v: V) -> Option<V> {
        Self::insert(self, i, v)
    }
    fn get(&self, i: A::Index) -> Option<&V> {
        Self::get(self, i)
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
    fn get(&self, i: A::Index) -> Option<&V> {
        Self::get(self, i)
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
    fn get(&self, i: A::Index) -> Option<&V> {
        Self::get(self, i).as_ref()
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
    fn get(&self, i: A::Index) -> Option<&V> {
        Self::get(self, &i.as_usize())
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
const BUSHY_FANOUT: usize = 4;

// Depths are cfg(miri)-reduced so the Miri-checked tests stay bounded; the
// benchmark always runs natively, so it uses the full sizes.
#[cfg(not(miri))]
const BUSHY_DEPTH: usize = 6; // (4^7 - 1) / 3 = 5461 nodes
#[cfg(miri)]
const BUSHY_DEPTH: usize = 3; // (4^4 - 1) / 3 = 85 nodes
#[cfg(not(miri))]
const REALISTIC_FANOUTS: &[usize] = &[16, 8, 4, 2, 1, 1, 1, 1]; // 5777 nodes
#[cfg(miri)]
const REALISTIC_FANOUTS: &[usize] = &[4, 2, 1]; // 1 + 4 + 8 + 8 = 21 nodes

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

/// Total node count a `build::<A, _>(shape)` produces — computed from the shape
/// parameters, independent of the recursive construction.
#[must_use]
pub fn expected_node_count<A: Arity>(shape: Shape) -> usize {
    match shape {
        Shape::Chain => key_depth::<A>(),
        Shape::Bushy => {
            // Full BUSHY_FANOUT-ary tree of depth BUSHY_DEPTH: a geometric sum.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "BUSHY_DEPTH is 3 or 6 — both fit in u32 with ample room"
            )]
            let depth_u32 = BUSHY_DEPTH as u32;
            (BUSHY_FANOUT.pow(depth_u32 + 1) - 1) / (BUSHY_FANOUT - 1)
        }
        Shape::Realistic => {
            let mut total = 1; // the root
            let mut level = 1; // nodes at the current depth
            for &f in REALISTIC_FANOUTS {
                level *= f;
                total += level;
            }
            total
        }
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

// Returns `Box<Trie>` and delegates map initialisation and Trie construction to
// helpers so that `build_node`'s own frame holds only small pointer-sized
// locals (~70 bytes) during deep recursion. Without this,
// `FixedArray<Option<Edge>, Arity256>` (12 KiB inline) would accumulate across
// 64 recursive Chain frames and overflow the test-thread stack (~1 MiB) in a
// debug build.
#[expect(
    clippy::unnecessary_box_returns,
    reason = "Box is load-bearing: keeps the 12 KiB FixedStore frame off the \
              recursive call stack in debug builds (see module-level comment)"
)]
fn build_node<A: Arity, S: ChildStore<A>>(shape: Shape, depth: usize) -> Box<Trie<A, S>> {
    let fanout = shape.fanout::<A>(depth);
    // INVARIANT: insert children one at a time into an empty map. This is what
    // gives each Gapped/Packed node its minimal power-of-two capacity; switching
    // to batch / From-based construction would change the clone-cost profile.
    let mut children_box = build_empty_map::<A, S>();
    for k in 0..fanout {
        let index = child_index::<A>(k, fanout);
        let child = build_node::<A, S>(shape, depth + 1);
        ChildMap::insert(&mut *children_box, index, Edge::Mutable(child));
    }
    box_trie::<A, S>(
        make_path::<A>(shape.path_len()),
        make_value(depth),
        children_box,
    )
}

// `ChildMap::empty()` returns the map by value; for `FixedStore + Arity256`
// that value is 12 KiB. Placing the call in its own stack frame keeps that
// temporary off `build_node`'s frame, which is live across the recursive call.
#[expect(
    clippy::unnecessary_box_returns,
    reason = "Box is load-bearing: confines the 12 KiB ChildMap temporary to \
              this helper's frame, not build_node's (see module-level comment)"
)]
fn build_empty_map<A: Arity, S: ChildStore<A>>() -> Box<<S as ChildStore<A>>::Map<Edge<A, S>>> {
    Box::new(ChildMap::empty())
}

// Consumes `children_box`, materialises `children` (up to 12 KiB for
// `FixedStore + Arity256`) in this helper's own frame rather than in
// `build_node`'s, then boxes the completed `Trie`.
fn box_trie<A: Arity, S: ChildStore<A>>(
    path: Box<[A::Index]>,
    value: Box<[u8]>,
    #[expect(
        clippy::boxed_local,
        reason = "Box is load-bearing: the caller passes a heap-allocated map so \
                  the 12 KiB FixedStore map does not sit in build_node's frame"
    )]
    children_box: Box<<S as ChildStore<A>>::Map<Edge<A, S>>>,
) -> Box<Trie<A, S>> {
    let children = *children_box;
    Box::new(Trie {
        path,
        value: Some(value),
        children,
    })
}
