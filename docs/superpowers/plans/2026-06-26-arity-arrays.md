# `arity-arrays` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `arity-arrays` crate — the `Arity` trait + six marker types, the full-width `FixedArray<T, A>`, the pointer-sized heap-packed `PackedArray<T, A>`, and their conversions — then the workspace closing phase (CI, package metadata, MSRV bump, publish-flag removal).

**Architecture:** A sealed `Arity` trait ties an index type (`arity_index::Niche`), a bitmap (`arity_bitmap::Bitmap`), and a `hybrid_array::ArraySize` together for arities 8/16/32/64/128/256. `FixedArray` is a thin wrapper over `hybrid_array::Array` indexed without bounds checks. `PackedArray` is firewood's `DenseChildren` generalized: a pointer-sized `Option<NonNull<Inner>>` whose heap block is a `#[repr(C)]` header (`A::Bitmap`) + exactly the present elements, addressed by bitmap rank-select.

**Tech Stack:** Rust (edition 2024, `#![no_std]` + `extern crate alloc`), depends on `arity-index`, `arity-bitmap`, `hybrid-array`. `unsafe` is used heavily in `PackedArray` (raw alloc, pointer arithmetic, manual drop) under a strict documented-SAFETY discipline; `proptest` (dev) for property tests.

This is **plan 3 of 3** (`arity-index` ✓ → `arity-bitmap` ✓ → **`arity-arrays`**). Design spec: `docs/superpowers/specs/2026-06-26-arity-arrays-design.md` ("`arity-arrays`", "Continuous integration", "Publishing and package metadata"). The two dependency crates are complete; this plan consumes their **as-built** APIs:

- `arity_index::Niche`: `const COUNT: usize`, `fn as_usize(self) -> usize`, `fn try_from_usize(usize) -> Option<Self>`, `fn all() -> NicheRangeInclusive<Self>` (double-ended, exact-size). Index types `U3`,`U4`,`U5`,`U6`,`U7` (+ native `u8`) with `MIN`/`MAX`/`COUNT`/`new_masked`/`as_usize`.
- `arity_bitmap::Bitmap`: `type Index: Niche`, `const WIDTH: usize`, `const ZERO: Self`, `is_zero`, `count_ones`, `test(Index)`, `with_bit(Index)`, `rank(Index)`, `bits(self) -> BitIter<Self>` (yields `Index`; `DoubleEndedIterator + ExactSizeIterator + FusedIterator`). **`bits()` is callable on any `B: Bitmap` including generically** (verified). Backings: `u8`/`u16`/`u32`/`u64`/`u128`/`arity_bitmap::U256`.
- `hybrid_array::Array<T, U: ArraySize>` (`pub` tuple field `.0`): `Array::from_fn(|usize| T)`, `map`, `Deref`/`AsRef` to `[T]`, `IntoIterator` for `Array`/`&Array`/`&mut Array`, `Clone`/`Copy`/`Default`/`PartialEq`/`Eq`/`Hash` (conditional), `Send`/`Sync`. `ArraySize: typenum::Unsigned` ⇒ `<A::Size>::USIZE`. `hybrid_array::typenum::{U8,U16,U32,U64,U128,U256}`.

## Global Constraints

- **`#![no_std]` + `extern crate alloc;`** — already in `crates/arity-arrays/src/lib.rs`. Use `core::`/`alloc::` paths only; no `std`.
- **`unsafe` discipline:** every `unsafe` block carries a `// SAFETY:` comment naming the invariant. `undocumented_unsafe_blocks` and `unsafe_op_in_unsafe_fn` are `deny` (workspace). No `#[allow]`; `#[expect(reason = …)]` only where unavoidable, scoped tightly. The `unsafe` lives in `packed.rs` (and the one `get_unchecked` in `fixed.rs`); keep `arity.rs` `unsafe`-free.
- **Lints:** `clippy::pedantic` + `clippy::nursery` (warn), `clippy::unwrap_used` (warn); CI denies warnings on `--all-targets`. **No `.unwrap()`** in lib or tests — use `.expect("…")` (allowed; `expect_used` is not enabled) or pattern matching. Satisfy nursery `#[must_use]`/`const fn` suggestions.
- **Edition 2024; MSRV 1.85** for implementation (everything used — `&raw`, `Layout`, `NonNull` — is ≤ 1.82). The workspace MSRV bump to **1.92** is a policy change made in the closing task, not a technical requirement.
- **Keep `hybrid_array::Array` out of public function signatures** — `FixedArray` exposes `Deref<Target=[T]>`/`AsRef<[T]>` instead, so the `typenum` dependency can be retired later without a breaking change.
- **`PackedArray` is immutable after construction** (no `insert`/`remove`); mutate by round-tripping through `FixedArray`.
- Add deps with `cargo add`. Comments/commits: imperative mood, conventional-commit style.

---

### Task 1: The `Arity` trait + markers

Define the sealed `Arity` trait and the six zero-sized marker types, each wiring index ↔ bitmap ↔ size, with a compile-time `Index::COUNT == LEN == Bitmap::WIDTH == Size::USIZE` assertion.

**Files:**
- Create: `crates/arity-arrays/src/arity.rs`
- Modify: `crates/arity-arrays/src/lib.rs` (add `pub mod arity;`, re-exports, a crate-private `Sealed` trait)

**Interfaces:**
- Produces: `pub trait Arity` with `const LEN: usize`, `type Index: Niche`, `type Bitmap: Bitmap<Index = Self::Index>`, `type Size: ArraySize`; markers `pub enum Arity8 {} … Arity256 {}` implementing it.

- [ ] **Step 1: Write the failing test**

Create `crates/arity-arrays/src/arity.rs`:

```rust
//! The [`Arity`] trait and its marker types, wiring an index type, a bitmap
//! backing, and a `hybrid-array` size together for each supported width.

use arity_bitmap::Bitmap;
use arity_index::Niche;
use hybrid_array::ArraySize;
use hybrid_array::typenum::{U8, U16, U32, U64, U128, U256, Unsigned};

/// A power-of-two arity (8, 16, 32, 64, 128, or 256) that ties together a niche
/// index type, a bitmap backing, and a `hybrid-array` size.
///
/// Sealed: implemented only by the `Arity8` … `Arity256` markers in this crate.
#[expect(
    private_bounds,
    reason = "Sealed is an intentionally private supertrait that seals Arity \
              against downstream implementations"
)]
pub trait Arity: crate::Sealed {
    /// Number of slots.
    const LEN: usize;
    /// The niche index type (`U3`…`U7` or `u8`).
    type Index: Niche;
    /// The bitmap backing, whose `Index` must match `Self::Index`.
    type Bitmap: Bitmap<Index = Self::Index>;
    /// The `hybrid-array` size used by `FixedArray` (a typenum equal to `LEN`).
    type Size: ArraySize;
}

macro_rules! arity {
    ($name:ident, $len:literal, $index:ty, $bitmap:ty, $size:ty) => {
        #[doc = concat!("Arity ", stringify!($len), ".")]
        pub enum $name {}

        impl crate::Sealed for $name {}

        impl Arity for $name {
            const LEN: usize = $len;
            type Index = $index;
            type Bitmap = $bitmap;
            type Size = $size;
        }

        // Wiring invariant: index domain == slot count == bitmap width == size.
        const _: () = assert!(<$index as Niche>::COUNT == $len);
        const _: () = assert!(<$bitmap as Bitmap>::WIDTH == $len);
        const _: () = assert!(<$size as Unsigned>::USIZE == $len);
    };
}

arity!(Arity8, 8, arity_index::U3, u8, U8);
arity!(Arity16, 16, arity_index::U4, u16, U16);
arity!(Arity32, 32, arity_index::U5, u32, U32);
arity!(Arity64, 64, arity_index::U6, u64, U64);
arity!(Arity128, 128, arity_index::U7, u128, U128);
arity!(Arity256, 256, u8, arity_bitmap::U256, U256);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiring_constants() {
        assert_eq!(Arity8::LEN, 8);
        assert_eq!(Arity16::LEN, 16);
        assert_eq!(Arity256::LEN, 256);
        assert_eq!(<Arity16 as Arity>::Size::USIZE, 16);
        assert_eq!(<<Arity16 as Arity>::Index as Niche>::COUNT, 16);
        assert_eq!(<<Arity16 as Arity>::Bitmap as Bitmap>::WIDTH, 16);
        assert_eq!(<<Arity256 as Arity>::Index as Niche>::COUNT, 256);
    }
}
```

