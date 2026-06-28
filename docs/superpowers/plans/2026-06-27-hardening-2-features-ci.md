# Hardening Plan 2 — Per-Arity Features & CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-arity cargo features (`8`/`16`/`32`/`64`/`128`/`256`, all-on by default, opt-out) across the three crates so consumers compile only the arities they use, then wire a CI feature matrix, harden the Miri job, and add a `mise` tool manifest.

**Architecture:** Each crate gains six arity features in its `default` set. A feature `#[cfg]`-gates its index type + `Niche` impl (`arity-index`), its `Bitmap` backing (`arity-bitmap`), and its `Arity` marker (`arity-arrays`); `arity-arrays`/`arity-bitmap` features forward to the leaf crates. Inter-crate deps are pulled `default-features = false` so a lean selection stays lean. Macro definitions and their supporting imports are gated alongside their invocations so a zero-arity build is warning-clean.

**Tech Stack:** Rust (edition 2024, `#![no_std]`), cargo features, GitHub Actions, `mise`, `just`. No new crate dependencies.

This is **plan 2 of 5** for the production-hardening effort
(`breaking-api` ✓ → **`features-ci`** → `mutation` → `serde-ethnum` → `publish`).
Design spec: `docs/superpowers/specs/2026-06-27-arity-arrays-hardening-design.md`
(sections "Per-arity features" and "Continuous integration").

> [!NOTE]
> The cross-cutting **`std` feature is deferred to plan 4** (`serde-ethnum`). It
> forwards to optional std-capable deps (`serde`, `ethnum`) via weak-dep syntax;
> none of those deps exist yet, and the one current dep with feature flags
> (`hybrid-array`) has **no `std` feature**, so a `std` feature here would forward
> to nothing. The serde/ethnum CI matrix columns and the `--all-features`-vs-default
> split likewise land in plan 4. This plan adds only the arity columns + lean/empty
> builds.

## Global Constraints

Copied from the spec and existing conventions; every task implicitly includes these.

- **`#![no_std]`** in all three crates (`arity-arrays` also uses `alloc`). `arity-bitmap`/`arity-index` add no `unsafe`; this plan adds none anywhere.
- **Lints strict (CI denies warnings):** `clippy::pedantic` + `clippy::nursery` at warn, `clippy::unwrap_used` at warn, `cargo_common_metadata`/`negative_feature_names`/`redundant_feature_names` at warn, `undocumented_unsafe_blocks` deny. **No `#[allow]`**; `#[expect(reason="…")]` only if unavoidable. The arity feature names are bare numbers (`"8"`…`"256"`) — these do not trip `negative_feature_names`/`redundant_feature_names`.
- **Feature names and default set, verbatim:** every crate has `default = ["8", "16", "32", "64", "128", "256"]` and one feature per arity. `arity-bitmap` `"N" = ["arity-index/N"]`; `arity-arrays` `"N" = ["arity-index/N", "arity-bitmap/N"]`; `arity-index` `"N" = []`.
- **Inter-crate deps must be `default-features = false`** so disabling a consumer's defaults actually drops the transitive arities. Without this, lean builds silently pull every arity.
- **Zero-arity build must be warning-clean**, not just compile: gate each `macro_rules!` definition and its supporting imports under `any(<the arities that use it>)`, and gate type-specific imports/invocations under the specific arity.
- Edition 2024, MSRV 1.92. Conventional-commit messages, imperative mood. Edit `Cargo.toml` by hand here (these are feature tables, not dependency adds).

### Verification model for this plan

Feature-gating is config, not logic, so the "test" for each gating task is a set of compile/lint checks under feature subsets, plus the existing unit tests as a regression guard:

- `cargo test -p <crate>` (default features = all arities) — existing tests stay green.
- `cargo clippy -p <crate> --all-targets -- -D warnings` (default) — clean with tests.
- `cargo clippy -p <crate> --no-default-features --features 16 -- -D warnings` — lean single-arity lib clean.
- `cargo clippy -p <crate> --no-default-features -- -D warnings` — zero-arity lib clean (this is the check that validates the macro/import gating).

(`--no-default-features` clippy runs without `--all-targets`, so the cross-arity test modules are not compiled — they reference types absent under a subset and are only built/run under the all-arity default.)

---

### Task 1: `arity-index` per-arity features

