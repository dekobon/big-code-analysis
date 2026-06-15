// bca: suppress-file(halstead, loc, nargs, nom, nexits)
// Markdown report templating; thin per-language orchestrators delegating to
// small write_* helpers. File-level halstead/loc and summed nargs/nom/nexits
// are string-formatting-volume / many-fn aggregation artifacts (the large
// in-file test module — rich fixture + cross-format checks — adds to the
// summed nexits count the same way it adds to the others).

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

pub(crate) mod advisory;
pub(crate) mod hotspot;
mod sections;

// `Align` now lives in the format-neutral `hotspot` module; re-export it so
// `sections`, the Markdown `write_table`, and their tests keep using `Align`
// unchanged.
pub(crate) use hotspot::Align;

// The single Cell→Markdown escaper, shared with the VCS report renderer
// (`crate::vcs_report`) so both reports map a [`hotspot::Cell`] to Markdown
// identically.
pub(crate) use sections::render_cell_md;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write;

use big_code_analysis::{FuncSpace, LANG, Metric, SpaceKind, SuppressionPolicy, SuppressionScope};

use crate::format_util::strip_path_prefix;

/// Compact per-function/class metric record for the markdown report pipeline.
#[derive(Debug)]
pub(crate) struct FunctionSummary {
    pub file: String,
    pub name: String,
    pub kind: SpaceKind,
    pub language: LANG,
    /// Effective suppression scope for this space: the enclosing file's
    /// `suppress-file` scope merged with the space's own `bca: suppress`
    /// markers. A hotspot table omits this entry when the table's
    /// [`Metric`] is covered here and the report honors markers
    /// (the default; `--no-suppress` opts back in). Mirrors the gate's
    /// `ThresholdSet::evaluate_with_policy` logic so the published report
    /// agrees with `bca check` and the SARIF emitter (issue #501).
    pub suppressed: SuppressionScope,
    pub start_line: usize,
    #[allow(dead_code)]
    pub end_line: usize,
    pub sloc: usize,
    pub ploc: usize,
    #[expect(dead_code)]
    pub lloc: usize,
    pub cloc: usize,
    pub tokens: usize,
    pub cyclomatic: f64,
    pub cognitive: f64,
    pub halstead_volume: f64,
    #[expect(dead_code)]
    pub halstead_difficulty: f64,
    pub halstead_effort: f64,
    pub halstead_bugs: f64,
    #[expect(dead_code)]
    pub halstead_time: f64,
    pub mi_original: f64,
    #[expect(dead_code)]
    pub mi_sei: f64,
    pub mi_visual_studio: f64,
    pub nargs: usize,
    pub nexits: usize,
    pub nom: usize,
    pub abc: f64,
    pub wmc: f64,
    pub npa: f64,
    pub npm: f64,
}

impl FunctionSummary {
    /// Whether this entry should be hidden from the hotspot table for
    /// `kind` under `policy`.
    ///
    /// Under [`SuppressionPolicy::Honor`] (the report default) an entry
    /// is hidden when its effective scope covers `kind` — i.e. a
    /// `bca: suppress(<kind>)` marker on the function, or a
    /// `bca: suppress-file(<kind>)` marker anywhere in the file, folded
    /// into [`Self::suppressed`] by [`extract_summaries`]. Under
    /// [`SuppressionPolicy::Ignore`] (`--no-suppress`) nothing is hidden,
    /// giving the raw audit view. Mirrors the gate's per-metric check in
    /// `ThresholdSet::evaluate_with_policy` (issue #501).
    pub(crate) fn is_hidden_for(&self, kind: Metric, policy: SuppressionPolicy) -> bool {
        matches!(policy, SuppressionPolicy::Honor) && self.suppressed.covers(kind)
    }
}

/// Extract [`FunctionSummary`] records from a [`FuncSpace`] tree in
/// pre-order.
///
/// `strip_prefix` is applied to `file` using `str::strip_prefix` semantics:
/// if the file path starts with the prefix it is removed, otherwise the path
/// is kept as-is.
///
/// The traversal is iterative (not recursive) so an adversarially deeply
/// nested AST cannot overflow the worker thread's stack — the thread pool's
/// default 2 MiB stack is small enough that pathological input matters.
/// Mirrors `ThresholdSet::evaluate_with_policy` in
/// [`crate::thresholds`]; see lesson 13 in
/// `docs/development/lessons_learned.md` for the analogous web-service
/// denial-of-service vector and issues #292 / #308 for the prior
/// `attach_function_suppression` fix that this extractor was missed by.
pub(crate) fn extract_summaries(
    space: &FuncSpace,
    file: &str,
    language: LANG,
    strip_prefix: &str,
    out: &mut Vec<FunctionSummary>,
) {
    let display_file = strip_path_prefix(file, strip_prefix);
    extract_summaries_inner(space, display_file, language, out);
}

fn extract_summaries_inner(
    space: &FuncSpace,
    display_file: &str,
    language: LANG,
    out: &mut Vec<FunctionSummary>,
) {
    // The root space IS the top-level `Unit`; its `suppressed` scope
    // carries any `bca: suppress-file` markers, which apply to every
    // function in the file. Capture it once so each summary's effective
    // scope is `file_scope ∪ own_scope` — the same union the threshold
    // gate forms in `ThresholdSet::evaluate_with_policy` (issue #501).
    let file_scope = &space.suppressed;

    // Iterative pre-order walk over the FuncSpace tree. Children are
    // pushed in reverse so `pop()` visits them in source order — this
    // produces the same FunctionSummary ordering as the prior recursive
    // form, preserving snapshot stability.
    let mut stack: Vec<&FuncSpace> = vec![space];
    while let Some(current) = stack.pop() {
        let m = &current.metrics;
        let mut suppressed = file_scope.clone();
        suppressed.merge(&current.suppressed);
        out.push(FunctionSummary {
            file: display_file.to_string(),
            name: current.name.clone().unwrap_or_default(),
            kind: current.kind,
            language,
            suppressed,
            start_line: current.start_line,
            end_line: current.end_line,
            sloc: m.loc.sloc() as usize,
            ploc: m.loc.ploc() as usize,
            lloc: m.loc.lloc() as usize,
            cloc: m.loc.cloc() as usize,
            tokens: m.tokens.tokens_sum() as usize,
            cyclomatic: m.cyclomatic.cyclomatic() as f64,
            cognitive: m.cognitive.cognitive() as f64,
            halstead_volume: m.halstead.volume(),
            halstead_difficulty: m.halstead.difficulty(),
            halstead_effort: m.halstead.effort(),
            halstead_bugs: m.halstead.bugs(),
            halstead_time: m.halstead.time(),
            mi_original: m.mi.original(),
            mi_sei: m.mi.sei(),
            mi_visual_studio: m.mi.visual_studio(),
            nargs: m.nargs.total() as usize,
            nexits: m.nexits.nexits_sum() as usize,
            nom: m.nom.total() as usize,
            abc: m.abc.magnitude(),
            wmc: m.wmc.total_wmc() as f64,
            npa: m.npa.total_npa() as f64,
            npm: m.npm.total_npm() as f64,
        });

        stack.extend(current.spaces.iter().rev());
    }
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

/// Escape a value for a GFM table cell. Delegates to the crate's single
/// GFM-cell escaper so backslash, pipe, and newline handling stay in sync
/// with the `check` report (issue #439 — the previous local copy escaped
/// `|` and newlines but not `\`, corrupting backslash-bearing paths).
fn escape_cell(s: &str) -> String {
    crate::check_format::escape_gfm_cell(s)
}

/// Escape an identifier for display inside a backtick code span in a GFM
/// table cell. Unlike [`escape_cell`], this must **not** double literal
/// backslashes: inside a code span GFM treats content literally and never
/// processes backslash escapes, so `escape_cell`'s `\` → `\\` rule (correct
/// for a bare cell) would render a doubled backslash (issue #846). The only
/// characters that still need handling inside the span are the backtick
/// (replaced with the modifier-letter U+02CB so it cannot close the span),
/// the pipe (GFM's table parser strips `\|` → `|` *before* code-span parsing,
/// so it renders correctly), and line breaks (collapsed to a space so a
/// multi-line name cannot break the table layout).
///
/// `pub(crate)` so the provenance footer can reuse this single code-span
/// escaping policy for the seed paths it renders (issue #848).
pub(crate) fn escape_name(s: &str) -> String {
    let mut span = String::with_capacity(s.len() + 2);
    span.push('`');
    for ch in s.chars() {
        match ch {
            '`' => span.push('\u{02CB}'),
            '|' => span.push_str("\\|"),
            '\n' | '\r' => span.push(' '),
            _ => span.push(ch),
        }
    }
    span.push('`');
    span
}

/// Format an integer with comma thousands separators (`15973` -> `15,973`).
/// `pub(crate)` so the VCS report's churn / commit counts share this one
/// formatter with the AST tables and cannot drift (issue #618). The HTML
/// sort comparator strips the commas before comparing, so the separators
/// are display-only.
pub(crate) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let len = s.len();
    if len <= 3 {
        return s;
    }
    let mut result = String::with_capacity(len + (len - 1) / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

pub(super) fn title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
        if matches!(c, '/' | ' ' | '-') {
            capitalize_next = true;
        }
    }
    result
}

/// Maps a language's canonical lowercase slug (`LANG::name()`, e.g. `"cpp"`,
/// `"csharp"`) to the human-readable name shown in report headings and the
/// global header's `Languages` row (`"C++"`, `"C#"`).
///
/// Slugs stay machine-readable by design (#540 bans `/` and `#`), so they make
/// poor headings — a reader unfamiliar with the convention can mistake
/// `"Csharp"` for a distinct dialect. This is a *display-only* mapping: CSS
/// `lang-{slug}` classes, anchors, and all structured/wire output keep the slug
/// (see #613).
///
/// The match over [`LANG`] is exhaustive (no wildcard) so adding a new language
/// is a compile error here until a display name is chosen. A slug that does not
/// parse to a known `LANG` — only possible for a future variant not yet
/// reflected in this build — degrades gracefully to [`title_case`].
pub(super) fn language_display_name(slug: &str) -> Cow<'_, str> {
    let Ok(lang) = slug.parse::<LANG>() else {
        return Cow::Owned(title_case(slug));
    };
    // bca: suppress(cyclomatic)
    // Exhaustive, wildcard-free `match LANG` display table (one arm per
    // language variant), kept whole on purpose: the doc comment above relies
    // on it being a compile error to add a language without choosing a name.
    // Its branch count is the size of the language set, not logic a reader
    // needs to follow; splitting it would only scatter the table.
    let display = match lang {
        LANG::Bash => "Bash",
        LANG::C => "C",
        LANG::Ccomment => "C Comment",
        LANG::Cpp => "C++",
        LANG::Csharp => "C#",
        LANG::Elixir => "Elixir",
        LANG::Go => "Go",
        LANG::Groovy => "Groovy",
        LANG::Irules => "iRules",
        LANG::Java => "Java",
        LANG::Javascript => "JavaScript",
        LANG::Kotlin => "Kotlin",
        LANG::Lua => "Lua",
        LANG::Mozcpp => "C++ (Mozilla)",
        LANG::Mozjs => "JavaScript (Mozilla)",
        LANG::Objc => "Objective-C",
        LANG::Perl => "Perl",
        LANG::Php => "PHP",
        LANG::Preproc => "C/C++ Preprocessor",
        LANG::Python => "Python",
        LANG::Ruby => "Ruby",
        LANG::Rust => "Rust",
        LANG::Tcl => "Tcl",
        LANG::Tsx => "TSX",
        LANG::Typescript => "TypeScript",
    };
    Cow::Borrowed(display)
}

