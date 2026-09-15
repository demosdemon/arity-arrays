//! Harness for the `tries_ops_*` fuzz targets: random operation sequences
//! against a `BTreeMap` oracle, with the structural invariant and the model
//! checked after every step.

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use arity_tries::Arity;
use arity_tries::ChildStore;
use arity_tries::get;
use arity_tries::hash;
use arity_tries::insert;
use arity_tries::iter;
use arity_tries::remove;
use arity_tries::remove_prefix;
use arity_tries::validate;

use super::tries_common::Fnv;
use super::tries_common::N;
use super::tries_common::Store;
use super::tries_common::contents;
use super::tries_common::path;

#[derive(Arbitrary, Debug)]
pub enum Op {
    Insert(Vec<u8>, u32),
    Remove(Vec<u8>),
    RemovePrefix(Vec<u8>),
    Get(Vec<u8>),
    IterFrom(Vec<u8>),
    Hash,
}

/// Runs `ops`. A `Hash` step additionally re-derives the oracle from the
/// trie, because the hasher rewrites values at depth two.
pub fn ops_run<A: Arity, S: ChildStore<A>>(ops: Vec<Op>) {
    let mut root: Option<N<A, S>> = None;
    let mut store = Store::default();
    let mut oracle: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
    for op in ops {
        match op {
            Op::Insert(k, v) => {
                let k: Vec<u8> = path::<A>(&k).as_bytes().to_vec();
                let prev = insert(&mut root, &mut store, &path::<A>(&k), v).unwrap();
                assert_eq!(prev, oracle.insert(k, v));
            }
            Op::Remove(k) => {
                let k: Vec<u8> = path::<A>(&k).as_bytes().to_vec();
                assert_eq!(remove(&mut root, &mut store, &path::<A>(&k)).unwrap(), oracle.remove(&k));
            }
            Op::RemovePrefix(p) => {
                let p: Vec<u8> = path::<A>(&p).as_bytes().to_vec();
                remove_prefix(&mut root, &mut store, &path::<A>(&p)).unwrap();
                oracle.retain(|k, _| !k.starts_with(&p));
            }
            Op::Get(k) => {
                let k: Vec<u8> = path::<A>(&k).as_bytes().to_vec();
                assert_eq!(get(root.as_ref(), &store, &path::<A>(&k)).unwrap(), oracle.get(&k).copied());
            }
            Op::IterFrom(s) => {
                let s: Vec<u8> = path::<A>(&s).as_bytes().to_vec();
                let live: Vec<(Vec<u8>, u32)> = iter(root.as_ref(), &store, Some(&path::<A>(&s)))
                    .map(|item| {
                        let (p, v) = item.unwrap();
                        (p.as_bytes().to_vec(), v)
                    })
                    .collect();
                let expected: Vec<(Vec<u8>, u32)> = oracle.range(s..).map(|(k, v)| (k.clone(), *v)).collect();
                assert_eq!(live, expected);
            }
            Op::Hash => {
                if let Some(r) = root.as_mut() {
                    let live = hash(r, &mut store, &Fnv).unwrap();
                    // The hasher rewrites values at depth two: rebuild from
                    // the live contents, which a fresh build then reproduces.
                    let mut fresh_root: Option<N<A, S>> = None;
                    let mut fresh_store = Store::default();
                    for (k, v) in contents(root.as_ref(), &store) {
                        insert(&mut fresh_root, &mut fresh_store, &path::<A>(&k), v).unwrap();
                    }
                    let fresh = hash(fresh_root.as_mut().unwrap(), &mut fresh_store, &Fnv).unwrap();
                    assert_eq!(live, fresh);
                    oracle = contents(root.as_ref(), &store).into_iter().collect();
                }
            }
        }
        validate(root.as_ref(), &store).unwrap();
        let expected: Vec<(Vec<u8>, u32)> = oracle.iter().map(|(k, v)| (k.clone(), *v)).collect();
        assert_eq!(contents(root.as_ref(), &store), expected);
    }
}

