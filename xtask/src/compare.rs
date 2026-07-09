//! Render the markdown A/B delta table published to CI job summaries and PR
//! comments (`xtask compare --head <run.json>... --base <baseline.json>...`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::bench_id::BenchId;
use crate::bench_id::Cell;
use crate::ingest::Measurement;

/// A median point estimate with its confidence interval, in ns.
#[derive(Clone, Copy)]
struct Interval {
    point: f64,
    lo: f64,
    hi: f64,
}

/// Two intervals overlap iff neither lies entirely above the other. Overlap
/// means the change is not distinguishable from run-to-run noise.
fn overlaps(a: Interval, b: Interval) -> bool {
    a.lo <= b.hi && b.lo <= a.hi
}

/// cell -> op -> subject -> Interval, keeping the largest occupancy seen for
/// each (cell, op, subject). Single-op family only (workload/convert added in a
/// later task).
type Grouped = BTreeMap<Cell, BTreeMap<String, BTreeMap<String, (usize, Interval)>>>;

fn group_single(ms: &[Measurement]) -> Grouped {
    let mut out: Grouped = BTreeMap::new();
    for m in ms {
        if let BenchId::Single {
            cell,
            op,
            subject,
            occupancy,
        } = &m.id
        {
            let entry = out
                .entry(*cell)
                .or_default()
                .entry(op.clone())
                .or_default()
                .entry(subject.clone())
                .or_insert((0, Interval {
                    point: 0.0,
                    lo: 0.0,
                    hi: 0.0,
                }));
            if *occupancy >= entry.0 {
                *entry = (*occupancy, Interval {
                    point: m.nanos,
                    lo: m.lo_nanos,
                    hi: m.hi_nanos,
                });
            }
        }
    }
    out
}

const fn cell_heading(cell: Cell) -> &'static str {
    match cell {
        Cell::A => "Cell A (Arity16)",
        Cell::B => "Cell B (Arity256)",
    }
}

/// Build the A/B delta table: one single-op table per cell, columns `op |
/// subject | base | head | Δ% | ` with a trailing `~` marker when the base/head
/// intervals overlap (within noise), then a non-failing summary line.
#[must_use]
pub fn render_compare(before: &[Measurement], after: &[Measurement]) -> String {
    let base = group_single(before);
    let head = group_single(after);
    let mut cells: BTreeSet<Cell> = BTreeSet::new();
    cells.extend(base.keys().copied());
    cells.extend(head.keys().copied());

    let mut s = String::new();
    let mut total = 0u32;
    let mut noisy = 0u32;
    for cell in cells {
        let (t, n, body) = cell_table(cell_heading(cell), cell, &base, &head);
        s.push_str(&body);
        s.push('\n');
        total += t;
        noisy += n;
    }
    let real = total - noisy;
    let _ = writeln!(
        s,
        "_{real}/{total} deltas exceed run-to-run noise (non-overlapping confidence intervals); `~` marks deltas within noise._"
    );
    s
}

