//! Shared test support: the three test stores, a failing wrapper, the FNV
//! test hashers, and a generic model runner.
#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::ops::Deref;
use std::rc::Rc;

use arity_arrays::index::Niche;
use arity_tries::Arity;
use arity_tries::ChildStore;
use arity_tries::EdgeStore;
use arity_tries::HashInput;
use arity_tries::Node;
use arity_tries::Path;
use arity_tries::TrieHasher;
use arity_tries::drop_subtree;
use arity_tries::get;
use arity_tries::hash;
use arity_tries::insert;
use arity_tries::iter;
use arity_tries::remove;
use arity_tries::remove_prefix;
use arity_tries::validate;
use arity_tries::visit;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use stable_deref_trait::StableDeref;

pub type V = u32;
pub type H = u64;

/// Builds an index from a byte the caller keeps in range.
pub fn idx<A: Arity>(b: u8) -> A::Index {
    A::Index::try_from_usize(usize::from(b)).expect("in range")
}

pub fn path<A: Arity>(bytes: &[u8]) -> Path<A> {
    Path::try_from_bytes(bytes).expect("in range")
}

// ---------------------------------------------------------------------------
// Firewood-shaped store: Box inline, Rc sealed, enum handle.

pub enum RcEdge<A: Arity, S: ChildStore<A>> {
    Inline(Box<Node<V, Self, A, S>>),
    Sealed(Rc<Node<V, Self, A, S>>, H),
}

impl<A: Arity, S: ChildStore<A>> Drop for RcEdge<A, S> {
    fn drop(&mut self) {
        let node = match self {
            Self::Inline(node) => std::mem::take(&mut **node),
            Self::Sealed(rc, _) => match Rc::get_mut(rc) {
                Some(node) => std::mem::take(node),
                None => return,
            },
        };
        if node.is_leaf() {
            return;
        }
        drop_subtree(node, |mut edge| match &mut edge {
            Self::Inline(node) => Some(std::mem::take(&mut **node)),
            Self::Sealed(rc, _) => Rc::get_mut(rc).map(std::mem::take),
        });
    }
}

pub enum RcShared<'e, N> {
    Borrowed(&'e N),
    Owned(Rc<N>),
}

impl<N> Deref for RcShared<'_, N> {
    type Target = N;

    fn deref(&self) -> &N {
        match self {
            Self::Borrowed(n) => n,
            Self::Owned(rc) => rc,
        }
    }
}

// SAFETY: both variants dereference to a heap node that does not move when
// the handle moves: a plain reference, or an `Rc` whose pointee is on the
// heap.
unsafe impl<N> StableDeref for RcShared<'_, N> {}

/// Counts materializations of sealed edges, the replacement record a
/// persistent store would keep.
#[derive(Default)]
pub struct RcStore {
    pub replaced: usize,
}

impl<A: Arity, S: ChildStore<A>> EdgeStore<V, A, S> for RcStore {
    type Edge = RcEdge<A, S>;
    type Hash = H;
    type Error = Infallible;
    type Shared<'e>
        = RcShared<'e, Node<V, Self::Edge, A, S>>
    where
        Self: 'e,
        V: 'e,
        A: 'e,
        S: 'e;

    fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<Self::Shared<'e>, Infallible> {
        Ok(match edge {
            RcEdge::Inline(node) => RcShared::Borrowed(node),
            RcEdge::Sealed(rc, _) => RcShared::Owned(Rc::clone(rc)),
        })
    }

    fn as_inline(edge: &mut Self::Edge) -> Option<&mut Node<V, Self::Edge, A, S>> {
        match edge {
            RcEdge::Inline(node) => Some(node),
            RcEdge::Sealed(..) => None,
        }
    }

    fn materialize<'e>(
        &mut self,
        edge: &'e mut Self::Edge,
    ) -> Result<&'e mut Node<V, Self::Edge, A, S>, Infallible> {
        if let RcEdge::Sealed(rc, _) = edge {
            let node = Rc::get_mut(rc).map_or_else(
                || unreachable!("no handle is live during a mutation"),
                std::mem::take,
            );
            self.replaced += 1;
            *edge = RcEdge::Inline(Box::new(node));
        }
        match edge {
            RcEdge::Inline(node) => Ok(node),
            RcEdge::Sealed(..) => unreachable!("just made inline"),
        }
    }

    fn inline(&mut self, node: Node<V, Self::Edge, A, S>) -> Self::Edge {
        RcEdge::Inline(Box::new(node))
    }

    fn seal(&mut self, edge: &mut Self::Edge, hash: H) {
        match edge {
            RcEdge::Inline(node) => {
                let node = std::mem::take(&mut **node);
                *edge = RcEdge::Sealed(Rc::new(node), hash);
            }
            RcEdge::Sealed(_, h) => *h = hash,
        }
    }

    fn hash(edge: &Self::Edge) -> Option<&H> {
        match edge {
            RcEdge::Inline(_) => None,
            RcEdge::Sealed(_, h) => Some(h),
        }
    }
}

// ---------------------------------------------------------------------------
// By-value store: the node lives inside the edge, inside the parent's map.

pub struct ValueEdge<A: Arity, S: ChildStore<A>> {
    node: Node<V, Self, A, S>,
    hash: Option<H>,
}

impl<A: Arity, S: ChildStore<A>> Drop for ValueEdge<A, S> {
    fn drop(&mut self) {
        if !self.node.is_leaf() {
            drop_subtree(std::mem::take(&mut self.node), |mut edge| {
                Some(std::mem::take(&mut edge.node))
            });
        }
    }
}

#[derive(Default)]
pub struct ValueStore;

impl<A: Arity, S: ChildStore<A>> EdgeStore<V, A, S> for ValueStore {
    type Edge = ValueEdge<A, S>;
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
        ValueEdge { node, hash: None }
    }

    fn seal(&mut self, edge: &mut Self::Edge, hash: H) {
        edge.hash = Some(hash);
    }

    fn hash(edge: &Self::Edge) -> Option<&H> {
        edge.hash.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Failing wrapper.

/// Wraps an infallible store and fails `read` on the `fail_read_at`th read
/// and `materialize` on the `fail_materialize_at`th materialize (1-based; `0`
/// never fails).
pub struct FailNth<St> {
    pub inner: St,
    reads: Cell<usize>,
    fail_read_at: usize,
    materializes: usize,
    fail_materialize_at: usize,
}

impl<St> FailNth<St> {
    pub const fn new(inner: St, fail_read_at: usize, fail_materialize_at: usize) -> Self {
        Self {
            inner,
            reads: Cell::new(0),
            fail_read_at,
            materializes: 0,
            fail_materialize_at,
        }
    }
}

impl<A, S, St> EdgeStore<V, A, S> for FailNth<St>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Error = Infallible>,
{
    type Edge = St::Edge;
    type Hash = St::Hash;
    type Error = &'static str;
    type Shared<'e>
        = St::Shared<'e>
    where
        Self: 'e,
        V: 'e,
        A: 'e,
        S: 'e;

    fn read<'e>(&'e self, edge: &'e Self::Edge) -> Result<Self::Shared<'e>, &'static str> {
        self.reads.set(self.reads.get() + 1);
        if self.reads.get() == self.fail_read_at {
            return Err("read");
        }
        self.inner.read(edge).map_err(|e| match e {})
    }

    fn as_inline(edge: &mut Self::Edge) -> Option<&mut Node<V, Self::Edge, A, S>> {
        St::as_inline(edge)
    }

    fn materialize<'e>(
        &mut self,
        edge: &'e mut Self::Edge,
    ) -> Result<&'e mut Node<V, Self::Edge, A, S>, &'static str> {
        self.materializes += 1;
        if self.materializes == self.fail_materialize_at {
            return Err("materialize");
        }
        self.inner.materialize(edge).map_err(|e| match e {})
    }

    fn inline(&mut self, node: Node<V, Self::Edge, A, S>) -> Self::Edge {
        self.inner.inline(node)
    }

    fn seal(&mut self, edge: &mut Self::Edge, hash: Self::Hash) {
        self.inner.seal(edge, hash);
    }

    fn hash(edge: &Self::Edge) -> Option<&Self::Hash> {
        St::hash(edge)
    }
}

