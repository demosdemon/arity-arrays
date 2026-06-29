//! Throughput benchmarks for `PackedArray` against the alternatives a consumer
//! would otherwise reach for. The bench bodies build on the shared support
//! module, which the `bench_internals` integration test also includes and
//! unit-tests.

#[path = "support.rs"]
mod support;

use std::collections::BTreeMap;
use std::collections::HashMap;

use arity_arrays::Arity;
use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::FixedArray;
use arity_arrays::PackedArray;
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

/// Generates the single-op bench module for one cell. `$ty` is the payload,
/// `$arity` its arity, `$occ`/`$occ_partial` the sample-point slices, and the
/// trailing list the concrete container types swept by divan.
macro_rules! single_op_benches {
    ($modname:ident, $ty:ty, $arity:ty, $occ:expr, $occ_partial:expr, [$($ctype:ty),+ $(,)?]) => {
        mod $modname {
            use super::*;

            // Present mid-rank slot: slots 0..occupancy are contiguous, so rank
            // == slot and the mid-rank element is at slot occupancy/2.
            const fn hit_index(occupancy: usize) -> usize {
                occupancy / 2
            }
            // First slot `fill` did not populate.
            const fn miss_index(occupancy: usize) -> usize {
                occupancy
            }

            #[divan::bench(types = [$($ctype),+], args = $occ)]
            fn get_hit<C: BenchContainer<$ty>>(bencher: divan::Bencher, occupancy: usize) {
                let c = C::fill(occupancy);
                let target = hit_index(occupancy);
                bencher.bench_local(|| divan::black_box(c.lookup(divan::black_box(target))));
            }

            #[divan::bench(types = [$($ctype),+], args = $occ_partial)]
            fn get_miss<C: BenchContainer<$ty>>(bencher: divan::Bencher, occupancy: usize) {
                let c = C::fill(occupancy);
                let target = miss_index(occupancy);
                bencher.bench_local(|| divan::black_box(c.lookup(divan::black_box(target))));
            }

            #[divan::bench(types = [$($ctype),+], args = $occ)]
            fn insert_replace<C: BenchContainer<$ty>>(
                bencher: divan::Bencher,
                occupancy: usize,
            ) {
                let target = hit_index(occupancy);
                bencher
                    .with_inputs(|| C::fill(occupancy))
                    .bench_local_values(|mut c| {
                        divan::black_box(c.set(target, <$ty as Payload>::make(target)))
                    });
            }

            #[divan::bench(types = [$($ctype),+], args = $occ_partial)]
            fn insert_new<C: BenchContainer<$ty>>(bencher: divan::Bencher, occupancy: usize) {
                let target = miss_index(occupancy);
                bencher
                    .with_inputs(|| C::fill(occupancy))
                    .bench_local_values(|mut c| {
                        divan::black_box(c.set(target, <$ty as Payload>::make(target)))
                    });
            }

            #[divan::bench(types = [$($ctype),+], args = $occ)]
            fn remove<C: BenchContainer<$ty>>(bencher: divan::Bencher, occupancy: usize) {
                let target = hit_index(occupancy);
                bencher
                    .with_inputs(|| C::fill(occupancy))
                    .bench_local_values(|mut c| divan::black_box(c.del(target)));
            }

            #[divan::bench(types = [$($ctype),+], args = $occ)]
            fn iter_present<C: BenchContainer<$ty>>(bencher: divan::Bencher, occupancy: usize) {
                let c = C::fill(occupancy);
                bencher.bench_local(|| divan::black_box(divan::black_box(&c).fold()));
            }
        }
    };
}

single_op_benches!(
    cell_a, [u8; 32], Arity16, OCC_A, OCC_A_PARTIAL,
    [
        PackedArray<[u8; 32], Arity16>,
        FixedArray<Option<[u8; 32]>, Arity16>,
        BoxArr<[u8; 32], Arity16>,
        BTreeMap<usize, [u8; 32]>,
        HashMap<usize, [u8; 32]>,
    ]
);

single_op_benches!(
    cell_b, u64, Arity256, OCC_B, OCC_B_PARTIAL,
    [
        PackedArray<u64, Arity256>,
        FixedArray<Option<u64>, Arity256>,
        BoxArr<u64, Arity256>,
        BTreeMap<usize, u64>,
        HashMap<usize, u64>,
    ]
);

