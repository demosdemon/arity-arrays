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

const USAGE: &str = "usage: xtask charts --head <run.json>... [--base <baseline.json>...]\n       xtask compare --head <run.json>... --base <baseline.json>...";

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

/// `xtask charts --head <run.json>... [--base <baseline.json>...]`: regenerate
/// the README tables and `docs/bench` SVGs from the run, plus run-vs-baseline
/// delta charts when a baseline is given.
///
/// Both sides take multiple captures, averaged the same way [`run_compare`]
/// averages them (point = mean, interval = envelope). That keeps a documented
/// table and the delta table that accompanies it derived from the same
/// replicates: publishing the mean of an interleaved run while comparing
/// against its average would otherwise disagree by exactly the noise the
/// replicates exist to cancel.
fn run_charts(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (head_paths, base_paths) = parse_sides(args)?;
    if head_paths.is_empty() {
        return Err("charts needs --head <file...>".into());
    }
    // Parse EVERYTHING (run, and baseline if given) before touching any file,
    // so a malformed input aborts without writing partial artifacts.
    let measurements = ingest::average_runs(&read_runs(&head_paths)?);
    let baseline = if base_paths.is_empty() {
        None
    } else {
        Some(ingest::average_runs(&read_runs(&base_paths)?))
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

/// `xtask compare --head <run.json>... --base <baseline.json>...`: average the
/// captures on each side (interleaved A/B/B/A replicates) and print the
/// markdown A/B delta table to stdout.
fn run_compare(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (head_paths, base_paths) = parse_sides(args)?;
    if head_paths.is_empty() || base_paths.is_empty() {
        return Err("compare needs --head <file...> and --base <file...>".into());
    }
    let head = ingest::average_runs(&read_runs(&head_paths)?);
    let base = ingest::average_runs(&read_runs(&base_paths)?);
    print!("{}", compare::render_compare(&base, &head));
    Ok(())
}

/// Split `--head <f>... --base <f>...` (either flag order) into two file
/// groups. Neither side is required here — each subcommand states its own
/// requirement, since `compare` needs both sides but `charts` needs only
/// `--head`. A path before any flag is still an error.
fn parse_sides(args: &[String]) -> Result<(Vec<String>, Vec<String>), String> {
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
                Side::None => {
                    return Err(format!(
                        "unexpected argument {other:?} before --head/--base"
                    ));
                }
            },
        }
    }
    Ok((head, base))
}

fn read_runs(
    paths: &[String],
) -> Result<Vec<Vec<ingest::Measurement>>, Box<dyn std::error::Error>> {
    let mut runs = Vec::new();
    for p in paths {
        let j = std::fs::read_to_string(p).map_err(|e| format!("read {p}: {e}"))?;
        runs.push(ingest::parse_run(&j)?);
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::parse_sides;

    #[test]
    fn splits_head_and_base_either_order() {
        let (h, b) = parse_sides(&[
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
        let (h, b) = parse_sides(&[
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
    fn a_missing_side_parses_empty_for_the_subcommand_to_reject() {
        // `charts` accepts a bare `--head`, so the split itself must not
        // reject one. `run_charts`/`run_compare` enforce their own arity.
        let (h, b) = parse_sides(&["--head".into(), "h.json".into()]).unwrap();
        assert_eq!(h, ["h.json"]);
        assert!(b.is_empty(), "absent --base yields an empty base side");

        let (h, b) = parse_sides(&["--base".into(), "b.json".into()]).unwrap();
        assert!(h.is_empty(), "absent --head yields an empty head side");
        assert_eq!(b, ["b.json"]);
    }

    #[test]
    fn errors_on_leading_arg_before_any_flag() {
        assert!(
            parse_sides(&[
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