/// Stable tie-break when the ranking metric is equal: file path, then start
/// line, then name. Single source of truth shared by `sort_by_metric_asc`/
/// `sort_by_metric_desc` and `hotspot::partial_top_n_desc`, so the partial-sort
/// partition and the final sort agree on equal-metric rows (a divergence would
/// silently change which rows survive top-N truncation).
pub(crate) fn tiebreak(a: &FunctionSummary, b: &FunctionSummary) -> std::cmp::Ordering {
    a.file
        .cmp(&b.file)
        .then_with(|| a.start_line.cmp(&b.start_line))
        .then_with(|| a.name.cmp(&b.name))
}

pub(super) fn sort_by_metric_desc(
    items: &mut [&FunctionSummary],
    metric: impl Fn(&FunctionSummary) -> f64,
) {
    items.sort_by(|a, b| metric(b).total_cmp(&metric(a)).then_with(|| tiebreak(a, b)));
}

pub(super) fn sort_by_metric_asc(
    items: &mut [&FunctionSummary],
    metric: impl Fn(&FunctionSummary) -> f64,
) {
    items.sort_by(|a, b| metric(a).total_cmp(&metric(b)).then_with(|| tiebreak(a, b)));
}

/// Ascending sort by `metric`, breaking ties on `secondary` (also ascending)
/// before the path/line/name [`tiebreak`]. The MI hotspot uses this so a
/// table whose displayed `mi_visual_studio` values all clamp to `0.0` still
/// ranks the worst files first by their unclamped `mi_original`, instead of
/// degenerating to alphabetical order (issue #627).
pub(super) fn sort_by_metric_asc_then(
    items: &mut [&FunctionSummary],
    metric: impl Fn(&FunctionSummary) -> f64,
    secondary: impl Fn(&FunctionSummary) -> f64,
) {
    items.sort_by(|a, b| {
        metric(a)
            .total_cmp(&metric(b))
            .then_with(|| secondary(a).total_cmp(&secondary(b)))
            .then_with(|| tiebreak(a, b))
    });
}

pub(super) fn is_class_like(kind: SpaceKind) -> bool {
    matches!(
        kind,
        SpaceKind::Class
            | SpaceKind::Struct
            | SpaceKind::Trait
            | SpaceKind::Impl
            | SpaceKind::Namespace
            | SpaceKind::Interface
    )
}

pub(super) fn mi_rating(mi: f64) -> &'static str {
    if mi >= 20.0 {
        "GOOD"
    } else if mi >= 10.0 {
        "MODERATE"
    } else {
        "LOW"
    }
}

/// Headline label for the SLOC-weighted average MI, shared by the Markdown
/// Summary row and the HTML per-language summary so the two formats name
/// the metric identically. The `(SLOC-weighted)` qualifier flags that the
/// figure is size-weighted and derived from the unclamped value, unlike the
/// per-file `MI` hotspot column (issue #725).
pub(super) const AVG_MI_LABEL: &str = "Average MI (SLOC-weighted)";

/// Visual Studio MI rescale factor. `mi.original()` is the raw `171 −
/// 5.2·ln V − 0.23·G − 16.2·ln SLOC` formula; `mi.visual_studio()`
/// rescales it onto 0–100 and floors at 0. Multiplying `mi_original` by
/// this factor recovers the *unclamped* Visual Studio value — the same
/// expression `mi_visual_studio` clamps — so a file whose displayed MI
/// saturates at 0 still contributes its true (often negative)
/// maintainability to the headline average (issue #725).
const MI_VS_SCALE: f64 = 100.0 / 171.0;

/// Per-file contribution to the SLOC-weighted average-MI numerator: the
/// file's *unclamped* Visual Studio MI times its SLOC. Summing this over
/// the units and dividing by their total SLOC yields the headline
/// `Average MI (SLOC-weighted)`.
///
/// Averaging the unclamped value (via `mi_original`) mirrors the #627
/// hotspot ranking, which already sorts on `mi_original` so the order
/// stays informative when the displayed `mi_visual_studio` clamps to 0.
/// Before #725 the headline averaged the *clamped* value, so a
/// catastrophically unmaintainable file (true MI ≈ −400, clamped to 0)
/// and a marginally bad one (true MI ≈ −5, also clamped to 0) both
/// contributed 0 and were indistinguishable. SLOC weighting additionally
/// lets a large file dominate the codebase-health reading instead of
/// counting equally with a five-line file.
pub(super) fn mi_weight_numerator(s: &FunctionSummary) -> f64 {
    s.mi_original * MI_VS_SCALE * s.sloc as f64
}

/// Divide a [`mi_weight_numerator`] sum by the units' total SLOC to get the
/// SLOC-weighted average MI. Returns 0.0 when the units carry no SLOC, so
/// an all-empty input yields a neutral headline rather than a division by
/// zero.
pub(super) fn sloc_weighted_avg_mi(mi_numerator: f64, total_sloc: usize) -> f64 {
    if total_sloc > 0 {
        mi_numerator / total_sloc as f64
    } else {
        0.0
    }
}

pub(crate) fn write_table(
    out: &mut String,
    headers: &[&str],
    aligns: &[Align],
    rows: &[Vec<String>],
) {
    debug_assert_eq!(headers.len(), aligns.len());
    let widths = column_widths(headers, rows);

    out.push('|');
    for (i, h) in headers.iter().enumerate() {
        push_cell(out, h, widths[i], aligns[i]);
        out.push('|');
    }
    out.push('\n');

    out.push('|');
    for (i, &a) in aligns.iter().enumerate() {
        push_separator(out, a, widths[i]);
    }
    out.push('\n');

    for row in rows {
        debug_assert_eq!(row.len(), headers.len());
        out.push('|');
        for (i, cell) in row.iter().enumerate() {
            push_cell(out, cell, widths[i], aligns[i]);
            out.push('|');
        }
        out.push('\n');
    }
}

/// The hotspot-column legend plus the global-header stat definitions
/// (PLOC / Comments / Comment ratio), so the AST report's legend defines
/// every number on the page — including the header stats no hotspot column
/// covers (issue #679). Shared by the Markdown and HTML AST renderers so the
/// two legends carry the same rows.
pub(crate) fn legend_entries_with_header_stats() -> Vec<(&'static str, &'static str)> {
    let mut entries = hotspot::legend_entries();
    entries.extend_from_slice(hotspot::HEADER_STAT_LEGEND);
    entries
}

/// Append a "Legend" subsection at the given heading `level` (number of
/// leading `#`): a definition-list-style bullet per `(header, definition)`
/// pair, so a Markdown reader (a PR comment, a pasted issue) can learn what
/// each column abbreviation means — the same definitions the HTML report
/// shows as hover tooltips, drawn from the shared column specs so the two
/// formats cannot drift (issue #611). The header is emitted in bold and the
/// definition is GFM-escaped. The heading level is threaded rather than
/// hardcoded so an embedding page keeps its outline gap-free — a standalone
/// VCS page (`#` title) needs an `##` legend, an embedded one (`##` section)
/// needs `###` (issue #618). Renders nothing when `entries` is empty.
pub(crate) fn write_legend(out: &mut String, level: usize, entries: &[(&str, &str)]) {
    if entries.is_empty() {
        return;
    }
    let hashes = "#".repeat(level);
    let _ = writeln!(out, "\n{hashes} Legend\n");
    for (header, definition) in entries {
        // Link the header to its hosted metric chapter when one exists, so a
        // one-line legend entry can hand the reader the full explanation
        // (issue #675). VCS / identity headers have no anchor and render bare.
        match hotspot::metric_doc_url(header) {
            Some(url) => {
                let _ = writeln!(out, "- **[{header}]({url})** — {}", escape_cell(definition));
            }
            None => {
                let _ = writeln!(out, "- **{header}** — {}", escape_cell(definition));
            }
        }
    }
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let cell_w = rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0);
            // Min 3 keeps the separator (`---` / `--:`) unambiguous for GFM.
            h.chars().count().max(cell_w).max(3)
        })
        .collect()
}

fn push_cell(out: &mut String, cell: &str, width: usize, align: Align) {
    let pad = width - cell.chars().count();
    out.push(' ');
    match align {
        Align::Left => {
            out.push_str(cell);
            out.extend(std::iter::repeat_n(' ', pad));
        }
        Align::Right => {
            out.extend(std::iter::repeat_n(' ', pad));
            out.push_str(cell);
        }
    }
    out.push(' ');
}

fn push_separator(out: &mut String, align: Align, width: usize) {
    out.push(' ');
    match align {
        Align::Left => out.extend(std::iter::repeat_n('-', width)),
        Align::Right => {
            out.extend(std::iter::repeat_n('-', width - 1));
            out.push(':');
        }
    }
    out.push(' ');
    out.push('|');
}

