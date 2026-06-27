# `arity-bitmap` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `arity-bitmap` crate — a sealed `Bitmap` trait keyed by a niche index type (`arity_index::Niche`), implemented for `u8`/`u16`/`u32`/`u64`/`u128` and a custom 256-bit `U256`, plus a double-ended `BitIter` that yields the set bits as the typed index.

**Architecture:** The public `Bitmap` trait is intentionally small (`type Index`, `WIDTH`, `ZERO`, `is_zero`, `count_ones`, `test`, `with_bit`, `rank`, `bits`). The low-level bit-scanning mechanics live on a **crate-private** `Raw` trait so they never reach the public surface; `BitIter<B: Raw>` is generic over `Raw`. Because every bit position is reconstructed through the statically-bounded `Niche` index, **the crate contains no `unsafe`** — index reconstruction uses a safe `.expect()` that can never fire.

**Tech Stack:** Rust (edition 2024, `#![no_std]`, no `alloc`), depends on `arity-index`; `proptest` (dev) for property tests.

This is **plan 2 of 3** (`arity-index` ✓ → **`arity-bitmap`** → `arity-arrays`). The design spec is `docs/superpowers/specs/2026-06-26-arity-arrays-design.md` ("`arity-bitmap`" section). `arity-index` is complete; this plan consumes its real API:

- `arity_index::Niche`: `const COUNT: usize`, `fn as_usize(self) -> usize`, `fn try_from_usize(i: usize) -> Option<Self>`, `fn all() -> NicheRangeInclusive<Self>` (sealed; `Copy + Ord`).
- Index types `arity_index::{U3, U4, U5, U6, U7}` and native `u8` all implement `Niche` with `COUNT` = 8/16/32/64/128/256 respectively.

## Key design decision (please confirm during plan review)

The bit-scanning primitives (`lowest set bit`, `highest set bit`, `clear bit`) are kept **off** the public `Bitmap` trait, per the spec ("`trailing_zeros`/`clear_lowest` are internal details of `BitIter`"). They live on a crate-private `Raw` trait. The chosen mechanism — `pub struct BitIter<B: Raw>` with a private-trait bound, and `Bitmap::bits()` returning `BitIter<Self> where Self: Raw<…>` — has been verified to compile cleanly (private trait in a generic bound / where-clause is permitted; only signature *types* and supertraits are restricted). The alternative (a per-backing associated iterator type) needs ~6 iterator definitions; the alternative (`#[doc(hidden)]` methods on `Bitmap`) leaves them technically public. The `Raw` split is the cleanest of the three.

## Global Constraints

- **`#![no_std]`, no `alloc`** in library code (`core::` only). Tests/doctests may use `alloc` locally (`extern crate alloc;` inside the test module/function).
- **No `unsafe` anywhere in this crate** (the spec mandates "entirely safe code"). Index reconstruction uses `Niche::try_from_usize(pos).expect("…")` — `pos` is provably `< WIDTH`, so it never panics. `clippy::unwrap_used` is denied-as-warning in CI, but `.expect()` is allowed (`clippy::expect_used` is not enabled).
- **Edition 2024; MSRV 1.85** (uses `u*::ilog2`, stable 1.67; `u*::trailing_zeros`/`count_ones` const).
- **Lints (already enforced workspace-wide):** `clippy::pedantic` + `clippy::nursery` (warn), `clippy::unwrap_used` (warn); CI runs `cargo clippy --all-targets --all-features` with warnings denied, so **tests must be clippy-clean** (no `.unwrap()`; prefer `assert_eq!` on `Option`). `#[must_use]` and `const fn` suggestions from nursery must be satisfied (add the attribute / make `const`).
- **Invariant (assert at compile time per impl):** `<B as Bitmap>::WIDTH == <B::Index as Niche>::COUNT`.
- **Clippy-clean integer idioms (verified):** `n.trailing_zeros() as usize` is lint-clean; use `n.ilog2()` for the highest set bit (clippy rejects `BITS-1 - leading_zeros()` as "manually reimplementing `ilog2`"); `x & (1 << k)` / `x & ((1 << k) - 1)` for test/rank are clean.
- Add deps with `cargo add`. Comments/commits: imperative mood, conventional-commit style.

---

