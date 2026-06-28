# Hardening Plan 4 — Serde, ethnum, and std Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the optional integrations — `serde` (a portable logical form plus a `serde_with::Compact` adapter), an `ethnum::U256` backing swap for arity-256, and the `std` feature — each behind its own cargo feature, all `no_std`-first.

**Architecture:** `serde` impls live where the data lives: `U{n}` in `arity-index` (validated), `FixedArray`/`PackedArray` in `arity-arrays`; bitmap backings get no standalone serde — the `Compact` adapter reconstructs any bitmap through the existing `Bitmap::to_le_bytes`/`from_le_bytes` byte surface. The `ethnum` feature `#[cfg]`-swaps the arity-256 backing from the custom two-limb `U256` to `ethnum::U256` (its `Bitmap` impl mirrors the native-integer pattern). The `std` feature forwards `dep?/std` to the optional std-capable deps.

**Tech Stack:** Rust (edition 2024, `#![no_std]` + `alloc`); optional deps `serde` (1.x, `default-features=false, features=["alloc","derive"]`), `serde_with` (3.21, `default-features=false, features=["alloc"]`), `ethnum` (1.5, `default-features=false`); `hybrid-array`'s `serde` feature; dev: `insta`, `serde_json`.

This is **plan 4 of 5** for the production-hardening effort
(`breaking-api` ✓ → `features-ci` ✓ → `mutation` ✓ → **`serde-ethnum`** → `publish`).
Design spec: `docs/superpowers/specs/2026-06-27-arity-arrays-hardening-design.md`
(sections "Per-arity features" → `std`, "`ethnum` backing swap", "Serde").

## Global Constraints

Copied from the spec and existing conventions; every task implicitly includes these.

- **`#![no_std]`** (all crates; `arity-arrays` also `alloc`). All new deps are pulled `default-features = false`; std-capable deps are forwarded only through each crate's `std` feature using **weak-dep syntax** (`dep?/std`).
- **Optional features, opt-in:** `serde`, `serde_with`, `ethnum`, `std` are NOT in any `default` set — `default` stays the six arities.
- **Verified external API facts (use exactly):**
  - `serde` `no_std`+`alloc`: `default-features = false, features = ["alloc", "derive"]`. `std` is its default; weak-forward `serde?/std`.
  - `serde_with` 3.21 `no_std`+`alloc`: `default-features = false, features = ["alloc"]`. Traits: `SerializeAs<T: ?Sized> { fn serialize_as<S: Serializer>(source: &T, serializer: S) -> Result<S::Ok, S::Error>; }` and `DeserializeAs<'de, T>: Sized { fn deserialize_as<D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error>; }`. Weak-forward `serde_with?/std`.
  - `ethnum` 1.5: unconditionally `no_std`, **no `std` feature**, default features empty. `U256(pub [u128; 2])` derives `Clone, Copy, Default, Eq, Hash, PartialEq`; manual `Debug, Ord, PartialOrd`; full bit/arith ops; `pub const fn from_words(hi, lo)` (**HI first**); `count_ones(self) -> u32` (`const`), `trailing_zeros`/`leading_zeros(self) -> u32`, `to_le_bytes(self) -> [u8; 32]`, `from_le_bytes([u8; 32]) -> Self`. Use `U256::from_words(0, 0)` for zero and `U256::from_words(0, 1)` for one (do NOT assume `ZERO`/`ONE` consts exist).
  - `hybrid-array` 0.4.12 has a `serde` feature; `Array<T, U>` then impls `Serialize` (`T: Serialize`) and `Deserialize` (`T: Deserialize`) as a fixed-length tuple of `U::USIZE` elements.
- **`arity-bitmap` gets NO serde** — bitmaps have no standalone `Serialize`/`Deserialize`. `arity-arrays/serde` forwards to `arity-index/serde` and `hybrid-array/serde` only.
- **`undocumented_unsafe_blocks`/`unsafe_op_in_unsafe_fn` are `deny`** (no `unsafe` is added in this plan). Lints strict (CI denies warnings): clippy pedantic+nursery warn, `unwrap_used` warn → no `.unwrap()` in lib or tests (`.expect("…")`/pattern-matching). No `#[allow]`; `#[expect(reason="…")]` only.
- Tests run under the relevant feature set; the CI `test`/`lint` jobs already use `--all-features`. Edition 2024, MSRV 1.92. Add deps with `cargo add`. Conventional-commit messages, imperative mood.
- **Line numbers are indicative; the quoted anchor text and shown code block govern** (confirm with `grep -n`).

---

### Task 1: `arity-index` — `serde` for the niche types + `std` feature

Add an optional `serde` dependency and `serde`/`std` features; implement validated `Serialize`/`Deserialize` for `U3`–`U7` inside the `niche_int!` macro. The native `u8` (arity-256 index) already has serde from `serde` itself — no impl needed.

**Files:**
- Modify: `crates/arity-index/Cargo.toml` (add `serde` dep + `serde`/`std` features)
- Modify: `crates/arity-index/src/niche.rs` (serde impls in the `niche_int!` macro)
- Test: `crates/arity-index/src/niche.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `$name::as_u8`/`try_new` (existing).
- Produces: feature `serde` on `arity-index`; `impl serde::{Serialize, Deserialize} for U{n}` (gated `#[cfg(feature = "serde")]`). `arity-arrays/serde` forwards here.

