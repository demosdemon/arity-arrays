# Hardening Plan 3 — In-Place PackedArray Mutation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add in-place mutation to `PackedArray` — `insert`/`remove`/`get_mut` — so callers no longer round-trip through `FixedArray`, landed behind a `cargo-fuzz` + Miri verification gate.

**Architecture:** Mutation keeps the exact-size, occupancy-proportional layout: a popcount-changing op allocates a new block (via the existing `alloc_block`) and `ptr::copy_nonoverlapping`s the elements in two segments around the insertion/removal point, then frees the old block without dropping (the elements were moved). Overwrite and `get_mut` are in-place via `data_ptr`. Element relocation runs no user code, so unlike `Clone`/`From<&_>` the mutation paths need no `FillGuard`.

**Tech Stack:** Rust (edition 2024, `#![no_std]` + `alloc`), raw allocation/pointer arithmetic under the existing documented-`unsafe` discipline; `proptest` (dev) for the oracle test; `cargo-fuzz` + `libfuzzer-sys` + `arbitrary` for the fuzz gate.

This is **plan 3 of 5** for the production-hardening effort
(`breaking-api` ✓ → `features-ci` ✓ → **`mutation`** → `serde-ethnum` → `publish`).
Design spec: `docs/superpowers/specs/2026-06-27-arity-arrays-hardening-design.md`
(section "In-place `PackedArray` mutation" + the `/// # Safety` docs on
`PackedArray`/`PackedAllIter` the spec defers to this plan).

## Global Constraints

Copied from the spec and existing conventions; every task implicitly includes these.

- **`#![no_std]` + `extern crate alloc`** in `arity-arrays`. Use `core::`/`alloc::` paths only.
- **`unsafe` discipline (workspace, `deny`):** every `unsafe` block carries a `// SAFETY:` comment naming the invariant it relies on; `unsafe_op_in_unsafe_fn` and `undocumented_unsafe_blocks` are `deny`. This plan adds the **largest new `unsafe`** in the workspace — hold the bar.
- **Lints strict (CI denies warnings):** `clippy::pedantic` + `clippy::nursery` warn, `clippy::unwrap_used` warn. **No `.unwrap()`** in lib or tests — `.expect("…")` or pattern matching. **No `#[allow]`**; `#[expect(reason="…")]` only where unavoidable, scoped tightly.
- **Mutation preserves the layout invariants:** exact-size (no spare capacity), heap cost strictly `bitmap + occupancy·size_of::<T>()`, and **"allocated ⇒ `bitmap != ZERO`"** (a `remove` that empties the array deallocates and stores `None`).
- **No `FillGuard` on the mutation paths:** element relocation is `ptr::copy` of bits (no user code, cannot panic); the only fallible step is allocation, which `alloc_block` routes through `handle_alloc_error` before any move. A `debug_assert_eq!` confirms the new bitmap's popcount equals the allocated element count.
- **Tests run under the default (all-arity) feature set** (per plan 2): new tests reference concrete `Arity{N}` markers and compile only with default features; never gate test code.
- Edition 2024, MSRV 1.92. Add deps with `cargo add`. Conventional-commit messages, imperative mood.
- **Line numbers are indicative; the quoted anchor text and shown code block govern.** Confirm anchors with `grep -n` before editing.

---

### Task 1: `get_mut` + safety-invariant documentation

