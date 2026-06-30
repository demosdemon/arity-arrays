//! Benchmark-export tooling: parse cargo-criterion JSON and regenerate the
//! comparison charts (SVG) and README tables.

// Tests assert on Result values directly; `unwrap` keeps them terse. This is
// the sanctioned test-wide suppression (crate root, cfg(test)-gated).
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub(crate) mod bench_id;
mod ingest;
mod tables;

fn main() {
    // Subcommands are wired in later tasks.
    eprintln!("xtask: no subcommand wired yet");
    std::process::exit(2);
}