- [ ] **Step 2: Write `lib.rs`**

Replace `crates/arity-arrays/src/lib.rs` with:

```rust
#![no_std]

//! Fixed-arity array storage indexed by bounds-check-free niche integers.
//!
//! [`FixedArray`] is a full-width inline array (one slot per index);
//! [`PackedArray`] is a pointer-sized, heap-packed representation that stores
//! only the present elements. Both are generic over the [`Arity`] trait, which
//! pairs an index type with a bitmap backing and a `hybrid-array` size.

extern crate alloc;

pub mod arity;
pub mod fixed;
pub mod packed;

pub use arity::{Arity, Arity8, Arity16, Arity32, Arity64, Arity128, Arity256};
pub use fixed::FixedArray;
pub use packed::PackedArray;

pub use arity_bitmap as bitmap;
pub use arity_index as index;

/// Prevents downstream crates from implementing [`Arity`](crate::Arity).
trait Sealed {}
```

> The scaffold's `pub mod fixed;`/`pub mod packed;` already exist (empty files);
> later tasks fill them. `lib.rs` referencing `fixed::FixedArray`/`packed::PackedArray`
> before they exist will not compile, so for THIS task temporarily comment out the
> `pub use fixed::FixedArray;` and `pub use packed::PackedArray;` lines (and add a
> `// TODO(task 2/4): re-enable` note). Re-enable them in Tasks 2 and 4.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p arity-arrays arity::`
Expected: FAIL — `Arity`/markers not defined (before Step 1's file is saved) or compile error referencing them.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-arrays`
Expected: PASS (`wiring_constants`). The `const _` asserts compile (proving the wiring) for all six arities.

- [ ] **Step 5: Run clippy + commit**

Run: `cargo clippy -p arity-arrays --all-targets`
Expected: clean.

```bash
git add crates/arity-arrays/src/arity.rs crates/arity-arrays/src/lib.rs
git commit -m "feat(arity-arrays): add sealed Arity trait and Arity8..Arity256 markers"
```

---

### Task 2: `FixedArray<T, A>` — core

Full-width inline storage over `hybrid_array::Array<T, A::Size>`, indexed by `A::Index` without bounds checks.

**Files:**
- Modify: `crates/arity-arrays/src/fixed.rs`
- Modify: `crates/arity-arrays/src/lib.rs` (re-enable `pub use fixed::FixedArray;`)

**Interfaces:**
- Consumes: `Arity`, `Niche::{as_usize, try_from_usize, all}`, `hybrid_array::Array`.
- Produces: `pub struct FixedArray<T, A: Arity>(Array<T, A::Size>)`; `from_fn`, `get`, `get_mut`, `replace`, `map`, `Index`/`IndexMut`, `Deref`/`AsRef<[T]>`, `IntoIterator` for value/ref/mut (yielding `(A::Index, …)`).

- [ ] **Step 1: Write the failing test**

Append to `crates/arity-arrays/src/fixed.rs` (after the impl in Step 3; write it now so the test exists):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arity16, Arity8};
    use arity_index::{U3, U4};

    extern crate alloc;
    use alloc::vec::Vec;

    #[test]
    fn from_fn_and_get() {
        let a = FixedArray::<u8, Arity16>::from_fn(|i| i.as_u8());
        assert_eq!(*a.get(U4::new_masked(0)), 0);
        assert_eq!(*a.get(U4::new_masked(15)), 15);
    }

    #[test]
    fn get_mut_and_replace() {
        let mut a = FixedArray::<u8, Arity8>::from_fn(|_| 0);
        *a.get_mut(U3::new_masked(2)) = 42;
        assert_eq!(*a.get(U3::new_masked(2)), 42);
        let prev = a.replace(U3::new_masked(2), 7);
        assert_eq!(prev, 42);
        assert_eq!(*a.get(U3::new_masked(2)), 7);
    }

    #[test]
    fn index_ops_and_deref() {
        let mut a = FixedArray::<u8, Arity16>::from_fn(|i| i.as_u8());
        assert_eq!(a[U4::new_masked(3)], 3);
        a[U4::new_masked(3)] = 99;
        assert_eq!(a[U4::new_masked(3)], 99);
        // Deref to [T]
        assert_eq!(a.len(), 16);
        assert_eq!(a.iter().copied().max(), Some(99));
    }

    #[test]
    fn into_iter_pairs_with_index() {
        let a = FixedArray::<u8, Arity8>::from_fn(|i| i.as_u8() * 2);
        let pairs: Vec<(u8, u8)> = (&a).into_iter().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(pairs, alloc::vec![(0, 0), (1, 2), (2, 4), (3, 6), (4, 8), (5, 10), (6, 12), (7, 14)]);
        // value iterator is double-ended
        let last = a.into_iter().next_back().map(|(i, v)| (i.as_u8(), v));
        assert_eq!(last, Some((7, 14)));
    }

    #[test]
    fn map_threads_index() {
        let a = FixedArray::<u8, Arity8>::from_fn(|_| 1);
        let b = a.map(|i, v| i.as_u8() + v);
        let got: Vec<u8> = (&b).into_iter().map(|(_, &v)| v).collect();
        assert_eq!(got, alloc::vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib fixed`
Expected: FAIL — `FixedArray` not defined.

- [ ] **Step 3: Write the implementation**

Put this at the TOP of `crates/arity-arrays/src/fixed.rs` (above the test module):

```rust
//! [`FixedArray`]: full-width inline storage, one `T` per slot.

use core::ops::{Deref, DerefMut, Index, IndexMut};

use arity_index::Niche;
use hybrid_array::Array;

use crate::Arity;

/// A full-width array with one `T` per slot, indexed by `A::Index` without bounds
/// checks.
///
/// `hybrid_array::Array` is an implementation detail — it never appears in a
/// public signature (the type exposes `Deref<Target = [T]>` / `AsRef<[T]>`), so
/// the `typenum` backing can be retired later without a breaking change.
pub struct FixedArray<T, A: Arity>(Array<T, A::Size>);

impl<T, A: Arity> FixedArray<T, A> {
    /// Builds a `FixedArray` by calling `f` for every index in ascending order.
    pub fn from_fn(mut f: impl FnMut(A::Index) -> T) -> Self {
        Self(Array::from_fn(|i| {
            // `i` ranges over `0..A::Size::USIZE == A::LEN == Index::COUNT`.
            f(A::Index::try_from_usize(i).expect("from_fn index < LEN"))
        }))
    }

    /// Returns a reference to the element at `index` (no bounds check).
    #[must_use]
    pub fn get(&self, index: A::Index) -> &T {
        // SAFETY: `A::Index::as_usize()` is always `< Index::COUNT == A::LEN`,
        // which equals the array length, so `index` is in bounds.
        unsafe { self.0.as_slice().get_unchecked(index.as_usize()) }
    }

    /// Returns a mutable reference to the element at `index` (no bounds check).
    #[must_use]
    pub fn get_mut(&mut self, index: A::Index) -> &mut T {
        // SAFETY: as in `get` — `index.as_usize() < A::LEN == array length`.
        unsafe { self.0.as_mut_slice().get_unchecked_mut(index.as_usize()) }
    }

    /// Replaces the element at `index`, returning the previous value.
    pub fn replace(&mut self, index: A::Index, value: T) -> T {
        core::mem::replace(self.get_mut(index), value)
    }

    /// Maps each element to a new value, with its index, returning a new array.
    pub fn map<O>(self, mut f: impl FnMut(A::Index, T) -> O) -> FixedArray<O, A> {
        let mut idx = A::Index::all();
        FixedArray(self.0.map(|t| {
            let i = idx.next().expect("map cursor stays in 0..LEN");
            f(i, t)
        }))
    }
}

impl<T, A: Arity> Index<A::Index> for FixedArray<T, A> {
    type Output = T;
    fn index(&self, index: A::Index) -> &T {
        self.get(index)
    }
}

impl<T, A: Arity> IndexMut<A::Index> for FixedArray<T, A> {
    fn index_mut(&mut self, index: A::Index) -> &mut T {
        self.get_mut(index)
    }
}

impl<T, A: Arity> Deref for FixedArray<T, A> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.0.as_slice()
    }
}

impl<T, A: Arity> DerefMut for FixedArray<T, A> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.as_mut_slice()
    }
}

impl<T, A: Arity> AsRef<[T]> for FixedArray<T, A> {
    fn as_ref(&self) -> &[T] {
        self.0.as_slice()
    }
}

