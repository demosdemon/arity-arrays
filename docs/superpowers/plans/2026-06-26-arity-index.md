# `arity-index` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `arity-index` crate — bounds-check-free niche integer index types `U3`–`U7`, a sealed `Niche` trait (with `u8` as the arity-256 index), and custom double-ended `NicheRange` / `NicheRangeInclusive` iterators.

**Architecture:** Each `U{n}` is a newtype over a fieldless enum with `2ⁿ` variants (`seq-macro`-generated), which earns niche optimization (`Option<U{n}>` is 1 byte) and bounds-check elision. The `Niche` trait unifies them so downstream crates can be generic; iteration over a niche type's domain is via the custom range iterators, which are the single place an index is reconstructed from a raw integer.

**Tech Stack:** Rust (edition 2024, `#![no_std]`, no `alloc`), `seq-macro` for variant generation.

This is **plan 1 of 3** for the arity-arrays project (`arity-index` → `arity-bitmap` → `arity-arrays`). It is the sole dependency leaf; nothing here depends on the other crates. The design spec is `docs/superpowers/specs/2026-06-26-arity-arrays-design.md` (sections "`arity-index`", "Range iterators", "The `Niche` trait").

## Global Constraints

- **`#![no_std]`, no `alloc`.** `arity-index/src/lib.rs` already declares `#![no_std]`. Use `core::` paths only.
- **Edition 2024.** Already set via `[workspace.package]`.
- **MSRV:** the workspace floor is currently `1.85`; `arity-index` must build on it (it uses `core::error::Error` [1.81] and `debug_assert!` in `const fn` [1.79], both within `1.85`). The bump to `1.92` happens in a later crate's plan, not here.
- **Lints (already enforced workspace-wide):** `clippy::pedantic` + `clippy::nursery` at `warn`, `clippy::unwrap_used` at `warn`, `undocumented_unsafe_blocks = "deny"`, `unsafe_op_in_unsafe_fn = "deny"`. CI runs `cargo clippy --all-targets --all-features` with warnings denied, so **test code must be clippy-clean too** — tests avoid `.unwrap()` (use `assert_eq!` on `Option`/pattern matching).
- **`unsafe` rule:** every `unsafe` block carries a `// SAFETY:` comment. No `#[allow]`; use `#[expect(reason = …)]` only where unavoidable.
- **Dependency rule:** add deps with `cargo add` (not hand-edited). `seq-macro = "0.3.6"` is already present.
- **Comments / commit messages:** imperative mood, conventional-commit style.

---

### Task 1: Crate skeleton — `sealed` module, `TryFromIntError`, `Niche` trait surface

Set up `lib.rs` (module declarations, sealed-trait module, error type) and define the `Niche` trait *without* the `all()` provided method (added in Task 6, once the range type exists). This task makes the crate compile with the public trait surface in place.

**Files:**
- Modify: `crates/arity-index/src/lib.rs`
- Create: `crates/arity-index/src/niche.rs`

**Interfaces:**
- Produces: `pub struct TryFromIntError;` (unit, `core::error::Error`); `pub trait Niche: Copy + Ord + Sized + sealed::Sealed { const COUNT: usize; fn as_usize(self) -> usize; fn try_from_usize(i: usize) -> Option<Self>; }`; `mod sealed { pub trait Sealed {} }` (crate-private).

- [ ] **Step 1: Write `lib.rs`**

```rust
#![no_std]

//! Bounds-check-free niche integer index types.
//!
//! Each `U{n}` (`U3`–`U7`) is a newtype over a fieldless enum with `2ⁿ`
//! variants, so `Option<U{n}>` is one byte (niche optimization) and indexing a
//! `2ⁿ`-length array can elide the bounds check. The [`Niche`] trait unifies the
//! index types (including the native `u8` for arity 256); iteration over a
//! type's whole domain is via [`NicheRange`] / [`NicheRangeInclusive`].

mod niche;
mod range;

// Re-exports grow as tasks land: `U3`–`U7` are added in Task 2, once the
// `niche_int!` macro defines them. Re-exporting them here before they exist
// would not compile.
pub use niche::{Niche, TryFromIntError};
pub use range::{NicheRange, NicheRangeInclusive};

mod sealed {
    /// Prevents downstream crates from implementing [`Niche`](crate::Niche).
    pub trait Sealed {}
}
```