/// Rich multi-language fixture exercising every report section, top-N
/// truncation (>5 cyclomatic entries), per-metric suppression, and a
/// class-like (WMC). Shared by the Markdown and HTML snapshot tests and
/// the cross-format consistency test so all three cover the same paths.
/// Lives here (not a `_tests.rs`) so the sibling `html_report` test module
/// can reach it via `crate::markdown_report::rich_fixture()`.
#[cfg(test)]
pub(crate) fn rich_fixture() -> Vec<FunctionSummary> {
    use big_code_analysis::{LANG, Metric, SpaceKind, SuppressionScope};
    use std::collections::BTreeSet;

    let base = |name: &str, file: &str, kind: SpaceKind, language: LANG, start_line: usize| {
        FunctionSummary {
            file: file.to_string(),
            name: name.to_string(),
            kind,
            language,
            suppressed: SuppressionScope::default(),
            start_line,
            end_line: start_line + 10,
            sloc: 20,
            ploc: 25,
            lloc: 15,
            cloc: 5,
            tokens: 30,
            cyclomatic: 0.0,
            cognitive: 0.0,
            halstead_volume: 100.0,
            halstead_difficulty: 5.0,
            halstead_effort: 0.0,
            halstead_bugs: 0.0,
            halstead_time: 28.0,
            mi_original: 80.0,
            mi_sei: 85.0,
            mi_visual_studio: 50.0,
            nargs: 0,
            nexits: 0,
            nom: 1,
            abc: 0.0,
            wmc: 0.0,
            npa: 0.0,
            npm: 0.0,
        }
    };

    // Per-function metric tuple: (name, line, cc, cognitive, effort, sloc,
    // tokens, nargs, nexits, abc). Volume = effort/2, bugs = cc/30.
    let func = |file: &str,
                lang: LANG,
                row: (&str, usize, f64, f64, f64, usize, usize, usize, usize, f64)| {
        let (name, line, cc, cog, effort, sloc, tokens, nargs, nexits, abc) = row;
        let mut f = base(name, file, SpaceKind::Function, lang, line);
        f.cyclomatic = cc;
        f.cognitive = cog;
        f.halstead_effort = effort;
        f.halstead_volume = effort / 2.0;
        f.halstead_bugs = cc / 30.0;
        f.sloc = sloc;
        f.ploc = sloc + 5;
        f.tokens = tokens;
        f.nargs = nargs;
        f.nexits = nexits;
        f.abc = abc;
        f
    };

    let mut v = Vec::new();

    // Rust units (Summary + MI table; MI keeps halstead_volume>0 && sloc>0).
    let mut u_lib = base("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust, 1);
    u_lib.sloc = 200;
    u_lib.ploc = 240;
    u_lib.cloc = 20;
    u_lib.tokens = 1500;
    u_lib.halstead_volume = 8000.0;
    // Unclamped Visual Studio MI = mi_original * 100/171, so keep the two
    // coherent: 20.5 * 100/171 ≈ 12.0 (the displayed per-file value).
    u_lib.mi_original = 20.5;
    u_lib.mi_visual_studio = 12.0;
    v.push(u_lib);
    let mut u_util = base("util.rs", "src/util.rs", SpaceKind::Unit, LANG::Rust, 1);
    u_util.sloc = 80;
    u_util.tokens = 600;
    u_util.halstead_volume = 2000.0;
    // 77.0 * 100/171 ≈ 45.0 (coherent with the displayed value).
    u_util.mi_original = 77.0;
    u_util.mi_visual_studio = 45.0;
    v.push(u_util);

    // Seven Rust functions, descending cyclomatic → top-5 truncates.
    for row in [
        (
            "process_request",
            10,
            25.0,
            30.0,
            9000.0,
            150,
            900,
            6,
            4,
            22.0,
        ),
        (
            "validate_input",
            40,
            20.0,
            24.0,
            7000.0,
            120,
            700,
            2,
            2,
            18.0,
        ),
        ("parse_config", 70, 15.0, 18.0, 5000.0, 90, 500, 5, 3, 14.0),
        ("handle_event", 100, 12.0, 14.0, 3000.0, 70, 400, 2, 6, 10.0),
        ("compute_score", 130, 8.0, 9.0, 1500.0, 40, 200, 4, 1, 6.0),
        ("format_output", 160, 5.0, 4.0, 800.0, 25, 120, 1, 1, 4.0),
        ("tiny_helper", 190, 3.0, 2.0, 300.0, 12, 50, 0, 1, 2.0),
    ] {
        v.push(func("src/lib.rs", LANG::Rust, row));
    }

    // Suppressed for Cyclomatic only: dropped from the CC table + note, but
    // still present in Cognitive/Halstead/etc. and the raw Actionable Summary.
    let mut secret = func(
        "src/lib.rs",
        LANG::Rust,
        (
            "secret_internal",
            220,
            99.0,
            80.0,
            20000.0,
            300,
            2000,
            8,
            10,
            50.0,
        ),
    );
    secret.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));
    v.push(secret);

    // Rust class-like (WMC table; drawn from the full all-kinds slice).
    let mut widget = base("Widget", "src/lib.rs", SpaceKind::Struct, LANG::Rust, 250);
    widget.wmc = 40.0;
    widget.nom = 8;
    widget.npa = 2.0;
    widget.npm = 6.0;
    widget.sloc = 180;
    widget.tokens = 1100;
    v.push(widget);

    // Python unit + functions.
    let mut u_main = base("main.py", "src/main.py", SpaceKind::Unit, LANG::Python, 1);
    u_main.sloc = 120;
    u_main.tokens = 800;
    u_main.halstead_volume = 3000.0;
    // 47.9 * 100/171 ≈ 28.0 (coherent with the displayed value).
    u_main.mi_original = 47.9;
    u_main.mi_visual_studio = 28.0;
    v.push(u_main);
    for row in [
        ("main", 10, 10.0, 12.0, 2000.0, 60, 300, 3, 2, 9.0),
        ("load_data", 40, 7.0, 6.0, 1200.0, 35, 180, 4, 2, 7.0),
    ] {
        v.push(func("src/main.py", LANG::Python, row));
    }

    v
}

