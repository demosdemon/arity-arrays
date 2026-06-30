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
| `serde` | | `Serialize`/`Deserialize` for `FixedArray` (a sequence of `LEN` elements) and `PackedArray` (a sequence of ascending `(index, value)` pairs, validated on decode). |
| `serde_with` | | Adds the [`Compact`] adapter (`#[serde_as(as = "Compact")]`) — a compact, backing-independent `PackedArray` encoding (fixed little-endian bitmap + dense values). Implies `serde`. |
| `ethnum` | | Forwards to `arity-bitmap/ethnum` (the arity-256 backing swap). |
| `std` | | Forwards `std` to the optional std-capable dependencies; the crate is `no_std` + `alloc`. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set — run `cargo test`, not a per-arity `cargo test`.

### Serialization stability

The serde wire formats (the logical `(index, value)` form and the `Compact`
form) are locked by snapshot tests so any drift is a reviewable diff, but they
are **not yet guaranteed stable**: they may change before `1.0` if a production
consumer's encoding needs differ. The `Compact` form is backing-independent — it
is identical whether the arity-256 backing is the custom `U256` or `ethnum::U256`.

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

The `trie` bench (`cargo bench -p arity-arrays --bench trie`) additionally times
recursive `Clone`/`Drop` of a trie fixture with non-POD node contents (`Edge`
children owning a `Box`/`Arc` subtree) across all four representations,
contrasting `FixedArray`'s full-width (`A::LEN`) per-node cost with `PackedArray`
(per live child) and `GappedArray` (per power-of-two capacity ≥ live count).

Absolute nanoseconds are machine-specific; the comparison *between*
representations is the durable signal. Highlights (median latency):

<!-- bench:start -->
**Cell A (Arity16) single-op (median, max occupancy)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `get_hit` | 3.46 ns | 0.60 ns | 0.81 ns | 4.68 ns | 7.19 ns | 1.07 ns |
| `get_miss` | 3.48 ns | 0.61 ns | 0.80 ns | 1.61 ns | 6.03 ns | 0.81 ns |
| `insert_new` | 28.09 ns | 23.97 ns | 11.13 ns | 59.99 ns | 37.97 ns | 37.86 ns |
| `insert_replace` | 59.80 ns | 22.89 ns | 11.63 ns | 24.54 ns | 41.99 ns | 18.07 ns |
| `iter_present` | 16.89 ns | 7.06 ns | 9.10 ns | 22.77 ns | 9.48 ns | 18.82 ns |
| `remove` | 72.28 ns | 25.59 ns | 11.22 ns | 23.80 ns | 45.86 ns | 45.96 ns |

**Cell B (Arity256) single-op (median, max occupancy)**

| op | BTreeMap | BoxArr | FixedArray | GappedArray | HashMap | PackedArray |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `get_hit` | 5.49 ns | 0.54 ns | 0.52 ns | 13.37 ns | 7.58 ns | 1.53 ns |
| `get_miss` | 9.68 ns | 0.49 ns | 0.48 ns | 0.78 ns | 5.99 ns | 0.81 ns |
| `insert_new` | 331.03 ns | 26.40 ns | 80.85 ns | 513.13 ns | 31.33 ns | 44.72 ns |
| `insert_replace` | 638.64 ns | 25.01 ns | 79.34 ns | 33.20 ns | 29.81 ns | 14.12 ns |
| `iter_present` | 183.61 ns | 51.74 ns | 81.38 ns | 542.46 ns | 135.68 ns | 659.03 ns |
| `remove` | 643.57 ns | 24.38 ns | 78.91 ns | 34.18 ns | 34.44 ns | 66.69 ns |

<!-- bench:end -->

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