Add the simplest mutation accessor (`get_mut`, no realloc) and the `/// # Safety` invariant docs the spec defers to this plan. This establishes the documented invariant that Task 2's realloc code must uphold.

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs` (the `PackedArray` struct doc near line 27; the main `impl<T, A: Arity> PackedArray<T, A>` block, after `iter()` near line 245; the `PackedAllIter` struct doc near line 415)
- Test: `crates/arity-arrays/src/packed.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `data_ptr<A, T>(NonNull<Inner<A, T>>) -> *mut T` (unsafe); `A::Bitmap::test`/`rank` (from `arity_bitmap::Bitmap`).
- Produces: `PackedArray::get_mut(&mut self, index: A::Index) -> Option<&mut T>`. Task 4's fuzz target and Task 3's oracle consume it.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/arity-arrays/src/packed.rs`:

```rust
#[test]
fn get_mut_mutates_present_only() {
    let mut src = FixedArray::<Option<u8>, Arity16>::new();
    src[U4::new_masked(2)] = Some(20);
    src[U4::new_masked(9)] = Some(90);
    let mut p = PackedArray::from(src);

    // Mutate a present slot through the &mut.
    if let Some(v) = p.get_mut(U4::new_masked(9)) {
        *v = 99;
    }
    assert_eq!(p.get(U4::new_masked(9)), Some(&99));
    assert_eq!(p.get(U4::new_masked(2)), Some(&20));

    // Absent slot yields None.
    assert!(p.get_mut(U4::new_masked(5)).is_none());
    // Empty array yields None.
    let mut empty = PackedArray::<u8, Arity16>::new();
    assert!(empty.get_mut(U4::new_masked(0)).is_none());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p arity-arrays get_mut_mutates_present_only`
Expected: FAIL to compile — `no method named `get_mut``.

- [ ] **Step 3: Implement `get_mut`**

In `crates/arity-arrays/src/packed.rs`, in the main `impl<T, A: Arity> PackedArray<T, A>` block, immediately after the `iter()` method (the method ending `}` just before the block's closing `}` near line 245-246), add:

```rust
    /// Returns a mutable reference to the element at `index`, or `None` if
    /// absent. Does not change which slots are present (no reallocation).
    pub fn get_mut(&mut self, index: A::Index) -> Option<&mut T> {
        let ptr = self.0?;
        // SAFETY: `ptr` is valid per the type invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        if !bm.test(index) {
            return None;
        }
        let rank = bm.rank(index) as usize;
        // SAFETY: `index` is present, so `rank < count`; `data_ptr(ptr).add(rank)`
        // is an initialised element within the allocation. The borrow is tied to
        // `&mut self`, which gives exclusive access for its lifetime.
        Some(unsafe { &mut *data_ptr(ptr).add(rank) })
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p arity-arrays get_mut_mutates_present_only`
Expected: PASS.

- [ ] **Step 5: Add the `# Safety` invariant doc to `PackedArray`**

In `crates/arity-arrays/src/packed.rs`, replace the `PackedArray` doc comment (currently near line 27, the block beginning `/// An immutable, pointer-sized, heap-packed array over arity `A`.`) with — note the word "immutable" is removed, since this plan adds mutation:

```rust
/// A pointer-sized, heap-packed array over arity `A`, storing only the present
/// elements.
///
/// `None` ↔ empty (no allocation). `Some(ptr)` ↔ a heap block sized to exactly
/// the present elements. The `NonNull` null-pointer niche makes this type the
/// size of a pointer for every `A`.
///
/// # Safety
///
/// Invariant upheld by every constructor and mutator: when `self.0` is
/// `Some(ptr)`, `ptr` points to a live allocation from
/// `alloc_layout::<A, T>(count)` whose `bitmap` field is initialised with
/// `bitmap != A::Bitmap::ZERO`, and whose `count == bitmap.count_ones()` element
/// slots are all initialised in ascending slot (rank) order. When `self.0` is
/// `None`, there is no allocation. The `unsafe` reads throughout this module
/// rely on this invariant.
```

- [ ] **Step 6: Add the `# Safety` invariant doc to `PackedAllIter`**

In the same file, append a `# Safety` section to the `PackedAllIter` doc comment (currently near line 415, the block beginning `/// Iterator over all slots of a [`PackedArray`].`). Add these lines immediately before the `pub struct PackedAllIter` line, after the existing doc text:

```rust
///
/// # Safety
///
/// Invariant: `front_rank` counts the present slots yielded from the front and
/// `back_consumed` counts those yielded from the back, with
/// `front_rank + back_consumed <= count` at all times. Because `slots`
/// partitions the index domain between the two ends, no present slot is counted
/// by both, so each computed dense rank is `< count` — which
/// [`PackedArray::elem_at_rank`] requires.
```

- [ ] **Step 7: Verify the crate is clean (lib + docs)**

