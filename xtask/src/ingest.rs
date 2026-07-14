//! Parse cargo-criterion's newline-delimited JSON (`--message-format=json`)
//! into normalized nanosecond measurements keyed by a typed `BenchId`.

use serde::Deserialize;

use crate::bench_id::BenchId;

/// One benchmark's median estimate with its confidence interval, all
/// normalized to nanoseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub id: BenchId,
    /// Median point estimate (ns).
    pub nanos: f64,
    /// Lower confidence bound of the median (ns).
    pub lo_nanos: f64,
    /// Upper confidence bound of the median (ns).
    pub hi_nanos: f64,
}

impl Measurement {
    /// A point measurement with a zero-width confidence interval (bounds equal
    /// the point). For tests and any consumer that needs only the point.
    // `#[must_use]` + `const` are mandatory, not optional: the workspace
    // promotes clippy pedantic/nursery to warnings and `just ci` runs
    // `clippy --all-targets -D warnings`, so a plain `pub fn` here trips
    // `must_use_candidate` and `missing_const_for_fn` and fails the gate.
    // (Moving the `String`-bearing `BenchId` into the return is valid in a
    // `const fn` — nothing is dropped in the body.)
    #[cfg(test)]
    #[must_use]
    pub const fn point(id: BenchId, nanos: f64) -> Self {
        Self {
            id,
            nanos,
            lo_nanos: nanos,
            hi_nanos: nanos,
        }
    }
}

#[derive(Deserialize)]
struct Line {
    reason: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    median: Option<Estimate>,
}

#[derive(Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "the `estimate` field mirrors cargo-criterion's own JSON key; renaming it would decouple the struct from the wire format it deserializes"
)]
struct Estimate {
    estimate: f64,
    lower_bound: f64,
    upper_bound: f64,
    unit: String,
}

/// Error from ingesting a cargo-criterion run.
#[derive(Debug)]
pub enum IngestError {
    Json(serde_json::Error),
    Id(crate::bench_id::BenchIdParseError),
    Unit(String),
    MissingMedian(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid JSON line: {e}"),
            Self::Id(e) => write!(f, "{e}"),
            Self::Unit(u) => write!(f, "unknown time unit {u:?}"),
            Self::MissingMedian(id) => write!(f, "benchmark {id:?} has no median estimate"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Id(e) => Some(e),
            Self::Unit(_) | Self::MissingMedian(_) => None,
        }
    }
}

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
            .median
            .ok_or_else(|| IngestError::MissingMedian(parsed.id.clone()))?;
        let id = parsed.id.parse::<BenchId>().map_err(IngestError::Id)?;
        out.push(Measurement {
            id,
            nanos: to_nanos(est.estimate, &est.unit)?,
            lo_nanos: to_nanos(est.lower_bound, &est.unit)?,
            hi_nanos: to_nanos(est.upper_bound, &est.unit)?,
        });
    }
    Ok(out)
}