impl<T, A: Arity> IntoIterator for FixedArray<T, A> {
    type Item = (A::Index, T);
    // `hybrid-array` has no public `IntoIter<T, N>` name; the owned iterator type
    // is the associated `IntoIter` of `Array`'s own `IntoIterator` impl.
    type IntoIter = core::iter::Zip<
        arity_index::NicheRangeInclusive<A::Index>,
        <hybrid_array::Array<T, A::Size> as IntoIterator>::IntoIter,
    >;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0)
    }
}

impl<'a, T, A: Arity> IntoIterator for &'a FixedArray<T, A> {
    type Item = (A::Index, &'a T);
    // Route through the slice so the iterator is the std `slice::Iter` (not
    // `Array::iter`'s inherent `hybrid_array::Iter`), matching the named type.
    type IntoIter = core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_slice().iter())
    }
}

impl<'a, T, A: Arity> IntoIterator for &'a mut FixedArray<T, A> {
    type Item = (A::Index, &'a mut T);
    type IntoIter = core::iter::Zip<arity_index::NicheRangeInclusive<A::Index>, core::slice::IterMut<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        A::Index::all().zip(self.0.as_mut_slice().iter_mut())
    }
}
```

Re-enable in `lib.rs`: `pub use fixed::FixedArray;`.

> The `Array` API used here is confirmed against `hybrid-array` 0.4.12:
> `Array::from_fn`, `Array::map`, `as_slice`/`as_mut_slice`, `AsRef`/`AsMut<[T]>`,
> and `Array: IntoIterator`. There is no `hybrid_array::IntoIter` type — name the
> owned iterator via `<Array<…> as IntoIterator>::IntoIter` as above.

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p arity-arrays --lib fixed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-arrays/src/fixed.rs crates/arity-arrays/src/lib.rs
git commit -m "feat(arity-arrays): add FixedArray core (indexing, deref, iterators, map)"
```

---

### Task 3: `FixedArray<Option<T>, A>` specialization

The `Option`-per-slot view used by the `PackedArray` conversions.

**Files:**
- Modify: `crates/arity-arrays/src/fixed.rs`

**Interfaces:**
- Produces: on `FixedArray<Option<T>, A>` — `new`, `count`, `take`, `iter_present`, `take_only_child`; `impl Default`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `fixed.rs`:

```rust
    #[test]
    fn option_new_count_take() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        assert_eq!(a.count(), 0);
        a[U4::new_masked(1)] = Some(10);
        a[U4::new_masked(9)] = Some(90);
        assert_eq!(a.count(), 2);
        assert_eq!(a.take(U4::new_masked(1)), Some(10));
        assert_eq!(a.count(), 1);
        assert_eq!(a.take(U4::new_masked(1)), None);
    }

    #[test]
    fn option_iter_present_ascending() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        a[U4::new_masked(3)] = Some(3);
        a[U4::new_masked(11)] = Some(11);
        let got: Vec<(u8, u8)> = a.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(got, alloc::vec![(3, 3), (11, 11)]);
    }

    #[test]
    fn option_take_only_child() {
        let mut a = FixedArray::<Option<u8>, Arity16>::new();
        assert_eq!(a.take_only_child(), None);
        a[U4::new_masked(5)] = Some(50);
        assert_eq!(a.take_only_child().map(|(i, v)| (i.as_u8(), v)), Some((5, 50)));
        assert_eq!(a.count(), 0);
        a[U4::new_masked(2)] = Some(20);
        a[U4::new_masked(6)] = Some(60);
        assert_eq!(a.take_only_child(), None); // two children → None, nothing taken
        assert_eq!(a.count(), 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib fixed::tests::option`
Expected: FAIL — `new`/`count`/`take`/`iter_present`/`take_only_child` not defined.

- [ ] **Step 3: Write the implementation**

Add to `fixed.rs` (above the test module):

```rust
impl<T, A: Arity> FixedArray<Option<T>, A> {
    /// Creates a `FixedArray` with every slot `None`.
    #[must_use]
    pub fn new() -> Self {
        Self::from_fn(|_| None)
    }

    /// Returns the number of `Some` slots.
    #[must_use]
    pub fn count(&self) -> usize {
        self.iter().filter(|slot| slot.is_some()).count()
    }

    /// Sets the slot at `index` to `None`, returning the previous value.
    pub fn take(&mut self, index: A::Index) -> Option<T> {
        self.replace(index, None)
    }

    /// Iterates over the present (`Some`) slots as `(A::Index, &T)`, ascending.
    pub fn iter_present(&self) -> impl DoubleEndedIterator<Item = (A::Index, &T)> {
        self.into_iter()
            .filter_map(|(i, slot)| slot.as_ref().map(|v| (i, v)))
    }

    /// If exactly one slot is present, takes and returns it with its index;
    /// otherwise returns `None` and leaves the array unchanged.
    pub fn take_only_child(&mut self) -> Option<(A::Index, T)> {
        let mut present = self.iter_present().map(|(i, _)| i);
        let only = present.next()?;
        if present.next().is_some() {
            return None;
        }
        drop(present);
        self.take(only).map(|v| (only, v))
    }
}

impl<T, A: Arity> Default for FixedArray<Option<T>, A> {
    fn default() -> Self {
        Self::new()
    }
}
```

> `iter_present` returns `impl DoubleEndedIterator` because `&FixedArray`'s
> iterator and `filter_map` are both double-ended. If clippy flags the `impl
> Trait` return for a `must_use`/lifetime reason, keep the signature — it is the
> idiomatic shape and matches firewood's `iter_present`.

- [ ] **Step 4: Run tests + clippy + commit**

Run: `cargo test -p arity-arrays --lib fixed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS; clean.

```bash
git add crates/arity-arrays/src/fixed.rs
git commit -m "feat(arity-arrays): add FixedArray<Option<T>> view (new/count/take/iter_present)"
```

---

### Task 4: `PackedArray<T, A>` — layout, construction, `get`

The pointer-sized heap block, its allocation helpers, the owned `From<FixedArray<Option<T>, A>>` constructor, and rank-select `get`. **This is the core `unsafe` task.**

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs`
- Modify: `crates/arity-arrays/src/lib.rs` (re-enable `pub use packed::PackedArray;`)

**Interfaces:**
- Consumes: `Arity`, `arity_bitmap::Bitmap::{ZERO, is_zero, count_ones, test, with_bit, rank}`, `FixedArray`.
- Produces: `pub struct PackedArray<T, A: Arity>`; `new`, `Default`, `bitmap`, `count`, `is_empty`, `get`, `From<FixedArray<Option<T>, A>>`. Pointer-size const assert.

- [ ] **Step 1: Write the failing test**

