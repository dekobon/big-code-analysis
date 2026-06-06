// bca: suppress-file(halstead, loc, nargs, nom)
// HTML report templating; thin per-language orchestrators delegating to small
// write_* helpers. File-level halstead/loc and summed nargs/nom are
// string-formatting-volume / many-fn aggregation artifacts.

// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::too_many_lines
)]

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

use big_code_analysis::{SpaceKind, SuppressionPolicy};

use crate::markdown_report::hotspot::{
    self, ACTIONABLE_SUMMARY_INDEX, Align, Cell, Column, HotspotSpec, SPECS, Source,
};
use crate::markdown_report::{FunctionSummary, is_class_like, mi_rating, thousands, title_case};

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
section.lang-irules{background:rgba(200,70,120,0.08);border-left-color:rgba(200,70,120,0.55)}\
section.lang-ruby{background:rgba(204,52,45,0.08);border-left-color:rgba(204,52,45,0.55)}\
section.lang-elixir{background:rgba(110,73,153,0.08);border-left-color:rgba(110,73,153,0.55)}\
section.lang-other{background:rgba(127,127,127,0.06);border-left-color:rgba(127,127,127,0.45)}\
@media (prefers-color-scheme:dark){\
section.lang-rust{background:rgba(222,128,82,0.16)}\
section.lang-python{background:rgba(58,118,196,0.18)}\
section.lang-javascript{background:rgba(229,202,71,0.16)}\
section.lang-typescript{background:rgba(46,116,194,0.18)}\
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
section.lang-irules{background:rgba(200,70,120,0.20)}\
section.lang-ruby{background:rgba(204,52,45,0.18)}\
section.lang-elixir{background:rgba(110,73,153,0.20)}\
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

/// `LANG::name()` -> CSS class suffix table. The renderer uses
/// every entry here; `language_palette_classes_have_css` walks
/// [`INLINE_CSS`] to confirm both the light and dark rules exist for
/// each suffix, so adding a row without the matching CSS fails the
/// suite. `"other"` is the neutral fallback for any name not listed.
///
/// Names match production output of [`big_code_analysis::LANG::name`]
/// (see `src/langs.rs`), which since #540 is the canonical lowercase
/// slug for every variant (`"cpp"`, `"csharp"`, `"tsx"`). `LANG::Tsx`
/// (`"tsx"`) reuses the `"typescript"` tint (it is TypeScript + JSX),
/// and the Mozilla-fork `"mozjs"` reuses the `"javascript"` tint (it
/// is JavaScript, just a different grammar) — neither needs its own
/// CSS rule.
const LANGUAGE_PALETTE: &[(&str, &str)] = &[
    ("rust", "rust"),
    ("python", "python"),
    ("javascript", "javascript"),
    ("mozjs", "javascript"),
    ("typescript", "typescript"),
    ("tsx", "typescript"),
    ("java", "java"),
    ("kotlin", "kotlin"),
    ("go", "go"),
    ("cpp", "cpp"),
    ("csharp", "csharp"),
    ("php", "php"),
    ("bash", "bash"),
    ("perl", "perl"),
    ("lua", "lua"),
    ("tcl", "tcl"),
    ("irules", "irules"),
    ("ruby", "ruby"),
    ("elixir", "elixir"),
];

fn language_palette_slug(lang_name: &str) -> &'static str {
    LANGUAGE_PALETTE
        .iter()
        .find_map(|&(name, slug)| (name == lang_name).then_some(slug))
        .unwrap_or("other")
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

// Multi-pattern tooltip strings shared by aliased headers
// ("MI"/"Avg MI", "CC"/"Avg CC", "Cognitive"/"Avg Cognitive").
const MI_TOOLTIP: &str = "Maintainability Index (Visual Studio scale, 0\u{2013}100): composite of Halstead volume, cyclomatic complexity, and SLOC; higher is more maintainable.";
const CC_TOOLTIP: &str = "Cyclomatic Complexity: number of linearly independent control-flow paths through the function.";
const COGNITIVE_TOOLTIP: &str = "Cognitive Complexity: how hard the code is for a human to follow; nesting and breaks in linear flow add weight.";

/// Plain-English tooltip catalogue for every metric column header
/// emitted by [`generate_html_report`]. Centralised so every section of
/// the report explains its columns identically. The
/// `metric_headers_carry_tooltips` test iterates this slice directly,
/// so a new entry is automatically required to appear in real output.
const HEADER_TOOLTIPS: &[(&str, &str)] = &[
    (
        "SLOC",
        "Source Lines Of Code: non-blank, non-comment source lines.",
    ),
    ("MI", MI_TOOLTIP),
    ("Avg MI", MI_TOOLTIP),
    (
        "Tokens",
        "Total lexical tokens (AST leaves excluding comments) in the unit.",
    ),
    ("CC", CC_TOOLTIP),
    ("Avg CC", CC_TOOLTIP),
    ("Cognitive", COGNITIVE_TOOLTIP),
    ("Avg Cognitive", COGNITIVE_TOOLTIP),
    (
        "Effort",
        "Halstead effort: estimated mental effort to (re)create the code.",
    ),
    (
        "Volume",
        "Halstead volume: program length weighted by vocabulary size.",
    ),
    (
        "Est. Bugs",
        "Halstead bugs: estimated defect count derived from program volume.",
    ),
    (
        "Exits",
        "Number of exit points (returns, throws, breaks out of the function).",
    ),
    (
        "ABC",
        "ABC magnitude: sqrt(A\u{B2} + B\u{B2} + C\u{B2}) over Assignments, Branches, and Conditions.",
    ),
    (
        "WMC",
        "Weighted Methods per Class: sum of cyclomatic complexity across the class's methods.",
    ),
    ("Methods", "Number of methods declared on the class."),
    ("NPA", "Number of Public Attributes declared on the class."),
    ("NPM", "Number of Public Methods declared on the class."),
    ("Args", "Number of declared parameters of the function."),
    ("Functions", "Number of functions and methods analysed."),
    ("Files", "Number of source files analysed."),
];

/// Plain-English tooltip for a metric column header, or `None` when the
/// header names a non-metric dimension (file, function, class, line,
/// language).
fn header_tooltip(header: &str) -> Option<&'static str> {
    HEADER_TOOLTIPS
        .iter()
        .find_map(|&(name, tip)| (name == header).then_some(tip))
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

/// Render a hotspot table from the shared [`Column`] descriptors. Builds the
/// parallel `headers`/`aligns`/`rows` arrays and delegates to [`write_table`],
/// the single source of truth for table bytes and escaping. HTML escapes
/// every cell uniformly, so the raw [`Cell`] payload (regardless of kind) is
/// handed to `write_table`, whose `escape_html` runs exactly once per cell.
fn write_hotspot_table(out: &mut String, columns: &[Column], entries: &[&FunctionSummary]) {
    let mut headers = Vec::with_capacity(columns.len());
    let mut aligns = Vec::with_capacity(columns.len());
    for col in columns {
        headers.push(col.header);
        aligns.push(col.align);
    }
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(entries.len());
    for s in entries {
        let mut row: Vec<String> = Vec::with_capacity(columns.len());
        for col in columns {
            let (Cell::Name(text) | Cell::Path(text) | Cell::Num(text)) = (col.cell)(s);
            row.push(text);
        }
        rows.push(row);
    }
    write_table(out, &headers, &aligns, &rows);
}

/// Per-language grouping of summaries, keyed by `LANG::name()` and
/// ordered alphabetically (so the report sections are deterministic).
type LangGroups<'a> = BTreeMap<&'a str, Vec<&'a FunctionSummary>>;

/// Group summaries by language name. The `BTreeMap` ordering drives the
/// alphabetical section order asserted by
/// `two_language_well_formed_and_alphabetical`.
fn group_by_language(summaries: &[FunctionSummary]) -> LangGroups<'_> {
    let mut map = LangGroups::new();
    for s in summaries {
        map.entry(s.language.name()).or_default().push(s);
    }
    map
}