- [ ] **Step 2: Write the `Niche` trait + error type into `niche.rs`**

```rust
//! Niche integer index types `U3`–`U7`, the [`Niche`] trait, and the `u8`
//! arity-256 index.

use crate::sealed::Sealed;

/// The error returned by `TryFrom<u8>` for a niche integer when the value is out
/// of range. Mirrors [`core::num::TryFromIntError`], which has no public
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TryFromIntError;

impl core::fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of range integral type conversion attempted")
    }
}

impl core::error::Error for TryFromIntError {}

/// A fixed-domain integer index whose value is always `< COUNT`.
///
/// Sealed: implemented only by `U3`–`U7` and `u8` (the arity-256 index).
pub trait Niche: Copy + Ord + Sized + Sealed {
    /// Number of valid values (`2^BITS`): 8, 16, 32, 64, 128, or 256.
    const COUNT: usize;

    /// Returns the value as a `usize`, always `< COUNT`.
    fn as_usize(self) -> usize;

    /// Constructs from a `usize`, or `None` if `i >= COUNT`.
    fn try_from_usize(i: usize) -> Option<Self>;
}
```

- [ ] **Step 3: Add a temporary `range.rs` stub so the crate compiles**

`lib.rs` declares `mod range;` and re-exports its types; create a minimal stub (replaced in Task 5). Create `crates/arity-index/src/range.rs`:

```rust
//! Double-ended range iterators over [`Niche`](crate::Niche) values.

use crate::Niche;
use core::marker::PhantomData;

/// Placeholder — implemented in a later task.
pub struct NicheRange<N: Niche>(PhantomData<N>);

/// Placeholder — implemented in a later task.
pub struct NicheRangeInclusive<N: Niche>(PhantomData<N>);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p arity-index`
Expected: builds with no errors (dead-code warnings on the stub structs are acceptable for now).

- [ ] **Step 5: Commit**

```bash
git add crates/arity-index/src/lib.rs crates/arity-index/src/niche.rs crates/arity-index/src/range.rs
git commit -m "feat(arity-index): scaffold Niche trait, sealed module, error type"
```

---

### Task 2: The `niche_int!` macro — generate `U3`–`U7` with their inherent API

Generate the fieldless `Repr` enum and the `U{n}` newtype with all inherent associated constants and constructors, plus `Default`, the sealed impl, and the `Niche` impl. This is the core of the crate.

**Files:**
- Modify: `crates/arity-index/src/niche.rs`
- Modify: `crates/arity-index/src/lib.rs` (extend the `niche::` re-export to add `U3, U4, U5, U6, U7`)

**Interfaces:**
- Produces, for each of `U3, U4, U5, U6, U7`: `const BITS: u32`, `const COUNT: usize`, `const MIN: Self`, `const MAX: Self`, `const fn try_new(u8) -> Option<Self>`, `const unsafe fn new_unchecked(u8) -> Self`, `const fn new_masked(u8) -> Self`, `const fn as_u8(self) -> u8`, `const fn as_usize(self) -> usize`; `impl Default`, `impl Sealed`, `impl Niche` (`as_usize`, `try_from_usize`; `COUNT`). Re-exported from the crate root.

- [ ] **Step 1: Write the failing test**

