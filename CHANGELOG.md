# Changelog

All notable changes to the `arity-*` crates are documented here. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), grouped per crate, and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) — while at
`0.x`, a breaking change bumps the minor version.

## [Unreleased]

### Fixed

- `arity-arrays`: `PackedArray::drop` no longer leaks its heap block when an
  element's destructor panics. The deallocation is now armed in a drop guard
  before the elements are dropped, so it still runs as the stack unwinds —
  matching `GappedArray` and `std::Vec`. A Miri-checked regression test covers
  the panicking-destructor path.

### Changed

- Migrate the benchmark harness from `divan` to `criterion`, run via
  `cargo-criterion`. Benchmark results now export to JSON and feed an
  in-workspace `xtask` that regenerates the README comparison tables and SVG
  charts under `docs/bench/`. Developer tooling only — no library API change.
- Add an automated CI A/B benchmark comparison: pull requests get an automatic quick
  comparison posted as a PR comment, `@exec-complete-benchmark-comparison` triggers an
  on-demand full-precision re-run, and every push to `main` compares against the
  previous commit. Developer tooling only — no library API change.

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

[0.1.0]: https://github.com/demosdemon/arity-arrays/releases/tag/v0.1.0
