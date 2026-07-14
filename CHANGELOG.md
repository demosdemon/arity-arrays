# Changelog

All notable changes to the `arity-*` crates are documented here. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), grouped per crate, and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) — while at
`0.x`, a breaking change bumps the minor version.

## [arity-index Unreleased]

### Documentation

- Document `Niche::as_usize`'s `< COUNT` contract as safety-critical:
  `arity-arrays` relies on it for its internal `slice::get_unchecked` calls.

## [arity-bitmap Unreleased]

### Changed

- Make the crate-internal `Raw` supertrait an `unsafe trait`, so a bitmap
  backing must assert its bit-position contract through an explicit
  `unsafe impl`. The crate's `unsafe_code` lint moves from
  `#![forbid(unsafe_code)]` to `#![deny(unsafe_code)]`; the crate still
  performs no unsafe operations (the only `unsafe` is the audited contract
  marker).

### Documentation

- Mark the safety-critical `Bitmap`/`Raw` methods (`select`, `rank`,
  `count_ones`, `bits`, and the raw scan primitives) whose results feed
  `arity-arrays`'s unchecked pointer arithmetic.

## [arity-arrays Unreleased]

### Fixed

- Bound `A::Bitmap: Send`/`Sync` on the `PackedArray`/`GappedArray` and
  present-only iterator `Send`/`Sync` impls. The heap block and iterators hold
  an `A::Bitmap` by value, so a future non-`Send` backing is now a compile
  error rather than silent unsoundness. Every current backing satisfies the
  bound, so no type loses a `Send`/`Sync` it currently has.

### Documentation

- Document the capacity-overflow panic precondition
  (`size_of::<T>() * A::LEN <= isize::MAX`) on `PackedArray`/`GappedArray` and
  their allocating operations, mirroring `Vec::with_capacity`.

## [arity-index 0.1.1] - 2026-07-14

### Changed

- Mark the trivial niche index accessors and conversions `#[inline]` so they
  inline across crate boundaries.

## [arity-bitmap 0.2.0-alpha.1] - 2026-07-14

### Added

- `Debug` and `Clone` on the public `BitIter`.
- `Bitmap::nearest_clear_at_or_below` and `Bitmap::nearest_clear_in`,
  O(1)-per-limb queries for the nearest clear bit at or below, or within, a
  range. Their result is safety-load-bearing for `arity-arrays`'s unchecked
  pointer arithmetic (documented at the source).

### Changed

- **Breaking:** replace `Bitmap::to_le_bytes`/`from_le_bytes` (a `&[u8]` API that
  panicked on a length mismatch) with an associated `type Bytes = [u8; N]` plus
  `to_bytes`/`from_bytes`, making a wrong-length buffer a compile error. A
  `try_from_bytes(&[u8]) -> Option<Self>` helper covers the runtime-length case
  (e.g. decoding a wire buffer) without panicking. The `Compact` serialization
  wire form is unchanged.
- **Breaking:** make `ethnum::U256` the sole 256-bit backing, re-exported as the
  documented `arity_bitmap::U256`, and remove the custom two-limb backing. The
  `256` feature now enables `ethnum` (a public dependency) and the standalone
  `ethnum` feature is removed.
- `Bitmap::select` is now O(log WIDTH) for both the native integer backings and
  the 256-bit backing, replacing the previous linear scan.

## [arity-arrays 0.2.0-alpha.1] - 2026-07-14

### Added

- `GappedArray<T, A>` — a pointer-sized, heap-backed array that keeps spare
  capacity and allows gaps, so deletes never move elements and inserts move only
  to reach a nearby hole. It trades memory for mutation throughput (geometric
  power-of-two growth bounded by `A::LEN`), complementing `PackedArray`'s
  occupancy-proportional layout. The surface mirrors the other containers:
  `get`/`get_mut`, move-free `remove` with capacity retention, `insert` with
  shift-or-respread placement, capacity management
  (`with_capacity`/`reserve`/`shrink_to_fit`/`clear`), present-only and
  all-slots double-ended iterators plus `IntoIterator`, panic-safe `Clone` and
  `Drop`, `Eq`/`Hash`/`Debug` and thread-safety impls, conversions to and from
  `FixedArray` and `PackedArray`, and optional `serde` (logical form) plus the
  `serde_with::Compact` adapter.
