//! CLI command implementations, one module per subcommand.

pub mod charts;
pub mod compare;
#[cfg(feature = "ec2-bench")]
pub mod ec2_bench;

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;

use crate::ingest::Measurement;

/// Read and parse one cargo-criterion JSONL capture.
fn read_measures(path: impl AsRef<Path>) -> anyhow::Result<Vec<Measurement>> {
    let path = path.as_ref();
    let jsonl = crate::fs::read_to_string(path)?;
    crate::ingest::parse_run(&jsonl).with_context(|| format!("failed to parse {}", path.display()))
}

/// Read several captures (one per repeated `--head`/`--base` flag).
fn read_runs(paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<Measurement>>> {
    paths.iter().map(read_measures).collect()
}
