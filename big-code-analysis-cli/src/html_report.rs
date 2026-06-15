// bca: suppress-file(halstead, loc, nargs, nexits, nom)
// HTML report templating; thin per-language orchestrators delegating to small
// write_* helpers. File-level halstead/loc and summed nargs/nom/nexits are
// string-formatting-volume / many-fn aggregation artifacts — the many tiny
// early-returning write helpers (and the in-file test module) inflate the
// file-level nexits sum the same way they inflate the others.

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
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use big_code_analysis::{SpaceKind, SuppressionPolicy};

use crate::markdown_report::advisory::{AdvisoryCounts, AdvisoryThresholds};
use crate::markdown_report::hotspot::{self, Align, Cell, HotspotSpec, SPECS, Source};
use crate::markdown_report::{
    FunctionSummary, is_class_like, language_display_name, mi_rating, mi_weight_numerator,
    sloc_weighted_avg_mi, thousands,
};

/// HTML-escape a string for safe interpolation into element text or
/// double-quoted attribute values. Returns a borrowed `Cow` when the
/// input is already safe so the common case (most metric column names,
/// well-formed paths) allocates nothing.
pub(crate) fn escape_html(s: &str) -> Cow<'_, str> {
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

/// Slugify a heading basis into an HTML `id`/fragment-safe token:
/// lowercase ASCII, every run of non-`[a-z0-9]` characters collapsed to a
/// single `-`, with leading/trailing hyphens trimmed. Built from the raw
/// language *slug* (`"cpp"`, `"csharp"`) or a section title, never from a
/// display name carrying HTML-special punctuation — so `C++` deep-links to
/// `#cpp`, not a broken `#c++` fragment (issue #622).
///
/// An empty or all-separator basis yields `"section"` so the id is never
/// the empty string (which is not a valid fragment target).
fn slugify(basis: &str) -> String {
    let mut slug = String::with_capacity(basis.len());
    let mut prev_dash = false;
    for ch in basis.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "section".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// One `h2` section in the table of contents plus the `h3` subsections
/// nested under it (in document order). `(display_text, id)` for the
/// section, then a child list of the same for each `h3` that followed it
/// before the next `h2`.
#[derive(Default)]
struct TocSection {
    text: String,
    id: String,
    children: Vec<(String, String)>,
}

/// Collects heading ids during body rendering: assigns a unique, slug-based
/// `id` to every `h2`/`h3` (deduplicating collisions with a `-2`, `-3`, …
/// suffix so two languages' "Summary" headings never share a fragment), and
/// records the `h2` sections plus their nested `h3` subsections for the
/// table-of-contents `<nav>`.
///
/// The TOC nests each language's `h3` hotspot subsections under its `h2`
/// entry as a collapsible list (issue #685), reusing the per-language-unique
/// ids `unique_id` mints so a reader can jump straight to one hotspot table.
/// (The original `h2`-only "compact nav" — issue #622 — abandoned a reader to
/// scrolling once per-language sections grew this many subsections.)
#[derive(Default)]
pub(crate) struct Headings {
    /// Slug -> number of times already emitted, for `-N` de-duplication.
    seen: HashMap<String, usize>,
    /// One entry per `h2`, in document order, each carrying its `h3` children.
    toc: Vec<TocSection>,
}

impl Headings {
    /// Reserve a unique id for `basis`, recording the collision count so a
    /// repeated basis (e.g. each language's "Summary") gets `-2`, `-3`, ….
    fn unique_id(&mut self, basis: &str) -> String {
        let slug = slugify(basis);
        let count = self.seen.entry(slug.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            slug
        } else {
            format!("{slug}-{count}")
        }
    }

    /// Record a TOC `h3` child under the most recent `h2`. A stray `h3` with
    /// no preceding `h2` is dropped from the TOC (it cannot nest under
    /// anything) — this never happens in practice, since every `h3` the
    /// renderers emit sits inside a language `<section>` whose `h2` came
    /// first.
    fn push_child(&mut self, text: String, id: String) {
        if let Some(section) = self.toc.last_mut() {
            section.children.push((text, id));
        }
    }

    /// Emit `<h2 id="…">text</h2>` and open a new TOC section. `id_basis` is
    /// the slug source (the raw language slug or a section title);
    /// `display_text` is the already-`escape_html`-ed visible text.
    pub(crate) fn emit_h2(&mut self, out: &mut String, id_basis: &str, display_text: &str) {
        let id = self.unique_id(id_basis);
        let _ = write!(out, "<h2 id=\"{id}\">{display_text}</h2>");
        self.toc.push(TocSection {
            text: display_text.to_owned(),
            id,
            children: Vec::new(),
        });
    }

    /// Emit `<h3 id="…">text</h3>` and nest it under the current `h2` in the
    /// TOC. `id_basis` is the slug source; `display_text` is already
    /// `escape_html`-ed.
    pub(crate) fn emit_h3(&mut self, out: &mut String, id_basis: &str, display_text: &str) {
        let id = self.unique_id(id_basis);
        let _ = writeln!(out, "<h3 id=\"{id}\">{display_text}</h3>");
        self.push_child(display_text.to_owned(), id);
    }

    /// Emit `<hN id="…">text</hN>` at a caller-chosen `level` (used by the
    /// VCS report, whose bus-factor subsection sits at `h2` on the
    /// standalone page and `h3` when embedded). A level-2 heading opens a new
    /// TOC section; a level-3 heading nests under the current one, matching
    /// `emit_h2`/`emit_h3`. `display_text` is already `escape_html`-ed.
    pub(crate) fn emit_heading(
        &mut self,
        out: &mut String,
        level: usize,
        id_basis: &str,
        display_text: &str,
    ) {
        let id = self.unique_id(id_basis);
        let _ = writeln!(out, "<h{level} id=\"{id}\">{display_text}</h{level}>");
        if level == 2 {
            self.toc.push(TocSection {
                text: display_text.to_owned(),
                id,
                children: Vec::new(),
            });
        } else if level == 3 {
            self.push_child(display_text.to_owned(), id);
        }
    }
}

/// Emit the table-of-contents `<nav>` linking each collected `h2` section,
/// with its `h3` subsections nested in a collapsible `<details>` so a reader
/// can jump straight to one hotspot table (issue #685). Renders nothing when
/// there are no sections (the empty-walk report). All `text` is already
/// `escape_html`-ed and every `id` is slug-safe ASCII, so each entry resolves
/// to a real anchor on the page.
fn write_toc(out: &mut String, toc: &[TocSection]) {
    if toc.is_empty() {
        return;
    }
    let _ = out.write_str("<nav class=\"toc\" aria-label=\"Sections\">\n<ul>\n");
    for section in toc {
        if section.children.is_empty() {
            let _ = writeln!(
                out,
                "<li><a href=\"#{}\">{}</a></li>",
                section.id, section.text
            );
        } else {
            // A `<details>` keeps the nested list collapsed by default so the
            // nav stays compact, yet expandable — robust across print, mobile,
            // and narrow viewports (issue #685).
            let _ = out.write_str("<li><details>\n<summary>");
            let _ = write!(out, "<a href=\"#{}\">{}</a>", section.id, section.text);
            let _ = out.write_str("</summary>\n<ul>\n");
            for (text, id) in &section.children {
                let _ = writeln!(out, "<li><a href=\"#{id}\">{text}</a></li>");
            }
            let _ = out.write_str("</ul>\n</details></li>\n");
        }
    }
    let _ = out.write_str("</ul>\n</nav>\n");
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
nav.toc{font-size:0.9rem;margin:0.8rem 0;padding:0.5rem 0.8rem;\
background:#fff;border:1px solid #e5e5e5;border-radius:4px;\
box-shadow:0 1px 2px rgba(0,0,0,0.06)}\
nav.toc ul{margin:0.2rem 0 0.2rem 1.2rem}\
nav.toc li{font-size:0.9rem}\
nav.toc details>summary{cursor:pointer;list-style:revert}\
nav.toc details ul{margin-left:1rem}\
div.global-actionable{margin:0.8rem 0}\
div.global-actionable ul{margin:0.2rem 0 0.2rem 1.2rem}\
footer.provenance{margin-top:2rem;padding-top:0.5rem;border-top:1px solid #ccc;\
font-size:0.8rem;color:#666}\
p.sort-hint{font-size:0.85rem;color:#555;margin:0.4rem 0;font-style:italic}\
@media (prefers-color-scheme:dark){nav.toc{background:#1e1e1e;border-color:#333}}\
details.legend{font-size:0.85rem;color:#444;margin:0.8rem 0}\
details.legend summary{cursor:pointer;font-weight:600;color:#222}\
details.legend dl{margin:0.4rem 0 0 0}\
details.legend dt{font-weight:600;margin-top:0.3rem}\
details.legend dd{margin:0 0 0 1rem;color:#555}\
ul{margin:0.4rem 0 0.4rem 1.2rem;padding:0}\
li{margin:0.15rem 0;font-size:0.9rem}\
div.table-wrap{overflow-x:auto;margin-bottom:0.5rem}\
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
table.hotspot td.risk-heat-0,table.hotspot td.risk-heat-1,\
table.hotspot td.risk-heat-2,table.hotspot td.risk-heat-3,\
table.hotspot td.risk-heat-4{font-weight:600}\
table.hotspot tr:nth-child(even) td.risk-heat-0,table.hotspot td.risk-heat-0\
{background:#f4b8b0;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-1,table.hotspot td.risk-heat-1\
{background:#f6cba3;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-2,table.hotspot td.risk-heat-2\
{background:#f1e3a3;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-3,table.hotspot td.risk-heat-3\
{background:#cfe6b0;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-4,table.hotspot td.risk-heat-4\
{background:#bfe3c0;color:#222}\
@media (prefers-color-scheme:dark){\
body{color:#e0e0e0;background:#121212}\
a{color:#6aa3e0}\
h2{border-bottom-color:#333}\
h3{color:#bbb}\
.summary{color:#bbb}\
.summary strong{color:#e0e0e0}\
.note{color:#999}\
footer.provenance{color:#888;border-top-color:#333}\
p.sort-hint{color:#999}\
details.legend{color:#bbb}\
details.legend summary{color:#e0e0e0}\
details.legend dd{color:#999}\
table.hotspot{background:#1e1e1e}\
table.hotspot th,table.hotspot td{border-bottom-color:#333}\
table.hotspot th{background:#2a2a2a}\
table.hotspot th:hover{background:#383838}\
table.hotspot tr:nth-child(even) td{background:#232323}\
table.hotspot tr:nth-child(even) td.risk-heat-0,table.hotspot td.risk-heat-0\
{background:#7a1f17;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-1,table.hotspot td.risk-heat-1\
{background:#7a4a12;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-2,table.hotspot td.risk-heat-2\
{background:#6b5e12;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-3,table.hotspot td.risk-heat-3\
{background:#33591f;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-4,table.hotspot td.risk-heat-4\
{background:#1f5a2e;color:#f0f0f0}\
}\
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
/// is JavaScript, just a different grammar), and likewise `"mozcpp"`
/// reuses the `"cpp"` tint — none needs its own CSS rule.
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
    ("c", "cpp"),
    ("mozcpp", "cpp"),
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

/// Tooltips for the Per-language overview table's columns, which are NOT
/// part of the hotspot `SPECS` (they are per-language aggregates, not
/// per-function rows). The averaged CC / Cognitive columns reuse the
/// shared metric definitions from [`crate::markdown_report::hotspot`] so
/// "Avg CC" and the hotspot "CC" column can never describe the same metric
/// differently. "Avg MI" is the exception: it is SLOC-weighted and derived
/// from the *unclamped* MI, so it uses [`hotspot::AVG_MI_TOOLTIP`] rather
/// than the per-file `MI_TOOLTIP` (issue #725).
/// "Files" here means *source files analysed*; the bus-factor table's
/// like-named column means files-per-directory and supplies its own
/// tooltip (issue #610), so this entry never leaks onto it.
const AST_OVERVIEW_TOOLTIPS: &[(&str, &str)] = &[
    ("SLOC", hotspot::SLOC_TOOLTIP),
    ("Avg MI", hotspot::AVG_MI_TOOLTIP),
    ("Avg CC", hotspot::CC_TOOLTIP),
    ("Avg Cognitive", hotspot::COGNITIVE_TOOLTIP),
    ("Functions", "Number of functions and methods analysed."),
    ("Files", "Number of source files analysed."),
];

/// Plain-English tooltip for a metric column header, or `None` when the
/// header names a non-metric dimension (file, function, class, line,
/// language) or a column whose table supplies its own tooltips
/// out-of-band (the hotspot and change-history tables, which pass their
/// spec tooltips through [`write_table_with_tooltips`]).
///
/// This lookup serves only the tables that call the plain [`write_table`]
/// (the Per-language overview), so it consults [`AST_OVERVIEW_TOOLTIPS`].
/// The hotspot and VCS columns own their definitions on
/// [`crate::markdown_report::hotspot::Column`] /
/// [`crate::vcs_report::VcsColumn`] and never route through here.
fn header_tooltip(header: &str) -> Option<&'static str> {
    AST_OVERVIEW_TOOLTIPS
        .iter()
        .find_map(|&(name, tip)| (name == header).then_some(tip))
}

/// The column a table is pre-ranked by, and its sort direction's
/// `aria-sort` value (`"ascending"` / `"descending"`). The HTML renderer
/// emits `aria-sort` on that header's `<th>` so the initial sort order is
/// visible (the existing CSS arrow shows on first render) and announced to
/// screen readers, rather than appearing only after a click (issue #622).
#[derive(Clone, Copy)]
pub(crate) struct RankedColumn {
    pub(crate) index: usize,
    pub(crate) dir: &'static str,
}

/// Write a `<table class="hotspot">` with one `<thead>` and one
/// `<tbody>`. `aligns` controls per-cell text alignment AND the
/// `data-numeric="1"` attribute that the inline sort handler reads to
/// pick numeric vs string comparison.
///
/// Cell strings are escaped via [`escape_html`]; do not pre-escape.
pub(crate) fn write_table(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    rows: &[Vec<String>],
) {
    // No extra per-cell classes: every cell carries only its
    // alignment-derived `numeric` class. The overview table is unranked,
    // so no header announces an initial sort.
    write_table_classed(out, headers, aligns, rows, |_, _| None);
}

/// Like [`write_table`], but a `cell_class` callback can contribute an
/// extra CSS class (e.g. a severity-heat band) to a specific `<td>`,
/// keyed by `(row_index, column_index)`. The returned class is appended
/// after the alignment-derived `numeric` class on a single
/// space-separated `class="…"` attribute, so [`escape_html`] still runs
/// exactly once per cell and the attribute is never doubled.
pub(crate) fn write_table_classed(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    rows: &[Vec<String>],
    cell_class: impl Fn(usize, usize) -> Option<&'static str>,
) {
    // Resolve each header's tooltip via the string-keyed overview
    // catalogue (the only caller that takes this path is the Per-language
    // overview).
    write_table_core(out, headers, aligns, rows, None, cell_class, |_, h| {
        header_tooltip(h)
    });
}

/// Like [`write_table`], but each column's tooltip is supplied explicitly
/// by index rather than resolved from a string-keyed catalogue. This is
/// the path the hotspot and change-history tables take: their tooltip is
/// already authoritative on the shared column spec
/// ([`crate::markdown_report::hotspot::Column`] /
/// [`crate::vcs_report::VcsColumn`]), and passing it positionally avoids
/// the header-string ambiguity that made the bus-factor "Files" column
/// inherit the unrelated "source files analysed" definition (issue #610).
/// `tooltips[i]` is the `title=` text for column `i`; `None` leaves the
/// header bare. `ranked` names the column the table is already sorted by
/// (and the direction) so its `<th>` carries `aria-sort` at render time;
/// pass `None` for an unranked table.
pub(crate) fn write_table_with_tooltips(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    tooltips: &[Option<&str>],
    ranked: Option<RankedColumn>,
    rows: &[Vec<String>],
) {
    write_table_classed_with_tooltips(out, headers, aligns, tooltips, ranked, rows, |_, _| None);
}

/// [`write_table_with_tooltips`] plus a `cell_class` callback (the
/// severity-heat tint the change-history table paints on its risk cell).
pub(crate) fn write_table_classed_with_tooltips(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    tooltips: &[Option<&str>],
    ranked: Option<RankedColumn>,
    rows: &[Vec<String>],
    cell_class: impl Fn(usize, usize) -> Option<&'static str>,
) {
    debug_assert_eq!(headers.len(), tooltips.len());
    write_table_core(out, headers, aligns, rows, ranked, cell_class, |i, _| {
        tooltips.get(i).copied().flatten()
    });
}

/// Shared table body for the HTML renderers: `cell_class` contributes an
/// optional per-cell CSS class keyed by `(row, col)`, and `tooltip_for`
/// resolves each header's `title=` text by `(col_index, header)`.
fn write_table_core<'t>(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    rows: &[Vec<String>],
    ranked: Option<RankedColumn>,
    cell_class: impl Fn(usize, usize) -> Option<&'static str>,
    tooltip_for: impl Fn(usize, &str) -> Option<&'t str>,
) {
    debug_assert_eq!(headers.len(), aligns.len());
    // Wrap every table in an overflow-x scroll container so a wide table
    // (the 21-column VCS table especially) scrolls horizontally instead of
    // clipping its right-most metric columns past a narrow viewport (issue
    // #686). `white-space:nowrap` cells stay; the wrapper handles overflow.
    let _ = out.write_str("<div class=\"table-wrap\">\n<table class=\"hotspot\">\n<thead><tr>");
    for (i, (h, a)) in headers.iter().zip(aligns).enumerate() {
        let numeric_attr = if a.is_numeric() {
            " data-numeric=\"1\""
        } else {
            ""
        };
        let _ = write!(out, "<th{numeric_attr}");
        // Announce the pre-ranked column's initial sort order at render
        // time. The `dir` string comes from `SortDir::aria_sort`, so it is
        // always the literal `ascending`/`descending` the inline JS toggles
        // (no escaping needed); the click handler reads and replaces it on
        // first interaction (issue #622).
        if let Some(rank) = ranked
            && rank.index == i
        {
            let _ = write!(out, " aria-sort=\"{}\"", rank.dir);
        }
        if let Some(tip) = tooltip_for(i, h) {
            let _ = write!(out, " title=\"{}\"", escape_html(tip));
        }
        // Link the header text to its hosted metric chapter when one exists
        // (issue #675). The link sits inside the still-clickable `<th>`, so
        // clicking the padding sorts and clicking the term opens the docs;
        // non-metric headers (File, Rank, overview averages) render bare.
        match hotspot::metric_doc_url(h) {
            Some(url) => {
                let _ = write!(
                    out,
                    "><a href=\"{}\">{}</a></th>",
                    escape_html(&url),
                    escape_html(h)
                );
            }
            None => {
                let _ = write!(out, ">{}</th>", escape_html(h));
            }
        }
    }
    let _ = out.write_str("</tr></thead>\n<tbody>\n");
    for (r, row) in rows.iter().enumerate() {
        debug_assert_eq!(row.len(), headers.len());
        let _ = out.write_str("<tr>");
        for (c, (cell, a)) in row.iter().zip(aligns).enumerate() {
            // Emit the alignment class and any per-cell extra as one
            // space-separated attribute, so each `<td>` carries at most a
            // single `class="…"`. Keep the no-extra-class path (the common
            // case for every table, including the unheated AST report)
            // allocation-free with `&'static str` arms; only the heated
            // cells pay for a `format!`.
            let _ = match (a.is_numeric(), cell_class(r, c)) {
                (false, None) => write!(out, "<td>{}</td>", escape_html(cell)),
                (true, None) => write!(out, "<td class=\"numeric\">{}</td>", escape_html(cell)),
                (false, Some(extra)) => {
                    write!(out, "<td class=\"{extra}\">{}</td>", escape_html(cell))
                }
                (true, Some(extra)) => {
                    write!(
                        out,
                        "<td class=\"numeric {extra}\">{}</td>",
                        escape_html(cell)
                    )
                }
            };
        }
        let _ = out.write_str("</tr>\n");
    }
    let _ = out.write_str("</tbody>\n</table>\n</div>\n");
}

/// Render a hotspot table from the shared [`Column`] descriptors. Builds the
/// parallel `headers`/`aligns`/`rows` arrays and delegates to [`write_table`],
/// the single source of truth for table bytes and escaping. HTML escapes
/// every cell uniformly, so the raw [`Cell`] payload (regardless of kind) is
/// handed to `write_table`, whose `escape_html` runs exactly once per cell.
fn write_hotspot_table(out: &mut String, spec: &HotspotSpec, entries: &[&FunctionSummary]) {
    let columns = spec.columns;
    let mut headers = Vec::with_capacity(columns.len());
    let mut aligns = Vec::with_capacity(columns.len());
    let mut tooltips = Vec::with_capacity(columns.len());
    for col in columns {
        headers.push(col.header);
        aligns.push(col.align);
        tooltips.push(col.tooltip);
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
    // The column spec carries the authoritative tooltip; pass it
    // positionally so each header's `title=` matches the legend exactly.
    // The spec also names the pre-ranked column and its direction, so the
    // table announces its initial sort order without renderer guesswork.
    let ranked = RankedColumn {
        index: spec.rank_col,
        dir: spec.dir.aria_sort(),
    };
    write_table_with_tooltips(out, &headers, &aligns, &tooltips, Some(ranked), &rows);
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

/// Write the shared HTML preamble (doctype, head with the inline CSS, and
/// the opening `<h1>`). `title` fills the `<title>` element and `heading`
/// the `<h1>`; both are escaped. Shared by the AST report and the VCS
/// report (`crate::vcs_report`) so the two pages carry identical styling
/// and the sortable-table CSS.
pub(crate) fn write_html_head(out: &mut String, title: &str, heading: &str) {
    let _ = out.write_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    let _ = out.write_str("<meta charset=\"utf-8\">\n");
    // Without a viewport meta, mobile browsers render the page at desktop
    // width zoomed out to unreadable; the report is published to GitHub Pages
    // where mobile and split-screen reading are normal (issue #686).
    let _ =
        out.write_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(
        out,
        "<title>{} \u{2014} big-code-analysis</title>",
        escape_html(title)
    );
    let _ = writeln!(out, "<style>{INLINE_CSS}</style>");
    let _ = out.write_str("</head>\n<body>\n");
    let _ = writeln!(out, "<h1>{}</h1>", escape_html(heading));
}

fn write_global_summary(out: &mut String, totals: &GlobalTotals, by_lang: &LangGroups<'_>) {
    let languages_list: String = by_lang
        .keys()
        .map(|k| language_display_name(k))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(
        out,
        "<p><strong>Files analyzed:</strong> {} <strong>Languages:</strong> {}</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&languages_list),
    );
    // PLOC / Comments carry hover tooltips (the legend defines them too —
    // issue #679); SLOC's definition already rides the hotspot SLOC column.
    let _ = writeln!(
        out,
        "<p><strong>Total SLOC:</strong> {} \
         <strong title=\"{}\">PLOC:</strong> {} \
         <strong title=\"{}\">Comments:</strong> {}</p>",
        escape_html(&thousands(totals.sloc)),
        escape_html(hotspot::PLOC_TOOLTIP),
        escape_html(&thousands(totals.ploc)),
        escape_html(hotspot::COMMENTS_TOOLTIP),
        escape_html(&thousands(totals.cloc)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Functions/methods:</strong> {} <strong>Types:</strong> {}</p>",
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
    let mut mi_numerator = 0.0f64;
    let mut func_count = 0usize;
    let mut cc_sum = 0.0f64;
    let mut cog_sum = 0.0f64;
    for s in lang_summaries {
        match s.kind {
            SpaceKind::Unit => {
                unit_count += 1;
                sloc += s.sloc;
                mi_numerator += mi_weight_numerator(s);
            }
            SpaceKind::Function => {
                func_count += 1;
                cc_sum += s.cyclomatic;
                cog_sum += s.cognitive;
            }
            _ => {}
        }
    }
    let avg_mi = sloc_weighted_avg_mi(mi_numerator, sloc);
    let (avg_cc, avg_cog) = if func_count > 0 {
        (cc_sum / func_count as f64, cog_sum / func_count as f64)
    } else {
        (0.0, 0.0)
    };
    vec![
        language_display_name(lang_name).into_owned(),
        thousands(unit_count),
        thousands(sloc),
        thousands(func_count),
        format!("{avg_mi:.1}"),
        format!("{avg_cc:.1}"),
        format!("{avg_cog:.1}"),
    ]
}

fn write_overview_table(out: &mut String, headings: &mut Headings, by_lang: &LangGroups<'_>) {
    headings.emit_h2(out, "Per-language overview", "Per-language overview");
    let _ = out.write_str("\n");
    // One discoverability hint near the first table: the columns are
    // sortable, which is otherwise invisible until a reader happens to
    // click a header (issue #622).
    let _ = out.write_str("<p class=\"sort-hint\">Click a column header to sort any table.</p>\n");
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

pub(crate) fn write_html_tail(out: &mut String) {
    let _ = writeln!(out, "<script>{INLINE_JS}</script>");
    let _ = out.write_str("</body>\n</html>\n");
}

/// Produce a self-contained HTML quality-metrics report from the
/// collected summaries. `top_n` controls how many entries appear in
/// each hotspot table. Footer-free convenience wrapper used by the snapshot
/// tests; the command path goes through [`generate_html_report_with_vcs`]
/// with a provenance footer.
#[cfg(test)]
pub(crate) fn generate_html_report(
    summaries: &[FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) -> String {
    generate_html_report_with_vcs(
        summaries,
        top_n,
        policy,
        &AdvisoryThresholds::DEFAULT,
        None,
        None,
    )
}

/// As [`generate_html_report`], optionally inserting a "Change-history
/// risk" section (`bca report --vcs`) before the closing tail, rendered
/// through [`crate::vcs_report`], and an optional provenance footer (issue
/// #680). `prov` is `None` in the snapshot tests and `Some` from the command.
pub(crate) fn generate_html_report_with_vcs(
    summaries: &[FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
    advisory: &AdvisoryThresholds,
    vcs: Option<&crate::vcs_command::Report>,
    prov: Option<&crate::provenance::Provenance<'_>>,
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

    write_html_head(
        &mut out,
        "Code Quality Metrics Summary",
        "Code Quality Metrics Summary",
    );
    write_global_summary(&mut out, &totals, &by_lang);

    // Render the body into a side buffer so the heading ids it assigns can
    // be collected into the table-of-contents `<nav>`, which must precede
    // the body. `headings` owns both the slug-dedup state and the collected
    // `h2` TOC entries (issue #622).
    let mut headings = Headings::default();
    let mut body = String::with_capacity(out.capacity());
    if !by_lang.is_empty() {
        write_overview_table(&mut body, &mut headings, &by_lang);
        for (&lang_name, lang_summaries) in &by_lang {
            write_language_section(
                &mut body,
                &mut headings,
                lang_name,
                lang_summaries,
                top_n,
                policy,
                *advisory,
            );
        }
        // A visible legend so the column definitions survive print, mobile,
        // and screen readers — the `title=` tooltips are hover-only and
        // invisible to all three (issue #611). Includes the global-header
        // stat definitions (PLOC / Comments / Comment ratio) so the legend
        // defines every number on the page (issue #679).
        write_legend_html(
            &mut body,
            &crate::markdown_report::legend_entries_with_header_stats(),
        );
    }
    if let Some(report) = vcs {
        crate::vcs_report::push_html_section(&mut body, &mut headings, report);
    }
    // Provenance footer, inside the body so it precedes the closing tail and
    // the document stays well-formed (issue #680).
    if let Some(prov) = prov {
        crate::provenance::push_html_footer(&mut body, prov);
    }

    write_toc(&mut out, &headings.toc);
    // A global cross-language roll-up of the Actionable Summary near the TOC,
    // so a multi-language report gives one top-of-page signal before the
    // per-language sections (issue #678).
    if !by_lang.is_empty() {
        write_global_actionable_summary(&mut out, summaries, *advisory);
    }
    out.push_str(&body);
    write_html_tail(&mut out);
    out
}

/// Render the global cross-language Actionable Summary near the TOC: the
/// per-language counts summed over every function in the walk (issue #678).
/// Suppression policy does not gate these counts — like the per-language
/// Actionable Summary, this is a raw whole-codebase health signal, not a
/// gate (issue #501). Renders the "no concerns" line when nothing clears a
/// threshold, mirroring the per-language block.
fn write_global_actionable_summary(
    out: &mut String,
    summaries: &[FunctionSummary],
    advisory: AdvisoryThresholds,
) {
    let funcs: Vec<&FunctionSummary> = summaries
        .iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .collect();
    let counts = advisory.count_over(&funcs);
    let _ = out.write_str(
        "<div class=\"summary global-actionable\">\n<p><strong>Actionable summary \
         (all languages)</strong></p>\n",
    );
    // Provenance so the global roll-up's cutoffs are attributable (issue #630).
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(advisory.provenance_line())
    );
    if counts.all_clear() {
        let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        let _ = out.write_str("</div>\n");
        return;
    }
    write_actionable_bullets(out, counts, advisory);
    let _ = out.write_str("</div>\n");
}

/// Emit a visible legend listing each metric column's abbreviation and its
/// one-line definition, drawn from the same `(header, tooltip)` pairs the
/// hover tooltips use so the two cannot drift. The `<details>` is rendered
/// **open** — the legend's whole purpose is to survive print, mobile, and
/// screen readers, all of which a collapsed `<details>` defeats just as the
/// hover tooltips do (issue #679); the page bottom is cheap vertical space.
/// Renders nothing when `entries` is empty.
pub(crate) fn write_legend_html(out: &mut String, entries: &[(&str, &str)]) {
    if entries.is_empty() {
        return;
    }
    let _ = out.write_str("<details class=\"legend\" open>\n<summary>Legend</summary>\n<dl>\n");
    for (header, tip) in entries {
        // Link the term to its hosted metric chapter when one exists (issue
        // #675); VCS / identity headers have no anchor and render bare.
        let term = match hotspot::metric_doc_url(header) {
            Some(url) => format!(
                "<a href=\"{}\">{}</a>",
                escape_html(&url),
                escape_html(header)
            ),
            None => escape_html(header).into_owned(),
        };
        let _ = writeln!(out, "<dt>{term}</dt><dd>{}</dd>", escape_html(tip));
    }
    let _ = out.write_str("</dl>\n</details>\n");
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
    /// SLOC-weighted numerator of the unclamped Visual Studio MI: the sum
    /// of [`mi_weight_numerator`] over the units. Divided by `sloc` (not
    /// `files`) to form the headline average (issue #725).
    mi_numerator: f64,
}

impl LanguageTotals {
    fn from_units(units: &[&FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            mi_numerator: 0.0,
        };
        for s in units {
            t.files += 1;
            t.sloc += s.sloc;
            t.ploc += s.ploc;
            t.cloc += s.cloc;
            t.mi_numerator += mi_weight_numerator(s);
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        comment_ratio_percent(self.sloc, self.cloc)
    }

    fn avg_mi(&self) -> f64 {
        sloc_weighted_avg_mi(self.mi_numerator, self.sloc)
    }
}

fn write_language_header(out: &mut String, headings: &mut Headings, lang_name: &str) {
    let display_name = language_display_name(lang_name);
    // `slug` is sourced from `LANGUAGE_PALETTE` (or the literal "other"
    // fallback) — always lowercase ASCII, so it is interpolated raw
    // into the class attribute without `escape_html`.
    let slug = language_palette_slug(lang_name);
    let _ = write!(out, "<section class=\"lang-section lang-{slug}\">");
    // The heading id is slug-based off the raw language name (`lang_name`,
    // e.g. "cpp"/"csharp"), NOT the display name — so `C++` deep-links to a
    // valid `#cpp` fragment rather than the punctuation-laden display text.
    headings.emit_h2(out, lang_name, &escape_html(&display_name));
    let _ = out.write_str("\n");
}

fn write_language_summary(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    units: &[&FunctionSummary],
) {
    let totals = LanguageTotals::from_units(units);
    let cr = totals.comment_ratio();
    let avg_mi = totals.avg_mi();
    let rating = mi_rating(avg_mi);

    headings.emit_h3(out, &format!("{id_prefix}-summary"), "Summary");
    let _ = writeln!(
        out,
        "<p class=\"note\">Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&thousands(totals.sloc)),
        escape_html(&thousands(totals.ploc)),
    );
    let _ = writeln!(
        out,
        "<p class=\"note\">{}: {avg_mi:.1} ({rating})</p>",
        crate::markdown_report::AVG_MI_LABEL
    );
}

/// Emit one hotspot section: an `<h3>` heading followed by the column-driven
/// table, from a shared [`HotspotSpec`] and its already-selected `rows`. The
/// logical title is `escape_html`-ed here defensively (a `>` in a `concept`
/// would become `&gt;`); since no current concept carries one (see
/// [`hotspot::HotspotTitle::render`]), it is a no-op for today's
/// metachar-free titles.
fn emit_html_section(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    spec: &HotspotSpec,
    top_n: usize,
    rows: &[&FunctionSummary],
) {
    let title = spec.title.render(top_n);
    // The id basis is the language slug plus the section's *stable* basis
    // ("<Concept> hotspots", without the `(top N by …)` clause), so the
    // fragment (`#rust-cyclomatic-complexity-hotspots`) does not shift with
    // `--top` (issue #677). The displayed text is the full rendered title.
    headings.emit_h3(
        out,
        &format!("{id_prefix}-{}", spec.title.id_basis()),
        &escape_html(&title),
    );
    write_hotspot_table(out, spec, rows);
}

/// The cyclomatic summary note under the CC hotspot table. A caption over the
/// same suppression-filtered set the table shows (see [`hotspot::select_cc`]),
/// matching the Markdown report; the raw, suppression-independent CC count
/// lives in the Actionable Summary instead. When `policy` honors suppression
/// the line is captioned `(excluding suppressed functions)` so a reader can
/// tell the two CC figures apart (issue #616).
fn emit_cc_note_html(
    out: &mut String,
    stats: &hotspot::CyclomaticStats,
    policy: SuppressionPolicy,
) {
    let caption = hotspot::cc_note_caption(policy)
        .map(|c| format!(" ({})", escape_html(c)))
        .unwrap_or_default();
    // Bands use the resolved advisory CC cutoff and its severe multiple
    // (issue #630): `> 10` / `> 20` by default, shifted by a manifest
    // `cyclomatic = N`.
    let _ = writeln!(
        out,
        "<p class=\"note\">Average CC: {:.1} | Max: {:.0} | CC &gt; {:.0}: {} functions | CC &gt; {:.0}: {} functions{caption}</p>",
        stats.avg(),
        stats.max,
        stats.primary_cutoff,
        stats.over_primary,
        stats.severe_cutoff,
        stats.over_severe,
    );
}

/// Emit the `<ul>` of advisory bullets shared by the per-language and global
/// Actionable Summaries, the resolved advisory cutoff embedded in each label
/// (issue #630). The caller has already emitted the heading / provenance /
/// caption and confirmed `!counts.all_clear()`.
fn write_actionable_bullets(
    out: &mut String,
    counts: AdvisoryCounts,
    advisory: AdvisoryThresholds,
) {
    let _ = out.write_str("<ul>\n");
    if counts.cc > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with CC &gt; {:.0}</li>",
            counts.cc, advisory.cc
        );
    }
    if counts.cognitive > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with cognitive complexity &gt; {:.0}</li>",
            counts.cognitive, advisory.cognitive
        );
    }
    if counts.sloc > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with SLOC &gt; {}</li>",
            counts.sloc, advisory.sloc
        );
    }
    if counts.nargs > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with more than {} parameters</li>",
            counts.nargs, advisory.nargs
        );
    }
    if counts.bugs > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with estimated Halstead bugs &gt; {:.1}</li>",
            counts.bugs, advisory.bugs
        );
    }
    let _ = out.write_str("</ul>\n");
}

fn write_actionable_summary(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    funcs: &[&FunctionSummary],
    policy: SuppressionPolicy,
    advisory: AdvisoryThresholds,
) {
    let counts = advisory.count_over(funcs);
    headings.emit_h3(
        out,
        &format!("{id_prefix}-actionable-summary"),
        "Actionable Summary",
    );
    // Provenance so the cutoffs are always attributable (issue #630).
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(advisory.provenance_line())
    );
    let breakdown = hotspot::suppressed_metric_breakdown(funcs, policy, advisory);
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(&hotspot::actionable_summary_caption(&breakdown))
    );
    if counts.all_clear() {
        let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        return;
    }
    write_actionable_bullets(out, counts, advisory);
}

/// Emit the section heading (`<h3>` + id) followed by the "table omitted: all
/// N matching functions suppressed" caption, in place of a hotspot table that
/// was rendered empty *solely because* suppression hid every matching row
/// (`count > 0`). The heading uses the same id basis `emit_html_section` does,
/// so deep links resolve regardless of suppression state (issue #681).
/// Mirrors the Markdown renderer's `emit_fully_suppressed_note_md` so a
/// summary bullet never points at a table absent from the document (issue
/// #616). A no-op when `count == 0`.
fn emit_fully_suppressed_note_html(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    spec: &HotspotSpec,
    top_n: usize,
    count: usize,
) {
    if count == 0 {
        return;
    }
    let title = spec.title.render(top_n);
    // Emit the section heading with the same stable id basis
    // `emit_html_section` uses, so a fully-suppressed section keeps its place
    // in the heading/id sequence and deep links resolve regardless of
    // suppression state (issue #681). The omission note is the section body.
    headings.emit_h3(
        out,
        &format!("{id_prefix}-{}", spec.title.id_basis()),
        &escape_html(&title),
    );
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(&hotspot::fully_suppressed_caption(&title, count))
    );
}

fn write_language_section(
    out: &mut String,
    headings: &mut Headings,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
    advisory: AdvisoryThresholds,
) {
    write_language_header(out, headings, lang_name);
    // Slug-based id prefix for this language's `h3` headings, so a hotspot
    // table deep-links to e.g. `#rust-cyclomatic-complexity-hotspots`. Built
    // from the raw language slug, matching the `h2` section id.
    let id_prefix = slugify(lang_name);
    let (units, funcs) = partition_by_kind(entries);
    write_language_summary(out, headings, &id_prefix, &units);

    // The Actionable Summary leads the section (directly after Summary,
    // before any hotspot table) so a reader sees the highest-altitude counts
    // first (issue #678).
    write_actionable_summary(out, headings, &id_prefix, &funcs, policy, advisory);

    // Drive every hotspot section from the shared `SPECS` table so the HTML
    // and Markdown reports cannot diverge in membership/order/suppression.
    // WMC draws from the full slice, MI from units, the rest from functions.
    for spec in SPECS {
        let base: &[&FunctionSummary] = match spec.source {
            Source::Units => &units,
            Source::Funcs => &funcs,
            Source::All => entries,
        };
        if spec.cc_note {
            let (rows, stats) = hotspot::select_cc(spec, base, top_n, policy, advisory);
            if rows.is_empty() {
                // Mirror the non-CC branch: a CC table emptied purely by
                // suppression earns the same "table omitted" caption, or the
                // Actionable Summary's raw CC bullets would dangle (#616).
                let suppressed = hotspot::fully_suppressed_count(spec, base, policy, advisory);
                emit_fully_suppressed_note_html(out, headings, &id_prefix, spec, top_n, suppressed);
            } else {
                emit_html_section(out, headings, &id_prefix, spec, top_n, &rows);
                emit_cc_note_html(out, &stats, policy);
            }
        } else {
            let rows = hotspot::select(spec, base, top_n, policy, advisory);
            if rows.is_empty() {
                // An empty table here is either a genuinely-absent metric or a
                // table whose every matching row was suppressed; only the
                // latter earns a caption so a summary bullet never dangles.
                let suppressed = hotspot::fully_suppressed_count(spec, base, policy, advisory);
                emit_fully_suppressed_note_html(out, headings, &id_prefix, spec, top_n, suppressed);
            } else {
                emit_html_section(out, headings, &id_prefix, spec, top_n, &rows);
                if spec.mi_note {
                    let _ = writeln!(
                        out,
                        "<p class=\"note\">{}</p>",
                        escape_html(hotspot::MI_NOTE)
                    );
                }
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
    use big_code_analysis::{LANG, Metric};

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
            // 85.5 * 100/171 = 50.0, so the unclamped Visual Studio value
            // matches the displayed `mi_visual_studio` and the SLOC-weighted
            // headline average stays a clean 50.0 (issue #725).
            mi_original: 85.5,
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

    // The shipped snapshot fixtures only cover Rust and Python, whose
    // display names equal their title-cased slug, so they cannot catch a
    // regression in the slug→display mapping (#613). Render a language
    // whose human name carries punctuation the slug strips (C++) and pin
    // both surfaces: the heading must show the display name, while the
    // CSS class must keep the machine slug.
    #[test]
    fn cpp_heading_uses_display_name_class_keeps_slug() {
        let summaries = vec![
            make_summary("lib.cpp", "src/lib.cpp", SpaceKind::Unit, LANG::Cpp),
            make_summary("compute", "src/lib.cpp", SpaceKind::Function, LANG::Cpp),
        ];
        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);
        assert!(
            out.contains(">C++</h2>"),
            "heading should render the display name, got:\n{out}"
        );
        // The heading id must be the slug ("cpp"), NOT a fragment derived
        // from the punctuation-laden display name "C++" (issue #622).
        assert!(
            out.contains("<h2 id=\"cpp\">C++</h2>"),
            "C++ heading id must be the slug, not the display text, got:\n{out}"
        );
        assert!(
            !out.contains("id=\"c\""),
            "the `+` chars must not collapse the id to a bare `c`, got:\n{out}"
        );
        // The TOC entry links to the slug fragment.
        assert!(
            out.contains("<a href=\"#cpp\">C++</a>"),
            "TOC must link the C++ section by slug fragment, got:\n{out}"
        );
        assert!(
            out.contains("lang-cpp"),
            "CSS class must keep the machine slug, got:\n{out}"
        );
        assert!(
            out.contains("<strong>Languages:</strong> C++"),
            "Languages line should use the display name, got:\n{out}"
        );
        // The raw title-cased slug must not leak into any heading.
        assert!(!out.contains(">Cpp</h2>"), "raw slug leaked into heading");
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
        assert!(out.contains(">Rust</h2>"));
        assert!(out.contains("class=\"hotspot\""));
        assert_html_well_formed(&out);
    }

    #[test]
    fn two_language_well_formed_and_alphabetical() {
        let out = generate_html_report(&two_lang_fixture(), 20, SuppressionPolicy::Honor);
        assert!(out.contains(">Python</h2>"));
        assert!(out.contains(">Rust</h2>"));
        let py = out.find(">Python</h2>").expect("python heading");
        let rs = out.find(">Rust</h2>").expect("rust heading");
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
            .split_once(">Cyclomatic complexity hotspots (top 5 by CC)</h3>")
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
            .split_once(">Type hotspots (top 20 by WMC)</h3>")
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

    /// Issue #680: with provenance supplied (the command path), the HTML
    /// report carries a `<footer class="provenance">` inside the body (before
    /// the closing tail, so the document stays well-formed) naming the same
    /// facts as the Markdown footer. The footer-free path emits none.
    #[test]
    fn provenance_footer_inside_body_when_supplied() {
        let s = two_lang_fixture();
        let prov = crate::provenance::Provenance {
            version: "9.9.9",
            date: "2026-06-11",
            paths: "src/",
            top: 0,
            policy: SuppressionPolicy::Ignore,
        };
        let out = generate_html_report_with_vcs(
            &s,
            0,
            SuppressionPolicy::Ignore,
            &AdvisoryThresholds::DEFAULT,
            None,
            Some(&prov),
        );
        assert!(out.contains("<footer class=\"provenance\">"));
        assert!(out.contains(
            "Generated by bca 9.9.9 on 2026-06-11 over src/ \u{2014} all entries per table, \
             suppression markers ignored."
        ));
        // The footer precedes the closing tail (well-formed document).
        let footer_at = out.find("<footer class=\"provenance\">").expect("footer");
        let body_close = out.find("</body>").expect("</body>");
        assert!(footer_at < body_close, "footer must sit inside <body>");
        assert_html_well_formed(&out);

        let plain = generate_html_report(&s, 20, SuppressionPolicy::Honor);
        assert!(!plain.contains("<footer class=\"provenance\">"));
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
        // The "Args" table is gated on nargs > 3 and the exit-points table on
        // nexits > 2 (issue #689); bump one function so both sections (and
        // thus their Args / Exits headers) actually render.
        summaries[1].nargs = 5;
        summaries[1].nexits = 3;
        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);

        // Drive the loop from the shared sources so a new tooltip is
        // required to appear in real output without anyone remembering to
        // update the test. The hotspot metric columns own their tooltip on
        // the spec (`hotspot::legend_entries`); the Per-language overview's
        // averaged / count columns supply theirs via `AST_OVERVIEW_TOOLTIPS`.
        // `needle` embeds the rendered title directly, so a divergence
        // between the spec tooltip and the `title=` attribute surfaces here.
        for (header, tip) in hotspot::legend_entries()
            .into_iter()
            .chain(AST_OVERVIEW_TOOLTIPS.iter().copied())
        {
            // A metric header now links its hosted chapter (issue #675), so
            // its text is wrapped in an `<a>`; a non-metric overview header
            // (Avg CC, …) stays bare. Either way the `title=` tooltip
            // precedes it.
            let needle = match hotspot::metric_doc_url(header) {
                Some(url) => format!(
                    " title=\"{}\"><a href=\"{}\">{header}</a></th>",
                    escape_html(tip),
                    escape_html(&url)
                ),
                None => format!(" title=\"{}\">{header}</th>", escape_html(tip)),
            };
            assert!(
                out.contains(&needle),
                "header {header:?} should render with title attribute; expected substring {needle:?}"
            );
        }

        // Non-metric labels must remain bare so click-to-sort UX is not
        // crowded with redundant tooltips for self-describing columns.
        for plain in ["File", "Function", "Type", "Line", "Language"] {
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
    fn html_report_renders_visible_legend() {
        // Issue #611/#679: the `title=` tooltips are hover-only (invisible in
        // print, on mobile, and to screen readers), so the report also emits
        // a visible legend, rendered `<details ... open>` so it survives those
        // same surfaces a collapsed block would defeat. It draws from the same
        // `legend_entries_with_header_stats` the tooltips use, so the
        // definition a reader sees on hover and the one in the legend cannot
        // diverge.
        let summaries = rust_fixture();
        let out = generate_html_report(&summaries, 20, SuppressionPolicy::Honor);
        assert!(
            out.contains("<details class=\"legend\" open>"),
            "visible legend block must be present and open (issue #679)"
        );
        // The header-stat definitions appear in the legend, each linking the
        // Lines-of-Code chapter (issue #679/#675).
        for header in ["PLOC", "Comments", "Comment ratio"] {
            let url = hotspot::metric_doc_url(header).expect("header stat has a doc anchor");
            assert!(
                out.contains(&format!(
                    "<dt><a href=\"{}\">{header}</a></dt>",
                    escape_html(&url)
                )),
                "legend missing linked header-stat entry {header:?}"
            );
        }
        for (header, tip) in hotspot::legend_entries() {
            // Only assert entries whose column the fixture actually renders;
            // every metric column the rust fixture exercises must define
            // itself in the legend. Metric terms link their chapter (#675), so
            // both the `<th>` render-check and the `<dt>` term carry the link.
            let (th_needle, dt_term) = match hotspot::metric_doc_url(header) {
                Some(url) => (
                    format!("<a href=\"{}\">{header}</a></th>", escape_html(&url)),
                    format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(&url),
                        escape_html(header)
                    ),
                ),
                None => (format!(">{header}</th>"), escape_html(header).into_owned()),
            };
            let dt = format!("<dt>{dt_term}</dt><dd>{}</dd>", escape_html(tip));
            if out.contains(&th_needle) {
                assert!(out.contains(&dt), "legend missing entry for {header:?}");
            }
        }
    }

    #[test]
    fn language_palette_slug_known_and_fallback() {
        assert_eq!(language_palette_slug("rust"), "rust");
        assert_eq!(language_palette_slug("python"), "python");
        // Since #540 the input is the canonical slug ("cpp" / "csharp"),
        // not the dropped pretty forms.
        assert_eq!(language_palette_slug("cpp"), "cpp");
        // The dedicated C language (#721) and the Mozilla C++ fork (#720)
        // both reuse the "cpp" tint via explicit rows.
        assert_eq!(language_palette_slug("c"), "cpp");
        assert_eq!(language_palette_slug("mozcpp"), "cpp");
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
            out.contains("<section class=\"lang-section lang-rust\"><h2 id=\"rust\">Rust</h2>"),
            "Rust section must carry stable lang-rust palette class and slug heading id"
        );
        assert!(
            out.contains(
                "<section class=\"lang-section lang-python\"><h2 id=\"python\">Python</h2>"
            ),
            "Python section must carry stable lang-python palette class and slug heading id"
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
            .find(">Per-language overview</h2>")
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
        func.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let honored = generate_html_report(
            &[unit_clone(&unit), func_clone(&func)],
            20,
            SuppressionPolicy::Honor,
        );
        // The CC table must not list the suppressed function, but the
        // cognitive table (a different metric) still does.
        let cc = html_section(&honored, "Cyclomatic complexity hotspots (top 20 by CC)");
        assert!(
            !cc.contains(">hot<"),
            "suppressed function must be omitted from the CC table:\n{cc}"
        );
        let cog = html_section(
            &honored,
            "Cognitive complexity hotspots (top 20 by Cognitive)",
        );
        assert!(
            cog.contains(">hot<"),
            "function suppressed only for cyclomatic stays in the Cognitive table:\n{cog}"
        );

        let audit = generate_html_report(&[unit, func], 20, SuppressionPolicy::Ignore);
        let cc_audit = html_section(&audit, "Cyclomatic complexity hotspots (top 20 by CC)");
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
        hot.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let honored = generate_html_report(&[unit, visible, hot], 20, SuppressionPolicy::Honor);
        let cc = html_section(&honored, "Cyclomatic complexity hotspots (top 20 by CC)");

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

    /// Issue #616 (HTML twin of the Markdown test): the CC note carries the
    /// "excluding suppressed functions" caption, the Actionable Summary names
    /// the suppressed count, and a hotspot table emptied solely by suppression
    /// emits a "table omitted" caption instead of vanishing.
    #[test]
    fn cc_statistics_are_captioned_by_population() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut visible = make_summary("visible", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        visible.cyclomatic = 12.0;
        // Suppressed for both cyclomatic (drops it from the CC note) and nargs
        // (empties the lone many-parameters table).
        let mut hidden = make_summary("hidden", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hidden.cyclomatic = 30.0;
        hidden.nargs = 7;
        hidden.suppressed =
            SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic, Metric::Nargs]));

        let report = generate_html_report(&[unit, visible, hidden], 20, SuppressionPolicy::Honor);

        assert!(
            report.contains(
                "CC &gt; 10: 1 functions | CC &gt; 20: 0 functions (excluding suppressed functions)"
            ),
            "HTML CC note must be captioned and exclude the suppressed cc=30:\n{report}"
        );
        assert!(
            report.contains(
                "Raw counts across all functions; the hotspot tables hide suppressed \
                 rows (cyclomatic: 1, nargs: 1) \u{2014} re-run with --no-suppress to list them."
            ),
            "HTML Actionable Summary must caption its raw population per metric:\n{report}"
        );
        assert!(
            report.contains("table omitted: all 1 matching functions suppressed"),
            "a fully-suppressed HTML table must leave an explanatory caption:\n{report}"
        );
        // Issue #681: the heading + deep-link id are still emitted so the
        // section keeps its place in the heading/id sequence; only the rows
        // (the `hidden` function) are absent from this section's body.
        assert!(
            report.contains(
                "<h3 id=\"rust-many-parameters-hotspots\">\
                 Many parameters hotspots (top 20 by Args)</h3>"
            ),
            "a fully-suppressed many-parameters table must still emit its heading + id:\n{report}"
        );
        let nargs_section = html_section(&report, "Many parameters hotspots (top 20 by Args)");
        assert!(
            !nargs_section.contains("<td>hidden</td>"),
            "the all-suppressed many-parameters section must not render a row:\n{nargs_section}"
        );
    }

    /// HTML twin of the Markdown `fully_suppressed_cc_table_is_captioned`
    /// test: the CC hotspot table (cc_note branch) emptied solely by
    /// suppression must emit the "table omitted" caption, not vanish (#616).
    #[test]
    fn fully_suppressed_cc_table_is_captioned_html() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut hot = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hot.cyclomatic = 25.0;
        hot.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let report = generate_html_report(&[unit, hot], 20, SuppressionPolicy::Honor);
        // Issue #681: the CC heading + deep-link id are still emitted; only
        // the CC table's rows are hidden (the function is suppressed solely
        // for cyclomatic, so it still appears in other sections).
        assert!(
            report.contains(
                "<h3 id=\"rust-cyclomatic-complexity-hotspots\">\
                 Cyclomatic complexity hotspots (top 20 by CC)</h3>"
            ),
            "a fully-suppressed CC table must still emit its heading + id:\n{report}"
        );
        let cc_section = html_section(&report, "Cyclomatic complexity hotspots (top 20 by CC)");
        assert!(
            !cc_section.contains("<td>hot</td>"),
            "the all-suppressed CC section must not render a table row:\n{cc_section}"
        );
        assert!(
            cc_section.contains("table omitted: all 1 matching functions suppressed"),
            "the fully-suppressed CC section's body is the omission caption:\n{cc_section}"
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

    /// Slice the rendered HTML from `<h3 id="…">{title}</h3>` to the next
    /// `<h3>` (or `</section>`), so a per-table membership check does not
    /// match a name in a sibling table. Headings carry a slug id (issue
    /// #622), so the needle matches the heading text+close, not the bare
    /// open tag.
    fn html_section<'a>(html: &'a str, title: &str) -> &'a str {
        let needle = format!(">{title}</h3>");
        let Some(start) = html.find(&needle) else {
            return "";
        };
        let rest = &html[start + needle.len()..];
        let end = rest
            .find("<h3")
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

    /// Issue #725 (HTML side): the per-language Summary headline averages
    /// the unclamped Visual Studio MI, SLOC-weighted, so a catastrophic
    /// file whose displayed value clamps to 0 pulls the headline negative
    /// instead of contributing 0. Mirrors the Markdown-side regression in
    /// `markdown_report::tests::avg_mi_is_sloc_weighted_over_unclamped_values`.
    #[test]
    fn avg_mi_headline_is_sloc_weighted_and_unclamped() {
        let mut bad = make_summary("bad.rs", "src/bad.rs", SpaceKind::Unit, LANG::Rust);
        bad.sloc = 400;
        bad.mi_original = -342.0; // unclamped VS = -200.0
        bad.mi_visual_studio = 0.0;
        let mut good = make_summary("good.rs", "src/good.rs", SpaceKind::Unit, LANG::Rust);
        good.sloc = 10;
        good.mi_original = 171.0; // unclamped VS = 100.0
        good.mi_visual_studio = 100.0;

        let out = generate_html_report(&[bad, good], 20, SuppressionPolicy::Honor);
        let note = out
            .lines()
            .find(|l| l.contains("class=\"note\"") && l.contains("Average MI (SLOC-weighted)"))
            .expect("headline note present");
        // -79_000 / 410 ≈ -192.7. Pinning the exact rendered value (not just
        // "negative + LOW") guards SLOC-weighting on the HTML render path:
        // an unweighted mean would be (-200 + 100) / 2 = -50.0, also negative
        // and LOW, so a looser check would not catch dropping the weight.
        assert!(
            note.contains("-192.7 (LOW)"),
            "headline must render the SLOC-weighted -192.7 (LOW):\n{note}"
        );
    }

    /// Collect every `<tag attr1="…" id="VALUE" …>` `id` attribute in
    /// document order. A minimal scanner sufficient for the well-formed,
    /// double-quoted attributes this renderer emits (no single quotes, no
    /// unquoted values) — it is not a general HTML parser.
    fn collect_ids(html: &str) -> Vec<String> {
        let mut ids = Vec::new();
        let mut rest = html;
        while let Some(pos) = rest.find(" id=\"") {
            rest = &rest[pos + " id=\"".len()..];
            if let Some((value, tail)) = rest.split_once('"') {
                ids.push(value.to_string());
                rest = tail;
            } else {
                break;
            }
        }
        ids
    }

    /// Collect every TOC `href="#FRAGMENT"` target (fragment without the `#`).
    fn collect_href_fragments(html: &str) -> Vec<String> {
        let mut frags = Vec::new();
        let mut rest = html;
        while let Some(pos) = rest.find("href=\"#") {
            rest = &rest[pos + "href=\"#".len()..];
            if let Some((value, tail)) = rest.split_once('"') {
                frags.push(value.to_string());
                rest = tail;
            } else {
                break;
            }
        }
        frags
    }

    #[test]
    fn slugify_is_fragment_safe_and_never_empty() {
        assert_eq!(
            slugify("Cyclomatic Complexity Hotspots"),
            "cyclomatic-complexity-hotspots"
        );
        // Punctuation collapses to single hyphens, trimmed at the ends.
        assert_eq!(
            slugify("Functions With Many Parameters (>3)"),
            "functions-with-many-parameters-3"
        );
        // Display names with HTML-special chars must NOT survive into a
        // fragment — they slugify to plain ASCII (issue #622).
        assert_eq!(slugify("C++"), "c");
        assert_eq!(slugify("C#"), "c");
        // An all-separator / empty basis yields a valid non-empty fragment.
        assert_eq!(slugify("()"), "section");
        assert_eq!(slugify(""), "section");
    }

    #[test]
    fn heading_ids_are_unique_and_present_for_every_section() {
        // Two languages emit the same h3 titles ("Summary", every hotspot),
        // so the de-duplicator must keep every id distinct.
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
        let ids = collect_ids(&out);
        assert!(!ids.is_empty(), "report must emit heading ids");
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "every emitted id must be unique; got duplicates in {ids:?}"
        );
        // Every h2 section is present and slug-based.
        for expect in ["per-language-overview", "python", "rust"] {
            assert!(
                ids.iter().any(|i| i == expect),
                "missing slug id {expect:?} in {ids:?}"
            );
        }
        // The per-language h3 ids are prefixed by the language slug so a
        // reader can deep-link to one table.
        assert!(
            ids.iter()
                .any(|i| i == "rust-cyclomatic-complexity-hotspots")
        );
        assert!(ids.iter().any(|i| i == "python-summary"));
    }

    #[test]
    fn toc_links_resolve_to_existing_ids() {
        // Parse (don't substring): every TOC fragment must name an id that
        // actually exists on the page, or the nav is a dead link (issue #622).
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
        assert!(
            out.contains("<nav class=\"toc\""),
            "TOC nav must be present"
        );
        let ids: std::collections::BTreeSet<String> = collect_ids(&out).into_iter().collect();
        let frags = collect_href_fragments(&out);
        assert!(!frags.is_empty(), "TOC must contain links");
        for frag in &frags {
            assert!(
                ids.contains(frag),
                "TOC href #{frag} resolves to no id on the page; ids: {ids:?}"
            );
        }
        // The TOC now nests each language's h3 subsections under its h2 entry
        // (issue #685), so it links the three h2 sections plus every h3 below
        // them. At minimum the three h2 anchors and the per-language hotspot
        // h3 anchors must appear.
        for expect in [
            "per-language-overview",
            "rust",
            "python",
            "rust-cyclomatic-complexity-hotspots",
            "python-summary",
        ] {
            assert!(
                frags.iter().any(|f| f == expect),
                "TOC should link #{expect}, got {frags:?}"
            );
        }
    }

    /// Issue #685: each language's h3 hotspot subsections are nested under its
    /// h2 entry in a collapsible `<details>` list, so a reader can jump
    /// straight to one table rather than landing at the top of the section.
    #[test]
    fn toc_nests_h3_sections_under_each_language() {
        let out = generate_html_report(&two_lang_fixture(), 5, SuppressionPolicy::Honor);
        let nav = out
            .split_once("<nav class=\"toc\"")
            .expect("TOC present")
            .1
            .split_once("</nav>")
            .expect("TOC closes")
            .0;
        // The nested lists are collapsible.
        assert!(
            nav.contains("<details>"),
            "TOC must use a collapsible <details> for nested subsections:\n{nav}"
        );
        // A per-language h3 hotspot anchor must appear inside the nav.
        assert!(
            nav.contains("href=\"#rust-cyclomatic-complexity-hotspots\""),
            "TOC must link the h3 hotspot subsections:\n{nav}"
        );
    }

    #[test]
    fn empty_report_emits_no_toc() {
        // No sections means no nav (and no dead links).
        let out = generate_html_report(&[], 20, SuppressionPolicy::Honor);
        assert!(!out.contains("<nav class=\"toc\""));
        assert_html_well_formed(&out);
    }

    /// Issue #686: the head carries a viewport meta (so mobile renders at
    /// device width) and every table is wrapped in an `overflow-x` scroll
    /// container (so a wide table scrolls instead of clipping).
    #[test]
    fn viewport_meta_and_table_overflow_wrapper() {
        let out = generate_html_report(&two_lang_fixture(), 20, SuppressionPolicy::Honor);
        assert!(
            out.contains(
                "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
            ),
            "viewport meta tag missing"
        );
        // The CSS defines the scroll container and every table is wrapped.
        assert!(
            out.contains("div.table-wrap{overflow-x:auto"),
            "table-wrap overflow CSS missing"
        );
        let tables = out.matches("<table class=\"hotspot\">").count();
        let wraps = out.matches("<div class=\"table-wrap\">").count();
        assert!(tables > 0, "report must emit at least one table");
        assert_eq!(
            tables, wraps,
            "every <table> must be wrapped in a .table-wrap div"
        );
        assert_html_well_formed(&out);
    }

    #[test]
    fn ranked_column_carries_aria_sort_at_render_time() {
        // The pre-ranked column of every hotspot table must announce its
        // initial sort order before any click — exactly one `aria-sort` per
        // table header row, on the column the spec ranks by, with the
        // direction matching `SortDir` (issue #622).
        let out = generate_html_report(
            &crate::markdown_report::rich_fixture(),
            5,
            SuppressionPolicy::Honor,
        );

        // The CC table ranks by CC descending: the `aria-sort` must sit on
        // the CC `<th>`, not on Function/File/Line.
        let cc = html_section(&out, "Cyclomatic complexity hotspots (top 5 by CC)");
        let header_row = cc.split_once("</thead>").expect("CC thead").0;
        assert_eq!(
            header_row.matches("aria-sort=").count(),
            1,
            "exactly one column may be marked as the initial sort:\n{header_row}"
        );
        assert!(
            header_row.contains("aria-sort=\"descending\" title=\"Cyclomatic Complexity"),
            "CC ranking column must carry aria-sort=descending:\n{header_row}"
        );

        // The MI table ranks ascending (lowest MI first) on the MI column.
        let mi = html_section(&out, "Maintainability Index hotspots (lowest 5 by MI)");
        let mi_header = mi.split_once("</thead>").expect("MI thead").0;
        assert_eq!(mi_header.matches("aria-sort=").count(), 1);
        assert!(
            mi_header.contains("aria-sort=\"ascending\" title=\"Maintainability Index"),
            "MI ranking column must carry aria-sort=ascending:\n{mi_header}"
        );
    }

    #[test]
    fn sort_hint_present_once_near_first_table() {
        let out = generate_html_report(&rust_fixture(), 5, SuppressionPolicy::Honor);
        assert_eq!(
            out.matches("class=\"sort-hint\"").count(),
            1,
            "the click-to-sort hint should appear exactly once"
        );
    }
}
