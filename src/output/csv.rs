//! CSV writer for [`FuncSpace`] trees.
//!
//! Emits one row per space (function, class, struct, unit, etc.),
//! flattened depth-first from the root. Each row carries the source
//! path, space name and kind, line range, and every leaf metric
//! value. The header order is fixed by [`CSV_HEADER`] so downstream
//! tools (Pandas, Excel, awk) can rely on positional access.
//!
//! Empty / non-finite metric values render as empty CSV cells (not
//! `0`, not `NaN`) — `f64::NAN` and `f64::INFINITY` mean "not
//! applicable for this space" in the underlying metric structs, and
//! we keep that signal across the format boundary.
//!
//! RFC 4180 quoting (commas, double-quotes, newlines in values) is
//! handled by the [`csv`] crate; nothing in this module hand-rolls
//! escaping.
//!
//! If the source path is not valid UTF-8, the writer emits the
//! header row only (no data rows) and warns to stderr. There is no
//! useful fallback for the CSV `path` column, mirroring the
//! convention established by the Checkstyle writer.

use std::io::{self, Write};
use std::path::Path;

use crate::output::funcspace_row::{IDENTITY_COLUMNS, METRIC_COUNT, metric_values};
use crate::output::numfmt::CellMetric;
use crate::spaces::FuncSpace;

// Compile-time guarantee that the metric tuple matches CSV_HEADER —
// catches drift the moment a metric is added to one without the other.
const _: () = assert!(IDENTITY_COLUMNS + METRIC_COUNT == CSV_HEADER.len());

/// File extension used when writing CSV output to a file path.
pub const CSV_EXTENSION: &str = ".csv";

/// Fixed column order for [`write_csv`] output. Asserted by tests so
/// downstream consumers can rely on positional access. Metric column
/// names use dotted JSON-style paths (`loc.lloc`, `halstead.volume`)
/// so a single name addresses the metric in both JSON and CSV.
pub const CSV_HEADER: &[&str] = &[
    // Identity columns
    "path",
    "space_name",
    "space_kind",
    "start_line",
    "end_line",
    // cognitive
    "cognitive.sum",
    "cognitive.average",
    "cognitive.min",
    "cognitive.max",
    // cyclomatic
    "cyclomatic.sum",
    "cyclomatic.average",
    "cyclomatic.min",
    "cyclomatic.max",
    "cyclomatic.modified.sum",
    "cyclomatic.modified.average",
    "cyclomatic.modified.min",
    "cyclomatic.modified.max",
    // halstead
    "halstead.n1",
    "halstead.N1",
    "halstead.n2",
    "halstead.N2",
    "halstead.length",
    "halstead.estimated_program_length",
    "halstead.purity_ratio",
    "halstead.vocabulary",
    "halstead.volume",
    "halstead.difficulty",
    "halstead.level",
    "halstead.effort",
    "halstead.time",
    "halstead.bugs",
    // loc
    "loc.sloc",
    "loc.ploc",
    "loc.lloc",
    "loc.cloc",
    "loc.blank",
    "loc.sloc_average",
    "loc.ploc_average",
    "loc.lloc_average",
    "loc.cloc_average",
    "loc.blank_average",
    "loc.sloc_min",
    "loc.sloc_max",
    "loc.cloc_min",
    "loc.cloc_max",
    "loc.ploc_min",
    "loc.ploc_max",
    "loc.lloc_min",
    "loc.lloc_max",
    "loc.blank_min",
    "loc.blank_max",
    // nom
    "nom.functions",
    "nom.closures",
    "nom.functions_average",
    "nom.closures_average",
    "nom.total",
    "nom.average",
    "nom.functions_min",
    "nom.functions_max",
    "nom.closures_min",
    "nom.closures_max",
    // nargs
    "nargs.total_functions",
    "nargs.total_closures",
    "nargs.average_functions",
    "nargs.average_closures",
    "nargs.total",
    "nargs.average",
    "nargs.functions_min",
    "nargs.functions_max",
    "nargs.closures_min",
    "nargs.closures_max",
    // nexits (serialized as "nexits" in JSON)
    "nexits.sum",
    "nexits.average",
    "nexits.min",
    "nexits.max",
    // tokens
    "tokens.sum",
    "tokens.average",
    "tokens.min",
    "tokens.max",
    // abc
    "abc.assignments",
    "abc.branches",
    "abc.conditions",
    "abc.magnitude",
    "abc.assignments_average",
    "abc.branches_average",
    "abc.conditions_average",
    "abc.assignments_min",
    "abc.assignments_max",
    "abc.branches_min",
    "abc.branches_max",
    "abc.conditions_min",
    "abc.conditions_max",
    // wmc
    "wmc.classes",
    "wmc.interfaces",
    "wmc.total",
    // npm
    "npm.classes",
    "npm.interfaces",
    "npm.class_methods",
    "npm.interface_methods",
    "npm.classes_average",
    "npm.interfaces_average",
    "npm.total",
    "npm.total_methods",
    "npm.average",
    // npa
    "npa.classes",
    "npa.interfaces",
    "npa.class_attributes",
    "npa.interface_attributes",
    "npa.classes_average",
    "npa.interfaces_average",
    "npa.total",
    "npa.total_attributes",
    "npa.average",
    // mi
    "mi.mi_original",
    "mi.mi_sei",
    "mi.mi_visual_studio",
];

