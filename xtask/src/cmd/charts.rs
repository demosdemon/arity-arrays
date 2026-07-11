//! `xtask charts`: regenerate the README comparison tables and the
//! `docs/bench` SVGs from a cargo-criterion capture.

use std::path::Path;
use std::path::PathBuf;

use clap::Args;

use crate::svg;
use crate::tables;

#[derive(Debug, Args)]
pub struct Charts {
    #[arg(value_name = "run.json")]
    pub run_path: PathBuf,
    #[arg(value_name = "baseline.json")]
    pub baseline_path: Option<PathBuf>,
}

impl Charts {
    pub fn run(&self) -> anyhow::Result<()> {
        // Parse EVERYTHING (run, and baseline if given) before touching any file,
        // so a malformed input aborts without writing partial artifacts.
        let measurements = super::read_measures(&self.run_path)?;
        let baseline = self
            .baseline_path
            .as_deref()
            .map(super::read_measures)
            .transpose()?;

        let table = tables::comparison_table(&measurements);
        let rewritten = [
            Path::new("README.md"),
            Path::new("crates/arity-arrays/README.md"),
        ]
        .into_iter()
        .map(|path: &Path| {
            let existing = crate::fs::read_to_string(path)?;
            Ok((path, tables::render_marked(&existing, &table)?))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

        let bench_dir = Path::new("docs/bench");
        let mut charts = svg::write_charts(&measurements, bench_dir)?;
        if let Some(before) = &baseline {
            charts.extend(svg::write_delta(before, &measurements, bench_dir)?);
        }
        for (path, contents) in &rewritten {
            crate::fs::write(path, contents)?;
        }
        eprintln!(
            "regenerated {} README table(s) and {} chart(s)",
            rewritten.len(),
            charts.len()
        );
        Ok(())
    }
}
