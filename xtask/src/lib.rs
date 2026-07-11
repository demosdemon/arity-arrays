#![expect(
    missing_docs,
    clippy::too_long_first_doc_paragraph,
    clippy::missing_errors_doc
)]

pub mod bench_id;
pub mod cmd;
pub mod delta;
pub mod fs;
pub mod ingest;
pub mod svg;
pub mod tables;

use clap::Parser;
use clap::Subcommand;

#[derive(Debug, Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Charts(cmd::charts::Charts),
    Compare(cmd::compare::Compare),
    #[cfg(feature = "ec2-bench")]
    #[command(name = "ec2-bench", subcommand)]
    Ec2Bench(cmd::ec2_bench::Ec2Bench),
}

impl Cli {
    pub fn main() -> anyhow::Result<()> {
        Self::parse().run()
    }

    pub fn run(&self) -> anyhow::Result<()> {
        self.command.run()
    }
}

impl Command {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            Self::Charts(cmd) => cmd.run(),
            Self::Compare(cmd) => cmd.run(),
            #[cfg(feature = "ec2-bench")]
            Self::Ec2Bench(cmd) => cmd.run(),
        }
    }
}