/// Write a CSV document for the metric tree rooted at `space`. The
/// `source_path` is recorded in the `path` column of every row; if it
/// is not valid UTF-8 the entire document is skipped (header + zero
/// rows) and a warning is emitted to stderr — there is no useful
/// fallback for a CSV identifier.
pub fn write_csv<W: Write>(space: &FuncSpace, source_path: &Path, writer: W) -> io::Result<()> {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false) // we drive the header manually so it stays in lock-step with CSV_HEADER
        .from_writer(writer);

    wtr.write_record(CSV_HEADER).map_err(csv_err)?;

    let Some(path_str) = source_path.to_str() else {
        eprintln!(
            "Warning: skipping non-UTF-8 source path in CSV output: {}",
            source_path.display()
        );
        return wtr.flush();
    };

    write_space_rows(&mut wtr, path_str, space)?;
    wtr.flush()
}

fn write_space_rows<W: Write>(
    wtr: &mut csv::Writer<W>,
    path_str: &str,
    space: &FuncSpace,
) -> io::Result<()> {
    write_one_row(wtr, path_str, space)?;
    for child in &space.spaces {
        write_space_rows(wtr, path_str, child)?;
    }
    Ok(())
}

fn write_one_row<W: Write>(
    wtr: &mut csv::Writer<W>,
    path_str: &str,
    space: &FuncSpace,
) -> io::Result<()> {
    let metrics = metric_values(space);

    let mut row: Vec<String> = Vec::with_capacity(CSV_HEADER.len());
    row.push(path_str.to_owned());
    row.push(space.name.as_deref().unwrap_or("").to_owned());
    row.push(space.kind.to_string());
    row.push(space.start_line.to_string());
    row.push(space.end_line.to_string());

    for v in metrics {
        row.push(CellMetric(v).to_string());
    }

    wtr.write_record(&row).map_err(csv_err)
}

