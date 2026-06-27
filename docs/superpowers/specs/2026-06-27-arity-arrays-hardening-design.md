# Arity Arrays — Production Hardening

> Status: proposed · 2026-06-27
>
> Hardening pass that takes the three `arity-*` crates from functionally complete
> to a publishable `0.1.0`: a completed `Bitmap`/index API, in-place
> `PackedArray` mutation, per-arity compile-scope features, optional `serde` and
> `ethnum` integration, and closed CI/Miri/fuzzing gaps. Builds on the original
> [Arity Arrays design](2026-06-26-arity-arrays-design.md); read that first for
> the architecture this extends.

## Premise

The three crates are functionally complete and the `unsafe` is sound, but the
workspace is not yet production-ready as a general-purpose dependency:

- the `Bitmap` trait is missing the inverse operations (`without_bit`, `select`)
  that any mutation or dense→logical mapping needs;
- `PackedArray` has no in-place mutation — every write round-trips through a
  full-width `FixedArray`, costing `O(LEN)` stack plus two allocations;
- the implemented CI runs `cargo +nightly miri test --workspace --lib`, so Miri
  covers only the inline `#[cfg(test)]` modules — skipping `tests/roundtrip.rs`
  and any future integration tests where a rank/slot bug would surface — and runs
  without the strict-provenance flags the `justfile` uses;
- there is no serialization, no way to limit which arities are compiled, and
  assorted API-consistency friction (`U256` lacks `Hash`, no `From<U{n}>`
  conversions, and the bitmap/index *operation* methods — `with_bit`, `test`,
  `rank`, `count_ones`, `is_zero` — are not `const fn`, even though the index
  constructors `try_new`/`new_masked`/`as_u8`/`as_usize` already are).

Nothing is published to crates.io yet, so this pass makes **all changes —
including breaking ones — before the first release**, and `0.1.0` ships already
complete.

## Scope

