# Hardening Plan 1 — Breaking API Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finalize the `arity-index` and `arity-bitmap` public API — the additions that must land before the first crates.io publish because they are breaking — so later plans (mutation, serde) build on a complete surface.

**Architecture:** Extend the existing sealed `Niche` and `Bitmap` traits. `arity-index` gains infallible `From<U{n}>` conversions and rustdoc safety-invariant sections on its range iterators. `arity-bitmap` gains three `Bitmap` operations (`without_bit`, `select`, and a backing-independent little-endian byte surface `BYTES`/`to_le_bytes`/`from_le_bytes`), plus `U256: Hash` and a `pub(crate) const fn from_limbs` helper.

**Tech Stack:** Rust (edition 2024, `#![no_std]`, no `alloc`), `seq-macro` (already present). No new dependencies.

This is **plan 1 of 5** for the production-hardening effort
(`breaking-api` → `features-ci` → `mutation` → `serde-ethnum` → `publish`).
Design spec: `docs/superpowers/specs/2026-06-27-arity-arrays-hardening-design.md`
(sections "Completed API surface" and the `Bitmap` trait additions). It touches
only `arity-index` and `arity-bitmap`; the `/// # Safety` docs on `PackedArray`
and `PackedAllIter` belong to the mutation plan (plan 3) and are out of scope
here.

## Global Constraints

Copied verbatim from the spec and the existing crate conventions; every task's
requirements implicitly include this section.

- **`#![no_std]`, no `alloc`** in both `arity-index` and `arity-bitmap`. Use
  `core::` paths only. `arity-bitmap` **contains no `unsafe`** today and this plan
  keeps it that way.
- **`unsafe` discipline (workspace):** every `unsafe` block carries a `// SAFETY:`
  comment; `undocumented_unsafe_blocks` and `unsafe_op_in_unsafe_fn` are `deny`.
  This plan adds no new `unsafe`.
- **Lints (workspace, warnings denied in CI on `--all-targets`):**
  `clippy::pedantic` + `clippy::nursery` at `warn`, `clippy::unwrap_used` at
  `warn`. **No `.unwrap()`** in lib or tests — use `.expect("…")` (allowed) or
  pattern matching. No `#[allow]`; use `#[expect(reason = "…")]` scoped to the
  smallest expression only where unavoidable. Satisfy nursery `#[must_use]` /
  `const fn` suggestions.
- **Edition 2024; MSRV 1.92.** Everything used here is well under 1.92.
- **`From<U{n}>` applies to the generated niche types only.** The native `u8`
  (arity-256 index) already converts to `u8`/`usize`; do not add impls for it.
- **`select` is a provided (default) trait method** built on the existing
  `bits()` iterator (`O(popcount)`-bounded; arities are ≤ 256). `without_bit` and
  the byte methods are per-backing required methods.
- Comments and commit messages: imperative mood, conventional-commit style.

---

### Task 1: `arity-index` — infallible `From<U{n}>` conversions

Add `From<U{n}> for u8` and `From<U{n}> for usize` to the `niche_int!` macro so
the five generated index types satisfy `Into<u8>` / `Into<usize>` bounds. The
value is provably in range, so the conversions are infallible.

