//! `NodeRef` and `Iter` dropped at every possible point over the store with
//! owning handles, the case a front-to-back drop of the handle chain breaks.

mod common;

use arity_arrays::Arity16;
use arity_tries::Packed;
use arity_tries::get_node;
use arity_tries::hash;
use arity_tries::insert;
use arity_tries::iter;
use common::*;

const KEYS: &[(&[u8], V)] = &[
    (&[1, 2, 3], 3),
    (&[1, 2, 4], 4),
    (&[1, 2, 4, 5], 5),
    (&[1, 2, 4, 5, 6, 7], 6),
    (&[1, 7], 7),
    (&[8], 8),
    (&[8, 0, 0, 0], 800),
];

fn build() -> Trie<Arity16, Packed, RcStore> {
    let (mut root, mut store) = (None, RcStore::default());
    for (k, v) in KEYS {
        insert(&mut root, &mut store, &path::<Arity16>(k), *v).expect("store ok");
    }
    // Seal everything so every edge below the root is `RcEdge::Sealed` and
    // every handle read through one is an owned `Rc`.
    hash(root.as_mut().expect("root"), &mut store, &Fnv::PLAIN).expect("store ok");
    (root, store)
}

#[test]
fn node_ref_dropped_after_use_at_every_key() {
    let (root, store) = build();
    for (k, v) in KEYS {
        let found = get_node(root.as_ref(), &store, &path::<Arity16>(k))
            .expect("store ok")
            .expect("present");
        assert_eq!(found.value(), Some(v));
        drop(found);
    }
    let unused = get_node(root.as_ref(), &store, &path::<Arity16>(&[1, 2, 4, 5, 6, 7]))
        .expect("store ok")
        .expect("present");
    drop(unused);
}

#[test]
fn iter_dropped_after_k_items_for_every_k() {
    let (root, store) = build();
    for k in 0..=KEYS.len() + 1 {
        let mut it = iter(root.as_ref(), &store, None);
        for _ in 0..k {
            if it.next().is_none() {
                break;
            }
        }
        drop(it);
    }
    for (start, _) in KEYS {
        let mut it = iter(root.as_ref(), &store, Some(&path::<Arity16>(start)));
        let first = it.next();
        drop(it);
        assert!(first.is_some());
    }
}
