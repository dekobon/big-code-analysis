//! HTML aggregated hotspot report.
//!
//! Sibling renderer to [`crate::markdown_report`]. Emits a single
//! self-contained HTML page covering the whole walk: a global summary
//! followed by per-language `<section>` blocks, each holding the same
//! hotspot tables the Markdown report produces (MI lowest, cyclomatic,
//! cognitive, Halstead effort, largest by SLOC, many-parameter
//! functions, class WMC, NEXITS, ABC magnitude). The page is fully
//! offline-renderable: inline CSS plus a small inline vanilla-JS
//! click-to-sort handler that binds to every `<table class="hotspot">`
//! independently. There is no CDN dependency, no external font, no
//! template engine.
//!
//! Determinism is preserved by mirroring the Markdown report's
//! `(value, file, start_line, name)` tie-breaker on every hotspot
//! table.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write;

use big_code_analysis::SpaceKind;

use crate::format_util::MetricScalar;
use crate::markdown_report::{
    FunctionSummary, is_class_like, mi_rating, sort_by_metric_asc, sort_by_metric_desc, thousands,
    title_case,
};

/// HTML-escape a string for safe interpolation into element text or
/// double-quoted attribute values. Returns a borrowed `Cow` when the
/// input is already safe so the common case (most metric column names,
/// well-formed paths) allocates nothing.
///
/// Keep in sync with `src/output/html.rs::escape_html` — both helpers
/// implement the same rule. The lib copy is intentionally private; we
/// duplicate rather than promote the symbol so the lib's public API
/// stays focused on metrics, not HTML utilities.
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

const INLINE_CSS: &str = "\
body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;\
margin:1.5rem;color:#222;background:#fafafa}\
h1{font-size:1.4rem;margin:0 0 0.5rem}\
h2{font-size:1.15rem;margin:1.5rem 0 0.5rem;\
border-bottom:1px solid #ccc;padding-bottom:0.25rem}\
h3{font-size:1rem;margin:1rem 0 0.4rem;color:#444}\
section{margin-top:2rem}\
section.lang-section{padding:0.5rem 1rem;border-radius:4px;\
border-left:3px solid rgba(127,127,127,0.35)}\
section.lang-section>h2{margin-top:0.25rem}\
section.lang-rust{background:rgba(222,128,82,0.08);border-left-color:rgba(222,128,82,0.55)}\
section.lang-python{background:rgba(58,118,196,0.08);border-left-color:rgba(58,118,196,0.55)}\
section.lang-javascript{background:rgba(229,202,71,0.10);border-left-color:rgba(229,202,71,0.65)}\
section.lang-typescript{background:rgba(46,116,194,0.08);border-left-color:rgba(46,116,194,0.55)}\
section.lang-tsx{background:rgba(86,156,214,0.08);border-left-color:rgba(86,156,214,0.55)}\
section.lang-java{background:rgba(196,69,60,0.08);border-left-color:rgba(196,69,60,0.55)}\
section.lang-kotlin{background:rgba(193,71,167,0.08);border-left-color:rgba(193,71,167,0.55)}\
section.lang-go{background:rgba(0,173,181,0.08);border-left-color:rgba(0,173,181,0.55)}\
section.lang-cpp{background:rgba(120,80,180,0.08);border-left-color:rgba(120,80,180,0.55)}\
section.lang-csharp{background:rgba(83,150,80,0.08);border-left-color:rgba(83,150,80,0.55)}\
section.lang-php{background:rgba(98,113,178,0.08);border-left-color:rgba(98,113,178,0.55)}\
section.lang-bash{background:rgba(96,128,96,0.08);border-left-color:rgba(96,128,96,0.55)}\
section.lang-perl{background:rgba(180,120,60,0.08);border-left-color:rgba(180,120,60,0.55)}\
section.lang-lua{background:rgba(0,86,180,0.08);border-left-color:rgba(0,86,180,0.55)}\
section.lang-tcl{background:rgba(160,90,140,0.08);border-left-color:rgba(160,90,140,0.55)}\
section.lang-other{background:rgba(127,127,127,0.06);border-left-color:rgba(127,127,127,0.45)}\
@media (prefers-color-scheme:dark){\
section.lang-rust{background:rgba(222,128,82,0.16)}\
section.lang-python{background:rgba(58,118,196,0.18)}\
section.lang-javascript{background:rgba(229,202,71,0.16)}\
section.lang-typescript{background:rgba(46,116,194,0.18)}\
section.lang-tsx{background:rgba(86,156,214,0.18)}\
section.lang-java{background:rgba(196,69,60,0.18)}\
section.lang-kotlin{background:rgba(193,71,167,0.18)}\
section.lang-go{background:rgba(0,173,181,0.18)}\
section.lang-cpp{background:rgba(120,80,180,0.20)}\
section.lang-csharp{background:rgba(83,150,80,0.18)}\
section.lang-php{background:rgba(98,113,178,0.20)}\
section.lang-bash{background:rgba(96,128,96,0.18)}\
section.lang-perl{background:rgba(180,120,60,0.18)}\
section.lang-lua{background:rgba(0,86,180,0.20)}\
section.lang-tcl{background:rgba(160,90,140,0.20)}\
section.lang-other{background:rgba(200,200,200,0.10)}\
}\
.summary{font-size:0.9rem;color:#444;margin-bottom:0.5rem}\
.summary strong{color:#222}\
.summary p{margin:0.2rem 0}\
.note{font-size:0.85rem;color:#555;margin:0.4rem 0}\
ul{margin:0.4rem 0 0.4rem 1.2rem;padding:0}\
li{margin:0.15rem 0;font-size:0.9rem}\
table.hotspot{border-collapse:collapse;width:100%;font-size:0.85rem;\
background:#fff;box-shadow:0 1px 2px rgba(0,0,0,0.06);margin-bottom:0.5rem}\
table.hotspot th,table.hotspot td{padding:0.4rem 0.6rem;\
border-bottom:1px solid #e5e5e5;text-align:left;white-space:nowrap}\
table.hotspot th{background:#f0f0f0;cursor:pointer;user-select:none;\
font-weight:600}\
table.hotspot th:hover{background:#e5e5e5}\
table.hotspot th[aria-sort=ascending]::after{content:\" \\2191\"}\
table.hotspot th[aria-sort=descending]::after{content:\" \\2193\"}\
table.hotspot tr:nth-child(even) td{background:#fafafa}\
table.hotspot td.numeric{text-align:right;font-variant-numeric:tabular-nums}\
";