/// Average N per-side captures (the interleaved A/B/B/A replicates) into one
/// measurement set: the point estimate is the arithmetic mean of the captures'
/// points; the confidence interval is their envelope (min lower bound, max
/// upper bound) — a deliberately conservative combined-uncertainty band. A
/// bench id present in only some captures is averaged over the captures that
/// carry it. Averaging a single capture is the identity. Keyed by the bench
/// id's canonical string form (parse/`Display` round-trip uniquely).
#[must_use]
pub fn average_runs(runs: &[Vec<Measurement>]) -> Vec<Measurement> {
    use std::collections::BTreeMap;
    // key -> (sum_point, count, min_lo, max_hi, id)
    let mut acc: BTreeMap<String, (f64, u32, f64, f64, BenchId)> = BTreeMap::new();
    for run in runs {
        for m in run {
            let e = acc
                .entry(m.id.to_string())
                .or_insert_with(|| (0.0, 0, f64::INFINITY, f64::NEG_INFINITY, m.id.clone()));
            e.0 += m.nanos;
            e.1 += 1;
            e.2 = e.2.min(m.lo_nanos);
            e.3 = e.3.max(m.hi_nanos);
        }
    }
    acc.into_values()
        .map(|(sum, n, lo, hi, id)| Measurement {
            id,
            nanos: sum / f64::from(n),
            lo_nanos: lo,
            hi_nanos: hi,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_id::BenchId;
    use crate::bench_id::Cell;

    // Two benchmark-complete lines plus a group-complete line that must be
    // ignored. `median` units differ to exercise normalization.
    const SAMPLE: &str = concat!(
        r#"{"reason":"benchmark-complete","id":"throughput/cell_a/get_hit/PackedArray/4","median":{"estimate":1.1,"lower_bound":1.0,"upper_bound":1.2,"unit":"ns"}}"#,
        "\n",
        r#"{"reason":"benchmark-complete","id":"throughput/cell_a/churn/PackedArray","median":{"estimate":7.9,"lower_bound":7.5,"upper_bound":8.3,"unit":"us"}}"#,
        "\n",
        r#"{"reason":"group-complete","group_name":"throughput/cell_a/get_hit","benchmarks":[]}"#,
        "\n",
    );

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "values parsed from JSON then multiplied by 1.0 round-trip exactly"
    )]
    fn parses_median_and_bounds() {
        let ms = parse_run(SAMPLE).expect("parse");
        assert_eq!(ms.len(), 2, "group-complete line ignored");
        assert_eq!(ms[0].nanos, 1.1);
        assert_eq!(ms[0].lo_nanos, 1.0);
        assert_eq!(ms[0].hi_nanos, 1.2);
        assert!((ms[1].nanos - 7_900.0).abs() < 1e-6, "us -> ns point");
        assert!((ms[1].lo_nanos - 7_500.0).abs() < 1e-6, "us -> ns lower");
        assert!((ms[1].hi_nanos - 8_300.0).abs() < 1e-6, "us -> ns upper");
        assert!(matches!(ms[0].id, BenchId::Single { cell: Cell::A, .. }));
    }

    #[test]
    fn rejects_unknown_unit() {
        let bad = r#"{"reason":"benchmark-complete","id":"throughput/cell_a/build/PackedArray","median":{"estimate":1.0,"lower_bound":1.0,"upper_bound":1.0,"unit":"furlongs"}}"#;
        assert!(parse_run(bad).is_err());
    }

    #[test]
    fn errors_when_median_missing() {
        // A benchmark-complete line with no `median` object is a hard error, not
        // a silently dropped measurement.
        let no_median = r#"{"reason":"benchmark-complete","id":"throughput/cell_a/build/PackedArray","typical":{"estimate":1.0,"lower_bound":1.0,"upper_bound":1.0,"unit":"ns"}}"#;
        assert!(matches!(
            parse_run(no_median),
            Err(IngestError::MissingMedian(_))
        ));
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "small integers averaged/enveloped are exact in f64"
    )]
    fn average_runs_means_points_and_envelopes_bounds() {
        let id = BenchId::Single {
            cell: Cell::A,
            op: "get_hit".to_owned(),
            subject: "PackedArray".to_owned(),
            occupancy: 16,
        };
        let run_a = vec![Measurement {
            id: id.clone(),
            nanos: 2.0,
            lo_nanos: 1.5,
            hi_nanos: 2.5,
        }];
        let run_b = vec![Measurement {
            id: id.clone(),
            nanos: 4.0,
            lo_nanos: 3.0,
            hi_nanos: 6.0,
        }];
        let avg = average_runs(&[run_a, run_b]);
        assert_eq!(avg.len(), 1);
        assert_eq!(avg[0].nanos, 3.0, "mean of 2 and 4");
        assert_eq!(avg[0].lo_nanos, 1.5, "envelope min lower");
        assert_eq!(avg[0].hi_nanos, 6.0, "envelope max upper");

        // An id present in only one of several captures is averaged over the
        // captures that carry it — not divided by the total capture count.
        let only_in_y = BenchId::Workload {
            cell: Cell::A,
            op: "build".to_owned(),
            subject: "PackedArray".to_owned(),
        };
        let run_x = vec![Measurement {
            id: id.clone(),
            nanos: 2.0,
            lo_nanos: 2.0,
            hi_nanos: 2.0,
        }];
        let run_y = vec![
            Measurement {
                id: id.clone(),
                nanos: 4.0,
                lo_nanos: 4.0,
                hi_nanos: 4.0,
            },
            Measurement {
                id: only_in_y.clone(),
                nanos: 9.0,
                lo_nanos: 9.0,
                hi_nanos: 9.0,
            },
        ];
        let mixed = average_runs(&[run_x, run_y]);
        let build = mixed
            .iter()
            .find(|m| m.id == only_in_y)
            .expect("partial id present");
        assert_eq!(
            build.nanos, 9.0,
            "single-capture id keeps its value, not /2"
        );

        // Averaging one capture is the identity.
        let single = vec![Measurement {
            id,
            nanos: 5.0,
            lo_nanos: 4.0,
            hi_nanos: 7.0,
        }];
        let id_avg = average_runs(std::slice::from_ref(&single));
        assert_eq!(id_avg, single);
    }

    #[test]
    fn source_exposes_the_wrapped_cause() {
        use std::error::Error;

        let json_err = parse_run("{ not json").expect_err("invalid JSON is an error");
        assert!(
            json_err.source().is_some(),
            "Json variant exposes its cause"
        );

        let unit_err = to_nanos(1.0, "furlongs").expect_err("unknown unit is an error");
        assert!(unit_err.source().is_none(), "Unit variant has no cause");
    }
}