mod convert {
    use super::Arity16;
    use super::Arity256;
    use super::BenchContainer;
    use super::FixedArray;
    use super::OCC_A;
    use super::OCC_B;
    use super::PackedArray;

    // pack: clone a populated FixedArray into a PackedArray (From<&FixedArray>).
    #[divan::bench(args = OCC_A)]
    fn pack_cell_a(bencher: divan::Bencher, occupancy: usize) {
        let src =
            <FixedArray<Option<[u8; 32]>, Arity16> as BenchContainer<[u8; 32]>>::fill(occupancy);
        bencher.bench_local(|| divan::black_box(PackedArray::from(divan::black_box(&src))));
    }

    #[divan::bench(args = OCC_B)]
    fn pack_cell_b(bencher: divan::Bencher, occupancy: usize) {
        let src = <FixedArray<Option<u64>, Arity256> as BenchContainer<u64>>::fill(occupancy);
        bencher.bench_local(|| divan::black_box(PackedArray::from(divan::black_box(&src))));
    }

    // unpack: clone a populated PackedArray back into a FixedArray
    // (From<&PackedArray>).
    #[divan::bench(args = OCC_A)]
    fn unpack_cell_a(bencher: divan::Bencher, occupancy: usize) {
        let src = <PackedArray<[u8; 32], Arity16> as BenchContainer<[u8; 32]>>::fill(occupancy);
        bencher.bench_local(|| {
            divan::black_box(FixedArray::<Option<[u8; 32]>, Arity16>::from(
                divan::black_box(&src),
            ))
        });
    }

    #[divan::bench(args = OCC_B)]
    fn unpack_cell_b(bencher: divan::Bencher, occupancy: usize) {
        let src = <PackedArray<u64, Arity256> as BenchContainer<u64>>::fill(occupancy);
        bencher.bench_local(|| {
            divan::black_box(FixedArray::<Option<u64>, Arity256>::from(divan::black_box(
                &src,
            )))
        });
    }
}

macro_rules! workload_benches {
    ($modname:ident, $ty:ty, $arity:ty, [$($ctype:ty),+ $(,)?]) => {
        mod $modname {
            use super::*;

            // build: N successive inserts from empty (burst-insert).
            #[divan::bench(types = [$($ctype),+])]
            fn build<C: BenchContainer<$ty>>(bencher: divan::Bencher) {
                let n = <$arity as Arity>::LEN;
                bencher.bench_local(|| {
                    let mut c = C::empty();
                    for i in 0..n {
                        c.set(i, <$ty as Payload>::make(i));
                    }
                    divan::black_box(c)
                });
            }

            // churn: hold ~half occupancy through max(256, 8N) alternating ops.
            #[divan::bench(types = [$($ctype),+])]
            fn churn<C: BenchContainer<$ty>>(bencher: divan::Bencher) {
                let ops = churn_ops::<$arity>();
                let half = <$arity as Arity>::LEN / 2;
                bencher
                    .with_inputs(|| C::fill(half))
                    .bench_local_values(|mut c| {
                        for &(op, slot) in &ops {
                            match op {
                                ChurnOp::Remove => {
                                    divan::black_box(c.del(slot));
                                }
                                ChurnOp::Insert => {
                                    divan::black_box(c.set(slot, <$ty as Payload>::make(slot)));
                                }
                            }
                        }
                        divan::black_box(c)
                    });
            }
        }
    };
}

workload_benches!(
    workload_a, [u8; 32], Arity16,
    [
        PackedArray<[u8; 32], Arity16>,
        FixedArray<Option<[u8; 32]>, Arity16>,
        BoxArr<[u8; 32], Arity16>,
        BTreeMap<usize, [u8; 32]>,
        HashMap<usize, [u8; 32]>,
    ]
);

workload_benches!(
    workload_b, u64, Arity256,
    [
        PackedArray<u64, Arity256>,
        FixedArray<Option<u64>, Arity256>,
        BoxArr<u64, Arity256>,
        BTreeMap<usize, u64>,
        HashMap<usize, u64>,
    ]
);

fn main() {
    divan::main();
}
