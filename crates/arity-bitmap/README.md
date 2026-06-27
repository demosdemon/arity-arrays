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

## `no_std`

This crate is `#![no_std]`. It depends only on [`arity-index`] and `core`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