/// Render one cell's single-op delta table. Returns `(delta_count,
/// noisy_count, markdown)`.
fn cell_table(heading: &str, want: Cell, base: &Grouped, head: &Grouped) -> (u32, u32, String) {
    let mut s = format!("**{heading} single-op (base vs head, median ns)**\n\n");
    s.push_str("| op | subject | base | head | Δ% | |\n");
    s.push_str("| :--- | :--- | ---: | ---: | ---: | :-- |\n");

    // Union of (op, subject) keys across both sides so a one-sided id is not
    // dropped.
    let empty = BTreeMap::new();
    let b_ops = base.get(&want).unwrap_or(&empty);
    let h_ops = head.get(&want).unwrap_or(&empty);
    let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
    for (op, subs) in b_ops.iter().chain(h_ops.iter()) {
        for subject in subs.keys() {
            keys.insert((op.clone(), subject.clone()));
        }
    }

    let mut total = 0u32;
    let mut noisy = 0u32;
    for (op, subject) in keys {
        let b = b_ops
            .get(&op)
            .and_then(|s| s.get(&subject))
            .map(|(_, iv)| *iv);
        let h = h_ops
            .get(&op)
            .and_then(|s| s.get(&subject))
            .map(|(_, iv)| *iv);
        let base_cell = b.map_or_else(|| "–".to_owned(), |iv| format!("{:.2}", iv.point));
        let head_cell = h.map_or_else(|| "–".to_owned(), |iv| format!("{:.2}", iv.point));
        let (pct, mark) = match (b, h) {
            (Some(bi), Some(hi)) if bi.point != 0.0 => {
                total += 1;
                let d = (hi.point - bi.point) / bi.point * 100.0;
                if overlaps(bi, hi) {
                    noisy += 1;
                    (format!("{d:+.1}%"), "~")
                } else {
                    (format!("{d:+.1}%"), "")
                }
            }
            _ => ("–".to_owned(), ""),
        };
        let _ = writeln!(
            s,
            "| `{op}` | {subject} | {base_cell} | {head_cell} | {pct} | {mark} |"
        );
    }
    (total, noisy, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_id::BenchId;

    // Zero-width interval helper: bounds equal the point (deltas are always
    // "real" unless the test uses `measurement_iv`).
    fn measurement(cell: Cell, op: &str, subject: &str, nanos: f64) -> Measurement {
        Measurement::point(
            BenchId::Single {
                cell,
                op: op.to_owned(),
                subject: subject.to_owned(),
                occupancy: 16,
            },
            nanos,
        )
    }

    // Explicit interval helper for the overlap/noise tests.
    fn measurement_iv(
        cell: Cell,
        op: &str,
        subject: &str,
        nanos: f64,
        lo: f64,
        hi: f64,
    ) -> Measurement {
        Measurement {
            id: BenchId::Single {
                cell,
                op: op.to_owned(),
                subject: subject.to_owned(),
                occupancy: 16,
            },
            nanos,
            lo_nanos: lo,
            hi_nanos: hi,
        }
    }

    #[test]
    fn renders_both_cells_with_deltas() {
        let before = vec![
            measurement(Cell::A, "get_hit", "PackedArray", 1.0),
            measurement(Cell::B, "get_hit", "PackedArray", 2.0),
        ];
        let after = vec![
            measurement(Cell::A, "get_hit", "PackedArray", 1.5),
            measurement(Cell::B, "get_hit", "PackedArray", 1.0),
        ];
        let table = render_compare(&before, &after);
        assert!(table.contains("Cell A"), "cell A heading present");
        assert!(table.contains("Cell B"), "cell B heading present");
        assert!(table.contains("+50.0%"), "cell A regression shown");
        assert!(table.contains("-50.0%"), "cell B improvement shown");
    }

    #[test]
    fn overlapping_intervals_are_marked_noise() {
        // base 10 [8,12], head 11 [9,13]: intervals overlap -> within noise.
        let before = vec![measurement_iv(
            Cell::A,
            "remove",
            "PackedArray",
            10.0,
            8.0,
            12.0,
        )];
        let after = vec![measurement_iv(
            Cell::A,
            "remove",
            "PackedArray",
            11.0,
            9.0,
            13.0,
        )];
        let table = render_compare(&before, &after);
        assert!(table.contains("+10.0%"), "delta still shown");
        assert!(table.contains("| ~ |"), "overlap marked with ~");
        assert!(
            table.contains("0/1 deltas exceed"),
            "noise summary counts it out"
        );
    }

    #[test]
    fn disjoint_intervals_are_not_noise() {
        // base 10 [9.5,10.5], head 20 [19,21]: disjoint -> a real change.
        let before = vec![measurement_iv(
            Cell::A,
            "remove",
            "PackedArray",
            10.0,
            9.5,
            10.5,
        )];
        let after = vec![measurement_iv(
            Cell::A,
            "remove",
            "PackedArray",
            20.0,
            19.0,
            21.0,
        )];
        let table = render_compare(&before, &after);
        assert!(table.contains("+100.0%"));
        assert!(table.contains("1/1 deltas exceed"), "counted as real");
    }

    #[test]
    fn one_sided_bench_id_renders_as_missing() {
        let before = vec![measurement(Cell::A, "get_hit", "Removed", 9.9)];
        let after = vec![measurement(Cell::A, "get_hit", "New", 3.3)];
        let table = render_compare(&before, &after);
        assert!(table.contains("| `get_hit` | New | – | 3.30 | – |  |"));
        assert!(table.contains("| `get_hit` | Removed | 9.90 | – | – |  |"));
    }
}
