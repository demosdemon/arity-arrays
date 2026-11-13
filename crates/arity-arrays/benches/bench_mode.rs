//! Shared `Criterion` config for the criterion benches (`throughput.rs` and
//! `trie.rs` here, `ops.rs` in arity-tries), included via `#[path]` (same
//! pattern as `support.rs`/`trie_fixture.rs`) since each bench target is a
//! separate binary crate and can't `use` a sibling bench file directly.

use std::time::Duration;

use criterion::Criterion;

/// Builds the `Criterion` config selected by the `BENCH_MODE` environment
/// variable:
///
/// | `BENCH_MODE`     | samples | warm-up | measurement | resamples |
/// | ---------------- | ------: | ------: | ----------: | --------: |
/// | unset or empty   |     100 |      3s |          5s |   100,000 |
/// | `ci`             |      50 |   500ms |        1.5s |    10,000 |
/// | `quick`          |      10 |   100ms |       500ms |     1,000 |
///
/// Unset or empty uses criterion's own defaults, for local runs. `ci` is the
/// full-precision CI comparison: it must fit four captures of every benchmark
/// point (an interleaved base/head/head/base run) on one hosted runner under
/// GitHub's 6h job limit. Projected from per-benchmark iteration times in a
/// 2026-11 ubuntu-26.04 capture, criterion's defaults would need ~12h for that
/// run and `ci` ~4h.
/// `quick` is the fast pull-request smoke comparison. Any other value panics,
/// so a typo cannot silently fall back to a different mode.
///
/// The mode comes from the environment because cargo-criterion does not
/// forward `--quick` (or any other criterion CLI flag) to the harness the way
/// plain `cargo bench` does, so `Criterion::configure_from_args` never sees
/// one.
///
/// `nresamples` (bootstrap resamples for the confidence-interval analysis
/// after each point's measurement) is cut alongside the timings because its
/// cost scales with the resample count and, at criterion's default of
/// `100_000`, it was a significant share of each point on a CI runner.
/// `quick`'s `1_000` sits exactly at criterion's warning threshold: the setter
/// warns for any `n <= 1000`, so each call in `quick` mode prints "Warning: It
/// is not recommended to reduce nresamples below 1000." to stderr, an accepted
/// trade-off for CI speed.
///
/// This must feed `criterion_group!`'s `config = ...` (the long form), not
/// just a `Criterion` built in `main`: the short form `criterion_group!(
/// benches, ...)` expands to a `benches()` function that constructs its own
/// internal `Criterion::default().configure_from_args()` and passes that to
/// every benchmark body, so a `Criterion` built in `main` and passed only to
/// `.final_summary()` afterward never reaches the benchmarks.
///
/// # Panics
///
/// Panics if `BENCH_MODE` is set to anything other than empty, `ci`, or
/// `quick`.
pub fn configured_criterion() -> Criterion {
    let c = Criterion::default();
    let mode = std::env::var("BENCH_MODE").unwrap_or_default();
    match mode.as_str() {
        "" => c,
        "ci" => c
            .sample_size(50)
            .warm_up_time(Duration::from_millis(500))
            .measurement_time(Duration::from_millis(1500))
            .nresamples(10_000),
        "quick" => c
            .sample_size(10)
            .warm_up_time(Duration::from_millis(100))
            .measurement_time(Duration::from_millis(500))
            .nresamples(1000),
        other => panic!("BENCH_MODE must be unset, empty, `ci`, or `quick`; got {other:?}"),
    }
}