### Task 1: Crate skeleton — `Raw` (private), `Bitmap` (public), `BitIter`

Define the trait/iterator structure so the crate compiles with no backing impls yet.

**Files:**
- Modify: `crates/arity-bitmap/src/lib.rs`
- Create: `crates/arity-bitmap/src/iter.rs`

**Interfaces:**
- Produces (public): `trait Bitmap: Copy + Eq + sealed::Sealed { type Index: Niche; const WIDTH: usize; const ZERO: Self; fn is_zero(self) -> bool; fn count_ones(self) -> u32; fn test(self, i: Self::Index) -> bool; fn with_bit(self, i: Self::Index) -> Self; fn rank(self, i: Self::Index) -> u32; fn bits(self) -> BitIter<Self> where Self: Raw<Index = <Self as Bitmap>::Index>; }`; `struct BitIter<B: Raw>`.
- Produces (crate-private): `trait Raw: Copy + Eq { type Index: Niche; fn raw_is_zero(self) -> bool; fn raw_popcount(self) -> u32; fn raw_lowest(self) -> Self::Index; fn raw_highest(self) -> Self::Index; fn raw_clear_lowest(self) -> Self; fn raw_clear_highest(self) -> Self; }`.

- [ ] **Step 1: Write `lib.rs`**

```rust
#![no_std]

//! Fixed-width bitmaps indexed by [`arity_index`] niche integers, with a
//! double-ended iterator over the set bits.
//!
//! The [`Bitmap`] trait is implemented for `u8`, `u16`, `u32`, `u64`, `u128`
//! (indexed by `U3`–`U7`) and the 256-bit [`U256`] (indexed by `u8`). The crate
//! contains no `unsafe` code: every bit position is reconstructed through the
//! statically-bounded [`arity_index::Niche`] index.

mod iter;

pub use iter::BitIter;

use arity_index::Niche;

mod sealed {
    /// Prevents downstream crates from implementing [`Bitmap`](crate::Bitmap).
    pub trait Sealed {}
}

/// Crate-private bit-scanning mechanics used by [`BitIter`]. Kept off the public
/// [`Bitmap`] surface deliberately. The `raw_lowest`/`raw_highest` methods have
/// the precondition `!self.raw_is_zero()`.
pub(crate) trait Raw: Copy + Eq {
    type Index: Niche;
    fn raw_is_zero(self) -> bool;
    fn raw_popcount(self) -> u32;
    fn raw_lowest(self) -> Self::Index;
    fn raw_highest(self) -> Self::Index;
    fn raw_clear_lowest(self) -> Self;
    fn raw_clear_highest(self) -> Self;
}

/// A fixed-width bitmap addressed by a [`Niche`] index type.
///
/// Sealed: implemented only by `u8`/`u16`/`u32`/`u64`/`u128` and [`U256`].
pub trait Bitmap: Copy + Eq + sealed::Sealed {
    /// The niche index type; `Index::COUNT == WIDTH`.
    type Index: Niche;
    /// The number of bits (`8`, `16`, `32`, `64`, `128`, or `256`).
    const WIDTH: usize;
    /// The empty bitmap.
    const ZERO: Self;

    /// Returns `true` if no bit is set.
    fn is_zero(self) -> bool;
    /// Returns the number of set bits.
    fn count_ones(self) -> u32;
    /// Returns `true` if the bit at `i` is set.
    fn test(self, i: Self::Index) -> bool;
    /// Returns `self` with the bit at `i` set.
    #[must_use]
    fn with_bit(self, i: Self::Index) -> Self;
    /// Returns the number of set bits strictly below `i` (the dense rank of `i`).
    fn rank(self, i: Self::Index) -> u32;
    /// Iterates over the set bits, ascending, as a double-ended iterator.
    fn bits(self) -> BitIter<Self>
    where
        Self: Raw<Index = <Self as Bitmap>::Index>,
    {
        BitIter::new(self)
    }
}
```

- [ ] **Step 2: Write `iter.rs`**

