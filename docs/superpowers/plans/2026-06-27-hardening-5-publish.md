# Hardening Plan 5 — Publish Prep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three crates publish-ready for a coherent `0.1.0`: per-crate "Cargo features" tables and the semver/feature-policy notes, a seeded `CHANGELOG`, and a verified `cargo publish --dry-run` in dependency order.

**Architecture:** Documentation and release-hygiene only — no library code changes. Each crate README gains a feature-flags table and the policy notes the spec assigns to it (arity-bitmap: the `ethnum`/`U256`-opacity contract; arity-arrays: the serde-format stability note); a workspace-level `CHANGELOG.md` enumerates the `0.1.0` surface; a verification task runs the publish dry-runs and re-confirms the CI runner labels.

**Tech Stack:** Markdown; `cargo publish --dry-run`; `cargo doc`. No new dependencies. No code.

This is **plan 5 of 5** for the production-hardening effort
(`breaking-api` ✓ → `features-ci` ✓ → `mutation` ✓ → `serde-ethnum` ✓ → **`publish`**).
Design spec: `docs/superpowers/specs/2026-06-27-arity-arrays-hardening-design.md`
(section "Publishing, semver, and MSRV").

## Global Constraints

Copied from the spec; every task implicitly includes these.

- **No code changes** — this plan touches only `README.md` files, a new `CHANGELOG.md`, and (verification only) runs `cargo publish --dry-run` / `cargo doc`. Do NOT modify any `src/` file or any `Cargo.toml` (the package metadata is already complete — `description`, `readme`, `keywords`, `categories`, `license`, `repository`, `homepage` are all set, satisfying `cargo_common_metadata`).
- **Version `0.1.0`** for all three crates (already set). `publish` is left unset (defaults `true`). **Do NOT run a real `cargo publish`** — only `--dry-run`. The actual push to crates.io is the maintainer's out-of-band step, gated on green CI.
- **Publish order (DAG):** `arity-index` → `arity-bitmap` → `arity-arrays`.
- **MSRV 1.92** (already declared and CI-enforced). Edition 2024.
- **Markdown:** GitHub-flavored; tables need a blank line before them; the existing READMEs end with `## no_std` / `## MSRV` / `## License` sections — insert the new sections before `## no_std` to keep License last. Conventional-commit messages, imperative mood.
- **Cargo.lock is gitignored** — never `git add` it.

---

### Task 1: README "Cargo features" tables + policy notes

Add a "Cargo features" section to each crate README (inserted before the existing `## no_std` section), plus the per-crate policy notes the spec assigns: the `ethnum`/`U256`-opacity contract (arity-bitmap) and the serde-format stability note (arity-arrays). Add the "tests run under the default feature set" note where relevant.

**Files:**
- Modify: `crates/arity-index/README.md` (insert before `## \`no_std\``)
- Modify: `crates/arity-bitmap/README.md` (insert before `## \`no_std\``)
- Modify: `crates/arity-arrays/README.md` (insert before `## \`no_std\``)

**Interfaces:**
- Consumes: the feature names as shipped (`8`…`256`, `serde`, `serde_with`, `ethnum`, `std`).
- Produces: documentation only.

- [ ] **Step 1: Add the features section to `crates/arity-index/README.md`**

Insert this block immediately before the `## \`no_std\`` heading:

```markdown
## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the index types you use. The numbers are the arity (`8` → `U3`, …, `128` → `U7`, `256` → the native `u8` index). To compile a subset, disable defaults: `arity-index = { version = "0.1", default-features = false, features = ["16"] }`. |
| `serde` | | `Serialize`/`Deserialize` for `U3`–`U7` (serialized as their integer value; deserialization **validates** the value is in range). `no_std`-compatible. |
| `std` | | Forwards `std` to optional std-capable dependencies. The crate is `no_std`-first; this feature only matters when `serde` is also enabled. |

The arity features are **additive** and safe to combine. The test suite compiles
and runs only under the default (all-arity) feature set — run `cargo test`, not a
per-arity `cargo test --no-default-features --features 16`.
```

- [ ] **Step 2: Add the features section + the `ethnum`/`U256` opacity contract to `crates/arity-bitmap/README.md`**

Insert this block immediately before the `## \`no_std\`` heading:

```markdown
## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the bitmap backings you use (`8` → `u8`, …, `128` → `u128`, `256` → the 256-bit backing). Forwards to the matching `arity-index` feature. |
| `ethnum` | | Swaps the arity-256 backing from the self-contained two-limb `U256` to [`ethnum::U256`](https://docs.rs/ethnum). Takes effect only when `256` is also enabled. |
| `std` | | Forwards `std`; the crate is `no_std`-first. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set.

### The 256-bit backing is opaque

By default, arity-256 uses a self-contained two-limb `U256`; the `ethnum` feature
swaps it for `ethnum::U256`. **The concrete 256-bit type is `#[doc(hidden)]` and
is not a stable API name.** Access the arity-256 bitmap only through the trait —
`<Arity256 as Arity>::Bitmap`, or generically as `B: Bitmap` — never by naming
`U256` directly. Because no supported code path names the concrete type, the
`ethnum` swap is a non-observable implementation detail (it does not change any
stable type identity). Naming `arity_bitmap::U256` directly is unsupported and may
break between releases.
```

- [ ] **Step 3: Add the features section + the serde-format note to `crates/arity-arrays/README.md`**

Insert this block immediately before the `## \`no_std\`` heading:

```markdown
## Cargo features

| Feature | Default | Description |
| :--- | :---: | :--- |
| `8`, `16`, `32`, `64`, `128`, `256` | ✓ | Per-arity gating — compile only the `Arity{N}` markers you use. Forwards to the matching `arity-index`/`arity-bitmap` features. The hexary (firewood) shape is `default-features = false, features = ["16"]`. |
| `serde` | | `Serialize`/`Deserialize` for `FixedArray` (a sequence of `LEN` elements) and `PackedArray` (a sequence of ascending `(index, value)` pairs, validated on decode). |
| `serde_with` | | Adds the [`Compact`] adapter (`#[serde_as(as = "Compact")]`) — a compact, backing-independent `PackedArray` encoding (fixed little-endian bitmap + dense values). Implies `serde`. |
| `ethnum` | | Forwards to `arity-bitmap/ethnum` (the arity-256 backing swap). |
| `std` | | Forwards `std` to the optional std-capable dependencies; the crate is `no_std` + `alloc`. |

The arity features are **additive**. The test suite runs only under the default
(all-arity) feature set — run `cargo test`, not a per-arity `cargo test`.

### Serialization stability

The serde wire formats (the logical `(index, value)` form and the `Compact`
form) are locked by snapshot tests so any drift is a reviewable diff, but they
are **not yet guaranteed stable**: they may change before `1.0` if a production
consumer's encoding needs differ. The `Compact` form is backing-independent — it
is identical whether the arity-256 backing is the custom `U256` or `ethnum::U256`.

[`Compact`]: https://docs.rs/arity-arrays/latest/arity_arrays/struct.Compact.html
```

- [ ] **Step 4: Verify the READMEs render and the workspace docs build**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
# Markdown sanity: the new tables have a blank line before them and License stays last.
grep -nE '## Cargo features|## `no_std`|## License' crates/*/README.md
# The docs job still builds clean (READMEs are not doctested, but confirm no doc regressions):
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: each README shows `## Cargo features` before `## \`no_std\`` before `## License`; docs build with no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arity-index/README.md crates/arity-bitmap/README.md crates/arity-arrays/README.md
git commit -m "docs: add Cargo feature tables and semver/opacity policy to READMEs"
```

---

### Task 2: workspace `CHANGELOG.md`

Create a workspace-level `CHANGELOG.md` seeded with the `0.1.0` entry enumerating the public surface of each crate.

**Files:**
- Create: `CHANGELOG.md` (workspace root)

**Interfaces:**
- Consumes: nothing.
- Produces: the changelog (documentation only).

- [ ] **Step 1: Create `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to the `arity-*` crates are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crates
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) — while at
`0.x`, a breaking change bumps the minor version.

## [0.1.0]

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
```

- [ ] **Step 2: Verify the changelog is valid Markdown**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
test -f CHANGELOG.md && head -5 CHANGELOG.md
grep -nE '## \[0.1.0\]|### `arity-(index|bitmap|arrays)`' CHANGELOG.md
```
Expected: the file exists and starts with `# Changelog`; the second grep shows the `## [0.1.0]` heading and all three `### \`arity-*\`` sub-sections (catches a truncated/mis-pasted body).

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add CHANGELOG seeded with the 0.1.0 surface"
```

---

### Task 3: pre-publish verification

Run `cargo publish --dry-run` in dependency order, re-confirm the CI runner image labels, and produce a green-CI checklist. No files change.

**Files:**
- None (verification only; the report is the deliverable).

**Interfaces:**
- Consumes: the published-ready manifests + READMEs + CHANGELOG (Tasks 1–2).
- Produces: a verification report.

- [ ] **Step 1: Dry-run publish each crate in dependency order**

The working tree must be clean first (commit Tasks 1–2). Run the **workspace** dry-run as a single command:
```bash
cd "$(git rev-parse --show-toplevel)"
cargo publish --dry-run --workspace
```
Expected: cargo packages, verify-builds, and dry-run-"uploads" all three crates with no errors (each ends in `warning: aborting upload due to dry run`).

> Do **not** use the per-crate form `cargo publish --dry-run -p arity-bitmap` — it FAILS with `error: ... no matching package named \`arity-index\` found`, because a dependent crate's packaging step resolves its `arity-index = "0.1.0"` dependency against crates.io, where it is not yet published. (`--no-verify` does **not** fix this — the failure is in packaging, not the verify build.) The workspace form (`cargo publish --workspace`, stabilized in Rust 1.89, available on the MSRV-1.92 toolchain) resolves the inter-crate deps within the publish set and is the correct way to dry-run an unpublished workspace. This was verified to succeed on this workspace.

