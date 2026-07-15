//! Regenerate the Markdown comparison tables embedded in the README files,
//! between `<!-- bench:start -->` / `<!-- bench:end -->` markers.

use anyhow::Context;
use anyhow::bail;

use crate::ingest::Measurement;

const START: &str = "<!-- bench:start -->";
const END: &str = "<!-- bench:end -->";

/// Replace the text between the markers with `generated`, keeping the markers
/// and the surrounding document intact. All-or-nothing: a missing or
/// out-of-order marker pair is an error, leaving the caller free to abort
/// without writing.
///
/// # Errors
/// Returns an error if either marker is absent or the end marker precedes the
/// start marker.
pub fn render_marked(existing: &str, generated: &str) -> anyhow::Result<String> {
    let start = existing
        .find(START)
        .with_context(|| format!("missing {START}"))?;
    let end = existing
        .find(END)
        .with_context(|| format!("missing {END}"))?;
    let body_start = start + START.len();
    if end < body_start {
        bail!("end marker precedes start marker");
    }
    let mut out = String::with_capacity(existing.len() + generated.len());
    out.push_str(&existing[..body_start]);
    out.push('\n');
    out.push_str(generated);
    out.push('\n');
    out.push_str(&existing[end..]);
    Ok(out)
}

/// Build the generated comparison block written into both READMEs: per cell, a
/// single-op table (with an occupancy column) and a workload table, then one
/// convert table. The same block is written into both READMEs.
#[must_use]
pub fn comparison_table(measurements: &[Measurement]) -> String {
    use crate::bench_id::Cell;
    let mut s = String::new();
    for cell in [Cell::A, Cell::B] {
        let heading = match cell {
            Cell::A => "Cell A (Arity16)",
            Cell::B => "Cell B (Arity256)",
        };
        s.push_str(&single_op_table(heading, cell, measurements));
        s.push('\n');
        s.push_str(&workload_table(heading, cell, measurements));
        s.push('\n');
    }
    s.push_str(&convert_table(measurements));
    let trie = trie_tables(measurements);
    if !trie.is_empty() {
        s.push('\n');
        s.push_str(&trie);
    }
    s
}

/// One cell's single-op table: rows are ops, an `occ` column reports each row's
/// occupancy (rows differ — `get_miss`/`insert_new` sweep the partial slices),
/// and columns are subjects, at the largest occupancy seen per (op, subject).
fn single_op_table(
    heading: &str,
    want: crate::bench_id::Cell,
    measurements: &[Measurement],
) -> String {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::bench_id::BenchId;

    let mut subjects = BTreeSet::new();
    // (op, subject) -> (max occupancy, nanos at that occupancy)
    let mut cells: BTreeMap<(String, String), (usize, f64)> = BTreeMap::new();
    // op -> max occupancy across its subjects (uniform per op in practice).
    let mut op_occ: BTreeMap<String, usize> = BTreeMap::new();
    for m in measurements {
        if let BenchId::Single {
            cell,
            op,
            subject,
            occupancy,
        } = &m.id
        {
            if *cell != want {
                continue;
            }
            subjects.insert(subject.clone());
            let entry = cells
                .entry((op.clone(), subject.clone()))
                .or_insert((0, 0.0));
            if *occupancy >= entry.0 {
                *entry = (*occupancy, m.nanos);
            }
            let occ = op_occ.entry(op.clone()).or_insert(0);
            *occ = (*occ).max(*occupancy);
        }
    }
    let subjects: Vec<String> = subjects.into_iter().collect();
    let ops: BTreeSet<String> = cells.keys().map(|(op, _)| op.clone()).collect();

    let mut s = format!("**{heading} single-op (median ns)**\n\n");
    s.push_str("| op | occ |");
    for subj in &subjects {
        let _ = write!(s, " {subj} |");
    }
    s.push_str("\n| :--- | ---: |");
    for _ in &subjects {
        s.push_str(" ---: |");
    }
    s.push('\n');
    for op in &ops {
        let occ = op_occ.get(op).copied().unwrap_or(0);
        let _ = write!(s, "| `{op}` | {occ} |");
        for subj in &subjects {
            match cells.get(&(op.clone(), subj.clone())) {
                Some((_, nanos)) => {
                    let _ = write!(s, " {nanos:.2} |");
                }
                None => s.push_str(" – |"),
            }
        }
        s.push('\n');
    }
    s
}

