// bca: suppress-file(halstead, nargs, nexits)
// CSV serializer over the FuncSpace tree; the offenders are mechanical-writer
// aggregation artifacts, not per-function logic complexity.

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
    "halstead.unique_operators",
    "halstead.total_operators",
    "halstead.unique_operands",
    "halstead.total_operands",
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
    "nargs.function_args",
    "nargs.closure_args",
    "nargs.function_args_average",
    "nargs.closure_args_average",
    "nargs.total",
    "nargs.average",
    "nargs.function_args_min",
    "nargs.function_args_max",
    "nargs.closure_args_min",
    "nargs.closure_args_max",
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
    "wmc.class_wmc_sum",
    "wmc.interface_wmc_sum",
    "wmc.total",
    // npm
    "npm.class_npm_sum",
    "npm.interface_npm_sum",
    "npm.class_methods",
    "npm.interface_methods",
    "npm.class_coa",
    "npm.interface_coa",
    "npm.total",
    "npm.total_methods",
    "npm.coa",
    // npa
    "npa.class_npa_sum",
    "npa.interface_npa_sum",
    "npa.class_attributes",
    "npa.interface_attributes",
    "npa.class_cda",
    "npa.interface_cda",
    "npa.total",
    "npa.total_attributes",
    "npa.cda",
    // mi
    "mi.original",
    "mi.sei",
    "mi.visual_studio",
];

/// Write a CSV document for the metric tree rooted at `space`. The
/// `source_path` is recorded in the `path` column of every row; if it
/// is not valid UTF-8 the entire document is skipped (header + zero
/// rows) and a warning is emitted to stderr — there is no useful
/// fallback for a CSV identifier.
///
/// # Errors
///
/// Returns any [`io::Error`] surfaced by the underlying [`csv::Writer`]
/// while emitting the header row or any of the per-`FuncSpace` data
/// rows. The error preserves the cause from the wrapped `writer`.
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

/// Write multiple metric trees into ONE CSV document under a single shared
/// header row.
///
/// [`write_csv`] emits [`CSV_HEADER`] on every call, so concatenating its
/// output for several files would repeat the header before each file's rows
/// — a structurally invalid CSV that downstream parsers ingest as data. This
/// emits the header exactly once, then every file's rows, for the
/// `--output <FILE>` aggregate path (#669). A non-UTF-8 source path skips
/// that file's rows with a stderr warning, matching [`write_csv`].
///
/// # Errors
///
/// Returns any [`io::Error`] surfaced by the underlying [`csv::Writer`].
pub fn write_csv_aggregate<'a, W, I>(spaces: I, writer: W) -> io::Result<()>
where
    W: Write,
    I: IntoIterator<Item = (&'a FuncSpace, &'a Path)>,
{
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(writer);
    wtr.write_record(CSV_HEADER).map_err(csv_err)?;
    for (space, source_path) in spaces {
        let Some(path_str) = source_path.to_str() else {
            eprintln!(
                "Warning: skipping non-UTF-8 source path in CSV output: {}",
                source_path.display()
            );
            continue;
        };
        write_space_rows(&mut wtr, path_str, space)?;
    }
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
    // `path` and `space_name` are free-text identifiers derived from
    // source content; defang any leading spreadsheet-formula trigger so a
    // function or file named `=cmd|'...'` cannot execute as a formula in
    // Excel / Google Sheets (CWE-1236, #703). The remaining cells —
    // `space_kind` (a fixed enum), the integer line numbers, and the
    // numeric metric cells — cannot begin with a trigger character, so
    // they need no guard.
    row.push(defang_formula(path_str));
    row.push(defang_formula(space.name.as_deref().unwrap_or("")));
    row.push(space.kind.to_string());
    row.push(space.start_line.to_string());
    row.push(space.end_line.to_string());

    for v in metrics {
        row.push(CellMetric(v).to_string());
    }

    wtr.write_record(&row).map_err(csv_err)
}