/// Footer-free convenience wrapper used by the snapshot tests; the command
/// path goes through [`generate_report_with_vcs`] with a provenance footer.
#[cfg(test)]
pub(crate) fn generate_report(
    summaries: &[FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) -> String {
    generate_report_with_vcs(
        summaries,
        top_n,
        policy,
        &advisory::AdvisoryThresholds::DEFAULT,
        None,
        None,
    )
}

/// As [`generate_report`], optionally appending a "Change-history risk"
/// section (`bca report --vcs`) rendered through [`crate::vcs_report`], and an
/// optional provenance footer (issue #680). `prov` is `None` in the snapshot
/// tests (the footer is provenance, separately tested in `crate::provenance`)
/// and `Some` from the command path.
pub(crate) fn generate_report_with_vcs(
    summaries: &[FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
    advisory: &advisory::AdvisoryThresholds,
    vcs: Option<&crate::vcs_command::Report>,
    prov: Option<&crate::provenance::Provenance<'_>>,
) -> String {
    let mut out = String::new();

    // Group by language display name (BTreeMap → deterministic alphabetical order).
    let by_lang = group_by_language(summaries);

    let totals = GlobalTotals::from_entries(summaries);
    write_global_header(&mut out, &totals, &by_lang);

    if !by_lang.is_empty() {
        write_per_language_overview(&mut out, &by_lang);

        for (&lang_name, lang_summaries) in &by_lang {
            write_language_section(
                &mut out,
                lang_name,
                lang_summaries,
                top_n,
                policy,
                *advisory,
            );
        }
        // Footer legend at `##` so it gets its own TOC entry and stops
        // nesting under the last language's `##` section, though it defines
        // columns for every language (issue #679). Draws from the shared
        // specs (issue #611), plus the global-header stats (PLOC / Comments /
        // Comment ratio) so the legend defines everything on the page.
        write_legend(&mut out, 2, &legend_entries_with_header_stats());
    }

    if let Some(report) = vcs {
        crate::vcs_report::push_markdown_section(&mut out, report);
    }

    // Provenance footer (version / date / paths / top / suppression state) so
    // a detached artifact is self-describing (issue #680).
    if let Some(prov) = prov {
        crate::provenance::push_markdown_footer(&mut out, prov);
    }

    out
}

fn group_by_language(summaries: &[FunctionSummary]) -> BTreeMap<&str, Vec<&FunctionSummary>> {
    let mut map = BTreeMap::<&str, Vec<&FunctionSummary>>::new();
    for s in summaries {
        map.entry(s.language.name()).or_default().push(s);
    }
    map
}

/// Aggregate counters for the global report header — file/SLOC/PLOC
/// totals plus function and class-like-symbol counts in one pass.
struct GlobalTotals {
    files: usize,
    sloc: usize,
    ploc: usize,
    cloc: usize,
    functions: usize,
    classes: usize,
}

impl GlobalTotals {
    fn from_entries(summaries: &[FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            functions: 0,
            classes: 0,
        };
        for s in summaries {
            if s.kind == SpaceKind::Unit {
                t.files += 1;
                t.sloc += s.sloc;
                t.ploc += s.ploc;
                t.cloc += s.cloc;
            }
            if s.kind == SpaceKind::Function {
                t.functions += 1;
            }
            if is_class_like(s.kind) {
                t.classes += 1;
            }
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        if self.sloc > 0 {
            (self.cloc as f64 / self.sloc as f64) * 100.0
        } else {
            0.0
        }
    }
}

fn write_global_header(
    out: &mut String,
    totals: &GlobalTotals,
    by_lang: &BTreeMap<&str, Vec<&FunctionSummary>>,
) {
    let languages_list: String = by_lang
        .keys()
        .map(|k| language_display_name(k))
        .collect::<Vec<_>>()
        .join(", ");

    let comment_ratio = totals.comment_ratio();

    // A two-column (metric | value) table renders identically in every GFM
    // dialect and in raw text. The earlier space-aligned single-newline layout
    // collapsed to a run-on paragraph in spec-conformant GFM, where a single
    // newline is a soft break and runs of spaces collapse to one (issue #671).
    let rows: Vec<Vec<String>> = vec![
        vec!["Files analyzed".to_string(), thousands(totals.files)],
        vec!["Languages".to_string(), languages_list],
        vec!["Total SLOC".to_string(), thousands(totals.sloc)],
        vec!["PLOC".to_string(), thousands(totals.ploc)],
        vec!["Comments".to_string(), thousands(totals.cloc)],
        vec!["Functions/methods".to_string(), thousands(totals.functions)],
        // "Types" covers all six kinds `is_class_like` counts (class,
        // struct, trait, impl, interface, namespace) rather than naming
        // three and underselling the rest (issue #687).
        vec!["Types".to_string(), thousands(totals.classes)],
        vec!["Comment ratio".to_string(), format!("{comment_ratio:.1}%")],
    ];

    let _ = writeln!(out, "# Code Quality Metrics Summary\n");
    write_table(
        out,
        &["Metric", "Value"],
        &[Align::Left, Align::Right],
        &rows,
    );
}

fn write_per_language_overview(out: &mut String, by_lang: &BTreeMap<&str, Vec<&FunctionSummary>>) {
    let _ = writeln!(out, "\n## Per-language overview\n");
    let mut overview_rows: Vec<Vec<String>> = Vec::with_capacity(by_lang.len());
    for (&lang_name, lang_summaries) in by_lang {
        overview_rows.push(lang_overview_row(lang_name, lang_summaries));
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
        &overview_rows,
    );
}

fn lang_overview_row(lang_name: &str, lang_summaries: &[&FunctionSummary]) -> Vec<String> {
    let (unit_count, lang_sloc, avg_mi) = unit_aggregates(lang_summaries);
    let (func_count, avg_cc, avg_cog) = function_aggregates(lang_summaries);
    vec![
        language_display_name(lang_name).into_owned(),
        thousands(unit_count),
        thousands(lang_sloc),
        thousands(func_count),
        format!("{avg_mi:.1}"),
        format!("{avg_cc:.1}"),
        format!("{avg_cog:.1}"),
    ]
}

fn unit_aggregates(lang_summaries: &[&FunctionSummary]) -> (usize, usize, f64) {
    let mut count = 0usize;
    let mut sloc = 0usize;
    let mut mi_numerator = 0.0f64;
    for s in lang_summaries.iter().filter(|s| s.kind == SpaceKind::Unit) {
        count += 1;
        sloc += s.sloc;
        mi_numerator += mi_weight_numerator(s);
    }
    let avg_mi = sloc_weighted_avg_mi(mi_numerator, sloc);
    (count, sloc, avg_mi)
}

fn function_aggregates(lang_summaries: &[&FunctionSummary]) -> (usize, f64, f64) {
    let mut count = 0usize;
    let mut cc_sum = 0.0f64;
    let mut cog_sum = 0.0f64;
    for s in lang_summaries
        .iter()
        .filter(|s| s.kind == SpaceKind::Function)
    {
        count += 1;
        cc_sum += s.cyclomatic;
        cog_sum += s.cognitive;
    }
    if count > 0 {
        (count, cc_sum / count as f64, cog_sum / count as f64)
    } else {
        (0, 0.0, 0.0)
    }
}

fn write_language_section(
    out: &mut String,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
    advisory: advisory::AdvisoryThresholds,
) {
    let display_name = language_display_name(lang_name);
    let _ = writeln!(out, "\n## {display_name}\n");

    let (units, funcs) = sections::split_units_and_functions(entries);
    sections::write_summary(out, &units);

    // The Actionable Summary is the highest-altitude block (the
    // "N functions with CC > 10"-style counts), so it leads the section —
    // directly after `### Summary`, before any hotspot table — rather than
    // being buried mid-stream where a reader who stops after one or two
    // tables never sees it (issue #678).
    sections::write_actionable_summary(out, &funcs, policy, advisory);

    // Drive every hotspot section from the shared `SPECS` table (the same
    // table the HTML report uses) so the two formats cannot diverge in
    // membership/order/suppression. WMC draws from the full slice, MI from
    // units, the rest from functions.
    for spec in hotspot::SPECS {
        let base: &[&FunctionSummary] = match spec.source {
            hotspot::Source::Units => &units,
            hotspot::Source::Funcs => &funcs,
            hotspot::Source::All => entries,
        };
        if spec.cc_note {
            let (rows, stats) = hotspot::select_cc(spec, base, top_n, policy, advisory);
            if rows.is_empty() {
                // Mirror the non-CC branch: a CC table emptied purely by
                // suppression earns the same "table omitted" caption, or the
                // Actionable Summary's raw CC bullets would dangle (#616).
                let suppressed = hotspot::fully_suppressed_count(spec, base, policy, advisory);
                sections::emit_fully_suppressed_note_md(out, &spec.title.render(top_n), suppressed);
            } else {
                sections::emit_section_md(out, spec, top_n, &rows);
                sections::emit_cc_note_md(out, &stats, policy);
            }
        } else {
            let rows = hotspot::select(spec, base, top_n, policy, advisory);
            if rows.is_empty() {
                // An empty table here is either a genuinely-absent metric or a
                // table whose every matching row was suppressed; only the
                // latter earns a caption so a summary bullet never dangles.
                let suppressed = hotspot::fully_suppressed_count(spec, base, policy, advisory);
                sections::emit_fully_suppressed_note_md(out, &spec.title.render(top_n), suppressed);
            } else {
                sections::emit_section_md(out, spec, top_n, &rows);
                if spec.mi_note {
                    sections::emit_mi_note_md(out);
                }
            }
        }
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
    use big_code_analysis::{CodeMetrics, FuncSpace, SpaceKind};

    /// Collapse runs of spaces to a single space so assertions can match
    /// the logical row content regardless of column-padding width.
    fn collapse_spaces(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut prev_space = false;
        for c in s.chars() {
            if c == ' ' {
                if !prev_space {
                    out.push(' ');
                }
                prev_space = true;
            } else {
                out.push(c);
                prev_space = false;
            }
        }
        out
    }

    fn make_space(name: &str, kind: SpaceKind, start: usize, end: usize) -> FuncSpace {
        FuncSpace {
            name: Some(name.to_string()),
            start_line: start,
            end_line: end,
            kind,
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
            suppressed: big_code_analysis::SuppressionScope::default(),
        }
    }

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

    /// Issue #725: the headline Average MI must average the *unclamped*
    /// Visual Studio MI (`mi_original` rescaled by 100/171), SLOC-weighted,
    /// so a catastrophic file whose displayed `mi_visual_studio` clamps to
    /// 0 still drags the headline down instead of contributing 0. The
    /// integer products are chosen so the unclamped values are exact:
    /// `-342 * 100/171 = -200`, `171 * 100/171 = 100`.
    #[test]
    fn avg_mi_is_sloc_weighted_over_unclamped_values() {
        // Catastrophic 400-SLOC file: true MI is deeply negative and the
        // displayed Visual Studio value clamps to 0.0.
        let mut bad = make_summary("bad.rs", "src/bad.rs", SpaceKind::Unit, LANG::Rust);
        bad.sloc = 400;
        bad.mi_original = -342.0; // unclamped VS = -200.0
        bad.mi_visual_studio = 0.0;
        // Healthy 10-SLOC file: unclamped VS = +100.0.
        let mut good = make_summary("good.rs", "src/good.rs", SpaceKind::Unit, LANG::Rust);
        good.sloc = 10;
        good.mi_original = 171.0; // unclamped VS = 100.0
        good.mi_visual_studio = 100.0;

        // numerator = -200*400 + 100*10 = -79_000; total SLOC = 410.
        let numerator = mi_weight_numerator(&bad) + mi_weight_numerator(&good);
        assert!(
            (numerator - (-79_000.0)).abs() < 1e-6,
            "weighted numerator should be -79000, got {numerator}"
        );
        let avg = sloc_weighted_avg_mi(numerator, 410);
        assert!(
            (avg - (-192.683)).abs() < 1e-3,
            "SLOC-weighted unclamped average should be ≈ -192.7, got {avg}"
        );
        assert_eq!(mi_rating(avg), "LOW");
        // Contrast: the pre-#725 clamped, unweighted mean was
        // (0.0 + 100.0) / 2 = 50.0 and rated GOOD — masking the
        // unmaintainable bulk that the new headline rates LOW.
        let pre_725_clamped_mean = 50.0;
        assert_eq!(mi_rating(pre_725_clamped_mean), "GOOD");

        // The rendered headline carries the SLOC-weighted label and a
        // negative, LOW-rated value (single language → one summary line).
        let report = generate_report(&[bad, good], 20, SuppressionPolicy::Honor);
        let line = report
            .lines()
            .find(|l| l.contains("Average MI (SLOC-weighted)"))
            .expect("headline row present");
        // The rendered headline pins both the value and the rating: -192.7
        // is the SLOC-weighted result (an unweighted mean would be -50.0).
        assert!(
            line.contains("-192.7 (LOW)"),
            "headline must render the SLOC-weighted -192.7 (LOW):\n{line}"
        );
    }

    /// A unit set carrying no SLOC must not divide by zero; the headline
    /// falls back to a neutral 0.0 (issue #725). The numerator is arbitrary
    /// and non-zero on purpose: the guard depends only on `total_sloc == 0`,
    /// so any numerator must still yield 0.0.
    #[test]
    fn sloc_weighted_avg_mi_zero_sloc_is_neutral() {
        assert_eq!(sloc_weighted_avg_mi(1_234.5, 0), 0.0);
    }

    /// Byte baseline for the whole Markdown report over the rich,
    /// all-sections fixture (top-N truncation, per-metric suppression, a
    /// class-like). The hotspot-spec unification must leave this unchanged.
    #[test]
    fn snapshot_rich_report() {
        let out = generate_report(&rich_fixture(), 5, SuppressionPolicy::Honor);
        insta::assert_snapshot!("markdown_report_rich", out);
    }

    /// Issue #611/#679: the Markdown report ends with a `## Legend` section
    /// (at `##` so it gets its own TOC entry and stops nesting under the last
    /// language — #679) that defines every metric column the hotspot tables
    /// used, plus the global-header stats, drawn from the shared
    /// `legend_entries_with_header_stats` so it cannot drift from the HTML
    /// tooltips. Identity columns (Function/File/Line/Type) carry no
    /// definition and must stay out of the legend.
    #[test]
    fn markdown_report_renders_metric_legend() {
        let out = generate_report(&rich_fixture(), 5, SuppressionPolicy::Honor);
        assert!(out.contains("## Legend"), "legend heading missing");
        assert!(
            !out.contains("### Legend"),
            "legend must be at ## scope, not ### (issue #679)"
        );
        for (header, definition) in legend_entries_with_header_stats() {
            // Headers with a hosted-docs anchor link to it (issue #675); the
            // rest render bare. Either way the definition follows.
            let bullet = match hotspot::metric_doc_url(header) {
                Some(url) => format!("- **[{header}]({url})** — {}", escape_cell(definition)),
                None => format!("- **{header}** — {}", escape_cell(definition)),
            };
            assert!(out.contains(&bullet), "legend missing entry for {header:?}");
        }
        // The header-stat definitions PLOC / Comments / Comment ratio are
        // now in the legend, each linking the Lines-of-Code chapter (#679/#675).
        for header in ["PLOC", "Comments", "Comment ratio"] {
            assert!(
                out.contains(&format!("- **[{header}](")),
                "legend missing header-stat entry {header:?}"
            );
        }
        // Identity columns describe the row, not a metric; they must not
        // appear as legend bullets.
        for identity in ["Function", "File", "Line", "Type"] {
            assert!(
                !out.contains(&format!("- **{identity}**")),
                "identity column {identity:?} should not be in the legend"
            );
        }
    }

    /// First-column cell values of a Markdown report section, in row order
    /// (header + separator skipped, identifier backticks stripped). A section
    /// title repeats once per language, so this gathers the rows of **every**
    /// occurrence — the cross-format check must cover all languages (e.g. the
    /// Rust block where suppression/truncation happen), not just the first.
    fn md_section_first_column(report: &str, title: &str) -> Vec<String> {
        let needle = format!("### {title}\n");
        let mut names = Vec::new();
        let mut rest = report;
        while let Some(pos) = rest.find(&needle) {
            rest = &rest[pos + needle.len()..];
            let mut header_seen = false;
            for line in rest.lines() {
                let line = line.trim();
                if line.is_empty() {
                    if header_seen {
                        break; // blank line ends this block's table
                    }
                    continue;
                }
                if !line.starts_with('|') {
                    break; // note / next heading ends this block
                }
                let first = line
                    .trim_start_matches('|')
                    .split('|')
                    .next()
                    .unwrap_or("")
                    .trim();
                if first.chars().all(|c| matches!(c, '-' | ':')) {
                    continue; // separator row
                }
                if !header_seen {
                    header_seen = true; // column-header row
                    continue;
                }
                names.push(first.trim_matches('`').to_string());
            }
        }
        names
    }

    /// First-column `<td>` text of a HTML report section, in row order,
    /// across **every** occurrence of the title (one per language) — matching
    /// the all-occurrences behaviour of [`md_section_first_column`].
    fn html_section_first_column(report: &str, title: &str) -> Vec<String> {
        // Headings now carry a slug `id=` attribute (issue #622), so match
        // on the text+close (`>{title}</h3>`) rather than the bare open tag.
        let needle = format!(">{title}</h3>");
        let mut names = Vec::new();
        let mut rest = report;
        while let Some(pos) = rest.find(&needle) {
            rest = &rest[pos + needle.len()..];
            let end = rest
                .find("<h3")
                .or_else(|| rest.find("</section>"))
                .unwrap_or(rest.len());
            for row in rest[..end].split("<tr>").skip(1) {
                // First `<td …>text</td>`: split past `<td`, past the tag's
                // `>`, then take the text before `</td>`. Header rows carry
                // `<th>`, so they match nothing. (Cell text is `escape_html`-ed,
                // so the only raw `>` is the tag close.)
                if let Some((_, after_td)) = row.split_once("<td")
                    && let Some((_, cell)) = after_td.split_once('>')
                    && let Some((text, _)) = cell.split_once("</td>")
                {
                    names.push(text.to_string());
                }
            }
        }
        names
    }

    /// The durable "same data" guarantee: the Markdown and HTML reports must
    /// list the identical rows, in the identical order, in every hotspot
    /// section — including the per-metric suppression (the rich fixture's
    /// `secret_internal` is suppressed for cyclomatic) and top-N truncation.
    #[test]
    fn html_and_markdown_report_identical_section_membership() {
        use crate::html_report::generate_html_report;
        let fixture = rich_fixture();
        let md = generate_report(&fixture, 5, SuppressionPolicy::Honor);
        let html = generate_html_report(&fixture, 5, SuppressionPolicy::Honor);

        // Titles come from the shared `#677` template, so both formats render
        // the identical string for each section (no `>` left to escape).
        // Deriving them from `SPECS` keeps the test in step with any future
        // concept/column rename.
        for spec in hotspot::SPECS {
            let title = spec.title.render(5);
            let md_names = md_section_first_column(&md, &title);
            let html_names = html_section_first_column(&html, &title);
            assert_eq!(
                md_names, html_names,
                "section '{title}': Markdown and HTML must list identical rows in identical order"
            );
            assert!(
                !md_names.is_empty(),
                "section '{title}' should be populated by the rich fixture"
            );
        }
    }

    // ── extract_summaries tests ────────────────────────────────────

    #[test]
    fn extract_single_space() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 10);
        let mut out = Vec::new();
        extract_summaries(&space, "src/root.rs", LANG::Rust, "", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].file, "src/root.rs");
        assert_eq!(out[0].name, "root.rs");
        assert_eq!(out[0].kind, SpaceKind::Unit);
        assert_eq!(out[0].start_line, 1);
        assert_eq!(out[0].end_line, 10);
    }

    #[test]
    fn extract_nested_spaces() {
        let mut root = make_space("root.rs", SpaceKind::Unit, 1, 20);
        let func_a = make_space("func_a", SpaceKind::Function, 2, 8);
        let mut class_b = make_space("ClassB", SpaceKind::Class, 10, 18);
        let func_c = make_space("method_c", SpaceKind::Function, 12, 16);
        class_b.spaces.push(func_c);
        root.spaces.push(func_a);
        root.spaces.push(class_b);

        let mut out = Vec::new();
        extract_summaries(&root, "src/root.rs", LANG::Rust, "", &mut out);

        assert_eq!(out.len(), 4);
        assert_eq!(out[0].kind, SpaceKind::Unit);
        assert_eq!(out[1].kind, SpaceKind::Function);
        assert_eq!(out[1].name, "func_a");
        assert_eq!(out[2].kind, SpaceKind::Class);
        assert_eq!(out[2].name, "ClassB");
        assert_eq!(out[3].kind, SpaceKind::Function);
        assert_eq!(out[3].name, "method_c");
        assert_eq!(out[3].start_line, 12);
        assert_eq!(out[3].end_line, 16);
    }

    #[test]
    fn strip_prefix_removes_matching_prefix() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 5);
        let mut out = Vec::new();
        extract_summaries(&space, "src/lib/root.rs", LANG::Rust, "src/lib/", &mut out);
        assert_eq!(out[0].file, "root.rs");
    }

    #[test]
    fn strip_prefix_passthrough_on_mismatch() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 5);
        let mut out = Vec::new();
        extract_summaries(&space, "other/root.rs", LANG::Rust, "src/lib/", &mut out);
        assert_eq!(out[0].file, "other/root.rs");
    }

    #[test]
    fn empty_tree_produces_one_summary() {
        let space = make_space("empty.rs", SpaceKind::Unit, 0, 0);
        let mut out = Vec::new();
        extract_summaries(&space, "empty.rs", LANG::Rust, "", &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn language_propagated_to_all_children() {
        let mut root = make_space("root.py", SpaceKind::Unit, 1, 10);
        root.spaces.push(make_space("f", SpaceKind::Function, 2, 5));

        let mut out = Vec::new();
        extract_summaries(&root, "root.py", LANG::Python, "", &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|s| s.language == LANG::Python));
    }

    #[test]
    fn deeply_nested_spaces_do_not_overflow_stack() {
        // Regression test for issue #338. The prior recursive form of
        // `extract_summaries_inner` walked one Rust stack frame per
        // nested FuncSpace; an adversarially deep AST (chained
        // lambdas, generated parser fixtures) trips the worker
        // thread's default 2 MiB stack. Mirrors the iterative-walk
        // pattern locked in by `ThresholdSet::evaluate_with_policy`
        // (issue #292 in the library crate, lesson 13).
        //
        // Run the walk on a deliberately tiny 256 KiB stack so the
        // assertion is deterministic regardless of release/debug
        // optimization, OS default, or `RUST_MIN_STACK`: any
        // recursive descent at DEPTH frames blows this budget, while
        // the iterative form's working memory is independent of
        // recursion depth. Reverting the production change to the
        // recursive `for child in &space.spaces` /
        // `extract_summaries_inner(child, …)` form aborts this test
        // with `thread … has overflowed its stack` on every supported
        // platform.
        const DEPTH: usize = 50_000;
        const TIGHT_STACK: usize = 256 * 1024;

        let handle = std::thread::Builder::new()
            .stack_size(TIGHT_STACK)
            .spawn(|| {
                // Build a synthetic FuncSpace tree by chaining each
                // level's child onto the previous one's `spaces`.
                // Constructed bottom-up so each `push` is O(1) and
                // the build itself does not recurse.
                let mut current = make_space("f_inner", SpaceKind::Function, DEPTH, DEPTH);
                for i in (0..DEPTH).rev() {
                    let mut parent = if i == 0 {
                        make_space("root.rs", SpaceKind::Unit, 1, DEPTH + 1)
                    } else {
                        make_space(&format!("f_{i}"), SpaceKind::Function, i, DEPTH + 1)
                    };
                    parent.spaces.push(current);
                    current = parent;
                }

                let mut out = Vec::new();
                extract_summaries(&current, "root.rs", LANG::Rust, "", &mut out);

                // DEPTH+1 because the chain has DEPTH wrappers plus
                // the innermost `f_inner` leaf.
                assert_eq!(out.len(), DEPTH + 1);
                // Pre-order: root first, innermost last.
                assert_eq!(out[0].kind, SpaceKind::Unit);
                assert_eq!(out[0].name, "root.rs");
                assert_eq!(out[DEPTH].name, "f_inner");

                // FuncSpace contains `spaces: Vec<FuncSpace>`, so
                // letting the chained tree drop at scope exit walks
                // one frame per nested space and would overflow this
                // tight stack — masking the production-side
                // assertions above with a Drop-side overflow on test
                // exit. The OS reclaims the memory at process exit;
                // this is fine for a test.
                std::mem::forget(current);
            })
            .expect("spawn worker thread with bounded stack");
        handle
            .join()
            .expect("iterative extract_summaries must not overflow even a 256 KiB stack");
    }

    // ── generate_report tests ──────────────────────────────────────

    #[test]
    fn two_language_report_contains_both_sections() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("do_stuff", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("main.py", "main.py", SpaceKind::Unit, LANG::Python),
            make_summary("run", "main.py", SpaceKind::Function, LANG::Python),
        ];
        let report = generate_report(&summaries, 20, SuppressionPolicy::Honor);

        assert!(report.contains("## Rust"), "missing Rust section header");
        assert!(
            report.contains("## Python"),
            "missing Python section header"
        );
        assert!(
            report.contains("## Per-language overview"),
            "missing overview"
        );

        // Overview table has a row for each language. Padding can vary,
        // so collapse runs of spaces before matching.
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| Rust |"),
            "missing Rust overview row in:\n{report}"
        );
        assert!(
            normalized.contains("| Python |"),
            "missing Python overview row in:\n{report}"
        );

        // Global header is a two-column table; totals are their own rows
        // (issue #671). Padding can vary, so collapse spaces before matching.
        assert!(
            normalized.contains("| Files analyzed | 2 |"),
            "missing Files analyzed header row in:\n{report}"
        );
        assert!(
            normalized.contains("| Functions/methods | 2 |"),
            "missing Functions/methods header row in:\n{report}"
        );
        assert!(
            normalized.contains("| Metric | Value |"),
            "global header table missing its column headers in:\n{report}"
        );
        // The right-aligned value column emits a `---:` GFM delimiter; its run
        // of dashes is layout-dependent, but the trailing `:` and closing pipe
        // are not, so match on that anchor.
        assert!(
            normalized.contains("-: |"),
            "global header table missing its GFM delimiter row in:\n{report}"
        );
    }

    #[test]
    fn halstead_section_omitted_when_no_effort() {
        let mut unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        unit.halstead_effort = 0.0;
        let mut func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.halstead_effort = 0.0;
        func.halstead_volume = 0.0;
        func.halstead_bugs = 0.0;

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        assert!(
            !report.contains("### Halstead effort hotspots"),
            "Halstead section should be omitted"
        );
    }

    #[test]
    fn top_n_truncation() {
        let mut summaries = Vec::new();
        summaries.push(make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        ));
        for i in 0..30 {
            let mut f = make_summary(
                &format!("func_{i}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            f.start_line = i + 1;
            f.cyclomatic = (i + 1) as f64;
            f.cognitive = (i + 1) as f64;
            f.halstead_effort = (i + 1) as f64 * 100.0;
            f.sloc = (i + 1) * 5;
            summaries.push(f);
        }
        let report = generate_report(&summaries, 5, SuppressionPolicy::Honor);

        // Count data rows (lines starting with "| `") in each section.
        let sections = [
            "### Cyclomatic complexity hotspots",
            "### Cognitive complexity hotspots",
            "### Halstead effort hotspots",
            "### Function size hotspots",
        ];
        for section_hdr in sections {
            let section_start = report
                .find(section_hdr)
                .unwrap_or_else(|| panic!("missing section: {section_hdr}"));
            let section_text = &report[section_start..];
            // Section ends at the next "###" or "##" — whichever comes first —
            // or end of string. (Take the minimum: the legend now sits at `##`
            // after the per-language sections, so preferring `## ` over `### `
            // would swallow every later subsection — issue #679.)
            let rest = &section_text[1..];
            let section_end = [rest.find("\n## "), rest.find("\n### ")]
                .into_iter()
                .flatten()
                .min()
                .map_or(section_text.len(), |p| p + 1);
            let section_body = &section_text[..section_end];
            let data_rows = section_body
                .lines()
                .filter(|l| l.starts_with("| `"))
                .count();
            assert_eq!(
                data_rows, 5,
                "expected 5 data rows in {section_hdr}, got {data_rows}"
            );
        }
    }

    #[test]
    fn determinism() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("alpha", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("beta", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("main.py", "main.py", SpaceKind::Unit, LANG::Python),
            make_summary("run", "main.py", SpaceKind::Function, LANG::Python),
        ];
        let a = generate_report(&summaries, 10, SuppressionPolicy::Honor);
        let b = generate_report(&summaries, 10, SuppressionPolicy::Honor);
        assert_eq!(a, b, "report must be byte-equal across runs");
    }

    /// Issue #680: with provenance supplied (the command path), the Markdown
    /// report closes with a footer naming the version, date, paths, top-N, and
    /// suppression state. Footer-free `generate_report` (the snapshot path)
    /// emits no footer.
    #[test]
    fn provenance_footer_present_only_when_supplied() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust),
        ];
        let prov = crate::provenance::Provenance {
            version: "9.9.9",
            date: "2026-06-11",
            paths: "src/",
            top: 10,
            policy: SuppressionPolicy::Honor,
        };
        let with = generate_report_with_vcs(
            &summaries,
            10,
            SuppressionPolicy::Honor,
            &advisory::AdvisoryThresholds::DEFAULT,
            None,
            Some(&prov),
        );
        assert!(
            with.contains(
                "Generated by bca 9.9.9 on 2026-06-11 over `src/` \u{2014} top 10 per table, \
                 suppression markers honored."
            ),
            "footer missing or malformed:\n{with}"
        );
        let without = generate_report(&summaries, 10, SuppressionPolicy::Honor);
        assert!(
            !without.contains("Generated by bca"),
            "footer-free path must not emit a provenance line"
        );
    }

    #[test]
    fn cell_escaping_pipe() {
        let mut f = make_summary("foo|bar", "dir/a|b.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 5.0;
        let unit = make_summary("a|b.rs", "dir/a|b.rs", SpaceKind::Unit, LANG::Rust);
        let report = generate_report(&[unit, f], 20, SuppressionPolicy::Honor);
        // The pipe inside the name must be escaped.
        assert!(
            report.contains("foo\\|bar"),
            "pipe in name not escaped: {report}"
        );
        assert!(
            report.contains("a\\|b.rs"),
            "pipe in file not escaped: {report}"
        );
    }

    #[test]
    fn cell_escaping_backtick() {
        let mut f = make_summary("foo`bar", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 5.0;
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let report = generate_report(&[unit, f], 20, SuppressionPolicy::Honor);
        // Backtick in a name is replaced with modifier letter grave accent.
        assert!(
            report.contains("foo\u{02CB}bar"),
            "backtick in name not replaced"
        );
    }

    #[test]
    fn nan_safe_sort_does_not_panic() {
        let mut unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        unit.mi_visual_studio = f64::NAN;
        let mut f = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = f64::NAN;
        f.cognitive = f64::NAN;
        f.halstead_effort = f64::NAN;
        // Must not panic.
        let report = generate_report(&[unit, f], 20, SuppressionPolicy::Honor);
        assert!(report.contains("# Code Quality Metrics Summary"));
    }

    #[test]
    fn sort_by_metric_desc_handles_nan() {
        let mut a = make_summary("a", "a.rs", SpaceKind::Function, LANG::Rust);
        a.cyclomatic = f64::NAN;
        let mut b = make_summary("b", "b.rs", SpaceKind::Function, LANG::Rust);
        b.cyclomatic = 5.0;
        let mut c = make_summary("c", "c.rs", SpaceKind::Function, LANG::Rust);
        c.cyclomatic = 10.0;

        let mut items: Vec<&FunctionSummary> = vec![&a, &b, &c];
        sort_by_metric_desc(&mut items, |s| s.cyclomatic);
        // Must not panic. total_cmp treats NaN as greater than all values,
        // so NaN sorts first in descending order.
        assert_eq!(items[0].name, "a");
        // Non-NaN values are in descending order after NaN.
        assert_eq!(items[1].name, "c");
        assert_eq!(items[2].name, "b");
    }

    #[test]
    fn sort_by_metric_asc_handles_nan() {
        let mut a = make_summary("a", "a.rs", SpaceKind::Unit, LANG::Rust);
        a.mi_visual_studio = f64::NAN;
        let mut b = make_summary("b", "b.rs", SpaceKind::Unit, LANG::Rust);
        b.mi_visual_studio = 30.0;
        let mut c = make_summary("c", "c.rs", SpaceKind::Unit, LANG::Rust);
        c.mi_visual_studio = 10.0;

        let mut items: Vec<&FunctionSummary> = vec![&a, &b, &c];
        sort_by_metric_asc(&mut items, |s| s.mi_visual_studio);
        // Must not panic. Non-NaN values sort ascending, NaN sorts last.
        assert_eq!(items[0].name, "c");
        assert_eq!(items[1].name, "b");
        assert_eq!(items[2].name, "a");
    }

    #[test]
    fn empty_input() {
        let report = generate_report(&[], 20, SuppressionPolicy::Honor);
        // Global header is a two-column table; collapse padding before matching.
        let normalized = collapse_spaces(&report);
        assert!(normalized.contains("| Files analyzed | 0 |"));
        assert!(normalized.contains("| Functions/methods | 0 |"));
        // No per-language sections.
        assert!(!report.contains("## Per-language overview"));
    }

    #[test]
    fn thousands_formatting() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(10_000_000), "10,000,000");
    }

    // ── write_table tests ──────────────────────────────────────────

    #[test]
    fn write_table_pads_left_and_right_columns() {
        let mut out = String::new();
        write_table(
            &mut out,
            &["Name", "Count"],
            &[Align::Left, Align::Right],
            &[
                vec!["a".to_string(), "1".to_string()],
                vec!["longname".to_string(), "1234".to_string()],
            ],
        );
        let expected = "\
| Name     | Count |
| -------- | ----: |
| a        |     1 |
| longname |  1234 |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_handles_empty_rows() {
        let mut out = String::new();
        write_table(&mut out, &["A", "B"], &[Align::Left, Align::Right], &[]);
        // Header (1-char) and right-align separator both expand to the
        // GFM-minimum width of 3.
        let expected = "\
| A   |   B |
| --- | --: |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_widens_to_longest_cell() {
        let mut out = String::new();
        write_table(
            &mut out,
            &["X", "Y"],
            &[Align::Left, Align::Right],
            &[vec!["wide-cell".to_string(), "100".to_string()]],
        );
        // X's column widens to 9 (longest cell), Y's to 3 (min).
        let expected = "\
| X         |   Y |
| --------- | --: |
| wide-cell | 100 |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_counts_chars_not_bytes_for_multibyte_cells() {
        // The grave-accent replacement char (\u{02CB}) is one column in a
        // monospace renderer but takes 3 bytes in UTF-8 — width must use
        // chars().count(), not byte length.
        let mut out = String::new();
        write_table(
            &mut out,
            &["Name"],
            &[Align::Left],
            &[vec!["abc".to_string()], vec!["a\u{02CB}c".to_string()]],
        );
        // Both cells are 3 chars; column width is 3.
        let expected = "\
| Name |
| ---- |
| abc  |
| a\u{02CB}c  |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn title_case_basic() {
        assert_eq!(title_case("rust"), "Rust");
        assert_eq!(title_case("python"), "Python");
        // `title_case` is the generic fallback used by
        // `language_display_name` for slugs that do not parse to a known
        // `LANG`; the actual report headings now go through the display
        // map (see `language_display_name_*` tests).
        assert_eq!(title_case("cpp"), "Cpp");
        assert_eq!(title_case("csharp"), "Csharp");
        assert_eq!(title_case("tsx"), "Tsx");
        // The `/`, `-`, and space delimiters still trigger re-capitalisation
        // for arbitrary inputs (no current language slug uses them).
        assert_eq!(title_case("c/c++"), "C/C++");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn language_display_name_known_slugs() {
        // The punctuation-bearing names #540 deliberately stripped from
        // the machine slug, restored for human headings (#613).
        assert_eq!(language_display_name("cpp"), "C++");
        assert_eq!(language_display_name("csharp"), "C#");
        assert_eq!(language_display_name("javascript"), "JavaScript");
        assert_eq!(language_display_name("typescript"), "TypeScript");
        assert_eq!(language_display_name("tsx"), "TSX");
        assert_eq!(language_display_name("php"), "PHP");
        assert_eq!(language_display_name("irules"), "iRules");
    }

    #[test]
    fn language_display_name_covers_every_lang() {
        // Every `LANG` slug must resolve to a non-empty display name. The
        // `match` in `language_display_name` is exhaustive, so a new
        // language is a compile error there; this guards the runtime
        // contract that no known slug ever falls through to the
        // `title_case` fallback (which would re-expose the raw slug).
        for lang in LANG::into_enum_iter() {
            let slug = lang.name();
            let display = language_display_name(slug);
            assert!(!display.is_empty(), "{slug} produced an empty display name");
            // A known slug must take the borrowed (mapped) arm, never the
            // owned `title_case` fallback reserved for unknown future
            // variants.
            assert!(
                matches!(display, Cow::Borrowed(_)),
                "{slug} fell through to the title_case fallback"
            );
        }
    }

    #[test]
    fn language_display_name_unknown_slug_falls_back() {
        // A slug that does not parse to a `LANG` degrades to `title_case`
        // rather than panicking — the graceful path for a future variant
        // not yet reflected in this build.
        assert_eq!(language_display_name("klingon"), "Klingon");
    }

    #[test]
    fn escape_cell_escapes_backslash_before_pipe() {
        // Regression for #439: a Windows-style path must render literally.
        // `\` is a GFM escape introducer, so an unescaped `src\main.rs`
        // renders as `srcmain.rs` (the `\m` is consumed). Escaping `\` to
        // `\\` keeps the backslash visible.
        assert_eq!(escape_cell("src\\main.rs"), "src\\\\main.rs");
        // `\` is escaped first so the cell cannot break out of the column:
        // `a\|b` becomes `a\\\|b` (escaped backslash + escaped pipe), not
        // `a\\|b` (which would split the cell).
        assert_eq!(escape_cell("a\\|b"), "a\\\\\\|b");
        // A trailing backslash must not consume the closing column border.
        assert_eq!(escape_cell("dir\\"), "dir\\\\");
        // Newlines still collapse to a single space — a cell cannot hold a
        // raw line break.
        assert_eq!(escape_cell("a\nb\rc"), "a b c");
    }

    #[test]
    fn escape_name_wraps_in_backticks() {
        assert_eq!(escape_name("hello"), "`hello`");
        assert_eq!(escape_name("a|b"), "`a\\|b`");
        assert_eq!(escape_name("a`b"), "`a\u{02CB}b`");
        assert_eq!(escape_name("a\nb"), "`a b`");
        // A literal backslash must NOT be doubled inside a code span: GFM
        // treats span content literally, so `escape_cell`'s `\` → `\\` rule
        // would render a visible doubled backslash (issue #846). A PHP-style
        // fully-qualified name `Foo\Bar` must keep its single backslash.
        assert_eq!(escape_name("Foo\\Bar"), "`Foo\\Bar`");
        // A backslash adjacent to a pipe stays single (one `\`) while the
        // pipe still gets its `\|` table escape — input chars a \ | b become
        // span content a \ \ | b, i.e. the literal backslash plus the pipe's
        // own `\|`.
        assert_eq!(escape_name("a\\|b"), "`a\\\\|b`");
    }

    #[test]
    fn actionable_summary_clean() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust),
        ];
        let report = generate_report(&summaries, 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("No major quality concerns detected."),
            "clean codebase should show no-concerns message"
        );
    }

    #[test]
    fn actionable_summary_with_concerns() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut f = make_summary("big_func", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 25.0;
        f.cognitive = 20.0;
        f.sloc = 150;
        f.nargs = 5;
        f.halstead_bugs = 2.0;

        let report = generate_report(&[unit, f], 20, SuppressionPolicy::Honor);
        assert!(report.contains("functions with CC > 10"));
        assert!(report.contains("functions with cognitive complexity > 15"));
        assert!(report.contains("functions with SLOC > 100"));
        assert!(report.contains("functions with more than 3 parameters"));
        assert!(report.contains("functions with estimated Halstead bugs > 1.0"));
        // Default provenance line is emitted unconditionally (issue #630).
        assert!(
            report.contains("Advisory thresholds: built-in defaults"),
            "default advisory provenance must be present:\n{report}"
        );
    }

    /// Issue #630: when a manifest sets per-metric thresholds, the report's
    /// advisory cutoffs (Actionable Summary bullets, CC note bands, Many-
    /// Parameters filter) use them instead of the hardcoded defaults, and the
    /// provenance line names the manifest. A function with CC=12 and 4 args is
    /// over the default cutoffs (CC>10, nargs>3) but under a manifest gating at
    /// cyclomatic=15 / nargs=5, so it must NOT be flagged.
    #[test]
    fn actionable_summary_uses_manifest_thresholds() {
        use advisory::AdvisoryThresholds;
        use std::collections::BTreeMap;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut f = make_summary("borderline", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 12.0;
        f.nargs = 4;
        let summaries = [unit, f];

        // Default advisory: CC>10 and nargs>3 both flag the function.
        let default = generate_report_with_vcs(
            &summaries,
            20,
            SuppressionPolicy::Honor,
            &AdvisoryThresholds::DEFAULT,
            None,
            None,
        );
        assert!(default.contains("functions with CC > 10"), "{default}");
        assert!(
            default.contains("functions with more than 3 parameters"),
            "{default}"
        );

        // Manifest gating at cyclomatic=15 / nargs=5: the same function clears
        // both bars, so neither bullet appears, and the CC-note primary band is
        // `CC > 15` (severe `> 30`, the 2x default multiple).
        let mut hard = BTreeMap::new();
        hard.insert("cyclomatic".to_string(), 15.0);
        hard.insert("nargs".to_string(), 5.0);
        let advisory = AdvisoryThresholds::from_manifest_hard(&hard);
        let manifest = generate_report_with_vcs(
            &summaries,
            20,
            SuppressionPolicy::Honor,
            &advisory,
            None,
            None,
        );
        assert!(
            manifest.contains("Advisory thresholds: sourced from the `bca.toml`"),
            "manifest provenance must be present:\n{manifest}"
        );
        assert!(
            !manifest.contains("functions with CC > 15"),
            "CC=12 is under the manifest cyclomatic=15 cutoff, so no CC bullet:\n{manifest}"
        );
        assert!(
            !manifest.contains("more than 5 parameters"),
            "nargs=4 is under the manifest nargs=5 cutoff, so no params bullet:\n{manifest}"
        );
        // The CC note's bands reflect the manifest cutoff and its 2x severe band.
        assert!(
            manifest.contains("CC > 15") && manifest.contains("CC > 30"),
            "CC note bands must reflect the manifest cutoff (15) and 2x severe (30):\n{manifest}"
        );
    }

    /// Issue #627: the MI section carries the explanatory note (variant,
    /// rating bands, clamping caveat, gate-variant caveat) in the Markdown
    /// report.
    #[test]
    fn mi_section_renders_explanatory_note() {
        let report = generate_report(&rich_fixture(), 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("Visual Studio rescale (mi_visual_studio)"),
            "MI note must name the rendered variant:\n{report}"
        );
        assert!(
            report.contains("GOOD \u{2265} 20, MODERATE \u{2265} 10, else LOW"),
            "MI note must state the rating bands:\n{report}"
        );
        assert!(
            report.contains("Large files commonly clamp to 0"),
            "MI note must state the clamping caveat:\n{report}"
        );
        assert!(
            report.contains("bca check gate may evaluate a different MI variant"),
            "MI note must warn the gate may use a different variant (issue #627 comment):\n{report}"
        );
    }

    /// Issue #616: the three differently-filtered CC statistics must each be
    /// captioned. With one cyclomatic-suppressed function the CC note is
    /// captioned "excluding suppressed functions" while the Actionable Summary
    /// — a raw roll-up that still counts the suppressed function — is captioned
    /// "including 1 suppressed", so a reader can reconcile the differing counts.
    #[test]
    fn cc_statistics_are_captioned_by_population() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut visible = make_summary("visible", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        visible.cyclomatic = 12.0;
        let mut hidden = make_summary("hidden", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hidden.cyclomatic = 30.0;
        hidden.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let summaries = [unit, visible, hidden];
        let report = generate_report(&summaries, 20, SuppressionPolicy::Honor);

        // CC note: captioned, and tallies only the visible function (max 12).
        assert!(
            report.contains(
                "Average CC: 12.0 | Max: 12 | CC > 10: 1 functions | CC > 20: 0 functions \
                 (excluding suppressed functions)"
            ),
            "CC note must be captioned and exclude the suppressed cc=30:\n{report}"
        );
        // Actionable Summary: raw, names the suppressed count so the reader can
        // reconcile it against the filtered CC note (2 functions > 10 here).
        assert!(
            report.contains(
                "Raw counts across all functions; the hotspot tables hide suppressed \
                 rows (cyclomatic: 1) — re-run with --no-suppress to list them."
            ),
            "Actionable Summary must caption its raw population per metric:\n{report}"
        );
        assert!(
            report.contains("**2** functions with CC > 10"),
            "raw summary still counts the suppressed function:\n{report}"
        );

        // Under --no-suppress nothing is hidden: the CC note drops its
        // suppression caption and the summary says markers were ignored.
        let audit = generate_report(&summaries, 20, SuppressionPolicy::Ignore);
        assert!(
            !audit.contains("(excluding suppressed functions)"),
            "no suppression caption when markers are ignored:\n{audit}"
        );
        assert!(
            audit.contains("Raw counts across all functions, ignoring suppression markers."),
            "--no-suppress summary caption omits the suppressed count:\n{audit}"
        );
    }

    /// Issue #616: when suppression hides every row of a hotspot table whose
    /// metric the Actionable Summary still reports, emit an explicit
    /// "table omitted" caption rather than silently dropping the table and
    /// leaving the summary bullet dangling.
    #[test]
    fn fully_suppressed_hotspot_table_is_captioned() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        // The lone many-parameters function is suppressed for nargs, so its
        // detail table is dropped — but the raw summary bullet survives.
        let mut wide = make_summary("wide", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        wide.nargs = 7;
        wide.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Nargs]));

        let report = generate_report(&[unit, wide], 20, SuppressionPolicy::Honor);
        // Issue #681: the section heading is still emitted so the section
        // keeps its place in the heading structure; only the table rows are
        // hidden, with the omission note as the body.
        assert!(
            report.contains("### Many parameters hotspots"),
            "a fully-suppressed table must still emit its heading:\n{report}"
        );
        assert!(
            !report.contains("| wide "),
            "the all-suppressed table must not render its rows:\n{report}"
        );
        assert!(
            report.contains("table omitted: all 1 matching functions suppressed"),
            "a fully-suppressed table must leave an explanatory caption:\n{report}"
        );
        assert!(
            report.contains("**1** functions with more than 3 parameters"),
            "the raw summary bullet still references the suppressed function:\n{report}"
        );
    }

    #[test]
    fn fully_suppressed_cc_table_is_captioned() {
        use big_code_analysis::SuppressionScope;
        use std::collections::BTreeSet;

        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        // The lone high-CC function is suppressed for cyclomatic, so the CC
        // hotspot table (the cc_note branch) is dropped — it must leave the
        // same "table omitted" caption the non-CC tables do (#616).
        let mut hot = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hot.cyclomatic = 25.0;
        hot.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let report = generate_report(&[unit, hot], 20, SuppressionPolicy::Honor);
        // Issue #681: the CC section heading is still emitted; only its rows
        // are hidden, with the omission note as the body.
        assert!(
            report.contains("### Cyclomatic complexity hotspots"),
            "a fully-suppressed CC table must still emit its heading:\n{report}"
        );
        assert!(
            !report.contains("| hot "),
            "the all-suppressed CC table must not render its rows:\n{report}"
        );
        assert!(
            report.contains("table omitted: all 1 matching functions suppressed"),
            "a fully-suppressed CC table must leave an explanatory caption:\n{report}"
        );
        assert!(
            report.contains("**1** functions with CC > 10"),
            "the raw summary bullet still references the suppressed function:\n{report}"
        );
    }

    #[test]
    fn mi_table_shows_lowest_first() {
        let mut unit_good = make_summary("good.rs", "good.rs", SpaceKind::Unit, LANG::Rust);
        unit_good.mi_visual_studio = 80.0;
        let mut unit_bad = make_summary("bad.rs", "bad.rs", SpaceKind::Unit, LANG::Rust);
        unit_bad.mi_visual_studio = 15.0;

        let report = generate_report(&[unit_good, unit_bad], 20, SuppressionPolicy::Honor);
        // The bad file should appear first in the MI table.
        let mi_section = report
            .find("### Maintainability Index")
            .expect("MI section missing");
        let after_mi = &report[mi_section..];
        let bad_pos = after_mi.find("bad.rs").expect("bad.rs missing in MI");
        let good_pos = after_mi.find("good.rs").expect("good.rs missing in MI");
        assert!(
            bad_pos < good_pos,
            "lowest MI file should appear first in MI table"
        );
    }

    // ── WMC / NEXITS / ABC section tests ───────────────────────────

    #[test]
    fn wmc_section_present_with_class_summaries() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut cls = make_summary("MyClass", "src/lib.rs", SpaceKind::Class, LANG::Rust);
        cls.wmc = 12.0;
        cls.nom = 4;
        cls.npa = 2.0;
        cls.npm = 3.0;
        cls.sloc = 80;
        let func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);

        let report = generate_report(&[unit, cls, func], 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("### Type hotspots"),
            "WMC section should be present when class-kind summaries exist"
        );
        // Verify the row renders the correct metric values. Padding may
        // pad cells with spaces; collapse runs of spaces before matching.
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `MyClass`"),
            "class name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 12 | 4 | 2 | 3 | 80 | 30 |"),
            "WMC row should contain wmc=12, nom=4, npa=2, npm=3, sloc=80, tokens=30 in:\n{report}"
        );
    }

    #[test]
    fn wmc_section_omitted_without_classes() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        assert!(
            !report.contains("### Type hotspots"),
            "WMC section should be absent when no class-kind summaries exist"
        );
    }

    #[test]
    fn nexits_section_present() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("multi_exit", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.nexits = 3;
        func.cyclomatic = 7.0;
        func.sloc = 40;

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("### Exit points hotspots"),
            "NEXITS section should be present when functions have exits > 0"
        );
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `multi_exit`"),
            "function name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 3 | 7 | 40 | 30 |"),
            "NEXITS row should contain exits=3, cc=7, sloc=40, tokens=30 in:\n{report}"
        );
    }

    /// Issue #689: a single `return` is the baseline, not a hotspot. The
    /// exit-points table admits only functions whose `nexits` clears the
    /// floor (`> 2`); a codebase whose worst function has `nexits == 2` omits
    /// the section entirely rather than listing noise.
    #[test]
    fn nexits_floor_omits_baseline_and_keeps_only_over_floor() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        // At/under the floor (2): never a hotspot.
        let mut baseline = make_summary("baseline", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        baseline.nexits = 2;
        let report = generate_report(
            &[
                make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
                baseline,
            ],
            20,
            SuppressionPolicy::Honor,
        );
        assert!(
            !report.contains("### Exit points hotspots"),
            "a max-nexits-2 codebase must omit the exit-points section:\n{report}"
        );

        // One function over the floor, one at it: only the former is listed.
        let mut hot = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        hot.nexits = 5;
        let mut at_floor = make_summary("at_floor", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        at_floor.nexits = 2;
        let report = generate_report(&[unit, hot, at_floor], 20, SuppressionPolicy::Honor);
        let body = section_body(&report, "### Exit points hotspots");
        assert!(
            body.contains("`hot`"),
            "over-floor function must be listed:\n{body}"
        );
        assert!(
            !body.contains("`at_floor`"),
            "at-floor function must not be listed:\n{body}"
        );
    }

    #[test]
    fn abc_section_present() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("complex", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.abc = 15.5;
        func.sloc = 35;

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("### ABC magnitude hotspots"),
            "ABC section should be present when functions have abc > 0"
        );
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `complex`"),
            "function name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 15.5 | 35 | 30 |"),
            "ABC row should contain abc=15.5, sloc=35, tokens=30 in:\n{report}"
        );
    }

    #[test]
    fn top_n_truncation_wmc_nexits_abc() {
        let mut summaries = Vec::new();
        summaries.push(make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        ));
        // 10 classes for WMC truncation.
        for i in 0..10 {
            let mut cls = make_summary(
                &format!("Class_{i}"),
                "src/lib.rs",
                SpaceKind::Class,
                LANG::Rust,
            );
            cls.wmc = (i + 1) as f64;
            cls.start_line = 100 + i;
            summaries.push(cls);
        }
        // 10 functions for NEXITS and ABC truncation.
        for i in 0..10 {
            let mut f = make_summary(
                &format!("func_{i}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            f.nexits = i + 1;
            f.abc = (i + 1) as f64 * 2.0;
            f.start_line = 200 + i;
            summaries.push(f);
        }
        let report = generate_report(&summaries, 3, SuppressionPolicy::Honor);

        let sections = [
            "### Type hotspots",
            "### Exit points hotspots",
            "### ABC magnitude hotspots",
        ];
        for section_hdr in sections {
            let section_start = report
                .find(section_hdr)
                .unwrap_or_else(|| panic!("missing section: {section_hdr}"));
            let section_text = &report[section_start..];
            // Next `### `/`## ` heading, whichever comes first (#679 moved the
            // legend to `##` after the per-language sections).
            let rest = &section_text[1..];
            let section_end = [rest.find("\n## "), rest.find("\n### ")]
                .into_iter()
                .flatten()
                .min()
                .map_or(section_text.len(), |p| p + 1);
            let section_body = &section_text[..section_end];
            let data_rows = section_body
                .lines()
                .filter(|l| l.starts_with("| `"))
                .count();
            assert_eq!(
                data_rows, 3,
                "expected 3 data rows in {section_hdr}, got {data_rows}"
            );
        }
    }

    #[test]
    fn tokens_column_present_in_hotspot_tables() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 5.0;
        func.cognitive = 4.0;
        func.halstead_effort = 200.0;
        func.nargs = 4;
        // > NEXITS_HOTSPOT_FLOOR (2) so the exit-points section renders (#689).
        func.nexits = 3;
        func.abc = 8.0;
        func.tokens = 42;
        let mut cls = make_summary("Cls", "src/lib.rs", SpaceKind::Class, LANG::Rust);
        cls.wmc = 6.0;
        cls.tokens = 99;

        let report = generate_report(&[unit, func, cls], 20, SuppressionPolicy::Honor);

        for header in [
            "### Maintainability Index",
            "### Cyclomatic complexity hotspots",
            "### Cognitive complexity hotspots",
            "### Halstead effort hotspots",
            "### Function size hotspots",
            "### Many parameters hotspots",
            "### Type hotspots",
            "### Exit points hotspots",
            "### ABC magnitude hotspots",
        ] {
            let start = report
                .find(header)
                .unwrap_or_else(|| panic!("missing section: {header}"));
            let header_row = report[start..]
                .lines()
                .find(|l| l.starts_with('|'))
                .expect("header row");
            assert!(
                header_row.contains("Tokens"),
                "Tokens column missing from {header} header row:\n{header_row}"
            );
        }

        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| 42 |"),
            "function token count should appear in normalized report"
        );
        assert!(
            normalized.contains("| 99 |"),
            "class token count should appear in normalized report"
        );
    }

    /// Issue #628: the Halstead Effort and Many-Parameters tables were the
    /// only per-function hotspots missing the `Line` column. Both renderers
    /// draw from the shared `hotspot::SPECS`, so one fix covers both — assert
    /// the header is now present in each format for each table.
    #[test]
    fn line_column_present_in_halstead_and_many_params() {
        use crate::html_report::generate_html_report;
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        // One function that surfaces in both tables: nargs > 3 (Many-Params
        // gate) and a positive Halstead effort.
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.halstead_effort = 200.0;
        func.nargs = 4;
        let fixture = [unit, func];

        let md = generate_report(&fixture, 20, SuppressionPolicy::Honor);
        let html = generate_html_report(&fixture, 20, SuppressionPolicy::Honor);

        for header in ["Halstead effort hotspots", "Many parameters hotspots"] {
            // Markdown: the header row immediately after the section heading.
            let md_hdr = format!("### {header}");
            let start = md
                .find(&md_hdr)
                .unwrap_or_else(|| panic!("missing Markdown section: {header}"));
            let header_row = md[start..]
                .lines()
                .find(|l| l.starts_with('|'))
                .expect("Markdown header row");
            assert!(
                header_row.contains("Line"),
                "Line column missing from Markdown {header} header row:\n{header_row}"
            );

            // HTML: the section emits a `<th>Line</th>` in its header row.
            // The section title now also appears as a TOC link (issue #685),
            // so match the heading element's text-close (`>{header}…</h3>`),
            // not a bare `>{header}` that would hit the nav `<a>` first.
            let title_needle = format!(">{header}");
            let start = html
                .match_indices(&title_needle)
                .map(|(i, _)| i)
                .find(|&i| {
                    html[i..]
                        .find('<')
                        .is_some_and(|lt| html[i + lt..].starts_with("</h3>"))
                })
                .unwrap_or_else(|| panic!("missing HTML section: {header}"));
            let table_slice = &html[start..];
            let body_start = table_slice.find("<tbody").unwrap_or(table_slice.len());
            assert!(
                table_slice[..body_start].contains(">Line</th>"),
                "Line column missing from HTML {header} header"
            );
        }
    }

    #[test]
    fn nexits_present_abc_absent() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary(
            "early_return",
            "src/lib.rs",
            SpaceKind::Function,
            LANG::Rust,
        );
        // > NEXITS_HOTSPOT_FLOOR (2) so the exit-points section renders (#689).
        func.nexits = 3;
        func.abc = 0.0;

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        assert!(
            report.contains("### Exit points hotspots"),
            "NEXITS section should be present"
        );
        assert!(
            !report.contains("### ABC magnitude hotspots"),
            "ABC section should be absent when all abc values are 0"
        );
    }

    // ── suppression-marker honoring (issue #501) ───────────────────

    use std::collections::BTreeSet;

    /// A `bca: suppress(cyclomatic)` marker on the function-local scope
    /// must drop it from the Cyclomatic table by default while leaving it
    /// in the Cognitive table — per-metric, not all-or-nothing.
    #[test]
    fn function_scope_suppression_drops_only_its_metric_table() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 25.0;
        func.cognitive = 18.0;
        func.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);

        let cc = section_body(&report, "### Cyclomatic complexity hotspots");
        assert!(
            !cc.contains("`hot`"),
            "suppressed function must be omitted from the Cyclomatic table:\n{cc}"
        );
        let cog = section_body(&report, "### Cognitive complexity hotspots");
        assert!(
            cog.contains("`hot`"),
            "function suppressed only for cyclomatic must still appear in Cognitive:\n{cog}"
        );
    }

    /// `--no-suppress` (SuppressionPolicy::Ignore) re-includes the same
    /// function in its suppressed metric's table — the audit view.
    #[test]
    fn no_suppress_policy_includes_suppressed_function() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 25.0;
        func.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Ignore);
        let cc = section_body(&report, "### Cyclomatic complexity hotspots");
        assert!(
            cc.contains("`hot`"),
            "--no-suppress must include the suppressed function:\n{cc}"
        );
    }

    /// A whole-file `bca: suppress-file` (scope `All`) lives on the Unit
    /// and is folded into every function by `extract_summaries`; here we
    /// simulate the merged effect on a function summary and confirm it is
    /// dropped from a hotspot table by default.
    #[test]
    fn file_scope_suppression_all_drops_function_from_table() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 25.0;
        func.cognitive = 18.0;
        // `suppress-file` (no metric list) is `All`; extract_summaries
        // merges it onto each function's effective scope.
        func.suppressed = SuppressionScope::All;

        let report = generate_report(&[unit, func], 20, SuppressionPolicy::Honor);
        let cc = section_body(&report, "### Cyclomatic complexity hotspots");
        assert!(
            !cc.contains("`hot`"),
            "file-scoped suppress-all must hide the function from every table:\n{cc}"
        );
        // Issue #681: with the only function suppressed, the Cognitive
        // section still emits its heading; its body is the omission note and
        // no row is rendered.
        let cog = section_body(&report, "### Cognitive complexity hotspots");
        assert!(
            !cog.contains("`hot`"),
            "the suppressed function must not appear in the Cognitive section:\n{cog}"
        );
        assert!(
            cog.contains("table omitted: all 1 matching functions suppressed"),
            "the fully-suppressed Cognitive section's body is the omission note:\n{cog}"
        );

        let report_audit = generate_report(&[unit2(), func_all()], 20, SuppressionPolicy::Ignore);
        let cc_audit = section_body(&report_audit, "### Cyclomatic complexity hotspots");
        assert!(
            cc_audit.contains("`hot`"),
            "--no-suppress must include the file-suppressed function:\n{cc_audit}"
        );
    }

    /// `Metric::Nexits` keys the NEXITS hotspot table: a
    /// `bca: suppress(nexits)` marker must drop the function from that
    /// table (post-#555 the suppression and threshold vocabularies share
    /// the canonical `nexits` spelling — no `exit` alias).
    #[test]
    fn exit_suppression_drops_function_from_nexits_table() {
        const NEXITS_HEADER: &str = "### Exit points hotspots";
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("multi_exit", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.nexits = 5;
        func.cyclomatic = 0.0;
        func.cognitive = 0.0;
        func.abc = 0.0;
        func.halstead_effort = 0.0;
        func.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Nexits]));

        let summaries = vec![unit, func];

        let honored = generate_report(&summaries, 20, SuppressionPolicy::Honor);
        // Issue #681: the NEXITS heading is still emitted; the suppressed
        // function is absent from the body and the omission note is the body.
        let nexits = section_body(&honored, NEXITS_HEADER);
        assert!(
            !nexits.contains("`multi_exit`"),
            "exit-suppressed function must be omitted from the NEXITS table:\n{nexits}"
        );
        assert!(
            nexits.contains("table omitted: all 1 matching functions suppressed"),
            "the fully-suppressed NEXITS section's body is the omission note:\n{nexits}"
        );

        // Positive control: the only thing keeping the table empty is the
        // suppression. Under --no-suppress the same function (nexits = 5)
        // is a hotspot and the table renders — so the absence above is
        // attributable to the exit-alias filter, not to a missing hotspot.
        let bypassed = generate_report(&summaries, 20, SuppressionPolicy::Ignore);
        assert!(
            bypassed.contains(NEXITS_HEADER),
            "--no-suppress must restore the exit-suppressed function:\n{bypassed}"
        );
    }

    /// `extract_summaries` must fold the Unit's `suppress-file` scope into
    /// every descendant function's effective `suppressed` scope.
    #[test]
    fn extract_summaries_folds_file_scope_into_functions() {
        let mut root = make_space("root.rs", SpaceKind::Unit, 1, 20);
        root.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Halstead]));
        let mut func = make_space("f", SpaceKind::Function, 2, 8);
        func.suppressed = SuppressionScope::Some(BTreeSet::from([Metric::Cyclomatic]));
        root.spaces.push(func);

        let mut out = Vec::new();
        extract_summaries(&root, "root.rs", LANG::Rust, "", &mut out);
        assert_eq!(out.len(), 2);
        // out[1] is the function (pre-order: root then child). Its
        // effective scope is the union of the file scope (halstead) and
        // its own scope (cyclomatic).
        let f = &out[1];
        assert_eq!(f.name, "f");
        assert!(f.suppressed.covers(Metric::Halstead), "file scope merged");
        assert!(f.suppressed.covers(Metric::Cyclomatic), "own scope kept");
        assert!(
            !f.suppressed.covers(Metric::Cognitive),
            "unrelated metric untouched"
        );
    }

    /// Helpers for `file_scope_suppression_all_drops_function_from_table`'s
    /// audit-view assertion (the first pair is moved by value into the
    /// honor-view report).
    fn unit2() -> FunctionSummary {
        make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust)
    }
    fn func_all() -> FunctionSummary {
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 25.0;
        func.suppressed = SuppressionScope::All;
        func
    }

    /// Slice the rendered report from `header` to the next `##`/`###`
    /// heading (or end), so a per-section membership assertion does not
    /// accidentally match a name appearing in a different table.
    fn section_body<'a>(report: &'a str, header: &str) -> &'a str {
        let Some(start) = report.find(header) else {
            return "";
        };
        let rest = &report[start..];
        let after = &rest[1..];
        // First `### `/`## ` boundary, whichever is nearer (#679 placed the
        // legend at `##` after the per-language sections).
        let end = [after.find("\n## "), after.find("\n### ")]
            .into_iter()
            .flatten()
            .min()
            .map_or(rest.len(), |p| p + 1);
        &rest[..end]
    }
}