Run:
```bash
cargo test -p arity-arrays get_mut_mutates_present_only
cargo clippy -p arity-arrays --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p arity-arrays --no-deps --all-features
```
Expected: test passes; clippy clean; docs build with no warnings (the new `# Safety` sections render; `[`PackedArray::elem_at_rank`]` resolves — `elem_at_rank` is a private method, so use the plain text `elem_at_rank` without intra-doc link if rustdoc warns about a private-item link: if `RUSTDOCFLAGS="-D warnings"` fails on that link, change `[`PackedArray::elem_at_rank`]` to `` `elem_at_rank` `` in Step 6).

- [ ] **Step 8: Commit**

```bash
git add crates/arity-arrays/src/packed.rs
git commit -m "feat(arity-arrays): add PackedArray::get_mut and document invariants"
```

---

### Task 2: `insert` and `remove`

The realloc pair — the largest new `unsafe`. `insert` overwrites in place or grows by one; `remove` shrinks by one or deallocates to empty.

**Files:**
- Modify: `crates/arity-arrays/src/packed.rs` (main `impl` block, after `get_mut`; add the `dealloc`/`copy_nonoverlapping`/`mem::replace` imports if not already present)
- Test: `crates/arity-arrays/src/packed.rs` (`mod tests`)

**Interfaces:**
- Consumes: `alloc_block<A, T>(A::Bitmap, usize) -> NonNull<Inner<A, T>>` (unsafe; precondition `count == bitmap.count_ones() > 0`); `data_ptr`; `alloc_layout::<A, T>(count)`; `dealloc`; `A::Bitmap::{with_bit, without_bit, test, rank, count_ones}`.
- Produces: `PackedArray::insert(&mut self, A::Index, T) -> Option<T>` and `PackedArray::remove(&mut self, A::Index) -> Option<T>`. Tasks 3–4 consume them.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/arity-arrays/src/packed.rs`:

```rust
#[test]
fn insert_into_empty_and_overwrite() {
    let mut p = PackedArray::<u8, Arity16>::new();
    assert_eq!(p.insert(U4::new_masked(7), 70), None); // into empty
    assert_eq!(p.count(), 1);
    assert_eq!(p.get(U4::new_masked(7)), Some(&70));
    // Overwrite returns the old value, no count change.
    assert_eq!(p.insert(U4::new_masked(7), 77), Some(70));
    assert_eq!(p.count(), 1);
    assert_eq!(p.get(U4::new_masked(7)), Some(&77));
}

#[test]
fn insert_grows_and_preserves_order() {
    let mut p = PackedArray::<u8, Arity16>::new();
    // Insert out of order; storage stays ascending by slot.
    assert_eq!(p.insert(U4::new_masked(9), 90), None); // back
    assert_eq!(p.insert(U4::new_masked(1), 10), None); // front
    assert_eq!(p.insert(U4::new_masked(5), 50), None); // middle
    assert_eq!(p.count(), 3);
    let present: alloc::vec::Vec<(u8, u8)> =
        p.iter_present().map(|(i, &v)| (i.as_u8(), v)).collect();
    assert_eq!(present, alloc::vec![(1, 10), (5, 50), (9, 90)]);
}

#[test]
fn remove_present_absent_and_to_empty() {
    let mut src = FixedArray::<Option<u8>, Arity16>::new();
    src[U4::new_masked(1)] = Some(10);
    src[U4::new_masked(5)] = Some(50);
    src[U4::new_masked(9)] = Some(90);
    let mut p = PackedArray::from(src);

    assert_eq!(p.remove(U4::new_masked(5)), Some(50)); // middle
    assert_eq!(p.count(), 2);
    assert_eq!(p.get(U4::new_masked(5)), None);
    assert_eq!(p.get(U4::new_masked(1)), Some(&10));
    assert_eq!(p.get(U4::new_masked(9)), Some(&90));

    assert_eq!(p.remove(U4::new_masked(5)), None); // absent
    assert_eq!(p.remove(U4::new_masked(1)), Some(10));
    assert_eq!(p.remove(U4::new_masked(9)), Some(90)); // last → empty
    assert!(p.is_empty());
    assert_eq!(p.bitmap(), <u16 as Bitmap>::ZERO);
    assert_eq!(p.remove(U4::new_masked(0)), None); // remove from empty
}

