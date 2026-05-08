//! Self-contained HTML writer for [`FuncSpace`] trees.
//!
//! Emits one `<table>` row per space (function, class, struct, unit,
//! etc.), flattened depth-first from the root. Columns mirror
//! [`super::csv::CSV_HEADER`] so HTML and CSV stay in lock-step — a
//! single column name addresses the metric in JSON, CSV, and HTML.
//!
//! The page is fully offline-renderable: inline CSS plus a small
//! inline vanilla-JS click-to-sort handler. There is no CDN
//! dependency, no external font, no template engine. Every
//! interpolated string is HTML-escaped via [`escape_html`] so a
//! crafted source path or function name cannot inject markup.
//!
//! Empty / non-finite metric values render as empty `<td>` cells (not
//! `0`, not `NaN`) — `f64::NAN` and `f64::INFINITY` mean "not
//! applicable for this space" in the underlying metric structs, and
//! we keep that signal across the format boundary.
//!
//! If the source path is not valid UTF-8, the writer emits a page
//! containing only the table header (no data rows) and warns to
//! stderr, mirroring the CSV writer's convention.

use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

use crate::output::csv::CSV_HEADER;
use crate::output::funcspace_row::{IDENTITY_COLUMNS, metric_values};
use crate::output::numfmt::CellMetric;
use crate::spaces::FuncSpace;

/// File extension used when writing HTML output to a file path.
pub const HTML_EXTENSION: &str = ".html";

const INLINE_CSS: &str = "\
body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;\
margin:1.5rem;color:#222;background:#fafafa}\
h1{font-size:1.25rem;margin:0 0 1rem;word-break:break-all}\
table{border-collapse:collapse;width:100%;font-size:0.85rem;background:#fff;\
box-shadow:0 1px 2px rgba(0,0,0,0.06)}\
th,td{padding:0.4rem 0.6rem;border-bottom:1px solid #e5e5e5;text-align:left;\
white-space:nowrap}\
th{position:sticky;top:0;background:#f0f0f0;cursor:pointer;user-select:none;\
font-weight:600}\
th:hover{background:#e5e5e5}\
th[aria-sort=ascending]::after{content:\" \\2191\"}\
th[aria-sort=descending]::after{content:\" \\2193\"}\
tr:nth-child(even) td{background:#fafafa}\
td.metric{font-variant-numeric:tabular-nums}\
td.numeric{text-align:right}\
";

const INLINE_JS: &str = "\
(function(){\
var table=document.getElementById('metrics');\
if(!table)return;\
var headers=table.querySelectorAll('thead th');\
headers.forEach(function(th,idx){\
th.addEventListener('click',function(){sort(table,idx,th);});\
});\
function sort(tbl,idx,th){\
var tbody=tbl.tBodies[0];\
var rows=Array.prototype.slice.call(tbody.rows);\
var numeric=th.dataset.numeric==='1';\
var dir=th.getAttribute('aria-sort')==='ascending'?'descending':'ascending';\
tbl.querySelectorAll('thead th').forEach(function(h){h.removeAttribute('aria-sort');});\
th.setAttribute('aria-sort',dir);\
var sign=dir==='ascending'?1:-1;\
rows.sort(function(a,b){\
var av=a.cells[idx].textContent;\
var bv=b.cells[idx].textContent;\
if(numeric){\
var an=av===''?Number.POSITIVE_INFINITY:parseFloat(av);\
var bn=bv===''?Number.POSITIVE_INFINITY:parseFloat(bv);\
if(an<bn)return -1*sign;\
if(an>bn)return 1*sign;\
return 0;\
}\
return av.localeCompare(bv)*sign;\
});\
rows.forEach(function(r){tbody.appendChild(r);});\
}\
})();\
";