/// Leading bytes that a spreadsheet application treats as the start of a
/// formula. A cell beginning with any of these is interpreted as a
/// formula by Excel / LibreOffice / Google Sheets, so an identifier such
/// as `=HYPERLINK(...)` or `@SUM(...)` taken from source content would
/// execute on open. The tab and carriage-return cases are included per
/// the OWASP CSV-injection guidance (some importers strip a leading tab
/// then re-evaluate the next character).
const FORMULA_TRIGGERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// Defang a free-text CSV cell against formula injection (CWE-1236) by
/// prefixing a single quote when the cell begins with a spreadsheet
/// formula trigger (`=`, `+`, `-`, `@`, tab, or carriage return), the
/// OWASP-recommended mitigation. The quote makes a spreadsheet treat the
/// whole cell as a literal string; the RFC 4180 quoting the `csv` crate
/// applies does *not* defang formulas (a quoted `"=cmd"` still evaluates
/// on import). Cells that do not start with a trigger — the
/// overwhelming majority — are returned unchanged with no allocation
/// beyond the owned `String` the row requires.
///
/// Apply this to every free-text, user-controlled CSV cell whose value
/// can be derived from repository content (file paths, identifier names).
/// Numeric or enum-valued cells that can never begin with a trigger
/// character need no defang. This is the shared helper behind both the
/// [`FuncSpace`] CSV writer here and the VCS-report CSV writer in the
/// CLI crate; do not duplicate the trigger list.
#[must_use]
pub fn defang_formula(cell: &str) -> String {
    if cell.starts_with(FORMULA_TRIGGERS) {
        let mut out = String::with_capacity(cell.len() + 1);
        out.push('\'');
        out.push_str(cell);
        out
    } else {
        cell.to_owned()
    }
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
    fn aggregate_emits_exactly_one_shared_header() {
        // The `--output <FILE>` aggregate (#669) concatenates several files'
        // rows into one document and MUST emit the header once. A naive
        // per-file `write_csv` loop repeats it before every file, which
        // downstream CSV parsers ingest as data rows (regression guard).
        let a = empty_space("a", SpaceKind::Unit, 1, 1);
        let b = empty_space("b", SpaceKind::Unit, 1, 1);
        let c = empty_space("c", SpaceKind::Unit, 1, 1);
        let spaces: Vec<(FuncSpace, &Path)> = vec![
            (a, Path::new("a.rs")),
            (b, Path::new("b.rs")),
            (c, Path::new("c.rs")),
        ];
        let mut buf = Vec::new();
        write_csv_aggregate(spaces.iter().map(|(s, p)| (s, *p)), &mut buf)
            .expect("writing to Vec is infallible");
        let out = String::from_utf8(buf).expect("output is UTF-8");

        let header_line = CSV_HEADER.join(",");
        let header_count = out.lines().filter(|l| *l == header_line).count();
        assert_eq!(header_count, 1, "exactly one header row, got:\n{out}");
        // One header + one data row per file (each Unit space is one row).
        assert_eq!(out.lines().count(), 4, "header + 3 data rows:\n{out}");
        assert!(
            out.lines().next() == Some(header_line.as_str()),
            "header first"
        );
    }

    #[test]
    fn header_constant_matches_documented_columns() {
        // Pins CSV_HEADER to the exact column list documented in
        // STABILITY.md ("Output report formats" -> "CSV columns") so the
        // written contract and the code cannot drift. A column added in
        // a minor bump must be appended both here and in STABILITY.md;
        // reordering or renaming is a 2.0 break (#559).
        let documented: &[&str] = &[
            "path",
            "space_name",
            "space_kind",
            "start_line",
            "end_line",
            "cognitive.sum",
            "cognitive.average",
            "cognitive.min",
            "cognitive.max",
            "cyclomatic.sum",
            "cyclomatic.average",
            "cyclomatic.min",
            "cyclomatic.max",
            "cyclomatic.modified.sum",
            "cyclomatic.modified.average",
            "cyclomatic.modified.min",
            "cyclomatic.modified.max",
            "halstead.unique_operators",
            "halstead.total_operators",
            "halstead.unique_operands",
            "halstead.total_operands",
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
            "nargs.function_args",
            "nargs.closure_args",
            "nargs.function_args_average",
            "nargs.closure_args_average",
            "nargs.total",
            "nargs.average",
            "nargs.function_args_min",
            "nargs.function_args_max",
            "nargs.closure_args_min",
            "nargs.closure_args_max",
            "nexits.sum",
            "nexits.average",
            "nexits.min",
            "nexits.max",
            "tokens.sum",
            "tokens.average",
            "tokens.min",
            "tokens.max",
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
            "wmc.class_wmc_sum",
            "wmc.interface_wmc_sum",
            "wmc.total",
            "npm.class_npm_sum",
            "npm.interface_npm_sum",
            "npm.class_methods",
            "npm.interface_methods",
            "npm.class_coa",
            "npm.interface_coa",
            "npm.total",
            "npm.total_methods",
            "npm.coa",
            "npa.class_npa_sum",
            "npa.interface_npa_sum",
            "npa.class_attributes",
            "npa.interface_attributes",
            "npa.class_cda",
            "npa.interface_cda",
            "npa.total",
            "npa.total_attributes",
            "npa.cda",
            "mi.original",
            "mi.sei",
            "mi.visual_studio",
        ];
        assert_eq!(CSV_HEADER, documented);
    }

    #[test]
    fn non_finite_metric_values_never_leak() {
        // A bare unit space must never emit `NaN` / `inf` in any cell.
        // Since #438 guarded the npa/npm accessibility ratios, a default
        // space no longer produces non-finite averages, so this asserts
        // the durable end-to-end invariant rather than the (now absent)
        // empty-cell columns. The non-finite -> empty-string rendering
        // itself is covered directly by
        // `numfmt::tests::cell_renders_non_finite_as_empty`.
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
    fn formula_injection_cell_is_defanged() {
        // A function (or path) named with a leading formula trigger must
        // not execute as a spreadsheet formula (CWE-1236, #703). The
        // `space_name` cell must be prefixed with `'` so Excel / Sheets
        // treat it as a literal string. The csv crate then RFC-4180
        // quotes the comma, but the leading `'` defang survives inside.
        let space = empty_space("=cmd|'/C calc'!A0", SpaceKind::Function, 1, 1);
        let out = render(&space, Path::new("p.rs"));
        let data_row = out.lines().nth(1).expect("data row");
        let name_cell = data_row.split(',').next().unwrap_or("");
        // The name cell is the second column; because it embeds a comma
        // the csv crate quotes it, so check the raw substring instead.
        assert!(
            out.contains("'=cmd|"),
            "formula-trigger name must be prefixed with a quote:\n{out}"
        );
        assert!(
            !data_row.starts_with("p.rs,=cmd"),
            "the un-defanged `=cmd` must not appear unquoted:\n{out}"
        );
        let _ = name_cell;
    }

    #[test]
    fn formula_injection_path_cell_is_defanged() {
        // The `path` cell is user-controlled too — a checked-in file
        // literally named `=cmd…` is the likelier attacker vector — so it
        // must be defanged with the same leading `'` (#703).
        let space = empty_space("f", SpaceKind::Function, 1, 1);
        let out = render(&space, Path::new("=cmd|'/C calc'!A0.rs"));
        assert!(
            out.contains("'=cmd|"),
            "formula-trigger path must be prefixed with a quote:\n{out}"
        );
        let data_row = out.lines().nth(1).expect("data row");
        assert!(
            !data_row.starts_with("=cmd"),
            "the un-defanged path `=cmd` must not lead the row:\n{out}"
        );
    }

    #[test]
    fn defang_formula_guards_every_trigger_but_leaves_plain_cells() {
        // Each OWASP-listed trigger character is defanged; a plain cell
        // and an empty cell are returned unchanged (no spurious quote).
        for trigger in ['=', '+', '-', '@', '\t', '\r'] {
            let cell = format!("{trigger}danger");
            let out = defang_formula(&cell);
            assert_eq!(out, format!("'{cell}"), "trigger {trigger:?} must defang");
        }
        // A bare trigger character alone is still defanged.
        assert_eq!(defang_formula("="), "'=");
        // Plain identifiers and the empty string are untouched.
        assert_eq!(defang_formula("compute"), "compute");
        assert_eq!(defang_formula(""), "");
        // A trigger in a non-leading position is harmless and untouched.
        assert_eq!(defang_formula("a=b"), "a=b");
        // A leading non-ASCII identifier is not a trigger.
        assert_eq!(defang_formula("café"), "café");
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
