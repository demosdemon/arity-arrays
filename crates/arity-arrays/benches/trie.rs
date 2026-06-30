//! Recursive `Clone`/`Drop` throughput for a compressed-trie fixture whose
//! children array is each of `GappedArray`, `PackedArray`, `FixedArray`, and a
//! `BTreeMap` baseline. Unlike the `throughput` suite (POD payloads), the
//! `Edge` payload (`Box`/`Arc`/`Box<[u8]>`) makes every node's children-array
//! clone/drop do real recursive work, so `FixedArray` (which pays for all
//! `A::LEN` slots) is contrasted with the live-count-proportional reps.

#[path = "trie_fixture.rs"]
mod fixture;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use fixture::BTreeStore;
use fixture::ChildStore;
use fixture::FixedStore;
use fixture::GappedStore;
use fixture::PackedStore;
use fixture::Shape;
use fixture::Trie;
use fixture::build;

const SHAPES: &[Shape] = &[Shape::Chain, Shape::Bushy, Shape::Realistic];

/// Lets the generic divan functions construct the right fixture from the
/// concrete `Trie<A, S>` type each `types = [...]` entry monomorphizes to.
trait TrieBench: Clone {
    fn make(shape: Shape) -> Self;
}

impl<A: Arity, S: ChildStore<A>> TrieBench for Trie<A, S> {
    fn make(shape: Shape) -> Self {
        build::<A, S>(shape)
    }
}

/// Stamps the `clone` and `drop` benches for one arity's four stores.
/// The types list is passed as `[$($ctype:ty),+]` so rustfmt sees an unexpanded
/// token tree and does not re-indent the list on each formatting pass.
macro_rules! trie_cell {
    ($modname:ident, [$($ctype:ty),+ $(,)?]) => {
        mod $modname {
            use super::*;

            #[divan::bench(types = [$($ctype),+], args = SHAPES)]
            fn clone<C: TrieBench>(bencher: divan::Bencher, shape: Shape) {
                let tree = C::make(shape);
                // divan defers the returned clone's drop past `sample_end`, so
                // only the clone is timed.
                bencher.bench_local(|| divan::black_box(divan::black_box(&tree).clone()));
            }

            #[divan::bench(types = [$($ctype),+], args = SHAPES)]
            fn drop<C: TrieBench>(bencher: divan::Bencher, shape: Shape) {
                // build untimed; time the recursive drop of the moved-in value.
                bencher
                    .with_inputs(|| C::make(shape))
                    .bench_local_values(::core::mem::drop);
            }
        }
    };
}

trie_cell!(arity16, [
    Trie<Arity16, GappedStore>,
    Trie<Arity16, PackedStore>,
    Trie<Arity16, FixedStore>,
    Trie<Arity16, BTreeStore>,
]);
trie_cell!(arity256, [
    Trie<Arity256, GappedStore>,
    Trie<Arity256, PackedStore>,
    Trie<Arity256, FixedStore>,
    Trie<Arity256, BTreeStore>,
]);

fn main() {
    divan::main();
}