// ---------------------------------------------------------------------------
// FNV test hashers.

/// A 64-bit FNV-1a hasher over everything in the input. With
/// `sensitive_depth`, children of a node at that full-path length hash in a
/// distinct lone form when they have no siblings. With `rewrite_depth`, a node
/// at that full-path length has its value replaced by a digest of its
/// children's hashes before hashing.
#[derive(Clone, Copy)]
pub struct Fnv {
    pub seed: u64,
    pub sensitive_depth: Option<usize>,
    pub rewrite_depth: Option<usize>,
}

impl Fnv {
    pub const PLAIN: Self = Self {
        seed: 0xcbf2_9ce4_8422_2325,
        sensitive_depth: None,
        rewrite_depth: None,
    };
    pub const SEEDED: Self = Self {
        seed: 1,
        ..Self::PLAIN
    };
    pub const SENSITIVE: Self = Self {
        sensitive_depth: Some(1),
        ..Self::PLAIN
    };
    pub const REWRITING: Self = Self {
        rewrite_depth: Some(1),
        ..Self::PLAIN
    };

    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn digest<'a, I: Niche>(children: impl Iterator<Item = (I, &'a u64)>) -> u64 {
        let mut h = 0x9e37_79b9_7f4a_7c15;
        for (i, child) in children {
            Self::feed(&mut h, &[
                u8::try_from(i.as_usize()).expect("index fits a byte")
            ]);
            Self::feed(&mut h, &child.to_le_bytes());
        }
        h
    }
}

impl<A: Arity> TrieHasher<V, A> for Fnv {
    type Hash = H;

    fn hash_node<'a, C>(&self, input: HashInput<'a, V, A, C>) -> H
    where
        C: Iterator<Item = (A::Index, &'a H)> + Clone,
    {
        let mut h = self.seed;
        Self::feed(&mut h, A::Index::as_u8_slice(input.leading_path));
        Self::feed(&mut h, &[0xff]);
        Self::feed(&mut h, A::Index::as_u8_slice(input.partial_path));
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

    fn sibling_sensitive(&self, leading_path: &[A::Index], partial_path: &[A::Index]) -> bool {
        self.sensitive_depth == Some(leading_path.len() + partial_path.len())
    }

    fn update_value<'a, C>(
        &self,
        leading_path: &[A::Index],
        partial_path: &[A::Index],
        children: C,
        value: &mut V,
    ) where
        C: Iterator<Item = (A::Index, &'a H)> + Clone,
    {
        if self.rewrite_depth == Some(leading_path.len() + partial_path.len()) {
            *value = u32::try_from(Self::digest(children) & u64::from(u32::MAX)).expect("masked");
        }
    }
}

// ---------------------------------------------------------------------------
// Model runner.

#[derive(Clone, Debug)]
pub enum Op {
    Insert(Vec<u8>, V),
    Remove(Vec<u8>),
    RemovePrefix(Vec<u8>),
    Get(Vec<u8>),
    Hash,
}

pub fn key<A: Arity>(max_len: usize) -> BoxedStrategy<Vec<u8>> {
    let len = u8::try_from(A::LEN.min(256) - 1).expect("fits");
    proptest::collection::vec(0..=len, 0..=max_len).boxed()
}

/// Keys concentrated on few indices and short lengths, so that single-child
/// and two-child transitions at depth one happen often.
pub fn narrow_key<A: Arity>() -> BoxedStrategy<Vec<u8>> {
    let len = u8::try_from(A::LEN.min(4) - 1).expect("fits");
    proptest::collection::vec(0..=len, 0..=3).boxed()
}

pub fn ops(keys: BoxedStrategy<Vec<u8>>, n: usize) -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        4 => (keys.clone(), any::<V>()).prop_map(|(k, v)| Op::Insert(k, v)),
        2 => keys.clone().prop_map(Op::Remove),
        1 => keys.clone().prop_map(Op::RemovePrefix),
        1 => keys.prop_map(Op::Get),
        2 => Just(Op::Hash),
    ];
    proptest::collection::vec(op, 0..n)
}