- `Debug` and `Clone` on the public packed and gapped iterator types
  (present-only and all-slots).

### Changed

- **Breaking:** remove the forwarding `ethnum` cargo feature. It re-exported
  `arity-bitmap`'s standalone `ethnum` feature, which no longer exists; `ethnum`
  is now pulled in unconditionally by the `256` feature.
- Narrow the `bitmap`/`index` facade modules to a fixed, named set of re-exports
  from each sibling crate instead of whole-crate re-exports. Every path that
  resolves today (e.g. `arity_arrays::index::U4`) still resolves; new sibling
  items no longer propagate automatically.

### Fixed

- `PackedArray::drop` no longer leaks its heap block when an element's destructor
  panics. The deallocation is now armed in a drop guard before the elements are
  dropped, so it still runs as the stack unwinds — matching `GappedArray` and
  `std::Vec`. A Miri-checked regression test covers the panicking-destructor
  path.
- `GappedArray` insert now locates the nearest hole with an O(log WIDTH) bitmap
  query instead of an O(distance) bit-by-bit scan, restoring its
  mutation-throughput advantage on the near-full / sequential workload it is
  designed for (previously ~3.8× slower `build` and ~2.3× slower `churn` than
  `PackedArray` for small payloads at wide arity).

## [0.1.0] - 2026-06-28

Initial release of three `no_std` crates for fixed-arity storage indexed by
bounds-check-free niche integers, generalizing the hexary trie child-storage
layout from [`ava-labs/firewood#2100`](https://github.com/ava-labs/firewood/pull/2100)
to power-of-two arities 8–256.

### `arity-index`

- Niche integer index types `U3`–`U7` (and the native `u8` for arity-256): each
  `Option<U{n}>` is one byte, and indexing a `2ⁿ`-length array elides the bounds
  check.
- The sealed `Niche` trait; `From<U{n}>` for `u8`/`usize`; `const fn`
  constructors; double-ended `NicheRange` / `NicheRangeInclusive` iterators.
- Per-arity cargo features (`8`–`256`); optional validated `serde`; `std`.

### `arity-bitmap`

- The sealed `Bitmap` trait over `u8`–`u128` and a 256-bit backing, indexed by the
  niche types: `test`, `with_bit`, `without_bit`, `rank`, `select`, `count_ones`,
  a backing-independent `to_le_bytes`/`from_le_bytes` surface, and a double-ended
  set-bit iterator. No `unsafe` code.
- Per-arity cargo features; an optional `ethnum::U256` backing for arity-256
  (the 256-bit type is `#[doc(hidden)]`, accessed only via `Arity256::Bitmap`);
  `std`.

### `arity-arrays`

- `FixedArray<T, A>` (full-width inline storage) and `PackedArray<T, A>`
  (pointer-sized, heap-packed, occupancy-proportional) over a sealed `Arity`
  trait for arities 8–256.
- In-place `PackedArray` mutation (`insert`, `remove`, `get_mut`); conversions to
  and from `FixedArray`; double-ended iterators; `Drop`/`Clone`/`Eq`/`Hash`/`Debug`.
- Per-arity cargo features; optional `serde` (logical form) and a
  `serde_with::Compact` adapter; an `ethnum` backing passthrough; `std`.

[arity-index Unreleased]: https://github.com/demosdemon/arity-arrays/compare/arity-index-v0.1.1...HEAD
[arity-bitmap Unreleased]: https://github.com/demosdemon/arity-arrays/compare/arity-bitmap-v0.2.0-alpha.1...HEAD
[arity-arrays Unreleased]: https://github.com/demosdemon/arity-arrays/compare/arity-arrays-v0.2.0-alpha.1...HEAD
[arity-index 0.1.1]: https://github.com/demosdemon/arity-arrays/releases/tag/arity-index-v0.1.1
[arity-bitmap 0.2.0-alpha.1]: https://github.com/demosdemon/arity-arrays/releases/tag/arity-bitmap-v0.2.0-alpha.1
[arity-arrays 0.2.0-alpha.1]: https://github.com/demosdemon/arity-arrays/releases/tag/arity-arrays-v0.2.0-alpha.1
[0.1.0]: https://github.com/demosdemon/arity-arrays/releases/tag/v0.1.0
