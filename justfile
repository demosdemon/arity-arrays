# Common workspace tasks. Run `just` with no arguments to list recipes.

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
    cargo clippy --workspace --all-targets --all-features

# Run tests (default: whole workspace; pass a package to scope, e.g. `just test arity-bitmap`).
test pkg='':
    cargo test {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }} --all-features

# Build docs with warnings denied (default: whole workspace; pass a package to scope).
doc pkg='':
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }}

# Every interpreted operation is far slower than native, so the proptests dominate;
# scope to a package (`just miri arity-index`) or lower PROPTEST_CASES to iterate faster.
# Check unsafe code under Miri — SLOW: a full workspace run takes ~6-10 min.
miri pkg='':
    MIRIFLAGS="{{ miri_flags }}" PROPTEST_CASES={{ proptest_cases }} \
        cargo +nightly miri test {{ if pkg == '' { '--workspace' } else { '--package ' + pkg } }}

# Install the pinned developer tooling (cargo subtools + just) via mise.
setup:
    mise install

# Build/lint the crates under representative feature subsets (mirrors CI `features`).
# Library/lints only — NOT tests. The test suite references types from several
# arities at once, so it compiles and runs only with the default (all-arity)
# feature set; run `just test`, not a per-arity `cargo test`.
features:
    cargo clippy --workspace --no-default-features --features 16 -- -D warnings
    cargo clippy --workspace --no-default-features --features 256 -- -D warnings
    cargo clippy --workspace --no-default-features -- -D warnings

# Run the fast checks (everything except the slow Miri pass).
ci: fmt-check lint features test doc