Append to `crates/arity-arrays/src/packed.rs` (test module; impl added in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arity16, Arity256, FixedArray};
    use arity_index::{Niche, U4};

    #[test]
    fn pointer_sized_and_empty() {
        assert_eq!(
            core::mem::size_of::<PackedArray<[u8; 32], Arity16>>(),
            core::mem::size_of::<*const ()>()
        );
        let p = PackedArray::<u8, Arity16>::new();
        assert_eq!(p.count(), 0);
        assert!(p.is_empty());
        assert_eq!(p.get(U4::new_masked(0)), None);
    }

    #[test]
    fn from_fixed_and_get() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(0)] = Some(10);
        src[U4::new_masked(7)] = Some(70);
        src[U4::new_masked(15)] = Some(150);
        let p = PackedArray::from(src);
        assert_eq!(p.count(), 3);
        assert_eq!(p.get(U4::new_masked(0)), Some(&10));
        assert_eq!(p.get(U4::new_masked(7)), Some(&70));
        assert_eq!(p.get(U4::new_masked(15)), Some(&150));
        assert_eq!(p.get(U4::new_masked(1)), None);
        assert_eq!(p.get(U4::new_masked(8)), None);
    }

    #[test]
    fn single_child_rank_zero_every_slot() {
        // Exercises the rank-zero boundary at every slot of every arity edge.
        for slot in 0..16u8 {
            let mut src = FixedArray::<Option<u8>, Arity16>::new();
            src[U4::new_masked(slot)] = Some(slot);
            let p = PackedArray::from(src);
            assert_eq!(p.count(), 1, "slot {slot}");
            assert_eq!(p.get(U4::new_masked(slot)), Some(&slot), "slot {slot}");
        }
    }

    #[test]
    fn arity256_boundary() {
        let mut src = FixedArray::<Option<u16>, Arity256>::new();
        src[0] = Some(1);
        src[255] = Some(2);
        let p = PackedArray::from(src);
        assert_eq!(p.count(), 2);
        assert_eq!(p.get(0), Some(&1));
        assert_eq!(p.get(255), Some(&2));
        assert_eq!(p.get(128), None);
        assert_eq!(
            core::mem::size_of::<PackedArray<u16, Arity256>>(),
            core::mem::size_of::<*const ()>()
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib packed`
Expected: FAIL — `PackedArray` not defined.

- [ ] **Step 3: Write the implementation**

Put this at the top of `crates/arity-arrays/src/packed.rs`:

```rust
//! [`PackedArray`]: a pointer-sized, heap-packed array storing only present
//! elements, addressed by bitmap rank-select.

use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;

use alloc::alloc::{alloc, dealloc, handle_alloc_error};

use arity_bitmap::Bitmap;

use crate::{Arity, FixedArray};

/// Header of the heap block: the bitmap followed (after alignment padding) by the
/// element array. `#[repr(C)]` makes `data` the canonical element-array address.
#[repr(C)]
struct Inner<A: Arity, T> {
    bitmap: A::Bitmap,
    /// Zero-sized address anchor for the trailing element array; obtain the base
    /// with `&raw mut (*p).data` (RFC 2582).
    data: [T; 0],
}

/// An immutable, pointer-sized, heap-packed array over arity `A`.
///
/// `None` ↔ empty (no allocation). `Some(ptr)` ↔ a heap block sized to exactly
/// the present elements. The `NonNull` null-pointer niche makes this type the
/// size of a pointer for every `A`.
pub struct PackedArray<T, A: Arity>(Option<NonNull<Inner<A, T>>>, PhantomData<alloc::boxed::Box<T>>);

// Compile-time guarantee: pointer-sized.
const _: () = assert!(
    core::mem::size_of::<PackedArray<[u8; 32], crate::Arity16>>()
        == core::mem::size_of::<*const ()>()
);

/// Layout of the heap block for `count` elements: `Inner` header extended by a
/// `[T; count]` array, padded to alignment.
fn alloc_layout<A: Arity, T>(count: usize) -> Layout {
    let (layout, _) = Layout::new::<Inner<A, T>>()
        .extend(Layout::array::<T>(count).expect("element layout overflow"))
        .expect("block layout overflow");
    layout.pad_to_align()
}

/// Base address of the element array within an `Inner` allocation.
///
/// # Safety
/// `inner` must point to a live allocation from `alloc_layout::<A, T>(count)`
/// with the `bitmap` field initialised.
unsafe fn data_ptr<A: Arity, T>(inner: NonNull<Inner<A, T>>) -> *mut T {
    // SAFETY: `inner` is valid per the precondition; `#[repr(C)]` places `data`
    // at the correct offset, so `&raw mut (*p).data` cast to `*mut T` is the
    // element-array base.
    unsafe { (&raw mut (*inner.as_ptr()).data).cast::<T>() }
}

impl<T, A: Arity> PackedArray<T, A> {
    /// Creates an empty `PackedArray` (no allocation).
    #[must_use]
    pub const fn new() -> Self {
        Self(None, PhantomData)
    }

    /// Returns the bitmap of present slots (`A::Bitmap::ZERO` when empty).
    #[must_use]
    pub fn bitmap(&self) -> A::Bitmap {
        match self.0 {
            None => A::Bitmap::ZERO,
            // SAFETY: `Some` ↔ a live allocation with an initialised bitmap.
            Some(p) => unsafe { p.as_ref().bitmap },
        }
    }

    /// Returns the number of present elements.
    #[must_use]
    pub fn count(&self) -> usize {
        self.bitmap().count_ones() as usize
    }

    /// Returns `true` if there are no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// Returns a reference to the element at `index`, or `None` if absent.
    #[must_use]
    pub fn get(&self, index: A::Index) -> Option<&T> {
        let ptr = self.0?;
        // SAFETY: `ptr` is valid per the invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        if !bm.test(index) {
            return None;
        }
        let rank = bm.rank(index) as usize;
        // SAFETY: `index` is present, so `rank < count`; `data_ptr` is valid and
        // `.add(rank)` is within the allocation, pointing at an initialised `T`.
        Some(unsafe { &*data_ptr(ptr).add(rank) })
    }
}

impl<T, A: Arity> Default for PackedArray<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Moves each `Some` element of a `FixedArray<Option<T>, A>` into a packed block;
/// `None` slots are dropped.
impl<T, A: Arity> From<FixedArray<Option<T>, A>> for PackedArray<T, A> {
    fn from(src: FixedArray<Option<T>, A>) -> Self {
        // Pass 1 (by ref): compute the bitmap.
        let mut bitmap = A::Bitmap::ZERO;
        for (i, slot) in &src {
            if slot.is_some() {
                bitmap = bitmap.with_bit(i);
            }
        }
        if bitmap.is_zero() {
            return Self::new();
        }
        let count = bitmap.count_ones() as usize;
        let layout = alloc_layout::<A, T>(count);
        // SAFETY: `count > 0` so `layout.size() > 0`; `alloc` returns null on
        // failure, handled below.
        let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
            handle_alloc_error(layout)
        };
        let inner = raw.cast::<Inner<A, T>>();
        // SAFETY: `inner` is freshly allocated and sized for `Inner<A, T>`;
        // writing the bitmap initialises the header before any element.
        unsafe { (&raw mut (*inner.as_ptr()).bitmap).write(bitmap) };
        // SAFETY: `inner` valid; `data_ptr` is the base of `count` element slots.
        let dp = unsafe { data_ptr(inner) };
        // Pass 2 (by value): move each `Some` into the next dense slot.
        let mut rank = 0usize;
        for (_i, slot) in src {
            if let Some(v) = slot {
                // SAFETY: `rank < count`; `dp.add(rank)` is an uninitialised slot
                // within the allocation; `write` initialises it.
                unsafe { dp.add(rank).write(v) };
                rank += 1;
            }
        }
        Self(Some(inner), PhantomData)
    }
}
```

Re-enable in `lib.rs`: `pub use packed::PackedArray;`.

> Note: `PackedArray` owns a heap allocation but has no `Drop` yet — Task 6 adds
> it. The Task 4/5 tests leak on drop until then; that is acceptable mid-plan
> (Miri's leak check runs in Task 8 after `Drop` exists). Do not run Miri yet.

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p arity-arrays --lib packed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS; clippy clean (every `unsafe` block has a `// SAFETY:`).

- [ ] **Step 5: Commit**

```bash
git add crates/arity-arrays/src/packed.rs crates/arity-arrays/src/lib.rs
git commit -m "feat(arity-arrays): add PackedArray layout, From<FixedArray>, and get"
```

---

### Task 5: `PackedArray` iterators