/// HTML-escape a string for safe interpolation into element text or
/// double-quoted attribute values. Returns a borrowed `Cow` when the
/// input is already safe so the common case (most metric column names,
/// well-formed paths) allocates nothing.
fn escape_html(s: &str) -> Cow<'_, str> {
    let needs_escape = s
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''));
    if !needs_escape {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

/// Write a self-contained HTML document for the metric tree rooted at
/// `space`. The `source_path` becomes the page heading and the value
/// of the `path` column on every row; if it is not valid UTF-8 the
/// document still renders but with zero data rows (header only) and a
/// warning is emitted to stderr — there is no useful fallback for an
/// HTML identifier cell.
pub fn write_html<W: Write>(
    space: &FuncSpace,
    source_path: &Path,
    mut writer: W,
) -> io::Result<()> {
    let path_display = source_path.to_str();
    let title = path_display.unwrap_or("(non-UTF-8 path)");
    let escaped_title = escape_html(title);

    writer.write_all(b"<!doctype html>\n<html lang=\"en\">\n<head>\n")?;
    writer.write_all(b"<meta charset=\"utf-8\">\n")?;
    writeln!(
        writer,
        "<title>{escaped_title} \u{2014} big-code-analysis</title>",
    )?;
    writer.write_all(b"<style>")?;
    writer.write_all(INLINE_CSS.as_bytes())?;
    writer.write_all(b"</style>\n</head>\n<body>\n")?;
    writeln!(writer, "<h1>{escaped_title}</h1>")?;
    writer.write_all(b"<table id=\"metrics\">\n<thead><tr>")?;
    // Columns 0..3 (path, space_name, space_kind) are textual; the
    // remaining columns — including start_line and end_line — sort
    // numerically. The JS sort handler keys off `data-numeric`.
    const FIRST_NUMERIC_COLUMN: usize = 3;
    for (idx, col) in CSV_HEADER.iter().enumerate() {
        let numeric = if idx >= FIRST_NUMERIC_COLUMN {
            "1"
        } else {
            "0"
        };
        let escaped_col = escape_html(col);
        write!(writer, "<th data-numeric=\"{numeric}\">{escaped_col}</th>")?;
    }
    writer.write_all(b"</tr></thead>\n<tbody>\n")?;

    if let Some(path_str) = path_display {
        write_space_rows(&mut writer, path_str, space)?;
    } else {
        eprintln!(
            "Warning: skipping non-UTF-8 source path in HTML output: {}",
            source_path.display()
        );
    }

    writer.write_all(b"</tbody>\n</table>\n<script>")?;
    writer.write_all(INLINE_JS.as_bytes())?;
    writer.write_all(b"</script>\n</body>\n</html>\n")?;
    writer.flush()
}

fn write_space_rows<W: Write>(writer: &mut W, path_str: &str, space: &FuncSpace) -> io::Result<()> {
    write_one_row(writer, path_str, space)?;
    for child in &space.spaces {
        write_space_rows(writer, path_str, child)?;
    }
    Ok(())
}

fn write_one_row<W: Write>(writer: &mut W, path_str: &str, space: &FuncSpace) -> io::Result<()> {
    writer.write_all(b"<tr>")?;

    // Identity cells (text columns: path, space_name, space_kind +
    // numeric line columns).
    write_text_cell(writer, "path", path_str)?;
    write_text_cell(writer, "space_name", space.name.as_deref().unwrap_or(""))?;
    let kind_str = space.kind.to_string();
    write_text_cell(writer, "space_kind", &kind_str)?;
    write_numeric_cell(writer, "start_line", space.start_line as f64)?;
    write_numeric_cell(writer, "end_line", space.end_line as f64)?;

    let metrics = metric_values(space);

    for (offset, value) in metrics.iter().enumerate() {
        // Safe: offset < metrics.len() == CSV_HEADER.len() - IDENTITY_COLUMNS,
        // so the index is in bounds.
        let metric_name = CSV_HEADER[IDENTITY_COLUMNS + offset];
        write_numeric_cell(writer, metric_name, *value)?;
    }

    writer.write_all(b"</tr>\n")
}

fn write_text_cell<W: Write>(writer: &mut W, metric: &str, value: &str) -> io::Result<()> {
    let escaped_metric = escape_html(metric);
    let escaped_value = escape_html(value);
    write!(
        writer,
        "<td class=\"metric\" data-metric=\"{escaped_metric}\" data-value=\"{escaped_value}\">{escaped_value}</td>",
    )
}

fn write_numeric_cell<W: Write>(writer: &mut W, metric: &str, value: f64) -> io::Result<()> {
    let cell = CellMetric(value);
    let escaped_metric = escape_html(metric);
    // Formatted numbers never contain HTML-special characters, so we
    // write the `CellMetric` Display adapter directly into the
    // formatter twice — no per-cell `String` allocation.
    write!(
        writer,
        "<td class=\"metric numeric\" data-metric=\"{escaped_metric}\" data-value=\"{cell}\">{cell}</td>",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spaces::SpaceKind;
    use std::path::PathBuf;

    fn empty_space(name: &str, kind: SpaceKind, start: usize, end: usize) -> FuncSpace {
        FuncSpace {
            name: Some(name.into()),
            start_line: start,
            end_line: end,
            kind,
            spaces: Vec::new(),
            metrics: Default::default(),
        }
    }

    fn render(space: &FuncSpace, path: &Path) -> String {
        let mut buf = Vec::new();
        write_html(space, path, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    #[test]
    fn header_columns_match_csv_header() {
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        for col in CSV_HEADER {
            assert!(
                out.contains(&format!(">{col}</th>")),
                "missing header column {col} in:\n{out}"
            );
        }
    }

    #[test]
    fn nan_metric_values_render_as_empty_data_value() {
        // A bare unit space has NaN for every average/min/max — those
        // must come out as empty cells, never `NaN`, never `0`.
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        assert!(
            !out.contains("NaN"),
            "NaN must not leak into HTML output:\n{out}"
        );
        assert!(
            !out.contains(">inf<") && !out.contains("\"inf\""),
            "infinity must not leak into HTML output:\n{out}"
        );
        // Empty data-value attributes should appear for the
        // non-applicable metric columns.
        assert!(
            out.contains("data-value=\"\""),
            "expected at least one empty data-value cell in:\n{out}"
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
        // Confirm the four space_name cells appear in depth-first order.
        let pos = |needle: &str| out.find(needle).expect("substring in output");
        let p_root = pos("data-metric=\"space_name\" data-value=\"root\"");
        let p_outer = pos("data-metric=\"space_name\" data-value=\"outer\"");
        let p_inner = pos("data-metric=\"space_name\" data-value=\"inner\"");
        let p_sibling = pos("data-metric=\"space_name\" data-value=\"sibling\"");
        assert!(
            p_root < p_outer && p_outer < p_inner && p_inner < p_sibling,
            "depth-first order violated: root={p_root}, outer={p_outer}, inner={p_inner}, sibling={p_sibling}",
        );
    }

    #[test]
    fn html_special_chars_in_path_and_name_are_escaped() {
        // Crafted input must not be able to inject markup or break out
        // of attributes (XSS). The five HTML metacharacters &, <, >,
        // ", ' must all be entity-encoded.
        let space = empty_space("<svg/onload=alert(1)>", SpaceKind::Function, 1, 1);
        let out = render(&space, Path::new("a&b\"c'd<e>f.rs"));

        // No raw script-injection vector should survive.
        assert!(
            !out.contains("<svg/onload"),
            "raw <svg/onload= survived escaping:\n{out}"
        );
        // The escaped form must appear instead.
        assert!(
            out.contains("&lt;svg/onload=alert(1)&gt;"),
            "function name was not entity-encoded in:\n{out}"
        );
        // Path metacharacters must be encoded too, both in the <h1>
        // and in the per-row data-value attribute.
        assert!(
            out.contains("a&amp;b&quot;c&#39;d&lt;e&gt;f.rs"),
            "path metacharacters were not entity-encoded in:\n{out}"
        );
        // No raw " inside an attribute value.
        assert!(
            !out.contains("data-value=\"a&amp;b\"c"),
            "unescaped quote survived in attribute value:\n{out}"
        );
    }

    #[test]
    fn integral_values_have_no_trailing_dot_zero() {
        let mut space = empty_space("root", SpaceKind::Unit, 1, 1);
        space.metrics.loc.init_unit_span(0, 42);
        let out = render(&space, Path::new("a.rs"));
        assert!(
            out.contains("data-metric=\"loc.sloc\" data-value=\"42\""),
            "expected loc.sloc=42 (no .0) in:\n{out}"
        );
    }

    #[test]
    fn data_metric_attributes_present_for_every_column() {
        // Tooling that wants to highlight specific metrics relies on
        // the data-metric attribute being stable across the entire
        // header. Spot-check a representative cross-section.
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        for col in [
            "path",
            "space_name",
            "space_kind",
            "start_line",
            "end_line",
            "loc.sloc",
            "halstead.volume",
            "mi.mi_visual_studio",
        ] {
            assert!(
                out.contains(&format!("data-metric=\"{col}\"")),
                "missing data-metric={col} in:\n{out}"
            );
        }
    }

    #[test]
    fn page_is_self_contained() {
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        let out = render(&space, Path::new("a.rs"));
        // No CDN references.
        assert!(!out.contains("http://"), "external http link in:\n{out}");
        assert!(!out.contains("https://"), "external https link in:\n{out}");
        // Inline CSS and JS must be present.
        assert!(out.contains("<style>"), "missing inline <style> in:\n{out}");
        assert!(
            out.contains("<script>"),
            "missing inline <script> in:\n{out}"
        );
    }

    #[test]
    fn non_utf8_path_emits_header_only_table() {
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;

            let bad = PathBuf::from(OsStr::from_bytes(b"\xff\xfe.rs"));
            let space = empty_space("root", SpaceKind::Unit, 1, 1);
            let out = render(&space, &bad);
            // Header must still be present.
            assert!(out.contains("<thead>"), "missing thead in:\n{out}");
            // No body row was emitted.
            assert!(
                !out.contains("data-metric=\"space_name\""),
                "data row should be suppressed for non-UTF-8 path:\n{out}",
            );
        }
    }

    #[test]
    fn empty_snapshot_is_stable() {
        let space = empty_space("root", SpaceKind::Unit, 1, 1);
        insta::assert_snapshot!("html_empty_unit", render(&space, Path::new("a.rs")));
    }

    #[test]
    fn nested_snapshot_is_stable() {
        let mut root = empty_space("root", SpaceKind::Unit, 1, 100);
        let mut outer = empty_space("outer", SpaceKind::Function, 10, 50);
        let inner = empty_space("inner", SpaceKind::Function, 20, 30);
        outer.spaces.push(inner);
        let sibling = empty_space("sibling", SpaceKind::Function, 60, 80);
        root.spaces.push(outer);
        root.spaces.push(sibling);
        insta::assert_snapshot!("html_nested", render(&root, Path::new("src/example.rs")));
    }
}
