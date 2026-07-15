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

use anyhow::Context;
use anyhow::bail;

const USAGE: &str = "usage: xtask charts <run.json> [<baseline.json>]\n       xtask compare --head <run.json>... --base <baseline.json>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("charts") => run_charts(&args[1..]).context("xtask charts"),
        Some("compare") => run_compare(&args[1..]).context("xtask compare"),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // Alternate Debug renders the whole context chain, plus a backtrace when
        // RUST_BACKTRACE is set.
        Err(e) => {
            eprintln!("{e:?}");
            ExitCode::from(1)
        }
    }
}

fn run_charts(args: &[String]) -> anyhow::Result<()> {
    let Some(run_path) = args.first() else {
        bail!("missing <run.json> path");
    };
    let jsonl = std::fs::read_to_string(run_path).with_context(|| format!("read {run_path}"))?;
    // Parse EVERYTHING (run, and baseline if given) before touching any file,
    // so a malformed input aborts without writing partial artifacts.
    let measurements = ingest::parse_run(&jsonl).with_context(|| format!("parse {run_path}"))?;
    let baseline = match args.get(1) {
        Some(p) => {
            let j = std::fs::read_to_string(p).with_context(|| format!("read {p}"))?;
            Some(ingest::parse_run(&j).with_context(|| format!("parse {p}"))?)
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
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        rewritten.push((
            path.to_path_buf(),
            tables::render_marked(&existing, &table)
                .with_context(|| format!("rewrite the bench table in {}", path.display()))?,
        ));
    }

    let bench_dir = Path::new("docs/bench");
    let mut charts = charts::write_charts(&measurements, bench_dir)?;
    if let Some(before) = &baseline {
        charts.extend(charts::write_delta(before, &measurements, bench_dir)?);
    }
    for (path, contents) in rewritten {
        std::fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
    }
    eprintln!(
        "regenerated {} README table(s) and {} chart(s)",
        readmes.len(),
        charts.len()
    );
    Ok(())
}

/// `xtask compare --head <run.json>... --base <baseline.json>...`: average the
/// captures on each side (interleaved A/B/B/A replicates) and print the
/// markdown A/B delta table to stdout.
fn run_compare(args: &[String]) -> anyhow::Result<()> {
    let (head_paths, base_paths) = parse_compare_args(args)?;
    let head = ingest::average_runs(&read_runs(&head_paths)?);
    let base = ingest::average_runs(&read_runs(&base_paths)?);
    print!("{}", compare::render_compare(&base, &head));
    Ok(())
}

/// Split `--head <f>... --base <f>...` (either flag order) into two file
/// groups. Each flag must precede at least one path.
fn parse_compare_args(args: &[String]) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    enum Side {
        None,
        Head,
        Base,
    }
    let mut head = Vec::new();
    let mut base = Vec::new();
    let mut side = Side::None;
    for a in args {
        match a.as_str() {
            "--head" => side = Side::Head,
            "--base" => side = Side::Base,
            other => match side {
                Side::Head => head.push(other.to_owned()),
                Side::Base => base.push(other.to_owned()),
                Side::None => bail!("unexpected argument {other:?} before --head/--base"),
            },
        }
    }
    if head.is_empty() || base.is_empty() {
        bail!("compare needs --head <file...> and --base <file...>");
    }
    Ok((head, base))
}

fn read_runs(paths: &[String]) -> anyhow::Result<Vec<Vec<ingest::Measurement>>> {
    let mut runs = Vec::new();
    for p in paths {
        let j = std::fs::read_to_string(p).with_context(|| format!("read {p}"))?;
        runs.push(ingest::parse_run(&j).with_context(|| format!("parse {p}"))?);
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::parse_compare_args;

    #[test]
    fn splits_head_and_base_either_order() {
        let (h, b) = parse_compare_args(&[
            "--head".into(),
            "h1.json".into(),
            "h2.json".into(),
            "--base".into(),
            "b1.json".into(),
        ])
        .unwrap();
        assert_eq!(h, ["h1.json", "h2.json"]);
        assert_eq!(b, ["b1.json"]);

        // Flag order does not matter.
        let (h, b) = parse_compare_args(&[
            "--base".into(),
            "b1.json".into(),
            "b2.json".into(),
            "--head".into(),
            "h1.json".into(),
        ])
        .unwrap();
        assert_eq!(h, ["h1.json"]);
        assert_eq!(b, ["b1.json", "b2.json"]);
    }

    #[test]
    fn errors_on_missing_side_or_leading_arg() {
        assert!(
            parse_compare_args(&["--head".into(), "h.json".into()]).is_err(),
            "no --base"
        );
        assert!(
            parse_compare_args(&["--base".into(), "b.json".into()]).is_err(),
            "no --head"
        );
        assert!(
            parse_compare_args(&[
                "stray.json".into(),
                "--head".into(),
                "h.json".into(),
                "--base".into(),
                "b.json".into(),
            ])
            .is_err(),
            "arg before any flag"
        );
    }
}