/// Map a `LANG::get_name()` string to the per-language CSS class
/// suffix used in [`INLINE_CSS`]. Keeping this in lockstep with the
/// palette in `INLINE_CSS` is enforced by `palette_classes_have_css`.
///
/// Returns `"other"` for any language without an explicit palette
/// entry — those still receive the neutral `lang-section` styling.
fn language_palette_slug(lang_name: &str) -> &'static str {
    match lang_name {
        "rust" => "rust",
        "python" => "python",
        "javascript" => "javascript",
        "typescript" => "typescript",
        "tsx" => "tsx",
        "java" => "java",
        "kotlin" => "kotlin",
        "go" => "go",
        "c/c++" => "cpp",
        "c#" => "csharp",
        "php" => "php",
        "bash" => "bash",
        "perl" => "perl",
        "lua" => "lua",
        "tcl" => "tcl",
        _ => "other",
    }
}

const INLINE_JS: &str = "\
(function(){\
function num(s){return s===''?Number.POSITIVE_INFINITY:parseFloat(s.replace(/,/g,''));}\
document.querySelectorAll('table.hotspot').forEach(function(table){\
var headers=table.querySelectorAll('thead th');\
headers.forEach(function(th,idx){\
th.addEventListener('click',function(){sort(table,idx,th);});\
});\
});\
function sort(tbl,idx,th){\
var tbody=tbl.tBodies[0];\
if(!tbody)return;\
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
var an=num(av);\
var bn=num(bv);\
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

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

impl Align {
    fn is_numeric(self) -> bool {
        matches!(self, Self::Right)
    }
}

/// Plain-English tooltip for a metric column header, or `None` when the
/// header names a non-metric dimension (file, function, class, line,
/// language). Centralises the abbreviation glossary so every section of
/// the aggregate HTML report explains its columns identically.
///
/// Only headers actually emitted by [`generate_html_report`] are
/// catalogued — see the `metric_headers_carry_tooltips` test, which
/// scans real output and requires every entry here to appear in at
/// least one rendered `<th>`.
fn header_tooltip(header: &str) -> Option<&'static str> {
    let tip = match header {
        "SLOC" => "Source Lines Of Code: non-blank, non-comment source lines.",
        "MI" | "Avg MI" => {
            "Maintainability Index (Visual Studio scale, 0\u{2013}100): composite of Halstead volume, cyclomatic complexity, and SLOC; higher is more maintainable."
        }
        "Tokens" => "Total lexical tokens (AST leaves excluding comments) in the unit.",
        "CC" | "Avg CC" => {
            "Cyclomatic Complexity: number of linearly independent control-flow paths through the function."
        }
        "Cognitive" | "Avg Cognitive" => {
            "Cognitive complexity: how hard the code is for a human to follow; nesting and breaks in linear flow add weight."
        }
        "Effort" => "Halstead effort: estimated mental effort to (re)create the code.",
        "Volume" => "Halstead volume: program length weighted by vocabulary size.",
        "Est. Bugs" => "Halstead bugs: estimated defect count derived from program volume.",
        "Exits" => "Number of exit points (returns, throws, breaks out of the function).",
        "ABC" => {
            "ABC magnitude: sqrt(A\u{B2} + B\u{B2} + C\u{B2}) over Assignments, Branches, and Conditions."
        }
        "WMC" => {
            "Weighted Methods per Class: sum of cyclomatic complexity across the class's methods."
        }
        "Methods" => "Number of methods declared on the class.",
        "NPA" => "Number of Public Attributes declared on the class.",
        "NPM" => "Number of Public Methods declared on the class.",
        "Args" => "Number of declared parameters of the function.",
        "Functions" => "Number of functions and methods analysed.",
        "Files" => "Number of source files analysed.",
        _ => return None,
    };
    Some(tip)
}

#[derive(Clone, Copy)]
enum SortDir {
    Asc,
    Desc,
}

