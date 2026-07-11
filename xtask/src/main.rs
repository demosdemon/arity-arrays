//! Benchmark-export tooling: parse cargo-criterion JSON, regenerate the
//! comparison charts (SVG) and README tables, and render the CI A/B delta
//! table consumed by the job summary / PR comment (`xtask compare`).

fn main() -> anyhow::Result<()> {
    xtask::Cli::main()
}
