# arity-arrays

Fixed-arity array storage indexed by bounds-check-free niche integers, with a
compact heap-packed sparse representation. Three small, `no_std` crates.

This workspace generalizes the hexary (16-ary) trie child-storage layout from
[`ava-labs/firewood#2100`](https://github.com/ava-labs/firewood/pull/2100) from a
single 16-wide layout to arbitrary power-of-two arities `N ∈ {8, 16, 32, 64, 128, 256}`.

## Three representations

A branch node stores its children in a fixed array indexed by a small integer.
Three layouts are provided:

- **Full-width** (`FixedArray<T, A>`) — one slot per index, every slot
  materialized. Cheap random access, fixed size regardless of occupancy.
- **Packed** (`PackedArray<T, A>`) — only the present entries stored, addressed by
  a bitmap via rank-select. Pointer-sized (and zero-heap) when empty; heap cost
  proportional to occupancy.
- **Gapped** (`GappedArray<T, A>`) — heap-backed with spare capacity kept at a
  geometric (power-of-two) size and gaps between elements so deletes are always
  move-free and inserts minimize moves. The write-throughput corner: trades
  memory for lower mutation cost.

The packed form is the memory-amplification mitigation from firewood#2100: a
16-slot `FixedArray<Option<[u8; 32]>>` occupies a constant **528 bytes**
regardless of how many slots are filled, while `PackedArray` costs one pointer
when empty and `bitmap + occupancy × size_of::<T>` (plus header padding) when
populated. Exact figures (handle + heap), computed by `cargo test --test
memory_report`:

### Cell A — Arity16 + [u8; 32]

| occupancy | `PackedArray` | `GappedArray` | `FixedArray` | `Box<[Option<T>]>` |
|----------:|------------:|------------:|-----------:|-----------------:|
| 0 | 8 | 8 | 528 | 544 |
| 1 | 42 | 46 | 528 | 544 |
| 4 | 138 | 142 | 528 | 544 |
| 8 | 266 | 270 | 528 | 544 |
| 16 | 522 | 526 | 528 | 544 |

All three rely on a **niche integer index** — a small type whose value is statically
known to be in `0..N`, which (a) makes `Option<Index>` one byte via niche
optimization and (b) lets the compiler elide bounds checks when indexing.

## Arity → index → bitmap

Each arity wires a niche index type to a bitmap backing of matching width:

| Arity `N` | Index type | `Option<Index>` | Bitmap backing |
| --------: | ---------- | --------------- | -------------- |
|         8 | `U3`       | 1 byte          | `u8`           |
|        16 | `U4`       | 1 byte          | `u16`          |
|        32 | `U5`       | 1 byte          | `u32`          |
|        64 | `U6`       | 1 byte          | `u64`          |
|       128 | `U7`       | 1 byte          | `u128`         |
|       256 | `u8`       | 2 bytes¹        | `U256`         |

¹ Arity-256 uses the native `u8` as its index. `u8`'s maximum (255) is already
`< 256`, so indexing a 256-element array elides the bounds check without a custom
type; `Option<u8>` is 2 bytes, but no `Option<u8>` is stored on a hot path, so
this costs nothing in practice.

The wiring is a compile-time guarantee: for every arity,
`Index::COUNT == LEN == Bitmap::WIDTH`.

## Throughput benchmarks

`just bench` runs both criterion benches via `cargo-criterion` (`cargo criterion
-p arity-arrays`; pass criterion args after `--`, e.g. `just bench --
--sample-size 50`). Export a run with `just bench-export <label>` and regenerate
the comparison tables below plus the SVG charts in `docs/bench/` with `just
bench-charts <label>`. `just bench-lto <args>` (e.g. `just bench-lto --save-baseline lto`) builds and
runs the throughput bench under the opt-in `lto-probe` profile (fat LTO) to
measure link-time optimization's effect; see `crates/arity-arrays/README.md`'s
Performance section for the finding on the hot `get` path.

Pull requests get an automatic quick A/B comparison (base vs head, same runner) posted
as a sticky comment and in the job summary; comment `@exec-complete-benchmark-comparison`
on a PR for a full-precision on-demand re-run. Every push to `main` runs the same
full-precision comparison against the previous commit, and the "Bench compare (manual)"
workflow's `workflow_dispatch` inputs trigger an ad-hoc comparison between any two refs.
None of these commit anything — see `.github/workflows/bench-compare.yml`. Compare two
local captures the same way with `just bench-compare <run> <baseline>`.

Medians below were captured on an AWS Graviton5 CPU (EC2 `c9g.4xlarge`); absolute
nanoseconds are machine-specific, but the comparison between representations — and
against `usize`-keyed `BTreeMap`/`HashMap` — is the durable signal:

In the tables, `BoxArr` is a naive `Box<[Option<T>]>` baseline; the `*Store` rows
pair each representation (and a `BTreeMap` baseline) with a trie fixture; and the
trie shapes are Chain (deep), Bushy (broad), and Realistic (tapered).

> [!NOTE]
> Sub-10 ns single points sit near criterion's measurement floor, and the fast
> `BENCH_QUICK` configuration used for quick CI comparisons (`sample_size = 10`)
> widens their confidence intervals further. Do not gate a regression or make a
> fine-grained ranking claim on a single-digit-nanosecond delta without first
> re-running that specific group at criterion defaults. The coarse multi-×
> differences between representations sit well above this noise floor and are the
> durable signal.

> [!NOTE]
> `get_hit`/`get_miss` probe a fixed slot every iteration, so their numbers
> reflect a fully branch-predicted, L1-resident load — best case. The
> `get_hit_rand`/`get_miss_rand` variants (in the bench harness; they populate
> the tables at the next capture refresh) probe a pseudo-random slot each
> iteration; the accessed working set is small enough to stay L1-resident
> regardless of access order, so their higher latency isolates the realistic
> branch-unfavorable cost — a mispredicted branch and a serialized dependent
> load on the index before the payload access — rather than a cache effect.
> Relative ranking between representations holds in both — every container
> gets the same treatment — but the fixed-slot absolutes understate real-world
> latency.

<!-- bench:start -->
**Cell A (Arity16) single-op (median ns)**

| op | occ | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `get_hit` | 16 | 2.64 | 0.87 | 0.86 | 16.25 | 10.08 | 0.91 |
| `get_miss` | 12 | 4.70 | 0.87 | 0.86 | 0.49 | 8.69 | 0.50 |
| `insert_new` | 12 | 14.49 | 4.73 | 3.79 | 15.17 | 17.53 | 37.20 |
| `insert_replace` | 16 | 13.17 | 4.23 | 3.76 | 8.84 | 22.59 | 7.81 |
| `iter_present` | 16 | 21.29 | 11.49 | 9.41 | 16.77 | 16.94 | 22.93 |
| `remove` | 16 | 24.77 | 4.24 | 3.40 | 7.25 | 24.33 | 31.26 |

**Cell A (Arity16) workload (median ns)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `build` | 176.60 | 27.39 | 41.90 | 306.02 | 739.32 | 232.16 |
| `churn` | 3450.89 | 695.04 | 719.96 | 4188.77 | 4401.46 | 3631.78 |

**Cell B (Arity256) single-op (median ns)**

| op | occ | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `get_hit` | 256 | 7.65 | 0.79 | 0.78 | 44.81 | 9.63 | 2.36 |
| `get_miss` | 192 | 11.37 | 0.79 | 0.78 | 0.93 | 8.66 | 0.93 |
| `insert_new` | 192 | 26.36 | 2.44 | 4.61 | 58.62 | 24.63 | 58.79 |
| `insert_replace` | 256 | 16.86 | 3.08 | 4.68 | 24.06 | 21.94 | 4.65 |
| `iter_present` | 256 | 241.90 | 75.07 | 104.03 | 648.99 | 202.97 | 978.63 |
| `remove` | 256 | 22.13 | 2.36 | 4.77 | 26.73 | 25.44 | 51.14 |

**Cell B (Arity256) workload (median ns)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `build` | 4140.60 | 252.94 | 269.08 | 14789.31 | 10512.02 | 9705.73 |
| `churn` | 32435.15 | 1299.63 | 1768.70 | 109084.58 | 41733.95 | 82408.86 |

**Conversion (median ns, max occupancy)**

| op | cell_a | cell_b |
| :--- | ---: | ---: |
| `pack` | 33.28 | 563.14 |
| `unpack` | 56.93 | 1144.67 |

**Trie arity16 clone (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 342483.78 | 14833.18 | 644597.76 |
| `FixedStore` | 888195.35 | 10039.49 | 1152781.04 |
| `GappedStore` | 308310.12 | 6956.91 | 472110.58 |
| `PackedStore` | 300149.33 | 7285.31 | 466877.12 |

**Trie arity16 drop (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 345463.32 | 9171.95 | 633331.14 |
| `FixedStore` | 293533.40 | 6070.14 | 459756.21 |
| `GappedStore` | 317851.80 | 8425.65 | 570922.04 |
| `PackedStore` | 328232.93 | 8301.13 | 565936.58 |

**Trie arity256 clone (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 342743.94 | 6923.96 | 606964.38 |
| `FixedStore` | 16407849.00 | 69793.24 | 18446429.00 |
| `GappedStore` | 386163.41 | 3320.39 | 511109.81 |
| `PackedStore` | 332862.64 | 3530.19 | 493300.63 |

**Trie arity256 drop (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 347580.21 | 4006.17 | 633991.25 |
| `FixedStore` | 4686250.00 | 9447.57 | 5529837.00 |
| `GappedStore` | 320097.02 | 3652.08 | 579639.72 |
| `PackedStore` | 328474.25 | 3517.00 | 566534.33 |


<!-- bench:end -->

`GappedArray` makes single-op deletes and gap-filling inserts cheap — deletes
never move elements, and an insert fills a nearby gap — at the cost of reads and
memory. Against `PackedArray`: `remove` is ~4× faster and `insert_new` ~2.4×
faster at Arity16 (`remove` ~2× faster, `insert_new` about even at Arity256).
But `get_hit` is ~18× slower (it scans past gaps), and on the aggregate
build/churn workloads it runs ~15-50% slower. Reach for it when deletes
dominate; `PackedArray` is the better default.

> [!NOTE]
> The `remove`/`insert_*` single-op benches build a fresh container per iteration
> outside the timed region (`iter_batched_ref`) and drop it outside the timed
> region too, so each sample times only the operation, not container teardown.
> Earlier revisions dropped the container in-region, which taxed the heap-backed
> representations (Packed/Gapped/maps) asymmetrically against inline `FixedArray`.

A second bench, `cargo bench -p arity-arrays --bench trie`, measures recursive
`Clone` and `Drop` of a compressed-trie fixture whose children array is each
representation, with non-plain-old-data (non-POD) node contents (`Edge` children
owning a `Box`/`Arc` subtree, plus a boxed value). It isolates the per-element
clone/drop cost — where `FixedArray` touches all `A::LEN` slots per node,
`PackedArray` pays per live child, and `GappedArray` pays for its power-of-two
capacity (≥ the live count) — across Chain (deep), Bushy (broad), and Realistic
(tapered) shapes.

## Crates

In dependency order:

| Crate | Purpose |
| :--- | :--- |
| [`arity-index`](crates/arity-index) | Bounds-check-free niche integer index types (`U3`–`U7`, and `u8` for arity-256) with double-ended range iterators. A small, contained `unsafe` surface (niche constructors and range-iterator internals). No `alloc`. |
| [`arity-bitmap`](crates/arity-bitmap) | Fixed-width bitmaps (`u8`–`u128`, `U256`) indexed by the niche integers, with a double-ended set-bit iterator. **No `unsafe` operations** (`#![deny(unsafe_code)]`; its only `unsafe` is an audited private `unsafe impl` contract marker). No `alloc`. |
| [`arity-arrays`](crates/arity-arrays) | `FixedArray`, `PackedArray`, and `GappedArray` over the sealed `Arity` trait. The only crate that needs `alloc`; carries `unsafe` for all three representations — bounds-check-elided indexing in `FixedArray`, and the heap layouts of `PackedArray` and `GappedArray` — as does `arity-index`, for its niche/range internals. |

Splitting this way keeps the primitive types reusable and lets their tests run
without touching the allocator. `arity-bitmap` depends on `arity-index` so the
`Bitmap` trait speaks in the typed index rather than raw `usize` — which makes
every bit position statically `< WIDTH`, eliminating the shift-undefined-behavior
(UB) precondition entirely.

## Example

```rust
use arity_arrays::{Arity16, FixedArray, PackedArray};
use arity_arrays::index::U4;

let mut full = FixedArray::<Option<u32>, Arity16>::new();
full[U4::new_masked(1)] = Some(10);
full[U4::new_masked(9)] = Some(90);

// Pack: pointer-sized handle, two elements on the heap.
let packed = PackedArray::from(&full);
assert_eq!(packed.count(), 2);
assert_eq!(packed.get(U4::new_masked(9)), Some(&90));

let present: Vec<(u8, u32)> =
    packed.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
assert_eq!(present, vec![(1, 10), (9, 90)]);

// Build straight from (index, value) pairs — no `full` array to populate by hand.
let collected: PackedArray<u32, Arity16> =
    [(U4::new_masked(1), 10), (U4::new_masked(9), 90)].into_iter().collect();

// Index a present slot (panics on an absent one, like a map).
assert_eq!(collected[U4::new_masked(9)], 90);

// Consume it back into (index, value) pairs, ascending.
let pairs: Vec<(u8, u32)> =
    collected.into_iter().map(|(i, v)| (i.as_u8(), v)).collect();
assert_eq!(pairs, vec![(1, 10), (9, 90)]);
```

## Cargo features

All three crates expose per-arity features `8`, `16`, `32`, `64`, `128`, `256`
(all default-on) so a consumer can compile only the widths it uses — e.g. the
hexary (firewood) shape is `default-features = false, features = ["16"]`. The
features are **additive** and safe to combine. The arrays crate additionally
offers `serde`, `serde_with` (the `Compact` adapter), and `std`. See each
crate's README for the full table.

> [!NOTE]
> The test suite runs only under the default (all-arity) feature set — run
> `cargo test`, not a per-arity `cargo test --no-default-features`. Feature
> gating is configuration, not logic, so the lean configurations are checked at
> the library-clippy level rather than re-run as a test matrix.

## Versioning and MSRV

These crates are not at a uniform version: `arity-arrays` and `arity-bitmap` are
**`0.2.0-alpha.2`**, `arity-index` is **`0.1.2`** — production-*worthy*, but
reserving the right to refine the API with real downstream use before a `1.0`
commitment. Under Cargo semver, each crate's `0.y.z` version means a breaking
change to that crate bumps its minor (`y`) version.

- `ethnum` is a **public dependency**, pulled in by the `256` feature: its `U256`
  is re-exported as the documented `arity_bitmap::U256`, the sole 256-bit backing.
  The supported surface is the `Bitmap` trait; `ethnum`'s inherent integer
  operations are reachable but not part of the stability guarantee. See the
  `arity-bitmap` README.
- The serde wire formats (the logical form and `Compact`) are snapshot-locked but
  **not yet guaranteed stable**: they may change before `1.0` if a production
  consumer's encoding needs differ.
- **Minimum Supported Rust Version (MSRV): 1.92.** MSRV bumps are documented as a
  minor-version event.

## License

MIT — see [LICENSE](LICENSE) or <https://opensource.org/licenses/MIT>.