`iter_present` (built on `bitmap.bits()`) and the all-slots `iter`, both double-ended.

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs`

**Interfaces:**
- Consumes: `Bitmap::bits`, `Bitmap::rank`, `Niche::all`, `PackedArray::get`.
- Produces: `PackedArray::iter_present() -> PackedPresentIter`, `PackedArray::iter() -> PackedAllIter`; both `DoubleEndedIterator + ExactSizeIterator + FusedIterator`.

- [ ] **Step 1: Write the failing test**

Add to the `packed.rs` test module:

```rust
    #[test]
    fn iter_present_ascending_and_double_ended() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(1)] = Some(1);
        src[U4::new_masked(4)] = Some(4);
        src[U4::new_masked(14)] = Some(14);
        let p = PackedArray::from(src);

        let fwd: alloc::vec::Vec<(u8, u8)> =
            p.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
        assert_eq!(fwd, alloc::vec![(1, 1), (4, 4), (14, 14)]);

        let mut it = p.iter_present();
        assert_eq!(it.len(), 3);
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((1, 1)));
        assert_eq!(it.next_back().map(|(i, &v)| (i.as_u8(), v)), Some((14, 14)));
        assert_eq!(it.next().map(|(i, &v)| (i.as_u8(), v)), Some((4, 4)));
        assert_eq!(it.next(), None);

        extern crate alloc;
    }

    #[test]
    fn iter_all_slots() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(5)] = Some(5);
        let p = PackedArray::from(src);
        let all: alloc::vec::Vec<(u8, Option<u8>)> =
            p.iter().map(|(i, opt)| (i.as_u8(), opt.copied())).collect();
        assert_eq!(all.len(), 16);
        assert_eq!(all[5], (5, Some(5)));
        assert_eq!(all[0], (0, None));
        assert_eq!(all[15], (15, None));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib packed::tests::iter`
Expected: FAIL — `iter_present`/`iter` not defined.

- [ ] **Step 3: Write the implementation**

Add to `packed.rs`. First the methods (inside the existing `impl<T, A: Arity> PackedArray<T, A>` block):

```rust
    /// Iterates over present elements as `(A::Index, &T)`, ascending. Double-ended.
    #[must_use]
    pub fn iter_present(&self) -> PackedPresentIter<'_, T, A> {
        match self.0 {
            None => PackedPresentIter {
                bits: A::Bitmap::ZERO.bits(),
                bitmap: A::Bitmap::ZERO,
                data: core::ptr::null(),
                _marker: PhantomData,
            },
            // SAFETY: `Some` ↔ a valid allocation with initialised bitmap/elements.
            Some(ptr) => unsafe {
                let bitmap = ptr.as_ref().bitmap;
                PackedPresentIter {
                    bits: bitmap.bits(),
                    bitmap,
                    data: data_ptr(ptr).cast_const(),
                    _marker: PhantomData,
                }
            },
        }
    }

    /// Iterates over all `A::LEN` slots as `(A::Index, Option<&T>)`, ascending.
    /// Double-ended.
    #[must_use]
    pub fn iter(&self) -> PackedAllIter<'_, T, A> {
        PackedAllIter {
            array: self,
            slots: A::Index::all(),
        }
    }
```

Then the iterator types (at module scope, below the `impl`s):

```rust
use arity_index::Niche;

/// Iterator over present elements of a [`PackedArray`]. See
/// [`PackedArray::iter_present`].
pub struct PackedPresentIter<'a, T, A: Arity> {
    bits: arity_bitmap::BitIter<A::Bitmap>,
    bitmap: A::Bitmap,
    data: *const T,
    _marker: PhantomData<&'a T>,
}

impl<'a, T, A: Arity> PackedPresentIter<'a, T, A> {
    fn elem(&self, index: A::Index) -> (A::Index, &'a T) {
        let rank = self.bitmap.rank(index) as usize;
        // SAFETY: `index` is a set bit of the original `bitmap` snapshot, so
        // `rank < count`; `data` is the element base; `.add(rank)` is in bounds
        // and initialised. `rank` uses the original bitmap, not the drained
        // `bits` state, so the offset is correct in either direction.
        (index, unsafe { &*self.data.add(rank) })
    }
}

impl<'a, T, A: Arity> Iterator for PackedPresentIter<'a, T, A> {
    type Item = (A::Index, &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        self.bits.next().map(|i| self.elem(i))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.bits.size_hint()
    }
}

impl<'a, T, A: Arity> DoubleEndedIterator for PackedPresentIter<'a, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.bits.next_back().map(|i| self.elem(i))
    }
}

impl<T, A: Arity> ExactSizeIterator for PackedPresentIter<'_, T, A> {
    fn len(&self) -> usize {
        self.bits.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for PackedPresentIter<'_, T, A> {}

/// Iterator over all slots of a [`PackedArray`]. See [`PackedArray::iter`].
pub struct PackedAllIter<'a, T, A: Arity> {
    array: &'a PackedArray<T, A>,
    slots: arity_index::NicheRangeInclusive<A::Index>,
}

impl<'a, T, A: Arity> Iterator for PackedAllIter<'a, T, A> {
    type Item = (A::Index, Option<&'a T>);
    fn next(&mut self) -> Option<Self::Item> {
        self.slots.next().map(|i| (i, self.array.get(i)))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.slots.size_hint()
    }
}

impl<'a, T, A: Arity> DoubleEndedIterator for PackedAllIter<'a, T, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.slots.next_back().map(|i| (i, self.array.get(i)))
    }
}

impl<T, A: Arity> ExactSizeIterator for PackedAllIter<'_, T, A> {
    fn len(&self) -> usize {
        self.slots.len()
    }
}

impl<T, A: Arity> core::iter::FusedIterator for PackedAllIter<'_, T, A> {}
```

> `PackedAllIter::get(i)` borrows `&'a self.array`; returns `Option<&'a T>` —
> sound because the iterator holds the shared borrow for `'a`. If the borrow
> checker rejects the `'a` on `get` (it returns `&'_ T` tied to `&self`), store
> the base `data: *const T` + `bitmap` like `PackedPresentIter` and compute the
> element inline instead of calling `self.array.get`.

- [ ] **Step 4: Run tests + clippy + commit**

Run: `cargo test -p arity-arrays --lib packed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS; clean.

```bash
git add crates/arity-arrays/src/packed.rs
git commit -m "feat(arity-arrays): add double-ended PackedArray iter_present and iter"
```

---

### Task 6: `PackedArray` `Drop`, `Clone`, and trait impls

Manual `Drop`, panic-safe `Clone`, and `Eq`/`Hash`/`Debug`/`Send`/`Sync`/unwind-safety.

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs`

**Interfaces:**
- Produces: `impl Drop`, `impl<T: Clone> Clone`, `impl<T: PartialEq> PartialEq`, `Eq`, `Hash`, `Debug`, `unsafe impl Send/Sync`, `UnwindSafe`/`RefUnwindSafe` for `PackedArray`.

- [ ] **Step 1: Write the failing test**

Add to the `packed.rs` test module (uses `std` in tests, which is fine):

```rust
    #[test]
    fn drop_runs_once_per_element() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counted(Arc<AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Counted>, Arity16>::new();
        src[U4::new_masked(2)] = Some(Counted(counter.clone()));
        src[U4::new_masked(7)] = Some(Counted(counter.clone()));
        let p = PackedArray::from(src);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(p);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn clone_is_independent() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counted(Arc<AtomicUsize>);
        impl Clone for Counted {
            fn clone(&self) -> Self {
                Self(self.0.clone())
            }
        }
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Counted>, Arity16>::new();
        src[U4::new_masked(1)] = Some(Counted(counter.clone()));
        src[U4::new_masked(9)] = Some(Counted(counter.clone()));
        let original = PackedArray::from(src);
        let cloned = original.clone();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(original);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        drop(cloned);
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn clone_panic_frees_partial() {
        use std::panic;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Panicky {
            drops: Arc<AtomicUsize>,
            clones: Arc<AtomicUsize>,
        }
        impl Clone for Panicky {
            fn clone(&self) -> Self {
                assert!(self.clones.fetch_add(1, Ordering::SeqCst) < 2, "boom");
                Self { drops: self.drops.clone(), clones: self.clones.clone() }
            }
        }
        impl Drop for Panicky {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let clones = Arc::new(AtomicUsize::new(0));
        let mut src = FixedArray::<Option<Panicky>, Arity16>::new();
        for i in 0..4u8 {
            src[U4::new_masked(i)] = Some(Panicky { drops: drops.clone(), clones: clones.clone() });
        }
        let p = PackedArray::from(src);
        let r = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = p.clone();
        }));
        assert!(r.is_err());
        // The 2 successfully-cloned elements were freed by the guard on unwind.
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(p);
        assert_eq!(drops.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn eq_and_debug() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(2)] = Some(2);
        let a = PackedArray::from(src);
        let b = a.clone();
        assert_eq!(a, b);
        let mut src2 = FixedArray::<Option<u8>, Arity16>::new();
        src2[U4::new_masked(3)] = Some(3);
        assert_ne!(a, PackedArray::from(src2));
        // Debug renders present slots.
        let s = std::format!("{a:?}");
        assert!(s.contains('2'));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib packed::tests::drop_runs`
Expected: FAIL — `Drop`/`Clone`/`PartialEq` not implemented (drop test counts 0, or clone missing).

- [ ] **Step 3: Write the implementation**

Add to `packed.rs`:

