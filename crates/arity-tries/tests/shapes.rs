//! Fixed shapes over every store: Firewood's edge cases and a deep chain
//! through every entry point on a small stack.

mod common;

use std::fmt::Debug;
use std::ops::ControlFlow;

use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_tries::Arity;
use arity_tries::ChildMap;
use arity_tries::ChildStore;
use arity_tries::EdgeStore;
use arity_tries::Fixed;
use arity_tries::Gapped;
use arity_tries::InMemory;
use arity_tries::Node;
use arity_tries::Packed;
use arity_tries::Path;
use arity_tries::get;
use arity_tries::hash;
use arity_tries::insert;
use arity_tries::iter;
use arity_tries::materialize_subtree;
use arity_tries::remove;
use arity_tries::remove_prefix;
use arity_tries::validate;
use arity_tries::visit;
use common::*;

fn edge_cases<A, S, St>()
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H> + Default,
    St::Error: Debug,
{
    let sequences: &[&[Op]] = &[
        // Root leaf, insert displacing a value, removal emptying the trie.
        &[
            Op::Insert(vec![1, 2], 1),
            Op::Insert(vec![1, 2], 2),
            Op::Hash,
            Op::Remove(vec![1, 2]),
        ],
        // Root with an empty partial path and a value, valued node with children.
        &[
            Op::Insert(vec![], 0),
            Op::Insert(vec![1], 1),
            Op::Insert(vec![2], 2),
            Op::Hash,
            Op::Remove(vec![]),
        ],
        // Single-child collapse at the root and mid-trie.
        &[
            Op::Insert(vec![1], 1),
            Op::Insert(vec![1, 2, 3], 3),
            Op::Hash,
            Op::Remove(vec![1]),
            Op::Hash,
        ],
        &[
            Op::Insert(vec![0, 1, 2], 1),
            Op::Insert(vec![0, 1, 3], 2),
            Op::Insert(vec![0, 2], 3),
            Op::Hash,
            Op::Remove(vec![0, 1, 2]),
            Op::Hash,
        ],
        // Removal turning a valued node childless.
        &[
            Op::Insert(vec![1], 1),
            Op::Insert(vec![1, 2], 2),
            Op::Hash,
            Op::Remove(vec![1, 2]),
            Op::Hash,
        ],
        // remove_prefix at the root: the empty prefix ends inside the root's
        // partial path and clears the trie.
        &[
            Op::Insert(vec![1, 2, 3], 3),
            Op::Insert(vec![1, 2, 4], 4),
            Op::Hash,
            Op::RemovePrefix(vec![]),
        ],
        // remove_prefix mid-trie: the prefix names a valueless branch, and
        // the parent collapses onto the survivor.
        &[
            Op::Insert(vec![1, 2, 3], 3),
            Op::Insert(vec![1, 2, 4], 4),
            Op::Insert(vec![1, 7], 7),
            Op::Hash,
            Op::RemovePrefix(vec![1, 2]),
            Op::Hash,
        ],
        // remove_prefix on a leaf, then twice on prefixes that diverge from
        // the collapsed root's partial path, which is the missing-prefix case.
        &[
            Op::Insert(vec![1, 2, 3], 3),
            Op::Insert(vec![1, 7], 7),
            Op::Hash,
            Op::RemovePrefix(vec![1, 2, 3]),
            Op::RemovePrefix(vec![1, 9]),
            Op::RemovePrefix(vec![1, 2]),
            Op::Hash,
        ],
        // remove_prefix ending inside a child's partial path: [1, 2, 3] stops
        // partway along the leaf [3, 4] under index 2, which leaves whole.
        &[
            Op::Insert(vec![1, 2, 3, 4], 1),
            Op::Insert(vec![1, 5], 2),
            Op::Hash,
            Op::RemovePrefix(vec![1, 2, 3]),
            Op::Hash,
        ],
    ];
    for ops in sequences {
        run_model::<A, S, St>(ops, &[1]).expect("model holds");
    }
}

#[test]
fn edge_cases_over_every_store() {
    edge_cases::<Arity16, Packed, InMemory<H>>();
    edge_cases::<Arity16, Fixed, InMemory<H>>();
    edge_cases::<Arity256, Gapped, InMemory<H>>();
    edge_cases::<Arity16, Packed, RcStore>();
    edge_cases::<Arity16, Fixed, RcStore>();
    edge_cases::<Arity256, Gapped, RcStore>();
    edge_cases::<Arity16, Packed, ValueStore>();
    edge_cases::<Arity256, Gapped, ValueStore>();
}