Append to `crates/arity-index/src/niche.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_accessors() {
        assert_eq!(U4::COUNT, 16);
        assert_eq!(U4::BITS, 4);
        assert_eq!(U4::MIN.as_u8(), 0);
        assert_eq!(U4::MAX.as_u8(), 15);
        assert_eq!(U4::MAX.as_usize(), 15);

        assert_eq!(U4::try_new(0), Some(U4::MIN));
        assert_eq!(U4::try_new(15), Some(U4::MAX));
        assert_eq!(U4::try_new(16), None);
        assert_eq!(U4::try_new(255), None);

        // new_masked keeps the low BITS bits.
        assert_eq!(U4::new_masked(0xF3).as_u8(), 0x3);
        assert_eq!(U3::new_masked(0xFF).as_u8(), 0x7);
    }

    #[test]
    fn domain_bounds_per_type() {
        assert_eq!(U3::COUNT, 8);
        assert_eq!(U5::COUNT, 32);
        assert_eq!(U6::COUNT, 64);
        assert_eq!(U7::COUNT, 128);
        assert_eq!(U7::MAX.as_usize(), 127);
        assert_eq!(U7::try_new(128), None);
        assert_eq!(U7::try_new(127), Some(U7::MAX));
    }

    #[test]
    fn option_is_one_byte() {
        assert_eq!(core::mem::size_of::<Option<U3>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U4>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U5>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U6>>(), 1);
        assert_eq!(core::mem::size_of::<Option<U7>>(), 1);
    }

    #[test]
    fn default_is_min() {
        assert_eq!(U4::default(), U4::MIN);
        assert_eq!(U7::default().as_u8(), 0);
    }

    #[test]
    fn niche_trait_round_trip() {
        fn round_trip<N: Niche + core::fmt::Debug + PartialEq>(count: usize) {
            assert_eq!(N::COUNT, count);
            assert!(N::try_from_usize(count).is_none());
            let last = N::try_from_usize(count - 1);
            assert!(last.is_some());
            if let Some(v) = last {
                assert_eq!(v.as_usize(), count - 1);
            }
        }
        round_trip::<U3>(8);
        round_trip::<U4>(16);
        round_trip::<U7>(128);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-index`
Expected: FAIL — `U3`/`U4`/etc. not found (only the trait + error exist).

- [ ] **Step 3: Write the macro and generate the types**

Insert this **above** the `#[cfg(test)]` module in `crates/arity-index/src/niche.rs`:

```rust
/// Generates a niche integer newtype `$name` over a fieldless enum `$repr` with
/// `$count == 2^$bits` variants.
macro_rules! niche_int {
    ($name:ident, $repr:ident, $bits:literal, $count:literal) => {
        ::seq_macro::seq!(N in 0..$count {
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
            enum $repr {
                #( V~N, )*
            }
        });

        #[doc = concat!("A ", stringify!($bits), "-bit unsigned integer index (`0..", stringify!($count), "`).")]
        ///
        /// Backed by a fieldless enum, so `Option<Self>` is one byte and indexing
        #[doc = concat!("a ", stringify!($count), "-element array can elide the bounds check.")]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($repr);

        impl $name {
            /// Number of bits in the value's domain.
            pub const BITS: u32 = $bits;
            /// Number of valid values (`2^BITS`).
            pub const COUNT: usize = $count;
            /// The smallest value (`0`).
            pub const MIN: Self = Self($repr::V0);
            /// The largest value (`COUNT - 1`).
            // SAFETY: `COUNT - 1 < COUNT`, a valid discriminant.
            pub const MAX: Self = unsafe { Self::new_unchecked(($count - 1) as u8) };

            ::seq_macro::seq!(N in 0..$count {
                /// Constructs from a `u8`, or `None` if `v >= COUNT`.
                #[must_use]
                pub const fn try_new(v: u8) -> Option<Self> {
                    match v {
                        #( N => Some(Self($repr::V~N)), )*
                        _ => None,
                    }
                }
            });

            /// Constructs without checking that `v < COUNT`.
            ///
            /// # Safety
            ///
            /// The caller must ensure `v < COUNT`.
            #[must_use]
            pub const unsafe fn new_unchecked(v: u8) -> Self {
                debug_assert!((v as usize) < Self::COUNT);
                match Self::try_new(v) {
                    Some(x) => x,
                    // SAFETY: the caller guarantees `v < COUNT`, so `try_new` is `Some`.
                    None => unsafe { ::core::hint::unreachable_unchecked() },
                }
            }

            /// Constructs from the low `BITS` bits of `v` (ignores the rest).
            #[must_use]
            pub const fn new_masked(v: u8) -> Self {
                // SAFETY: masking by `COUNT - 1` (a power of two minus one) yields a
                // value `< COUNT`.
                unsafe { Self::new_unchecked(v & (($count - 1) as u8)) }
            }

            /// Returns the value as a `u8`.
            #[must_use]
            pub const fn as_u8(self) -> u8 {
                self.0 as u8
            }

            /// Returns the value as a `usize`.
            #[must_use]
            pub const fn as_usize(self) -> usize {
                self.0 as usize
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::MIN
            }
        }

        // `Debug` forwards to the integer value (prints `15`, not `U4(V15)`). It
        // lives here, not in Task 4, because this task's `assert_eq!` tests
        // require `Debug`. The `$repr` enum is deliberately NOT `Debug` (nothing
        // prints it), so the struct cannot derive `Debug`.
        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Debug::fmt(&self.as_u8(), f)
            }
        }

        impl Sealed for $name {}

        impl Niche for $name {
            const COUNT: usize = $count;

            fn as_usize(self) -> usize {
                self.0 as usize
            }

            fn try_from_usize(i: usize) -> Option<Self> {
                match u8::try_from(i) {
                    Ok(v) => Self::try_new(v),
                    Err(_) => None,
                }
            }
        }
    };
}

niche_int!(U3, Repr3, 3, 8);
niche_int!(U4, Repr4, 4, 16);
niche_int!(U5, Repr5, 5, 32);
niche_int!(U6, Repr6, 6, 64);
niche_int!(U7, Repr7, 7, 128);
```