fn csv_err(e: csv::Error) -> io::Error {
    // csv::Error wraps an io::Error for I/O failures; propagate
    // unchanged so callers see the original errno. Other variants
    // collapse into InvalidData since they are protocol-level
    // problems, not I/O. csv::Error has no public From<ErrorKind>
    // constructor, so format the kind via Debug to retain diagnostic
    // detail.
    match e.into_kind() {
        csv::ErrorKind::Io(io_err) => io_err,
        other => io::Error::new(io::ErrorKind::InvalidData, format!("{other:?}")),
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::spaces::{CodeMetrics, SpaceKind};

    fn empty_space(name: &str, kind: SpaceKind, start: usize, end: usize) -> FuncSpace {
        FuncSpace {
            name: Some(name.into()),
            name_was_lossy: false,
            start_line: start,
            end_line: end,
            kind,
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
            suppressed: crate::SuppressionScope::default(),
        }
    }

    fn render(space: &FuncSpace, path: &Path) -> String {
        let mut buf = Vec::new();
        write_csv(space, path, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    #[test]
    fn header_constant_matches_first_row() {
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        let first = out.lines().next().expect("at least the header row");
        let expected: Vec<&str> = CSV_HEADER.to_vec();
        let got: Vec<&str> = first.split(',').collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn empty_metric_values_render_as_empty_cells() {
        // A bare unit space has NaN for every average/min/max — those
        // must come out as empty cells, never `NaN`, never `0`.
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        assert!(
            !out.contains("NaN"),
            "NaN must not leak into CSV output:\n{out}"
        );
        assert!(
            !out.contains("inf"),
            "infinity must not leak into CSV output:\n{out}"
        );
        // Two adjacent commas indicate an empty field — there must be
        // at least one such pair given the empty space's NaN columns.
        assert!(out.contains(",,"), "expected empty cells in:\n{out}");
    }

    #[test]
    fn nested_spaces_flatten_depth_first() {
        let mut root = empty_space("root", SpaceKind::Unit, 1, 100);
        let mut outer = empty_space("outer", SpaceKind::Function, 10, 50);
        let inner = empty_space("inner", SpaceKind::Function, 20, 30);
        outer.spaces.push(inner);
        let sibling = empty_space("sibling", SpaceKind::Function, 60, 80);
        root.spaces.push(outer);
        root.spaces.push(sibling);

        let out = render(&root, Path::new("a.rs"));
        let names: Vec<&str> = out
            .lines()
            .skip(1) // header
            .map(|line| line.split(',').nth(1).unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["root", "outer", "inner", "sibling"]);
    }

    #[test]
    fn rfc_4180_quoting_handled_by_csv_crate() {
        // Names with commas, double-quotes and newlines must round-trip
        // through the csv crate's quoting; we never hand-roll escapes.
        let space = empty_space("a,b\"c\nd", SpaceKind::Function, 1, 1);
        let out = render(&space, Path::new("p.rs"));
        // The `csv` crate doubles embedded `"` and wraps the field in `"`s.
        assert!(
            out.contains(
                r#""a,b""c
d""#
            ),
            "expected RFC 4180 quoting in:\n{out}"
        );
    }

    #[test]
    fn non_utf8_path_skips_data_rows() {
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            use std::path::PathBuf;

            let bad = PathBuf::from(OsStr::from_bytes(b"\xff\xfe.rs"));
            let space = empty_space("root", SpaceKind::Unit, 1, 1);
            let out = render(&space, &bad);
            assert_eq!(
                out.lines().count(),
                1,
                "header should be the only line, got:\n{out}"
            );
        }
    }

    #[test]
    fn integral_values_have_no_trailing_dot_zero() {
        // Match the JSON serializer convention: integer-valued f64s
        // render as `42`, not `42.0`.
        let mut space = empty_space("root", SpaceKind::Unit, 1, 1);
        // Force a known LOC value via the public API. With `unit=true`
        // sloc = end - start, so (0, 42) yields 42.
        space.metrics.loc.init_unit_span(0, 42);
        let out = render(&space, Path::new("a.rs"));
        let row = out.lines().nth(1).expect("data row");
        let cells: Vec<&str> = row.split(',').collect();
        // Find the sloc column by header position.
        let sloc_idx = CSV_HEADER
            .iter()
            .position(|h| *h == "loc.sloc")
            .expect("loc.sloc in header");
        assert_eq!(cells[sloc_idx], "42", "row was: {row}");
    }
}