/// One cell's workload table: rows are workload ops (`build`, `churn`), columns
/// are subjects. No occupancy (the macro sweeps the full arity).
fn workload_table(
    heading: &str,
    want: crate::bench_id::Cell,
    measurements: &[Measurement],
) -> String {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::bench_id::BenchId;

    let mut subjects = BTreeSet::new();
    let mut cells: BTreeMap<(String, String), f64> = BTreeMap::new();
    for m in measurements {
        if let BenchId::Workload { cell, op, subject } = &m.id {
            if *cell != want {
                continue;
            }
            subjects.insert(subject.clone());
            cells.insert((op.clone(), subject.clone()), m.nanos);
        }
    }
    let subjects: Vec<String> = subjects.into_iter().collect();
    let ops: BTreeSet<String> = cells.keys().map(|(op, _)| op.clone()).collect();

    let mut s = format!("**{heading} workload (median ns)**\n\n");
    s.push_str("| op |");
    for subj in &subjects {
        let _ = write!(s, " {subj} |");
    }
    s.push_str("\n| :--- |");
    for _ in &subjects {
        s.push_str(" ---: |");
    }
    s.push('\n');
    for op in &ops {
        let _ = write!(s, "| `{op}` |");
        for subj in &subjects {
            match cells.get(&(op.clone(), subj.clone())) {
                Some(nanos) => {
                    let _ = write!(s, " {nanos:.2} |");
                }
                None => s.push_str(" – |"),
            }
        }
        s.push('\n');
    }
    s
}

/// The convert table: rows are conversion ops (`pack`, `unpack`), columns are
/// cells, at the largest occupancy seen for each (op, cell).
fn convert_table(measurements: &[Measurement]) -> String {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::bench_id::BenchId;
    use crate::bench_id::Cell;

    // (op, cell) -> (max occupancy, nanos)
    let mut cells: BTreeMap<(String, Cell), (usize, f64)> = BTreeMap::new();
    for m in measurements {
        if let BenchId::Convert {
            op,
            cell,
            occupancy,
        } = &m.id
        {
            let entry = cells.entry((op.clone(), *cell)).or_insert((0, 0.0));
            if *occupancy >= entry.0 {
                *entry = (*occupancy, m.nanos);
            }
        }
    }
    let ops: BTreeSet<String> = cells.keys().map(|(op, _)| op.clone()).collect();

    let mut s = String::from("**Conversion (median ns, max occupancy)**\n\n");
    s.push_str("| op | cell_a | cell_b |\n");
    s.push_str("| :--- | ---: | ---: |\n");
    for op in &ops {
        let _ = write!(s, "| `{op}` |");
        for cell in [Cell::A, Cell::B] {
            match cells.get(&(op.clone(), cell)) {
                Some((_, nanos)) => {
                    let _ = write!(s, " {nanos:.2} |");
                }
                None => s.push_str(" – |"),
            }
        }
        s.push('\n');
    }
    s
}

