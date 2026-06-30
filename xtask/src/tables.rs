//! Regenerate the Markdown comparison tables embedded in the README files,
//! between `<!-- bench:start -->` / `<!-- bench:end -->` markers.

// Items are consumed by later tasks once subcommands are wired; until then the
// binary entry point does not reference this module's types.
#![expect(dead_code, reason = "consumed by later tasks that wire subcommands")]

use crate::ingest::Measurement;

const START: &str = "<!-- bench:start -->";
const END: &str = "<!-- bench:end -->";

/// Error from rewriting a marked region.
#[derive(Debug)]
pub struct TableError(pub String);

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TableError {}

/// Replace the text between the markers with `generated`, keeping the markers
/// and the surrounding document intact. All-or-nothing: a missing or
/// out-of-order marker pair is an error, leaving the caller free to abort
/// without writing.
///
/// # Errors
/// Returns [`TableError`] if either marker is absent or the end marker precedes
/// the start marker.
pub fn render_marked(existing: &str, generated: &str) -> Result<String, TableError> {
    let start = existing
        .find(START)
        .ok_or_else(|| TableError(format!("missing {START}")))?;
    let end = existing
        .find(END)
        .ok_or_else(|| TableError(format!("missing {END}")))?;
    let body_start = start + START.len();
    if end < body_start {
        return Err(TableError("end marker precedes start marker".to_owned()));
    }
    let mut out = String::with_capacity(existing.len() + generated.len());
    out.push_str(&existing[..body_start]);
    out.push('\n');
    out.push_str(generated);
    out.push('\n');
    out.push_str(&existing[end..]);
    Ok(out)
}

/// Build the generated comparison block: one single-op median table per cell
/// (A then B), each with one row per `op` and one column per subject, at the
/// largest occupancy seen for that (op, subject). The same block is written
/// into both READMEs, so neither cell's data is lost.
#[must_use]
pub fn comparison_table(measurements: &[Measurement]) -> String {
    use crate::bench_id::Cell;
    let mut s = String::new();
    s.push_str(&cell_table("Cell A (Arity16)", Cell::A, measurements));
    s.push('\n');
    s.push_str(&cell_table("Cell B (Arity256)", Cell::B, measurements));
    s
}

/// One cell's single-op median table, titled with `heading`.
fn cell_table(heading: &str, want: crate::bench_id::Cell, measurements: &[Measurement]) -> String {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::bench_id::BenchId;

    // (op, subject) -> (max occupancy seen, nanos at that occupancy).
    let mut subjects = BTreeSet::new();
    let mut cells: BTreeMap<(String, String), (usize, f64)> = BTreeMap::new();
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
        }
    }
    let subjects: Vec<String> = subjects.into_iter().collect();
    let ops: BTreeSet<String> = cells.keys().map(|(op, _)| op.clone()).collect();

    let mut s = format!("**{heading} single-op (median, max occupancy)**\n\n");
    s.push_str("| op |");
    for subj in &subjects {
        s.push(' ');
        s.push_str(subj);
        s.push_str(" |");
    }
    s.push_str("\n| :--- |");
    for _ in &subjects {
        s.push_str(" ---: |");
    }
    s.push('\n');
    for op in &ops {
        s.push_str("| `");
        s.push_str(op);
        s.push_str("` |");
        for subj in &subjects {
            match cells.get(&(op.clone(), subj.clone())) {
                Some((_, nanos)) => {
                    let _ = write!(s, " {nanos:.2} ns |");
                }
                None => s.push_str(" – |"),
            }
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
    fn comparison_table_covers_both_cells() {
        use crate::bench_id::BenchId;
        use crate::bench_id::Cell;
        let ms = vec![
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
                    cell: Cell::B,
                    op: "get_hit".to_owned(),
                    subject: "HashMap".to_owned(),
                    occupancy: 256,
                },
                nanos: 7.3,
            },
        ];
        let table = comparison_table(&ms);
        // Both cells' data must survive into the single generated block.
        assert!(table.contains("Cell A"), "cell A table present");
        assert!(table.contains("Cell B"), "cell B table present");
        assert!(table.contains("PackedArray"));
        assert!(table.contains("HashMap"));
    }
}
