//! Render the markdown A/B delta table published to CI job summaries and PR
//! comments (`xtask compare --head <run.json>... --base <baseline.json>...`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write;

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

/// Per-cell workload grouping: cell -> op -> subject -> Interval (no
/// occupancy).
type GroupedW = BTreeMap<Cell, BTreeMap<String, BTreeMap<String, Interval>>>;

/// cell -> op -> subject -> Interval for the workload family (no occupancy).
fn group_workload(ms: &[Measurement]) -> GroupedW {
    let mut out: GroupedW = BTreeMap::new();
    for m in ms {
        if let BenchId::Workload { cell, op, subject } = &m.id {
            out.entry(*cell)
                .or_default()
                .entry(op.clone())
                .or_default()
                .insert(subject.clone(), Interval {
                    point: m.nanos,
                    lo: m.lo_nanos,
                    hi: m.hi_nanos,
                });
        }
    }
    out
}

/// op -> `cell_slug` -> Interval for the convert family, at max occupancy.
fn group_convert(ms: &[Measurement]) -> BTreeMap<String, BTreeMap<String, (usize, Interval)>> {
    let mut out: BTreeMap<String, BTreeMap<String, (usize, Interval)>> = BTreeMap::new();
    for m in ms {
        if let BenchId::Convert {
            op,
            cell,
            occupancy,
        } = &m.id
        {
            let slug = match cell {
                Cell::A => "cell_a",
                Cell::B => "cell_b",
            };
            let entry = out
                .entry(op.clone())
                .or_default()
                .entry(slug.to_owned())
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

/// `(arity, op)` -> (store, shape) -> Interval for the trie family (no
/// occupancy).
type GroupedT = BTreeMap<(String, String), BTreeMap<(String, String), Interval>>;

/// Group trie measurements by `(arity, op)`, then by `(store, shape)`.
fn group_trie(ms: &[Measurement]) -> GroupedT {
    let mut out: GroupedT = BTreeMap::new();
    for m in ms {
        if let BenchId::Trie {
            arity,
            op,
            store,
            shape,
        } = &m.id
        {
            out.entry((arity.to_string(), op.clone()))
                .or_default()
                .insert((store.clone(), shape.clone()), Interval {
                    point: m.nanos,
                    lo: m.lo_nanos,
                    hi: m.hi_nanos,
                });
        }
    }
    out
}

/// Render the trie delta tables — one per `(arity, op)`, rows = store × shape —
/// reusing `delta_row`. Returns `(delta_count, noisy_count, markdown)`.
fn trie_tables(base: &GroupedT, head: &GroupedT) -> (u32, u32, String) {
    let mut s = String::new();
    let mut total = 0u32;
    let mut noisy = 0u32;

    let mut groups: BTreeSet<(String, String)> = BTreeSet::new();
    groups.extend(base.keys().cloned());
    groups.extend(head.keys().cloned());

    let empty = BTreeMap::new();
    for (arity, op) in groups {
        let _ = write!(s, "**Trie {arity} {op} (base vs head, median ns)**");
        s.push_str("\n\n| store | shape | base | head | Δ% | |\n");
        s.push_str("| :--- | :--- | ---: | ---: | ---: | :-- |\n");

        let b_cells = base.get(&(arity.clone(), op.clone())).unwrap_or(&empty);
        let h_cells = head.get(&(arity.clone(), op.clone())).unwrap_or(&empty);
        let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
        keys.extend(b_cells.keys().cloned());
        keys.extend(h_cells.keys().cloned());
        for (store, shape) in keys {
            // `base_iv`/`head_iv`, not `bi`/`hi`, so the head interval never
            // reads as the `Interval.hi` upper-bound field.
            let base_iv = b_cells.get(&(store.clone(), shape.clone())).copied();
            let head_iv = h_cells.get(&(store.clone(), shape.clone())).copied();
            let (is_delta, is_noisy) =
                delta_row(&mut s, &format!("`{store}`"), &shape, base_iv, head_iv);
            total += u32::from(is_delta);
            noisy += u32::from(is_noisy);
        }
        s.push('\n');
    }
    (total, noisy, s)
}

/// Render one `| left1 | left2 | base | head | Δ% | mark |` row into `s`,
/// returning `(is_delta, is_noisy)`.
fn delta_row(
    s: &mut String,
    left1: &str,
    left2: &str,
    b: Option<Interval>,
    h: Option<Interval>,
) -> (bool, bool) {
    let base_cell = b.map_or_else(|| "–".to_owned(), |iv| format!("{:.2}", iv.point));
    let head_cell = h.map_or_else(|| "–".to_owned(), |iv| format!("{:.2}", iv.point));
    let (pct, mark, is_delta, is_noisy) = match (b, h) {
        (Some(bi), Some(hi)) if bi.point != 0.0 => {
            let d = (hi.point - bi.point) / bi.point * 100.0;
            if overlaps(bi, hi) {
                (format!("{d:+.1}%"), "~", true, true)
            } else {
                (format!("{d:+.1}%"), "", true, false)
            }
        }
        _ => ("–".to_owned(), "", false, false),
    };
    let _ = writeln!(
        s,
        "| {left1} | {left2} | {base_cell} | {head_cell} | {pct} | {mark} |"
    );
    (is_delta, is_noisy)
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
        let (is_delta, is_noisy) = delta_row(&mut s, &format!("`{op}`"), &subject, b, h);
        total += u32::from(is_delta);
        noisy += u32::from(is_noisy);
    }
    (total, noisy, s)
}

/// Render one cell's workload delta table. Returns `(delta_count, noisy_count,
/// markdown)` — mirrors `cell_table` for the occupancy-free workload family.
fn workload_table(
    heading: &str,
    want: Cell,
    base: &GroupedW,
    head: &GroupedW,
) -> (u32, u32, String) {
    let mut s = format!("**{heading} workload (base vs head, median ns)**\n\n");
    s.push_str("| op | subject | base | head | Δ% | |\n");
    s.push_str("| :--- | :--- | ---: | ---: | ---: | :-- |\n");
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
        let b = b_ops.get(&op).and_then(|m| m.get(&subject)).copied();
        let h = h_ops.get(&op).and_then(|m| m.get(&subject)).copied();
        let (is_delta, is_noisy) = delta_row(&mut s, &format!("`{op}`"), &subject, b, h);
        total += u32::from(is_delta);
        noisy += u32::from(is_noisy);
    }
    (total, noisy, s)
}

/// Build the A/B delta table: per cell a single-op table then a workload table,
/// then one convert table — each with base/head medians, a trailing `~` marker
/// when the base/head confidence intervals overlap (within noise), and one
/// non-failing summary counting deltas that exceed noise across every family.
#[must_use]
pub fn render_compare(before: &[Measurement], after: &[Measurement]) -> String {
    let mut s = String::new();
    let mut total = 0u32;
    let mut noisy = 0u32;

    // Single-op tables (per cell).
    let base = group_single(before);
    let head = group_single(after);
    let mut cells: BTreeSet<Cell> = BTreeSet::new();
    cells.extend(base.keys().copied());
    cells.extend(head.keys().copied());
    for cell in cells {
        let (t, n, body) = cell_table(cell_heading(cell), cell, &base, &head);
        s.push_str(&body);
        s.push('\n');
        total += t;
        noisy += n;
    }

    // Workload tables (per cell).
    let base_w = group_workload(before);
    let head_w = group_workload(after);
    let mut w_cells: BTreeSet<Cell> = BTreeSet::new();
    w_cells.extend(base_w.keys().copied());
    w_cells.extend(head_w.keys().copied());
    for cell in w_cells {
        let (t, n, body) = workload_table(cell_heading(cell), cell, &base_w, &head_w);
        s.push_str(&body);
        s.push('\n');
        total += t;
        noisy += n;
    }

    // Convert table (op x cell).
    let base_c = group_convert(before);
    let head_c = group_convert(after);
    if !base_c.is_empty() || !head_c.is_empty() {
        s.push_str("**Conversion (base vs head, median ns, max occupancy)**\n\n");
        s.push_str("| op | cell | base | head | Δ% | |\n");
        s.push_str("| :--- | :--- | ---: | ---: | ---: | :-- |\n");
        let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
        for (op, cells_m) in base_c.iter().chain(head_c.iter()) {
            for slug in cells_m.keys() {
                keys.insert((op.clone(), slug.clone()));
            }
        }
        for (op, slug) in keys {
            let b = base_c
                .get(&op)
                .and_then(|m| m.get(&slug))
                .map(|(_, iv)| *iv);
            let h = head_c
                .get(&op)
                .and_then(|m| m.get(&slug))
                .map(|(_, iv)| *iv);
            let (is_delta, is_noisy) = delta_row(&mut s, &format!("`{op}`"), &slug, b, h);
            total += u32::from(is_delta);
            noisy += u32::from(is_noisy);
        }
        s.push('\n');
    }

    // Trie tables (per arity x op).
    let base_t = group_trie(before);
    let head_t = group_trie(after);
    if !base_t.is_empty() || !head_t.is_empty() {
        let (t, n, body) = trie_tables(&base_t, &head_t);
        s.push_str(&body);
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

    #[test]
    fn workload_and_convert_families_appear() {
        use crate::bench_id::BenchId;
        let before = vec![
            Measurement::point(
                BenchId::Workload {
                    cell: Cell::A,
                    op: "build".to_owned(),
                    subject: "GappedArray".to_owned(),
                },
                40.0,
            ),
            Measurement::point(
                BenchId::Convert {
                    op: "pack".to_owned(),
                    cell: Cell::A,
                    occupancy: 16,
                },
                9.0,
            ),
        ];
        let after = vec![
            Measurement::point(
                BenchId::Workload {
                    cell: Cell::A,
                    op: "build".to_owned(),
                    subject: "GappedArray".to_owned(),
                },
                60.0,
            ),
            Measurement::point(
                BenchId::Convert {
                    op: "pack".to_owned(),
                    cell: Cell::A,
                    occupancy: 16,
                },
                18.0,
            ),
        ];
        let table = render_compare(&before, &after);
        assert!(
            table.contains("workload (base vs head"),
            "workload section present"
        );
        assert!(table.contains("`build`"), "build row present");
        assert!(
            table.contains("Conversion (base vs head"),
            "convert section present"
        );
        assert!(table.contains("`pack`"), "pack row present");
        assert!(table.contains("+50.0%"), "build delta");
        assert!(table.contains("+100.0%"), "convert delta");
    }

    #[test]
    fn trie_family_appears() {
        use crate::bench_id::BenchId;
        use crate::bench_id::TrieArity;
        let before = vec![Measurement::point(
            BenchId::Trie {
                arity: TrieArity::A16,
                op: "clone".to_owned(),
                store: "PackedStore".to_owned(),
                shape: "Bushy".to_owned(),
            },
            10.0,
        )];
        let after = vec![Measurement::point(
            BenchId::Trie {
                arity: TrieArity::A16,
                op: "clone".to_owned(),
                store: "PackedStore".to_owned(),
                shape: "Bushy".to_owned(),
            },
            20.0,
        )];
        let table = render_compare(&before, &after);
        assert!(
            table.contains("**Trie arity16 clone (base vs head, median ns)**"),
            "trie section present"
        );
        assert!(
            table.contains("| `PackedStore` | Bushy |"),
            "store x shape row present"
        );
        assert!(table.contains("+100.0%"), "trie delta rendered");
        assert!(
            table.contains("1/1 deltas exceed"),
            "trie delta counted in the family-spanning summary"
        );
    }
}