#[test]
fn insert_remove_arity256_and_zst() {
    // Arity-256 boundary slots.
    let mut p = PackedArray::<u16, Arity256>::new();
    assert_eq!(p.insert(0, 1), None);
    assert_eq!(p.insert(255, 2), None);
    assert_eq!(p.insert(128, 3), None);
    assert_eq!(p.count(), 3);
    assert_eq!(p.get(128), Some(&3));
    assert_eq!(p.remove(128), Some(3));
    assert_eq!(p.get(128), None);
    assert_eq!(p.count(), 2);

    // Zero-sized T: writes/reads are no-ops, but rank-select still holds.
    let mut z = PackedArray::<(), Arity16>::new();
    assert_eq!(z.insert(U4::new_masked(3), ()), None);
    assert_eq!(z.insert(U4::new_masked(0), ()), None);
    assert_eq!(z.count(), 2);
    assert_eq!(z.get(U4::new_masked(3)), Some(&()));
    assert_eq!(z.remove(U4::new_masked(0)), Some(()));
    assert_eq!(z.count(), 1);
    assert_eq!(z.get(U4::new_masked(3)), Some(&()));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p arity-arrays insert_ remove_`
Expected: FAIL to compile — `no method named `insert`/`remove``.

- [ ] **Step 3: Ensure the needed imports are present**

At the top of `crates/arity-arrays/src/packed.rs`, the imports already include `alloc::alloc::dealloc`, `core::ptr::NonNull`, etc. Confirm with `grep -n 'use ' crates/arity-arrays/src/packed.rs`. The new code uses `core::ptr::copy_nonoverlapping` and `core::mem::replace` via fully-qualified paths (no new `use` needed).

- [ ] **Step 4: Implement `insert` and `remove`**

In the main `impl<T, A: Arity> PackedArray<T, A>` block, immediately after the `get_mut` method from Task 1, add:

```rust
    /// Inserts `value` at `index`, returning the previous value if the slot was
    /// already present (otherwise `None`).
    ///
    /// On a new insertion the array reallocates to exactly hold one more element
    /// (`O(count)` move). Overwriting a present slot is in place.
    pub fn insert(&mut self, index: A::Index, value: T) -> Option<T> {
        let Some(ptr) = self.0 else {
            // Empty → fresh single-element block.
            let bitmap = A::Bitmap::ZERO.with_bit(index);
            debug_assert_eq!(bitmap.count_ones(), 1);
            // SAFETY: `count == 1 == bitmap.count_ones() > 0`; the write below
            // initialises the sole element slot before any read.
            let inner = unsafe { alloc_block::<A, T>(bitmap, 1) };
            // SAFETY: `inner` is freshly allocated; `data_ptr(inner)` is its first
            // (uninitialised) element slot; `write` initialises it.
            unsafe { data_ptr(inner).write(value) };
            self.0 = Some(inner);
            return None;
        };
        // SAFETY: `ptr` is valid per the type invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        let rank = bm.rank(index) as usize;
        if bm.test(index) {
            // Present → overwrite in place; return the old value.
            // SAFETY: `index` present ⇒ `rank < count`; `data_ptr(ptr).add(rank)`
            // is an initialised element; `&mut self` gives exclusive access, so
            // forming `&mut *slot` and `mem::replace` (read old, write new) is
            // sound and drops nothing.
            Some(unsafe { core::mem::replace(&mut *data_ptr(ptr).add(rank), value) })
        } else {
            // Absent → grow by one: new block, copy the two segments around `rank`.
            let old_count = bm.count_ones() as usize;
            let new_count = old_count + 1;
            let new_bm = bm.with_bit(index);
            debug_assert_eq!(new_bm.count_ones() as usize, new_count);
            // SAFETY: `new_count == new_bm.count_ones() > 0`; the copies + write
            // below initialise all `new_count` slots before any read.
            let new_inner = unsafe { alloc_block::<A, T>(new_bm, new_count) };
            // SAFETY: both pointers are valid element bases (old per invariant,
            // new freshly allocated).
            let src = unsafe { data_ptr(ptr) };
            let dst = unsafe { data_ptr(new_inner) };
            // SAFETY: `rank <= old_count`. Copy `[0, rank)` to `dst[0..]`, write
            // `value` at `dst[rank]`, copy the old `[rank, old_count)` to
            // `dst[rank+1..]`. `copy_nonoverlapping` moves the elements bitwise
            // (no drop, no user code); the old block is freed without dropping.
            unsafe {
                core::ptr::copy_nonoverlapping(src, dst, rank);
                dst.add(rank).write(value);
                core::ptr::copy_nonoverlapping(src.add(rank), dst.add(rank + 1), old_count - rank);
            }
            // SAFETY: the old block came from `alloc_layout::<A, T>(old_count)`;
            // its elements were moved out above, so free it without dropping.
            unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<A, T>(old_count)) };
            self.0 = Some(new_inner);
            None
        }
    }

    /// Removes and returns the element at `index`, or `None` if absent.
    ///
    /// Reallocates to exactly hold one fewer element (`O(count)` move); removing
    /// the last element deallocates and leaves the array empty.
    pub fn remove(&mut self, index: A::Index) -> Option<T> {
        let ptr = self.0?;
        // SAFETY: `ptr` is valid per the type invariant.
        let bm = unsafe { ptr.as_ref().bitmap };
        if !bm.test(index) {
            return None;
        }
        let rank = bm.rank(index) as usize;
        let old_count = bm.count_ones() as usize;
        // SAFETY: `index` present ⇒ `rank < old_count`; `read` moves the element
        // out (it is not dropped here — it is returned to the caller).
        let removed = unsafe { data_ptr(ptr).add(rank).read() };
        let new_count = old_count - 1;
        if new_count == 0 {
            // Last element removed → deallocate, become empty (upholds
            // "allocated ⇒ bitmap != ZERO").
            // SAFETY: the sole element was moved out above; free the old block.
            unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<A, T>(old_count)) };
            self.0 = None;
        } else {
            let new_bm = bm.without_bit(index);
            debug_assert_eq!(new_bm.count_ones() as usize, new_count);
            // SAFETY: `new_count == new_bm.count_ones() > 0`; the two copies below
            // initialise all `new_count` slots before any read.
            let new_inner = unsafe { alloc_block::<A, T>(new_bm, new_count) };
            // SAFETY: both pointers are valid element bases.
            let src = unsafe { data_ptr(ptr) };
            let dst = unsafe { data_ptr(new_inner) };
            // SAFETY: copy the survivors `[0, rank)` and `[rank+1, old_count)`
            // around the already-read-out slot `rank`. Bitwise move, no drop.
            unsafe {
                core::ptr::copy_nonoverlapping(src, dst, rank);
                core::ptr::copy_nonoverlapping(src.add(rank + 1), dst.add(rank), old_count - rank - 1);
            }
            // SAFETY: survivors moved, removed element read out; free the old block.
            unsafe { dealloc(ptr.as_ptr().cast(), alloc_layout::<A, T>(old_count)) };
            self.0 = Some(new_inner);
        }
        Some(removed)
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p arity-arrays insert_ remove_`
Expected: PASS (all four tests).

- [ ] **Step 6: Verify the crate is clean**

Run: `cargo test -p arity-arrays && cargo clippy -p arity-arrays --all-targets --all-features -- -D warnings`
Expected: all tests pass; clippy clean.

- [ ] **Step 7: Verify under Miri (the mutation paths)**

Run: `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-disable-isolation" cargo +nightly miri test -p arity-arrays insert_ remove_ get_mut_`
Expected: PASS with no Miri errors (no provenance/alignment/leak/use-after-free in the alloc/copy/dealloc paths).