/// Comment lines as a percentage of source lines, guarding the
/// zero-SLOC case. Shared by the global and per-language roll-ups so the
/// formula lives in one place.
fn comment_ratio_percent(sloc: usize, cloc: usize) -> f64 {
    if sloc > 0 {
        (cloc as f64 / sloc as f64) * 100.0
    } else {
        0.0
    }
}

/// Whole-walk roll-up shown in the global `<div class="summary">` block.
/// Only `SpaceKind::Unit` summaries contribute file-level line counts;
/// functions and class-likes are counted separately.
struct GlobalTotals {
    files: usize,
    sloc: usize,
    ploc: usize,
    cloc: usize,
    functions: usize,
    classes: usize,
}

impl GlobalTotals {
    fn from_summaries(summaries: &[FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            functions: 0,
            classes: 0,
        };
        for s in summaries {
            match s.kind {
                SpaceKind::Unit => {
                    t.files += 1;
                    t.sloc += s.sloc;
                    t.ploc += s.ploc;
                    t.cloc += s.cloc;
                }
                SpaceKind::Function => t.functions += 1,
                _ => {}
            }
            if is_class_like(s.kind) {
                t.classes += 1;
            }
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        comment_ratio_percent(self.sloc, self.cloc)
    }
}

fn write_html_head(out: &mut String) {
    let _ = out.write_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    let _ = out.write_str("<meta charset=\"utf-8\">\n");
    let _ = writeln!(
        out,
        "<title>Code Quality Metrics Summary \u{2014} big-code-analysis</title>"
    );
    let _ = writeln!(out, "<style>{INLINE_CSS}</style>");
    let _ = out.write_str("</head>\n<body>\n");
    let _ = out.write_str("<h1>Code Quality Metrics Summary</h1>\n");
}

fn write_global_summary(out: &mut String, totals: &GlobalTotals, by_lang: &LangGroups<'_>) {
    let languages_list: String = by_lang
        .keys()
        .map(|k| title_case(k))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(
        out,
        "<p><strong>Files analyzed:</strong> {} <strong>Languages:</strong> {}</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&languages_list),
    );
    let _ = writeln!(
        out,
        "<p><strong>Total SLOC:</strong> {} <strong>PLOC:</strong> {} <strong>Comments:</strong> {}</p>",
        escape_html(&thousands(totals.sloc)),
        escape_html(&thousands(totals.ploc)),
        escape_html(&thousands(totals.cloc)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Functions/methods:</strong> {} <strong>Classes/impls/traits:</strong> {}</p>",
        escape_html(&thousands(totals.functions)),
        escape_html(&thousands(totals.classes)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Comment ratio:</strong> {:.1}%</p>",
        totals.comment_ratio()
    );
    let _ = out.write_str("</div>\n");
}

/// Build the seven-cell per-language overview row (Files / SLOC /
/// Functions averaged from the unit and function summaries of one
/// language).
fn overview_row(lang_name: &str, lang_summaries: &[&FunctionSummary]) -> Vec<String> {
    let mut unit_count = 0usize;
    let mut sloc = 0usize;
    let mut mi_sum = 0.0f64;
    let mut func_count = 0usize;
    let mut cc_sum = 0.0f64;
    let mut cog_sum = 0.0f64;
    for s in lang_summaries {
        match s.kind {
            SpaceKind::Unit => {
                unit_count += 1;
                sloc += s.sloc;
                mi_sum += s.mi_visual_studio;
            }
            SpaceKind::Function => {
                func_count += 1;
                cc_sum += s.cyclomatic;
                cog_sum += s.cognitive;
            }
            _ => {}
        }
    }
    let avg_mi = if unit_count > 0 {
        mi_sum / unit_count as f64
    } else {
        0.0
    };
    let (avg_cc, avg_cog) = if func_count > 0 {
        (cc_sum / func_count as f64, cog_sum / func_count as f64)
    } else {
        (0.0, 0.0)
    };
    vec![
        title_case(lang_name),
        thousands(unit_count),
        thousands(sloc),
        thousands(func_count),
        format!("{avg_mi:.1}"),
        format!("{avg_cc:.1}"),
        format!("{avg_cog:.1}"),
    ]
}

fn write_overview_table(out: &mut String, by_lang: &LangGroups<'_>) {
    let _ = out.write_str("<h2>Per-language overview</h2>\n");
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(by_lang.len());
    for (&lang_name, lang_summaries) in by_lang {
        rows.push(overview_row(lang_name, lang_summaries));
    }
    write_table(
        out,
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
        &rows,
    );
}

fn write_html_tail(out: &mut String) {
    let _ = writeln!(out, "<script>{INLINE_JS}</script>");
    let _ = out.write_str("</body>\n</html>\n");
}

/// Produce a self-contained HTML quality-metrics report from the
/// collected summaries. `top_n` controls how many entries appear in
/// each hotspot table.
pub(crate) fn generate_html_report(
    summaries: &[FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) -> String {
    // Each summary contributes at most one row across all hotspot
    // tables (sections × top_n is bounded), but the per-language
    // overview table plus the inline CSS/JS already costs a few KB of
    // boilerplate. Pre-size for the boilerplate plus a generous per-
    // summary slack so a multi-MB report does not realloc dozens of
    // times.
    let mut out = String::with_capacity(8 * 1024 + summaries.len() * 64);
    let by_lang = group_by_language(summaries);
    let totals = GlobalTotals::from_summaries(summaries);

    write_html_head(&mut out);
    write_global_summary(&mut out, &totals, &by_lang);
    if !by_lang.is_empty() {
        write_overview_table(&mut out, &by_lang);
        for (&lang_name, lang_summaries) in &by_lang {
            write_language_section(&mut out, lang_name, lang_summaries, top_n, policy);
        }
    }
    write_html_tail(&mut out);
    out
}

/// Split a per-language slice into its unit (file) and function buckets
/// in a single pass. Class-likes are intentionally dropped from both
/// buckets — the WMC hotspot sources them straight from `entries`.
fn partition_by_kind<'a>(
    entries: &[&'a FunctionSummary],
) -> (Vec<&'a FunctionSummary>, Vec<&'a FunctionSummary>) {
    let mut units: Vec<&FunctionSummary> = Vec::with_capacity(entries.len());
    let mut funcs: Vec<&FunctionSummary> = Vec::with_capacity(entries.len());
    for &s in entries {
        match s.kind {
            SpaceKind::Unit => units.push(s),
            SpaceKind::Function => funcs.push(s),
            _ => {}
        }
    }
    (units, funcs)
}

/// File-level roll-up backing one language's `<h3>Summary</h3>` note.
/// Only `SpaceKind::Unit` summaries feed it.
struct LanguageTotals {
    files: usize,
    sloc: usize,
    ploc: usize,
    cloc: usize,
    mi_sum: f64,
}

impl LanguageTotals {
    fn from_units(units: &[&FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            mi_sum: 0.0,
        };
        for s in units {
            t.files += 1;
            t.sloc += s.sloc;
            t.ploc += s.ploc;
            t.cloc += s.cloc;
            t.mi_sum += s.mi_visual_studio;
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        comment_ratio_percent(self.sloc, self.cloc)
    }

    fn avg_mi(&self) -> f64 {
        if self.files > 0 {
            self.mi_sum / self.files as f64
        } else {
            0.0
        }
    }
}

fn write_language_header(out: &mut String, lang_name: &str) {
    let display_name = title_case(lang_name);
    // `slug` is sourced from `LANGUAGE_PALETTE` (or the literal "other"
    // fallback) — always lowercase ASCII, so it is interpolated raw
    // into the class attribute without `escape_html`.
    let slug = language_palette_slug(lang_name);
    let _ = writeln!(
        out,
        "<section class=\"lang-section lang-{slug}\"><h2>{}</h2>",
        escape_html(&display_name)
    );
}

fn write_language_summary(out: &mut String, units: &[&FunctionSummary]) {
    let totals = LanguageTotals::from_units(units);
    let cr = totals.comment_ratio();
    let avg_mi = totals.avg_mi();
    let rating = mi_rating(avg_mi);

    let _ = out.write_str("<h3>Summary</h3>\n");
    let _ = writeln!(
        out,
        "<p class=\"note\">Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&thousands(totals.sloc)),
        escape_html(&thousands(totals.ploc)),
    );
    let _ = writeln!(
        out,
        "<p class=\"note\">Average MI: {avg_mi:.1} ({rating})</p>"
    );
}

/// Emit one hotspot section: an `<h3>` heading followed by the column-driven
/// table, from a shared [`HotspotSpec`] and its already-selected `rows`. The
/// logical title is `escape_html`-ed here (e.g. the `>` in the many-parameters
/// heading → `&gt;`); it is a no-op for the metachar-free titles.
fn emit_html_section(
    out: &mut String,
    spec: &HotspotSpec,
    top_n: usize,
    rows: &[&FunctionSummary],
) {
    let title = spec.title.render(top_n);
    let _ = writeln!(out, "<h3>{}</h3>", escape_html(&title));
    write_hotspot_table(out, spec.columns, rows);
}

/// The cyclomatic summary note under the CC hotspot table. A caption over the
/// same suppression-filtered set the table shows (see [`hotspot::select_cc`]),
/// matching the Markdown report; the raw, suppression-independent CC count
/// lives in the Actionable Summary instead.
fn emit_cc_note_html(out: &mut String, stats: &hotspot::CyclomaticStats) {
    let _ = writeln!(
        out,
        "<p class=\"note\">Average CC: {:.1} | Max: {:.0} | CC &gt; 10: {} functions | CC &gt; 20: {} functions</p>",
        stats.avg(),
        stats.max,
        stats.gt10,
        stats.gt20,
    );
}

/// Bucket counts behind the `<h3>Actionable Summary</h3>` block.
struct ActionableCounts {
    cc_gt10: usize,
    cog_gt15: usize,
    sloc_gt100: usize,
    nargs_gt3: usize,
    bugs_gt1: usize,
}

impl ActionableCounts {
    fn from_funcs(funcs: &[&FunctionSummary]) -> Self {
        let mut a = Self {
            cc_gt10: 0,
            cog_gt15: 0,
            sloc_gt100: 0,
            nargs_gt3: 0,
            bugs_gt1: 0,
        };
        for s in funcs {
            a.cc_gt10 += usize::from(s.cyclomatic > 10.0);
            a.cog_gt15 += usize::from(s.cognitive > 15.0);
            a.sloc_gt100 += usize::from(s.sloc > 100);
            a.nargs_gt3 += usize::from(s.nargs > 3);
            a.bugs_gt1 += usize::from(s.halstead_bugs > 1.0);
        }
        a
    }

    fn all_clear(&self) -> bool {
        self.cc_gt10 == 0
            && self.cog_gt15 == 0
            && self.sloc_gt100 == 0
            && self.nargs_gt3 == 0
            && self.bugs_gt1 == 0
    }
}

fn write_actionable_summary(out: &mut String, funcs: &[&FunctionSummary]) {
    let counts = ActionableCounts::from_funcs(funcs);
    let _ = out.write_str("<h3>Actionable Summary</h3>\n");
    if counts.all_clear() {
        let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        return;
    }
    let _ = out.write_str("<ul>\n");
    if counts.cc_gt10 > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with CC &gt; 10</li>",
            counts.cc_gt10
        );
    }
    if counts.cog_gt15 > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with cognitive complexity &gt; 15</li>",
            counts.cog_gt15
        );
    }
    if counts.sloc_gt100 > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with SLOC &gt; 100</li>",
            counts.sloc_gt100
        );
    }
    if counts.nargs_gt3 > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with more than 3 parameters</li>",
            counts.nargs_gt3
        );
    }
    if counts.bugs_gt1 > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with estimated Halstead bugs &gt; 1.0</li>",
            counts.bugs_gt1
        );
    }
    let _ = out.write_str("</ul>\n");
}