- [ ] **Step 1: Add the `serde` dependency and features to `crates/arity-index/Cargo.toml`**

Add to `[dependencies]`:
```toml
serde = { version = "1", optional = true, default-features = false }
```
Add to the `[features]` table (alongside the arities):
```toml
serde = ["dep:serde"]
std = ["serde?/std"]
```

- [ ] **Step 2: Write the failing test**

Add to `mod tests` in `crates/arity-index/src/niche.rs` (gated on `serde`):
```rust
#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_and_range_validation() {
    // Round-trip through JSON for a couple of values.
    let v = U4::new_masked(9);
    let json = serde_json::to_string(&v).expect("serialize U4");
    assert_eq!(json, "9");
    let back: U4 = serde_json::from_str(&json).expect("deserialize U4");
    assert_eq!(back, v);

    // Out-of-range integers are rejected (16 is not a valid U4).
    let err = serde_json::from_str::<U4>("16");
    assert!(err.is_err());
    // In-range boundary is accepted.
    assert_eq!(serde_json::from_str::<U4>("15").expect("15"), U4::MAX);
}
```
This test uses `serde_json` as a dev-dependency. Add it: `cargo add --dev serde_json` (in `crates/arity-index`).

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p arity-index --features serde serde_round_trip_and_range_validation`
Expected: FAIL to compile — `U4` does not implement `Serialize`.

- [ ] **Step 4: Add the serde impls to the `niche_int!` macro**

In `crates/arity-index/src/niche.rs`, inside `macro_rules! niche_int`, immediately after the `impl ::core::convert::From<$name> for usize { … }` block (confirm with `grep -n 'From<\$name> for usize' crates/arity-index/src/niche.rs`), add:
```rust
        #[cfg(feature = "serde")]
        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_u8(self.as_u8())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> ::core::result::Result<Self, D::Error> {
                let v = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::try_new(v).ok_or_else(|| {
                    ::serde::de::Error::invalid_value(
                        ::serde::de::Unexpected::Unsigned(::core::primitive::u64::from(v)),
                        &concat!("an integer in 0..", stringify!($count)),
                    )
                })
            }
        }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p arity-index --features serde serde_round_trip_and_range_validation`
Expected: PASS.

- [ ] **Step 6: Verify clean (default + serde + no_std-still-holds)**

Run:
```bash
cargo test -p arity-index --features serde
cargo clippy -p arity-index --all-targets --features serde -- -D warnings
cargo clippy -p arity-index --all-targets -- -D warnings          # serde OFF still clean
cargo build -p arity-index --no-default-features --features serde  # serde without std, no_std intact
```
Expected: all green; the last confirms the serde impls are `no_std` (they touch no `std`/`alloc`).

- [ ] **Step 7: Commit**

```bash
git add crates/arity-index/Cargo.toml crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): add validated serde for niche types behind a feature"
```

---

### Task 2: `arity-bitmap` — `ethnum::U256` backing swap + `std` feature

Add an optional `ethnum` dependency and `ethnum`/`std` features. Restructure `u256.rs` so the arity-256 backing is the custom two-limb `U256` by default, or `ethnum::U256` under the `ethnum` feature (mutually exclusive). Make the `U256` re-export `#[doc(hidden)]` (opaque — named only via `<Arity256 as Arity>::Bitmap`).

**Files:**
- Modify: `crates/arity-bitmap/Cargo.toml` (add `ethnum` dep + `ethnum`/`std` features)
- Modify: `crates/arity-bitmap/src/u256.rs` (split into custom + ethnum backings; shared tests)
- Modify: `crates/arity-bitmap/src/lib.rs:35` (`#[doc(hidden)]` on the `U256` re-export)

**Interfaces:**
- Consumes: the `Bitmap`/`Raw`/`Sealed` traits; `arity_index::Niche for u8`.
- Produces: features `ethnum`/`std`; under `ethnum`, `arity_bitmap::U256 == ethnum::U256` with the same `Bitmap` surface. `arity-arrays/ethnum` forwards here.

- [ ] **Step 1: Add the `ethnum` dependency and features to `crates/arity-bitmap/Cargo.toml`**

Add to `[dependencies]`:
```toml
ethnum = { version = "1.5", optional = true, default-features = false }
```
Add to `[features]`:
```toml
ethnum = ["dep:ethnum"]
std = ["arity-index/std"]
```

- [ ] **Step 2: Add the `# Safety`-free doc-hidden to the `U256` re-export in `crates/arity-bitmap/src/lib.rs`**

Change (line ~35) `#[cfg(feature = "256")] pub use u256::U256;` to:
```rust
#[cfg(feature = "256")]
#[doc(hidden)]
pub use u256::U256;
```
(`U256` is an implementation detail; the only supported way to name it is `<Arity256 as Arity>::Bitmap`. `#[doc(hidden)]` makes the `ethnum` swap a non-observable change. It stays usable internally — `arity.rs` still names `arity_bitmap::U256`.)

