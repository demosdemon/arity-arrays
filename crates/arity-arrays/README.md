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

Throughput measured with [`divan`](https://crates.io/crates/divan) over the two
representative cells (Arity16 + 32-byte hash; Arity256 + 8-byte pointer
stand-in), comparing `PackedArray` against `FixedArray`, `Box<[Option<T>]>`,
`BTreeMap`, and `HashMap`. `GappedArray` is also a benchmark subject in the
full suite but is omitted from the snapshot below; reproduce all results with
`just bench`.

The `trie` bench (`cargo bench -p arity-arrays --bench trie`) additionally times
recursive `Clone`/`Drop` of a trie fixture with non-POD node contents (`Edge`
children owning a `Box`/`Arc` subtree) across all four representations,
contrasting `FixedArray`'s full-width (`A::LEN`) per-node cost with `PackedArray`
(per live child) and `GappedArray` (per power-of-two capacity ≥ live count).

> measured on: Apple M3 Max, rustc 1.98.0-nightly (f428d123a 2026-06-19), 2026-06-28

Absolute nanoseconds are machine-specific; the comparison *between*
representations is the durable signal. Highlights (median latency):

| benchmark | occupancy | PackedArray | FixedArray | Box<[Option<T>]> | BTreeMap | HashMap |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| Cell A `get_hit` | 8/16 | 1.10 ns | 0.69 ns | 0.78 ns | 2.16 ns | 10.05 ns |
| Cell A `insert_new` | 4/16 | 35.24 ns | 9.36 ns | 13.75 ns | 23.19 ns | 25.55 ns |
| Cell A `churn` | – | 7.915 µs | 333 ns | 276 ns | 2.87 µs | 3.34 µs |
| Cell B `get_hit` | 128/256 | 1.43 ns | 0.50 ns | 0.74 ns | 4.56 ns | 7.29 ns |
| Cell B `insert_new` | 64/256 | 47.61 ns | 68.11 ns | 11.51 ns | 150.8 ns | 26.12 ns |
| Cell B `churn` | – | 90.56 µs | 958 ns | 903 ns | 35.79 µs | 42.70 µs |

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