```rust
//! The double-ended set-bit iterator.

use crate::Raw;
use core::iter::FusedIterator;

/// Yields the set bits of a bitmap, ascending, as the bitmap's [`Niche`] index.
///
/// Holds a `Copy` snapshot of the bitmap and drains it from both ends.
///
/// [`Niche`]: arity_index::Niche
pub struct BitIter<B: Raw> {
    remaining: B,
}

impl<B: Raw> BitIter<B> {
    pub(crate) fn new(remaining: B) -> Self {
        Self { remaining }
    }
}

impl<B: Raw> Iterator for BitIter<B> {
    type Item = B::Index;

    fn next(&mut self) -> Option<B::Index> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let i = self.remaining.raw_lowest();
        self.remaining = self.remaining.raw_clear_lowest();
        Some(i)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining.raw_popcount() as usize;
        (n, Some(n))
    }
}

impl<B: Raw> DoubleEndedIterator for BitIter<B> {
    fn next_back(&mut self) -> Option<B::Index> {
        if self.remaining.raw_is_zero() {
            return None;
        }
        let i = self.remaining.raw_highest();
        self.remaining = self.remaining.raw_clear_highest();
        Some(i)
    }
}

impl<B: Raw> ExactSizeIterator for BitIter<B> {
    fn len(&self) -> usize {
        self.remaining.raw_popcount() as usize
    }
}

impl<B: Raw> FusedIterator for BitIter<B> {}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p arity-bitmap`
Expected: builds with no errors. (There are no `Bitmap`/`Raw` impls yet; the generic code compiles regardless.)

- [ ] **Step 4: Commit**

```bash
git add crates/arity-bitmap/src/lib.rs crates/arity-bitmap/src/iter.rs
git commit -m "feat(arity-bitmap): scaffold Bitmap/Raw traits and BitIter"
```

---

### Task 2: Native bitmap impls (`u8`–`u128`) via macro

Generate `Sealed`, `Raw`, and `Bitmap` for the five native unsigned integers, each paired with its niche index, with a compile-time `WIDTH == Index::COUNT` assertion.

**Files:**
- Create: `crates/arity-bitmap/src/native.rs`
- Modify: `crates/arity-bitmap/src/lib.rs` (add `mod native;`)

**Interfaces:**
- Consumes: `Raw`, `Bitmap`, `sealed::Sealed`, `arity_index::{Niche, U3, U4, U5, U6, U7}`.
- Produces: `impl Bitmap for u8` (Index = `U3`), `u16` (`U4`), `u32` (`U5`), `u64` (`U6`), `u128` (`U7`).

- [ ] **Step 1: Write the failing test**

Create `crates/arity-bitmap/src/native.rs` with the test module first (the impls in Step 3 come after):

```rust
//! `Bitmap`/`Raw` impls for the native unsigned integers `u8`..`u128`.

use crate::{sealed::Sealed, Bitmap, Raw};
use arity_index::{Niche, U3, U4, U5, U6, U7};

// (impl macro + invocations inserted here in Step 3)

#[cfg(test)]
mod tests {
    use super::*;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn width_and_zero() {
        assert_eq!(<u16 as Bitmap>::WIDTH, 16);
        assert_eq!(<u8 as Bitmap>::WIDTH, 8);
        assert_eq!(<u128 as Bitmap>::WIDTH, 128);
        assert_eq!(<u16 as Bitmap>::ZERO, 0u16);
        assert!(<u16 as Bitmap>::ZERO.is_zero());
        assert_eq!(<u16 as Bitmap>::ZERO.count_ones(), 0);
    }

    #[test]
    fn test_with_bit_and_count() {
        let bm = u16::ZERO.with_bit(u4(0)).with_bit(u4(7)).with_bit(u4(15));
        assert!(bm.test(u4(0)));
        assert!(bm.test(u4(7)));
        assert!(bm.test(u4(15)));
        assert!(!bm.test(u4(1)));
        assert!(!bm.test(u4(8)));
        assert_eq!(bm.count_ones(), 3);
        assert!(!bm.is_zero());
        assert_eq!(bm, 0b1000_0000_1000_0001u16);
    }

    #[test]
    fn rank_is_dense_index() {
        // bits 0, 7, 15 set: rank(0)=0, rank(7)=1, rank(15)=2.
        let bm = u16::ZERO.with_bit(u4(0)).with_bit(u4(7)).with_bit(u4(15));
        assert_eq!(bm.rank(u4(0)), 0);
        assert_eq!(bm.rank(u4(7)), 1);
        assert_eq!(bm.rank(u4(15)), 2);
        // rank counts bits strictly below i, regardless of whether i is set.
        assert_eq!(bm.rank(u4(8)), 2);
        assert_eq!(bm.rank(u4(1)), 1);
    }

    #[test]
    fn bits_forward_and_back() {
        let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(14));
        let fwd: alloc::vec::Vec<u8> = bm.bits().map(U4::as_u8).collect();
        assert_eq!(fwd, alloc::vec![1, 4, 14]);
        let back: alloc::vec::Vec<u8> = bm.bits().rev().map(U4::as_u8).collect();
        assert_eq!(back, alloc::vec![14, 4, 1]);
        assert_eq!(bm.bits().len(), 3);
        // meet in the middle
        let mut it = bm.bits();
        assert_eq!(it.next().map(U4::as_u8), Some(1));
        assert_eq!(it.next_back().map(U4::as_u8), Some(14));
        assert_eq!(it.next().map(U4::as_u8), Some(4));
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
    }

    #[test]
    fn edge_widths_u8_and_u128() {
        let b8 = u8::ZERO.with_bit(U3::MIN).with_bit(U3::MAX);
        assert_eq!(b8.count_ones(), 2);
        assert_eq!(b8.rank(U3::MAX), 1);
        assert_eq!(b8, 0b1000_0001u8);

        let b128 = u128::ZERO.with_bit(U7::MAX); // bit 127
        assert_eq!(b128.count_ones(), 1);
        assert_eq!(b128.rank(U7::MAX), 0);
        assert_eq!(b128, 1u128 << 127);
        let only: alloc::vec::Vec<u8> = b128.bits().map(U7::as_u8).collect();
        assert_eq!(only, alloc::vec![127]);
    }

    extern crate alloc;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-bitmap`