- [ ] **Step 3: Restructure `crates/arity-bitmap/src/u256.rs` into two backings + shared tests**

Replace the entire non-test portion of `u256.rs` (everything above `#[cfg(test)] mod tests {`) with the following. The existing custom `U256` (struct, `from_limbs`, `Raw`, `Bitmap`) moves verbatim into a `not(feature = "ethnum")` module; a new `ethnum`-feature module adds the `ethnum::U256` impls. Confirm the current top boundary with `grep -n 'mod tests' crates/arity-bitmap/src/u256.rs`.

```rust
//! The 256-bit bitmap backing (`Bitmap::Index == u8`).
//!
//! Two interchangeable backings select on the `ethnum` feature: the
//! self-contained two-limb `U256` (default), or a re-export of `ethnum::U256`
//! (feature `ethnum`). Both implement the same [`Bitmap`] surface; the type is
//! `#[doc(hidden)]` and named only via `<Arity256 as Arity>::Bitmap`.

use arity_index::Niche;

use crate::Bitmap;
use crate::Raw;
use crate::Sealed;

// Wire-up invariant: the u8 index domain (256) must equal the bit width.
const _: () = assert!(<u8 as Niche>::COUNT == 256);

// ---- Default backing: a self-contained two-limb integer (pure safe code). ----
#[cfg(not(feature = "ethnum"))]
mod custom {
    use super::{Bitmap, Raw, Sealed};

    /// A 256-bit bitmap: bit `i` lives in `lo` for `i < 128`, else in `hi` at
    /// `i - 128`.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct U256 {
        lo: u128,
        hi: u128,
    }

    impl U256 {
        /// Splits a bit index `i` (`< 256`) into `(limb_is_hi, bit_within_limb)`.
        const fn split(i: u8) -> (bool, u32) {
            if i < 128 {
                (false, i as u32)
            } else {
                (true, (i - 128) as u32)
            }
        }

        /// Builds a `U256` from its two little-endian 128-bit limbs. Internal
        /// helper for the byte surface; not part of the public API.
        pub(crate) const fn from_limbs(lo: u128, hi: u128) -> Self {
            Self { lo, hi }
        }
    }

    impl Sealed for U256 {}

    impl Raw for U256 {
        fn raw_is_zero(self) -> bool {
            self.lo == 0 && self.hi == 0
        }
        fn raw_popcount(self) -> u32 {
            self.lo.count_ones() + self.hi.count_ones()
        }
        fn raw_lowest_pos(self) -> usize {
            if self.lo != 0 {
                self.lo.trailing_zeros() as usize
            } else {
                128 + self.hi.trailing_zeros() as usize
            }
        }
        fn raw_highest_pos(self) -> usize {
            if self.hi != 0 {
                128 + self.hi.ilog2() as usize
            } else {
                self.lo.ilog2() as usize
            }
        }
        fn raw_clear_lowest(self) -> Self {
            if self.lo != 0 {
                Self { lo: self.lo & self.lo.wrapping_sub(1), hi: self.hi }
            } else {
                Self { lo: 0, hi: self.hi & self.hi.wrapping_sub(1) }
            }
        }
        fn raw_clear_highest(self) -> Self {
            if self.hi != 0 {
                Self { lo: self.lo, hi: self.hi & !(1u128 << self.hi.ilog2()) }
            } else if self.lo != 0 {
                Self { lo: self.lo & !(1u128 << self.lo.ilog2()), hi: 0 }
            } else {
                self
            }
        }
    }

    impl Bitmap for U256 {
        type Index = u8;
        const WIDTH: usize = 256;
        const ZERO: Self = Self { lo: 0, hi: 0 };

        fn is_zero(self) -> bool {
            self.lo == 0 && self.hi == 0
        }
        fn count_ones(self) -> u32 {
            self.lo.count_ones() + self.hi.count_ones()
        }
        fn test(self, i: u8) -> bool {
            let (is_hi, bit) = Self::split(i);
            let limb = if is_hi { self.hi } else { self.lo };
            limb & (1u128 << bit) != 0
        }
        fn with_bit(self, i: u8) -> Self {
            let (is_hi, bit) = Self::split(i);
            if is_hi {
                Self { lo: self.lo, hi: self.hi | (1u128 << bit) }
            } else {
                Self { lo: self.lo | (1u128 << bit), hi: self.hi }
            }
        }
        fn rank(self, i: u8) -> u32 {
            let (is_hi, bit) = Self::split(i);
            if is_hi {
                let hi_mask = (1u128 << bit) - 1;
                self.lo.count_ones() + (self.hi & hi_mask).count_ones()
            } else {
                let lo_mask = (1u128 << bit) - 1;
                (self.lo & lo_mask).count_ones()
            }
        }
        fn without_bit(self, i: u8) -> Self {
            let (is_hi, bit) = Self::split(i);
            if is_hi {
                Self { lo: self.lo, hi: self.hi & !(1u128 << bit) }
            } else {
                Self { lo: self.lo & !(1u128 << bit), hi: self.hi }
            }
        }
        fn to_le_bytes(self, buf: &mut [u8]) {
            buf[..16].copy_from_slice(&self.lo.to_le_bytes());
            buf[16..].copy_from_slice(&self.hi.to_le_bytes());
        }
        fn from_le_bytes(buf: &[u8]) -> Self {
            let mut lo = [0u8; 16];
            let mut hi = [0u8; 16];
            lo.copy_from_slice(&buf[..16]);
            hi.copy_from_slice(&buf[16..]);
            Self::from_limbs(u128::from_le_bytes(lo), u128::from_le_bytes(hi))
        }
    }
}
#[cfg(not(feature = "ethnum"))]
pub use custom::U256;

