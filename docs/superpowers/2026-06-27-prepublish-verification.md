# Pre-publish Verification — 2026-06-27

Workspace: `arity-arrays` · Branch: `hardening/publish` · Rust toolchain: stable (MSRV 1.92)

---

## Step 1: Workspace dry-run (`cargo publish --dry-run --workspace`)

```
   Updating crates.io index
   Packaging arity-index v0.1.0 (crates/arity-index)
    Packaged 8 files, 30.1KiB (8.5KiB compressed)
   Packaging arity-bitmap v0.1.0 (crates/arity-bitmap)
    Packaged 11 files, 46.1KiB (12.1KiB compressed)
   Packaging arity-arrays v0.1.0 (crates/arity-arrays)
    Packaged 17 files, 111.3KiB (28.2KiB compressed)
   Verifying arity-index v0.1.0 (crates/arity-index)
   Compiling arity-index v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.99s
   Verifying arity-bitmap v0.1.0 (crates/arity-bitmap)
   Compiling arity-bitmap v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.36s
   Verifying arity-arrays v0.1.0 (crates/arity-arrays)
   Compiling arity-arrays v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.75s
   Uploading arity-index v0.1.0 (crates/arity-index)
warning: aborting upload due to dry run
   Uploading arity-bitmap v0.1.0 (crates/arity-bitmap)
warning: aborting upload due to dry run
   Uploading arity-arrays v0.1.0 (crates/arity-arrays)
warning: aborting upload due to dry run
```

**Result: PASS** — all three crates packaged, verify-built, and dry-run-uploaded without errors.

---

## Step 2: CI runner image label confirmation

Labels extracted from `.github/workflows/ci.yml`:

```
macos-26
ubuntu-26.04
ubuntu-26.04-arm
windows-2025-vs2026
```

Status against the `actions/runner-images` catalog (fetched 2026-06-27):

| Label | Status |
| :--- | :--- |
| `windows-2025-vs2026` | **Present** (preview — Windows Server 2025 + Visual Studio 2026, x64) |
| `macos-26` | **Present** (GA — arm64; `-intel`/`-xlarge` variants also available) |
| `ubuntu-26.04` | **Present** (preview — x64) |
| `ubuntu-26.04-arm` | **Present** (preview — arm64; also listed as `ubuntu-26.04-arm64`) |

**Result: PASS** — all four labels are valid in the current runner-images catalog. No `ci.yml` changes required.

---

## Step 3: Local CI-equivalent fast suite

All commands run from the workspace root on 2026-06-27.

| Command | Result |
| :--- | :--- |
| `cargo +nightly fmt --all --check` | **PASS** |
| `cargo +stable clippy --workspace --all-targets --all-features -- -D warnings` | **PASS** |
| `cargo test --workspace --all-features` | **PASS** (49 unit + integration + doc tests) |
| `cargo test --workspace` | **PASS** (default features — custom U256 backing) |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` | **PASS** |

---

## Maintainer Checklist

Before issuing `cargo publish --workspace`:

- [x] `cargo publish --dry-run --workspace` clean for all three crates (Step 1 above)
- [x] CI runner labels confirmed valid against the current runner-images catalog (Step 2 above)
- [x] Local fmt / clippy / test / doc green (Step 3 above)
- [ ] CI green on the pushed branch — confirm on GitHub (includes Miri and fuzz jobs, which are not part of the fast local suite)
- [ ] Publish: `cargo publish --workspace` (sequential in dependency order — publishes arity-index, then arity-bitmap, then arity-arrays; a failure after the first upload leaves the earlier crates live on crates.io), **or** if publishing individually: `cargo publish -p arity-index`, then `cargo publish -p arity-bitmap`, then `cargo publish -p arity-arrays` — each after the previous has landed on crates.io
- [ ] Tag `v0.1.0`