This spec covers the **`arity-arrays` workspace only**, as a general-purpose OSS
crate. The motivating consumer (replacing the vendored `DenseChildren`/`U4` work
in [`ava-labs/firewood#2100`](https://github.com/ava-labs/firewood/pull/2100)
with a dependency on `arity-arrays`) is the primary motivating use case, but no
firewood-side migration or firewood-specific API shaping is in scope here —
firewood is treated as one downstream consumer among others.

### Goals

- Complete the `Bitmap`/index API so the abstractions are usable without escape
  hatches.
- Add in-place `PackedArray` mutation, removing the `FixedArray` round-trip as
  the only write path.
- Per-arity cargo features (all-on by default, opt-out) so consumers compile only
  the arities they use.
- Optional `serde` (logical default) + `serde_with::Compact` (compact) serialization.
- Optional `ethnum::U256` backing for arity-256, swappable without touching the
  `Arity`/`Bitmap` interface.
- Close CI/Miri/fuzzing gaps so the `unsafe` is checked against the cases it does
  not explicitly enumerate.
- Ship a coherent `0.1.0` with a stated semver/MSRV policy.

### Non-goals

- No firewood-side migration or firewood-specific API shaping (separate effort).
- No `1.0` stability commitment in this spec — `0.1.0` with intent-to-stabilize.
- No general-purpose `U256` arithmetic beyond what the bitmap surface requires.
- No runtime or non-power-of-two arities.
- **Deferred — capacity-bearing packed variant.** A third packed type that
  decouples capacity from occupancy (Vec-like spare capacity to avoid
  realloc-per-mutation, a `with_capacity(n)` constructor, and retention of a
  possibly zero-element allocation after the count reaches 0 so empty↔nonempty
  churn does not re-allocate). This needs a capacity word and breaks
  `PackedArray`'s exact-size, strictly-occupancy-proportional invariant, so it is
  a **separate type**, not a mode of `PackedArray`. Noted now so `PackedArray`'s
  docs frame it as "the exact-size variant" and leave room for the sibling.
- **Deferred — nightly-gated optimizations.** Opt-in optimizations enabled by
  probing the build environment (e.g. `build.rs` + `rustversion`/`autocfg` cfg
  detection) so the same source still compiles on stable. Candidates: `impl Step`
  for native range iteration, `const` trait impls for the `Bitmap` ops,
  allocator-API hooks, SIMD popcount/select. The existing CI nightly leg keeps the
  crates building on nightly, keeping this viable later.

## Work ordering (risk-phased)

The implementation plan inherits this order; the high-risk new `unsafe` lands
behind its own verification gate before publish.

1. **Breaking API surface** — finalize the trait/type surface while unpublished:
   `Bitmap::without_bit` + `select` + `to_le_bytes`/`from_le_bytes`; `From<U{n}>`
   conversions; `const fn` on the inherent operation methods; `U256: Hash`
   (custom backing); `/// # Safety` docs on the unsafe-bearing types.
2. **Per-arity features** — across all three crates, plus the CI feature matrix.
3. **CI / Miri hardening** — Miri over `--tests` with strict provenance.
4. **In-place mutation** — `insert`/`remove`/`get_mut`, gated on `cargo-fuzz` +
   Miri verification before merge.
5. **Serde** — logical default plus the `serde_with::Compact` adapter; `ethnum`
   backing swap.
6. **Publish prep** — docs pass, `CHANGELOG`, `cargo publish --dry-run` in
   dependency order, cut `0.1.0`.

> [!NOTE]
> The `ethnum` backing is introduced alongside the serde work in step 5 because
> the `Compact` wire format must be backing-independent (see [Serde](#serde)).
> The generic `Bitmap::to_le_bytes`/`from_le_bytes` methods (step 1) are what make
> that possible without naming a concrete `U256`; the `U256: Hash` addition in
> step 1 is scoped to the **custom backing only** — under `ethnum` it comes from
> `ethnum::U256` itself.

## Completed API surface

All breaking changes land before the first publish (step 1).

### `Bitmap` trait additions (`arity-bitmap`)

```rust
pub trait Bitmap: /* … existing … */ {
    // existing: is_zero, count_ones, test, with_bit, rank, bits
    const BYTES: usize;                                  // WIDTH / 8
    fn without_bit(self, i: Self::Index) -> Self;        // inverse of with_bit
    fn select(self, n: u32) -> Option<Self::Index>;      // index of the n-th set bit (0-based)
    fn to_le_bytes(self, buf: &mut [u8]);                // writes BYTES little-endian bytes
    fn from_le_bytes(buf: &[u8]) -> Self;                // reads BYTES little-endian bytes
}
```

- `without_bit` is the missing inverse of `with_bit`, required by in-place
  `remove` and any generic mutation. Implemented for every backing (native:
  `self & !(1 << i.as_usize())`; `U256`/`ethnum`: clear the bit in the correct
  limb).
- `select(n)` is the inverse of `rank`: it returns the `A::Index` of the `n`-th
  set bit, or `None` when `n >= count_ones()`. It completes the rank/select pair
  so a dense slot maps back to a logical index without iterating.
- `to_le_bytes`/`from_le_bytes` give a **backing-independent** fixed-width
  little-endian byte form of the whole mask (`BYTES == WIDTH / 8`: 1/2/4/8/16/32
  bytes for arity 8…256). They are what lets the `Compact` serde adapter
  reconstruct any `A::Bitmap` generically without naming a concrete `U256` or
  depending on the custom backing's `from_limbs` (see [Serde](#serde) and the
  [`ethnum` swap](#ethnum-backing-swap)). `buf` must be exactly `Self::BYTES`
  long; callers size it from the const. Each backing implements these over its
  native `to_le_bytes`/`from_le_bytes` (or `U256`'s two limbs / `ethnum`'s
  `to_le_bytes`), keeping the limb-ordering detail in one verified place per
  backing.

### Index ergonomics (`arity-index`)

- `impl From<U{n}> for u8` and `impl From<U{n}> for usize` — infallible (the value
  is provably in range), so the index types satisfy `Into<u8>`/`Into<usize>`
  bounds. The native `u8` (arity-256 index) already has these.
- `const fn` where it is both feasible and has a consumer. The niche index
  constructors (`try_new`, `new_masked`, `as_u8`, `as_usize`) are already `const`.
  The bitmap *operations* are exposed through the `Bitmap` trait, and trait methods
  cannot be `const` on stable Rust — nor can inherent `const fn` wrappers be added
  to the native primitive backings (`u8`–`u128`), which are foreign types. The one
  backing defined in this workspace, `U256`, gets a `const fn from_limbs` (free, and
  used by its `from_le_bytes`). Broader inherent `const` construction on `U256`
  (const `with_bit`/`from_le_bytes`, for building 256-bit masks at compile time) is
  **deferred** — no current or planned consumer needs it (serde `Compact`
  deserializes at runtime), and adding inherent `const fn` later is purely additive
  and non-breaking, so there is no cost to waiting for a real const-context use.

### `U256` — custom backing only (`#[cfg(not(feature = "ethnum"))]`)

- `derive(Hash)` — currently absent while `u8`–`u128` have it; required so a
  generic `B: Bitmap + Hash` bound works for every backing.
- A `const fn from_limbs(lo: u128, hi: u128) -> Self` constructor reconstructs a
  mask without `O(popcount)` `with_bit` calls. It is declared **`pub(crate)`** — an
  internal helper of the custom backing, used only by that backing's
  `Bitmap::from_le_bytes` impl (same crate); it is *not* part of the public API
  (it does not exist under `ethnum`, so exposing it would be non-additive — see
  the [`ethnum` swap](#ethnum-backing-swap)). Generic callers reconstruct a mask
  through the trait method `Bitmap::from_le_bytes` instead. The `ethnum` backing
  implements `from_le_bytes` over `ethnum::U256`'s own little-endian conversion.

### Safety documentation

Add `/// # Safety` rustdoc sections (rendering the standard `#safety` anchor) to
the `unsafe fn` caller contracts (`new_unchecked`) and to the type-level
invariants the `unsafe` relies on:

- `NicheRange` / `NicheRangeInclusive`: cursors stay `≤ COUNT - 1`.
- `PackedAllIter` (the existing double-ended all-slots iterator returned by
  `PackedArray::iter()`, yielding `(A::Index, Option<&T>)`):
  `front_rank + back_consumed ≤ count`, where `front_rank` counts set bits yielded
  from the front and `back_consumed` counts set bits yielded from the back.
- `PackedArray`: when allocated, `bitmap != 0`.

Call-site `// SAFETY:` line comments are unchanged — they remain the clippy
`undocumented_unsafe_blocks` convention and are not rustdoc.

## In-place `PackedArray` mutation

The largest new `unsafe` (step 4). Additive — the `FixedArray` round-trip path
stays.

```rust
impl<T, A: Arity> PackedArray<T, A> {
    pub fn insert(&mut self, index: A::Index, value: T) -> Option<T>;
    pub fn remove(&mut self, index: A::Index) -> Option<T>;
    pub fn get_mut(&mut self, index: A::Index) -> Option<&mut T>;
}
```

### Semantics

- `insert`: if `bitmap.test(index)` → overwrite the element at `rank(index)` in
  place and return `Some(old)`, no realloc. The overwrite is
  `ptr::read(slot)` to move out the old `T` (leaving the slot uninitialized, no
  `Drop` run) followed by `ptr::write(slot, value)` to initialize it — the old
  value is handed to the caller as `Some(old)`, so it requires neither `T: Copy`
  nor a guard and produces no double-drop. Otherwise → grow by one element, shift
  the tail right, write `value` at `rank(index)`, set
  `bitmap = bitmap.with_bit(index)`, return `None`. From the empty (`None`)
  pointer → allocate a fresh single-element block.
- `remove`: if the bit is clear → `None`. Otherwise → read out the element at
  `rank(index)`, shift the tail left, shrink by one, set
  `bitmap = bitmap.without_bit(index)`; if the count reaches 0 → `dealloc` and set
  the pointer to `None`. Return `Some(value)`. Upholds the "allocated ⇒
  `bitmap != 0`" invariant.
- `get_mut`: bit set → `&mut *data.add(rank(index))`; else `None`.

### Allocation strategy — exact-size, no spare capacity

Every popcount-changing mutation reallocates to the exact new size. This
preserves the crate's defining properties: pointer-sized, zero-heap-when-empty,
and a heap cost of strictly `bitmap + occupancy · size_of::<T>()` with no capacity
field and no slack. The cost is `O(count)` memcpy plus one (de)allocation per
structural mutation, documented on the methods.

Vec-style amortized spare capacity is **rejected** here (it would add a capacity
word and break the exact-size invariant) and instead captured as the deferred
capacity-bearing sibling type in [Non-goals](#non-goals).

### Implementation — allocate-new + two-segment copy

`insert`/`remove` allocate the new block, `ptr::copy_nonoverlapping` two segments
around the insertion/removal point, then `dealloc` the old block. `insert` copies
`[0, rank)` and `[rank, old_count)` (opening a hole at `rank`); `remove` copies
`[0, rank)` and `[rank + 1, old_count)` (closing the vacated slot). (`realloc`
cannot open or close a hole mid-array, so an explicit two-segment copy is used.)

### Panic safety

Element relocation is `ptr::copy` of bits — it runs **no user code**, so the
shift cannot panic mid-way and there is no partial-move state to guard. The only
fallible step is allocation, handled by the standard `handle_alloc_error` path
**before** any moves occur. The existing `Clone` and owned
`From<FixedArray<Option<T>, A>>` paths need the `FillGuard` drop-guard (it frees
already-moved/cloned elements if a later `T::clone` or move panics mid-fill); the
mutation paths need no such guard, because their only user-code-free moves cannot
panic. A `debug_assert!` confirms `popcount == element count` after each op, and
the fuzz + Miri gate (below) exercises the relocation/alloc/drop interactions
directly.

### Verification gate (lands with the code)

A `cargo-fuzz` target driving randomized `insert`/`remove`/`get_mut`/`get`/
`clone`/`drop` sequences against a `BTreeMap<usize, T>` oracle with a
drop-counting `T`, plus Miri over the new tests with strict provenance. The
mutation code is not considered done until this gate is green.

## Per-arity features

Feature names are consistent across all three crates and all start in `default`:
`"8"`, `"16"`, `"32"`, `"64"`, `"128"`, `"256"`.

### What each arity feature gates

Each row gates its index type **and** that type's `Niche` impl, the matching
`Bitmap` backing impl, and the `Arity` marker:

| Feature | `arity-index` (index + `Niche`) | `arity-bitmap` (`Bitmap` backing) | `arity-arrays` (marker) |
| :------ | :------------------------------ | :-------------------------------- | :---------------------- |
| `"8"`   | `U3`                            | `u8`                              | `Arity8`                |
| `"16"`  | `U4`                            | `u16`                             | `Arity16`               |
| `"32"`  | `U5`                            | `u32`                             | `Arity32`               |
| `"64"`  | `U6`                            | `u64`                             | `Arity64`               |
| `"128"` | `U7`                            | `u128`                            | `Arity128`              |
| `"256"` | `u8` (impl only)¹               | `U256` / `ethnum`                 | `Arity256`              |

¹ Arity-256 uses the primitive `u8` as its index, which always exists; the `"256"`
feature gates only its `Niche` impl (and the `u8` `Bitmap` is the arity-8 backing,
distinct from arity-256's `U256`).

The `seq-macro` invocation for each generated niche type (`U3`–`U7`) is
`#[cfg]`-gated. `arity-arrays`
features forward to the leaves, e.g.:

```toml
[features]
default = ["8", "16", "32", "64", "128", "256"]
"16" = ["arity-index/16", "arity-bitmap/16"]
# … one per arity …
```

### Cross-cutting features

- `std` — off by default (crates stay `no_std`-first). Every dependency that has
  an `std` feature is pulled with `default-features = false`, and the `std`
  feature forwards via weak-dep syntax: `std = ["ethnum?/std", "serde?/std",
  "serde_with?/std", …]`. (Weak-dep features need cargo ≥ 1.60, under MSRV 1.92.)
- `serde`, `serde_with`, `ethnum` — optional, independent of arity selection.
  `ethnum` takes effect only under `"256"` (its impl is
  `#[cfg(all(feature = "256", feature = "ethnum"))]`), so enabling `ethnum`
  without `"256"` is a harmless no-op.

### Edge handling

The primitive `u8` always exists; only its arity-256 `Niche`/`Bitmap` *impls*
gate behind `"256"`. A build with zero arity features compiles (no arity types)
rather than erroring — consistent with the all-on default making an empty build a
deliberate, unusual choice.

### CI feature matrix

A handful of representative columns rather than the full powerset:

- `--all-features` (all arities + `ethnum` + `serde_with` + `std`),
- default (all arities; no serde/ethnum/std),
- `--no-default-features --features 16` (lean single-arity; the firewood shape),
- `--no-default-features --features "16,serde"` (the logical serde path **without**
  `serde_with`, so a misplaced `#[cfg(feature = "serde_with")]` on the logical
  impls is caught),
- `--no-default-features --features "256,ethnum,serde_with,std"` (backing swap +
  compact serde + std-forwarding together),
- `--no-default-features` (no arities) — build-only, proving the crates still
  compile empty.

## `ethnum` backing swap

When the `ethnum` feature is enabled **and** the `256` arity is compiled, the
`Bitmap`/`Raw` impl targets `ethnum::U256` and the custom `U256 { lo, hi }` and
its impl are `#[cfg]`'d out entirely. The two are mutually exclusive — at most one
256-bit backing type ever exists ("wholly replace"). `ethnum::U256` already
provides `count_ones`, `trailing_zeros`, `leading_zeros`, the bit operators,
`Hash`, `Display`, and `to_le_bytes`/`from_le_bytes`, so the bespoke `Hash` and
limb-reconstruction work is unnecessary under this feature.

**`U256` is not a stable public name.** To keep the swap a true implementation
detail, the 256-bit backing type is **not** exported as a stable API: its
crate-root re-export is `#[doc(hidden)]`, and the only supported way to name the
arity-256 bitmap is the associated type `<Arity256 as Arity>::Bitmap`. Naming the
concrete type falls outside the stability guarantee — the `#[doc(hidden)]`
re-export stays importable (Rust does not mechanically forbid `use
arity_bitmap::U256`), but doing so is unsupported — so swapping it between the
custom struct and `ethnum::U256` has **no *supported* type-identity consequence**.
This pushes the Cargo feature-additivity hazard below the stability boundary
rather than merely papering over it with a documentation request. (Cargo only
ever *adds* features across a dependency tree; a type-identity change that
downstream code could observe *within the stability guarantee* would otherwise be
an un-opt-out-able breaking change.)

> [!NOTE]
> **Limb-order footgun.** `ethnum::U256::from_words` takes `(hi, lo)` — high word
> first — the reverse of the custom backing's `from_limbs(lo, hi)`. The byte
> path avoids the issue entirely: each backing implements `from_le_bytes` over
> its own native little-endian conversion, so the limb order lives in exactly one
> verified place per backing and the generic `Compact` adapter never assembles
> words by hand. Implementation note: the `ethnum` `from_le_bytes` branch must
> not be written by analogy to `from_limbs`.

`ethnum` is pulled with `default-features = false` to stay `no_std`; this is
verified at implementation time. Its impl is `#[cfg(all(feature = "256",
feature = "ethnum"))]`, so enabling `ethnum` without `"256"` is a harmless no-op.

## Serde

Two optional features: `serde` (the logical default form) and `serde_with` (the
`Compact` adapter, which implies `serde`).

### Feature wiring

- `serde` on each crate, forwarded:
  `arity-arrays/serde = ["dep:serde", "arity-index/serde", "arity-bitmap/serde"]`.
  The leaf-crate serde impls avoid `alloc` (they touch only fixed-size values).
- `serde_with` lives on **`arity-arrays` only** — the `Compact` adapter needs
  `alloc` for deserialization, which only `arity-arrays` has.
- All pulled `default-features = false`; `std` forwards `serde?/std` and
  `serde_with?/std`.

### Logical form (the default)

- **Index types `U{n}`** — serialized as their integer value; deserialization
  **validates `< COUNT`** and errors otherwise (load-bearing for untrusted input).
- **Bitmap backings get no standalone `Serialize`/`Deserialize`.** No format —
  logical or `Compact` — gives a bitmap a standalone impl. The logical forms
  serialize only index–value pairs or element sequences (below); the `Compact`
  path reads and writes the bitmap through the generic
  `Bitmap::to_le_bytes`/`from_le_bytes` byte surface (see
  [the `Compact` adapter](#the-compact-adapter-serde_with-feature)). So there is
  no standalone `U256` serde impl and therefore no custom-vs-`ethnum` format
  divergence to reason about.
- **`FixedArray<T, A>`** — a sequence of exactly `LEN` elements; deserialization
  requires exactly `LEN`.
- **`PackedArray<T, A>`** — a sequence of `(A::Index, T)` pairs in ascending slot
  order. Format-agnostic (works in JSON, bincode, etc.; no integer-map-key
  requirement). Deserialization validates each index in range and **strictly
  ascending/unique**, then rebuilds the bitmap, rejecting malformed input with
  clear serde errors.

### The `Compact` adapter (`serde_with` feature)

A unit struct `Compact` — defined and exported by `arity-arrays` (as
`arity_arrays::Compact`, **not** from `serde_with`) — implementing
`SerializeAs<PackedArray<T, A>>` / `DeserializeAs<PackedArray<T, A>>`, used as:

```rust
use arity_arrays::{Compact, PackedArray, Arity16};
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize)]
struct Node {
    #[serde_as(as = "Compact")]
    children: PackedArray<Hash, Arity16>,
}
```

Wire form: the bitmap as a fixed `Bitmap::BYTES`-length little-endian array
(1/2/4/8/16/32 bytes for arity 8…256), written and read through
`Bitmap::to_le_bytes`/`from_le_bytes` — so it is **identical regardless of
custom-vs-`ethnum` backing** — followed by the dense values in slot order.
Deserialization validates `popcount(bitmap) == values.len()` and rejects
mismatch.

## Testing strategy

| Area | Tests |
| :--- | :--- |
| `Bitmap` additions | `without_bit`/`select` proptested vs a reference bitset across all backings incl. `U256`/`ethnum`; `select(n) == bits().nth(n)`; `rank`/`select` inverse property |
| Mutation | op-sequence proptests + `cargo-fuzz` vs a `BTreeMap` oracle; drop-exactly-once under `insert`/`remove`; empty↔nonempty churn upholds "allocated ⇒ `bitmap != 0`"; Miri over all of it |
| Features | the CI matrix compiles; the lean `16` build excludes other arities' types |
| Serde | logical + `Compact` round-trip proptests; cross-backing `Compact` byte-identity; adversarial-decode rejects cleanly (out-of-range index, non-ascending, popcount mismatch, wrong length) |
| `ethnum` swap | the same generic test helper, run under both the `256` and `256,ethnum` feature configurations, produces identical results — asserting behavioral parity across the backing swap (the two backings cannot coexist in one build, so parity is checked across two CI columns, not within one binary) |

### Snapshot tests (`insta`)

`insta` is added as a dev-dependency to lock representations prone to silent
drift:

- **Serde output** — JSON snapshots of the `logical` and `Compact` forms for
  representative `PackedArray`/`FixedArray` values across a couple of arities, so
  wire-format changes show up as reviewable snapshot diffs in PRs.
- **`Debug` / `Display` / `LowerHex`** output for `U{n}`, `U256` (both backings,
  asserting identical rendering), and `PackedArray`'s present-slot map.

`insta` is added where used (`arity-arrays`, plus `arity-index`/`arity-bitmap`
for the type renderings); `cargo-insta` is in the tool manifest for the review
workflow.

## Continuous integration

Changes to the existing `.github/workflows/ci.yml`:

- **`test`** — add the [feature-matrix columns](#ci-feature-matrix) above.
- **`miri`** — run over **`--tests`** (not `--lib`) so `tests/roundtrip.rs` and
  the new mutation/serde integration tests run under Miri; set
  `MIRIFLAGS: "-Zmiri-strict-provenance -Zmiri-disable-isolation"` to match the
  `justfile`; bound runtime with `PROPTEST_CASES: 32` in the job env (low enough
  for Miri's interpreter, high enough to exercise the targeted mutation
  properties — `packed_ops` depth is covered separately by out-of-band fuzz soaks).
- **`fuzz`** (new) — time-boxed `cargo +nightly fuzz run` over each target
  (smoke-level in CI via a fixed `-max_total_time`; deeper soaks run out-of-band).
  Corpora can be cached.
- **`lint` / `docs` / `msrv`** — unchanged, except `docs` now covers the new
  public API (still `-D warnings`, so any undocumented new public item or broken
  intra-doc link fails).

### Fuzz targets (`cargo-fuzz`)

- `packed_ops` — randomized `insert`/`remove`/`get`/`get_mut`/`clone`/`drop`
  sequences vs a `BTreeMap<usize, T>` oracle, with a drop-counting/leak-detecting
  `T`. The primary guard for the new mutation `unsafe`.
- `serde_roundtrip` — arbitrary `PackedArray` → `logical`/`Compact` encode/decode
  is identity; arbitrary bytes → decode never panics (clean `Err` only).
- `conversions` — `FixedArray ↔ PackedArray` (owned + by-ref) round-trips.

### Developer tooling (`mise`)

A `mise.toml` at the repo root declares the common tooling CI and `just` share,
pinned in one place and provisioned identically locally and in CI:

- Cargo subtools: `cargo-nextest`, `cargo-fuzz`, `cargo-insta`, `just`.
- **Toolchain split:** CI keeps `dtolnay/rust-toolchain` for the stable / nightly
  / 1.92 matrix (mise does not drive that cleanly), while `mise` (via
  `jdx/mise-action` in CI, and directly for local dev) provisions the auxiliary
  cargo tools the `just` recipes call. The spec notes this division so the two do
  not fight over the toolchain.
- `just` recipes are updated to assume mise-provided tools; a "Developer setup"
  README note points at `mise install`.

## Publishing, semver, and MSRV

### Publish gating

All breaking changes land **before** the first crates.io push, so `0.1.0` ships
already complete and no published API is broken by this work. Publish order
respects the DAG — `arity-index` → `arity-bitmap` → `arity-arrays` — each via
`cargo publish --dry-run` first.

### Version

Cut **`0.1.0`** (not `1.0`): production-*worthy*, but reserving the right to
refine the API with real downstream use before a stability commitment. Under
Cargo semver, `0.1.x` already signals "breaking changes bump the minor," the
honest contract here. A `1.0` is explicit future work once the API has soaked.

### Semver / feature policy (documented in each README)

- The arity features are **additive** and safe to combine.
- `ethnum` is **additive within the stability guarantee**: it changes the concrete
  type of the 256-bit backing, but that type is `#[doc(hidden)]` and is not a
  stable API name, so the swap has no *supported* type-identity consequence.
  Consumers access the arity-256 bitmap only through `<Arity256 as Arity>::Bitmap`
  / `Bitmap`, never by naming `U256`. This contract is stated in the `arity-bitmap`
  README (the only place it can be, since the `U256` rustdoc entry is hidden).
- The serde wire formats (logical and `Compact`) are **`insta`-snapshot-locked but
  not yet guaranteed stable**: both are documented as subject to change before
  `1.0`. The snapshot tests make any drift visible and reviewable in a PR, but the
  formats are intentionally not promised across `0.x` until a real consumer has
  exercised them. This preserves room to adjust length/alignment encoding without
  a contortion if the first production user finds friction.

### MSRV

Keep the declared **1.92**, enforced by the existing `msrv` CI job. The new code
(weak-dep features ≥ 1.60, `const fn` bodies, allocation APIs) is comfortably
under it. MSRV bumps are documented as a minor-version event.

### Release hygiene

- A `CHANGELOG.md` (workspace-level, or per crate), seeded with the `0.1.0`
  entry enumerating the surface.
- A README "feature flags" table per crate (arities, `std`, `serde`,
  `serde_with`, `ethnum`).
- Re-confirm the pinned CI runner images still exist at authoring time (the
  original design flagged these as preview labels).
- `publish` is left unset (it defaults to `true`); a pre-publish checklist task gates the actual push on
  green CI across the full matrix + Miri + fuzz smoke.

## Summary of new public surface

| Crate | Added |
| :--- | :--- |
| `arity-index` | `From<U{n}> for u8`/`usize`; `const fn` on inherent operation methods; `serde` (validated); `/// # Safety` docs; per-arity features |
| `arity-bitmap` | `Bitmap::without_bit`, `select`, `to_le_bytes`/`from_le_bytes`, `BYTES`; `U256: Hash` (custom backing; `U256` itself `#[doc(hidden)]`, named only via `Arity256::Bitmap`); `ethnum` backing swap; `serde` on `U{n}`; per-arity features; `std` |
| `arity-arrays` | `PackedArray::insert`/`remove`/`get_mut`; `serde` (logical) + `arity_arrays::Compact` (`serde_with` adapter); per-arity features; `std`; `/// # Safety` docs |