Gate the five generated niche types, the `u8` arity-256 `Niche` impl, and the re-exports; add the feature table.

**Files:**
- Modify: `crates/arity-index/src/niche.rs` (gate `macro_rules! niche_int`, its 5 invocations, and the 2 `u8` impls)
- Modify: `crates/arity-index/src/lib.rs:30-34` (gate the `U3`–`U7` re-exports)
- Modify: `crates/arity-index/Cargo.toml` (add `[features]`)

**Interfaces:**
- Consumes: nothing (leaf crate).
- Produces: features `"8"`…`"256"` on `arity-index`. `"8"`→`U3`, `"16"`→`U4`, `"32"`→`U5`, `"64"`→`U6`, `"128"`→`U7`, `"256"`→`impl Niche for u8`. `arity-bitmap`/`arity-arrays` forward to these.

- [ ] **Step 1: Add the feature table to `crates/arity-index/Cargo.toml`**

Append at end of file:

```toml
[features]
default = ["8", "16", "32", "64", "128", "256"]
"8" = []
"16" = []
"32" = []
"64" = []
"128" = []
"256" = []
```

- [ ] **Step 2: Gate the `niche_int!` macro definition and invocations in `crates/arity-index/src/niche.rs`**

Add `#[cfg(...)]` immediately before `macro_rules! niche_int {` (currently line 50):

```rust
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
macro_rules! niche_int {
```

Replace the five invocation lines (currently 204-208) with gated versions:

```rust
#[cfg(feature = "8")]
niche_int!(U3, Repr3, 3, 8);
#[cfg(feature = "16")]
niche_int!(U4, Repr4, 4, 16);
#[cfg(feature = "32")]
niche_int!(U5, Repr5, 5, 32);
#[cfg(feature = "64")]
niche_int!(U6, Repr6, 6, 64);
#[cfg(feature = "128")]
niche_int!(U7, Repr7, 7, 128);
```

- [ ] **Step 3: Gate the `u8` arity-256 impls in `crates/arity-index/src/niche.rs`**

The two `u8` impls (currently lines 210-223) become:

```rust
#[cfg(feature = "256")]
impl Sealed for u8 {}

#[cfg(feature = "256")]
impl Niche for u8 {
    const COUNT: usize = 256;

    fn as_usize(self) -> usize {
        usize::from(self)
    }

    fn try_from_usize(i: usize) -> Option<Self> {
        // `Self::try_from` succeeds iff `i <= 255`, i.e. `i < COUNT`. No cast.
        Self::try_from(i).ok()
    }
}
```

- [ ] **Step 4: Gate the re-exports in `crates/arity-index/src/lib.rs`**

Replace lines 30-34 (`pub use niche::U3;` … `pub use niche::U7;`) with:

```rust
#[cfg(feature = "8")]
pub use niche::U3;
#[cfg(feature = "16")]
pub use niche::U4;
#[cfg(feature = "32")]
pub use niche::U5;
#[cfg(feature = "64")]
pub use niche::U6;
#[cfg(feature = "128")]
pub use niche::U7;
```

(Leave `pub use niche::Niche;`, `pub use niche::TryFromIntError;`, and the two `range::` re-exports ungated.)

- [ ] **Step 5: Verify default tests and lint stay green**

Run: `cargo test -p arity-index && cargo clippy -p arity-index --all-targets -- -D warnings`
Expected: all tests pass (default = all arities, unchanged behavior); clippy clean.

- [ ] **Step 6: Verify lean and zero-arity builds are warning-clean**

Run:
```bash
cargo clippy -p arity-index --no-default-features --features 16 -- -D warnings
cargo clippy -p arity-index --no-default-features -- -D warnings
```
Expected: both succeed with no warnings. (The first compiles only `U4`; the second compiles no niche types — the gated macro/invocations produce no `unused_macros`.)

- [ ] **Step 7: Commit**

```bash
git add crates/arity-index/Cargo.toml crates/arity-index/src/niche.rs crates/arity-index/src/lib.rs
git commit -m "feat(arity-index): gate niche types behind per-arity features"
```

---

### Task 2: `arity-bitmap` per-arity features

Gate the native backings and `U256`; forward features to `arity-index`; pull `arity-index` with `default-features = false`.

