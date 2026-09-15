//! In-order iteration over the valued nodes of a trie.
//!
//! Both [`iter`] and [`visit`] walk every valued node in ascending full-path
//! order, starting at the first key `>= start`. Pre-order emission is correct
//! because a node's full path is a prefix of, and therefore sorts before,
//! every key beneath it.

use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;
use core::ops::ControlFlow;
use core::ptr;

use arity_arrays::Arity;
use arity_arrays::bitmap::Bitmap;

use crate::Node;
use crate::Path;
use crate::chain::ChainStack;
use crate::children::ChildMap;
use crate::children::ChildStore;
use crate::path::common_prefix;
use crate::store::EdgeStore;

/// A pre-order walk over every node, valued or not, shared by iteration and
/// validation. It owns a handle chain and a frame per chain entry plus the
/// root, `(node, full path length, children still to visit)`, with one shared
/// path buffer.
pub(crate) struct Walk<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    store: &'a St,
    chain: ChainStack<'a, St, V, A, S>,
    frames: Vec<Frame<V, St::Edge, A, S>>,
    buf: Vec<A::Index>,
    /// The root, until the first step takes it.
    root: Option<*const Node<V, St::Edge, A, S>>,
    state: State,
    start: Option<Path<A>>,
    _borrow: PhantomData<&'a Node<V, St::Edge, A, S>>,
}

/// A node on the current path, the length of its full path in the shared
/// buffer, and its children still to visit.
type Frame<V, E, A, S> = (*const Node<V, E, A, S>, usize, <A as Arity>::Bitmap);

enum State {
    /// Nothing visited yet.
    Fresh,
    /// Descending towards `start`.
    Seeking,
    Walking,
    Done,
}

impl<'a, V, A, S, St> Walk<'a, V, A, S, St>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    pub(crate) fn new(
        root: Option<&'a Node<V, St::Edge, A, S>>,
        store: &'a St,
        start: Option<&[A::Index]>,
    ) -> Self {
        Self {
            store,
            chain: ChainStack::new(),
            frames: Vec::new(),
            buf: Vec::new(),
            root: root.map(ptr::from_ref),
            state: State::Fresh,
            start: start.map(Path::from),
            _borrow: PhantomData,
        }
    }

    /// The next node in pre-order with its full path, or `None` at the end.
    /// After an error the walk is done.
    ///
    /// The reference is valid until the next call: it points at the root or
    /// at the pointee of a chain handle, which stays in place until popped.
    #[expect(
        clippy::type_complexity,
        reason = "the layering is the API: a store error, then end or not, then the path and node"
    )]
    pub(crate) fn advance(
        &mut self,
    ) -> Result<Option<(&[A::Index], &Node<V, St::Edge, A, S>)>, St::Error> {
        let next = match self.state {
            State::Done => return Ok(None),
            State::Fresh => match self.root.take() {
                None => Ok(None),
                Some(root) => {
                    // SAFETY: the root is borrowed for `'a`, which `self`
                    // cannot outlive.
                    let node = unsafe { &*root };
                    self.buf.extend_from_slice(node.partial_path());
                    self.frames
                        .push((root, self.buf.len(), node.children().present()));
                    self.state = if self.start.is_some() {
                        State::Seeking
                    } else {
                        State::Walking
                    };
                    match self.state {
                        State::Seeking => self.seek(),
                        _ => Ok(Some(root)),
                    }
                }
            },
            State::Seeking => self.seek(),
            State::Walking => self.step(),
        };
        match next {
            Ok(Some(node)) => {
                // SAFETY: `node` is the root or the pointee of a live chain
                // handle; see `Walk::advance`'s documentation.
                Ok(Some((&self.buf, unsafe { &*node })))
            }
            Ok(None) => {
                self.state = State::Done;
                Ok(None)
            }
            Err(e) => {
                self.state = State::Done;
                Err(e)
            }
        }
    }

    /// The next node in pre-order, with the walk already positioned.
    #[expect(
        clippy::type_complexity,
        reason = "the pointer stands in for a node reference the chain owns; a named alias would \
                  only hide that"
    )]
    fn step(&mut self) -> Result<Option<*const Node<V, St::Edge, A, S>>, St::Error> {
        loop {
            let Some((node, path_len, remaining)) = self.frames.last_mut() else {
                return Ok(None);
            };
            let Some(i) = remaining.select(0) else {
                self.frames.pop();
                if !self.frames.is_empty() {
                    self.chain.pop();
                }
                continue;
            };
            *remaining = remaining.without_bit(i);
            self.buf.truncate(*path_len);
            self.buf.push(i);
            // SAFETY: the frame's node is the root or the pointee of a chain
            // handle at the frame's depth, which is still in the chain.
            let parent = unsafe { &**node };
            let edge = parent.children().get(i).expect("present");
            let handle = self.store.read(edge)?;
            let child = self.chain.push(handle);
            // SAFETY: just pushed; its handle stays until popped.
            let child_node = unsafe { &*child };
            self.buf.extend_from_slice(child_node.partial_path());
            self.frames
                .push((child, self.buf.len(), child_node.children().present()));
            return Ok(Some(child));
        }
    }

    /// Descends towards `start`, pruning children that sort before it, and
    /// returns the first node whose full path is `>= start`.
    #[expect(
        clippy::type_complexity,
        reason = "the pointer stands in for a node reference the chain owns; a named alias would \
                  only hide that"
    )]
    fn seek(&mut self) -> Result<Option<*const Node<V, St::Edge, A, S>>, St::Error> {
        let start = self.start.take().expect("seeking has a start");
        loop {
            let (node, _, remaining) = self.frames.last_mut().expect("positioned");
            let o = common_prefix(&self.buf, &start);
            match (o.unique_a.first(), o.unique_b.first()) {
                // At `start`, or past it with `start` a proper prefix.
                (_, None) => {
                    self.state = State::Walking;
                    return Ok(Some(*node));
                }
                // The node's path is a proper prefix of `start`: keep the
                // children at or after the next index of `start`, and descend
                // into the one at that index if present.
                (None, Some(&si)) => {
                    while let Some(j) = remaining.select(0) {
                        if j >= si {
                            break;
                        }
                        *remaining = remaining.without_bit(j);
                    }
                    if !remaining.test(si) {
                        // This node sorts before `start` and the remaining
                        // children after it.
                        self.state = State::Walking;
                        return self.step();
                    }
                    self.step()?;
                }
                // Diverged: the whole subtree is on one side of `start`.
                (Some(a), Some(b)) => {
                    self.state = State::Walking;
                    if a > b {
                        return Ok(Some(*node));
                    }
                    self.frames.pop();
                    if !self.frames.is_empty() {
                        self.chain.pop();
                    }
                    return self.step();
                }
            }
        }
    }
}

