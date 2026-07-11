//! Golden snapshots of the CI-facing render output: the compare delta table
//! (published verbatim to job summaries and PR comments) and the README
//! comparison tables. The stored snapshots match origin/main's rendering of
//! the same fixture, captured before the uniformity refactor — a failure
//! here means the published bytes changed.

/// Two benchmark-complete lines covering both table families
/// (throughput + trie).
const FIXTURE: &str = concat!(
    r#"{"reason":"benchmark-complete","id":"throughput/cell_a/get_hit/PackedArray/4","median":{"estimate":1.1,"lower_bound":1.0,"upper_bound":1.2,"unit":"ns"}}"#,
    "\n",
    r#"{"reason":"benchmark-complete","id":"trie/arity16/clone/PackedStore/Bushy","median":{"estimate":250.0,"lower_bound":240.0,"upper_bound":260.0,"unit":"ns"}}"#,
    "\n",
);

#[test]
fn compare_delta_table() {
    let run = xtask::ingest::parse_run(FIXTURE).expect("fixture parses");
    let avg = xtask::ingest::average_runs(&[run]);
    insta::assert_snapshot!(xtask::delta::render_compare(&avg, &avg));
}

#[test]
fn readme_comparison_table() {
    let run = xtask::ingest::parse_run(FIXTURE).expect("fixture parses");
    insta::assert_snapshot!(xtask::tables::comparison_table(&run));
}
