//! Correctness tests for the bench-only trie fixture
//! (`benches/trie_fixture.rs`), which the `trie` benchmark uses with `harness =
//! false` and so cannot self-test under `cargo test`. Included here via
//! `#[path]`, mirroring how `bench_internals.rs` includes `support.rs`. No
//! file-level `cfg(not(miri))` gate: the drop/clone tests run under Miri at the
//! cfg(miri)-reduced sizes.

#[path = "../benches/trie_fixture.rs"]
mod fixture;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::index::Niche;
use fixture::BTreeStore;
use fixture::ChildMap;
use fixture::ChildStore;
use fixture::Edge;
use fixture::FixedStore;
use fixture::GappedStore;
use fixture::PackedStore;
use fixture::Trie;

/// A childless node for the store `S` over arity `A`.
fn leaf<A: Arity, S: ChildStore<A>>() -> Trie<A, S> {
    Trie {
        path: Box::new([]),
        value: None,
        children: ChildMap::empty(),
    }
}

/// Insert one `Mutable` leaf at index 0, read it back, clone, and drop — for a
/// single store. Generic so every store exercises the GAT and the `Clone`
/// bound.
fn roundtrip<A: Arity, S: ChildStore<A>>()
where
    <S as ChildStore<A>>::Map<Edge<A, S>>: Clone,
{
    let mut root: Trie<A, S> = leaf();
    let i0 = <A::Index as Niche>::try_from_usize(0).expect("0 < LEN");
    ChildMap::insert(&mut root.children, i0, Edge::Mutable(Box::new(leaf())));
    assert!(matches!(
        ChildMap::get(&root.children, i0),
        Some(Edge::Mutable(_))
    ));
    let clone = root.clone();
    assert!(matches!(
        ChildMap::get(&clone.children, i0),
        Some(Edge::Mutable(_))
    ));
    drop(root);
    // The clone is independent: it survives the original's drop.
    assert!(matches!(
        ChildMap::get(&clone.children, i0),
        Some(Edge::Mutable(_))
    ));
}

#[test]
fn childmap_roundtrip_and_clone_all_stores() {
    roundtrip::<Arity16, GappedStore>();
    roundtrip::<Arity16, PackedStore>();
    roundtrip::<Arity16, FixedStore>();
    roundtrip::<Arity16, BTreeStore>();
    roundtrip::<Arity256, GappedStore>();
    roundtrip::<Arity256, PackedStore>();
    roundtrip::<Arity256, FixedStore>();
    roundtrip::<Arity256, BTreeStore>();
}