/// Trie tables: one per `(arity, op)` — rows are child stores, columns are trie
/// shapes — at the median (ns). The trie family is four-dimensional
/// (arity x op x store x shape), so it renders one table per `(arity, op)`
/// rather than folding into the per-cell layout the other families use. Returns
/// the empty string when there are no trie measurements.
///
/// The distinct `(arity, op)` groups are collected first, then each group's
/// cells are gathered into a flat `(store, shape) -> nanos` map — this keeps
/// every annotated local at the one-level-nesting depth the sibling tables use,
/// so it does not trip `clippy::type_complexity`.
fn trie_tables(measurements: &[Measurement]) -> String {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::bench_id::BenchId;

    // Distinct (arity, op) groups, in stable (alphabetical) order.
    let mut groups: BTreeSet<(String, String)> = BTreeSet::new();
    for m in measurements {
        if let BenchId::Trie { arity, op, .. } = &m.id {
            groups.insert((arity.to_string(), op.clone()));
        }
    }

    let mut s = String::new();
    for (arity, op) in &groups {
        // (store, shape) -> nanos for this (arity, op).
        let mut cells: BTreeMap<(String, String), f64> = BTreeMap::new();
        for m in measurements {
            if let BenchId::Trie {
                arity: a,
                op: o,
                store,
                shape,
            } = &m.id
                && a.to_string() == *arity
                && o == op
            {
                cells.insert((store.clone(), shape.clone()), m.nanos);
            }
        }
        let stores: BTreeSet<String> = cells.keys().map(|(store, _)| store.clone()).collect();
        let shapes: BTreeSet<String> = cells.keys().map(|(_, shape)| shape.clone()).collect();

        let _ = write!(s, "**Trie {arity} {op} (median ns)**");
        s.push_str("\n\n| store |");
        for shape in &shapes {
            let _ = write!(s, " {shape} |");
        }
        s.push_str("\n| :--- |");
        for _ in &shapes {
            s.push_str(" ---: |");
        }
        s.push('\n');
        for store in &stores {
            let _ = write!(s, "| `{store}` |");
            for shape in &shapes {
                match cells.get(&(store.clone(), shape.clone())) {
                    Some(nanos) => {
                        let _ = write!(s, " {nanos:.2} |");
                    }
                    None => s.push_str(" – |"),
                }
            }
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_between_markers() {
        let doc = "intro\n<!-- bench:start -->\nOLD\n<!-- bench:end -->\noutro\n";
        let out = render_marked(doc, "NEW").unwrap();
        assert_eq!(
            out,
            "intro\n<!-- bench:start -->\nNEW\n<!-- bench:end -->\noutro\n"
        );
    }

    #[test]
    fn errors_when_markers_missing() {
        assert!(render_marked("no markers here", "NEW").is_err());
    }

    #[test]
    fn is_idempotent() {
        let doc = "<!-- bench:start -->\nOLD\n<!-- bench:end -->\n";
        let once = render_marked(doc, "NEW").unwrap();
        let twice = render_marked(&once, "NEW").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn comparison_table_covers_single_workload_and_convert() {
        use crate::bench_id::BenchId;
        use crate::bench_id::Cell;
        let ms = vec![
            Measurement::point(
                BenchId::Single {
                    cell: Cell::A,
                    op: "get_miss".to_owned(),
                    subject: "PackedArray".to_owned(),
                    occupancy: 8,
                },
                0.8,
            ),
            Measurement::point(
                BenchId::Single {
                    cell: Cell::A,
                    op: "get_hit".to_owned(),
                    subject: "PackedArray".to_owned(),
                    occupancy: 16,
                },
                1.1,
            ),
            // Cell B single-op exercises the second arm of `for cell in [A, B]`.
            Measurement::point(
                BenchId::Single {
                    cell: Cell::B,
                    op: "get_hit".to_owned(),
                    subject: "PackedArray".to_owned(),
                    occupancy: 256,
                },
                1.5,
            ),
            Measurement::point(
                BenchId::Workload {
                    cell: Cell::A,
                    op: "build".to_owned(),
                    subject: "GappedArray".to_owned(),
                },
                42.0,
            ),
            Measurement::point(
                BenchId::Convert {
                    op: "pack".to_owned(),
                    cell: Cell::B,
                    occupancy: 256,
                },
                7.3,
            ),
        ];
        let table = comparison_table(&ms);
        // Single-op table carries an occupancy column with the per-row value.
        assert!(
            table.contains("Cell A (Arity16) single-op (median ns)"),
            "cell A single-op heading"
        );
        assert!(
            table.contains("Cell B (Arity256) single-op (median ns)"),
            "cell B single-op heading"
        );
        assert!(table.contains("| occ |"), "occupancy column present");
        assert!(
            table.contains("| `get_miss` | 8 |"),
            "partial-sweep row shows its own occupancy"
        );
        assert!(
            table.contains("| `get_hit` | 16 |"),
            "cell A full-sweep row shows its own occupancy"
        );
        assert!(
            table.contains("| `get_hit` | 256 |"),
            "cell B full-sweep row shows its own occupancy"
        );
        // Workload table (no occupancy).
        assert!(table.contains("workload (median ns)"), "workload heading");
        assert!(table.contains("`build`"), "build row present");
        // Convert table (op x cell).
        assert!(table.contains("Conversion (median ns"), "convert heading");
        assert!(table.contains("`pack`"), "pack row present");
    }

    #[test]
    fn comparison_table_covers_trie_family() {
        use crate::bench_id::BenchId;
        use crate::bench_id::TrieArity;
        let ms = vec![
            Measurement::point(
                BenchId::Trie {
                    arity: TrieArity::A16,
                    op: "clone".to_owned(),
                    store: "PackedStore".to_owned(),
                    shape: "Bushy".to_owned(),
                },
                12.5,
            ),
            Measurement::point(
                BenchId::Trie {
                    arity: TrieArity::A256,
                    op: "drop".to_owned(),
                    store: "FixedStore".to_owned(),
                    shape: "Realistic".to_owned(),
                },
                34.0,
            ),
        ];
        let table = comparison_table(&ms);
        assert!(
            table.contains("**Trie arity16 clone (median ns)**"),
            "arity16 clone heading present"
        );
        assert!(
            table.contains("**Trie arity256 drop (median ns)**"),
            "arity256 drop heading present"
        );
        assert!(
            table.contains("| store | Bushy |"),
            "store x shape header row"
        );
        assert!(
            table.contains("| `PackedStore` | 12.50 |"),
            "median cell rendered"
        );
    }
}
