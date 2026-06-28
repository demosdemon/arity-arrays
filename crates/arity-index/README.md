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

## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the index types you use. The numbers are the arity (`8` → `U3`, …, `128` → `U7`, `256` → the native `u8` index). To compile a subset, disable defaults: `arity-index = { version = "0.1", default-features = false, features = ["16"] }`. |
| `serde` | | `Serialize`/`Deserialize` for `U3`–`U7` (serialized as their integer value; deserialization **validates** the value is in range). `no_std`-compatible. |
| `std` | | Forwards `std` to optional std-capable dependencies. The crate is `no_std`-first; this feature only matters when `serde` is also enabled. |

The arity features are **additive** and safe to combine. The test suite compiles
and runs only under the default (all-arity) feature set — run `cargo test`, not a
per-arity `cargo test --no-default-features --features 16`.

## `no_std`

This crate is `#![no_std]`. It has no dependencies beyond `core`.

## MSRV

Minimum Supported Rust Version: **1.92**.

## License

MIT — see [LICENSE](../../LICENSE) or <https://opensource.org/licenses/MIT>.