Then extend the crate-root re-export in `crates/arity-index/src/lib.rs` so the
types are public:

```rust
pub use niche::{Niche, TryFromIntError, U3, U4, U5, U6, U7};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-index`
Expected: PASS (all five tests).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p arity-index --all-targets`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): generate U3..U7 niche integers via seq-macro"
```

---

### Task 3: `u8` as the arity-256 index

Implement `Sealed` and `Niche` for the native `u8`. No custom type is needed: `u8`'s max (255) is `< 256`, so it already indexes a 256-element array without a bounds check.

**Files:**
- Modify: `crates/arity-index/src/niche.rs`

**Interfaces:**
- Produces: `impl Niche for u8` with `COUNT = 256`, `as_usize`, `try_from_usize`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `niche.rs`:

```rust
    #[test]
    fn u8_is_arity_256_index() {
        assert_eq!(<u8 as Niche>::COUNT, 256);
        assert_eq!(Niche::as_usize(255u8), 255);
        assert_eq!(<u8 as Niche>::try_from_usize(0), Some(0u8));
        assert_eq!(<u8 as Niche>::try_from_usize(255), Some(255u8));
        assert_eq!(<u8 as Niche>::try_from_usize(256), None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-index u8_is_arity_256_index`
Expected: FAIL — `Niche` not implemented for `u8`.

- [ ] **Step 3: Implement `Niche for u8`**

Add below the `niche_int!` invocations in `niche.rs`:

```rust
impl Sealed for u8 {}

impl Niche for u8 {
    const COUNT: usize = 256;

    fn as_usize(self) -> usize {
        usize::from(self)
    }

    fn try_from_usize(i: usize) -> Option<Self> {
        // `u8::try_from` succeeds iff `i <= 255`, i.e. `i < COUNT`. No cast.
        u8::try_from(i).ok()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-index`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): implement Niche for u8 (arity-256 index)"
```

---

### Task 4: Formatting impls and `TryFrom<u8>` for `U3`–`U7`

Add `Debug`, `Display`, `LowerHex`, `UpperHex`, `Binary`, and `TryFrom<u8>` to each niche type (all forwarding to `as_u8`).

**Files:**
- Modify: `crates/arity-index/src/niche.rs`

**Interfaces:**
- Produces: `impl core::fmt::{Display, LowerHex, UpperHex, Binary}` and `impl TryFrom<u8, Error = TryFromIntError>` for each `U{n}` (`Debug` is already provided by Task 2).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
    #[test]
    fn formatting_and_tryfrom() {
        extern crate alloc;
        use alloc::format;

        assert_eq!(format!("{:?}", U4::MAX), "15");
        assert_eq!(format!("{}", U4::MAX), "15");
        assert_eq!(format!("{:x}", U4::new_masked(10)), "a");
        assert_eq!(format!("{:X}", U4::new_masked(10)), "A");
        assert_eq!(format!("{:b}", U4::new_masked(5)), "101");

        assert_eq!(U4::try_from(7u8), Ok(U4::new_masked(7)));
        assert_eq!(U4::try_from(16u8), Err(TryFromIntError));
    }
```