pub type Model = BTreeMap<Vec<u8>, V>;

pub fn contents<A, S, St>(root: Option<&Node<V, St::Edge, A, S>>, store: &St) -> Vec<(Vec<u8>, V)>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
    St::Error: Debug,
{
    iter(root, store, None)
        .map(|item| {
            let (path, value) = item.expect("store ok");
            (path.as_bytes().to_vec(), value)
        })
        .collect()
}

/// A trie and its store.
pub type Trie<A, S, St> = (Option<Node<V, <St as EdgeStore<V, A, S>>::Edge, A, S>>, St);

pub fn fresh<A, S, St>(model: &Model) -> Trie<A, S, St>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S> + Default,
    St::Error: Debug,
{
    let (mut root, mut store) = (None, St::default());
    for (k, v) in model {
        insert(&mut root, &mut store, &path::<A>(k), *v).expect("store ok");
    }
    (root, store)
}

pub fn fresh_hash<A, S, St>(model: &Model, hasher: &Fnv) -> Option<H>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H> + Default,
    St::Error: Debug,
{
    let (mut root, mut store) = fresh::<A, S, St>(model);
    root.as_mut()
        .map(|r| hash(r, &mut store, hasher).expect("store ok"))
}

/// Checks the live trie against the model through `validate`, `iter`,
/// `visit`, `get`, and `iter` from `start`.
pub fn check<A, S, St>(
    root: Option<&Node<V, St::Edge, A, S>>,
    store: &St,
    model: &Model,
    start: &[u8],
) -> Result<(), TestCaseError>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
    St::Error: Debug,
{
    prop_assert!(
        validate(root, store).is_ok(),
        "{:?}",
        validate(root, store).err()
    );
    let expected = || -> Vec<(Vec<u8>, V)> { model.iter().map(|(k, v)| (k.clone(), *v)).collect() };
    prop_assert_eq!(contents(root, store), expected());
    let mut visited = Vec::new();
    let flow = visit(root, store, None, |p, n| {
        visited.push((
            A::Index::as_u8_slice(p).to_vec(),
            *n.value().expect("valued"),
        ));
        std::ops::ControlFlow::Continue(())
    })
    .expect("store ok");
    prop_assert_eq!(flow, std::ops::ControlFlow::Continue(()));
    prop_assert_eq!(visited, expected());
    for (k, v) in model {
        prop_assert_eq!(get(root, store, &path::<A>(k)).expect("store ok"), Some(*v));
    }
    let ranged: Vec<(Vec<u8>, V)> = model
        .range(start.to_vec()..)
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let live: Vec<(Vec<u8>, V)> = iter(root, store, Some(&path::<A>(start)))
        .map(|item| {
            let (p, v) = item.expect("store ok");
            (p.as_bytes().to_vec(), v)
        })
        .collect();
    prop_assert_eq!(live, ranged);
    Ok(())
}

/// Applies `op` to the live trie and the model, hashing under `hasher`.
pub fn apply<A, S, St>(
    root: &mut Option<Node<V, St::Edge, A, S>>,
    store: &mut St,
    model: &mut Model,
    hasher: &Fnv,
    op: &Op,
) -> Result<(), TestCaseError>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H>,
    St::Error: Debug,
{
    match op {
        Op::Insert(k, v) => {
            prop_assert_eq!(
                insert(root, store, &path::<A>(k), *v).expect("store ok"),
                model.insert(k.clone(), *v)
            );
        }
        Op::Remove(k) => {
            prop_assert_eq!(
                remove(root, store, &path::<A>(k)).expect("store ok"),
                model.remove(k)
            );
        }
        Op::RemovePrefix(p) => {
            remove_prefix(root, store, &path::<A>(p)).expect("store ok");
            model.retain(|k, _| !k.starts_with(p));
        }
        Op::Get(k) => {
            prop_assert_eq!(
                get(root.as_ref(), store, &path::<A>(k)).expect("store ok"),
                model.get(k).copied()
            );
        }
        Op::Hash => {
            if let Some(r) = root.as_mut() {
                hash(r, store, hasher).expect("store ok");
            }
        }
    }
    Ok(())
}