fn chain<A, S, St>(store: &mut St, depth: usize, leaf: V) -> Node<V, St::Edge, A, S>
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S>,
{
    let mut node: Node<V, St::Edge, A, S> = Node::leaf(Path::new(), leaf);
    for _ in 0..depth {
        let mut parent: Node<V, St::Edge, A, S> = Node::new(Path::new(), None, S::Map::default());
        parent
            .children_mut()
            .insert(idx::<A>(0), store.inline(node));
        node = parent;
    }
    node
}

fn deep<A, S, St>(depth: usize)
where
    A: Arity,
    S: ChildStore<A>,
    St: EdgeStore<V, A, S, Hash = H> + Default,
    St::Error: Debug,
{
    let mut store = St::default();
    let mut root = Some(chain::<A, S, St>(&mut store, depth, 7));
    let key = vec![idx::<A>(0); depth];
    assert_eq!(get(root.as_ref(), &store, &key).expect("store ok"), Some(7));
    assert_eq!(iter(root.as_ref(), &store, None).count(), 1);
    assert_eq!(
        iter(root.as_ref(), &store, Some(&key[..depth / 2])).count(),
        1
    );
    assert_eq!(
        visit(
            root.as_ref(),
            &store,
            None,
            |_, _| ControlFlow::Continue(())
        )
        .expect("store ok"),
        ControlFlow::Continue(())
    );
    // Hand-built chains violate the invariant on purpose.
    assert!(validate(root.as_ref(), &store).is_err());
    hash(root.as_mut().expect("root"), &mut store, &Fnv::PLAIN).expect("store ok");
    materialize_subtree(root.as_mut().expect("root"), &mut store).expect("store ok");
    let mut below = key.clone();
    below.push(idx::<A>(1));
    assert_eq!(
        insert(&mut root, &mut store, &below, 8).expect("store ok"),
        None
    );
    assert_eq!(
        remove(&mut root, &mut store, &below).expect("store ok"),
        Some(8)
    );
    assert_eq!(
        remove(&mut root, &mut store, &key).expect("store ok"),
        Some(7)
    );
    assert!(root.is_none());
    let mut root = Some(chain::<A, S, St>(&mut store, depth, 7));
    remove_prefix(&mut root, &mut store, &key[..1]).expect("store ok");
    assert!(root.is_none());
    let root = Some(chain::<A, S, St>(&mut store, depth, 7));
    drop(root);
}

#[test]
fn deep_chain_over_every_store_on_a_small_stack() {
    // The test hasher feeds the whole leading path at every level, so the
    // hash of a chain is quadratic in its depth; ten thousand levels still
    // overflow a 64 KiB stack if any entry point recurses.
    let depth = if cfg!(miri) { 128 } else { 10_000 };
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            deep::<Arity16, Packed, InMemory<H>>(depth);
            deep::<Arity16, Fixed, InMemory<H>>(depth);
            deep::<Arity256, Gapped, InMemory<H>>(depth);
            deep::<Arity16, Packed, RcStore>(depth);
            deep::<Arity16, Fixed, RcStore>(depth);
            deep::<Arity16, Packed, ValueStore>(depth);
            deep::<Arity256, Gapped, ValueStore>(depth);
        })
        .expect("spawn");
    handle.join().expect("no overflow");
}

#[test]
fn chain_value_recovery_over_every_store() {
    fn case<A, S, St>()
    where
        A: Arity,
        S: ChildStore<A>,
        St: EdgeStore<V, A, S, Hash = H> + Default,
        St::Error: Debug,
    {
        let mut store = St::default();
        let mut root = Some(chain::<A, S, St>(&mut store, 3, 42));
        assert_eq!(
            remove(&mut root, &mut store, &[idx::<A>(0); 3]).expect("store ok"),
            Some(42)
        );
        assert!(root.is_none());

        let mut parent: Node<V, St::Edge, A, S> = Node::new(Path::new(), None, S::Map::default());
        let chain_edge = {
            let c = chain::<A, S, St>(&mut store, 3, 42);
            store.inline(c)
        };
        parent.children_mut().insert(idx::<A>(0), chain_edge);
        let leaf = store.inline(Node::leaf(Path::new(), 99));
        parent.children_mut().insert(idx::<A>(1), leaf);
        let mut root = Some(parent);
        hash(root.as_mut().expect("root"), &mut store, &Fnv::PLAIN).expect("store ok");
        assert_eq!(
            remove(&mut root, &mut store, &[idx::<A>(0); 4]).expect("store ok"),
            Some(42)
        );
        assert_eq!(
            get(root.as_ref(), &store, &[idx::<A>(1)]).expect("store ok"),
            Some(99)
        );
        assert!(validate(root.as_ref(), &store).is_ok());
    }
    case::<Arity16, Packed, InMemory<H>>();
    case::<Arity16, Fixed, RcStore>();
    case::<Arity256, Gapped, ValueStore>();
}
