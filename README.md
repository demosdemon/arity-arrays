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
runs the throughput bench under the opt-in `lto-probe` profile; see
`crates/arity-arrays/README.md`'s Performance section for the codegen finding on
the hot `get` path.

`just bench-ab` measures what that profile actually buys. `lto-probe` moves two
knobs at once — `lto = "fat"` *and* `codegen-units = 1` — so comparing it against
the default profile cannot attribute a delta to link-time optimization;
`codegen-units = 1` is a sizeable effect on its own. The recipe adds a third arm
that moves only `codegen-units`, then runs all three interleaved as a palindrome
(`A B C C B A`) so that every arm shares the run's centroid and slow linear
thermal drift cancels within each pairwise differential. It reports three
contrasts — codegen-units alone, LTO's marginal effect, and the two combined —
and `--update-docs` republishes the tables below and the `docs/bench/` SVGs from
the default-profile arm. See `scripts/bench-ab.sh` for the full rationale.

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
| `get_hit` | 16 | 2.59 | 0.87 | 0.86 | 5.28 | 9.98 | 0.91 |
| `get_hit_rand` | 16 | 4.28 | 1.96 | 1.97 | 14.84 | 10.40 | 1.94 |
| `get_miss` | 12 | 4.64 | 0.87 | 0.86 | 0.61 | 8.71 | 0.47 |
| `get_miss_rand` | 12 | 4.92 | 1.96 | 1.98 | 1.83 | 9.16 | 1.83 |
| `insert_new` | 12 | 12.78 | 4.54 | 3.70 | 15.09 | 17.76 | 37.37 |
| `insert_replace` | 16 | 12.63 | 4.14 | 3.73 | 8.79 | 22.82 | 7.77 |
| `iter_present` | 16 | 17.05 | 9.91 | 9.63 | 15.61 | 15.89 | 19.53 |
| `remove` | 16 | 23.61 | 4.20 | 3.36 | 7.43 | 25.08 | 31.01 |

**Cell A (Arity16) workload (median ns)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `build` | 153.08 | 27.38 | 41.80 | 308.13 | 688.69 | 231.42 |
| `churn` | 3659.93 | 633.79 | 678.38 | 4240.68 | 4500.03 | 3643.22 |

**Cell B (Arity256) single-op (median ns)**

| op | occ | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `get_hit` | 256 | 7.56 | 0.79 | 0.78 | 24.55 | 9.74 | 3.34 |
| `get_hit_rand` | 256 | 6.99 | 1.88 | 1.89 | 24.87 | 10.22 | 4.69 |
| `get_miss` | 192 | 11.09 | 0.79 | 0.78 | 1.78 | 9.17 | 1.22 |
| `get_miss_rand` | 192 | 11.82 | 1.89 | 1.89 | 2.19 | 9.60 | 1.99 |
| `insert_new` | 192 | 26.60 | 2.48 | 4.77 | 56.58 | 23.42 | 60.41 |
| `insert_replace` | 256 | 19.79 | 3.06 | 4.86 | 25.73 | 22.82 | 5.43 |
| `iter_present` | 256 | 232.75 | 66.60 | 101.45 | 792.16 | 205.13 | 1018.52 |
| `remove` | 256 | 21.75 | 2.35 | 4.88 | 27.86 | 25.84 | 52.69 |

**Cell B (Arity256) workload (median ns)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `build` | 3877.77 | 256.83 | 264.48 | 15766.56 | 9543.46 | 9410.79 |
| `churn` | 29844.15 | 1298.70 | 1768.79 | 115448.69 | 41602.73 | 85754.27 |

**Conversion (median ns, max occupancy)**

| op | cell_a | cell_b |
| :--- | ---: | ---: |
| `pack` | 32.11 | 568.93 |
| `unpack` | 51.09 | 1491.96 |

**Trie arity16 clone (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 343855.69 | 14846.58 | 608994.65 |
| `FixedStore` | 824907.17 | 9863.06 | 1070773.76 |
| `GappedStore` | 297417.52 | 6937.69 | 472361.82 |
| `PackedStore` | 300310.33 | 7284.32 | 467169.88 |

**Trie arity16 drop (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 344237.00 | 9134.78 | 631959.21 |
| `FixedStore` | 294097.07 | 6063.66 | 459894.15 |
| `GappedStore` | 317950.72 | 8438.24 | 572480.21 |
| `PackedStore` | 328355.13 | 8246.17 | 565187.05 |

**Trie arity256 clone (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 345455.19 | 6900.82 | 614308.08 |
| `FixedStore` | 16297541.50 | 71858.83 | 18232711.42 |
| `GappedStore` | 403092.90 | 3516.57 | 522134.97 |
| `PackedStore` | 343410.63 | 3309.06 | 493513.44 |

**Trie arity256 drop (median ns)**

| store | Bushy | Chain | Realistic |
| :--- | ---: | ---: | ---: |
| `BTreeStore` | 346426.77 | 3994.90 | 634741.91 |
| `FixedStore` | 4696145.25 | 9376.78 | 5594844.25 |
| `GappedStore` | 325523.20 | 3777.09 | 593511.89 |
| `PackedStore` | 328288.12 | 3539.99 | 567456.19 |


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
**`0.2.0-alpha.2`**, `arity-index` is **`0.1.3`** — production-*worthy*, but
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