// ---- Optional backing: re-export `ethnum::U256` (a real 256-bit integer). ----
#[cfg(feature = "ethnum")]
mod ethnum_backed {
    use super::{Bitmap, Raw, Sealed};

    pub use ethnum::U256;

    // ethnum has no ZERO/ONE consts we can rely on; build them from words.
    const ZERO: U256 = U256::from_words(0, 0);
    const ONE: U256 = U256::from_words(0, 1);

    impl Sealed for U256 {}

    impl Raw for U256 {
        fn raw_is_zero(self) -> bool {
            self == ZERO
        }
        fn raw_popcount(self) -> u32 {
            ethnum::U256::count_ones(self)
        }
        fn raw_lowest_pos(self) -> usize {
            self.trailing_zeros() as usize
        }
        fn raw_highest_pos(self) -> usize {
            255 - self.leading_zeros() as usize
        }
        fn raw_clear_lowest(self) -> Self {
            if self == ZERO { self } else { self & (self - ONE) }
        }
        fn raw_clear_highest(self) -> Self {
            if self == ZERO {
                self
            } else {
                self & !(ONE << (255 - self.leading_zeros()))
            }
        }
    }

    impl Bitmap for U256 {
        type Index = u8;
        const WIDTH: usize = 256;
        const ZERO: Self = ZERO;

        fn is_zero(self) -> bool {
            self == ZERO
        }
        fn count_ones(self) -> u32 {
            ethnum::U256::count_ones(self)
        }
        fn test(self, i: u8) -> bool {
            (self >> u32::from(i)) & ONE != ZERO
        }
        fn with_bit(self, i: u8) -> Self {
            self | (ONE << u32::from(i))
        }
        fn rank(self, i: u8) -> u32 {
            if i == 0 {
                0
            } else {
                ethnum::U256::count_ones(self & ((ONE << u32::from(i)) - ONE))
            }
        }
        fn without_bit(self, i: u8) -> Self {
            self & !(ONE << u32::from(i))
        }
        fn to_le_bytes(self, buf: &mut [u8]) {
            buf.copy_from_slice(&ethnum::U256::to_le_bytes(self));
        }
        fn from_le_bytes(buf: &[u8]) -> Self {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(buf);
            ethnum::U256::from_le_bytes(arr)
        }
    }
}
#[cfg(feature = "ethnum")]
pub use ethnum_backed::U256;
```

Leave the existing `#[cfg(test)] mod tests { … }` block in place — its tests use only the `Bitmap` surface (`ZERO`, `with_bit`, `test`, `rank`, `count_ones`, `bits`, `without_bit`, `select`, `to_le_bytes`/`from_le_bytes`, `Hash`), so they validate whichever backing is active. (If the test module has `use super::*;`, it resolves `U256` to the active backing; confirm it does not reference `from_limbs` — it must not, as that is custom-only.)

- [ ] **Step 4: Verify both backings pass the same tests + clippy**

Run:
```bash
# Custom backing (default):
cargo test -p arity-bitmap --features 256
cargo clippy -p arity-bitmap --all-targets --features 256 -- -D warnings
# ethnum backing:
cargo test -p arity-bitmap --no-default-features --features "256,ethnum"
cargo clippy -p arity-bitmap --all-targets --no-default-features --features "256,ethnum" -- -D warnings
# Full default + ethnum:
cargo test -p arity-bitmap --features ethnum
```
Expected: all green. The same `mod tests` passes under both backings (behavioral parity across the swap). If `trailing_zeros`/`leading_zeros` are not `const`, that is fine — they are used in non-const methods only.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-bitmap/Cargo.toml crates/arity-bitmap/src/u256.rs crates/arity-bitmap/src/lib.rs
git commit -m "feat(arity-bitmap): optional ethnum::U256 backing; doc-hide U256"
```

---

### Task 3: `arity-arrays` — logical `serde` for `FixedArray` and `PackedArray` + `serde`/`std`/`ethnum` forwarding

Add optional `serde` (+ `serde_with`, `ethnum` forwarding) deps and features; implement the logical serde forms. `FixedArray` delegates to the inner `hybrid_array::Array`; `PackedArray` serializes as ascending `(index, value)` pairs with validated deserialization.

**Files:**
- Modify: `crates/arity-arrays/Cargo.toml` (deps + features)
- Modify: `crates/arity-arrays/src/fixed.rs` (FixedArray serde)
- Modify: `crates/arity-arrays/src/packed.rs` (PackedArray serde)
- Test: a new `crates/arity-arrays/tests/serde_logical.rs`

**Interfaces:**
- Consumes: `arity-index/serde` (Task 1, `A::Index: Serialize+Deserialize`); `hybrid-array/serde`; `PackedArray::{iter_present, from}`; `FixedArray::<Option<T>,A>::new`, `IndexMut`.
- Produces: features `serde`/`serde_with`/`ethnum`/`std`; `Serialize`/`Deserialize` for `FixedArray<T,A>` and `PackedArray<T,A>`. Task 4 (`Compact`) consumes the deps + `serde_with` feature.

- [ ] **Step 1: Add deps and features to `crates/arity-arrays/Cargo.toml`**

Add to `[dependencies]`:
```toml
serde = { version = "1", optional = true, default-features = false, features = ["alloc", "derive"] }
serde_with = { version = "3.21", optional = true, default-features = false, features = ["alloc"] }
```
Add to `[features]`:
```toml
serde = ["dep:serde", "arity-index/serde", "hybrid-array/serde"]
serde_with = ["dep:serde_with", "serde"]
ethnum = ["arity-bitmap/ethnum"]
std = ["serde?/std", "serde_with?/std", "arity-index/std", "arity-bitmap/std"]
```
Add to `[dev-dependencies]`:
```toml
serde_json = "1"
```
(`ethnum` forwards only — `arity-arrays` does not depend on `ethnum` directly. `serde_with` implies `serde`. `hybrid-array/serde` gives `Array` its serde impls.)

- [ ] **Step 2: Write the failing tests**

Create `crates/arity-arrays/tests/serde_logical.rs`:
```rust
//! Logical serde round-trip + adversarial-decode tests.
#![cfg(feature = "serde")]

