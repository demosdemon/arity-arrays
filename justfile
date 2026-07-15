# Common workspace tasks. Run `just` with no arguments to list recipes.
#
# Recipe doc comments are oriented for `just --list`, which shows ONLY the last
# comment line directly above a recipe. Keep any extended explanation on the
# lines above, and make the final comment line a self-contained one-line summary.

# Number of proptest cases used by the Miri recipe. Override per-invocation,
# e.g. `PROPTEST_CASES=16 just miri` to trade coverage for speed.
proptest_cases := env_var_or_default('PROPTEST_CASES', '64')

# Stricter Miri configuration: enforce strict pointer provenance, and disable
# host isolation because proptest reads/writes its regression files on disk.
miri_flags := '-Zmiri-strict-provenance -Zmiri-disable-isolation'

# List available recipes.
default:
    @just --list

# Format all code. Requires nightly: .rustfmt.toml enables nightly-only options.
fmt:
    cargo +nightly fmt --all

# Check formatting without modifying files (for CI).
fmt-check:
    cargo +nightly fmt --all -- --check

# Workspace lints promote pedantic/nursery to warnings and deny undocumented unsafe.
# Lint with Clippy over every target and feature.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Trailing args go to cargo, e.g. `just test arity-arrays --test roundtrip`.
# Run tests (default: whole workspace; pass a package to scope, e.g. `just test arity-bitmap`).
test pkg='' *args:
    cargo test {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }} --all-features {{ args }}

# nextest runs unit + integration + (in test mode) bench targets via --all-targets;
# doctests run separately via `cargo test --doc` because nextest cannot execute
# them. The default-features pass mirrors CI's fast shipping-default check. The
# msrv lane drops xtask, which needs a newer Rust than the workspace MSRV.
# Run the workspace test suite exactly as CI does, for a given toolchain lane.
ci-test toolchain='stable':
    cargo nextest run --workspace{{ if toolchain == 'msrv' { ' --exclude xtask' } else { '' } }} --all-features --all-targets
    cargo test --workspace{{ if toolchain == 'msrv' { ' --exclude xtask' } else { '' } }} --all-features --doc
    cargo nextest run --workspace{{ if toolchain == 'msrv' { ' --exclude xtask' } else { '' } }}

# --all-features documents every feature-gated item and matches CI.
# Build docs with warnings denied (default: whole workspace; pass a package to scope).
doc pkg='':
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }}

# Locks the #![no_std] discipline: if any selected feature transitively pulls std,
# the build fails (no std in the sysroot). Requires the target first:
# `rustup target add thumbv7em-none-eabihf`.
# Build the crate for a bare-metal no_std target.
nostd:
    cargo build -p arity-arrays --no-default-features --features "16,serde" --target thumbv7em-none-eabihf

# Every interpreted operation is far slower than native, so the proptests dominate;
# scope to a package (`just miri arity-index`) or lower PROPTEST_CASES to iterate faster.
# Check unsafe code under Miri — SLOW: a full workspace run takes ~6-10 min.
miri pkg='':
    MIRIFLAGS="{{ miri_flags }}" PROPTEST_CASES={{ proptest_cases }} \
        cargo +nightly miri nextest run {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }}

# Install the pinned developer tooling (cargo subtools + just) via mise.
setup:
    mise install

# Arities are mutually exclusive in the powerset, so each is linted alone
# (8/16/32/64/128/256) rather than in redundant multi-arity subsets; the orthogonal
# serde/serde_with/std features are powerset on top, capped at 4 simultaneous
# flags. --exclude-features default drops the synthetic all-arity `default` member
# (the all-arity build is already covered by `lint` and the test job); without it
# cargo-hack would re-introduce multi-arity combos that slip past
# --mutually-exclusive-features. --depth is the dial if this grows too slow. xtask
# has no arity features and needs a newer Rust than the workspace MSRV, so it is
# excluded. Library/bin lints only (no --all-targets): the tests reference several
# arities at once and won't compile under a single arity set.
# Lint every meaningful feature combination via cargo-hack (mirrors the CI `features` job).
features:
    cargo hack --workspace --exclude xtask --feature-powerset \
        --exclude-features default --mutually-exclusive-features 8,16,32,64,128,256 \
        --depth 4 clippy -- -D warnings

# Mirrors the CI jobs that gate a PR, minus `nostd` — its cross-compile target may
# not be installed locally (`rustup target add thumbv7em-none-eabihf && just nostd`).
# Run the fast checks (everything except the slow Miri pass).
ci: fmt-check lint features ci-test doc

# Run a fuzz target on the host (omit the CI-only gnu target pin). Default 60s.
fuzz target time="60":
    cargo +nightly fuzz run {{target}} -- -max_total_time={{time}} -rss_limit_mb=4096

# Pins the gnu host triple so cargo-fuzz does not default to the musl triple of its
# prebuilt binary (ASAN cannot link against static musl libc). For host-native local
# fuzzing use `just fuzz` instead.
# Run a fuzz target the way CI does, with the gnu host triple pinned. Default 60s.
ci-fuzz target time='60':
    cargo +nightly fuzz run {{target}} --target x86_64-unknown-linux-gnu -- -max_total_time={{time}} -rss_limit_mb=4096