**Files:**
- Modify: `crates/arity-index/src/niche.rs` (inside `macro_rules! niche_int`, after the `TryFrom<u8>` impl at lines 177-183)
- Test: `crates/arity-index/src/niche.rs` (the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: the existing `$name::as_u8(self) -> u8` and `as_usize(self) -> usize` inherent methods.
- Produces: `impl From<U3|U4|U5|U6|U7> for u8` and `… for usize`. Later plans rely on `usize::from(index)` and `u8::from(index)` being available.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/arity-index/src/niche.rs`:

```rust
#[test]
fn into_u8_and_usize() {
    // Infallible widening conversions for generic `Into` bounds.
    assert_eq!(u8::from(U4::MAX), 15u8);
    assert_eq!(usize::from(U4::MAX), 15usize);
    assert_eq!(u8::from(U7::MAX), 127u8);
    assert_eq!(usize::from(U3::MIN), 0usize);

    // Usable through a generic `Into<usize>` bound.
    fn take(i: impl Into<usize>) -> usize {
        i.into()
    }
    assert_eq!(take(U5::new_masked(5)), 5usize);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p arity-index into_u8_and_usize`
Expected: FAIL to compile — `the trait bound `u8: From<U4>` is not satisfied`.

- [ ] **Step 3: Add the conversions to the macro**

In `crates/arity-index/src/niche.rs`, immediately after the `impl ::core::convert::TryFrom<u8> for $name { … }` block (currently ending at line 183), add:

```rust
        impl ::core::convert::From<$name> for u8 {
            fn from(v: $name) -> u8 {
                v.as_u8()
            }
        }

        impl ::core::convert::From<$name> for usize {
            fn from(v: $name) -> usize {
                v.as_usize()
            }
        }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p arity-index into_u8_and_usize`
Expected: PASS.

- [ ] **Step 5: Verify the whole crate is clean**

Run: `cargo test -p arity-index && cargo clippy -p arity-index --all-targets -- -D warnings`
Expected: all green, no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): add infallible From<U{n}> for u8 and usize"
```

---

### Task 2: `arity-index` — `/// # Safety` invariant sections on range iterators

Document the internal invariant that the `unsafe { … unwrap_unchecked() }` calls
in `NicheRange`/`NicheRangeInclusive` rely on, as rustdoc `# Safety` sections (the
standard `#safety` anchor) on the two public structs. Doc-only — no behavior
change.

**Files:**
- Modify: `crates/arity-index/src/range.rs:13-19` (`NicheRange` doc comment)
- Modify: `crates/arity-index/src/range.rs:73-80` (`NicheRangeInclusive` doc comment)

**Interfaces:**
- Consumes: nothing. Produces: nothing (documentation only).

- [ ] **Step 1: Add the `# Safety` section to `NicheRange`**

In `crates/arity-index/src/range.rs`, replace the `NicheRange` doc comment (line 13) with the expanded version below. (The `#[derive(Clone, Debug)]` and `pub struct NicheRange<N: Niche> {` lines are shown for context and are **not** changed.)

```rust
/// A half-open range `[start, end)` over the values of a [`Niche`] type.
///
/// # Safety
///
/// Internal invariant upheld by every method: the cursors `lo` and `hi` never
/// exceed `N::COUNT`, and a value is only reconstructed from a cursor proven
/// strictly `< N::COUNT` (`lo` when `lo < hi`, or `hi - 1` after a guarded
/// decrement). The `unsafe { N::try_from_usize(..).unwrap_unchecked() }` in the
/// iterator impls depends on this; any new method that mutates `lo`/`hi` must
/// preserve it.
#[derive(Clone, Debug)]
pub struct NicheRange<N: Niche> {
```

- [ ] **Step 2: Add the `# Safety` section to `NicheRangeInclusive`**

In the same file, replace the `NicheRangeInclusive` doc comment (line 73) with the expanded version below. (The `#[derive(Clone, Debug)]` and `pub struct NicheRangeInclusive<N: Niche> {` lines are shown for context and are **not** changed.)

```rust
/// A closed range `[start, end]` over the values of a [`Niche`] type.
///
/// # Safety
///
/// Internal invariant upheld by every method: the cursors `lo` and `hi` stay in
/// `[0, N::COUNT - 1]`, and `done` is `true` whenever the range is empty, so a
/// value is only reconstructed from a cursor proven `< N::COUNT`. The
/// `unsafe { N::try_from_usize(..).unwrap_unchecked() }` in the iterator impls
/// depends on this; any new method that mutates `lo`/`hi`/`done` must preserve
/// it.
#[derive(Clone, Debug)]
pub struct NicheRangeInclusive<N: Niche> {
```

- [ ] **Step 3: Verify docs build clean and tests still pass**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc -p arity-index --no-deps && cargo test -p arity-index`
Expected: docs build with no warnings; all tests pass (no behavior change).

- [ ] **Step 4: Commit**

```bash
git add crates/arity-index/src/range.rs
git commit -m "docs(arity-index): document range-iterator safety invariants"
```

---

### Task 3: `arity-bitmap` — `Bitmap::without_bit` and `Bitmap::select`

Add the inverse of `with_bit` (`without_bit`, per-backing required) and the
inverse of `rank` (`select`, a provided default method over `bits()`). The trait
addition and all six impls land together so the crate compiles.

**Files:**
- Modify: `crates/arity-bitmap/src/lib.rs:77-101` (the `Bitmap` trait)
- Modify: `crates/arity-bitmap/src/native.rs:54-79` (the `impl Bitmap for $ty` body in `impl_native_bitmap!`)
- Modify: `crates/arity-bitmap/src/u256.rs:91-136` (the `impl Bitmap for U256` body)
- Test: `crates/arity-bitmap/src/native.rs` and `crates/arity-bitmap/src/u256.rs` (existing `mod tests`)

**Interfaces:**
- Consumes: existing `Bitmap::with_bit`, `rank`, `count_ones`, `bits()`; `U256::split`.
- Produces: `Bitmap::without_bit(self, i: Self::Index) -> Self` and `Bitmap::select(self, n: u32) -> Option<Self::Index>`. The mutation plan (plan 3) consumes `without_bit` in `PackedArray::remove`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/arity-bitmap/src/native.rs`:

```rust
#[test]
fn without_bit_clears_one_bit() {
    let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(9));
    let cleared = bm.without_bit(u4(4));
    assert!(!cleared.test(u4(4)));
    assert!(cleared.test(u4(1)));
    assert!(cleared.test(u4(9)));
    assert_eq!(cleared.count_ones(), 2);
    // Clearing an unset bit is a no-op.
    assert_eq!(bm.without_bit(u4(2)), bm);
}

#[test]
fn select_is_inverse_of_rank() {
    let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(4)).with_bit(u4(9));
    assert_eq!(bm.select(0).map(U4::as_u8), Some(1));
    assert_eq!(bm.select(1).map(U4::as_u8), Some(4));
    assert_eq!(bm.select(2).map(U4::as_u8), Some(9));
    assert_eq!(bm.select(3), None);
    // select(rank(i)) == i for every set bit i.
    for i in bm.bits() {
        assert_eq!(bm.select(bm.rank(i)), Some(i));
    }
}
```

Add to `mod tests` in `crates/arity-bitmap/src/u256.rs`:

```rust
#[test]
fn without_bit_across_limbs() {
    let bm = U256::ZERO.with_bit(3).with_bit(127).with_bit(128).with_bit(254);
    let cleared = bm.without_bit(128);
    assert!(!cleared.test(128));
    assert!(cleared.test(127));
    assert!(cleared.test(254));
    assert_eq!(cleared.count_ones(), 3);
    assert_eq!(bm.without_bit(200), bm); // unset bit: no-op
}

#[test]
fn select_spans_limbs() {
    let bm = U256::ZERO.with_bit(3).with_bit(127).with_bit(128).with_bit(254);
    assert_eq!(bm.select(0), Some(3));
    assert_eq!(bm.select(1), Some(127));
    assert_eq!(bm.select(2), Some(128));
    assert_eq!(bm.select(3), Some(254));
    assert_eq!(bm.select(4), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p arity-bitmap without_bit select`
Expected: FAIL to compile — `no method named `without_bit`` / `select` found.

- [ ] **Step 3: Add the methods to the `Bitmap` trait**

In `crates/arity-bitmap/src/lib.rs`, inside `pub trait Bitmap`, after the `rank` method declaration (line 97, before the `bits` method), add:

```rust
    /// Returns `self` with the bit at `i` cleared (the inverse of
    /// [`with_bit`](Bitmap::with_bit)). Clearing an unset bit is a no-op.
    #[must_use]
    fn without_bit(self, i: Self::Index) -> Self;
    /// Returns the index of the `n`-th set bit (0-based), or `None` if
    /// `n >= count_ones()`. The inverse of [`rank`](Bitmap::rank):
    /// `select(rank(i)) == Some(i)` for every set `i`.
    ///
    /// Provided over [`bits`](Bitmap::bits); runs in `O(n)`.
    fn select(self, n: u32) -> Option<Self::Index> {
        self.bits().nth(n as usize)
    }
```

- [ ] **Step 4: Add `without_bit` to the native macro**

In `crates/arity-bitmap/src/native.rs`, inside `impl Bitmap for $ty`, after the `rank` method (line 78), add:

```rust
            fn without_bit(self, i: $idx) -> Self {
                self & !(1 << i.as_usize())
            }
```

- [ ] **Step 5: Add `without_bit` to the `U256` impl**

In `crates/arity-bitmap/src/u256.rs`, inside `impl Bitmap for U256`, after the `rank` method (line 135), add:

```rust
    fn without_bit(self, i: u8) -> Self {
        let (is_hi, bit) = Self::split(i);
        if is_hi {
            Self {
                lo: self.lo,
                hi: self.hi & !(1u128 << bit),
            }
        } else {
            Self {
                lo: self.lo & !(1u128 << bit),
                hi: self.hi,
            }
        }
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p arity-bitmap without_bit select`
Expected: PASS (all four tests).

- [ ] **Step 7: Verify the whole crate is clean**

Run: `cargo test -p arity-bitmap && cargo clippy -p arity-bitmap --all-targets -- -D warnings`
Expected: all green, no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/arity-bitmap/src/lib.rs crates/arity-bitmap/src/native.rs crates/arity-bitmap/src/u256.rs
git commit -m "feat(arity-bitmap): add Bitmap::without_bit and select"
```

---

### Task 4: `arity-bitmap` — backing-independent little-endian byte surface

Add `Bitmap::BYTES` (the `WIDTH / 8` byte length), `to_le_bytes` (write into a
caller buffer), and `from_le_bytes` (read from a buffer). This is what lets the
serde `Compact` adapter (plan 4) reconstruct any `A::Bitmap` generically without
naming a concrete `U256`. The `U256` impl uses a new `pub(crate) const fn
from_limbs`.

**Files:**
- Modify: `crates/arity-bitmap/src/lib.rs` (the `Bitmap` trait — add `BYTES`, `to_le_bytes`, `from_le_bytes`)
- Modify: `crates/arity-bitmap/src/native.rs` (the `impl Bitmap for $ty` body)
- Modify: `crates/arity-bitmap/src/u256.rs` (add `from_limbs`; impl the three trait items)
- Test: `crates/arity-bitmap/src/native.rs` and `crates/arity-bitmap/src/u256.rs`

**Interfaces:**
- Consumes: the primitives' inherent `to_le_bytes`/`from_le_bytes`; `U256` fields `lo`/`hi`.
- Produces: `const BYTES: usize`, `fn to_le_bytes(self, buf: &mut [u8])`, `fn from_le_bytes(buf: &[u8]) -> Self` on `Bitmap`; `U256::from_limbs(lo, hi)` (`pub(crate) const`). Plan 4's `Compact` adapter consumes all three trait items.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/arity-bitmap/src/native.rs`:

```rust
#[test]
fn le_bytes_round_trip_native() {
    assert_eq!(<u16 as Bitmap>::BYTES, 2);
    assert_eq!(<u8 as Bitmap>::BYTES, 1);
    assert_eq!(<u128 as Bitmap>::BYTES, 16);

    let bm = u16::ZERO.with_bit(u4(1)).with_bit(u4(9));
    let mut buf = [0u8; 2];
    bm.to_le_bytes(&mut buf);
    assert_eq!(buf, 0b0000_0010_0000_0010u16.to_le_bytes());
    assert_eq!(<u16 as Bitmap>::from_le_bytes(&buf), bm);
}
```

Add to `mod tests` in `crates/arity-bitmap/src/u256.rs`:

```rust
#[test]
fn le_bytes_round_trip_u256() {
    assert_eq!(<U256 as Bitmap>::BYTES, 32);
    let bm = U256::ZERO.with_bit(3).with_bit(127).with_bit(128).with_bit(254);
    let mut buf = [0u8; 32];
    bm.to_le_bytes(&mut buf);
    // bit 128 is the lowest bit of the high limb -> first byte of the second half.
    assert_eq!(buf[16], 0b0000_0001);
    assert_eq!(<U256 as Bitmap>::from_le_bytes(&buf), bm);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p arity-bitmap le_bytes`
Expected: FAIL to compile — `no associated item named `BYTES`` / `no method named `to_le_bytes``.

- [ ] **Step 3: Add the three items to the `Bitmap` trait**

In `crates/arity-bitmap/src/lib.rs`, inside `pub trait Bitmap`, after `const WIDTH: usize;` (line 81), add the const:

```rust
    /// The number of bytes in the little-endian byte form (`WIDTH / 8`).
    const BYTES: usize;
```

and after the `select` method added in Task 3 (before `bits`), add the two methods:

```rust
    /// Writes the bitmap as `BYTES` little-endian bytes into `buf`.
    ///
    /// `buf.len()` must equal [`BYTES`](Bitmap::BYTES); a wrong length panics.
    /// The byte form is backing-independent — it does not depend on the limb
    /// layout of any particular backing.
    fn to_le_bytes(self, buf: &mut [u8]);
    /// Reads a bitmap from `BYTES` little-endian bytes.
    ///
    /// `buf.len()` must equal [`BYTES`](Bitmap::BYTES); a wrong length panics.
    fn from_le_bytes(buf: &[u8]) -> Self;
```

- [ ] **Step 4: Add the byte surface to the native macro**

In `crates/arity-bitmap/src/native.rs`, inside `impl Bitmap for $ty`, add the const immediately after `const WIDTH: usize = $width;` (so `BYTES` sits next to `WIDTH`, matching the trait):

```rust
            const BYTES: usize = $width / 8;
```

and after the new `without_bit` body (before the closing `}` of `impl Bitmap for $ty`), add:

```rust
            fn to_le_bytes(self, buf: &mut [u8]) {
                // Inherent primitive method (1 arg) — unambiguous with the trait's.
                buf.copy_from_slice(&<$ty>::to_le_bytes(self));
            }

            fn from_le_bytes(buf: &[u8]) -> Self {
                let mut arr = [0u8; $width / 8];
                arr.copy_from_slice(buf);
                // Inherent primitive method (owned array arg) — unambiguous.
                <$ty>::from_le_bytes(arr)
            }
```

- [ ] **Step 5: Add `from_limbs` and the byte surface to `U256`**

In `crates/arity-bitmap/src/u256.rs`, inside `impl U256` (after `split`, line 30), add:

```rust
    /// Builds a `U256` from its two little-endian 128-bit limbs (`lo` is bits
    /// `0..128`, `hi` is bits `128..256`). Internal helper for the byte surface;
    /// not part of the public API.
    pub(crate) const fn from_limbs(lo: u128, hi: u128) -> Self {
        Self { lo, hi }
    }
```

and inside `impl Bitmap for U256`, add the const immediately after `const WIDTH: usize = 256;` (so `BYTES` sits next to `WIDTH`, matching the trait):

```rust
    const BYTES: usize = 32;
```

and after the new `without_bit` body (before the closing `}` of `impl Bitmap for U256`), add:

```rust
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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p arity-bitmap le_bytes`
Expected: PASS (both tests).

- [ ] **Step 7: Verify the whole crate is clean**

Run: `cargo test -p arity-bitmap && cargo clippy -p arity-bitmap --all-targets -- -D warnings`
Expected: all green, no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/arity-bitmap/src/lib.rs crates/arity-bitmap/src/native.rs crates/arity-bitmap/src/u256.rs
git commit -m "feat(arity-bitmap): add backing-independent BYTES/to_le_bytes/from_le_bytes"
```

---

### Task 5: `arity-bitmap` — derive `Hash` for `U256`

`u8`–`u128` implement `Hash` as primitives; `U256` does not. Add it so a generic
`B: Bitmap + Hash` bound works for every backing.

**Files:**
- Modify: `crates/arity-bitmap/src/u256.rs:11` (the `derive` on `U256`)
- Test: `crates/arity-bitmap/src/u256.rs` (`mod tests`)

**Interfaces:**
- Consumes: nothing. Produces: `U256: core::hash::Hash`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/arity-bitmap/src/u256.rs` (no `std`/`alloc` needed — a tiny `core::hash::Hasher` is defined inline):

```rust
#[test]
fn u256_is_hash() {
    use core::hash::{Hash, Hasher};

    // Minimal no_std hasher: XOR-folds written bytes.
    #[derive(Default)]
    struct XorHasher(u64);
    impl Hasher for XorHasher {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for &b in bytes {
                self.0 = self.0.rotate_left(8) ^ u64::from(b);
            }
        }
    }

    fn hash_of(v: U256) -> u64 {
        let mut h = XorHasher::default();
        v.hash(&mut h);
        h.finish()
    }

    let a = U256::ZERO.with_bit(3).with_bit(200);
    let b = U256::ZERO.with_bit(3).with_bit(200);
    let c = U256::ZERO.with_bit(4);
    assert_eq!(hash_of(a), hash_of(b)); // equal values hash equally
    assert_ne!(hash_of(a), hash_of(c)); // different values differ here
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p arity-bitmap u256_is_hash`
Expected: FAIL to compile — `the trait bound `U256: Hash` is not satisfied`.

- [ ] **Step 3: Add `Hash` to the derive**

In `crates/arity-bitmap/src/u256.rs`, change line 11 from:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
```

to:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p arity-bitmap u256_is_hash`
Expected: PASS.

- [ ] **Step 5: Verify the whole workspace is clean**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: all green, no warnings (confirms the `arity-index` and `arity-bitmap` changes compose).

- [ ] **Step 6: Commit**

```bash
git add crates/arity-bitmap/src/u256.rs
git commit -m "feat(arity-bitmap): derive Hash for U256"
```

---

## Self-Review

- **Spec coverage (Completed API surface):**
  - `Bitmap::without_bit` → Task 3. ✓
  - `Bitmap::select` → Task 3 (provided default). ✓
  - `Bitmap::to_le_bytes`/`from_le_bytes`/`BYTES` → Task 4. ✓
  - `From<U{n}> for u8`/`usize` → Task 1. ✓
  - `const fn` (feasible scope with a consumer): `U256::from_limbs` is `const` (Task 4); the index constructors were already `const`. ✓ Native inherent `const` wrappers are infeasible (foreign types); broader inherent `const` construction on `U256` (const `with_bit`/`from_le_bytes`) is **deferred** per the spec — no consumer needs it and it is additive/non-breaking to add later.
  - `U256: Hash` → Task 5. ✓
  - `/// # Safety` docs on `NicheRange`/`NicheRangeInclusive` → Task 2. ✓
  - `/// # Safety` docs on `PackedArray`/`PackedAllIter` → **deferred to plan 3** (mutation), which owns `packed.rs`. Noted in the header.
- **Out of scope here (later plans):** per-arity features, serde, `ethnum`, `from_limbs` being reached cross-crate (it is consumed only by `U256::from_le_bytes` in-crate; the `Compact` adapter uses the trait's `from_le_bytes`).
- **Placeholder scan:** none — every step has complete code and exact commands.
- **Type consistency:** `without_bit(self, Self::Index) -> Self`, `select(self, u32) -> Option<Self::Index>`, `to_le_bytes(self, &mut [u8])`, `from_le_bytes(&[u8]) -> Self`, `BYTES: usize`, `U256::from_limbs(u128, u128) -> Self` are used identically in the trait, the native macro, the `U256` impl, and the tests.