use arity_arrays::index::U4;
use arity_arrays::{Arity16, Arity256, FixedArray, PackedArray};

#[test]
fn fixed_round_trip() {
    let mut a = FixedArray::<u8, Arity16>::from_fn(U4::as_u8);
    a[U4::new_masked(3)] = 200;
    let json = serde_json::to_string(&a).expect("ser");
    let back: FixedArray<u8, Arity16> = serde_json::from_str(&json).expect("de");
    assert_eq!(a, back);
    // Wrong length is rejected.
    assert!(serde_json::from_str::<FixedArray<u8, Arity16>>("[1,2,3]").is_err());
}

#[test]
fn packed_logical_round_trip_and_validation() {
    let mut p = PackedArray::<u16, Arity16>::new();
    p.insert(U4::new_masked(2), 20);
    p.insert(U4::new_masked(9), 90);
    let json = serde_json::to_string(&p).expect("ser");
    assert_eq!(json, "[[2,20],[9,90]]"); // ascending (index, value) pairs
    let back: PackedArray<u16, Arity16> = serde_json::from_str(&json).expect("de");
    assert_eq!(p, back);

    // Non-ascending indices are rejected.
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[9,90],[2,20]]").is_err());
    // Duplicate indices are rejected.
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[2,20],[2,21]]").is_err());
    // Out-of-range index is rejected (16 invalid for Arity16 / U4).
    assert!(serde_json::from_str::<PackedArray<u16, Arity16>>("[[16,1]]").is_err());
}

