# arity-arrays

Fixed, pointer-sized heap-packed, and gapped arrays over a generic arity, indexed without bounds checks.

`FixedArray<T, A>` is a full-width inline array (one slot per index); `PackedArray<T, A>` is a pointer-sized, heap-packed representation that stores only the present elements; `GappedArray<T, A>` is a pointer-sized, heap-backed representation with spare capacity and gaps that minimizes mutation cost. All three are generic over the `Arity` trait, which pairs an index type with a bitmap backing and a `hybrid-array` size. Six concrete arities are provided: `Arity8`, `Arity16`, `Arity32`, `Arity64`, `Arity128`, and `Arity256`.

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
```

## Memory layout

A 16-slot `FixedArray<Option<[u8; 32]>>` occupies a constant **528 bytes**
regardless of how many slots are filled, while `PackedArray` costs one pointer
when empty and `bitmap + occupancy × size_of::<T>` (plus header padding) when
populated. Exact figures (handle + heap), computed by `cargo test --test
memory_report`:

### Cell A — Arity16 + [u8; 32]

| occupancy | PackedArray | GappedArray | FixedArray | Box<[Option<T>]> |
|----------:|------------:|------------:|-----------:|-----------------:|
| 0 | 8 | 8 | 528 | 544 |
| 1 | 42 | 46 | 528 | 544 |
| 4 | 138 | 142 | 528 | 544 |
| 8 | 266 | 270 | 528 | 544 |
| 16 | 522 | 526 | 528 | 544 |

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the `Arity{N}` markers you use. Forwards to the matching `arity-index`/`arity-bitmap` features. The hexary (firewood) shape is `default-features = false, features = ["16"]`. |
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

Throughput measured with [`criterion`](https://crates.io/crates/criterion)
(run via [`cargo-criterion`](https://crates.io/crates/cargo-criterion)) over the
two representative cells (Arity16 + 32-byte hash; Arity256 + 8-byte pointer
stand-in), comparing `PackedArray` against `GappedArray`, `FixedArray`,
`Box<[Option<T>]>`, `BTreeMap`, and `HashMap`. Reproduce all results with
`just bench`; refresh the tables below and the charts in `docs/bench/` with
`just bench-export <label>` then `just bench-charts <label>`.

Pull requests get an automatic quick A/B comparison (base vs head, same runner) posted
as a sticky comment and in the job summary; comment `@exec-complete-benchmark-comparison`
on a PR for a full-precision on-demand re-run. Every push to `main` runs the same
full-precision comparison against the previous commit. Compare two local captures the
same way with `just bench-compare <run> <baseline>`.

The `trie` bench (`cargo bench -p arity-arrays --bench trie`) additionally times
recursive `Clone`/`Drop` of a trie fixture with non-POD node contents (`Edge`
children owning a `Box`/`Arc` subtree) across all four representations,
contrasting `FixedArray`'s full-width (`A::LEN`) per-node cost with `PackedArray`
(per live child) and `GappedArray` (per power-of-two capacity ≥ live count).

Absolute nanoseconds are machine-specific (these were captured on an AWS
Graviton5 CPU — EC2 `c9g.4xlarge`); the comparison *between* representations is
the durable signal. Highlights (median latency):

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

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
