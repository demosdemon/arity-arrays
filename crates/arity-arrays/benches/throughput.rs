//! Throughput benchmarks for `PackedArray` against the alternatives a consumer
//! would otherwise reach for. The bench bodies build on the shared support
//! module, which the `bench_internals` integration test also includes and
//! unit-tests.

#[path = "support.rs"]
mod support;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hint::black_box;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use support::BenchContainer;
use support::BoxArr;
use support::ChurnOp;
use support::Payload;
use support::churn_ops;

// Cell A: Arity16 + 32-byte hash. Cell B: Arity256 + 8-byte pointer stand-in.
const OCC_A: &[usize] = &[1, 4, 8, 16];
const OCC_B: &[usize] = &[1, 16, 64, 128, 256];
// get_miss / insert_new need an absent slot, so they exclude the full point.
const OCC_A_PARTIAL: &[usize] = &[1, 4, 8];
const OCC_B_PARTIAL: &[usize] = &[1, 16, 64, 128];

// Present mid-rank slot: slots 0..occupancy are contiguous, so rank == slot and
// the mid-rank element is at slot occupancy/2.
const fn hit_index(occupancy: usize) -> usize {
    occupancy / 2
}
// First slot `fill` did not populate.
const fn miss_index(occupancy: usize) -> usize {
    occupancy
}

/// Registers the six single-op benches for one cell. `$cell` is the id-path
/// cell segment, `$ty` the payload, `$occ`/`$occ_partial` the occupancy slices,
/// and the trailing list the concrete container types swept.
macro_rules! single_op_benches {
    ($fn:ident, $cell:literal, $ty:ty, $occ:expr, $occ_partial:expr, [$($ctype:ty),+ $(,)?]) => {
        fn $fn(c: &mut Criterion) {
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/get_hit"));
                $( for &occ in $occ {
                    let cont = <$ctype as BenchContainer<$ty>>::fill(occ);
                    let target = hit_index(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter(|| black_box(cont.lookup(black_box(target)))),
                    );
                } )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/get_miss"));
                $( for &occ in $occ_partial {
                    let cont = <$ctype as BenchContainer<$ty>>::fill(occ);
                    let target = miss_index(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter(|| black_box(cont.lookup(black_box(target)))),
                    );
                } )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/insert_replace"));
                $( for &occ in $occ {
                    let target = hit_index(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter_batched(
                            || <$ctype as BenchContainer<$ty>>::fill(occ),
                            |mut cont| black_box(cont.set(target, <$ty as Payload>::make(target))),
                            BatchSize::SmallInput,
                        ),
                    );
                } )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/insert_new"));
                $( for &occ in $occ_partial {
                    let target = miss_index(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter_batched(
                            || <$ctype as BenchContainer<$ty>>::fill(occ),
                            |mut cont| black_box(cont.set(target, <$ty as Payload>::make(target))),
                            BatchSize::SmallInput,
                        ),
                    );
                } )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/remove"));
                $( for &occ in $occ {
                    let target = hit_index(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter_batched(
                            || <$ctype as BenchContainer<$ty>>::fill(occ),
                            |mut cont| black_box(cont.del(target)),
                            BatchSize::SmallInput,
                        ),
                    );
                } )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/iter_present"));
                $( for &occ in $occ {
                    let cont = <$ctype as BenchContainer<$ty>>::fill(occ);
                    g.bench_with_input(
                        BenchmarkId::new(<$ctype as BenchContainer<$ty>>::NAME, occ),
                        &occ,
                        |b, _| b.iter(|| black_box(black_box(&cont).fold())),
                    );
                } )+
                g.finish();
            }
        }
    };
}

single_op_benches!(
    single_cell_a, "cell_a", [u8; 32], OCC_A, OCC_A_PARTIAL,
    [
        PackedArray<[u8; 32], Arity16>,
        GappedArray<[u8; 32], Arity16>,
        FixedArray<Option<[u8; 32]>, Arity16>,
        BoxArr<[u8; 32], Arity16>,
        BTreeMap<usize, [u8; 32]>,
        HashMap<usize, [u8; 32]>,
    ]
);

single_op_benches!(
    single_cell_b, "cell_b", u64, OCC_B, OCC_B_PARTIAL,
    [
        PackedArray<u64, Arity256>,
        GappedArray<u64, Arity256>,
        FixedArray<Option<u64>, Arity256>,
        BoxArr<u64, Arity256>,
        BTreeMap<usize, u64>,
        HashMap<usize, u64>,
    ]
);