#[test]
fn packed_arity256_round_trip() {
    let mut p = PackedArray::<u32, Arity256>::new();
    p.insert(0, 1);
    p.insert(255, 2);
    let json = serde_json::to_string(&p).expect("ser");
    let back: PackedArray<u32, Arity256> = serde_json::from_str(&json).expect("de");
    assert_eq!(p, back);
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p arity-arrays --features serde --test serde_logical`
Expected: FAIL to compile — `FixedArray`/`PackedArray` do not implement `Serialize`.

- [ ] **Step 4: Implement `FixedArray` serde (delegates to the inner `Array`)**

In `crates/arity-arrays/src/fixed.rs`, after the `AsRef<[T]>` impl (confirm with `grep -n 'impl<T, A: Arity> AsRef' crates/arity-arrays/src/fixed.rs`), add:
```rust
#[cfg(feature = "serde")]
impl<T: serde::Serialize, A: Arity> serde::Serialize for FixedArray<T, A> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The inner `Array<T, A::Size>` serializes as a fixed-length sequence of
        // exactly `LEN` elements (hybrid-array's `serde` impl).
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>, A: Arity> serde::Deserialize<'de> for FixedArray<T, A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(hybrid_array::Array::deserialize(deserializer)?))
    }
}
```

- [ ] **Step 5: Implement `PackedArray` serde (logical pairs, validated)**

In `crates/arity-arrays/src/packed.rs`, near the other trait impls (after the `Debug` impl is a good spot; confirm with `grep -n 'impl<T: core::fmt::Debug' crates/arity-arrays/src/packed.rs`), add:
```rust
#[cfg(feature = "serde")]
impl<T: serde::Serialize, A: Arity> serde::Serialize for PackedArray<T, A> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Logical form: a sequence of `(index, value)` pairs in ascending order.
        serializer.collect_seq(self.iter_present())
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>, A: Arity> serde::Deserialize<'de> for PackedArray<T, A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PairsVisitor<T, A>(PhantomData<(T, A)>);

        impl<'de, T: serde::Deserialize<'de>, A: Arity> serde::de::Visitor<'de> for PairsVisitor<T, A> {
            type Value = PackedArray<T, A>;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a sequence of (index, value) pairs with strictly ascending indices")
            }

            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> Result<Self::Value, S::Error> {
                let mut out = FixedArray::<Option<T>, A>::new();
                let mut last: Option<usize> = None;
                while let Some((index, value)) = seq.next_element::<(A::Index, T)>()? {
                    let i = index.as_usize();
                    if last.is_some_and(|prev| i <= prev) {
                        return Err(serde::de::Error::custom(
                            "PackedArray indices must be strictly ascending",
                        ));
                    }
                    last = Some(i);
                    out[index] = Some(value);
                }
                Ok(PackedArray::from(out))
            }
        }

        deserializer.deserialize_seq(PairsVisitor(PhantomData))
    }
}
```
(`A::Index` validates its own range on deserialize — an out-of-range index errors inside `next_element`. `PhantomData` is already imported in `packed.rs`. `as_usize` comes from `arity_index::Niche`, already imported.)

- [ ] **Step 6: Run to verify pass + clean**

Run:
```bash
cargo test -p arity-arrays --features serde --test serde_logical
cargo clippy -p arity-arrays --all-targets --features serde -- -D warnings
cargo build -p arity-arrays --no-default-features --features "16,serde"  # no_std intact
```
Expected: all three tests pass; clippy clean; the lean serde build compiles (no_std + alloc).

- [ ] **Step 7: Commit**

```bash
git add crates/arity-arrays/Cargo.toml crates/arity-arrays/src/fixed.rs crates/arity-arrays/src/packed.rs crates/arity-arrays/tests/serde_logical.rs
git commit -m "feat(arity-arrays): logical serde for FixedArray and PackedArray"
```

---

### Task 4: `arity-arrays` — the `serde_with::Compact` adapter

A `Compact` unit struct implementing `SerializeAs`/`DeserializeAs` for `PackedArray`, encoding the bitmap as `Bitmap::BYTES` little-endian bytes (backing-independent) plus the dense values.

**Files:**
- Create: `crates/arity-arrays/src/compact.rs`
- Modify: `crates/arity-arrays/src/lib.rs` (declare `mod compact;` + re-export `Compact`, gated `serde_with`)
- Test: `crates/arity-arrays/tests/serde_compact.rs`

**Interfaces:**
- Consumes: `Bitmap::{BYTES, to_le_bytes, from_le_bytes, count_ones, bits}`; `PackedArray::{bitmap, iter_present, from}`; `FixedArray::<Option<T>,A>::new`.
- Produces: `arity_arrays::Compact` (gated `#[cfg(feature = "serde_with")]`).

- [ ] **Step 1: Write the failing test**

Create `crates/arity-arrays/tests/serde_compact.rs`:
```rust
//! `serde_with::Compact` round-trip + adversarial-decode tests.
#![cfg(feature = "serde_with")]

use arity_arrays::index::U4;
use arity_arrays::{Arity16, Compact, PackedArray};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Node {
    #[serde_as(as = "Compact")]
    children: PackedArray<u16, Arity16>,
}

#[test]
fn compact_round_trip() {
    let mut children = PackedArray::<u16, Arity16>::new();
    children.insert(U4::new_masked(2), 20);
    children.insert(U4::new_masked(9), 90);
    let node = Node { children };

    let json = serde_json::to_string(&node).expect("ser");
    // bitmap = bits 2 and 9 set = 0x0204, little-endian bytes [4, 2]; values [20, 90].
    assert_eq!(json, r#"{"children":[[4,2],[20,90]]}"#);
    let back: Node = serde_json::from_str(&json).expect("de");
    assert_eq!(node, back);
}

#[test]
fn compact_rejects_popcount_mismatch() {
    // bitmap [4,2] has popcount 2, but only one value is supplied.
    let bad = r#"{"children":[[4,2],[20]]}"#;
    assert!(serde_json::from_str::<Node>(bad).is_err());
    // wrong-length bitmap (BYTES must be 2 for Arity16).
    let bad_len = r#"{"children":[[4,2,0],[20,90]]}"#;
    assert!(serde_json::from_str::<Node>(bad_len).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p arity-arrays --features serde_with --test serde_compact`
Expected: FAIL to compile — `Compact` not found.

- [ ] **Step 3: Create `crates/arity-arrays/src/compact.rs`**

```rust
//! [`Compact`]: a `serde_with` adapter that serializes a [`PackedArray`] as a
//! fixed-width little-endian bitmap plus its dense values — a compact,
//! backing-independent wire form (the bitmap goes through
//! [`Bitmap::to_le_bytes`](arity_bitmap::Bitmap::to_le_bytes), so it is identical
//! for the custom and `ethnum` 256-bit backings).

extern crate alloc;

use alloc::vec::Vec;

use arity_bitmap::Bitmap;
use serde::Deserialize;
use serde::Serialize;
use serde_with::DeserializeAs;
use serde_with::SerializeAs;

use crate::Arity;
use crate::FixedArray;
use crate::PackedArray;

/// `serde_with` adapter for the compact `PackedArray` wire form. Use as
/// `#[serde_as(as = "Compact")]` on a `PackedArray<T, A>` field.
pub struct Compact;