Expected: FAIL — `Bitmap`/`Raw` not implemented for the native ints (`with_bit`/`ZERO`/etc. unresolved).

- [ ] **Step 3: Write the macro and generate the impls**

Insert above the `#[cfg(test)]` module in `native.rs`:

```rust
/// Implements `Sealed` + `Raw` + `Bitmap` for a native unsigned integer `$ty`
/// (width `$width`) indexed by niche type `$idx` (with `$idx::COUNT == $width`).
macro_rules! impl_native_bitmap {
    ($ty:ty, $idx:ty, $width:literal) => {
        // Wire-up invariant: the index domain must equal the bit width.
        const _: () = assert!(<$idx as Niche>::COUNT == $width);

        impl Sealed for $ty {}

        impl Raw for $ty {
            type Index = $idx;

            fn raw_is_zero(self) -> bool {
                self == 0
            }

            fn raw_popcount(self) -> u32 {
                self.count_ones()
            }

            fn raw_lowest(self) -> $idx {
                // Precondition: self != 0, so trailing_zeros() < WIDTH == COUNT.
                <$idx>::try_from_usize(self.trailing_zeros() as usize)
                    .expect("nonzero bitmap has a lowest set bit < WIDTH")
            }

            fn raw_highest(self) -> $idx {
                // Precondition: self != 0, so ilog2() (highest set bit) < WIDTH.
                <$idx>::try_from_usize(self.ilog2() as usize)
                    .expect("nonzero bitmap has a highest set bit < WIDTH")
            }

            fn raw_clear_lowest(self) -> Self {
                self & self.wrapping_sub(1)
            }

            fn raw_clear_highest(self) -> Self {
                if self == 0 {
                    0
                } else {
                    self & !(1 << self.ilog2())
                }
            }
        }

        impl Bitmap for $ty {
            type Index = $idx;
            const WIDTH: usize = $width;
            const ZERO: Self = 0;

            fn is_zero(self) -> bool {
                self == 0
            }

            fn count_ones(self) -> u32 {
                <$ty>::count_ones(self)
            }

            fn test(self, i: $idx) -> bool {
                self & (1 << i.as_usize()) != 0
            }

            fn with_bit(self, i: $idx) -> Self {
                self | (1 << i.as_usize())
            }

            fn rank(self, i: $idx) -> u32 {
                let below = (1 << i.as_usize()) - 1;
                (self & below).count_ones()
            }
        }
    };
}

impl_native_bitmap!(u8, U3, 8);
impl_native_bitmap!(u16, U4, 16);
impl_native_bitmap!(u32, U5, 32);
impl_native_bitmap!(u64, U6, 64);
impl_native_bitmap!(u128, U7, 128);
```