/// Every valued node at or after `start`, ascending, as an owned path and a
/// cloned value.
///
/// The iterator owns a chain of read handles, so it cannot lend its path
/// buffer or a borrow of a node; [`visit`] is the form that allocates nothing
/// per item. A store error is yielded once, after which the iterator is
/// finished.
pub fn iter<'a, V, A, S, St>(
    root: Option<&'a Node<V, St::Edge, A, S>>,
    store: &'a St,
    start: Option<&[A::Index]>,
) -> Iter<'a, V, A, S, St>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    Iter {
        walk: Walk::new(root, store, start),
    }
}

/// The iterator returned by [`iter`].
pub struct Iter<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + 'a,
{
    walk: Walk<'a, V, A, S, St>,
}

impl<V, A, S, St> Iterator for Iter<'_, V, A, S, St>
where
    V: Clone,
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    type Item = Result<(Path<A>, V), St::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.walk.advance() {
                Ok(Some((path, node))) => {
                    if let Some(value) = node.value() {
                        return Some(Ok((Path::from(path), value.clone())));
                    }
                }
                Ok(None) => return None,
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

impl<V, A, S, St> fmt::Debug for Iter<'_, V, A, S, St>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter")
            .field("depth", &self.walk.frames.len())
            .finish_non_exhaustive()
    }
}

// SAFETY: the raw pointers stand in for `&'a Node` references and the chain
// is a `Vec` of `St::Shared<'a>` handles, so the bounds are those the safe
// equivalent would get for free.
unsafe impl<'a, V, A, S, St> Send for Walk<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    A::Index: Send,
    A::Bitmap: Send,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + Sync + 'a,
    St::Shared<'a>: Send,
    Node<V, St::Edge, A, S>: Sync,
{
}

// SAFETY: sharing a walk shares `&Node` and `&St::Shared<'a>` per level,
// which is sound exactly when both are `Sync`.
unsafe impl<'a, V, A, S, St> Sync for Walk<'a, V, A, S, St>
where
    V: 'a,
    A: Arity + 'a,
    A::Index: Sync,
    A::Bitmap: Sync,
    S: ChildStore<A> + 'a,
    St: EdgeStore<V, A, S> + Sync + 'a,
    St::Shared<'a>: Sync,
    Node<V, St::Edge, A, S>: Sync,
{
}

