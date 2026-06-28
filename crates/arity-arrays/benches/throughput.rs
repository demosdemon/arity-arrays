//! Throughput benchmarks for `PackedArray` against the alternatives a consumer
//! would otherwise reach for. The bench bodies build on the shared support
//! module, which the `bench_internals` integration test also includes and
//! unit-tests.

#[path = "support.rs"]
mod support;

use std::collections::BTreeMap;
use std::collections::HashMap;

use arity_arrays::Arity16;
use arity_arrays::Arity256;
use arity_arrays::FixedArray;
use arity_arrays::PackedArray;
use support::BenchContainer;
use support::BoxArr;
use support::Payload;

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
                bencher.bench_local(|| divan::black_box(c.fold()));
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

fn main() {
    divan::main();
}