/// Registers `build` and `churn` for one cell under `throughput/<cell>/<op>`,
/// with no occupancy parameter (the macro sweeps the full arity).
macro_rules! workload_benches {
    ($fn:ident, $cell:literal, $ty:ty, $arity:ty, [$($ctype:ty),+ $(,)?]) => {
        fn $fn(c: &mut Criterion) {
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/build"));
                let n = <$arity as Arity>::LEN;
                $(
                    // bench_function (no parameter) so the id is the exact
                    // four-segment `throughput/<cell>/build/<NAME>`. Using
                    // BenchmarkId::new(NAME, "") would append an empty segment.
                    g.bench_function(<$ctype as BenchContainer<$ty>>::NAME, |b| {
                        b.iter_with_large_drop(|| {
                            let mut cont = <$ctype as BenchContainer<$ty>>::empty();
                            for i in 0..n {
                                cont.set(i, <$ty as Payload>::make(i));
                            }
                            cont
                        })
                    });
                )+
                g.finish();
            }
            {
                let mut g = c.benchmark_group(concat!("throughput/", $cell, "/churn"));
                let ops = churn_ops::<$arity>();
                let half = <$arity as Arity>::LEN / 2;
                $(
                    g.bench_function(<$ctype as BenchContainer<$ty>>::NAME, |b| {
                        b.iter_batched(
                            || <$ctype as BenchContainer<$ty>>::fill(half),
                            |mut cont| {
                                for &(op, slot) in &ops {
                                    match op {
                                        ChurnOp::Remove => { black_box(cont.del(slot)); }
                                        ChurnOp::Insert => {
                                            black_box(cont.set(slot, <$ty as Payload>::make(slot)));
                                        }
                                    }
                                }
                                cont
                            },
                            BatchSize::SmallInput,
                        )
                    });
                )+
                g.finish();
            }
        }
    };
}

workload_benches!(
    workload_cell_a, "cell_a", [u8; 32], Arity16,
    [
        PackedArray<[u8; 32], Arity16>,
        GappedArray<[u8; 32], Arity16>,
        FixedArray<Option<[u8; 32]>, Arity16>,
        BoxArr<[u8; 32], Arity16>,
        BTreeMap<usize, [u8; 32]>,
        HashMap<usize, [u8; 32]>,
    ]
);

workload_benches!(
    workload_cell_b, "cell_b", u64, Arity256,
    [
        PackedArray<u64, Arity256>,
        GappedArray<u64, Arity256>,
        FixedArray<Option<u64>, Arity256>,
        BoxArr<u64, Arity256>,
        BTreeMap<usize, u64>,
        HashMap<usize, u64>,
    ]
);

/// pack/unpack between a populated `FixedArray` and a `PackedArray`, swept by
/// occupancy per cell, under `throughput/convert/<op>`.
fn convert(c: &mut Criterion) {
    {
        let mut g = c.benchmark_group("throughput/convert/pack");
        for &occ in OCC_A {
            let src =
                <FixedArray<Option<[u8; 32]>, Arity16> as BenchContainer<[u8; 32]>>::fill(occ);
            g.bench_with_input(BenchmarkId::new("cell_a", occ), &occ, |b, _| {
                b.iter_with_large_drop(|| PackedArray::from(black_box(&src)));
            });
        }
        for &occ in OCC_B {
            let src = <FixedArray<Option<u64>, Arity256> as BenchContainer<u64>>::fill(occ);
            g.bench_with_input(BenchmarkId::new("cell_b", occ), &occ, |b, _| {
                b.iter_with_large_drop(|| PackedArray::from(black_box(&src)));
            });
        }
        g.finish();
    }
    {
        let mut g = c.benchmark_group("throughput/convert/unpack");
        for &occ in OCC_A {
            let src = <PackedArray<[u8; 32], Arity16> as BenchContainer<[u8; 32]>>::fill(occ);
            g.bench_with_input(BenchmarkId::new("cell_a", occ), &occ, |b, _| {
                b.iter_with_large_drop(|| {
                    FixedArray::<Option<[u8; 32]>, Arity16>::from(black_box(&src))
                });
            });
        }
        for &occ in OCC_B {
            let src = <PackedArray<u64, Arity256> as BenchContainer<u64>>::fill(occ);
            g.bench_with_input(BenchmarkId::new("cell_b", occ), &occ, |b, _| {
                b.iter_with_large_drop(|| {
                    FixedArray::<Option<u64>, Arity256>::from(black_box(&src))
                });
            });
        }
        g.finish();
    }
}

// `BENCH_QUICK=1` shrinks sample size/timing for a fast CI comparison.
// cargo-criterion does not forward `--quick` (or any other criterion CLI
// flag) to the harness the way plain `cargo bench` does, so this has to be
// read directly rather than via `Criterion::configure_from_args`. `trie.rs`
// carries an identical copy of this helper for the same reason.
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
    } else {
        c
    }
}

criterion_group!(
    name = benches;
    config = quick_criterion();
    targets = single_cell_a, single_cell_b, workload_cell_a, workload_cell_b, convert
);
criterion_main!(benches);