- [ ] **Step 8: Commit**

```bash
git add crates/arity-arrays/src/packed.rs
git commit -m "feat(arity-arrays): add in-place PackedArray insert and remove"
```

---

### Task 3: oracle property test + drop-exactly-once under mutation

A `proptest` op-sequence against a `BTreeMap` oracle (correctness), plus a drop-counting unit test (no leak / no double-drop across `insert`/`remove`/overwrite).

**Files:**
- Create: `crates/arity-arrays/tests/mutation.rs`
- Test: same file

**Interfaces:**
- Consumes: `PackedArray::{insert, remove, get, get_mut, count}` (Tasks 1–2).
- Produces: nothing (test-only).

- [ ] **Step 1: Write the oracle proptest and the drop-count test**

Create `crates/arity-arrays/tests/mutation.rs`:

```rust
//! Property and drop-safety tests for in-place `PackedArray` mutation.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use arity_arrays::index::U4;
use arity_arrays::{Arity16, PackedArray};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Insert(u8, u16),
    Remove(u8),
    GetMut(u8, u16),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u8..16, any::<u16>()).prop_map(|(s, v)| Op::Insert(s, v)),
        (0u8..16).prop_map(Op::Remove),
        (0u8..16, any::<u16>()).prop_map(|(s, v)| Op::GetMut(s, v)),
    ]
}

proptest! {
    #[test]
    fn packed_mutation_matches_btreemap(ops in proptest::collection::vec(op_strategy(), 0..200)) {
        let mut packed: PackedArray<u16, Arity16> = PackedArray::new();
        let mut oracle: BTreeMap<u8, u16> = BTreeMap::new();

        for op in ops {
            match op {
                Op::Insert(slot, val) => {
                    let i = U4::new_masked(slot);
                    let prev_p = packed.insert(i, val);
                    let prev_o = oracle.insert(i.as_u8(), val);
                    prop_assert_eq!(prev_p, prev_o);
                }
                Op::Remove(slot) => {
                    let i = U4::new_masked(slot);
                    prop_assert_eq!(packed.remove(i), oracle.remove(&i.as_u8()));
                }
                Op::GetMut(slot, val) => {
                    let i = U4::new_masked(slot);
                    if let Some(p) = packed.get_mut(i) { *p = val; }
                    if let Some(o) = oracle.get_mut(&i.as_u8()) { *o = val; }
                }
            }
            // Full-state cross-check after every op.
            prop_assert_eq!(packed.count(), oracle.len());
            for slot in 0..16u8 {
                let i = U4::new_masked(slot);
                prop_assert_eq!(packed.get(i), oracle.get(&i.as_u8()));
            }
        }
    }
}

/// A value that bumps a shared counter on drop, to detect leaks / double-drops.
struct Counted(Arc<AtomicUsize>);
impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn mutation_drops_each_element_exactly_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut p = PackedArray::<Counted, Arity16>::new();

    // Insert 4 elements (no drops yet).
    for s in [1u8, 4, 9, 14] {
        assert!(p.insert(U4::new_masked(s), Counted(drops.clone())).is_none());
    }
    assert_eq!(drops.load(Ordering::SeqCst), 0);

    // Overwrite slot 4: the old value is returned and dropped here.
    let old = p.insert(U4::new_masked(4), Counted(drops.clone()));
    assert!(old.is_some());
    drop(old);
    assert_eq!(drops.load(Ordering::SeqCst), 1);

    // Remove slot 9: returned then dropped.
    let r = p.remove(U4::new_masked(9));
    assert!(r.is_some());
    drop(r);
    assert_eq!(drops.load(Ordering::SeqCst), 2);

    // Drop the array: the remaining 3 elements (slots 1, 4, 14) drop exactly once.
    drop(p);
    assert_eq!(drops.load(Ordering::SeqCst), 5);
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p arity-arrays --test mutation`
Expected: PASS — `packed_mutation_matches_btreemap` (256 proptest cases) and `mutation_drops_each_element_exactly_once`.

