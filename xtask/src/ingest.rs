//! Parse cargo-criterion's newline-delimited JSON (`--message-format=json`)
//! into normalized nanosecond measurements keyed by a typed `BenchId`.

use serde::Deserialize;

use crate::bench_id::BenchId;

/// One benchmark's typical estimate, normalized to nanoseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub id: BenchId,
    pub nanos: f64,
}

#[derive(Deserialize)]
struct Line {
    reason: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    typical: Option<Estimate>,
}

#[derive(Deserialize)]
struct Estimate {
    estimate: f64,
    unit: String,
}

/// Error from ingesting a cargo-criterion run.
#[derive(Debug)]
pub enum IngestError {
    Json(serde_json::Error),
    Id(crate::bench_id::BenchIdParseError),
    Unit(String),
    MissingTypical(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid JSON line: {e}"),
            Self::Id(e) => write!(f, "{e}"),
            Self::Unit(u) => write!(f, "unknown time unit {u:?}"),
            Self::MissingTypical(id) => write!(f, "benchmark {id:?} has no typical estimate"),
        }
    }
}

impl std::error::Error for IngestError {}

fn to_nanos(estimate: f64, unit: &str) -> Result<f64, IngestError> {
    let factor = match unit {
        "ps" => 1e-3,
        "ns" => 1.0,
        "us" | "µs" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        other => return Err(IngestError::Unit(other.to_owned())),
    };
    Ok(estimate * factor)
}

/// Parse a full cargo-criterion `--message-format=json` stream. Blank lines and
/// every message other than `benchmark-complete` are ignored.
pub fn parse_run(jsonl: &str) -> Result<Vec<Measurement>, IngestError> {
    let mut out = Vec::new();
    for raw in jsonl.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Line = serde_json::from_str(line).map_err(IngestError::Json)?;
        if parsed.reason != "benchmark-complete" {
            continue;
        }
        let est = parsed
            .typical
            .ok_or_else(|| IngestError::MissingTypical(parsed.id.clone()))?;
        let id = parsed.id.parse::<BenchId>().map_err(IngestError::Id)?;
        out.push(Measurement {
            id,
            nanos: to_nanos(est.estimate, &est.unit)?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_id::BenchId;
    use crate::bench_id::Cell;

    // Two benchmark-complete lines plus a group-complete line that must be
    // ignored. `typical` units differ to exercise normalization.
    const SAMPLE: &str = concat!(
        r#"{"reason":"benchmark-complete","id":"throughput/cell_a/get_hit/PackedArray/4","typical":{"estimate":1.1,"unit":"ns"}}"#,
        "\n",
        r#"{"reason":"benchmark-complete","id":"throughput/cell_a/churn/PackedArray","typical":{"estimate":7.9,"unit":"us"}}"#,
        "\n",
        r#"{"reason":"group-complete","group_name":"throughput/cell_a/get_hit","benchmarks":[]}"#,
        "\n",
    );

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "1.1 parsed from JSON then multiplied by 1.0 round-trips exactly"
    )]
    fn parses_and_normalizes_units() {
        let ms = parse_run(SAMPLE).expect("parse");
        assert_eq!(ms.len(), 2, "group-complete line ignored");
        assert_eq!(ms[0].nanos, 1.1);
        assert!((ms[1].nanos - 7_900.0).abs() < 1e-6, "us -> ns");
        assert!(matches!(ms[0].id, BenchId::Single { cell: Cell::A, .. }));
    }

    #[test]
    fn rejects_unknown_unit() {
        let bad = r#"{"reason":"benchmark-complete","id":"throughput/cell_a/build/PackedArray","typical":{"estimate":1.0,"unit":"furlongs"}}"#;
        assert!(parse_run(bad).is_err());
    }
}
