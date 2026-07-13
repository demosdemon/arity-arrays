# arity-bitmap

Fixed-width bitmaps (`u8`–`u128`, `U256`) indexed by [`arity-index`] niche integers, with a double-ended iterator over the set bits.

The [`Bitmap`] trait is implemented for `u8`, `u16`, `u32`, `u64`, `u128` (indexed by `U3`–`U7`) and the 256-bit `U256` type (indexed by `u8`). The crate contains no `unsafe` code: every bit position is reconstructed through the statically-bounded niche index.

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

`Bitmap::nearest_clear_at_or_below` and `Bitmap::nearest_clear_in` locate the
nearest **clear** bit at or below, or within, a range in `O(1)` per limb.
`arity-arrays` uses their result for unchecked pointer arithmetic, so their
contract — a returned position always names a clear bit `< WIDTH` — is
safety-load-bearing for that crate even though this one is
`#![forbid(unsafe_code)]`.

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the bitmap backings you use (`8` → `u8`, …, `128` → `u128`, `256` → the 256-bit backing). Forwards to the matching `arity-index` feature. |
| `ethnum` | | Swaps the arity-256 backing from the self-contained two-limb `U256` to [`ethnum::U256`](https://docs.rs/ethnum). Takes effect only when `256` is also enabled. |
| `std` | | Forwards `std`; the crate is `no_std`-first. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set — run `cargo test`, not a per-arity `cargo test`.

### The 256-bit backing is opaque

By default, arity-256 uses a self-contained two-limb `U256`; the `ethnum` feature
swaps it for `ethnum::U256`. **The concrete 256-bit type is `#[doc(hidden)]` and
is not a stable API name.** Access the arity-256 bitmap only through the trait —
`<Arity256 as Arity>::Bitmap`, or generically as `B: Bitmap` — never by naming
`U256` directly. Because no supported code path names the concrete type, the
`ethnum` swap is a non-observable implementation detail (it does not change any
stable type identity). Naming `arity_bitmap::U256` directly is unsupported and may
break between releases.

## `no_std`

This crate is `#![no_std]`. It depends only on [`arity-index`] and `core`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