/// Write a `<table class="hotspot">` with one `<thead>` and one
/// `<tbody>`. `aligns` controls per-cell text alignment AND the
/// `data-numeric="1"` attribute that the inline sort handler reads to
/// pick numeric vs string comparison.
///
/// Cell strings are escaped via [`escape_html`]; do not pre-escape.
fn write_table(out: &mut String, headers: &[&str], aligns: &[Align], rows: &[Vec<String>]) {
    debug_assert_eq!(headers.len(), aligns.len());
    let _ = out.write_str("<table class=\"hotspot\">\n<thead><tr>");
    for (h, a) in headers.iter().zip(aligns) {
        let numeric_attr = if a.is_numeric() {
            " data-numeric=\"1\""
        } else {
            ""
        };
        let _ = write!(out, "<th{numeric_attr}");
        if let Some(tip) = header_tooltip(h) {
            let _ = write!(out, " title=\"{}\"", escape_html(tip));
        }
        let _ = write!(out, ">{}</th>", escape_html(h));
    }
    let _ = out.write_str("</tr></thead>\n<tbody>\n");
    for row in rows {
        debug_assert_eq!(row.len(), headers.len());
        let _ = out.write_str("<tr>");
        for (cell, a) in row.iter().zip(aligns) {
            let class = if a.is_numeric() {
                " class=\"numeric\""
            } else {
                ""
            };
            let _ = write!(out, "<td{class}>{}</td>", escape_html(cell));
        }
        let _ = out.write_str("</tr>\n");
    }
    let _ = out.write_str("</tbody>\n</table>\n");
}

/// Emit one hotspot section: filter `base` with `keep`, sort by
/// `metric` in `dir`, take the top `top_n`, write an `<h3>{title}</h3>`
/// header followed by the table. Returns `true` if a table was
/// emitted, so callers that need a trailing summary line (CC stats)
/// can gate it on actual content.
#[allow(clippy::too_many_arguments)]
fn emit_hotspot(
    out: &mut String,
    title: &str,
    base: &[&FunctionSummary],
    keep: impl Fn(&FunctionSummary) -> bool,
    metric: impl Fn(&FunctionSummary) -> f64,
    dir: SortDir,
    top_n: usize,
    headers: &[&str],
    aligns: &[Align],
    row: impl Fn(&FunctionSummary) -> Vec<String>,
) -> bool {
    let mut entries: Vec<&FunctionSummary> = base.iter().copied().filter(|s| keep(s)).collect();
    if entries.is_empty() {
        return false;
    }
    match dir {
        SortDir::Asc => sort_by_metric_asc(&mut entries, &metric),
        SortDir::Desc => sort_by_metric_desc(&mut entries, &metric),
    }
    let count = entries.len().min(top_n);
    let _ = writeln!(out, "<h3>{title}</h3>");
    let rows: Vec<Vec<String>> = entries[..count].iter().map(|s| row(s)).collect();
    write_table(out, headers, aligns, &rows);
    true
}