- [ ] **Step 3: Verify the oracle test under Miri (bounded cases)**

Run: `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-disable-isolation" PROPTEST_CASES=16 cargo +nightly miri test -p arity-arrays --test mutation`
Expected: PASS with no Miri errors. (Plan 2's CI `miri` job runs `--tests`, so this file is covered in CI automatically; this step confirms it locally.)

- [ ] **Step 4: Verify clippy on the new test target**

Run: `cargo clippy -p arity-arrays --test mutation --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-arrays/tests/mutation.rs
git commit -m "test(arity-arrays): oracle proptest and drop-count test for mutation"
```

---

### Task 4: `cargo-fuzz` target — the fuzz gate

A `cargo-fuzz` target driving randomized `insert`/`remove`/`get`/`get_mut`/`clone`/`drop` sequences against a `BTreeMap` oracle with a heap-owning value type (so the libFuzzer+ASAN run catches leaks/double-frees). This is the verification gate for the new `unsafe`.

**Files:**
- Create: `fuzz/Cargo.toml`
- Create: `fuzz/fuzz_targets/packed_ops.rs`
- Create: `fuzz/.gitignore`

**Interfaces:**
- Consumes: the public `PackedArray` mutation API (Tasks 1–2).
- Produces: a `packed_ops` fuzz binary, run by Task 5's CI job.

> The `fuzz/` crate is **not** a workspace member (`members = ["crates/*"]` excludes it) and builds only via `cargo +nightly fuzz run`. It must not perturb `cargo build --workspace`.

- [ ] **Step 1: Create `fuzz/Cargo.toml`**

```toml
[package]
name = "arity-arrays-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
arbitrary = { version = "1", features = ["derive"] }

[dependencies.arity-arrays]
path = "../crates/arity-arrays"

[[bin]]
name = "packed_ops"
path = "fuzz_targets/packed_ops.rs"
test = false
doc = false
bench = false

# Build the fuzz target with debug assertions and overflow checks on.
[profile.release]
debug = true
debug-assertions = true
overflow-checks = true
```

- [ ] **Step 2: Create `fuzz/.gitignore`**

```gitignore
target
corpus
artifacts
coverage
```

- [ ] **Step 3: Create `fuzz/fuzz_targets/packed_ops.rs`**

```rust
#![no_main]

use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use arity_arrays::index::U4;
use arity_arrays::{Arity16, PackedArray};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
enum Op {
    Insert(u8, Vec<u8>),
    Remove(u8),
    GetMut(u8, Vec<u8>),
}

fuzz_target!(|ops: Vec<Op>| {
    // Heap-owning value type (`Vec<u8>`) so ASAN sees leaks / double-frees of
    // element buffers across the realloc/copy/dealloc paths.
    let mut packed: PackedArray<Vec<u8>, Arity16> = PackedArray::new();
    let mut oracle: BTreeMap<u8, Vec<u8>> = BTreeMap::new();

    for op in ops {
        match op {
            Op::Insert(slot, val) => {
                let i = U4::new_masked(slot);
                let prev_p = packed.insert(i, val.clone());
                let prev_o = oracle.insert(i.as_u8(), val);
                assert_eq!(prev_p, prev_o);
            }
            Op::Remove(slot) => {
                let i = U4::new_masked(slot);
                assert_eq!(packed.remove(i), oracle.remove(&i.as_u8()));
            }
            Op::GetMut(slot, val) => {
                let i = U4::new_masked(slot);
                if let Some(p) = packed.get_mut(i) {
                    *p = val.clone();
                }
                if let Some(o) = oracle.get_mut(&i.as_u8()) {
                    *o = val;
                }
            }
        }
        // Full-state cross-check.
        assert_eq!(packed.count(), oracle.len());
        for slot in 0..16u8 {
            let i = U4::new_masked(slot);
            assert_eq!(packed.get(i), oracle.get(&i.as_u8()));
        }
    }

    // Clone equivalence, then both drop (ASAN catches any leak / double-free).
    let cloned = packed.clone();
    assert_eq!(cloned.count(), oracle.len());
});
```

- [ ] **Step 4: Verify the fuzz target builds and runs a brief smoke pass**

Requires `cargo-fuzz` (install with `cargo binstall cargo-fuzz` or `cargo install cargo-fuzz` if not present; the repo's `mise.toml` pins it as `cargo:cargo-fuzz`).

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
cargo +nightly fuzz build packed_ops
cargo +nightly fuzz run packed_ops -- -max_total_time=30 -rss_limit_mb=4096
```
Expected: builds; the 30-second run completes with no crash/leak (`Done … 0 crashes`). If `cargo-fuzz` is not installed and cannot be installed in this environment, report DONE_WITH_CONCERNS noting the build/run could not be executed locally — the files are still committed and Task 5's CI job exercises them.

- [ ] **Step 5: Confirm the workspace build is unaffected by `fuzz/`**

Run: `cargo build --workspace && cargo test --workspace`
Expected: unchanged — the `fuzz/` crate is not a workspace member and is not built.

- [ ] **Step 6: Commit**

```bash
git add fuzz/Cargo.toml fuzz/.gitignore fuzz/fuzz_targets/packed_ops.rs
git commit -m "test(arity-arrays): add cargo-fuzz packed_ops oracle target"
```

---

### Task 5: CI `fuzz` job (time-boxed) + Miri coverage confirmation

Add a time-boxed `fuzz` job that installs `cargo-fuzz` via `mise` and runs `packed_ops`. Miri already runs `--tests` (plan 2), so the Task 3 oracle test is covered in CI automatically — confirm and note it.

**Files:**
- Modify: `.github/workflows/ci.yml` (add the `fuzz` job)

**Interfaces:**
- Consumes: the `packed_ops` target (Task 4); `mise.toml`'s `cargo:cargo-fuzz` pin (plan 2).
- Produces: a `fuzz` CI job.

- [ ] **Step 1: Add the `fuzz` job to `.github/workflows/ci.yml`**

Insert this job (after the `miri` job):

```yaml
  fuzz:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: Swatinem/rust-cache@v2
      # Installs cargo-fuzz from mise.toml (the cargo:cargo-fuzz pin).
      - uses: jdx/mise-action@v2
      # Smoke-level fuzzing: a fixed wall-clock budget catches regressions
      # without unbounded CI cost. Deeper soaks run out-of-band.
      - run: cargo +nightly fuzz run packed_ops -- -max_total_time=60 -rss_limit_mb=4096
```

- [ ] **Step 2: Validate the workflow YAML and the job wiring**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
yq '.jobs | keys' .github/workflows/ci.yml
yq '.jobs.fuzz.steps[-1].run' .github/workflows/ci.yml
```
Expected: the job list now includes `fuzz`; the last step prints the `cargo +nightly fuzz run packed_ops … -max_total_time=60 …` command. (`yq` is installed.)

- [ ] **Step 3: Confirm Miri coverage of the mutation oracle (no CI change needed)**

Run: `yq '.jobs.miri.steps[-1].run' .github/workflows/ci.yml`
Expected: `cargo +nightly miri test --workspace --tests` — i.e. the plan-2 Miri job already runs `--tests`, which includes `tests/mutation.rs`. No edit needed; this step documents that the mutation paths are Miri-covered in CI via the existing job.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add time-boxed cargo-fuzz job for packed_ops"
```

---

## Self-Review

- **Spec coverage ("In-place PackedArray mutation"):**
  - `insert`/`remove`/`get_mut` → Tasks 1–2. ✓
  - Exact-size, allocate-new + two-segment copy → Task 2 `insert`/`remove` bodies. ✓
  - Overwrite via in-place read/write → Task 2 (`mem::replace`, which is read-old + write-new, satisfying the spec's `ptr::read`/`ptr::write` intent with no double-drop). ✓
  - "allocated ⇒ `bitmap != ZERO`" (remove-to-empty deallocates) → Task 2 `remove` `new_count == 0` branch; tested in `remove_present_absent_and_to_empty`. ✓
  - No `FillGuard`; `debug_assert_eq!` popcount == count → Task 2 (the `debug_assert_eq!` calls). ✓
  - Panic-safety argument (relocation runs no user code; alloc via `handle_alloc_error`) → encoded in the SAFETY comments; no drop-guard added. ✓
  - Verification gate: `cargo-fuzz` `packed_ops` (Task 4) + Miri over the new tests (Task 2 Step 7, Task 3 Step 3, Task 5 Step 3). ✓
  - `/// # Safety` docs on `PackedArray` + `PackedAllIter` → Task 1 Steps 5–6. ✓
  - Testing: op-sequence proptest vs `BTreeMap` (Task 3); drop-exactly-once under `insert`/`remove` (Task 3); empty↔nonempty churn (covered by `remove_…_to_empty` + the proptest); fuzz `packed_ops` (Task 4). ✓
  - CI `fuzz` job (time-boxed) + `mise`/`cargo-fuzz` (Task 5). ✓
- **Deferred (not this plan):** serde/ethnum (plan 4); README "Cargo features"/publish (plan 5). The capacity-bearing sibling type and nightly-gated optimizations remain non-goals per the spec.
- **Placeholder scan:** none — every step has complete code and exact commands.
- **Type/signature consistency:** `insert(&mut self, A::Index, T) -> Option<T>`, `remove(&mut self, A::Index) -> Option<T>`, `get_mut(&mut self, A::Index) -> Option<&mut T>` are used identically across the impl, the unit tests, the oracle proptest, and the fuzz target. `alloc_block`/`data_ptr`/`alloc_layout`/`dealloc` signatures match `packed.rs` as-built.
