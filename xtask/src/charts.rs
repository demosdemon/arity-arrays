//! Render grouped-bar comparison SVGs from normalized measurements.

// Items are consumed by later tasks once subcommands are wired; until then the
// binary entry point does not reference this module's types.
#![expect(dead_code, reason = "consumed by later tasks that wire subcommands")]

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use plotters::prelude::*;

use crate::bench_id::BenchId;
use crate::bench_id::Cell;
use crate::ingest::Measurement;

/// Error from rendering charts.
#[derive(Debug)]
pub struct ChartError(pub String);

impl std::fmt::Display for ChartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ChartError {}

/// op -> subject -> value (ns, or % change). The renderer is value-agnostic.
type Grouped = BTreeMap<String, BTreeMap<String, f64>>;

const fn cell_slug(cell: Cell) -> &'static str {
    match cell {
        Cell::A => "cell_a",
        Cell::B => "cell_b",
    }
}

/// Horizontal span of subject `si` (of `n_sub`) within an op's unit-width slot
/// `[0.0, 1.0)`. Returns `(x0, x1)` as fractions to be offset by the integer op
/// index. Kept pure and separate so the no-zero-width-bar invariant is unit
/// tested directly, independent of SVG rendering.
#[expect(
    clippy::cast_precision_loss,
    reason = "subject indices are < 6; f64 represents them exactly"
)]
fn bar_span(_oi: usize, si: usize, n_sub: usize) -> (f64, f64) {
    let n = n_sub.max(1) as f64;
    let slot = 0.8 / n; // 0.1 padding on each side of the slot
    let x0 = 0.1 + slot * si as f64;
    (x0, x0 + slot)
}

/// Reduce single-op measurements to cell -> op -> subject -> value, keeping the
/// value at the largest occupancy seen for each (cell, op, subject).
fn group_single_ops(measurements: &[Measurement]) -> BTreeMap<Cell, Grouped> {
    let mut max_occ: BTreeMap<(Cell, String, String), usize> = BTreeMap::new();
    let mut out: BTreeMap<Cell, Grouped> = BTreeMap::new();
    for m in measurements {
        if let BenchId::Single {
            cell,
            op,
            subject,
            occupancy,
        } = &m.id
        {
            let key = (*cell, op.clone(), subject.clone());
            let seen = max_occ.entry(key).or_insert(0);
            if *occupancy >= *seen {
                *seen = *occupancy;
                out.entry(*cell)
                    .or_default()
                    .entry(op.clone())
                    .or_default()
                    .insert(subject.clone(), m.nanos);
            }
        }
    }
    out
}

/// Per-(cell, op, subject) percentage change from `before` to `after`
/// (positive = slower). Pure; unit tested.
fn delta_pct(before: &[Measurement], after: &[Measurement]) -> BTreeMap<Cell, Grouped> {
    let b = group_single_ops(before);
    let a = group_single_ops(after);
    let mut out: BTreeMap<Cell, Grouped> = BTreeMap::new();
    for (cell, ops) in &a {
        for (op, subjects) in ops {
            for (subject, av) in subjects {
                let Some(bv) = b
                    .get(cell)
                    .and_then(|o| o.get(op))
                    .and_then(|m| m.get(subject))
                else {
                    continue;
                };
                if *bv != 0.0 {
                    out.entry(*cell)
                        .or_default()
                        .entry(op.clone())
                        .or_default()
                        .insert(subject.clone(), (av - bv) / bv * 100.0);
                }
            }
        }
    }
    out
}

/// Write one absolute-ns grouped-bar SVG per cell.
///
/// # Errors
/// Returns [`ChartError`] if the output directory cannot be created or an SVG
/// cannot be rendered/written.
pub fn write_charts(
    measurements: &[Measurement],
    out_dir: &Path,
) -> Result<Vec<PathBuf>, ChartError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| ChartError(format!("create {}: {e}", out_dir.display())))?;
    let mut written = Vec::new();
    for (cell, ops) in &group_single_ops(measurements) {
        let path = out_dir.join(format!("{}-single-op.svg", cell_slug(*cell)));
        render_grouped(
            &path,
            &format!("{} single-op (ns, lower is better)", cell_slug(*cell)),
            "ns",
            ops,
        )?;
        written.push(path);
    }
    Ok(written)
}

/// Write one run-vs-baseline delta SVG per cell (% change, after vs before).
///
/// # Errors
/// Returns [`ChartError`] if the output directory cannot be created or an SVG
/// cannot be rendered/written.
pub fn write_delta(
    before: &[Measurement],
    after: &[Measurement],
    out_dir: &Path,
) -> Result<Vec<PathBuf>, ChartError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| ChartError(format!("create {}: {e}", out_dir.display())))?;
    let mut written = Vec::new();
    for (cell, ops) in &delta_pct(before, after) {
        let path = out_dir.join(format!("{}-delta.svg", cell_slug(*cell)));
        render_grouped(
            &path,
            &format!(
                "{} single-op Δ vs baseline (%, lower is better)",
                cell_slug(*cell)
            ),
            "% change",
            ops,
        )?;
        written.push(path);
    }
    Ok(written)
}