/// Produce a self-contained HTML quality-metrics report from the
/// collected summaries. `top_n` controls how many entries appear in
/// each hotspot table.
pub(crate) fn generate_html_report(summaries: &[FunctionSummary], top_n: usize) -> String {
    // Each summary contributes at most one row across all hotspot
    // tables (sections × top_n is bounded), but the per-language
    // overview table plus the inline CSS/JS already costs a few KB of
    // boilerplate. Pre-size for the boilerplate plus a generous per-
    // summary slack so a multi-MB report does not realloc dozens of
    // times.
    let mut out = String::with_capacity(8 * 1024 + summaries.len() * 64);

    let by_lang = {
        let mut map = BTreeMap::<&str, Vec<&FunctionSummary>>::new();
        for s in summaries {
            map.entry(s.language.get_name()).or_default().push(s);
        }
        map
    };

    let (total_files, total_sloc, total_ploc, total_cloc, total_functions, total_classes) =
        summaries.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize),
            |(files, sloc, ploc, cloc, funcs, classes), s| {
                (
                    files + usize::from(s.kind == SpaceKind::Unit),
                    sloc + if s.kind == SpaceKind::Unit { s.sloc } else { 0 },
                    ploc + if s.kind == SpaceKind::Unit { s.ploc } else { 0 },
                    cloc + if s.kind == SpaceKind::Unit { s.cloc } else { 0 },
                    funcs + usize::from(s.kind == SpaceKind::Function),
                    classes + usize::from(is_class_like(s.kind)),
                )
            },
        );
    let comment_ratio = if total_sloc > 0 {
        (total_cloc as f64 / total_sloc as f64) * 100.0
    } else {
        0.0
    };

    let languages_list: String = by_lang
        .keys()
        .map(|k| title_case(k))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = out.write_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    let _ = out.write_str("<meta charset=\"utf-8\">\n");
    let _ = writeln!(
        out,
        "<title>Code Quality Metrics Summary \u{2014} big-code-analysis</title>"
    );
    let _ = writeln!(out, "<style>{INLINE_CSS}</style>");
    let _ = out.write_str("</head>\n<body>\n");
    let _ = out.write_str("<h1>Code Quality Metrics Summary</h1>\n");

    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(
        out,
        "<p><strong>Files analyzed:</strong> {} <strong>Languages:</strong> {}</p>",
        escape_html(&thousands(total_files)),
        escape_html(&languages_list),
    );
    let _ = writeln!(
        out,
        "<p><strong>Total SLOC:</strong> {} <strong>PLOC:</strong> {} <strong>Comments:</strong> {}</p>",
        escape_html(&thousands(total_sloc)),
        escape_html(&thousands(total_ploc)),
        escape_html(&thousands(total_cloc)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Functions/methods:</strong> {} <strong>Classes/impls/traits:</strong> {}</p>",
        escape_html(&thousands(total_functions)),
        escape_html(&thousands(total_classes)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Comment ratio:</strong> {comment_ratio:.1}%</p>"
    );
    let _ = out.write_str("</div>\n");

    if !by_lang.is_empty() {
        let _ = out.write_str("<h2>Per-language overview</h2>\n");
        let mut overview_rows: Vec<Vec<String>> = Vec::with_capacity(by_lang.len());
        for (&lang_name, lang_summaries) in &by_lang {
            let (lang_unit_count, lang_sloc, mi_sum) = lang_summaries
                .iter()
                .filter(|s| s.kind == SpaceKind::Unit)
                .fold((0usize, 0usize, 0.0f64), |(c, sl, mi), s| {
                    (c + 1, sl + s.sloc, mi + s.mi_visual_studio)
                });
            let avg_mi = if lang_unit_count > 0 {
                mi_sum / lang_unit_count as f64
            } else {
                0.0
            };
            let (func_count, avg_cc, avg_cog) = {
                let (count, cc_sum, cog_sum) = lang_summaries
                    .iter()
                    .filter(|s| s.kind == SpaceKind::Function)
                    .fold((0usize, 0.0f64, 0.0f64), |(c, cc, cog), s| {
                        (c + 1, cc + s.cyclomatic, cog + s.cognitive)
                    });
                if count > 0 {
                    (count, cc_sum / count as f64, cog_sum / count as f64)
                } else {
                    (0, 0.0, 0.0)
                }
            };
            overview_rows.push(vec![
                title_case(lang_name),
                thousands(lang_unit_count),
                thousands(lang_sloc),
                thousands(func_count),
                format!("{avg_mi:.1}"),
                format!("{avg_cc:.1}"),
                format!("{avg_cog:.1}"),
            ]);
        }
        write_table(
            &mut out,
            &[
                "Language",
                "Files",
                "SLOC",
                "Functions",
                "Avg MI",
                "Avg CC",
                "Avg Cognitive",
            ],
            &[
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
            ],
            &overview_rows,
        );

        for (&lang_name, lang_summaries) in &by_lang {
            write_language_section(&mut out, lang_name, lang_summaries, top_n);
        }
    }

    let _ = writeln!(out, "<script>{INLINE_JS}</script>");
    let _ = out.write_str("</body>\n</html>\n");
    out
}

fn write_language_section(
    out: &mut String,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
) {
    let display_name = title_case(lang_name);
    let slug = language_palette_slug(lang_name);
    let _ = writeln!(
        out,
        "<section class=\"lang-section lang-{slug}\"><h2>{}</h2>",
        escape_html(&display_name)
    );

    // Single pass that splits `entries` into per-kind buckets — the
    // earlier two-filter version walked the slice twice.
    let mut units: Vec<&FunctionSummary> = Vec::with_capacity(entries.len());
    let mut funcs: Vec<&FunctionSummary> = Vec::with_capacity(entries.len());
    for &s in entries {
        match s.kind {
            SpaceKind::Unit => units.push(s),
            SpaceKind::Function => funcs.push(s),
            _ => {}
        }
    }

    // ── Summary ─────────────────────────────────────────────────────
    {
        let (files, sloc, ploc, cloc, mi_sum) = units.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0.0f64),
            |(f, sl, pl, cl, mi), s| {
                (
                    f + 1,
                    sl + s.sloc,
                    pl + s.ploc,
                    cl + s.cloc,
                    mi + s.mi_visual_studio,
                )
            },
        );
        let cr = if sloc > 0 {
            (cloc as f64 / sloc as f64) * 100.0
        } else {
            0.0
        };
        let avg_mi = if files > 0 {
            mi_sum / files as f64
        } else {
            0.0
        };
        let rating = mi_rating(avg_mi);

        let _ = out.write_str("<h3>Summary</h3>\n");
        let _ = writeln!(
            out,
            "<p class=\"note\">Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%</p>",
            escape_html(&thousands(files)),
            escape_html(&thousands(sloc)),
            escape_html(&thousands(ploc)),
        );
        let _ = writeln!(
            out,
            "<p class=\"note\">Average MI: {avg_mi:.1} ({rating})</p>"
        );
    }

    // ── Maintainability Index (lowest files) ────────────────────────
    emit_hotspot(
        out,
        &format!("Maintainability Index (lowest files, top-{top_n})"),
        &units,
        |s| s.mi_visual_studio > 0.0,
        |s| s.mi_visual_studio,
        SortDir::Asc,
        top_n,
        &["File", "MI", "SLOC", "Tokens"],
        &[Align::Left, Align::Right, Align::Right, Align::Right],
        |s| {
            vec![
                s.file.clone(),
                format!("{:.1}", s.mi_visual_studio),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── Cyclomatic Complexity Hotspots ──────────────────────────────
    // Kept inline because it appends a stats note line after the table.
    {
        let (cc_sum, cc_count, max_cc, count_gt10, count_gt20) =
            funcs.iter().filter(|s| s.cyclomatic > 0.0).fold(
                (0.0f64, 0usize, f64::NAN, 0usize, 0usize),
                |(sum, cnt, mx, g10, g20), s| {
                    let c = s.cyclomatic;
                    (
                        sum + c,
                        cnt + 1,
                        f64::max(mx, c),
                        g10 + usize::from(c > 10.0),
                        g20 + usize::from(c > 20.0),
                    )
                },
            );
        let emitted = emit_hotspot(
            out,
            "Cyclomatic Complexity Hotspots",
            &funcs,
            |s| s.cyclomatic > 0.0,
            |s| s.cyclomatic,
            SortDir::Desc,
            top_n,
            &[
                "Function",
                "File",
                "Line",
                "CC",
                "Cognitive",
                "SLOC",
                "Tokens",
            ],
            &[
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
            ],
            |s| {
                vec![
                    s.name.clone(),
                    s.file.clone(),
                    s.start_line.to_string(),
                    MetricScalar(s.cyclomatic).to_string(),
                    MetricScalar(s.cognitive).to_string(),
                    thousands(s.sloc),
                    thousands(s.tokens),
                ]
            },
        );
        if emitted {
            let avg_cc = if cc_count > 0 {
                cc_sum / cc_count as f64
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "<p class=\"note\">Average CC: {avg_cc:.1} | Max: {max_cc:.0} | CC &gt; 10: {count_gt10} functions | CC &gt; 20: {count_gt20} functions</p>"
            );
        }
    }

    // ── Cognitive Complexity Hotspots ───────────────────────────────
    emit_hotspot(
        out,
        "Cognitive Complexity Hotspots",
        &funcs,
        |s| s.cognitive > 0.0,
        |s| s.cognitive,
        SortDir::Desc,
        top_n,
        &[
            "Function",
            "File",
            "Line",
            "Cognitive",
            "CC",
            "SLOC",
            "Tokens",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.start_line.to_string(),
                MetricScalar(s.cognitive).to_string(),
                MetricScalar(s.cyclomatic).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── Halstead Effort Hotspots ────────────────────────────────────
    emit_hotspot(
        out,
        "Halstead Effort Hotspots",
        &funcs,
        |s| s.halstead_effort > 0.0,
        |s| s.halstead_effort,
        SortDir::Desc,
        top_n,
        &[
            "Function",
            "File",
            "Effort",
            "Volume",
            "Est. Bugs",
            "SLOC",
            "Tokens",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                MetricScalar(s.halstead_effort).to_string(),
                MetricScalar(s.halstead_volume).to_string(),
                format!("{:.2}", s.halstead_bugs),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── Largest Functions by SLOC ───────────────────────────────────
    emit_hotspot(
        out,
        "Largest Functions by SLOC",
        &funcs,
        |s| s.sloc > 0,
        |s| s.sloc as f64,
        SortDir::Desc,
        top_n,
        &[
            "Function",
            "File",
            "Line",
            "SLOC",
            "Tokens",
            "CC",
            "Cognitive",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.start_line.to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
                MetricScalar(s.cyclomatic).to_string(),
                MetricScalar(s.cognitive).to_string(),
            ]
        },
    );

    // ── Functions With Many Parameters (>3) ─────────────────────────
    emit_hotspot(
        out,
        "Functions With Many Parameters (&gt;3)",
        &funcs,
        |s| s.nargs > 3,
        |s| s.nargs as f64,
        SortDir::Desc,
        top_n,
        &["Function", "File", "Args", "SLOC", "Tokens"],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.nargs.to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── Actionable Summary ──────────────────────────────────────────
    {
        let (cc_gt10, cog_gt15, sloc_gt100, nargs_gt3, bugs_gt1) = funcs.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize),
            |(a, b, c, d, e), s| {
                (
                    a + usize::from(s.cyclomatic > 10.0),
                    b + usize::from(s.cognitive > 15.0),
                    c + usize::from(s.sloc > 100),
                    d + usize::from(s.nargs > 3),
                    e + usize::from(s.halstead_bugs > 1.0),
                )
            },
        );
        let _ = out.write_str("<h3>Actionable Summary</h3>\n");
        if cc_gt10 == 0 && cog_gt15 == 0 && sloc_gt100 == 0 && nargs_gt3 == 0 && bugs_gt1 == 0 {
            let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        } else {
            let _ = out.write_str("<ul>\n");
            if cc_gt10 > 0 {
                let _ = writeln!(
                    out,
                    "<li><strong>{cc_gt10}</strong> functions with CC &gt; 10</li>"
                );
            }
            if cog_gt15 > 0 {
                let _ = writeln!(
                    out,
                    "<li><strong>{cog_gt15}</strong> functions with cognitive complexity &gt; 15</li>"
                );
            }
            if sloc_gt100 > 0 {
                let _ = writeln!(
                    out,
                    "<li><strong>{sloc_gt100}</strong> functions with SLOC &gt; 100</li>"
                );
            }
            if nargs_gt3 > 0 {
                let _ = writeln!(
                    out,
                    "<li><strong>{nargs_gt3}</strong> functions with more than 3 parameters</li>"
                );
            }
            if bugs_gt1 > 0 {
                let _ = writeln!(
                    out,
                    "<li><strong>{bugs_gt1}</strong> functions with estimated Halstead bugs &gt; 1.0</li>"
                );
            }
            let _ = out.write_str("</ul>\n");
        }
    }

    // ── Class/Trait/Impl Hotspots (WMC) ─────────────────────────────
    // Sources from `entries` (all kinds), not `funcs`/`units`, because
    // class-likes are filtered out of both buckets.
    emit_hotspot(
        out,
        "Class/Trait/Impl Hotspots (WMC)",
        entries,
        |s| is_class_like(s.kind) && s.wmc > 0.0,
        |s| s.wmc,
        SortDir::Desc,
        top_n,
        &[
            "Class", "File", "Line", "WMC", "Methods", "NPA", "NPM", "SLOC", "Tokens",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.start_line.to_string(),
                MetricScalar(s.wmc).to_string(),
                s.nom.to_string(),
                MetricScalar(s.npa).to_string(),
                MetricScalar(s.npm).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── Functions with the most exit points (NEXITS) ────────────────
    emit_hotspot(
        out,
        "Functions with the most exit points (NEXITS)",
        &funcs,
        |s| s.nexits > 0,
        |s| s.nexits as f64,
        SortDir::Desc,
        top_n,
        &["Function", "File", "Line", "Exits", "CC", "SLOC", "Tokens"],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.start_line.to_string(),
                s.nexits.to_string(),
                MetricScalar(s.cyclomatic).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    // ── ABC Magnitude Hotspots ──────────────────────────────────────
    emit_hotspot(
        out,
        "ABC Magnitude Hotspots",
        &funcs,
        |s| s.abc > 0.0,
        |s| s.abc,
        SortDir::Desc,
        top_n,
        &["Function", "File", "Line", "ABC", "SLOC", "Tokens"],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        |s| {
            vec![
                s.name.clone(),
                s.file.clone(),
                s.start_line.to_string(),
                format!("{:.1}", s.abc),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        },
    );

    let _ = out.write_str("</section>\n");
}

// Pull in the same `quick-xml`-driven well-formedness walker the
// per-file metrics HTML output uses (see
// `big-code-analysis-cli/tests/common/validators.rs`). Declared at
// module scope so the `#[path]` attribute resolves relative to `src/`,
// which exists on disk — nesting under `mod tests` would resolve
// relative to a phantom `src/html_report/tests/` directory.
#[cfg(test)]
#[path = "../tests/common/validators.rs"]
#[allow(dead_code)]
mod validators_for_tests;

#[cfg(test)]
mod tests {
    use super::validators_for_tests::assert_html_well_formed;
    use super::*;
    use big_code_analysis::LANG;

    fn make_summary(name: &str, file: &str, kind: SpaceKind, language: LANG) -> FunctionSummary {
        FunctionSummary {
            file: file.to_string(),
            name: name.to_string(),
            kind,
            language,
            start_line: 1,
            end_line: 10,
            sloc: 20,
            ploc: 25,
            lloc: 15,
            cloc: 5,
            tokens: 30,
            cyclomatic: 3.0,
            cognitive: 2.0,
            halstead_volume: 100.0,
            halstead_difficulty: 5.0,
            halstead_effort: 500.0,
            halstead_bugs: 0.1,
            halstead_time: 28.0,
            mi_original: 80.0,
            mi_sei: 85.0,
            mi_visual_studio: 50.0,
            nargs: 2,
            nexits: 1,
            nom: 1,
            abc: 5.0,
            wmc: 3.0,
            npa: 0.0,
            npm: 0.0,
        }
    }

    fn rust_fixture() -> Vec<FunctionSummary> {
        vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("do_stuff", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("compute", "src/lib.rs", SpaceKind::Function, LANG::Rust),
        ]
    }

    fn two_lang_fixture() -> Vec<FunctionSummary> {
        let mut v = rust_fixture();
        v.push(make_summary(
            "main.py",
            "src/main.py",
            SpaceKind::Unit,
            LANG::Python,
        ));
        v.push(make_summary(
            "greet",
            "src/main.py",
            SpaceKind::Function,
            LANG::Python,
        ));
        v
    }

    #[test]
    fn escape_html_passthrough() {
        let s = "plain text with no entities";
        assert!(matches!(escape_html(s), Cow::Borrowed(b) if b == s));
    }

    #[test]
    fn escape_html_replaces_all_metacharacters() {
        let escaped = escape_html("a&b<c>d\"e'f");
        assert_eq!(escaped, "a&amp;b&lt;c&gt;d&quot;e&#39;f");
    }

    #[test]
    fn empty_summaries_emit_no_tables() {
        let out = generate_html_report(&[], 20);
        assert!(out.contains("<h1>Code Quality Metrics Summary</h1>"));
        assert!(!out.contains("<table"));
        assert_html_well_formed(&out);
    }

    #[test]
    fn js_handler_binds_all_hotspot_tables() {
        let out = generate_html_report(&[], 20);
        assert!(
            out.contains("document.querySelectorAll('table.hotspot')"),
            "JS sort handler must bind to every hotspot table by class, not by id"
        );
    }

    #[test]
    fn js_numeric_sort_strips_thousands_separators() {
        // Regression: numeric cells use `thousands()` to insert commas
        // (e.g. "5,521"). JavaScript's `parseFloat("5,521")` returns 5,
        // which would sort SLOC and Tokens columns by leading-digit
        // prefix instead of by value. The JS comparator must strip
        // commas before parsing.
        assert!(
            INLINE_JS.contains("replace(/,/g,'')"),
            "JS comparator must strip thousands separators before parseFloat"
        );

        // Verify the cells the comparator will operate on actually do
        // contain commas in real output, so this test stays meaningful
        // as the renderer evolves.
        let mut summaries = vec![make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        )];
        for i in 0..3 {
            let mut s = make_summary(
                &format!("fn_{i}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            s.sloc = 10_000 * (i + 1);
            s.tokens = 1_500_000 * (i + 1);
            summaries.push(s);
        }
        let out = generate_html_report(&summaries, 5);
        assert!(
            out.contains(">10,000<") && out.contains(">1,500,000<"),
            "expected thousands-formatted cells in output"
        );
    }

    #[test]
    fn single_language_well_formed() {
        let out = generate_html_report(&rust_fixture(), 20);
        assert!(out.contains("<h2>Rust</h2>"));
        assert!(out.contains("class=\"hotspot\""));
        assert_html_well_formed(&out);
    }

    #[test]
    fn two_language_well_formed_and_alphabetical() {
        let out = generate_html_report(&two_lang_fixture(), 20);
        assert!(out.contains("<h2>Python</h2>"));
        assert!(out.contains("<h2>Rust</h2>"));
        let py = out.find("<h2>Python</h2>").expect("python heading");
        let rs = out.find("<h2>Rust</h2>").expect("rust heading");
        assert!(
            py < rs,
            "language sections must be alphabetical: python at {py}, rust at {rs}"
        );
        assert_html_well_formed(&out);
    }

    #[test]
    fn xss_payload_is_escaped() {
        let mut summaries = rust_fixture();
        summaries[1].name = "<script>alert(1)</script>".to_string();
        summaries[1].file = "a&b\"c'd<e>".to_string();

        let out = generate_html_report(&summaries, 20);
        assert!(
            !out.contains("<script>alert(1)"),
            "raw <script> payload must not appear in output"
        );
        assert!(out.contains("&lt;script&gt;"), "< must escape to &lt;");
        assert!(out.contains("&amp;"), "& must escape to &amp;");
        assert!(out.contains("&quot;"), "\" must escape to &quot;");
        assert!(out.contains("&#39;"), "' must escape to &#39;");
        assert_html_well_formed(&out);
    }

    #[test]
    fn top_n_truncates_hotspot_rows() {
        let mut summaries = vec![make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        )];
        for i in 0..30 {
            let mut s = make_summary(
                &format!("fn_{i:02}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            s.cyclomatic = (i + 1) as f64;
            s.start_line = 100 + i;
            summaries.push(s);
        }

        let out = generate_html_report(&summaries, 5);
        let cc_section = out
            .split_once("<h3>Cyclomatic Complexity Hotspots</h3>")
            .expect("cyclomatic section present")
            .1;
        let cc_table = cc_section.split_once("</table>").expect("table closes").0;
        let row_count = cc_table.matches("<tr>").count();
        // <thead><tr> + 5 body <tr>s = 6.
        assert_eq!(
            row_count, 6,
            "expected 5 body rows + 1 header, got {row_count}"
        );
        assert_html_well_formed(&out);
    }

    #[test]
    fn output_is_byte_deterministic() {
        let s = two_lang_fixture();
        let a = generate_html_report(&s, 20);
        let b = generate_html_report(&s, 20);
        assert_eq!(a, b, "renderer must be byte-deterministic across runs");
    }

    #[test]
    fn nan_metric_input_does_not_crash_renderer() {
        // Smoke test only: NaN in any `metric > 0.0`-filtered field is
        // dropped before sort, but it still flows through the global
        // fold (`f64::max`, `+`, `{:.0}`) and the per-language
        // averages. This test verifies those don't panic. For the
        // sort-with-NaN safety claim, see `sort_by_metric_desc_handles_nan`.
        let mut summaries = rust_fixture();
        summaries[1].cyclomatic = f64::NAN;
        summaries[2].cyclomatic = 5.0;
        let out = generate_html_report(&summaries, 20);
        assert_html_well_formed(&out);
    }

    #[test]
    fn sort_by_metric_desc_handles_nan() {
        // The hotspot filters (`metric > 0.0`) drop NaN before it
        // reaches sort. This test bypasses the filters by calling the
        // sorter directly with a NaN-valued comparator, so a future
        // regression from `total_cmp` to `partial_cmp` would actually
        // panic and fail this test.
        let a = make_summary("a", "f.rs", SpaceKind::Function, LANG::Rust);
        let b = make_summary("b", "f.rs", SpaceKind::Function, LANG::Rust);
        let c = make_summary("c", "f.rs", SpaceKind::Function, LANG::Rust);
        let mut entries: Vec<&FunctionSummary> = vec![&a, &b, &c];
        sort_by_metric_desc(&mut entries, |s| match s.name.as_str() {
            "a" => f64::NAN,
            "b" => 1.0,
            _ => 5.0,
        });
        // No panic = pass. Asserting on the order would couple to
        // total_cmp's NaN placement (currently treats NaN as larger
        // than any finite value); the contract is "doesn't panic".
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn metric_headers_carry_tooltips() {
        // Every metric abbreviation listed in issue #138 must render
        // with a `title="…"` attribute so a casual reader can discover
        // what each column means without leaving the page. Non-metric
        // columns (File, Function, Class, Line, Language) intentionally
        // have no tooltip — they describe the row, not a metric.
        let mut summaries = rust_fixture();
        // Force a class-like row so the WMC table is emitted, which
        // owns the only Methods/NPA/NPM headers.
        summaries.push(make_summary(
            "Widget",
            "src/lib.rs",
            SpaceKind::Class,
            LANG::Rust,
        ));
        // The "Args" table is gated on nargs > 3; bump one function so
        // the section actually renders.
        summaries[1].nargs = 5;
        let out = generate_html_report(&summaries, 20);

        for header in [
            "SLOC",
            "MI",
            "Tokens",
            "CC",
            "Exits",
            "ABC",
            "WMC",
            "Methods",
            "NPA",
            "NPM",
            "Args",
            "Cognitive",
            "Effort",
            "Volume",
            "Est. Bugs",
            "Files",
            "Functions",
            "Avg MI",
            "Avg CC",
            "Avg Cognitive",
        ] {
            let tip = header_tooltip(header)
                .unwrap_or_else(|| panic!("missing tooltip mapping for header {header:?}"));
            let needle = format!(" title=\"{}\">{header}</th>", escape_html(tip));
            assert!(
                out.contains(&needle),
                "header {header:?} should render with title attribute; expected substring {needle:?}"
            );
        }

        // Non-metric labels must remain bare so click-to-sort UX is not
        // crowded with redundant tooltips for self-describing columns.
        for plain in ["File", "Function", "Class", "Line", "Language"] {
            assert!(
                header_tooltip(plain).is_none(),
                "header {plain:?} should not carry a tooltip"
            );
            let needle = format!(">{plain}</th>");
            assert!(
                out.contains(&needle),
                "expected bare <th>{plain}</th> in output"
            );
        }
    }

    #[test]
    fn language_palette_slug_known_and_fallback() {
        assert_eq!(language_palette_slug("rust"), "rust");
        assert_eq!(language_palette_slug("python"), "python");
        assert_eq!(language_palette_slug("c/c++"), "cpp");
        assert_eq!(language_palette_slug("c#"), "csharp");
        // Languages without an explicit palette entry fall through to
        // the neutral tint rather than fabricating a slug.
        assert_eq!(language_palette_slug("ccomment"), "other");
        assert_eq!(language_palette_slug("preproc"), "other");
        assert_eq!(language_palette_slug(""), "other");
    }

    #[test]
    fn per_language_sections_carry_palette_class() {
        let out = generate_html_report(&two_lang_fixture(), 5);
        assert!(
            out.contains("<section class=\"lang-section lang-rust\"><h2>Rust</h2>"),
            "Rust section must carry stable lang-rust palette class"
        );
        assert!(
            out.contains("<section class=\"lang-section lang-python\"><h2>Python</h2>"),
            "Python section must carry stable lang-python palette class"
        );
        // Both palette rules must be present in the inline stylesheet
        // so the class actually paints something.
        assert!(out.contains("section.lang-rust{background:"));
        assert!(out.contains("section.lang-python{background:"));
        // Dark-mode adapter is present so contrast holds in both themes.
        assert!(out.contains("@media (prefers-color-scheme:dark)"));
    }

    #[test]
    fn unknown_language_falls_back_to_lang_other() {
        // The renderer never sees a language outside `LANG`, but the
        // slug mapper must still degrade gracefully — exercised here by
        // calling the helper directly so a future grammar addition
        // (no palette entry yet) still renders cleanly.
        let slug = language_palette_slug("zig");
        assert_eq!(slug, "other");
        assert!(INLINE_CSS.contains("section.lang-other{background:"));
    }

    #[test]
    fn overview_table_and_actionable_summary_not_tinted() {
        let out = generate_html_report(&two_lang_fixture(), 5);
        // The per-language overview table sits between the global
        // <h2>Per-language overview</h2> and the first per-language
        // <section>. It must not be wrapped in a tinted section.
        let overview = out
            .find("<h2>Per-language overview</h2>")
            .expect("overview heading present");
        let first_section = out
            .find("<section class=\"lang-section")
            .expect("at least one per-language section");
        assert!(
            overview < first_section,
            "overview heading must precede the first tinted section"
        );
        let between = &out[overview..first_section];
        assert!(
            !between.contains("lang-section"),
            "overview region must not pick up a per-language tint"
        );

        // Actionable summaries live inside per-language sections by
        // design (one per language); ensure no fixture language fell
        // through to the neutral fallback class on a `<section>`.
        assert!(!out.contains("lang-section lang-other"));
    }

    #[test]
    fn snapshot_two_lang_report() {
        let out = generate_html_report(&two_lang_fixture(), 5);
        insta::assert_snapshot!("html_report_two_lang", out);
    }
}