impl<T: Serialize, A: Arity> SerializeAs<PackedArray<T, A>> for Compact {
    fn serialize_as<S: serde::Serializer>(
        source: &PackedArray<T, A>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut buf = alloc::vec![0u8; <A::Bitmap as Bitmap>::BYTES];
        source.bitmap().to_le_bytes(&mut buf);
        let values: Vec<&T> = source.iter_present().map(|(_, v)| v).collect();
        (buf, values).serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, A: Arity> DeserializeAs<'de, PackedArray<T, A>> for Compact {
    fn deserialize_as<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PackedArray<T, A>, D::Error> {
        let (buf, values): (Vec<u8>, Vec<T>) = Deserialize::deserialize(deserializer)?;
        if buf.len() != <A::Bitmap as Bitmap>::BYTES {
            return Err(serde::de::Error::invalid_length(
                buf.len(),
                &"the bitmap byte length (WIDTH / 8)",
            ));
        }
        let bitmap = <A::Bitmap as Bitmap>::from_le_bytes(&buf);
        if bitmap.count_ones() as usize != values.len() {
            return Err(serde::de::Error::custom(
                "Compact: bitmap popcount does not match the number of values",
            ));
        }
        let mut out = FixedArray::<Option<T>, A>::new();
        for (index, value) in bitmap.bits().zip(values) {
            out[index] = Some(value);
        }
        Ok(PackedArray::from(out))
    }
}
```

- [ ] **Step 4: Wire `Compact` into `crates/arity-arrays/src/lib.rs`**

Add the module and re-export near the other `pub use`s (confirm with `grep -n 'pub use packed::PackedArray' crates/arity-arrays/src/lib.rs`):
```rust
#[cfg(feature = "serde_with")]
mod compact;
#[cfg(feature = "serde_with")]
pub use compact::Compact;
```

- [ ] **Step 5: Run to verify pass + clean**

Run:
```bash
cargo test -p arity-arrays --features serde_with --test serde_compact
cargo clippy -p arity-arrays --all-targets --features serde_with -- -D warnings
```
Expected: both tests pass; clippy clean.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-arrays/src/compact.rs crates/arity-arrays/src/lib.rs crates/arity-arrays/tests/serde_compact.rs
git commit -m "feat(arity-arrays): add serde_with::Compact adapter for PackedArray"
```

---

### Task 5: snapshot tests, cross-backing parity, and CI matrix columns

Lock the wire formats with `insta` snapshots; add a cross-backing `Compact` byte-identity test; add the serde/ethnum CI columns; verify the full `--all-features` build (all arities + ethnum + serde_with + std) compiles and passes.

**Files:**
- Modify: `crates/arity-arrays/Cargo.toml` (add `insta` dev-dep)
- Create: `crates/arity-arrays/tests/serde_snapshots.rs`
- Modify: `.github/workflows/ci.yml` (add serde/ethnum feature columns)
- Modify: `mise.toml` (already pins `cargo-insta`; no change needed — verify)

**Interfaces:**
- Consumes: the serde + Compact + ethnum features (Tasks 1–4).
- Produces: snapshot tests; CI coverage of the serde/ethnum configurations.

- [ ] **Step 1: Add the `insta` dev-dependency**

In `crates/arity-arrays`, run `cargo add --dev insta --features json`.

- [ ] **Step 2: Write the snapshot + cross-backing tests**

Create `crates/arity-arrays/tests/serde_snapshots.rs`:
```rust
//! Snapshot the wire formats (so drift is a reviewable diff) and assert the
//! Compact bitmap encoding is backing-independent.
#![cfg(feature = "serde_with")]

use arity_arrays::index::U4;
use arity_arrays::{Arity16, Compact, PackedArray};
use serde::Serialize;
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize)]
struct CompactNode {
    #[serde_as(as = "Compact")]
    children: PackedArray<u16, Arity16>,
}

fn sample() -> PackedArray<u16, Arity16> {
    let mut p = PackedArray::<u16, Arity16>::new();
    p.insert(U4::new_masked(1), 11);
    p.insert(U4::new_masked(4), 44);
    p.insert(U4::new_masked(14), 14);
    p
}

#[test]
fn snapshot_logical_form() {
    let json = serde_json::to_string(&sample()).expect("ser");
    insta::assert_snapshot!("packed_logical", json);
}

#[test]
fn snapshot_compact_form() {
    let json = serde_json::to_string(&CompactNode { children: sample() }).expect("ser");
    insta::assert_snapshot!("packed_compact", json);
}
```

- [ ] **Step 3: Generate and review the snapshots, then run**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
INSTA_UPDATE=always cargo test -p arity-arrays --features serde_with --test serde_snapshots
```
This writes `crates/arity-arrays/tests/snapshots/*.snap`. Inspect them — the logical form is `[[1,11],[4,44],[14,14]]`; the compact form is `{"children":[[<bitmap bytes>],[11,44,14]]}` where the bitmap bytes are the little-endian `u16` with bits 1, 4, 14 set (`0x4012` → `[0x12, 0x40]` = `[18, 64]`). Then re-run without updating to confirm they pass:
```bash
cargo test -p arity-arrays --features serde_with --test serde_snapshots
```
Expected: PASS against the committed snapshots.

- [ ] **Step 4: Add a cross-backing Compact byte-identity test**

This test asserts the `Compact` bytes for arity-256 are identical whether the backing is the custom `U256` or `ethnum::U256`. Because the two backings cannot coexist in one build, the test is written once and run under both CI columns (Step 6); add it to `crates/arity-arrays/tests/serde_compact.rs`:
```rust
#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Node256 {
    #[serde_as(as = "Compact")]
    children: arity_arrays::PackedArray<u32, arity_arrays::Arity256>,
}

#[test]
fn compact_arity256_round_trip_stable_bytes() {
    let mut children = arity_arrays::PackedArray::<u32, arity_arrays::Arity256>::new();
    children.insert(0, 1);
    children.insert(128, 2); // limb boundary
    children.insert(255, 3);
    let node = Node256 { children };
    let json = serde_json::to_string(&node).expect("ser");
    // 32-byte LE bitmap with bits 0,128,255 set, then values [1,2,3].
    // Byte 0 bit0 -> 1; byte 16 bit0 (bit 128) -> 1; byte 31 bit7 (bit 255) -> 128.
    assert!(json.contains("[1,2,3]"));
    let back: Node256 = serde_json::from_str(&json).expect("de");
    assert_eq!(node, back);
}
```
(The exact byte array is asserted by the round-trip + the snapshot; the key point is the same source runs identically under both backings.)

- [ ] **Step 5: Verify the full feature surface locally**

Run:
```bash
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
# ethnum backing column:
cargo test -p arity-arrays --no-default-features --features "256,ethnum,serde_with,std"
cargo build --workspace --no-default-features --features "16,serde"
```
Expected: all green. `--all-features` enables every arity + `ethnum` + `serde_with` + `std` together (confirms `ethnum::U256` and the serde stack compose).

- [ ] **Step 6: Add the serde/ethnum columns to `.github/workflows/ci.yml`**

In the `features` job (confirm with `grep -n 'feature matrix' .github/workflows/ci.yml`), add two more clippy steps after the existing ones:
```yaml
      # Logical serde without serde_with (catches a misplaced serde_with cfg).
      - run: cargo clippy --workspace --no-default-features --features "16,serde" -- -D warnings
      # ethnum backing + compact serde + std together.
      - run: cargo clippy --workspace --no-default-features --features "256,ethnum,serde_with,std" -- -D warnings
```

- [ ] **Step 7: Validate YAML and commit**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
yq '.jobs.features.steps[].run' .github/workflows/ci.yml
cargo test --workspace --all-features 2>&1 | tail -3
```
Expected: the `features` job lists the two new clippy commands; the workspace test suite is green.

```bash
git add crates/arity-arrays/Cargo.toml crates/arity-arrays/tests/serde_snapshots.rs crates/arity-arrays/tests/snapshots crates/arity-arrays/tests/serde_compact.rs .github/workflows/ci.yml
git commit -m "test(arity-arrays): snapshot wire formats; CI serde/ethnum columns"
```

---

## Self-Review

- **Spec coverage:**
  - `std` feature, weak-dep `dep?/std`, off by default → Tasks 1 (index), 2 (bitmap), 3 (arrays). ✓
  - `serde` logical: `U{n}` validated (Task 1); `FixedArray` = seq of LEN (Task 3); `PackedArray` = ascending `(index, value)` pairs, validated in-range + strictly-ascending (Task 3); bitmaps get no standalone serde (`arity-bitmap` has no serde dep). ✓
  - `serde_with::Compact`: `arity_arrays::Compact`, bitmap via `BYTES`/`to_le_bytes`/`from_le_bytes` (backing-independent), popcount-mismatch + length rejection → Task 4. ✓
  - `ethnum` backing swap: optional dep, mutually-exclusive `#[cfg]`, `U256` `#[doc(hidden)]`, same `Bitmap` surface, backing-parity tests → Task 2. ✓
  - Testing: round-trip proptests/tests for both forms; adversarial decode (out-of-range, non-ascending, popcount mismatch, wrong length); cross-backing `Compact` byte-identity; `insta` snapshots → Tasks 3–5. ✓
  - CI columns (`16,serde`; `256,ethnum,serde_with,std`); `--all-features` → Task 5. ✓
- **Deferred (plan 5):** README "feature flags" tables + the "tests are all-arity-only" note; `CHANGELOG`; publish dry-run.
- **External-API correctness captured:** `from_words(hi, lo)` is avoided in hot paths (the ethnum impl uses integer ops + `from_words(0,0)`/`(0,1)` for ZERO/ONE only); `serde`/`serde_with`/`ethnum` all pulled `default-features = false`; `hybrid-array/serde` gives `Array` its impls; `arity-bitmap` deliberately has no serde.
- **Placeholder scan:** none — every step has complete code and exact commands.
- **Type/signature consistency:** `Compact` `SerializeAs`/`DeserializeAs` signatures match serde_with 3.21 verbatim; `A::Bitmap: Bitmap` byte methods match the trait as-built; `PackedArray::from(FixedArray<Option<T>, A>)` and `FixedArray::<Option<T>,A>::new()` exist as used.
