# arity-index

Bounds-check-free niche integer index types (`U3`–`U7`) with double-ended range iterators.

Each `U{n}` is a newtype over a fieldless enum with `2ⁿ` variants, so `Option<U{n}>` is one byte (niche optimization) and indexing a `2ⁿ`-length array can elide the bounds check. The [`Niche`] trait unifies the index types (including the native `u8` for arity 256); iteration over a type's whole domain is via `NicheRange` / `NicheRangeInclusive`.

## Usage

```rust
use arity_index::{Niche, U4, NicheRange};

// The whole domain, ascending:
let all: Vec<u8> = U4::all().map(U4::as_u8).collect();
assert_eq!(all.len(), 16);

// A sub-range, double-ended:
let mut r = NicheRange::new(U4::new_masked(1), U4::new_masked(4));
assert_eq!(r.next().map(U4::as_u8), Some(1));
assert_eq!(r.next_back().map(U4::as_u8), Some(3));
```

## `no_std`

This crate is `#![no_std]`. It has no dependencies beyond `core`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