fn write_language_section(
    out: &mut String,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) {
    write_language_header(out, lang_name);
    let (units, funcs) = partition_by_kind(entries);
    write_language_summary(out, &units);

    // Drive every hotspot section from the shared `SPECS` table so the HTML
    // and Markdown reports cannot diverge in membership/order/suppression.
    // The Actionable Summary (raw, format-specific) splices in at its fixed
    // index; WMC draws from the full slice, MI from units, the rest from
    // functions.
    for (i, spec) in SPECS.iter().enumerate() {
        if i == ACTIONABLE_SUMMARY_INDEX {
            write_actionable_summary(out, &funcs);
        }
        let base: &[&FunctionSummary] = match spec.source {
            Source::Units => &units,
            Source::Funcs => &funcs,
            Source::All => entries,
        };
        if spec.cc_note {
            let (rows, stats) = hotspot::select_cc(spec, base, top_n, policy);
            if !rows.is_empty() {
                emit_html_section(out, spec, top_n, &rows);
                emit_cc_note_html(out, &stats);
            }
        } else {
            let rows = hotspot::select(spec, base, top_n, policy);
            if !rows.is_empty() {
                emit_html_section(out, spec, top_n, &rows);
            }
        }
    }

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
    use super::validators_for_tests::assert_html_well_formed;
    use super::*;
    use big_code_analysis::{LANG, MetricKind};

    fn make_summary(name: &str, file: &str, kind: SpaceKind, language: LANG) -> FunctionSummary {
        FunctionSummary {
            file: file.to_string(),
            name: name.to_string(),
            kind,
            language,
            suppressed: big_code_analysis::SuppressionScope::default(),
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
        let out = generate_html_report(&[], 20, SuppressionPolicy::Honor);
        assert!(out.contains("<h1>Code Quality Metrics Summary</h1>"));
        assert!(!out.contains("<table"));
        assert_html_well_formed(&out);
    }

    #[test]
    fn js_handler_binds_all_hotspot_tables() {
        let out = generate_html_report(&[], 20, SuppressionPolicy::Honor);
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
        let out = generate_html_report(&summaries, 5, SuppressionPolicy::Honor);
        assert!(
            out.contains(">10,000<") && out.contains(">1,500,000<"),
            "expected thousands-formatted cells in output"
        );
    }

    #[test]
    fn single_language_well_formed() {
        let out = generate_html_report(&rust_fixture(), 20, SuppressionPolicy::Honor);
        assert!(out.contains("<h2>Rust</h2>"));
        assert!(out.contains("class=\"hotspot\""));
        assert_html_well_formed(&out);
    }

    #[test]
    fn two_language_well_formed_and_alphabetical() {
        let out = generate_html_report(&two_lang_fixture(), 20, SuppressionPolicy::Honor);
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

        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);
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

        let out = generate_html_report(&summaries, 5, SuppressionPolicy::Honor);
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
    fn wmc_hotspot_sources_class_likes() {
        // The WMC hotspot draws from the full per-language `entries`
        // slice, not the `funcs` bucket: `partition_by_kind` drops
        // class-likes from both `units` and `funcs`. A class-like
        // summary must therefore still land in the WMC table even when
        // the language has zero `SpaceKind::Function` summaries. Were
        // the spec sourced from `funcs` (empty here), `emit_hotspot`
        // would short-circuit and the `<h3>` below would never be
        // written, panicking the `expect` (issue #402).
        let entries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("Widget", "src/lib.rs", SpaceKind::Class, LANG::Rust),
        ];
        let out = generate_html_report(&entries, 20, SuppressionPolicy::Honor);
        let wmc_table = out
            .split_once("<h3>Class/Trait/Impl Hotspots (WMC)</h3>")
            .expect("WMC section present even with no functions")
            .1
            .split_once("</table>")
            .expect("WMC table closes")
            .0;
        assert!(
            wmc_table.contains("<td>Widget</td>"),
            "class-like summary must appear in WMC table even with no functions"
        );
        assert_html_well_formed(&out);
    }

    #[test]
    fn output_is_byte_deterministic() {
        let s = two_lang_fixture();
        let a = generate_html_report(&s, 20, SuppressionPolicy::Honor);
        let b = generate_html_report(&s, 20, SuppressionPolicy::Honor);
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
        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);
        assert_html_well_formed(&out);
    }

    #[test]
    fn sort_by_metric_desc_handles_nan() {
        use crate::markdown_report::sort_by_metric_desc;
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
        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);

        // Drive the loop from the catalogue itself so a new tooltip
        // arm is required to appear in real output without anyone
        // remembering to update the test. `needle` embeds the table
        // value directly, so any divergence between `header_tooltip`
        // and `HEADER_TOOLTIPS` would surface as a missing substring
        // here rather than via a separate (tautological) assert_eq.
        for &(header, tip) in HEADER_TOOLTIPS {
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
        // Since #540 the input is the canonical slug ("cpp" / "csharp"),
        // not the dropped pretty forms.
        assert_eq!(language_palette_slug("cpp"), "cpp");
        assert_eq!(language_palette_slug("csharp"), "csharp");
        // `LANG::Tsx` now reports the distinct "tsx" slug (#540) and
        // reuses the "typescript" tint via an explicit row. Since #507
        // the Mozilla fork reports "mozjs" and reuses the "javascript"
        // tint via an explicit row.
        assert_eq!(language_palette_slug("typescript"), "typescript");
        assert_eq!(language_palette_slug("tsx"), "typescript");
        assert_eq!(language_palette_slug("javascript"), "javascript");
        assert_eq!(language_palette_slug("mozjs"), "javascript");
        assert_eq!(language_palette_slug("ruby"), "ruby");
        assert_eq!(language_palette_slug("elixir"), "elixir");
        // Languages without an explicit palette entry fall through to
        // the neutral tint rather than fabricating a slug.
        assert_eq!(language_palette_slug("ccomment"), "other");
        assert_eq!(language_palette_slug("preproc"), "other");
        assert_eq!(language_palette_slug(""), "other");
    }

    #[test]
    fn language_palette_classes_have_css() {
        // The slug table and the inline stylesheet must move in
        // lockstep: every entry in `LANGUAGE_PALETTE` (plus the
        // `"other"` fallback) needs both a light-mode rule and a
        // dark-mode override, otherwise a `<section class="lang-X">`
        // would render as plain `lang-section`. This is the test the
        // doc-comment on `language_palette_slug` advertises.
        let dark_block = INLINE_CSS
            .split_once("@media (prefers-color-scheme:dark){")
            .expect("dark-mode adapter present")
            .1;
        for slug in LANGUAGE_PALETTE
            .iter()
            .map(|&(_, slug)| slug)
            .chain(std::iter::once("other"))
        {
            let light = format!("section.lang-{slug}{{background:");
            assert!(
                INLINE_CSS.contains(&light),
                "missing light-mode CSS rule for slug {slug:?}: expected substring {light:?}"
            );
            assert!(
                dark_block.contains(&light),
                "missing dark-mode override for slug {slug:?}: expected substring {light:?} inside @media block"
            );
        }
    }

    #[test]
    fn tsx_section_uses_typescript_palette() {
        // `LANG::Tsx::name() == "tsx"` (#540), mapped to the
        // "typescript" tint by an explicit palette row, so a TSX-only
        // walk must end up tinted as typescript — not as a fabricated
        // `lang-tsx` (no such CSS rule) and not as the neutral
        // `lang-other` fallback.
        let entries = vec![
            make_summary("App.tsx", "src/App.tsx", SpaceKind::Unit, LANG::Tsx),
            make_summary("render", "src/App.tsx", SpaceKind::Function, LANG::Tsx),
        ];
        let out = generate_html_report(&entries, 5, SuppressionPolicy::Honor);
        assert!(
            out.contains("<section class=\"lang-section lang-typescript\">"),
            "Tsx must reuse the typescript palette class"
        );
        assert!(!out.contains("lang-tsx"));
        assert!(!out.contains("lang-section lang-other"));
    }

    #[test]
    fn per_language_sections_carry_palette_class() {
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
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
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
        // The per-language overview heading + table must not sit
        // inside a `<section class="lang-section …">`. We verify
        // structurally: the prefix from the start of the document
        // through the close of the overview table must contain zero
        // `<section class="lang-section` open tags. This catches both
        // a wrapping section opened before the heading AND one
        // opened between the heading and the table close — earlier
        // versions of this test only caught the former.
        let overview = out
            .find("<h2>Per-language overview</h2>")
            .expect("overview heading present");
        // Anchor on the table that immediately follows the heading
        // first, then find ITS closing tag — guards against a future
        // change introducing another `<table>` between heading and
        // overview, which would otherwise shrink the search window.
        let overview_table = overview
            + out[overview..]
                .find("<table")
                .expect("overview table present");
        let overview_end = overview_table
            + out[overview_table..]
                .find("</table>")
                .expect("overview table closes")
            + "</table>".len();
        assert!(
            !out[..overview_end].contains("<section class=\"lang-section"),
            "overview region must not be wrapped in a per-language tinted section"
        );

        // Actionable summaries live inside per-language sections by
        // design (one per language); ensure no fixture language fell
        // through to the neutral fallback class on a `<section>`.
        assert!(!out.contains("lang-section lang-other"));
    }

    /// Honoring suppression markers drops a function from its metric's
    /// HTML hotspot table by default, and `--no-suppress` re-includes it.
    /// Mirrors the markdown-side coverage for the parallel HTML renderer
    /// (issue #501).
    #[test]
    fn suppression_honored_by_default_and_bypassed_by_no_suppress() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 25.0;
        func.cognitive = 18.0;
        func.suppressed = SuppressionScope::Some(BTreeSet::from([MetricKind::Cyclomatic]));

        let honored = generate_html_report(
            &[unit_clone(&unit), func_clone(&func)],
            20,
            SuppressionPolicy::Honor,
        );
        // The CC table must not list the suppressed function, but the
        // cognitive table (a different metric) still does.
        let cc = html_section(&honored, "Cyclomatic Complexity Hotspots");
        assert!(
            !cc.contains(">hot<"),
            "suppressed function must be omitted from the CC table:\n{cc}"
        );
        let cog = html_section(&honored, "Cognitive Complexity Hotspots");
        assert!(
            cog.contains(">hot<"),
            "function suppressed only for cyclomatic stays in the Cognitive table:\n{cog}"
        );

        let audit = generate_html_report(&[unit, func], 20, SuppressionPolicy::Ignore);
        let cc_audit = html_section(&audit, "Cyclomatic Complexity Hotspots");
        assert!(
            cc_audit.contains(">hot<"),
            "--no-suppress must include the suppressed function:\n{cc_audit}"
        );
    }

    /// The cyclomatic summary note is a caption for its table: it tallies
    /// the same suppression-filtered set, never a function the table omits,
    /// and stays identical to the Markdown report. The raw CC count lives in
    /// the Actionable Summary instead.
    #[test]
    fn cc_summary_note_excludes_suppressed_functions() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut visible = make_summary("cool", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        visible.cyclomatic = 5.0;
        let mut hot = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hot.cyclomatic = 25.0;
        hot.suppressed = SuppressionScope::Some(BTreeSet::from([MetricKind::Cyclomatic]));

        let honored = generate_html_report(&[unit, visible, hot], 20, SuppressionPolicy::Honor);
        let cc = html_section(&honored, "Cyclomatic Complexity Hotspots");

        // The suppressed high-CC function is dropped from the table...
        assert!(
            !cc.contains(">hot<"),
            "suppressed function must be omitted from the CC table:\n{cc}"
        );
        // ...so the note's tallies cover only the visible functions: nothing
        // exceeds 10 and the max is 5, not the suppressed 25.
        assert!(
            cc.contains("CC &gt; 10: 0 functions"),
            "note must not count the suppressed CC>10 function:\n{cc}"
        );
        assert!(
            !cc.contains("Max: 25"),
            "note max must reflect only visible functions, not the suppressed 25:\n{cc}"
        );
    }

    fn unit_clone(s: &FunctionSummary) -> FunctionSummary {
        let mut c = make_summary(&s.name, &s.file, s.kind, s.language);
        c.suppressed = s.suppressed.clone();
        c
    }
    fn func_clone(s: &FunctionSummary) -> FunctionSummary {
        let mut c = make_summary(&s.name, &s.file, s.kind, s.language);
        c.cyclomatic = s.cyclomatic;
        c.cognitive = s.cognitive;
        c.suppressed = s.suppressed.clone();
        c
    }

    /// Slice the rendered HTML from `<h3>{title}</h3>` to the next `<h3>`
    /// (or `</section>`), so a per-table membership check does not match a
    /// name in a sibling table.
    fn html_section<'a>(html: &'a str, title: &str) -> &'a str {
        let needle = format!("<h3>{title}</h3>");
        let Some(start) = html.find(&needle) else {
            return "";
        };
        let rest = &html[start + needle.len()..];
        let end = rest
            .find("<h3>")
            .or_else(|| rest.find("</section>"))
            .unwrap_or(rest.len());
        &rest[..end]
    }

    /// Byte baseline for the whole HTML report over the shared rich,
    /// all-sections fixture (top-N truncation, per-metric suppression, a
    /// class-like). The hotspot-spec unification must leave this unchanged.
    #[test]
    fn snapshot_rich_report() {
        let out = generate_html_report(
            &crate::markdown_report::rich_fixture(),
            5,
            SuppressionPolicy::Honor,
        );
        insta::assert_snapshot!("html_report_rich", out);
    }

    #[test]
    fn snapshot_two_lang_report() {
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
        insta::assert_snapshot!("html_report_two_lang", out);
    }
}
