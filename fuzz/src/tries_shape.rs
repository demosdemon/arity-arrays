//! Harness for the `tries_shape_*` fuzz targets: arbitrary node shapes,
//! including deep chains and invariant violations, run through every entry
//! point.

use std::ops::ControlFlow;

use arbitrary::Arbitrary;
use arity_tries::Arity;
use arity_tries::ChildMap;
use arity_tries::ChildStore;
use arity_tries::EdgeStore;
use arity_tries::Node;
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

use super::tries_common::Fnv;
use super::tries_common::N;
use super::tries_common::Store;
use super::tries_common::contents;
use super::tries_common::idx;
use super::tries_common::path;

/// An arbitrary node shape, built through `Node::new` so it may violate the
/// structural invariant.
#[derive(Arbitrary, Debug)]
pub enum Shape {
    Leaf(Vec<u8>, Option<u32>),
    Branch(Vec<u8>, Option<u32>, Vec<(u8, Shape)>),
    /// A chain of valueless single-child nodes of the given depth (capped
    /// at 2048, since the test hasher's cost is quadratic in depth) ending
    /// at a leaf.
    Chain(u16, Option<u32>),
}

/// Builds `shape`: iteratively for a `Chain`, and by recursion over the
/// children of a `Branch`, whose nesting depth is bounded by the size of the
/// fuzz input.
fn build<A: Arity, S: ChildStore<A>>(shape: Shape, store: &mut Store) -> N<A, S> {
    match shape {
        Shape::Leaf(p, v) => Node::new(path::<A>(&p), v, S::Map::default()),
        Shape::Chain(depth, v) => {
            let mut node: N<A, S> = Node::new(Path::new(), v, S::Map::default());
            for _ in 0..depth % 2048 {
                let mut parent: N<A, S> = Node::new(Path::new(), None, S::Map::default());
                parent.children_mut().insert(idx::<A>(0), store.inline(node));
                node = parent;
            }
            node
        }
        Shape::Branch(p, v, children) => {
            let mut node: N<A, S> = Node::new(path::<A>(&p), v, S::Map::default());
            for (i, child) in children {
                let child = build::<A, S>(child, store);
                node.children_mut().insert(idx::<A>(i), store.inline(child));
            }
            node
        }
    }
}

/// Every entry point over an arbitrary shape: nothing may panic, and a hash
/// followed by `materialize_subtree` must leave the number of keys unchanged.
pub fn shape_run<A: Arity, S: ChildStore<A>>(shape: Shape, keys: Vec<Vec<u8>>) {
    let mut store = Store::default();
    let mut root: Option<N<A, S>> = Some(build::<A, S>(shape, &mut store));
    let before = contents(root.as_ref(), &store);
    let _ = validate(root.as_ref(), &store);
    let _ = visit(root.as_ref(), &store, None, |_, _| ControlFlow::Continue(()));
    for k in &keys {
        let k = path::<A>(k);
        let _ = get(root.as_ref(), &store, &k);
        let _ = iter(root.as_ref(), &store, Some(&k)).count();
    }
    hash(root.as_mut().unwrap(), &mut store, &Fnv).unwrap();
    // Hand-built shapes may violate the structural invariant, so a fresh
    // build cannot reproduce them and the hash is not compared; the key set
    // must survive sealing and unsealing, though.
    materialize_subtree(root.as_mut().unwrap(), &mut store).unwrap();
    assert_eq!(contents(root.as_ref(), &store).len(), before.len());
    for k in keys {
        let k = path::<A>(&k);
        let _ = insert(&mut root, &mut store, &k, 1).unwrap();
        let _ = remove(&mut root, &mut store, &k).unwrap();
        remove_prefix(&mut root, &mut store, &k[..k.len() / 2]).unwrap();
    }
    drop(root);
}
