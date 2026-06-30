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
    #[expect(
        dead_code,
        reason = "Frozen models structural sharing; populated by a future frozen-ratio fixture"
    )]
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
        // The wildcard arm avoids explicitly referencing `Frozen`, which would
        // suppress its `dead_code` lint and make the `#[expect(dead_code)]` on
        // the variant unfulfilled.
        #[expect(
            clippy::match_wildcard_for_single_variants,
            reason = "wildcard preserves dead_code for the Frozen variant"
        )]
        match self {
            Self::Mutable(t) => Self::Mutable(t.clone()),
            // Frozen is never constructed by current code; this arm exists only
            // for exhaustiveness. When the frozen-ratio fixture is added this
            // arm must be replaced with the explicit pattern.
            _ => unreachable!("Frozen variant is not yet populated"),
        }
    }
}
