# arity-arrays

Fixed, pointer-sized heap-packed, and gapped arrays over a generic arity, indexed without bounds checks.

`FixedArray<T, A>` is a full-width inline array (one slot per index); `PackedArray<T, A>` is a pointer-sized, heap-packed representation that stores only the present elements; `GappedArray<T, A>` is a pointer-sized, heap-backed representation with spare capacity and gaps that minimize mutation cost. All three are generic over the `Arity` trait, which pairs an index type with a bitmap backing and a `hybrid-array` size. Six concrete arities are provided: `Arity8`, `Arity16`, `Arity32`, `Arity64`, `Arity128`, and `Arity256`.

## Usage

```rust
use arity_arrays::{Arity16, FixedArray, PackedArray};
use arity_arrays::index::{Niche, U4};

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

## Memory layout

A 16-slot `FixedArray<Option<[u8; 32]>>` occupies a constant **528 bytes**
regardless of how many slots are filled, while `PackedArray` costs one pointer
when empty and `bitmap + occupancy × size_of::<T>` (plus header padding) when
populated. `GappedArray` follows the same formula but sizes its heap block to a
**capacity** rounded up to the next power of two (bounded by `A::LEN`) rather
than to the exact occupancy, and its header carries a second bitmap plus a
one-byte capacity exponent — so even at the power-of-two occupancies in the
table below (where the rounding is a no-op) it costs a few bytes more than
`PackedArray`, and a non-power-of-two occupancy pays the rounding difference on
top. Exact figures (handle + heap), computed by `cargo test --test
memory_report`:

### Cell A — Arity16 + [u8; 32]

| occupancy | `PackedArray` | `GappedArray` | `FixedArray` | `Box<[Option<T>]>` |
|----------:|------------:|------------:|-----------:|-----------------:|
| 0 | 8 | 8 | 528 | 544 |
| 1 | 42 | 46 | 528 | 544 |
| 4 | 138 | 142 | 528 | 544 |
| 8 | 266 | 270 | 528 | 544 |
| 16 | 522 | 526 | 528 | 544 |

## Capacity overflow

Every allocating operation on `PackedArray` or `GappedArray` requires
`size_of::<T>() * A::LEN <= isize::MAX`: `insert`, `Clone`, the `From`
conversions that build either type, and `GappedArray`'s
`with_capacity`/`reserve`. A `T` large enough to cross that bound (tens of
petabytes per element on 64-bit for `A::LEN == 256`) makes those operations
panic on the overflowing layout computation, mirroring `Vec::with_capacity`'s
overflow behavior — though the panic message text (`element layout overflow`
or `block layout overflow`) differs from `Vec`'s. This is unreachable for any
practical element type.

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the `Arity{N}` markers you use. Forwards to the matching `arity-index`/`arity-bitmap` features. The hexary/16-ary shape — the trie child-storage layout used by the [Firewood](https://github.com/ava-labs/firewood) database — is `default-features = false, features = ["16"]`. |
| `serde` | | `Serialize`/`Deserialize` for `FixedArray` (a sequence of `LEN` elements) and for `PackedArray` and `GappedArray` (each a sequence of ascending `(index, value)` pairs, validated on decode). |
| `serde_with` | | Adds the [`Compact`] adapter (`#[serde_as(as = "Compact")]`) — a compact encoding for `PackedArray` and `GappedArray` (fixed little-endian bitmap + dense values). Implies `serde`. |
| `std` | | Forwards `std` to the optional std-capable dependencies; the crate is `no_std` + `alloc`. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set — run `cargo test`, not a per-arity `cargo test`.

### Serialization stability

The serde wire formats (the logical `(index, value)` form and the `Compact`
form) are locked by snapshot tests so any drift is a reviewable diff, but they
are **not yet guaranteed stable**: they may change before `1.0` if a production
consumer's encoding needs differ. The `Compact` form is a canonical
little-endian encoding, independent of the in-memory representation.

[`Compact`]: https://docs.rs/arity-arrays/latest/arity_arrays/struct.Compact.html

## `no_std`

This crate is `#![no_std]` but requires `alloc` (heap allocation for `PackedArray` and `GappedArray`).

## MSRV

Minimum Supported Rust Version: **1.92**.

## Performance

Throughput is measured with [`criterion`](https://crates.io/crates/criterion)
(run via [`cargo-criterion`](https://crates.io/crates/cargo-criterion)) over the
two representative cells (Arity16 + 32-byte hash; Arity256 + 8-byte pointer
stand-in), comparing `PackedArray` against `GappedArray`, `FixedArray`,
`Box<[Option<T>]>`, `BTreeMap`, and `HashMap`. Reproduce all results with
`just bench`; refresh the tables below and the charts in `docs/bench/` with
`just bench-export <label>` then `just bench-charts <label>`.

The published numbers use the default `release`/`bench` profiles, without
link-time optimization. Downstream builds choose their own profile, so the hot
`get`/`insert`/`remove` path is tuned to be fast *without* LTO: every concrete
`arity-bitmap` `Bitmap`/`Raw` method it crosses into carries `#[inline]`, making
it cross-crate-inlinable without a whole-program pass. An opt-in `lto-probe`
profile (`cargo build --profile lto-probe`, or `just bench-lto`) runs fat
LTO — the strongest link-time pass available — purely to measure its ceiling
on that path: if fat LTO finds nothing to inline, no weaker LTO setting will
either. The committed codegen probe
([`examples/inline_probe.rs`](examples/inline_probe.rs), reproduce with `cargo
asm --profile lto-probe -p arity-arrays --all-features --example inline_probe
probe_packed_get`) confirms that on the hot `get` path, the `release` and
`lto-probe` builds both emit call-free code, so LTO adds no cross-crate
inlining there. Consumers who enable LTO in their own builds may still see
whole-program gains this narrow probe does not capture.

Pull requests get an automatic quick A/B comparison (base vs head, same runner) posted
as a sticky comment and in the job summary; comment `@exec-complete-benchmark-comparison`
on a PR for a full-precision on-demand re-run. Every push to `main` runs the same
full-precision comparison against the previous commit. Compare two local captures the
same way with `just bench-compare <run> <baseline>`.

The `trie` bench (`cargo bench -p arity-arrays --bench trie`) additionally times
recursive `Clone`/`Drop` of a trie fixture with non-plain-old-data (non-POD)
node contents (`Edge` children owning a `Box`/`Arc` subtree) across all four
representations,
contrasting `FixedArray`'s full-width (`A::LEN`) per-node cost with `PackedArray`
(per live child) and `GappedArray` (per power-of-two capacity ≥ live count).

Absolute nanoseconds are machine-specific (these were captured on an AWS
Graviton5 CPU — EC2 `c9g.4xlarge`); the comparison *between* representations is
the durable signal.

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

Highlights (median latency):

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

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