> Note: this test uses `alloc` for `format!`. `arity-index` is `no_std` with no
> `alloc` dependency in its own code, but test builds link `std`, so `extern
> crate alloc; use alloc::format;` inside the test function is fine and keeps the
> library itself `alloc`-free.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-index formatting_and_tryfrom`
Expected: FAIL — `Display`/`LowerHex`/etc. and `TryFrom<u8>` not implemented.

- [ ] **Step 3: Extend the macro with fmt + `TryFrom`**

Inside `macro_rules! niche_int!`, after the `impl Niche for $name` block, add
(`Debug` is already implemented in Task 2 — do **not** re-add it here):

```rust
        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::LowerHex for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::LowerHex::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::UpperHex for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::UpperHex::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::fmt::Binary for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Binary::fmt(&self.as_u8(), f)
            }
        }

        impl ::core::convert::TryFrom<u8> for $name {
            type Error = crate::TryFromIntError;

            fn try_from(v: u8) -> ::core::result::Result<Self, Self::Error> {
                Self::try_new(v).ok_or(crate::TryFromIntError)
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-index`
Expected: PASS.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p arity-index --all-targets`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): add fmt impls and TryFrom<u8> to niche integers"
```

---

### Task 5: `NicheRange` and `NicheRangeInclusive` — double-ended range iterators

Replace the `range.rs` stub with the real iterators. They store bounds as `usize` (so the inclusive form avoids `MAX + 1` overflow and can be empty) and reconstruct each value at yield time via `Niche::try_from_usize(..).unwrap_unchecked()` — the single documented `unsafe` reconstruction site.

**Files:**
- Modify: `crates/arity-index/src/range.rs`

**Interfaces:**
- Consumes: `Niche` (`COUNT`, `as_usize`, `try_from_usize`).
- Produces:
  - `pub struct NicheRange<N: Niche>`; `pub fn new(start: N, end: N) -> Self` (half-open `[start, end)`); `impl Iterator<Item = N> + DoubleEndedIterator + ExactSizeIterator + FusedIterator`.
  - `pub struct NicheRangeInclusive<N: Niche>`; `pub fn new(start: N, end: N) -> Self` (closed `[start, end]`); `pub(crate) fn full() -> Self` (`[0, COUNT-1]`); same iterator impls.

- [ ] **Step 1: Write the failing test**

Replace the contents of `crates/arity-index/src/range.rs` **test section** by adding this module at the end (after the implementation in Step 3); for now, write it so the test exists. Put this block at the bottom of `range.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{U3, U4};

    extern crate alloc;
    use alloc::vec::Vec;

    fn u4(v: u8) -> U4 {
        U4::new_masked(v)
    }

    #[test]
    fn half_open_forward_and_len() {
        let r = NicheRange::new(u4(2), u4(6));
        assert_eq!(r.len(), 4);
        let got: Vec<u8> = r.map(|x| x.as_u8()).collect();
        assert_eq!(got, alloc::vec![2, 3, 4, 5]);
    }

    #[test]
    fn half_open_empty() {
        let r = NicheRange::new(u4(6), u4(2));
        assert_eq!(r.len(), 0);
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn half_open_double_ended_meets_in_middle() {
        let mut r = NicheRange::new(u4(0), u4(4));
        assert_eq!(r.next().map(U4::as_u8), Some(0));
        assert_eq!(r.next_back().map(U4::as_u8), Some(3));
        assert_eq!(r.next().map(U4::as_u8), Some(1));
        assert_eq!(r.next_back().map(U4::as_u8), Some(2));
        assert_eq!(r.next(), None);
        assert_eq!(r.next_back(), None);
    }

    #[test]
    fn inclusive_forward_and_len() {
        let r = NicheRangeInclusive::new(u4(2), u4(5));
        assert_eq!(r.len(), 4);
        let got: Vec<u8> = r.map(|x| x.as_u8()).collect();
        assert_eq!(got, alloc::vec![2, 3, 4, 5]);
    }

    #[test]
    fn inclusive_single_element() {
        let mut r = NicheRangeInclusive::new(u4(7), u4(7));
        assert_eq!(r.len(), 1);
        assert_eq!(r.next().map(U4::as_u8), Some(7));
        assert_eq!(r.next(), None);
    }

    #[test]
    fn inclusive_double_ended_meets_in_middle() {
        let mut r = NicheRangeInclusive::new(u4(0), u4(3));
        assert_eq!(r.len(), 4);
        assert_eq!(r.next().map(U4::as_u8), Some(0));
        assert_eq!(r.next_back().map(U4::as_u8), Some(3));
        assert_eq!(r.next().map(U4::as_u8), Some(1));
        assert_eq!(r.next_back().map(U4::as_u8), Some(2));
        assert_eq!(r.next(), None);
        assert_eq!(r.next_back(), None);
    }

    #[test]
    fn inclusive_full_covers_domain() {
        let r = NicheRangeInclusive::<U3>::full();
        assert_eq!(r.len(), 8);
        let got: Vec<u8> = r.map(|x| x.as_u8()).collect();
        assert_eq!(got, alloc::vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-index`
Expected: FAIL — `NicheRange::new` / iterator impls do not exist (the stub has no methods).

- [ ] **Step 3: Replace the stub with the implementation**

Replace the **non-test** contents of `crates/arity-index/src/range.rs` (everything above the `#[cfg(test)]` module) with:

```rust
//! Double-ended range iterators over [`Niche`](crate::Niche) values.
//!
//! `std`'s `Range`/`RangeInclusive` cannot iterate a niche integer (that needs
//! the unstable `Step` trait), so these custom iterators provide it. They store
//! bounds as `usize` and reconstruct each value at yield time; the cursor is
//! always `< COUNT` by construction, so the reconstruction is sound.

use crate::Niche;
use core::iter::FusedIterator;
use core::marker::PhantomData;

/// A half-open range `[start, end)` over the values of a [`Niche`] type.
#[derive(Clone, Debug)]
pub struct NicheRange<N: Niche> {
    lo: usize,
    hi: usize,
    _marker: PhantomData<N>,
}

impl<N: Niche> NicheRange<N> {
    /// Creates the half-open range `[start, end)`. Empty if `start >= end`.
    #[must_use]
    pub fn new(start: N, end: N) -> Self {
        Self {
            lo: start.as_usize(),
            hi: end.as_usize(),
            _marker: PhantomData,
        }
    }
}

impl<N: Niche> Iterator for NicheRange<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        if self.lo >= self.hi {
            return None;
        }
        // SAFETY: `lo < hi <= COUNT`, so `lo < COUNT` and `try_from_usize` is `Some`.
        let v = unsafe { N::try_from_usize(self.lo).unwrap_unchecked() };
        self.lo += 1;
        Some(v)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.len();
        (n, Some(n))
    }
}

impl<N: Niche> DoubleEndedIterator for NicheRange<N> {
    fn next_back(&mut self) -> Option<N> {
        if self.lo >= self.hi {
            return None;
        }
        self.hi -= 1;
        // SAFETY: `hi < COUNT` (it was `<= COUNT` and just decremented).
        Some(unsafe { N::try_from_usize(self.hi).unwrap_unchecked() })
    }
}

impl<N: Niche> ExactSizeIterator for NicheRange<N> {
    fn len(&self) -> usize {
        self.hi.saturating_sub(self.lo)
    }
}

impl<N: Niche> FusedIterator for NicheRange<N> {}

/// A closed range `[start, end]` over the values of a [`Niche`] type.
#[derive(Clone, Debug)]
pub struct NicheRangeInclusive<N: Niche> {
    lo: usize,
    hi: usize,
    done: bool,
    _marker: PhantomData<N>,
}

impl<N: Niche> NicheRangeInclusive<N> {
    /// Creates the closed range `[start, end]`. Empty if `start > end`.
    #[must_use]
    pub fn new(start: N, end: N) -> Self {
        let (lo, hi) = (start.as_usize(), end.as_usize());
        Self {
            lo,
            hi,
            done: lo > hi,
            _marker: PhantomData,
        }
    }

    /// The whole domain `[0, COUNT - 1]`. Backs [`Niche::all`](crate::Niche::all).
    pub(crate) fn full() -> Self {
        Self {
            lo: 0,
            hi: N::COUNT - 1,
            done: false,
            _marker: PhantomData,
        }
    }
}

impl<N: Niche> Iterator for NicheRangeInclusive<N> {
    type Item = N;

    fn next(&mut self) -> Option<N> {
        if self.done {
            return None;
        }
        // SAFETY: `lo <= hi <= COUNT - 1 < COUNT`.
        let v = unsafe { N::try_from_usize(self.lo).unwrap_unchecked() };
        if self.lo == self.hi {
            self.done = true;
        } else {
            self.lo += 1;
        }
        Some(v)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.len();
        (n, Some(n))
    }
}

impl<N: Niche> DoubleEndedIterator for NicheRangeInclusive<N> {
    fn next_back(&mut self) -> Option<N> {
        if self.done {
            return None;
        }
        // SAFETY: `lo <= hi <= COUNT - 1 < COUNT`.
        let v = unsafe { N::try_from_usize(self.hi).unwrap_unchecked() };
        if self.lo == self.hi {
            self.done = true;
        } else {
            self.hi -= 1;
        }
        Some(v)
    }
}

impl<N: Niche> ExactSizeIterator for NicheRangeInclusive<N> {
    fn len(&self) -> usize {
        if self.done {
            0
        } else {
            self.hi - self.lo + 1
        }
    }
}

impl<N: Niche> FusedIterator for NicheRangeInclusive<N> {}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-index`
Expected: PASS (all range tests).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p arity-index --all-targets`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-index/src/range.rs
git commit -m "feat(arity-index): add double-ended NicheRange and NicheRangeInclusive"
```

---

### Task 6: `Niche::all()` provided method + cross-type domain iteration

Add the `all()` provided method to the `Niche` trait (returning `NicheRangeInclusive<Self>` over the whole domain), and a generic test that exercises it for every index type including `u8`.

**Files:**
- Modify: `crates/arity-index/src/niche.rs`

**Interfaces:**
- Consumes: `NicheRangeInclusive::full()`.
- Produces: `Niche::all() -> NicheRangeInclusive<Self>` (provided method, `len() == COUNT`).

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `niche.rs`:

```rust
    #[test]
    fn all_covers_domain_double_ended() {
        fn check<N: Niche>(count: usize) {
            // Forward length and order.
            let fwd = N::all();
            assert_eq!(fwd.len(), count);
            let collected: usize = N::all().count();
            assert_eq!(collected, count);

            // First and last via both ends.
            let mut it = N::all();
            assert_eq!(it.next().map(Niche::as_usize), Some(0));
            assert_eq!(it.next_back().map(Niche::as_usize), Some(count - 1));

            // Ascending and exact.
            let mut prev: Option<usize> = None;
            let mut seen = 0usize;
            for v in N::all() {
                let cur = v.as_usize();
                if let Some(p) = prev {
                    assert!(cur == p + 1, "not ascending by 1");
                }
                prev = Some(cur);
                seen += 1;
            }
            assert_eq!(seen, count);
        }
        check::<U3>(8);
        check::<U4>(16);
        check::<U5>(32);
        check::<U6>(64);
        check::<U7>(128);
        check::<u8>(256);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p arity-index all_covers_domain_double_ended`
Expected: FAIL — `Niche::all` does not exist.

- [ ] **Step 3: Add `all()` to the `Niche` trait**

In `niche.rs`, add the import and the provided method. Change the top-of-file import and the trait body:

```rust
use crate::range::NicheRangeInclusive;
use crate::sealed::Sealed;
```

Add to the `pub trait Niche` body (after `try_from_usize`):

```rust
    /// Iterates over all values ascending (`MIN..=MAX`) as a double-ended,
    /// exact-size iterator. `len() == COUNT`.
    #[must_use]
    fn all() -> NicheRangeInclusive<Self> {
        NicheRangeInclusive::full()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p arity-index`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-index/src/niche.rs
git commit -m "feat(arity-index): add Niche::all() double-ended domain iterator"
```

---

### Task 7: Crate doctest, doc build, and Miri gate

Add a crate-level usage doctest, ensure docs build clean, and run the unsafe surface under Miri.

**Files:**
- Modify: `crates/arity-index/src/lib.rs`

**Interfaces:**
- No new API. Documentation + verification only.

- [ ] **Step 1: Add a doctest to `lib.rs`**

Append to the crate-level doc comment block at the top of `lib.rs` (inside `//!`):

```rust
//!
//! ```
//! use arity_index::{Niche, U4, NicheRange};
//!
//! // The whole domain, ascending:
//! let all: alloc::vec::Vec<u8> = U4::all().map(U4::as_u8).collect();
//! assert_eq!(all.len(), 16);
//!
//! // A sub-range, double-ended:
//! let mut r = NicheRange::new(U4::new_masked(1), U4::new_masked(4));
//! assert_eq!(r.next().map(U4::as_u8), Some(1));
//! assert_eq!(r.next_back().map(U4::as_u8), Some(3));
//! # extern crate alloc;
//! ```
```

> The `# extern crate alloc;` hidden line plus `alloc::vec::Vec` keeps the
> doctest `std`-linked (doctests always link `std`) without pulling `alloc` into
> the library.

- [ ] **Step 2: Run the doctest**

Run: `cargo test -p arity-index --doc`
Expected: PASS.

- [ ] **Step 3: Build docs with warnings denied**

Run: `RUSTDOCFLAGS="-D warnings" cargo doc -p arity-index --no-deps`
Expected: builds with no warnings (no broken intra-doc links, no missing-doc issues).

- [ ] **Step 4: Run the full test suite under Miri**

Run: `cargo +nightly miri test -p arity-index`
Expected: PASS — no undefined behavior in `new_unchecked`, `unreachable_unchecked`, or the iterators' `unwrap_unchecked` reconstruction.

> If Miri is not installed: `rustup +nightly component add miri`. If `cargo
> +nightly miri test` reports it must run a setup first, run `cargo +nightly miri
> setup` once.

- [ ] **Step 5: Final clippy + fmt gate**

Run: `cargo clippy -p arity-index --all-targets --all-features` then `cargo +nightly fmt --all --check`
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add crates/arity-index/src/lib.rs
git commit -m "docs(arity-index): add crate doctest and verify under Miri"
```

---

## Self-Review

**Spec coverage** (against the `arity-index` portions of the spec):

- Niche enum trick / `seq-macro` codegen → Task 2 ✓
- Generated surface (`BITS`, `COUNT`, `MIN`, `MAX`, `try_new`, `new_unchecked`, `new_masked`, `as_u8`, `as_usize`, `Default`, fmt, `TryFrom`) → Tasks 2 + 4 ✓
- `Niche` trait (`COUNT`, `as_usize`, `try_from_usize`, `all()`), sealed → Tasks 1, 2, 6 ✓
- `u8` arity-256 index → Task 3 ✓
- `NicheRange` / `NicheRangeInclusive` (double-ended, exact-size, fused; `usize` bounds; the single `unwrap_unchecked` reconstruction site; inclusive `len = hi - lo + 1`) → Task 5 ✓
- Niche size assertions (`Option<U{n}>` is 1 byte), double-ended iteration tests, ascending order → Tasks 2, 5, 6 ✓
- `no_std`, no `alloc` in library code → enforced throughout (tests/doctests use `extern crate alloc` locally) ✓
- `unsafe` documented + Miri → every `unsafe` block has `// SAFETY:`; Task 7 runs Miri ✓

Not in this plan (correctly — later crates/phases): `Bitmap`, `U256`, `BitIter` (plan 2 `arity-bitmap`); `Arity`, `FixedArray`, `PackedArray` (plan 3 `arity-arrays`); CI workflow, package metadata, MSRV bump to 1.92, publish flag (closing phase of plan 3).

**Placeholder scan:** the Task 1 `range.rs` stub is intentionally temporary and is fully replaced in Task 5 (not a residual placeholder). No `TODO`/`TBD`/"add error handling" left.

**Type consistency:** `try_from_usize`, `as_usize`, `as_u8`, `new_masked`, `new_unchecked`, `COUNT`, `all`, `NicheRange::new`, `NicheRangeInclusive::new` / `full` are used identically across tasks. `Niche::all()` (Task 6) returns `NicheRangeInclusive<Self>` defined in Task 5. ✓