**Files:**
- Modify: `crates/arity-bitmap/src/native.rs` (gate imports, `macro_rules! impl_native_bitmap`, and its 5 invocations)
- Modify: `crates/arity-bitmap/src/lib.rs:31,35` (gate `mod u256;` and `pub use u256::U256;`)
- Modify: `crates/arity-bitmap/Cargo.toml` (add `[features]`; set `arity-index` `default-features = false`)

**Interfaces:**
- Consumes: `arity-index` features `"8"`…`"256"` (Task 1).
- Produces: features `"8"`…`"256"` on `arity-bitmap`, each forwarding to `arity-index/N`. `"8"`→`impl Bitmap for u8`, …, `"128"`→`u128`, `"256"`→`U256`.

- [ ] **Step 1: Update `crates/arity-bitmap/Cargo.toml`**

Set the dependency to drop transitive default arities, and add the feature table. Replace the `[dependencies]` section and append `[features]`:

```toml
[dependencies]
arity-index = { workspace = true, default-features = false }

[features]
default = ["8", "16", "32", "64", "128", "256"]
"8" = ["arity-index/8"]
"16" = ["arity-index/16"]
"32" = ["arity-index/32"]
"64" = ["arity-index/64"]
"128" = ["arity-index/128"]
"256" = ["arity-index/256"]
```

- [ ] **Step 2: Gate imports and the macro in `crates/arity-bitmap/src/native.rs`**

Replace the import block (currently lines 3-12) with gated imports. `Niche`, `Bitmap`, `Raw`, `Sealed` are used only by the macro, so they gate under the same `any(...)` as the macro; each `U{n}` gates under its arity:

```rust
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use arity_index::Niche;
#[cfg(feature = "8")]
use arity_index::U3;
#[cfg(feature = "16")]
use arity_index::U4;
#[cfg(feature = "32")]
use arity_index::U5;
#[cfg(feature = "64")]
use arity_index::U6;
#[cfg(feature = "128")]
use arity_index::U7;

#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Bitmap;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Raw;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
use crate::Sealed;
```

Add the same `any(...)` gate immediately before `macro_rules! impl_native_bitmap {` (currently line 17):

```rust
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128"
))]
macro_rules! impl_native_bitmap {
```

- [ ] **Step 3: Gate the native invocations in `crates/arity-bitmap/src/native.rs`**

Replace the five invocations (currently lines 83-87) with:

```rust
#[cfg(feature = "8")]
impl_native_bitmap!(u8, U3, 8);
#[cfg(feature = "16")]
impl_native_bitmap!(u16, U4, 16);
#[cfg(feature = "32")]
impl_native_bitmap!(u32, U5, 32);
#[cfg(feature = "64")]
impl_native_bitmap!(u64, U6, 64);
#[cfg(feature = "128")]
impl_native_bitmap!(u128, U7, 128);
```

- [ ] **Step 4: Gate `U256` in `crates/arity-bitmap/src/lib.rs`**

Change `mod u256;` (line 31) to:

```rust
#[cfg(feature = "256")]
mod u256;
```

Change `pub use u256::U256;` (line 35) to:

```rust
#[cfg(feature = "256")]
pub use u256::U256;
```

(Leave `mod native;`, `mod iter;`, `pub use iter::BitIter;`, and `use arity_index::Niche;` ungated — `native`/`iter` contain only generic code plus the gated macro, and the crate-root `Niche` import is used by the `Bitmap` trait definition.)

- [ ] **Step 5: Verify default tests and lint stay green**

Run: `cargo test -p arity-bitmap && cargo clippy -p arity-bitmap --all-targets -- -D warnings`
Expected: all tests pass; clippy clean.

- [ ] **Step 6: Verify lean and zero-arity builds are warning-clean**

Run:
```bash
cargo clippy -p arity-bitmap --no-default-features --features 16 -- -D warnings
cargo clippy -p arity-bitmap --no-default-features --features 256 -- -D warnings
cargo clippy -p arity-bitmap --no-default-features -- -D warnings
```
Expected: all three succeed with no warnings. (`--features 16` compiles only the `u16` backing; `--features 256` compiles only `U256`; the empty build compiles neither, with no `unused_macros`/`unused_imports`.)

- [ ] **Step 7: Commit**

```bash
git add crates/arity-bitmap/Cargo.toml crates/arity-bitmap/src/native.rs crates/arity-bitmap/src/lib.rs
git commit -m "feat(arity-bitmap): gate bitmap backings behind per-arity features"
```

---

