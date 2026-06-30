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
use fixture::Shape;
use fixture::Trie;
use fixture::build;
use fixture::expected_node_count;
use fixture::key_depth;

/// A childless node for the store `S` over arity `A`.
fn leaf<A: Arity, S: ChildStore<A>>() -> Trie<A, S> {
    Trie {
        path: Box::new([]),
        value: None,
        children: ChildMap::empty(),
    }
}

/// Insert one `Mutable` child at index 0, give that child its own `Mutable`
/// grandchild at index 0, then clone the root and verify the full two-level
/// tree is present and independent in the clone.
fn roundtrip<A: Arity, S: ChildStore<A>>() {
    let mut root: Trie<A, S> = leaf();
    let i0 = <A::Index as Niche>::try_from_usize(0).expect("0 < LEN");

    // Build a two-level tree: root → child → grandchild.
    let mut child: Trie<A, S> = leaf();
    ChildMap::insert(&mut child.children, i0, Edge::Mutable(Box::new(leaf())));
    ChildMap::insert(&mut root.children, i0, Edge::Mutable(Box::new(child)));

    assert!(matches!(
        ChildMap::get(&root.children, i0),
        Some(Edge::Mutable(_))
    ));

    let clone = root.clone();

    // Root-level child is present in the clone.
    assert!(matches!(
        ChildMap::get(&clone.children, i0),
        Some(Edge::Mutable(_))
    ));

    // Grandchild is present in the clone (exercises recursive clone at depth 2).
    let grandchild_present = match ChildMap::get(&clone.children, i0) {
        Some(Edge::Mutable(c)) => matches!(ChildMap::get(&c.children, i0), Some(Edge::Mutable(_))),
        _ => false,
    };
    assert!(grandchild_present);

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

/// Count nodes store-agnostically: a node plus its present `Mutable` children.
/// Fixtures are all-`Mutable`, so `Frozen` never appears.
fn count_nodes<A: Arity, S: ChildStore<A>>(t: &Trie<A, S>) -> usize {
    let mut n = 1;
    for i in <A::Index as Niche>::all() {
        if let Some(Edge::Mutable(child)) = ChildMap::get(&t.children, i) {
            n += count_nodes::<A, S>(child);
        }
    }
    n
}

fn check_count<A: Arity, S: ChildStore<A>>(shape: Shape) {
    assert_eq!(
        count_nodes(&build::<A, S>(shape)),
        expected_node_count::<A>(shape),
        "node count mismatch",
    );
}

#[test]
fn node_counts_match_across_stores() {
    for shape in [Shape::Chain, Shape::Bushy, Shape::Realistic] {
        check_count::<Arity16, GappedStore>(shape);
        check_count::<Arity16, PackedStore>(shape);
        check_count::<Arity16, FixedStore>(shape);
        check_count::<Arity16, BTreeStore>(shape);
        check_count::<Arity256, GappedStore>(shape);
        check_count::<Arity256, PackedStore>(shape);
        check_count::<Arity256, FixedStore>(shape);
        check_count::<Arity256, BTreeStore>(shape);
    }
}

#[test]
fn chain_depth_is_key_depth() {
    // A Chain is a single path: its node count equals its depth.
    assert_eq!(
        count_nodes(&build::<Arity16, GappedStore>(Shape::Chain)),
        key_depth::<Arity16>()
    );
    assert_eq!(
        count_nodes(&build::<Arity256, GappedStore>(Shape::Chain)),
        key_depth::<Arity256>()
    );
}

fn check_clone_independent<A: Arity, S: ChildStore<A>>(shape: Shape)
where
    <S as ChildStore<A>>::Map<Edge<A, S>>: Clone,
{
    let original = build::<A, S>(shape);
    let expected = count_nodes(&original);
    let clone = original.clone();
    assert_eq!(count_nodes(&clone), expected);
    drop(original); // recursive Drop through this store (Miri checks for UB/leaks)
    assert_eq!(
        count_nodes(&clone),
        expected,
        "clone must survive the original's drop"
    );
}

#[test]
fn clone_is_independent_across_stores() {
    // Bushy gives several children per node, exercising per-array Clone/Drop.
    check_clone_independent::<Arity16, GappedStore>(Shape::Bushy);
    check_clone_independent::<Arity16, PackedStore>(Shape::Bushy);
    check_clone_independent::<Arity16, FixedStore>(Shape::Bushy);
    check_clone_independent::<Arity16, BTreeStore>(Shape::Bushy);
    check_clone_independent::<Arity256, GappedStore>(Shape::Realistic);
    check_clone_independent::<Arity256, PackedStore>(Shape::Realistic);
    check_clone_independent::<Arity256, FixedStore>(Shape::Realistic);
    check_clone_independent::<Arity256, BTreeStore>(Shape::Realistic);
}
