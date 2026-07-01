//! Shared `Criterion` config for `throughput.rs` and `trie.rs`, included via
//! `#[path]` (same pattern as `support.rs`/`trie_fixture.rs`) since each bench
//! target is a separate binary crate and can't `use` a sibling bench file
//! directly.

use criterion::Criterion;

/// `BENCH_QUICK=1` shrinks sample size/timing for a fast CI comparison.
/// cargo-criterion does not forward `--quick` (or any other criterion CLI
/// flag) to the harness the way plain `cargo bench` does, so this has to be
/// read directly rather than via `Criterion::configure_from_args`.
///
/// `nresamples` (bootstrap resamples for the confidence-interval analysis
/// that follows each point's measurement) dominates per-point wall time far
/// more than `sample_size`/`measurement_time` do: `sample_size(10)` is
/// already criterion's enforced floor, but the default `nresamples` of
/// `100_000` still ran a multi-second bootstrap after every point, which is
/// what actually timed out CI. `1_000` is criterion's own documented minimum
/// before it warns.
///
/// This must feed `criterion_group!`'s `config = ...` (the long form), not
/// just a `Criterion` built in `main`: the short form `criterion_group!(
/// benches, ...)` expands to a `benches()` function that constructs its own
/// internal `Criterion::default().configure_from_args()` and passes that to
/// every benchmark body — a `Criterion` built in `main` and passed only to
/// `.final_summary()` afterward never actually reaches the benchmarks.
pub fn quick_criterion() -> Criterion {
    let c = Criterion::default();
    if std::env::var_os("BENCH_QUICK").is_some() {
        c.sample_size(10)
            .warm_up_time(std::time::Duration::from_millis(100))
            .measurement_time(std::time::Duration::from_millis(500))
            .nresamples(1000)
    } else {
        c
    }
}