### Task 3: `arity-arrays` per-arity features

Gate the six `Arity` markers and their re-exports; forward features to both leaf crates; pull both with `default-features = false`.

**Files:**
- Modify: `crates/arity-arrays/src/arity.rs` (gate the typenum-size + `Unsigned` imports, `macro_rules! arity`, and its 6 invocations)
- Modify: `crates/arity-arrays/src/lib.rs:35-40` (gate the `Arity8`–`Arity256` re-exports)
- Modify: `crates/arity-arrays/Cargo.toml` (add `[features]`; set `arity-index`/`arity-bitmap` `default-features = false`)

**Interfaces:**
- Consumes: `arity-index` and `arity-bitmap` features `"8"`…`"256"` (Tasks 1–2).
- Produces: features `"8"`…`"256"` on `arity-arrays`, each forwarding to `arity-index/N` + `arity-bitmap/N`. `"8"`→`Arity8`, …, `"256"`→`Arity256`.

- [ ] **Step 1: Update `crates/arity-arrays/Cargo.toml`**

Set the inter-crate deps to drop transitive default arities, and add the feature table. Replace the `[dependencies]` section and append `[features]` (leave `hybrid-array` as-is — it has no `std` feature and its defaults are needed):

```toml
[dependencies]
arity-bitmap = { workspace = true, default-features = false }
arity-index = { workspace = true, default-features = false }
hybrid-array = "0.4.12"

[features]
default = ["8", "16", "32", "64", "128", "256"]
"8" = ["arity-index/8", "arity-bitmap/8"]
"16" = ["arity-index/16", "arity-bitmap/16"]
"32" = ["arity-index/32", "arity-bitmap/32"]
"64" = ["arity-index/64", "arity-bitmap/64"]
"128" = ["arity-index/128", "arity-bitmap/128"]
"256" = ["arity-index/256", "arity-bitmap/256"]
```

- [ ] **Step 2: Gate the size imports in `crates/arity-arrays/src/arity.rs`**

The typenum size aliases are each used only by one arity's invocation, and `Unsigned` is used only inside the macro/tests. Replace the import block (currently lines 4-12) so the always-needed trait imports stay ungated and the size/`Unsigned` imports gate:

```rust
use arity_bitmap::Bitmap;
use arity_index::Niche;
use hybrid_array::ArraySize;
#[cfg(feature = "8")]
use hybrid_array::typenum::U8;
#[cfg(feature = "16")]
use hybrid_array::typenum::U16;
#[cfg(feature = "32")]
use hybrid_array::typenum::U32;
#[cfg(feature = "64")]
use hybrid_array::typenum::U64;
#[cfg(feature = "128")]
use hybrid_array::typenum::U128;
#[cfg(feature = "256")]
use hybrid_array::typenum::U256;
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
use hybrid_array::typenum::Unsigned;
```

(`Bitmap`, `Niche`, `ArraySize` stay ungated — they are named in the `Arity` trait definition itself.)

- [ ] **Step 3: Gate the `arity!` macro definition and invocations in `crates/arity-arrays/src/arity.rs`**

Add the `any(...)` gate immediately before `macro_rules! arity {` (currently line 34):

```rust
#[cfg(any(
    feature = "8",
    feature = "16",
    feature = "32",
    feature = "64",
    feature = "128",
    feature = "256"
))]
macro_rules! arity {
```

Replace the six invocations (currently lines 60-65) with:

```rust
#[cfg(feature = "8")]
arity!(Arity8, 8, arity_index::U3, u8, U8);
#[cfg(feature = "16")]
arity!(Arity16, 16, arity_index::U4, u16, U16);
#[cfg(feature = "32")]
arity!(Arity32, 32, arity_index::U5, u32, U32);
#[cfg(feature = "64")]
arity!(Arity64, 64, arity_index::U6, u64, U64);
#[cfg(feature = "128")]
arity!(Arity128, 128, arity_index::U7, u128, U128);
#[cfg(feature = "256")]
arity!(Arity256, 256, u8, arity_bitmap::U256, U256);
```

- [ ] **Step 4: Gate the re-exports in `crates/arity-arrays/src/lib.rs`**

Replace lines 35-40 (`pub use arity::Arity8;` … `pub use arity::Arity256;`) with:

