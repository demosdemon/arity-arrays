//! `xtask compare`: render the A/B delta table CI posts to the job summary.

use std::path::PathBuf;

use clap::Args;

use crate::delta;
use crate::ingest;

#[derive(Debug, Args)]
pub struct Compare {
    #[arg(long, value_name = "run.json", num_args = 1.., required = true)]
    pub head_paths: Vec<PathBuf>,
    #[arg(long, value_name = "baseline.json", num_args = 1.., required = true)]
    pub base_paths: Vec<PathBuf>,
}

impl Compare {
    pub fn run(&self) -> anyhow::Result<()> {
        let head = ingest::average_runs(&super::read_runs(&self.head_paths)?);
        let base = ingest::average_runs(&super::read_runs(&self.base_paths)?);
        print!("{}", delta::render_compare(&base, &head));
        Ok(())
    }
}
