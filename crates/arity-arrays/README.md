# arity-arrays

Fixed and pointer-sized heap-packed arrays over a generic arity, indexed without bounds checks.

`FixedArray<T, A>` is a full-width inline array (one slot per index); `PackedArray<T, A>` is a pointer-sized, heap-packed representation that stores only the present elements. Both are generic over the `Arity` trait, which pairs an index type with a bitmap backing and a `hybrid-array` size. Six concrete arities are provided: `Arity8`, `Arity16`, `Arity32`, `Arity64`, `Arity128`, and `Arity256`.

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

This crate is `#![no_std]` but requires `alloc` (heap allocation for `PackedArray`).

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