```rust
#[cfg(feature = "8")]
pub use arity::Arity8;
#[cfg(feature = "16")]
pub use arity::Arity16;
#[cfg(feature = "32")]
pub use arity::Arity32;
#[cfg(feature = "64")]
pub use arity::Arity64;
#[cfg(feature = "128")]
pub use arity::Arity128;
#[cfg(feature = "256")]
pub use arity::Arity256;
```

(Leave `pub use arity::Arity;`, the `bitmap`/`index` re-exports, and `FixedArray`/`PackedArray` ungated.)

- [ ] **Step 5: Verify default tests and lint stay green (whole workspace)**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: all tests pass (default = all arities); clippy clean across the workspace.

- [ ] **Step 6: Verify lean, firewood-shape, and zero-arity builds are warning-clean**

Run:
```bash
cargo clippy -p arity-arrays --no-default-features --features 16 -- -D warnings
cargo clippy -p arity-arrays --no-default-features --features 256 -- -D warnings
cargo clippy -p arity-arrays --no-default-features -- -D warnings
cargo build -p arity-arrays --no-default-features --features 16
```
Expected: all succeed with no warnings. The `--features 16` build is the firewood shape (only `Arity16`/`U4`/`u16`); confirm `Arity8`/`Arity256` etc. are absent yet `FixedArray`/`PackedArray` still compile (they are generic over `Arity`).

- [ ] **Step 7: Commit**

```bash
git add crates/arity-arrays/Cargo.toml crates/arity-arrays/src/arity.rs crates/arity-arrays/src/lib.rs
git commit -m "feat(arity-arrays): gate Arity markers behind per-arity features"
```

---

### Task 4: CI feature matrix and Miri hardening

Add a `features` job exercising lean/zero-arity builds, and change the `miri` job to run over `--tests` with strict provenance and bounded proptest cases.

**Files:**
- Modify: `.github/workflows/ci.yml` (add `features` job; change `miri` job)

**Interfaces:**
- Consumes: the per-arity features from Tasks 1–3.
- Produces: a `features` CI job and a hardened `miri` job. No code interface.

- [ ] **Step 1: Add the `features` job to `.github/workflows/ci.yml`**

Insert this job (after the `lint` job, before `miri`):

```yaml
  features:
    name: feature matrix
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      # Lean single-arity build (the firewood shape): library compiles and lints clean.
      - run: cargo clippy --workspace --no-default-features --features 16 -- -D warnings
      # Arity-256 alone: exercises the U256 backing in isolation.
      - run: cargo clippy --workspace --no-default-features --features 256 -- -D warnings
      # No arities selected: the crates still compile (empty of arity types), warning-free.
      - run: cargo clippy --workspace --no-default-features -- -D warnings
```

(No `--all-targets`: the cross-arity test modules reference types absent under a subset and are built only under the all-arity default in the `test` job.)

- [ ] **Step 2: Harden the `miri` job in `.github/workflows/ci.yml`**

Replace the existing `miri` job with:

```yaml
  miri:
    runs-on: ubuntu-26.04
    env:
      MIRIFLAGS: "-Zmiri-strict-provenance -Zmiri-disable-isolation"
      PROPTEST_CASES: "32"
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - uses: Swatinem/rust-cache@v2
      - run: cargo +nightly miri test --workspace --tests
```

(`--tests` runs the lib unit tests **and** `tests/roundtrip.rs` — the all-arity proptest round-trip — under Miri, matching the `justfile`; `--lib` skipped the integration tests. `MIRIFLAGS` matches the `justfile`; `PROPTEST_CASES: 32` bounds the proptests for the interpreter.)

- [ ] **Step 3: Validate the workflow YAML and run the feature-matrix commands locally**

