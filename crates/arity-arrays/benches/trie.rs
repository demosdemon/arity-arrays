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

// `BENCH_QUICK=1` shrinks sample size/timing for a fast CI comparison.
// cargo-criterion does not forward `--quick` (or any other criterion CLI
// flag) to the harness the way plain `cargo bench` does, so this has to be
// read directly rather than via `Criterion::configure_from_args`.
// `throughput.rs` carries an identical copy of this helper for the same
// reason.
//
// `nresamples` (bootstrap resamples for the confidence-interval analysis
// that follows each point's measurement) dominates per-point wall time far
// more than `sample_size`/`measurement_time` do: `sample_size(10)` is
// already criterion's enforced floor, but the default `nresamples` of
// 100_000 still ran a multi-second bootstrap after every point, which is
// what actually timed out CI. 1_000 is criterion's own documented minimum
// before it warns.
//
// This must feed `criterion_group!`'s `config = ...` (the long form below),
// not just a `Criterion` built in `main`: the short form `criterion_group!(
// benches, ...)` expands to a `benches()` function that constructs its own
// internal `Criterion::default().configure_from_args()` and passes that to
// every benchmark body — a `Criterion` built in `main` and passed only to
// `.final_summary()` afterward never actually reaches the benchmarks.
fn quick_criterion() -> Criterion {
    let c = Criterion::default();
    if std::env::var_os("BENCH_QUICK").is_some() {
        c.sample_size(10)
            .warm_up_time(std::time::Duration::from_millis(100))
            .measurement_time(std::time::Duration::from_millis(500))
            .nresamples(1000)
    } else {
        c
    }
}

criterion_group!(
    name = benches;
    config = quick_criterion();
    targets = arity16, arity256
);

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
            quick_criterion().configure_from_args().final_summary();
        })
        .expect("spawn bench harness thread")
        .join()
        .expect("bench harness thread panicked");
}
