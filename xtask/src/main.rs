//! Benchmark-export tooling: parse cargo-criterion JSON, regenerate the
//! comparison charts (SVG) and README tables, and render the CI A/B delta
//! table consumed by the job summary / PR comment (`xtask compare`).

// Tests assert on Result values directly; `unwrap` keeps them terse. This is
// the sanctioned test-wide suppression (crate root, cfg(test)-gated).
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub(crate) mod bench_id;
mod charts;
mod compare;
mod ingest;
mod tables;

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: xtask charts <run.json> [<baseline.json>]\n       xtask compare <run.json> <baseline.json>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("charts") => match run_charts(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask charts: {e}");
                ExitCode::from(1)
            }
        },
        Some("compare") => match run_compare(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask compare: {e}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_charts(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let run_path = args.first().ok_or("missing <run.json> path")?;
    let jsonl = std::fs::read_to_string(run_path).map_err(|e| format!("read {run_path}: {e}"))?;
    // Parse EVERYTHING (run, and baseline if given) before touching any file,
    // so a malformed input aborts without writing partial artifacts.
    let measurements = ingest::parse_run(&jsonl)?;
    let baseline = match args.get(1) {
        Some(p) => {
            let j = std::fs::read_to_string(p).map_err(|e| format!("read {p}: {e}"))?;
            Some(ingest::parse_run(&j)?)
        }
        None => None,
    };

    let table = tables::comparison_table(&measurements);
    let readmes = [
        Path::new("README.md"),
        Path::new("crates/arity-arrays/README.md"),
    ];
    // Render all README replacements in memory; write nothing until every
    // marker substitution has succeeded.
    let mut rewritten: Vec<(PathBuf, String)> = Vec::new();
    for path in readmes {
        let existing =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        rewritten.push((
            path.to_path_buf(),
            tables::render_marked(&existing, &table)?,
        ));
    }

    let bench_dir = Path::new("docs/bench");
    let mut charts = charts::write_charts(&measurements, bench_dir)?;
    if let Some(before) = &baseline {
        charts.extend(charts::write_delta(before, &measurements, bench_dir)?);
    }
    for (path, contents) in rewritten {
        std::fs::write(&path, contents).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    eprintln!(
        "regenerated {} README table(s) and {} chart(s)",
        readmes.len(),
        charts.len()
    );
    Ok(())
}

/// `xtask compare <run.json> <baseline.json>`: print the markdown A/B delta
/// table (run vs baseline) to stdout. Argument order matches `charts`
/// (current/run first, comparison target/baseline second).
fn run_compare(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let run_path = args.first().ok_or("missing <run.json> path")?;
    let baseline_path = args.get(1).ok_or("missing <baseline.json> path")?;
    let run_jsonl =
        std::fs::read_to_string(run_path).map_err(|e| format!("read {run_path}: {e}"))?;
    let baseline_jsonl =
        std::fs::read_to_string(baseline_path).map_err(|e| format!("read {baseline_path}: {e}"))?;
    let run = ingest::parse_run(&run_jsonl)?;
    let baseline = ingest::parse_run(&baseline_jsonl)?;
    print!("{}", compare::render_compare(&baseline, &run));
    Ok(())
}