Run:
```bash
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml ok')"
cargo clippy --workspace --no-default-features --features 16 -- -D warnings
cargo clippy --workspace --no-default-features --features 256 -- -D warnings
cargo clippy --workspace --no-default-features -- -D warnings
```
Expected: `yaml ok`; all three clippy runs clean (these are the exact commands the new `features` job runs).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add per-arity feature matrix job; run Miri over --tests with strict provenance"
```

---

### Task 5: `mise` tool manifest and `just` recipes

Add a `mise.toml` pinning the shared developer tooling, and `just` recipes to install it and run the feature matrix locally.

**Files:**
- Create: `mise.toml`
- Modify: `justfile` (add `setup` and `features` recipes; add `features` to the `ci` aggregate)

**Interfaces:**
- Consumes: the per-arity features (Tasks 1–3) for the `features` recipe.
- Produces: `mise.toml`; `just setup` and `just features` recipes. No code interface.

- [ ] **Step 1: Create `mise.toml`**

```toml
# Pinned developer tooling shared by the `just` recipes and CI.
#
# Install everything locally with `mise install` (or `just setup`).
# CI installs the Rust toolchains via dtolnay/rust-toolchain (the stable/nightly/
# MSRV matrix); this manifest covers the auxiliary cargo tools those toolchains
# drive. CI jobs that need these tools install them via jdx/mise-action (added by
# the plans that introduce the tools — cargo-fuzz in the mutation plan,
# cargo-insta in the serde plan).
[tools]
just = "latest"
"cargo:cargo-nextest" = "latest"
"cargo:cargo-fuzz" = "latest"
"cargo:cargo-insta" = "latest"
```

- [ ] **Step 2: Add the `setup` and `features` recipes to `justfile`**

Insert before the final `ci:` recipe:

```just
# Install the pinned developer tooling (cargo subtools + just) via mise.
setup:
    mise install

# Build/lint the crates under representative feature subsets (mirrors CI `features`).
features:
    cargo clippy --workspace --no-default-features --features 16 -- -D warnings
    cargo clippy --workspace --no-default-features --features 256 -- -D warnings
    cargo clippy --workspace --no-default-features -- -D warnings
```

- [ ] **Step 3: Add `features` to the `ci` aggregate recipe in `justfile`**

Change the final recipe from:

```just
# Run the fast checks (everything except the slow Miri pass).
ci: fmt-check lint test doc
```

to:

```just
# Run the fast checks (everything except the slow Miri pass).
ci: fmt-check lint features test doc
```

- [ ] **Step 4: Verify the justfile parses and the feature recipe runs clean**

Run:
```bash
just --list
just features
```
Expected: `just --list` shows `setup` and `features`; `just features` runs the three clippy checks clean (end-to-end validation of the Tasks 1–3 gating at the workspace level).

- [ ] **Step 5: Commit**

```bash
git add mise.toml justfile
git commit -m "chore: add mise tool manifest and just setup/features recipes"
```

---

## Self-Review

- **Spec coverage (Per-arity features + CI sections):**
  - Feature names `"8"`…`"256"`, all-on `default`, opt-out → Tasks 1–3. ✓
  - Forwarding (`arity-arrays`→leaves, `arity-bitmap`→`arity-index`) → Tasks 2–3 feature tables. ✓
  - Each arity gates its index type + `Niche` impl, `Bitmap` backing, `Arity` marker → Tasks 1–3. ✓
  - `u8` primitive always exists; only its `Niche` impl gates under `"256"` → Task 1 Step 3. ✓
  - Zero-arity build compiles (and, beyond the spec, warning-clean) → macro/import gating in Tasks 1–3; verified in each Step 6. ✓
  - CI matrix columns (lean `16`, plus `256`, plus empty) → Task 4 `features` job. ✓ (The `--all-features` column is the existing `test` job; the serde/ethnum/`std`/default columns are **deferred to plan 4**, noted in the header.)
  - Miri over `--tests` + strict provenance + `PROPTEST_CASES` → Task 4 Step 2. ✓
  - `mise` manifest + `just` recipes + CI tool split → Task 5. ✓ (CI `mise-action` integration deferred to the plans that add `cargo-fuzz`/`cargo-insta`, per the spec's tool split — noted in `mise.toml`.)
  - **`std` feature → deferred to plan 4** (no std-capable deps exist yet; `hybrid-array` has no `std` feature), noted in the header.
- **Critical correctness points captured:** inter-crate deps `default-features = false` (Tasks 2–3 Step 1) — without this, lean builds are not actually lean; macro defs + supporting imports gated (Tasks 1–3) — without this, the empty build warns. Both are verified by the `--no-default-features … -D warnings` checks.
- **Placeholder scan:** none — every step has exact code, file targets, and commands.
- **Type/feature consistency:** the feature names, `default` set, and forwarding strings are identical across all three `Cargo.toml` tables and the `#[cfg(feature = "…")]` attributes; the `any(...)` macro-gate arity lists match each macro's set of invocations (`arity-index`/`arity-bitmap`: 8–128; `arity-arrays`: 8–256).
