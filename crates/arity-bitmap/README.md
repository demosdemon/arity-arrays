# arity-bitmap

Fixed-width bitmaps (`u8`–`u128`, `U256`) indexed by [`arity-index`] niche integers, with a double-ended iterator over the set bits.

The [`Bitmap`] trait is implemented for `u8`, `u16`, `u32`, `u64`, `u128` (indexed by `U3`–`U7`) and the 256-bit `U256` type (indexed by `u8`). The crate performs no `unsafe` operations: every bit position is reconstructed through the statically-bounded niche index.

## Usage

```rust
use arity_bitmap::Bitmap;
use arity_index::{Niche, U4};

let bm = u16::ZERO
    .with_bit(U4::new_masked(1))
    .with_bit(U4::new_masked(4))
    .with_bit(U4::new_masked(9));

assert_eq!(bm.count_ones(), 3);
assert!(bm.test(U4::new_masked(4)));
assert_eq!(bm.rank(U4::new_masked(4)), 1); // one set bit below index 4

let set: Vec<u8> = bm.bits().map(U4::as_u8).collect();
assert_eq!(set, vec![1, 4, 9]);
```

### Safety-critical query methods

Two methods locate a **clear** bit in `O(1)` per limb (a *limb* is one 64-bit
word of the bitmap's backing integer):

- `Bitmap::nearest_clear_at_or_below(self, from: usize)` returns the greatest
  clear index at or below `from`, searching toward index `0`; `None` if every
  bit in `0..=from` is set.
- `Bitmap::nearest_clear_in(self, from: usize, limit: usize)` returns the
  least clear index in the half-open range `[from, limit)`, searching toward
  `limit`; `None` if that range is fully set.

`arity-arrays` (a downstream crate in this workspace that consumes `Bitmap`
for unchecked array indexing) uses their result for unchecked pointer
arithmetic, so their contract — a returned position always names a clear bit
`< WIDTH` — is
safety-load-bearing for that crate. This crate performs no unsafe operations
of its own; its only `unsafe` is the private `unsafe impl Raw` contract marker
(`#![deny(unsafe_code)]`).

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the bitmap backings you use (`8` → `u8`, …, `128` → `u128`, `256` → the 256-bit backing). Forwards to the matching `arity-index` feature. |
| `std` | | Forwards `std`; the crate is `no_std`-first. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set — run `cargo test`, not a per-arity `cargo test`.

### The 256-bit backing

Arity-256 is backed by [`ethnum::U256`](https://docs.rs/ethnum), re-exported as
`arity_bitmap::U256`. The supported surface is the [`Bitmap`] trait; `ethnum`'s
inherent integer operations are reachable but not part of the stability
guarantee. `ethnum` is a public dependency, pulled in by the `256` feature.

## `no_std`

This crate is `#![no_std]`. With default features it also depends on `ethnum`
for the 256-bit backing (pulled in by the `256` feature); with only the
`≤128` arities enabled (`default-features = false`, opting back into `8`
through `128` as needed), its only dependencies are [`arity-index`] and
`core`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.

[`Bitmap`]: https://docs.rs/arity-bitmap/latest/arity_bitmap/trait.Bitmap.html
[`arity-index`]: https://crates.io/crates/arity-index