/// The model property: every operation agrees with the model, the trie is
/// valid after each, and the final hash equals a fresh build's.
pub fn run_model<A, S, St>(ops: &[Op], start: &[u8]) -> Result<(), TestCaseError>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H> + Default,
    St::Error: Debug,
{
    let (mut root, mut store, mut model) = (None, St::default(), Model::new());
    for op in ops {
        apply(&mut root, &mut store, &mut model, &Fnv::PLAIN, op)?;
        check(root.as_ref(), &store, &model, start)?;
    }
    let live = root
        .as_mut()
        .map(|r| hash(r, &mut store, &Fnv::PLAIN).expect("store ok"));
    prop_assert_eq!(live, fresh_hash::<A, S, St>(&model, &Fnv::PLAIN));
    Ok(())
}

/// The hash property under a hasher that rewrites values: compared against a
/// fresh build after every hash, through the root hash and every value.
pub fn run_hash<A, S, St>(ops: &[Op], hasher: &Fnv) -> Result<(), TestCaseError>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H> + Default,
    St::Error: Debug,
{
    let (mut root, mut store, mut model) = (None, St::default(), Model::new());
    for op in ops {
        apply(&mut root, &mut store, &mut model, hasher, op)?;
        if matches!(op, Op::Hash) {
            let (mut fresh_root, mut fresh_store) = fresh::<A, S, St>(&model);
            let expected = fresh_root
                .as_mut()
                .map(|r| hash(r, &mut fresh_store, hasher).expect("store ok"));
            let live = root
                .as_mut()
                .map(|r| hash(r, &mut store, hasher).expect("store ok"));
            prop_assert_eq!(live, expected);
            prop_assert_eq!(
                contents(root.as_ref(), &store),
                contents(fresh_root.as_ref(), &fresh_store)
            );
            // The rewritten values do not match the model; refresh it so the
            // next operations compare against what the trie now holds.
            model = contents(root.as_ref(), &store).into_iter().collect();
        }
    }
    Ok(())
}

/// Failure atomicity: after building and sealing, one operation under a store
/// that fails on the nth materialize or read leaves contents and validity as
/// they were.
pub fn run_atomic<A, S, St>(
    ops: &[Op],
    op: &Op,
    fail_at: usize,
    fail_read: bool,
) -> Result<(), TestCaseError>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H, Error = Infallible> + Default,
{
    let (mut root, mut store, mut model) = (None, St::default(), Model::new());
    for op in ops {
        apply(&mut root, &mut store, &mut model, &Fnv::PLAIN, op)?;
    }
    if let Some(r) = root.as_mut() {
        hash(r, &mut store, &Fnv::PLAIN).expect("store ok");
    }
    let before = contents(root.as_ref(), &store);
    let valid_before = validate(root.as_ref(), &store).is_ok();
    let mut failing = if fail_read {
        FailNth::new(store, fail_at, 0)
    } else {
        FailNth::new(store, 0, fail_at)
    };
    let result: Result<(), &'static str> = match op {
        Op::Insert(k, v) => insert(&mut root, &mut failing, &path::<A>(k), *v).map(|_| ()),
        Op::Remove(k) => remove(&mut root, &mut failing, &path::<A>(k)).map(|_| ()),
        Op::RemovePrefix(p) => remove_prefix(&mut root, &mut failing, &path::<A>(p)),
        Op::Get(k) => get(root.as_ref(), &failing, &path::<A>(k)).map(|_| ()),
        Op::Hash => root.as_mut().map_or(Ok(()), |r| {
            hash(r, &mut failing, &Fnv::SENSITIVE).map(|_| ())
        }),
    };
    if result.is_err() {
        prop_assert_eq!(contents(root.as_ref(), &failing.inner), before);
        prop_assert_eq!(
            validate(root.as_ref(), &failing.inner).is_ok(),
            valid_before
        );
    }
    Ok(())
}