```rust
impl<T, A: Arity> Drop for PackedArray<T, A> {
    fn drop(&mut self) {
        let Some(ptr) = self.0 else { return };
        // SAFETY: `ptr` is valid per the invariant; the bitmap is initialised.
        let count = unsafe { ptr.as_ref().bitmap.count_ones() as usize };
        // SAFETY: `ptr` valid; `data_ptr` is the base of `count` initialised `T`.
        let dp = unsafe { data_ptr(ptr) };
        // SAFETY: `dp..dp+count` are initialised; `drop_in_place` over the slice
        // drops each exactly once.
        unsafe { core::ptr::drop_in_place(core::ptr::slice_from_raw_parts_mut(dp, count)) };
        // SAFETY: `ptr` came from `alloc(alloc_layout::<A, T>(count))`; elements
        // are dropped; this is the sole deallocation.
        unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<A, T>(count)) };
    }
}

impl<T: Clone, A: Arity> Clone for PackedArray<T, A> {
    fn clone(&self) -> Self {
        let Some(ptr) = self.0 else { return Self::new() };
        // SAFETY: `ptr` valid per the invariant.
        let bitmap = unsafe { ptr.as_ref().bitmap };
        let count = bitmap.count_ones() as usize;
        let layout = alloc_layout::<A, T>(count);
        // SAFETY: `count > 0`; null handled below.
        let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
            handle_alloc_error(layout)
        };
        let new_inner = raw.cast::<Inner<A, T>>();
        // SAFETY: freshly allocated; write initialises the header.
        unsafe { (&raw mut (*new_inner.as_ptr()).bitmap).write(bitmap) };
        // SAFETY: both pointers are valid element bases for `count` elements.
        let src = unsafe { data_ptr(ptr).cast_const() };
        let dst = unsafe { data_ptr(new_inner) };

        // Frees already-cloned elements + the allocation if `T::clone` panics.
        struct CloneGuard<A: Arity, T> {
            inner: NonNull<Inner<A, T>>,
            initialized: usize,
            capacity: usize,
        }
        impl<A: Arity, T> Drop for CloneGuard<A, T> {
            fn drop(&mut self) {
                // SAFETY: `inner` is a live allocation from `alloc_layout::<A,T>(capacity)`;
                // `initialized` leading elements are initialised.
                unsafe {
                    let dp = data_ptr(self.inner);
                    core::ptr::drop_in_place(core::ptr::slice_from_raw_parts_mut(dp, self.initialized));
                    dealloc(self.inner.as_ptr().cast(), alloc_layout::<A, T>(self.capacity));
                }
            }
        }

        let mut guard = CloneGuard { inner: new_inner, initialized: 0, capacity: count };
        for i in 0..count {
            // SAFETY: `i < count`; `src.add(i)` is initialised; `dst.add(i)` is an
            // uninitialised slot; `write` initialises it.
            unsafe { dst.add(i).write((*src.add(i)).clone()) };
            guard.initialized = i + 1;
        }
        core::mem::forget(guard);
        Self(Some(new_inner), PhantomData)
    }
}

impl<T: PartialEq, A: Arity> PartialEq for PackedArray<T, A> {
    fn eq(&self, other: &Self) -> bool {
        self.bitmap() == other.bitmap()
            && self.iter_present().map(|(_, v)| v).eq(other.iter_present().map(|(_, v)| v))
    }
}

impl<T: Eq, A: Arity> Eq for PackedArray<T, A> {}

impl<T: core::hash::Hash, A: Arity> core::hash::Hash for PackedArray<T, A> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.count().hash(state);
        for (i, v) in self.iter_present() {
            i.as_usize().hash(state);
            v.hash(state);
        }
    }
}

impl<T: core::fmt::Debug, A: Arity> core::fmt::Debug for PackedArray<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map()
            .entries(self.iter_present().map(|(i, v)| (i.as_usize(), v)))
            .finish()
    }
}

// SAFETY: `PackedArray` exclusively owns its allocation; sending it across
// threads is sound when `T: Send`.
unsafe impl<T: Send, A: Arity> Send for PackedArray<T, A> {}
// SAFETY: `&PackedArray` yields only `&T`; no interior mutability.
unsafe impl<T: Sync, A: Arity> Sync for PackedArray<T, A> {}

// `NonNull` is `!UnwindSafe`; `PackedArray` owns its data with no shared/cyclic
// state, so these hold whenever `T` does.
impl<T: core::panic::UnwindSafe, A: Arity> core::panic::UnwindSafe for PackedArray<T, A> {}
impl<T: core::panic::RefUnwindSafe, A: Arity> core::panic::RefUnwindSafe for PackedArray<T, A> {}

// `PackedPresentIter` holds a `*const T` (which suppresses the auto-impls) but
// only ever yields `&T` — it behaves like a `slice::Iter`, so it is `Send`/`Sync`
// exactly when `T: Sync`. (`PackedAllIter` borrows `&PackedArray`, so it derives
// `Send`/`Sync` automatically once `PackedArray: Sync`.)
// SAFETY: the raw pointer is used only for shared reads bounded by `&'a self`.
unsafe impl<T: Sync, A: Arity> Send for crate::packed::PackedPresentIter<'_, T, A> {}
// SAFETY: as above — shared, read-only access; no interior mutability.
unsafe impl<T: Sync, A: Arity> Sync for crate::packed::PackedPresentIter<'_, T, A> {}
```

(`PackedPresentIter` is defined in this same `packed.rs`, so reference it as
`PackedPresentIter<'_, T, A>` directly rather than via `crate::packed::`.)

> `eq` compares `bitmap()` then element streams via `Iterator::eq` (avoids needing
> a slice). `Hash` mixes count + (index, value) pairs. If clippy's `nursery`
> flags `derivable_impls` or similar, none should apply (these are all manual for
> good reason). The `Hash`/`Eq` consistency (equal values ⇒ equal hashes) holds:
> both iterate present slots in the same ascending order.

- [ ] **Step 4: Run tests + clippy**

Run: `cargo test -p arity-arrays --lib packed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS (drop-once, clone independence, **clone-panic guard**, eq/debug); clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-arrays/src/packed.rs
git commit -m "feat(arity-arrays): add PackedArray Drop, panic-safe Clone, Eq/Hash/Debug/Send/Sync"
```

---

### Task 7: `PackedArray` ↔ `FixedArray` reverse + by-ref conversions