If the dry-run reports an error (missing metadata, an uncommitted change, a packaging problem), capture it verbatim — it is a real publish blocker.

- [ ] **Step 2: Re-confirm the CI runner image labels still exist**

The workflow pins preview-era runner labels. Confirm each is still a valid GitHub-hosted runner image against the current catalog (`https://github.com/actions/runner-images`), using a web fetch of that page or its README:
```bash
grep -oE '(windows|macos|ubuntu)-[0-9a-z.-]+' .github/workflows/ci.yml | sort -u
```
For each label (`windows-2025-vs2026`, `macos-26`, `ubuntu-26.04`, `ubuntu-26.04-arm`), confirm it appears in the runner-images catalog. If any label has been renamed or removed, report it (do NOT change `ci.yml` in this task — flag it for a follow-up so the change is reviewed deliberately).

- [ ] **Step 3: Run the full local CI-equivalent suite**

Confirm the workspace is green across the gates the publish depends on:
```bash
cd "$(git rev-parse --show-toplevel)"
cargo +nightly fmt --all --check
cargo +stable clippy --workspace --all-targets --all-features -- -D warnings   # CI lints on +stable
cargo test --workspace --all-features
cargo test --workspace            # default features (custom U256 backing)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Expected: all green. (The Miri and fuzz jobs are slow and not part of this local fast suite — they run in CI; the maintainer confirms them on the pushed branch.)

- [ ] **Step 4: Write the verification report**

Create `docs/superpowers/2026-06-27-prepublish-verification.md`. Open it with the captured output from Steps 1–3 — the workspace dry-run pass/fail lines verbatim, the runner-label confirmation (or the flagged discrepancy), and the fast-suite results — then append this checklist for the maintainer's out-of-band publish:
- [ ] `cargo publish --dry-run --workspace` clean for all three crates (Step 1);
- [ ] CI runner labels confirmed valid, or the discrepancy flagged (Step 2);
- [ ] local fmt/clippy/test/doc green (Step 3);
- [ ] CI green on the pushed branch (the maintainer confirms on GitHub — includes the Miri and fuzz jobs, which are not part of the fast local suite);
- [ ] publish: `cargo publish --workspace` (sequential in dependency order — publishes arity-index, then arity-bitmap, then arity-arrays; a failure after the first upload leaves the earlier crates live on crates.io), or, if publishing individually, `cargo publish -p arity-index` then `-p arity-bitmap` then `-p arity-arrays`, each after the previous lands on crates.io;
- [ ] tag `v0.1.0`.

- [ ] **Step 5: Commit the verification report into the repo for the record**

This task changes no source, but the report is a durable artifact. Write it to `docs/superpowers/2026-06-27-prepublish-verification.md` and commit:
```bash
git add docs/superpowers/2026-06-27-prepublish-verification.md
git commit -m "docs: record pre-publish dry-run verification and checklist"
```

---

## Self-Review

- **Spec coverage ("Publishing, semver, and MSRV"):**
  - README "feature flags" table per crate (arities, `std`, `serde`, `serde_with`, `ethnum`) → Task 1. ✓
  - Semver/feature policy in each README (arity features additive; `ethnum` opacity contract in arity-bitmap; serde formats snapshot-locked-not-guaranteed in arity-arrays) → Task 1 Steps 2–3. ✓
  - "tests are all-arity-only" note → Task 1 (each README). ✓
  - `CHANGELOG.md` seeded with the `0.1.0` surface → Task 2. ✓
  - `cargo publish --dry-run` in dependency order → Task 3 Step 1. ✓
  - Re-confirm the pinned CI runner images → Task 3 Step 2. ✓
  - Version `0.1.0`, `publish` unset, no real publish (maintainer gates on green CI) → Global Constraints + Task 3 checklist. ✓
  - MSRV 1.92 (already enforced) → unchanged. ✓
- **Out of scope (intentionally not done here):** changing `ci.yml` runner labels (flag-only, to keep the change reviewed); a `1.0` stability commitment (explicit future work); doctesting the README examples (would add infrastructure beyond release hygiene).
- **Placeholder scan:** none — every step has the complete markdown/commands.
- **Consistency:** the feature names in the README tables (`8`–`256`, `serde`, `serde_with`, `ethnum`, `std`) match the shipped `[features]` tables exactly; the publish order and the `0.1.0` version match the spec and the manifests.