# Builds the image on first use. Never forces emulation: amd64 under qemu aborts in
# ASAN. Default 60s.
# Run a fuzz target inside a host-native Linux container (faithful glibc + ASAN + libFuzzer).
fuzz-linux target time="60":
    docker build -q -f fuzz/Dockerfile -t arity-arrays-fuzz fuzz
    docker run --rm \
      -v {{justfile_directory()}}:/src \
      -v arity-arrays-fuzz-registry:/usr/local/cargo/registry \
      -w /src/fuzz arity-arrays-fuzz \
      cargo fuzz run {{target}} -- -max_total_time={{time}} -rss_limit_mb=4096

# Pass criterion args after `--`, e.g. `just bench -- --sample-size 50`.
# Run both criterion benches via cargo-criterion.
bench *args:
    cargo criterion -p arity-arrays {{ args }}

# The separate build is a fail-fast compile check before the timed run (always
# nightly, release profile). For the local charting workflow use `just bench`
# (cargo-criterion). Extra args pass through to the criterion harness; CI sends
# `--quick` on pull requests to assert the benches run without paying for
# full-precision measurement, and nothing (full suite) on merges to main.
# Both lines name the two criterion benches explicitly: `--benches` (and a bare
# `--workspace`) also runs the libtest unit-test harness in bench mode, which
# would reject a pass-through flag like `--quick`.
# Build then run the benches as a smoke check, the way CI does.
ci-bench *args:
    cargo build --release -p arity-arrays --all-features --bench throughput --bench trie
    cargo bench -p arity-arrays --all-features --bench throughput --bench trie -- {{ args }}

# Build+run the throughput bench under the opt-in `lto-probe` profile (fat
# LTO, codegen-units=1). This measures the profile as a whole, NOT LTO: the
# profile moves `lto` and `codegen-units` together, so a delta against the
# default cannot be attributed to either one. Use `bench-ab` for that question;
# it splits the two apart. Baseline against another `cargo bench -p
# arity-arrays --all-features --bench throughput` run at the default profile —
# NOT against `just bench`, which drives cargo-criterion: the comparison is
# only meaningful between two runs of the same harness (`cargo bench`), so
# baseline with that harness directly rather than via `ci-bench` (which always
# benches `throughput` and `trie` together, never `throughput` alone).
# cargo-criterion rejects `--profile`, which is why this recipe uses `cargo
# bench`; it does honor `CARGO_PROFILE_BENCH_*` overrides, which is how
# `bench-ab` switches profiles while keeping cargo-criterion's JSON export.
# Extra args pass through to criterion, e.g. `just bench-lto --save-baseline lto`.
# Run the throughput bench under the opt-in lto-probe profile.
bench-lto *args:
    cargo build --profile lto-probe -p arity-arrays --all-features --bench throughput
    cargo bench --profile lto-probe -p arity-arrays --all-features --bench throughput -- {{ args }}

# <label> names the capture under the gitignored bench-data/ dir; suffix it with a
# git SHA to keep before/after runs distinct (reusing a label overwrites the earlier
# capture).
# Capture a benchmark run as JSON for charting.
bench-export label:
    mkdir -p bench-data
    cargo criterion -p arity-arrays --message-format=json > bench-data/{{ label }}.json

# With a second <baseline> label, also writes per-cell delta charts (run vs baseline),
# e.g. `just bench-charts branch main`. This recipe passes one capture per side;
# for a capture set with replicates, call `xtask charts --head a.json b.json`
# directly — it averages each side the way `xtask compare` does.
# Regenerate docs/bench/ SVGs and the README comparison tables from a capture.
bench-charts run baseline='':
    cargo run -p xtask -- charts --head bench-data/{{ run }}.json {{ if baseline == '' { '' } else { '--base bench-data/' + baseline + '.json' } }}

# Answers what the `lto-probe` profile actually buys, and whether it is LTO or
# codegen-units: the profile moves both at once, so a two-way comparison cannot
# attribute the delta. Runs three arms (default / lto-probe / codegen-units-only)
# interleaved as a palindrome so every arm shares the run's centroid and linear
# thermal drift cancels, and reports the noise floor alongside the three
# contrasts. `--quick` is directional only; `--update-docs` republishes the
# README tables and SVGs from the default-profile arm. Full precision takes
# hours — see scripts/bench-ab.sh for the rationale and the arm table.
# Run the interleaved A/B/C profile comparison (default vs lto-probe vs codegen-units).
bench-ab *args:
    scripts/bench-ab.sh {{ args }}

# Unlike `bench-charts`, both labels are required: `xtask compare` always needs two
# captures (there is no useful single-run mode). Prints the markdown table CI posts to
# the job summary / PR comment, e.g. `just bench-compare branch main`.
# Print the A/B delta table (run vs baseline) for two captures.
bench-compare run baseline:
    cargo run -p xtask -- compare --head bench-data/{{ run }}.json --base bench-data/{{ baseline }}.json