Then add `mod native;` to `lib.rs` (after `mod iter;`).

> Note on `1 << i.as_usize()`: the literal `1` infers to `$ty`, so the shift is in
> the bitmap's own width. `i.as_usize() < COUNT == WIDTH`, so the shift never
> reaches the type width (no overflow). In `rank`, `(1 << k) - 1` for `k < WIDTH`
> is the low-`k` mask; for `k == WIDTH-1` it is `(top_bit) - 1`, still valid.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-bitmap`
Expected: PASS (all five tests).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p arity-bitmap --all-targets`
Expected: clean. (If nursery flags a `#[must_use]`/`const fn` on a `Raw`/`Bitmap` method, add it — but trait-method impls inherit the trait's attributes, so none should be needed.)

- [ ] **Step 6: Commit**

```bash
git add crates/arity-bitmap/src/native.rs crates/arity-bitmap/src/lib.rs
git commit -m "feat(arity-bitmap): implement Bitmap for u8..u128 via macro"
```

---

### Task 3: `U256` — the 256-bit bitmap

Add the two-limb `U256` type and its `Sealed`/`Raw`/`Bitmap` impls (indexed by `u8`). All two-limb math is plain safe code.

**Files:**
- Create: `crates/arity-bitmap/src/u256.rs`
- Modify: `crates/arity-bitmap/src/lib.rs` (`mod u256; pub use u256::U256;`)

**Interfaces:**
- Consumes: `Raw`, `Bitmap`, `sealed::Sealed`, `arity_index::Niche` (for `u8`).
- Produces: `pub struct U256 { lo: u128, hi: u128 }` with `impl Bitmap for U256` (Index = `u8`, WIDTH = 256), `Clone`, `Copy`, `PartialEq`, `Eq`, `Debug`, `Default`.

- [ ] **Step 1: Write the failing test**

Create `crates/arity-bitmap/src/u256.rs` with the test module first:

```rust
//! The 256-bit bitmap backing (`Bitmap::Index == u8`). Pure safe code.

use crate::{sealed::Sealed, Bitmap, Raw};
use arity_index::Niche;

// (struct + impls inserted in Step 3)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_zero_and_basic_bits() {
        assert_eq!(<U256 as Bitmap>::WIDTH, 256);
        assert!(U256::ZERO.is_zero());
        assert_eq!(U256::ZERO.count_ones(), 0);

        // bits spanning both limbs: 0 (lo), 127 (lo top), 128 (hi bottom), 255 (hi top)
        let bm = U256::ZERO
            .with_bit(0)
            .with_bit(127)
            .with_bit(128)
            .with_bit(255);
        assert_eq!(bm.count_ones(), 4);
        assert!(bm.test(0));
        assert!(bm.test(127));
        assert!(bm.test(128));
        assert!(bm.test(255));
        assert!(!bm.test(1));
        assert!(!bm.test(129));
    }

    #[test]
    fn rank_across_the_limb_boundary() {
        let bm = U256::ZERO.with_bit(0).with_bit(127).with_bit(128).with_bit(255);
        assert_eq!(bm.rank(0), 0);
        assert_eq!(bm.rank(127), 1);
        assert_eq!(bm.rank(128), 2); // bits 0 and 127 are below 128
        assert_eq!(bm.rank(255), 3);
        assert_eq!(bm.rank(200), 3); // 0,127,128 below 200
    }

    #[test]
    fn bits_forward_and_back_span_limbs() {
        let bm = U256::ZERO.with_bit(3).with_bit(127).with_bit(128).with_bit(254);
        let fwd: alloc::vec::Vec<u8> = bm.bits().collect();
        assert_eq!(fwd, alloc::vec![3u8, 127, 128, 254]);
        let back: alloc::vec::Vec<u8> = bm.bits().rev().collect();
        assert_eq!(back, alloc::vec![254u8, 128, 127, 3]);

        let mut it = bm.bits();
        assert_eq!(it.next(), Some(3));
        assert_eq!(it.next_back(), Some(254));
        assert_eq!(it.next(), Some(127));
        assert_eq!(it.next_back(), Some(128));
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
    }

    extern crate alloc;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-bitmap`
Expected: FAIL — `U256` does not exist.

- [ ] **Step 3: Write `U256` and its impls**

Insert above the `#[cfg(test)]` module in `u256.rs`:

```rust
/// A 256-bit bitmap: bit `i` lives in `lo` for `i < 128`, else in `hi` at
/// `i - 128`. Only the [`Bitmap`] surface is implemented (no arithmetic).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct U256 {
    lo: u128,
    hi: u128,
}

// Wire-up invariant: the u8 index domain (256) must equal the bit width.
const _: () = assert!(<u8 as Niche>::COUNT == 256);

impl U256 {
    /// Splits a bit index `i` (`< 256`) into `(limb_is_hi, bit_within_limb)`.
    const fn split(i: usize) -> (bool, u32) {
        if i < 128 {
            (false, i as u32)
        } else {
            (true, (i - 128) as u32)
        }
    }
}

impl Sealed for U256 {}

impl Raw for U256 {
    type Index = u8;

    fn raw_is_zero(self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    fn raw_popcount(self) -> u32 {
        self.lo.count_ones() + self.hi.count_ones()
    }

    fn raw_lowest(self) -> u8 {
        // Precondition: self != 0.
        let pos = if self.lo != 0 {
            self.lo.trailing_zeros() as usize
        } else {
            128 + self.hi.trailing_zeros() as usize
        };
        <u8 as Niche>::try_from_usize(pos).expect("lowest set bit < 256")
    }

    fn raw_highest(self) -> u8 {
        // Precondition: self != 0.
        let pos = if self.hi != 0 {
            128 + self.hi.ilog2() as usize
        } else {
            self.lo.ilog2() as usize
        };
        <u8 as Niche>::try_from_usize(pos).expect("highest set bit < 256")
    }

    fn raw_clear_lowest(self) -> Self {
        if self.lo != 0 {
            Self {
                lo: self.lo & self.lo.wrapping_sub(1),
                hi: self.hi,
            }
        } else {
            Self {
                lo: 0,
                hi: self.hi & self.hi.wrapping_sub(1),
            }
        }
    }

    fn raw_clear_highest(self) -> Self {
        if self.hi != 0 {
            Self {
                lo: self.lo,
                hi: self.hi & !(1u128 << self.hi.ilog2()),
            }
        } else if self.lo != 0 {
            Self {
                lo: self.lo & !(1u128 << self.lo.ilog2()),
                hi: 0,
            }
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
        let (is_hi, bit) = Self::split(i as usize);
        let limb = if is_hi { self.hi } else { self.lo };
        limb & (1u128 << bit) != 0
    }

    fn with_bit(self, i: u8) -> Self {
        let (is_hi, bit) = Self::split(i as usize);
        if is_hi {
            Self {
                lo: self.lo,
                hi: self.hi | (1u128 << bit),
            }
        } else {
            Self {
                lo: self.lo | (1u128 << bit),
                hi: self.hi,
            }
        }
    }

    fn rank(self, i: u8) -> u32 {
        let (is_hi, bit) = Self::split(i as usize);
        if is_hi {
            // all of lo, plus the bits of hi below `bit`
            let hi_mask = (1u128 << bit) - 1;
            self.lo.count_ones() + (self.hi & hi_mask).count_ones()
        } else {
            let lo_mask = (1u128 << bit) - 1;
            (self.lo & lo_mask).count_ones()
        }
    }
}
```

Then add `mod u256;` and `pub use u256::U256;` to `lib.rs`.

> Note: `i as usize` where `i: u8` is a widening cast (clippy-clean). `bit` is
> `0..128`, so `1u128 << bit` never overflows. In `rank`, when `bit == 0` the mask
> `(1 << 0) - 1 == 0` correctly counts zero bits below.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-bitmap`
Expected: PASS.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p arity-bitmap --all-targets`
Expected: clean. (If nursery suggests `#[must_use]` on `split`/`with_bit`, add it.)

- [ ] **Step 6: Commit**

```bash
git add crates/arity-bitmap/src/u256.rs crates/arity-bitmap/src/lib.rs
git commit -m "feat(arity-bitmap): add U256 256-bit bitmap backing"
```

---

### Task 4: Property tests against a reference model

Cross-check every backing (including `U256`) against an independent `BTreeSet<usize>` reference for `test`, `with_bit`, `rank`, `count_ones`, and `bits()` (both directions).

**Files:**
- Create: `crates/arity-bitmap/tests/proptests.rs`
- Modify: `crates/arity-bitmap/Cargo.toml` (add `proptest` dev-dependency via `cargo add`)

**Interfaces:**
- Consumes the public API: `Bitmap`, `BitIter`, `U256`, `arity_index::{Niche, U4, U7}`.

- [ ] **Step 1: Add the dev-dependency**

Run: `cargo add --package arity-bitmap --dev proptest`
Expected: `proptest` added under `[dev-dependencies]` in `crates/arity-bitmap/Cargo.toml`.

- [ ] **Step 2: Write the property tests**

Create `crates/arity-bitmap/tests/proptests.rs`:

```rust
//! Property tests: each `Bitmap` backing must agree with a `BTreeSet<usize>`
//! reference model for membership, rank, popcount, and ordered iteration.

use std::collections::BTreeSet;

use arity_bitmap::Bitmap;
use arity_index::Niche;
use proptest::prelude::*;

proptest! {
    #[test]
    fn u16_matches_model(indices in proptest::collection::vec(0usize..16, 0..16)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = u16::ZERO;
        for &i in &model {
            let idx = arity_index::U4::try_from_usize(i).expect("i < 16");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..16 {
            let idx = arity_index::U4::try_from_usize(i).expect("i < 16");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(|x| x.as_usize()).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(&fwd, &expected);
        let mut back: Vec<usize> = bm.bits().rev().map(|x| x.as_usize()).collect();
        back.reverse();
        prop_assert_eq!(&back, &expected);
    }

    #[test]
    fn u128_matches_model(indices in proptest::collection::vec(0usize..128, 0..128)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = u128::ZERO;
        for &i in &model {
            let idx = arity_index::U7::try_from_usize(i).expect("i < 128");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..128 {
            let idx = arity_index::U7::try_from_usize(i).expect("i < 128");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(|x| x.as_usize()).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(fwd, expected);
    }

    #[test]
    fn u256_matches_model(indices in proptest::collection::vec(0usize..256, 0..256)) {
        let model: BTreeSet<usize> = indices.iter().copied().collect();
        let mut bm = arity_bitmap::U256::ZERO;
        for &i in &model {
            let idx = u8::try_from_usize(i).expect("i < 256");
            bm = bm.with_bit(idx);
        }
        prop_assert_eq!(bm.count_ones() as usize, model.len());
        for i in 0usize..256 {
            let idx = u8::try_from_usize(i).expect("i < 256");
            prop_assert_eq!(bm.test(idx), model.contains(&i));
            let rank = model.iter().filter(|&&m| m < i).count();
            prop_assert_eq!(bm.rank(idx) as usize, rank);
        }
        let fwd: Vec<usize> = bm.bits().map(Niche::as_usize).collect();
        let expected: Vec<usize> = model.iter().copied().collect();
        prop_assert_eq!(fwd, expected);
    }
}
```

> The three `proptest!` blocks use only the public API (`ZERO`, `with_bit`,
> `test`, `rank`, `count_ones`, `bits`) — verified that `bits()` is callable from
> an external crate despite its private `Raw` bound. This is an integration test
> (`tests/` dir), so it links `std` — `Vec`/`BTreeSet` are fine.

- [ ] **Step 3: Run the property tests**

Run: `cargo test -p arity-bitmap --test proptests`
Expected: PASS (256 cases per property by default).

- [ ] **Step 4: Run clippy on the tests**

Run: `cargo clippy -p arity-bitmap --all-targets`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-bitmap/tests/proptests.rs crates/arity-bitmap/Cargo.toml
git commit -m "test(arity-bitmap): property tests against a BTreeSet reference model"
```

---

### Task 5: Crate doctest, doc build, and final gates

Add a crate-level doctest, build docs warning-free, and run the final clippy/fmt/Miri gates.

**Files:**
- Modify: `crates/arity-bitmap/src/lib.rs`

**Interfaces:**
- No new API. Documentation + verification only.

- [ ] **Step 1: Add a doctest to `lib.rs`**

Append to the crate-level `//!` doc block in `lib.rs`:

```rust
//!
//! ```
//! # extern crate alloc;
//! use arity_bitmap::Bitmap;
//! use arity_index::{Niche, U4};
//!
//! let bm = u16::ZERO
//!     .with_bit(U4::new_masked(1))
//!     .with_bit(U4::new_masked(4))
//!     .with_bit(U4::new_masked(9));
//!
//! assert_eq!(bm.count_ones(), 3);
//! assert!(bm.test(U4::new_masked(4)));
//! assert_eq!(bm.rank(U4::new_masked(4)), 1); // one set bit below index 4
//!
//! let set: alloc::vec::Vec<u8> = bm.bits().map(U4::as_u8).collect();
//! assert_eq!(set, alloc::vec![1, 4, 9]);
//! ```
```

- [ ] **Step 2: Run the doctest**

Run: `cargo test -p arity-bitmap --doc`
Expected: PASS.

- [ ] **Step 3: Build docs with warnings denied**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc -p arity-bitmap --no-deps`
Expected: builds with no warnings (`Raw`/`BitIter::new` are crate-private, so no broken public intra-doc links).

- [ ] **Step 4: Run the suite under Miri**

Run: `cargo +nightly miri test -p arity-bitmap`
Expected: PASS. (No `unsafe` in this crate, but Miri still validates the bit arithmetic and catches any accidental overflow/UB in the shift/`ilog2` paths.)

> If Miri is not installed: `rustup +nightly component add miri`, then if prompted
> `cargo +nightly miri setup`. The `proptests` integration test runs many cases
> under Miri and may be slow; that is expected — let it finish.

- [ ] **Step 5: Final clippy + fmt gate**

Run: `cargo clippy -p arity-bitmap --all-targets --all-features` then `cargo +nightly fmt --all --check`
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-bitmap/src/lib.rs
git commit -m "docs(arity-bitmap): add crate doctest and verify under Miri"
```

---

## Self-Review

**Spec coverage** (against the `arity-bitmap` section of the design spec):

- `Bitmap` trait keyed by `type Index: Niche`, taking `Self::Index` (not `usize`) for `test`/`with_bit`/`rank` → Tasks 1–3 ✓ (shift-UB precondition eliminated by the typed index).
- `WIDTH`, `ZERO`, `is_zero`, `count_ones`, `test`, `with_bit`, `rank`, `bits` → Tasks 1–3 ✓
- `trailing_zeros`/`clear_lowest` kept OFF the public trait (on the private `Raw` trait) → Task 1 ✓
- Double-ended `BitIter<B>` that takes a `Copy` of the bitmap and yields set bits as `B::Index`; `ExactSizeIterator` (`len == count_ones`) + `FusedIterator` → Task 1 ✓
- Impls for `u8`/`u16`/`u32`/`u64`/`u128` (indexed by `U3`–`U7`) → Task 2 ✓
- `U256` (`{lo, hi}`), bit-ops only, no arithmetic/`From`/`Display`, indexed by `u8` → Task 3 ✓
- `arity-bitmap` depends on `arity-index`; **entirely safe code** (no `unsafe`) → enforced throughout; index reconstruction via `.expect()` ✓
- `WIDTH == Index::COUNT` compile-time assertion per impl → Tasks 2, 3 ✓
- Property tests vs reference model for every backing incl. `U256`, both iteration directions → Task 4 ✓
- `#![no_std]`, no `alloc` in library code → enforced (tests/doctests use `alloc` locally) ✓

Not in this plan (later): the `Arity` trait, `FixedArray`, `PackedArray` (plan 3); CI workflow, package metadata, publish-flag removal (closing phase of plan 3).

**Placeholder scan:** none. The Task 4 `proptest!` blocks are complete and self-contained; no `TODO`/`TBD`/"sketch" code remains.

**Type consistency:** `Raw` method names (`raw_is_zero`/`raw_popcount`/`raw_lowest`/`raw_highest`/`raw_clear_lowest`/`raw_clear_highest`) are identical across `iter.rs`, `native.rs`, and `u256.rs`. `Bitmap` associated `Index`/`WIDTH`/`ZERO` and methods match between the trait (Task 1) and every impl (Tasks 2–3). `bits()` returns `BitIter<Self>` defined in Task 1.