/// Render a grouped-bar SVG: x = op, bars = subjects, y = value. The y-domain
/// spans `0` so positive and negative (delta) values both read against a zero
/// baseline.
#[expect(
    clippy::cast_precision_loss,
    reason = "op count is tiny (< 8); f64 represents the axis bound exactly"
)]
#[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "x is a plotters tick value in [0, n_ops); never negative or fractional at a label tick"
)]
fn render_grouped(
    path: &Path,
    caption: &str,
    y_desc: &str,
    ops: &Grouped,
) -> Result<(), ChartError> {
    let subjects: BTreeSet<String> = ops.values().flat_map(|s| s.keys().cloned()).collect();
    let subjects: Vec<String> = subjects.into_iter().collect();
    let op_names: Vec<String> = ops.keys().cloned().collect();
    let n_ops = op_names.len().max(1);

    let values: Vec<f64> = ops.values().flat_map(|s| s.values().copied()).collect();
    let max = values.iter().copied().fold(f64::MIN, f64::max);
    let min = values.iter().copied().fold(f64::MAX, f64::min);
    let (min, max) = if values.is_empty() {
        (0.0, 1.0)
    } else {
        (min, max)
    };
    let lo = min.min(0.0);
    let hi = max.max(0.0);
    let pad = ((hi - lo) * 0.1).max(1.0);
    let y_range = (lo - if lo < 0.0 { pad } else { 0.0 })..(hi + pad);

    let root = SVGBackend::new(path, (900, 480)).into_drawing_area();
    root.fill(&WHITE).map_err(|e| ChartError(e.to_string()))?;

    // Float x-domain so manually-positioned grouped bars keep sub-integer
    // widths. (An integer domain truncates neighbouring bars to width 0.)
    let mut chart = ChartBuilder::on(&root)
        .caption(caption, ("sans-serif", 20))
        .margin(16)
        .x_label_area_size(48)
        .y_label_area_size(56)
        .build_cartesian_2d(0.0_f64..(n_ops as f64), y_range)
        .map_err(|e| ChartError(e.to_string()))?;

    chart
        .configure_mesh()
        .y_desc(y_desc)
        .x_labels(n_ops)
        .x_label_formatter(&|x| op_names.get(*x as usize).cloned().unwrap_or_default())
        .draw()
        .map_err(|e| ChartError(e.to_string()))?;

    let n_sub = subjects.len().max(1);
    for (si, subject) in subjects.iter().enumerate() {
        let color = Palette99::pick(si).to_rgba();
        chart
            .draw_series(op_names.iter().enumerate().filter_map(|(oi, op)| {
                let value = *ops.get(op)?.get(subject)?;
                let (f0, f1) = bar_span(oi, si, n_sub);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "op index is tiny; f64 represents it exactly"
                )]
                let base = oi as f64;
                Some(Rectangle::new(
                    [(base + f0, 0.0), (base + f1, value)],
                    color.filled(),
                ))
            }))
            .map_err(|e| ChartError(e.to_string()))?
            .label(subject.clone())
            .legend(move |(x, y)| Rectangle::new([(x, y - 5), (x + 10, y + 5)], color.filled()));
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()
        .map_err(|e| ChartError(e.to_string()))?;
    root.present().map_err(|e| ChartError(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_id::BenchId;
    use crate::bench_id::Cell;

    fn sample() -> Vec<Measurement> {
        vec![
            Measurement {
                id: BenchId::Single {
                    cell: Cell::A,
                    op: "get_hit".to_owned(),
                    subject: "PackedArray".to_owned(),
                    occupancy: 16,
                },
                nanos: 1.1,
            },
            Measurement {
                id: BenchId::Single {
                    cell: Cell::A,
                    op: "get_hit".to_owned(),
                    subject: "FixedArray".to_owned(),
                    occupancy: 16,
                },
                nanos: 0.7,
            },
        ]
    }

    // Directly guards the zero-width-bar defect: with an integer x-axis and
    // `as usize`-truncated offsets, neighbouring bars collapse to width 0. The
    // float span must stay strictly positive for any realistic subject count.
    #[test]
    fn bar_spans_are_positive_for_all_widths() {
        for n_sub in 1..=6 {
            for si in 0..n_sub {
                let (x0, x1) = bar_span(0, si, n_sub);
                assert!(x1 > x0, "n_sub={n_sub} si={si}: zero/negative width");
                assert!(
                    x0 >= 0.0 && x1 <= 1.0,
                    "bars must stay within their op slot"
                );
            }
        }
    }

    #[test]
    fn writes_valid_svg() {
        let dir = std::env::temp_dir().join("xtask-charts-test");
        std::fs::create_dir_all(&dir).unwrap();
        let written = write_charts(&sample(), &dir).expect("charts");
        assert!(!written.is_empty(), "at least one chart written");
        let svg = std::fs::read_to_string(&written[0]).unwrap();
        assert!(svg.contains("<svg"), "output is an SVG document");
        assert!(svg.contains("PackedArray"), "subject label present");
    }

    #[test]
    fn delta_pct_computes_signed_change() {
        let before = sample(); // PackedArray get_hit @16 = 1.1
        let after = vec![Measurement {
            id: BenchId::Single {
                cell: Cell::A,
                op: "get_hit".to_owned(),
                subject: "PackedArray".to_owned(),
                occupancy: 16,
            },
            nanos: 2.2, // doubled => +100%
        }];
        let d = delta_pct(&before, &after);
        let pct = d[&Cell::A]["get_hit"]["PackedArray"];
        assert!((pct - 100.0).abs() < 1e-6, "expected +100%, got {pct}");
    }
}
