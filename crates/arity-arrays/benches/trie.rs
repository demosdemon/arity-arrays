//! Recursive `Clone`/`Drop` throughput for a compressed-trie fixture whose
//! children array is each of `GappedArray`, `PackedArray`, `FixedArray`, and a
//! `BTreeMap` baseline. Unlike the `throughput` suite (POD payloads), the
//! `Edge` payload (`Box`/`Arc`/`Box<[u8]>`) makes every node's children-array
//! clone/drop do real recursive work, so `FixedArray` (which pays for all
//! `A::LEN` slots) is contrasted with the live-count-proportional reps.

#[path = "trie_fixture.rs"]
mod fixture;

use std::hint::black_box;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use fixture::BTreeStore;
use fixture::ChildStore;
use fixture::FixedStore;
use fixture::GappedStore;
use fixture::PackedStore;
use fixture::Shape;
use fixture::Trie;
use fixture::build;

const SHAPES: &[Shape] = &[Shape::Chain, Shape::Bushy, Shape::Realistic];

/// Constructs the fixture for a concrete `Trie<A, S>` and exposes the store's
/// stable label for the `BenchmarkId`.
trait TrieBench: Clone {
    const STORE: &'static str;
    fn make(shape: Shape) -> Self;
}

impl<A: Arity, S: ChildStore<A>> TrieBench for Trie<A, S> {
    const STORE: &'static str = <S as ChildStore<A>>::NAME;
    fn make(shape: Shape) -> Self {
        build::<A, S>(shape)
    }
}

/// Registers the `clone` and `drop` benches for one arity's four stores under
/// `trie/<arity>/<op>`.
macro_rules! trie_cell {
    ($fn:ident, $arity:literal, [$($ctype:ty),+ $(,)?]) => {
        fn $fn(c: &mut Criterion) {
            {
                let mut g = c.benchmark_group(concat!("trie/", $arity, "/clone"));
                $(
                    for &shape in SHAPES {
                        let tree = <$ctype as TrieBench>::make(shape);
                        g.bench_with_input(
                            BenchmarkId::new(<$ctype as TrieBench>::STORE, shape),
                            &shape,
                            |b, _| {
                                // PerIteration: one cloned tree live at a time,
                                // dropped outside the timing window, so only the
                                // recursive clone is measured (not the drop).
                                b.iter_batched(
                                    || (),
                                    |()| black_box(black_box(&tree).clone()),
                                    BatchSize::PerIteration,
                                );
                            },
                        );
                    }
                )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("trie/", $arity, "/drop"));
                $(
                    for &shape in SHAPES {
                        g.bench_with_input(
                            BenchmarkId::new(<$ctype as TrieBench>::STORE, shape),
                            &shape,
                            |b, &shape| {
                                // PerIteration: build one large tree per iter
                                // (untimed setup), time its recursive drop.
                                b.iter_batched(
                                    || <$ctype as TrieBench>::make(shape),
                                    ::core::mem::drop,
                                    BatchSize::PerIteration,
                                );
                            },
                        );
                    }
                )+
                g.finish();
            }
        }
    };
}

trie_cell!(arity16, "arity16", [
    Trie<Arity16, GappedStore>,
    Trie<Arity16, PackedStore>,
    Trie<Arity16, FixedStore>,
    Trie<Arity16, BTreeStore>,
]);
trie_cell!(arity256, "arity256", [
    Trie<Arity256, GappedStore>,
    Trie<Arity256, PackedStore>,
    Trie<Arity256, FixedStore>,
    Trie<Arity256, BTreeStore>,
]);

criterion_group!(benches, arity16, arity256);

fn main() {
    // Equivalent to `criterion_main!(benches)`, but run on a thread with an
    // ample stack. The `Chain` fixture recurses to `key_depth` (128 levels for
    // Arity16, 64 for Arity256), and `Trie::clone` returns each node by value —
    // a `FixedStore` + `Arity256` node carries its children array inline
    // (~12 KiB), so the recursive clone needs well over 2 MiB of stack. That
    // exceeds Windows' ~1 MiB default main-thread stack in debug builds (e.g.
    // when `cargo test` runs this bench in test mode), aborting with a stack
    // overflow; Linux and macOS (8 MiB main stack) survive. criterion runs the
    // benched routine on the calling thread, so it inherits this stack.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            benches();
            Criterion::default().configure_from_args().final_summary();
        })
        .expect("spawn bench harness thread")
        .join()
        .expect("bench harness thread panicked");
}