/// Calls `f` with the full path and node of every valued node at or after
/// `start`, ascending, until it returns `Break`.
///
/// Allocates nothing per item; the path and node are borrowed for the call.
///
/// # Errors
///
/// The store's error from a [`read`](EdgeStore::read); the walk stops there.
pub fn visit<'a, V, A, S, St, F>(
    root: Option<&'a Node<V, St::Edge, A, S>>,
    store: &'a St,
    start: Option<&[A::Index]>,
    mut f: F,
) -> Result<ControlFlow<()>, St::Error>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
    F: FnMut(&[A::Index], &Node<V, St::Edge, A, S>) -> ControlFlow<()>,
{
    let mut walk = Walk::new(root, store, start);
    while let Some((path, node)) = walk.advance()? {
        if node.value().is_some() && f(path, node).is_break() {
            return Ok(ControlFlow::Break(()));
        }
    }
    Ok(ControlFlow::Continue(()))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;
    use core::ops::ControlFlow;

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
    use crate::store::EdgeStore;
    use crate::testing::Failing;

    type N = Node<u32, MemEdge<u32, Arity16, Packed, u64>, Arity16, Packed>;
    type St = InMemory<u64>;

    fn p(bytes: &[u8]) -> Path<Arity16> {
        Path::try_from_bytes(bytes).expect("in range")
    }

    const KEYS: &[(&[u8], u32)] = &[
        (&[1, 2, 3], 3),
        (&[1, 2, 4], 4),
        (&[1, 2, 4, 5], 5),
        (&[1, 7], 7),
        (&[8], 8),
        (&[8, 0, 0, 0], 800),
    ];

    fn build(keys: &[(&[u8], u32)]) -> (Option<N>, St, BTreeMap<Vec<u8>, u32>) {
        let (mut root, mut store) = (None, St::default());
        let mut model = BTreeMap::new();
        for (key, value) in keys {
            insert(&mut root, &mut store, &p(key), *value).expect("infallible");
            model.insert(key.to_vec(), *value);
        }
        (root, store, model)
    }

    fn collect(root: Option<&N>, store: &St, start: Option<&[u8]>) -> Vec<(Vec<u8>, u32)> {
        let start = start.map(p);
        iter(root, store, start.as_deref())
            .map(|item| {
                let (path, value) = item.expect("infallible");
                (path.as_bytes().to_vec(), value)
            })
            .collect()
    }

    #[test]
    fn iter_yields_every_key_ascending() {
        let (root, store, model) = build(KEYS);
        let expected: Vec<(Vec<u8>, u32)> = model.into_iter().collect();
        assert_eq!(collect(root.as_ref(), &store, None), expected);
    }

    #[test]
    fn iter_over_the_empty_trie_yields_nothing() {
        let store = St::default();
        assert_eq!(collect(None, &store, None), []);
        assert_eq!(collect(None, &store, Some(&[1])), []);
    }

    #[test]
    fn iter_from_a_start_matches_the_model_range() {
        let (root, store, model) = build(KEYS);
        for start in [
            &[][..],
            &[0],
            &[1],
            &[1, 2],
            &[1, 2, 3],
            &[1, 2, 3, 0],
            &[1, 2, 4],
            &[1, 2, 4, 4],
            &[1, 2, 4, 5, 0],
            &[1, 3],
            &[1, 7, 7],
            &[7],
            &[8],
            &[8, 0],
            &[8, 0, 0, 0],
            &[8, 0, 0, 1],
            &[9],
            &[15, 15, 15],
        ] {
            let expected: Vec<(Vec<u8>, u32)> = model
                .range(start.to_vec()..)
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            assert_eq!(
                collect(root.as_ref(), &store, Some(start)),
                expected,
                "start {start:?}"
            );
        }
    }

    #[test]
    fn iter_yields_a_store_error_once_and_then_ends() {
        let (root, store, _) = build(KEYS);
        // Reads 1 to 3 reach the first leaf; the fourth is the next leaf.
        let failing = Failing::new(store, 4, 0);
        let mut it = iter(root.as_ref(), &failing, None);
        assert!(it.next().expect("first").is_ok());
        assert_eq!(it.next().expect("second").err(), Some("read"));
        assert!(it.next().is_none());
        assert!(it.next().is_none());
    }

    #[test]
    fn iter_over_a_deep_chain_does_not_recurse() {
        let depth = if cfg!(miri) { 256 } else { 100_000 };
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let mut store = St::default();
                let mut node = N::leaf(Path::new(), 7);
                for _ in 0..depth {
                    let mut parent = N::new(Path::new(), None, PackedArray::default());
                    parent
                        .children_mut()
                        .insert(U4::new_masked(0), store.inline(node));
                    node = parent;
                }
                let found = iter(Some(&node), &store, None).count();
                let stopped = visit(Some(&node), &store, None, |_, _| ControlFlow::Break(()))
                    .expect("infallible");
                drop(node);
                (found, stopped)
            })
            .expect("spawn");
        assert_eq!(
            handle.join().expect("no overflow"),
            (1, ControlFlow::Break(()))
        );
    }

    #[test]
    fn visit_hands_out_borrowed_views_and_stops_on_break() {
        let (root, store, model) = build(KEYS);
        let mut seen = Vec::new();
        let flow = visit(root.as_ref(), &store, Some(&p(&[1, 2, 4])), |path, node| {
            seen.push((
                U4::as_u8_slice(path).to_vec(),
                *node.value().expect("valued"),
            ));
            if seen.len() == 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("infallible");
        assert_eq!(flow, ControlFlow::Break(()));
        let expected: Vec<(Vec<u8>, u32)> = model
            .range(alloc::vec![1, 2, 4]..)
            .take(3)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        assert_eq!(seen, expected);
        let flow = visit(
            root.as_ref(),
            &store,
            None,
            |_, _| ControlFlow::Continue(()),
        )
        .expect("infallible");
        assert_eq!(flow, ControlFlow::Continue(()));
    }

    #[test]
    fn visit_returns_a_store_error() {
        let (root, store, _) = build(KEYS);
        let failing = Failing::new(store, 1, 0);
        assert_eq!(
            visit(root.as_ref(), &failing, None, |_, _| ControlFlow::Continue(
                ()
            )),
            Err("read")
        );
    }
}
