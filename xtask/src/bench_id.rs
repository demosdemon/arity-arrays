//! Typed model of a criterion benchmark id path, the stable contract between
//! the `arity-arrays` benches and this tool. Parsing is total: an unrecognized
//! path is an error, never a silently mis-bucketed measurement.

use core::fmt;
use core::str::FromStr;

use anyhow::Context;
use anyhow::bail;

/// Which payload cell a throughput bench belongs to. `Ord` so it can key the
/// `BTreeMap`s the chart renderer builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cell {
    A,
    B,
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::A => "cell_a",
            Self::B => "cell_b",
        })
    }
}

impl FromStr for Cell {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cell_a" => Ok(Self::A),
            "cell_b" => Ok(Self::B),
            other => bail!("unknown cell {other:?}"),
        }
    }
}

/// Which arity a trie bench belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrieArity {
    A16,
    A256,
}

impl fmt::Display for TrieArity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::A16 => "arity16",
            Self::A256 => "arity256",
        })
    }
}

impl FromStr for TrieArity {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "arity16" => Ok(Self::A16),
            "arity256" => Ok(Self::A256),
            other => bail!("unknown arity {other:?}"),
        }
    }
}

/// A parsed criterion benchmark id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchId {
    /// `throughput/<cell>/<op>/<subject>/<occupancy>`
    Single {
        cell: Cell,
        op: String,
        subject: String,
        occupancy: usize,
    },
    /// `throughput/<cell>/<op>/<subject>` (no occupancy)
    Workload {
        cell: Cell,
        op: String,
        subject: String,
    },
    /// `throughput/convert/<op>/<cell>/<occupancy>`
    Convert {
        op: String,
        cell: Cell,
        occupancy: usize,
    },
    /// `trie/<arity>/<op>/<store>/<shape>`
    Trie {
        arity: TrieArity,
        op: String,
        store: String,
        shape: String,
    },
}

/// Throughput ops that carry no occupancy segment.
const WORKLOAD_OPS: &[&str] = &["build", "churn"];

fn occ(s: &str) -> anyhow::Result<usize> {
    s.parse::<usize>()
        .with_context(|| format!("occupancy {s:?} is not a number"))
}

/// The body of [`BenchId::from_str`], split out so the id under parse is
/// attached as context exactly once, around every way the parse can fail.
fn parse_path(s: &str) -> anyhow::Result<BenchId> {
    let parts: Vec<&str> = s.split('/').collect();
    match parts.as_slice() {
        ["trie", arity, op, store, shape] => Ok(BenchId::Trie {
            arity: arity.parse()?,
            op: (*op).to_owned(),
            store: (*store).to_owned(),
            shape: (*shape).to_owned(),
        }),
        ["throughput", "convert", op, cell, occupancy] => Ok(BenchId::Convert {
            op: (*op).to_owned(),
            cell: cell.parse()?,
            occupancy: occ(occupancy)?,
        }),
        ["throughput", cell, op, subject] if WORKLOAD_OPS.contains(op) => Ok(BenchId::Workload {
            cell: cell.parse()?,
            op: (*op).to_owned(),
            subject: (*subject).to_owned(),
        }),
        ["throughput", cell, op, subject, occupancy] if !WORKLOAD_OPS.contains(op) => {
            Ok(BenchId::Single {
                cell: cell.parse()?,
                op: (*op).to_owned(),
                subject: (*subject).to_owned(),
                occupancy: occ(occupancy)?,
            })
        }
        _ => bail!("unrecognized id path"),
    }
}

impl FromStr for BenchId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_path(s).with_context(|| format!("malformed benchmark id {s:?}"))
    }
}

impl fmt::Display for BenchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single {
                cell,
                op,
                subject,
                occupancy,
            } => write!(f, "throughput/{cell}/{op}/{subject}/{occupancy}"),
            Self::Workload { cell, op, subject } => {
                write!(f, "throughput/{cell}/{op}/{subject}")
            }
            Self::Convert {
                op,
                cell,
                occupancy,
            } => write!(f, "throughput/convert/{op}/{cell}/{occupancy}"),
            Self::Trie {
                arity,
                op,
                store,
                shape,
            } => write!(f, "trie/{arity}/{op}/{store}/{shape}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive corpus: one id per shape the benches actually emit.
    const SAMPLES: &[&str] = &[
        "throughput/cell_a/get_hit/PackedArray/4",
        "throughput/cell_b/iter_present/HashMap/256",
        "throughput/cell_a/insert_new/GappedArray/8",
        "throughput/cell_a/build/PackedArray",
        "throughput/cell_b/churn/BTreeMap",
        "throughput/convert/pack/cell_a/8",
        "throughput/convert/unpack/cell_b/128",
        "trie/arity16/clone/PackedStore/Bushy",
        "trie/arity256/drop/FixedStore/Realistic",
    ];

    #[test]
    fn parse_display_roundtrips() {
        for &s in SAMPLES {
            let parsed: BenchId = s.parse().unwrap_or_else(|e| panic!("parse {s}: {e}"));
            assert_eq!(parsed.to_string(), s, "roundtrip mismatch for {s}");
        }
    }

    #[test]
    fn rejects_malformed_paths() {
        for bad in [
            "",
            "throughput",
            "throughput/cell_a/get_hit/PackedArray", // single-op needs occupancy
            "throughput/cell_a/get_hit/PackedArray/notnum",
            "trie/arity16/clone/PackedStore", // trie needs shape
            "unknown/cell_a/get_hit/PackedArray/4",
        ] {
            assert!(bad.parse::<BenchId>().is_err(), "should reject {bad:?}");
        }
    }
}