The remaining three `From` impls completing the symmetric conversion set.

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs`

**Interfaces:**
- Produces: `From<&FixedArray<Option<T>, A>> for PackedArray<T, A>` (`T: Clone`); `From<PackedArray<T, A>> for FixedArray<Option<T>, A>`; `From<&PackedArray<T, A>> for FixedArray<Option<T>, A>` (`T: Clone`).

- [ ] **Step 1: Write the failing test**

Add to the `packed.rs` test module:

```rust
    #[test]
    fn owned_roundtrip_lossless() {
        for slots in [&[][..], &[0, 7, 15][..], &(0..16).collect::<Vec<u8>>()[..]] {
            let mut src = FixedArray::<Option<u8>, Arity16>::new();
            for &s in slots {
                src[U4::new_masked(s)] = Some(s);
            }
            let packed = PackedArray::from(src);
            let back: FixedArray<Option<u8>, Arity16> = packed.into();
            for s in 0..16u8 {
                let expected = slots.contains(&s).then_some(s);
                assert_eq!(*back.get(U4::new_masked(s)), expected, "slot {s}");
            }
        }
        extern crate alloc;
        use alloc::vec::Vec;
    }

    #[test]
    fn by_ref_roundtrip_lossless() {
        let mut src = FixedArray::<Option<u8>, Arity16>::new();
        src[U4::new_masked(4)] = Some(4);
        src[U4::new_masked(9)] = Some(9);
        let packed = PackedArray::<u8, Arity16>::from(&src);
        let back: FixedArray<Option<u8>, Arity16> = (&packed).into();
        for s in 0..16u8 {
            let expected = matches!(s, 4 | 9).then_some(s);
            assert_eq!(*back.get(U4::new_masked(s)), expected, "slot {s}");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-arrays --lib packed::tests::owned_roundtrip`
Expected: FAIL — the reverse / by-ref `From` impls do not exist.

- [ ] **Step 3: Write the implementation**

Add to `packed.rs`:

```rust
/// Clones each present element of a `&FixedArray<Option<T>, A>` into a packed block.
impl<T: Clone, A: Arity> From<&FixedArray<Option<T>, A>> for PackedArray<T, A> {
    fn from(src: &FixedArray<Option<T>, A>) -> Self {
        let mut bitmap = A::Bitmap::ZERO;
        for (i, slot) in src {
            if slot.is_some() {
                bitmap = bitmap.with_bit(i);
            }
        }
        if bitmap.is_zero() {
            return Self::new();
        }
        let count = bitmap.count_ones() as usize;
        let layout = alloc_layout::<A, T>(count);
        // SAFETY: `count > 0`; null handled below.
        let Some(raw) = NonNull::new(unsafe { alloc(layout) }) else {
            handle_alloc_error(layout)
        };
        let inner = raw.cast::<Inner<A, T>>();
        // SAFETY: freshly allocated; write initialises the header.
        unsafe { (&raw mut (*inner.as_ptr()).bitmap).write(bitmap) };
        let dp = unsafe { data_ptr(inner) };

        // Drop guard for panic safety while cloning into the new block.
        struct InitGuard<A: Arity, T> {
            inner: NonNull<Inner<A, T>>,
            initialized: usize,
            capacity: usize,
        }
        impl<A: Arity, T> Drop for InitGuard<A, T> {
            fn drop(&mut self) {
                // SAFETY: `initialized` leading elements of a live `capacity`-sized block.
                unsafe {
                    let dp = data_ptr(self.inner);
                    core::ptr::drop_in_place(core::ptr::slice_from_raw_parts_mut(dp, self.initialized));
                    dealloc(self.inner.as_ptr().cast(), alloc_layout::<A, T>(self.capacity));
                }
            }
        }
        let mut guard = InitGuard { inner, initialized: 0, capacity: count };
        for (_i, v) in src.iter_present() {
            // SAFETY: at most `count` present elements; `dp.add(initialized)` is an
            // uninitialised in-bounds slot; `write` initialises it.
            unsafe { dp.add(guard.initialized).write(v.clone()) };
            guard.initialized += 1;
        }
        core::mem::forget(guard);
        Self(Some(inner), PhantomData)
    }
}

/// Moves each element of a `PackedArray` back into a `FixedArray<Option<T>, A>`
/// (no `T: Clone` bound).
impl<T, A: Arity> From<PackedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: PackedArray<T, A>) -> Self {
        let mut out = FixedArray::<Option<T>, A>::new();
        // Prevent `PackedArray::drop` so we can move elements out, then free.
        let src = core::mem::ManuallyDrop::new(src);
        if let Some(ptr) = src.0 {
            // SAFETY: `ptr` valid per the invariant.
            let bitmap = unsafe { ptr.as_ref().bitmap };
            let count = bitmap.count_ones() as usize;
            // SAFETY: `ptr` valid; base of `count` initialised elements.
            let dp = unsafe { data_ptr(ptr) };
            for (rank, index) in bitmap.bits().enumerate() {
                // SAFETY: `rank < count`; `dp.add(rank)` is an initialised element;
                // `read` moves it out without dropping. `ManuallyDrop` prevents a
                // double free; each element is read exactly once (bits() yields
                // each set index once, ascending == storage order).
                let v = unsafe { dp.add(rank).read() };
                out[index] = Some(v);
            }
            let layout = alloc_layout::<A, T>(count);
            // SAFETY: elements moved out; sole deallocation of this block.
            unsafe { dealloc(ptr.as_ptr().cast(), layout) };
        }
        out
    }
}

/// Clones each present element of a `&PackedArray` into a `FixedArray<Option<T>, A>`.
impl<T: Clone, A: Arity> From<&PackedArray<T, A>> for FixedArray<Option<T>, A> {
    fn from(src: &PackedArray<T, A>) -> Self {
        let mut out = FixedArray::<Option<T>, A>::new();
        for (index, v) in src.iter_present() {
            out[index] = Some(v.clone());
        }
        out
    }
}
```

> The owned reverse conversion relies on `bits()` yielding set indices in the
> same ascending order elements are stored (rank order) — `enumerate()`'s counter
> therefore equals each index's `rank`. This holds because both `From<FixedArray>`
> (storage) and `bits()` (iteration) are ascending.

- [ ] **Step 4: Run tests + clippy + commit**

Run: `cargo test -p arity-arrays --lib packed` then `cargo clippy -p arity-arrays --all-targets`
Expected: PASS; clean.

```bash
git add crates/arity-arrays/src/packed.rs
git commit -m "feat(arity-arrays): add by-ref and reverse PackedArray<->FixedArray conversions"
```

---

### Task 8: Cross-arity property tests, doctest, doc build, Miri

Property-test round-trips across all six arities, add a crate doctest, and run the full unsafe surface under Miri.

**Files:**
- Create: `crates/arity-arrays/tests/roundtrip.rs`
- Modify: `crates/arity-arrays/Cargo.toml` (add `proptest` dev-dep)
- Modify: `crates/arity-arrays/src/lib.rs` (crate doctest)

**Interfaces:**
- Consumes the public API only.

- [ ] **Step 1: Add the dev-dependency**

Run: `cargo add --package arity-arrays --dev proptest`

- [ ] **Step 2: Write the round-trip property tests**

Create `crates/arity-arrays/tests/roundtrip.rs`:

```rust
//! `FixedArray<Option<T>, A>` → `PackedArray` → back is the identity, for every
//! arity, checked against a `BTreeMap` reference of the chosen slots.

use std::collections::BTreeMap;

use arity_arrays::index::Niche;
use arity_arrays::{Arity, FixedArray, PackedArray};
use proptest::prelude::*;

fn check<A: Arity>(present: &BTreeMap<usize, u32>) {
    let mut src = FixedArray::<Option<u32>, A>::new();
    for (&i, &v) in present {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        src[idx] = Some(v);
    }
    let packed = PackedArray::from(&src);
    // count + membership + values
    assert_eq!(packed.count(), present.len());
    for i in 0..A::LEN {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        assert_eq!(packed.get(idx), present.get(&i));
    }
    // iter_present is ascending and matches the model (forward and reverse)
    let fwd: Vec<(usize, u32)> = packed.iter_present().map(|(i, &v)| (i.as_usize(), v)).collect();
    let model: Vec<(usize, u32)> = present.iter().map(|(&i, &v)| (i, v)).collect();
    assert_eq!(fwd, model);
    let mut back: Vec<(usize, u32)> = packed.iter_present().rev().map(|(i, &v)| (i.as_usize(), v)).collect();
    back.reverse();
    assert_eq!(back, model);
    // owned round-trip is the identity
    let recovered: FixedArray<Option<u32>, A> = PackedArray::from(&src).into();
    for i in 0..A::LEN {
        let idx = A::Index::try_from_usize(i).expect("i < LEN");
        assert_eq!(*recovered.get(idx), present.get(&i).copied());
    }
}

macro_rules! roundtrip_for {
    ($test:ident, $arity:ty, $len:expr) => {
        proptest! {
            #[test]
            fn $test(entries in proptest::collection::vec((0usize..$len, any::<u32>()), 0..$len)) {
                let model: BTreeMap<usize, u32> = entries.into_iter().collect();
                check::<$arity>(&model);
            }
        }
    };
}

roundtrip_for!(arity8, arity_arrays::Arity8, 8);
roundtrip_for!(arity16, arity_arrays::Arity16, 16);
roundtrip_for!(arity32, arity_arrays::Arity32, 32);
roundtrip_for!(arity64, arity_arrays::Arity64, 64);
roundtrip_for!(arity128, arity_arrays::Arity128, 128);
roundtrip_for!(arity256, arity_arrays::Arity256, 256);
```

- [ ] **Step 3: Run the property tests**

Run: `cargo test -p arity-arrays --test roundtrip`
Expected: PASS (all six arities).

- [ ] **Step 4: Add a crate doctest to `lib.rs`**

Append to the crate-level `//!` block:

```rust
//!
//! ```
//! use arity_arrays::{Arity16, FixedArray, PackedArray};
//! use arity_arrays::index::{Niche, U4};
//!
//! let mut full = FixedArray::<Option<u32>, Arity16>::new();
//! full[U4::new_masked(1)] = Some(10);
//! full[U4::new_masked(9)] = Some(90);
//!
//! // Pack: pointer-sized handle, two elements on the heap.
//! let packed = PackedArray::from(&full);
//! assert_eq!(packed.count(), 2);
//! assert_eq!(packed.get(U4::new_masked(9)), Some(&90));
//!
//! let present: alloc::vec::Vec<(u8, u32)> =
//!     packed.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
//! assert_eq!(present, alloc::vec![(1, 10), (9, 90)]);
//! # extern crate alloc;
//! ```
```

- [ ] **Step 5: Run doctest + doc build**

Run: `cargo test -p arity-arrays --doc` then `RUSTDOCFLAGS="-D warnings" cargo doc -p arity-arrays --no-deps`
Expected: PASS; docs clean.

- [ ] **Step 6: Run under Miri**

Run: `cargo +nightly miri test -p arity-arrays --lib`
Expected: PASS — no UB/leaks in the alloc/get/drop/clone/conversion paths.

> Run Miri on `--lib` (the unit tests cover the full `unsafe` surface with small
> inputs). The `roundtrip` proptest target is many-cases and slow under Miri; if
> you want Miri coverage of conversions specifically, run
> `PROPTEST_CASES=4 cargo +nightly miri test -p arity-arrays --test roundtrip`
> once, but `--lib` is the gating run. Install Miri via `rustup +nightly component
> add miri` / `cargo +nightly miri setup` if needed.

- [ ] **Step 7: Final clippy + fmt + commit**

Run: `cargo clippy -p arity-arrays --all-targets --all-features` then `cargo +nightly fmt --all --check`
Expected: clean.

```bash
git add crates/arity-arrays/tests/roundtrip.rs crates/arity-arrays/Cargo.toml crates/arity-arrays/src/lib.rs
git commit -m "test(arity-arrays): cross-arity round-trip proptests; add doctest; Miri-verify"
```

---

### Task 9: Closing phase — CI, MSRV, package metadata, publish

Workspace-wide finalization: GitHub Actions CI, the MSRV bump, per-crate metadata + READMEs, and removing the publish gate.

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `Cargo.toml` (workspace: `rust-version`, drop `publish = false`)
- Modify: each `crates/*/Cargo.toml` (description, keywords, categories, readme)
- Create: `crates/arity-index/README.md`, `crates/arity-bitmap/README.md`, `crates/arity-arrays/README.md`

**Interfaces:** none (build/release infrastructure).

- [ ] **Step 1: Write the CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    name: test ${{ matrix.os }} / ${{ matrix.toolchain }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [windows-2025-vs2026, macos-26, ubuntu-26.04, ubuntu-26.04-arm]
        toolchain: [stable, nightly]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.toolchain }}
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace --all-features --all-targets
      - run: cargo test --workspace --all-features --doc

  lint:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo +nightly fmt --all --check
      - run: cargo +stable clippy --workspace --all-targets --all-features -- -D warnings

  miri:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - uses: Swatinem/rust-cache@v2
      - run: cargo +nightly miri test --workspace --lib

  msrv:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.92.0
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --workspace --all-features
      - run: cargo test --workspace --all-features

  docs:
    runs-on: ubuntu-26.04
    env:
      RUSTDOCFLAGS: -D warnings
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo doc --workspace --no-deps --all-features
```

> Confirm the four runner labels against the current GitHub Actions image catalog
> when authoring (they are newer/preview images; a renamed label fails at queue
> time). `cargo +nightly miri test --workspace --lib` skips the slow proptest
> integration targets — the `--lib` unit tests cover the `unsafe` surface.

- [ ] **Step 2: Bump the workspace MSRV and drop the publish gate**

In the root `Cargo.toml` `[workspace.package]`, change `rust-version = "1.85"` to
`rust-version = "1.92"`, and **remove** the `publish = false` line. Then in each
crate that inherits it (`publish.workspace = true` lines), remove those lines
(crates publish by default once the workspace gate is gone).

- [ ] **Step 3: Add per-crate metadata**

For each crate's `Cargo.toml` `[package]` section add `description`, `readme`,
`keywords`, and `categories` (the table is from the spec):

```toml
# crates/arity-index/Cargo.toml
description = "Bounds-check-free niche integer index types (U3-U7) with double-ended range iterators"
readme = "README.md"
keywords = ["niche", "integer", "index", "no-std", "bitfield"]
categories = ["no-std", "data-structures", "rust-patterns"]
```
```toml
# crates/arity-bitmap/Cargo.toml
description = "Fixed-width bitmaps (u8-u128, U256) indexed by niche integers, with a double-ended set-bit iterator"
readme = "README.md"
keywords = ["bitmap", "bitset", "niche", "no-std", "u256"]
categories = ["no-std", "data-structures"]
```
```toml
# crates/arity-arrays/Cargo.toml
description = "Fixed and pointer-sized heap-packed arrays over a generic arity, indexed without bounds checks"
readme = "README.md"
keywords = ["array", "sparse", "packed", "trie", "no-std"]
categories = ["no-std", "data-structures", "memory-management"]
```

> Edit these `Cargo.toml` keys directly (they are static metadata, not
> dependencies — the `cargo add`-only rule applies to dependencies).

- [ ] **Step 4: Write the per-crate READMEs**

Create a short `README.md` for each crate (title, one-paragraph summary, a minimal
usage example, MSRV/`no_std` note, license). Keep each consistent with the crate's
`description`. (Content is straightforward prose; mirror the crate doc comment.)

- [ ] **Step 5: Verify the workspace builds clean and dry-run publish**

Run: `cargo build --workspace --all-features` then
`cargo clippy --workspace --all-targets --all-features` then
`cargo +nightly fmt --all --check` then
`cargo doc --workspace --no-deps`.
Expected: all clean (no `cargo_common_metadata` warnings now that metadata is filled).

Then dry-run publish in dependency order:
Run: `cargo publish -p arity-index --dry-run`, then `arity-bitmap`, then `arity-arrays`.
Expected: each packages successfully (path deps resolve via the workspace versions).

> If `cargo publish --dry-run` on `arity-bitmap`/`arity-arrays` errors that the
> path dependencies are unpublished, that is expected for an un-released
> workspace — confirm the rest of the packaging (file list, metadata) is correct
> and note it; real publication happens when the crates are released in order.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml Cargo.toml crates/*/Cargo.toml crates/*/README.md
git commit -m "chore: add CI, fill package metadata, bump MSRV to 1.92, enable publishing"
```

---

## Self-Review

**Spec coverage** (against the `arity-arrays`, CI, and publishing sections):

- `Arity` trait + six markers, sealed, with `Index::COUNT == LEN == Bitmap::WIDTH == Size::USIZE` compile-time asserts → Task 1 ✓
- `FixedArray<T, A>`: `from_fn`, `get`/`get_mut` (unchecked), `replace`, `Index`/`IndexMut`, `Deref`/`AsRef<[T]>`, `IntoIterator` ×3 (zipping `Index::all()`, double-ended), `map` → Task 2 ✓; `FixedArray<Option<T>>`: `new`/`count`/`take`/`iter_present`/`take_only_child` → Task 3 ✓
- `PackedArray<T, A>`: pointer-sized (asserted), `repr(C) Inner` + `&raw mut … data`, `alloc_layout`, `new`/`Default`/`bitmap`/`count`/`get` (rank-select), `From<FixedArray<Option<T>>>` → Task 4 ✓; double-ended `iter_present` (on `bits()`, rank against the original snapshot) + all-slots `iter` → Task 5 ✓; `Drop`, panic-safe `Clone` (`CloneGuard`), `Eq`/`Hash`/`Debug`/`Send`/`Sync`/unwind-safety → Task 6 ✓; all four conversions → Tasks 4 + 7 ✓
- `unsafe` quality bar (documented blocks, Miri), cross-arity + round-trip proptests, doctest → Task 8 ✓
- CI (four images × stable/nightly + lint/miri/msrv/docs), MSRV bump to 1.92, package metadata + READMEs, publish-flag removal, dry-run → Task 9 ✓
- `hybrid_array::Array` kept out of public signatures (Deref/AsRef only) → Task 2 ✓
- `PackedArray` immutable-after-construction → no `insert`/`remove` anywhere ✓

**Intentional deferrals (YAGNI, noted):** firewood's `Children::each_ref`/`each_mut`/`merge` are **not** ported — they are not needed by the arrays' own functionality or the conversions, and `each_ref` is awkward over `hybrid_array::Array`. Addable later without breaking changes. (If strict firewood parity is wanted, add a follow-up task.)

**Placeholder scan:** none. Two implementation-note callouts (the `hybrid_array::IntoIter` associated-type names in Task 2, and the `PackedAllIter` borrow fallback in Task 5) are concrete "if the compiler disagrees, do X" guidance, not unfinished work.

**Type consistency:** `A::Index`/`A::Bitmap`/`A::Size`, `Niche::{as_usize, try_from_usize, all}`, `Bitmap::{ZERO, is_zero, count_ones, test, with_bit, rank, bits}`, `data_ptr`/`alloc_layout`/`Inner`, and the iterator type names (`PackedPresentIter`/`PackedAllIter`) are used consistently across tasks. `From` directions match the spec's symmetric set.
