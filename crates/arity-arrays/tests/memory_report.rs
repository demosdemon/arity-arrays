//! Computes the exact in-memory footprint of the layout-deterministic
//! representations across occupancy and locks the table as a snapshot. The
//! `.snap` is the human-readable baseline copied into the READMEs and a
//! layout-change tripwire. Numbers are exact and machine-independent.
//!
//! Maps are excluded: their heap use is implementation-defined node/bucket
//! overhead, not an analytic function of occupancy.
#![cfg(not(miri))]

use core::fmt::Write;
use core::mem::size_of;

use arity_arrays::Arity;
use arity_arrays::FixedArray;
use arity_arrays::GappedArray;
use arity_arrays::PackedArray;
use arity_arrays::index::Niche;

/// Build a `PackedArray` with `occupancy` contiguous slots present and report
/// its total bytes: the pointer-sized handle plus the live heap block.
fn packed_bytes<T, A: Arity>(occupancy: usize, make: impl Fn(usize) -> T) -> usize {
    let mut p = PackedArray::<T, A>::new();
    for i in 0..occupancy {
        let idx = <A::Index as Niche>::try_from_usize(i).expect("i < occupancy <= LEN");
        p.insert(idx, make(i));
    }
    size_of::<PackedArray<T, A>>() + p.allocated_size()
}

/// Total bytes of a `Box<[Option<T>]>` of length `n`: the fat-pointer handle
/// plus `n` elements on the heap. Independent of occupancy.
const fn boxed_bytes<T, A: Arity>() -> usize {
    size_of::<Box<[Option<T>]>>() + A::LEN * size_of::<Option<T>>()
}

/// Build a `GappedArray` with `occupancy` contiguous slots present and report
/// its total bytes: the pointer-sized handle plus the live heap block (capacity
/// is the next power-of-two ≥ occupancy, so the number differs from packed's
/// exact sizing).
fn gapped_bytes<T: Copy, A: Arity>(occupancy: usize, make: impl Fn(usize) -> T) -> usize {
    let mut g = GappedArray::<T, A>::new();
    for i in 0..occupancy {
        let idx = <A::Index as Niche>::try_from_usize(i).expect("i < occupancy <= LEN");
        g.insert(idx, make(i));
    }
    size_of::<GappedArray<T, A>>() + g.allocated_size()
}

fn render_cell<T: Copy, A: Arity>(
    title: &str,
    occupancies: &[usize],
    make: impl Fn(usize) -> T,
) -> String {
    let fixed = size_of::<FixedArray<Option<T>, A>>();
    let boxed = boxed_bytes::<T, A>();
    let mut out = String::new();
    let _ = write!(out, "### {title}\n\n");
    out.push_str("| occupancy | PackedArray | GappedArray | FixedArray | Box<[Option<T>]> |\n");
    out.push_str("|----------:|------------:|------------:|-----------:|-----------------:|\n");
    for &o in occupancies {
        let packed = packed_bytes::<T, A>(o, &make);
        let gapped = gapped_bytes::<T, A>(o, &make);
        let _ = writeln!(out, "| {o} | {packed} | {gapped} | {fixed} | {boxed} |");
    }
    out.push('\n');
    out
}

#[test]
fn memory_table() {
    let mut table = String::new();
    table.push_str(
        "Exact bytes (handle + heap). PackedArray heap = bitmap + occupancy × \
         size_of::<T> + padding. GappedArray heap = layout for next pow2 capacity ≥ \
         occupancy. FixedArray and Box are occupancy-independent. \
         Maps (BTreeMap/HashMap) are omitted: their heap use is impl-defined \
         per-entry overhead, not an analytic function.\n\n",
    );
    table.push_str(&render_cell::<[u8; 32], arity_arrays::Arity16>(
        "Cell A — Arity16 + [u8; 32]",
        &[0, 1, 4, 8, 16],
        |_| [0u8; 32],
    ));
    table.push_str(&render_cell::<u64, arity_arrays::Arity256>(
        "Cell B — Arity256 + u64",
        &[0, 1, 16, 64, 128, 256],
        |_| 0u64,
    ));
    insta::assert_snapshot!("memory_table", table);
}
