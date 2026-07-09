//! Render the markdown A/B delta table published to CI job summaries and PR
//! comments (`xtask compare <run.json> <baseline.json>`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::bench_id::Cell;
use crate::charts::Paired;
use crate::charts::join_single_ops;
use crate::ingest::Measurement;

/// Build the two-cell markdown delta table: one table per cell, columns `op |
/// subject | base (ns) | head (ns) | Δ%`. A bench id present on only one side
/// renders `–` for the missing value and for `Δ%`, rather than being dropped.
#[must_use]
pub fn render_compare(before: &[Measurement], after: &[Measurement]) -> String {
    let joined = join_single_ops(before, after);
    let mut s = String::new();
    s.push_str(&cell_table("Cell A (Arity16)", Cell::A, &joined));
    s.push('\n');
    s.push_str(&cell_table("Cell B (Arity256)", Cell::B, &joined));
    s
}

fn cell_table(heading: &str, want: Cell, joined: &BTreeMap<Cell, Paired>) -> String {
    let mut s = format!("**{heading} single-op (base vs head)**\n\n");
    s.push_str("| op | subject | base (ns) | head (ns) | Δ% |\n");
    s.push_str("| :--- | :--- | ---: | ---: | ---: |\n");
    let Some(ops) = joined.get(&want) else {
        return s;
    };
    for (op, subjects) in ops {
        for (subject, (bv, av)) in subjects {
            let base_cell = bv.map_or_else(|| "–".to_owned(), |v| format!("{v:.2}"));
            let head_cell = av.map_or_else(|| "–".to_owned(), |v| format!("{v:.2}"));
            let pct_cell = match (bv, av) {
                (Some(b), Some(a)) if *b != 0.0 => format!("{:+.1}%", (a - b) / b * 100.0),
                _ => "–".to_owned(),
            };
            let _ = writeln!(
                s,
                "| `{op}` | {subject} | {base_cell} | {head_cell} | {pct_cell} |"
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_id::BenchId;

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
    fn one_sided_bench_id_renders_as_missing() {
        let before = vec![measurement(Cell::A, "get_hit", "Removed", 9.9)];
        let after = vec![measurement(Cell::A, "get_hit", "New", 3.3)];
        let table = render_compare(&before, &after);
        assert!(table.contains("| `get_hit` | New | – | 3.30 | – |"));
        assert!(table.contains("| `get_hit` | Removed | 9.90 | – | – |"));
    }
}
