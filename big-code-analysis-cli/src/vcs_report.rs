// bca: suppress-file(halstead, nargs, nom)
// The `VCS_SPECS` table is declarative data: ~17 capture-free cell-projector
// closures inflate the file-level nom/nargs/halstead sums (each closure is a
// one-arg "function"), the same string-formatting / many-fn aggregation
// artifact the sibling report renderers suppress (see
// `markdown_report/hotspot.rs`) — not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

//! Rendered change-history (VCS) report (issue #573).
//!
//! Produces the Markdown and HTML pages for `bca vcs --format
//! markdown|html`. Mirrors the AST report's shared-spec design: the
//! column set lives in one [`VCS_SPECS`] table that both formats
//! consume, so Markdown and HTML cannot drift (guarded by
//! `markdown_and_html_columns_match`). Table markup and escaping are
//! delegated to the existing renderers' `write_table` helpers
//! (`crate::markdown_report::write_table` /
//! `crate::html_report::write_table`, or `write_table_classed` for the
//! HTML severity-heat tint on the `risk_score` cell, see
//! [`risk_heat_class`]) and the shared HTML scaffolding
//! (`write_html_head` / `write_html_tail`), so the page carries
//! `report.html`'s styling and inline click-to-sort behaviour.

use std::fmt::Write as _;

use crate::html_report::{
    Headings, RankedColumn, write_html_head, write_html_tail, write_legend_html,
    write_table_classed_with_tooltips as write_html_table_classed, write_table_with_tooltips,
};
use crate::markdown_report::hotspot::{self, Cell};
use crate::markdown_report::{
    Align, render_cell_md, thousands, write_legend as write_md_legend,
    write_table as write_md_table,
};
use crate::vcs_command::{FileEntry, Report};

/// One column of the change-history table: header, alignment, a
/// capture-free projector to a [`Cell`], and an optional plain-English
/// `tooltip`. The `rank` argument (1-based) lets the Rank column be part
/// of the shared spec rather than special-cased in each renderer, where it
/// could drift. The `tooltip` is the single source of a column's
/// definition — both the HTML `title=` attribute and the rendered legend
/// draw from it (issue #611); identity columns (Rank, File) carry `None`.
struct VcsColumn {
    header: &'static str,
    align: Align,
    cell: fn(rank: usize, &FileEntry) -> Cell,
    tooltip: Option<&'static str>,
}

// Plain-English definitions for the change-history columns, defined once
// and shared by the recent/total aliased pairs so the HTML tooltip and the
// legend cannot drift. Moved here from `html_report::VCS_HEADER_TOOLTIPS`
// (issue #611) to make `VCS_SPECS` the single source of truth.
const RISK_TOOLTIP: &str = "Composite change-history risk score: recent churn and commit frequency dominate, raised by author dilution, bug-/security-fix history, and newness. Ordinal: only relative ranks carry meaning, not the absolute value.";
const COMMITS_TOOLTIP: &str = "Distinct commits that touched this file within the analysis window.";
const CHURN_TOOLTIP: &str = "Lines added + deleted to this file within the analysis window.";
const AUTHORS_TOOLTIP: &str = "Distinct authors who touched this file within the analysis window.";
const OWNERSHIP_TOOLTIP: &str = "Top-author edit share (0\u{2013}1): fraction of edits by the single most active author; lower means more diluted ownership.";
const BURST_TOOLTIP: &str =
    "Recency of change (0\u{2013}1): recent commits as a share of long-window commits.";
const BUG_FIXES_TOOLTIP: &str =
    "Commits classified as bug fixes (by message) within the long window.";
const SEC_FIXES_TOOLTIP: &str = "Commits classified as security fixes (by message) within the long window; weighted more heavily than bug fixes.";
const REVERTS_TOOLTIP: &str = "Revert commits touching this file within the long window.";
const AGE_TOOLTIP: &str =
    "Days since the first in-window commit touching this file (capped at the long window).";
const LAST_MOD_TOOLTIP: &str = "Days since the most recent in-window commit touching this file.";
const CHANGE_ENTROPY_TOOLTIP: &str = "Change entropy (bits): how scattered the changes to this file are across commits (Hassan 2009). Higher means more diffuse, fault-prone change.";
const COCHANGE_ENTROPY_TOOLTIP: &str = "Co-change entropy (bits): how widely changes to this file ripple to other files. Higher means coupling to many different partners.";
const HOTSPOT_TOOLTIP: &str = "Complexity \u{D7} recent churn: high-complexity files that also change often. Shown only when AST metrics are joined (e.g. report --vcs); omitted by plain bca vcs.";

/// Format an integer count as a `Cell::Num` with comma thousands
/// separators, so the VCS table renders `15,973` like the AST tables
/// rather than the bare `15973` it used to (issue #618). Counts are
/// `u32` / `u64`; the conversion to the `thousands` helper's `usize`
/// saturates rather than wrapping on a (practically unreachable) overflow,
/// keeping a sane render instead of a wrong one.
fn num_cell(n: u64) -> Cell {
    Cell::Num(thousands(usize::try_from(n).unwrap_or(usize::MAX)))
}

/// The change-history columns, defined once and rendered identically by
/// both formats. Order and content mirror the structured CSV record (so
/// the rendered page is the complete, sortable view of the same data),
/// with a leading Rank column.
const VCS_SPECS: &[VcsColumn] = &[
    VcsColumn {
        header: "Rank",
        align: Align::Right,
        cell: |rank, _| num_cell(rank as u64),
        tooltip: None,
    },
    VcsColumn {
        header: "File",
        align: Align::Left,
        cell: |_, e| Cell::Path(e.path.clone()),
        tooltip: None,
    },
    VcsColumn {
        header: "Risk",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.1}", e.vcs.risk_score)),
        tooltip: Some(RISK_TOOLTIP),
    },
    VcsColumn {
        header: "Commits (recent)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.commits_recent.into()),
        tooltip: Some(COMMITS_TOOLTIP),
    },
    VcsColumn {
        header: "Commits (total)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.commits_long.into()),
        tooltip: Some(COMMITS_TOOLTIP),
    },
    VcsColumn {
        header: "Churn (recent)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.churn_recent),
        tooltip: Some(CHURN_TOOLTIP),
    },
    VcsColumn {
        header: "Churn (total)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.churn_long),
        tooltip: Some(CHURN_TOOLTIP),
    },
    VcsColumn {
        header: "Authors (recent)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.authors_recent.into()),
        tooltip: Some(AUTHORS_TOOLTIP),
    },
    VcsColumn {
        header: "Authors (total)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.authors_long.into()),
        tooltip: Some(AUTHORS_TOOLTIP),
    },
    VcsColumn {
        header: "Ownership",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.ownership_top_share)),
        tooltip: Some(OWNERSHIP_TOOLTIP),
    },
    VcsColumn {
        header: "Burst",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.burst)),
        tooltip: Some(BURST_TOOLTIP),
    },
    VcsColumn {
        header: "Bug fixes",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.bug_fix_commits.into()),
        tooltip: Some(BUG_FIXES_TOOLTIP),
    },
    VcsColumn {
        header: "Sec fixes",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.security_fix_commits.into()),
        tooltip: Some(SEC_FIXES_TOOLTIP),
    },
    VcsColumn {
        header: "Reverts",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.revert_commits.into()),
        tooltip: Some(REVERTS_TOOLTIP),
    },
    VcsColumn {
        header: "Age (d)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.age_days.into()),
        tooltip: Some(AGE_TOOLTIP),
    },
    VcsColumn {
        header: "Last mod (d)",
        align: Align::Right,
        cell: |_, e| num_cell(e.vcs.last_modified_days.into()),
        tooltip: Some(LAST_MOD_TOOLTIP),
    },
    VcsColumn {
        header: "Change entropy (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.change_entropy_recent)),
        tooltip: Some(CHANGE_ENTROPY_TOOLTIP),
    },
    VcsColumn {
        header: "Change entropy (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.change_entropy_long)),
        tooltip: Some(CHANGE_ENTROPY_TOOLTIP),
    },
    VcsColumn {
        header: "Co-change entropy (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.cochange_entropy_recent)),
        tooltip: Some(COCHANGE_ENTROPY_TOOLTIP),
    },
    VcsColumn {
        header: "Co-change entropy (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.cochange_entropy_long)),
        tooltip: Some(COCHANGE_ENTROPY_TOOLTIP),
    },
    VcsColumn {
        header: HOTSPOT_HEADER,
        align: Align::Right,
        // Rendered only when some row carries a score (see `active_specs`);
        // the `unwrap_or_default` is a defensive blank for the unreachable
        // mixed case where the column survives but a single row is `None`.
        cell: |_, e| {
            Cell::Num(
                e.vcs
                    .hotspot_score
                    .map(|h| format!("{h:.1}"))
                    .unwrap_or_default(),
            )
        },
        tooltip: Some(HOTSPOT_TOOLTIP),
    },
];

/// Header of the Hotspot column. The column is data-dependent: it renders
/// only when at least one row carries a `hotspot_score` (i.e. AST metrics
/// were joined, as in `report --vcs`), and is dropped entirely otherwise so
/// plain `bca vcs` shows no permanently-blank column (issue #615). Named so
/// [`active_specs`] can match it structurally rather than by position.
const HOTSPOT_HEADER: &str = "Hotspot";

/// The columns to render for `report`: every spec, minus the Hotspot column
/// when no row carries a score. Both formats and the legend draw from this
/// one function so they cannot drift (the cross-format parity guard checks
/// the result, not the raw `VCS_SPECS`).
fn active_specs(report: &Report) -> Vec<&'static VcsColumn> {
    let any_hotspot = report.files.iter().any(|e| e.vcs.hotspot_score.is_some());
    VCS_SPECS
        .iter()
        .filter(|c| any_hotspot || c.header != HOTSPOT_HEADER)
        .collect()
}

/// Header of the column that carries the severity-heat tint in the HTML
/// report. Matched structurally against [`VCS_SPECS`] (see
/// [`risk_column_index`]) so the heat lands on the right `<td>` even if
/// the column order changes; the Markdown path ignores it entirely.
const RISK_COLUMN_HEADER: &str = "Risk";

/// Number of discrete severity-heat bands for the `risk_score` cell.
/// Five bands give a green→yellow→red gradient coarse enough to stay
/// WCAG-contrast-correct and snapshot-stable, yet fine enough to separate
/// the riskiest files. `risk_score` is *ordinal*, so the band derives
/// from each row's rank, never the absolute value — see [`risk_heat_class`].
const HEAT_BAND_COUNT: usize = 5;

/// CSS class names for the five severity-heat bands, most-severe first.
/// Index 0 (`risk-heat-0`, red) tints the top-ranked rows and index 4
/// (`risk-heat-4`, green) the lowest-ranked. Matching rules live in
/// `html_report::INLINE_CSS`.
const HEAT_CLASSES: [&str; HEAT_BAND_COUNT] = [
    "risk-heat-0",
    "risk-heat-1",
    "risk-heat-2",
    "risk-heat-3",
    "risk-heat-4",
];

/// Index of the risk column within [`VCS_SPECS`], or `None` if no column
/// carries [`RISK_COLUMN_HEADER`] (a spec edit would have to remove it).
fn risk_column_index() -> Option<usize> {
    VCS_SPECS
        .iter()
        .position(|c| c.header == RISK_COLUMN_HEADER)
}

/// Severity-heat CSS class for the risk cell of the `row`-th file in a
/// report of `n_rows` risk-ranked files, or `None` when no band applies.
///
/// The band is a function of *rank*, not the raw `risk_score`:
/// `report.files` is sorted by descending risk, so `row` (0-based) is the
/// rank. Splitting `[0, n_rows)` into [`HEAT_BAND_COUNT`] equal-width
/// slices maps the top rows to band 0 (most severe) and the bottom rows to
/// the last band (least severe), independent of the absolute values.
///
/// Edge case: a single row has no ranking to express, so it gets the
/// least-severe band (a lone file should not look alarming); `n_rows == 0`
/// never calls this (no rows are rendered).
fn risk_heat_class(row: usize, n_rows: usize) -> Option<&'static str> {
    if n_rows <= 1 {
        return HEAT_CLASSES.last().copied();
    }
    // Equal-width rank slices: floor(row * BANDS / n_rows), in range since `row < n_rows`.
    let band = (row * HEAT_BAND_COUNT) / n_rows;
    HEAT_CLASSES.get(band).copied()
}

/// The shared page/section heading.
const HEADING: &str = "Change-history risk";

/// Heading level for subsections (Bus factor, Legend) on the *standalone*
/// page, whose title is an `#`/`<h1>`: subsections start one level deeper
/// at `##`/`<h2>` so the outline has no MD001 gap (issue #618).
const STANDALONE_SUBSECTION_LEVEL: usize = 2;

/// Heading level for subsections when the report is *embedded* in
/// `bca report --vcs`, whose section header is `##`/`<h2>`: subsections
/// start at `###`/`<h3>`.
const EMBEDDED_SUBSECTION_LEVEL: usize = 3;

/// Shown in both formats when no tracked file matched the walk filters.
const EMPTY_MESSAGE: &str = "No tracked files matched.";

fn headers(specs: &[&VcsColumn]) -> Vec<&'static str> {
    specs.iter().map(|c| c.header).collect()
}

fn aligns(specs: &[&VcsColumn]) -> Vec<Align> {
    specs.iter().map(|c| c.align).collect()
}

/// The ranked rows as `Cell`s — one inner `Vec` per file, in column
/// order. `report.files` is already risk-ranked, so the enumeration
/// index drives the 1-based Rank column.
fn cell_rows(report: &Report, specs: &[&VcsColumn]) -> Vec<Vec<Cell>> {
    report
        .files
        .iter()
        .enumerate()
        .map(|(i, entry)| specs.iter().map(|c| (c.cell)(i + 1, entry)).collect())
        .collect()
}

/// Extract a `Cell`'s text payload for HTML, where `write_table` escapes
/// every kind uniformly (so the kind itself does not matter here).
fn cell_text(cell: Cell) -> String {
    let (Cell::Name(t) | Cell::Path(t) | Cell::Num(t)) = cell;
    t
}

/// One-line provenance shared by both formats: window lengths and the
/// risk-formula version, plus the ordinal-only caveat. The
/// `vcs_schema_version` is wire jargon (it stamps the structured CSV/JSON
/// record, not anything a human reader can act on) and is deliberately
/// omitted from the rendered formats — it stays in the structured output.
/// The Risk column is *ordinal*: only relative ranks carry meaning, never
/// the absolute magnitude (see `src/vcs/score.rs`), so the page says so
/// rather than presenting `Risk | 11.9` as if its value had a scale.
fn provenance(report: &Report) -> String {
    format!(
        "Long window {}d, recent window {}d, risk formula v{}. \
         Risk is ordinal: only relative ranks carry meaning, not the absolute value.",
        report.long_window_days, report.recent_window_days, report.risk_score_version,
    )
}

const SHALLOW_NOTE: &str =
    "Shallow clone detected: history is truncated, so counts are lower bounds.";

// --- Markdown ---------------------------------------------------------

/// Render the standalone Markdown change-history page (`#` title +
/// provenance + table).
pub(crate) fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {HEADING}\n");
    // The page title is `#` (h1), so its subsections start at `##` (h2):
    // a level-1 title with level-3 subsections skips h2 and trips MD001
    // (the very lint this tool runs over its own Markdown) — issue #618.
    write_markdown_body(&mut out, report, STANDALONE_SUBSECTION_LEVEL);
    out
}

/// Append the change-history section under a level-2 heading, for
/// embedding in the aggregated `bca report --vcs` page (whose global
/// header owns the `#` H1). Paths are the canonical repo-relative git
/// paths from the index — `bca report --strip-prefix` deliberately does
/// not rewrite them, since they are not filesystem walk paths.
pub(crate) fn push_markdown_section(out: &mut String, report: &Report) {
    let _ = writeln!(out, "\n## {HEADING}\n");
    // Embedded section header is `##` (h2), so its subsections start at
    // `###` (h3), keeping the document outline gap-free either way.
    write_markdown_body(out, report, EMBEDDED_SUBSECTION_LEVEL);
}

/// Provenance line + ranked table (or the empty-set message). No
/// heading — the caller supplies the right level. `subsection_level` is
/// the heading depth for the Bus factor and Legend subsections, one level
/// below whatever heading the caller emitted for this section.
fn write_markdown_body(out: &mut String, report: &Report, subsection_level: usize) {
    let _ = writeln!(out, "_{}_", provenance(report));
    if report.truncated_shallow_clone {
        let _ = writeln!(out, "\n> **Note:** {SHALLOW_NOTE}");
    }
    out.push('\n');
    let specs = active_specs(report);
    if report.files.is_empty() {
        let _ = writeln!(out, "{EMPTY_MESSAGE}");
    } else {
        let rows: Vec<Vec<String>> = cell_rows(report, &specs)
            .into_iter()
            .map(|row| row.into_iter().map(render_cell_md).collect())
            .collect();
        write_md_table(out, &headers(&specs), &aligns(&specs), &rows);
    }
    if let Some(aggregate) = &report.vcs_aggregate {
        write_markdown_bus_factor(out, &aggregate.bus_factor, subsection_level);
    }
    // Footer legend defining every rendered change-history column (issue
    // #611); an omitted Hotspot column (issue #615) drops its legend entry.
    if !report.files.is_empty() {
        write_md_legend(out, subsection_level, &legend_entries(&specs));
    }
}

/// Column headers / alignments for the per-directory bus-factor table,
/// shared by both formats so they cannot drift.
fn bus_factor_headers() -> [&'static str; 3] {
    ["Directory", "Bus factor", "Files"]
}

/// Alignments matching [`bus_factor_headers`].
fn bus_factor_aligns() -> [Align; 3] {
    [Align::Left, Align::Right, Align::Right]
}

const BUS_FACTOR_TOOLTIP: &str = "Number of authors whose departure would orphan more than half the files (by Avelino Degree-of-Authorship); lower is riskier.";
const BUS_FACTOR_FILES_TOOLTIP: &str = "Files in this directory contributing to its bus factor.";

/// Per-header tooltips for the bus-factor table, by column index. The
/// "Files" column means files-*in-this-directory* — distinct from the
/// per-language overview's "source files analysed" — so it carries its own
/// definition rather than inheriting a like-named one (issue #610).
fn bus_factor_tooltips() -> [Option<&'static str>; 3] {
    [
        None,
        Some(BUS_FACTOR_TOOLTIP),
        Some(BUS_FACTOR_FILES_TOOLTIP),
    ]
}

/// The deduplicated `(header, tooltip)` pairs for the change-history table,
/// in column order, skipping identity columns. Both the HTML legend and the
/// Markdown legend draw from this so a column's definition renders
/// identically in the `title=` tooltip and the legend (issue #611).
fn legend_entries(specs: &[&VcsColumn]) -> Vec<(&'static str, &'static str)> {
    // The bus-factor columns render in the same page but live outside
    // `VCS_SPECS`, so chain them onto the spec-driven pairs.
    hotspot::dedup_legend(specs.iter().map(|col| (col.header, col.tooltip)).chain([
        ("Bus factor", Some(BUS_FACTOR_TOOLTIP)),
        ("Files", Some(BUS_FACTOR_FILES_TOOLTIP)),
    ]))
}

/// Tooltips for the ranked-file table, by column index (from the active
/// spec set, so an omitted Hotspot column drops its tooltip too).
fn vcs_tooltips(specs: &[&VcsColumn]) -> Vec<Option<&'static str>> {
    specs.iter().map(|c| c.tooltip).collect()
}

/// The per-directory rows (directory, bus factor, files) for the shared
/// table renderers.
fn bus_factor_rows(bf: &big_code_analysis::vcs::BusFactor) -> Vec<Vec<String>> {
    bf.by_directory
        .iter()
        .map(|dir| {
            vec![
                dir.directory.clone(),
                dir.group.bus_factor.to_string(),
                dir.group.files.to_string(),
            ]
        })
        .collect()
}

/// Plain-English explanation of the bus-factor number, shared by both
/// formats: what it counts and which direction is the risk. Replaces the
/// bare "Avelino Degree-of-Authorship, coverage threshold 0.50." debug
/// stamp that named a method without saying what the number meant (#618).
const BUS_FACTOR_EXPLANATION: &str = "Bus factor: the number of authors whose departure would orphan more than half the files; lower is riskier.";

/// `"file"` or `"files"` for `n`, so the rendered count reads naturally
/// instead of the placeholder-ese `file(s)` (issue #618).
fn files_plural(n: u32) -> &'static str {
    if n == 1 { "file" } else { "files" }
}

/// Append the bus-factor subsection: a plain-English sentence, the repo
/// number, then the per-directory breakdown via the shared Markdown table
/// renderer (which escapes cells). `level` is the heading depth supplied
/// by the caller so the document outline stays gap-free (issue #618).
fn write_markdown_bus_factor(
    out: &mut String,
    bf: &big_code_analysis::vcs::BusFactor,
    level: usize,
) {
    let hashes = "#".repeat(level);
    let _ = writeln!(out, "\n{hashes} Bus factor\n");
    let _ = writeln!(out, "_{BUS_FACTOR_EXPLANATION}_\n");
    let _ = writeln!(
        out,
        "**Repository:** {} (over {} {}, coverage threshold {:.2}).\n",
        bf.repo.bus_factor,
        thousands(bf.repo.files as usize),
        files_plural(bf.repo.files),
        bf.coverage_threshold,
    );
    if !bf.by_directory.is_empty() {
        write_md_table(
            out,
            &bus_factor_headers(),
            &bus_factor_aligns(),
            &bus_factor_rows(bf),
        );
    }
}

// --- HTML -------------------------------------------------------------

/// Render the standalone HTML change-history page: the shared scaffolding
/// (inline CSS + click-to-sort JS, so the page sorts like `report.html`)
/// wrapping the change-history section. The page `<h1>` (from
/// `write_html_head`) is the heading, so the body carries no `<h2>`.
pub(crate) fn render_html(report: &Report) -> String {
    let mut out = String::with_capacity(8 * 1024 + report.files.len() * 128);
    write_html_head(&mut out, HEADING, HEADING);
    // The standalone page has no TOC nav, but heading ids still aid
    // deep-linking; a local `Headings` supplies the slug-dedup state.
    let mut headings = Headings::default();
    let _ = out.write_str("<section>\n");
    // Page title is `<h1>`, so subsections start at `<h2>` (issue #618).
    write_html_body(&mut out, &mut headings, report, STANDALONE_SUBSECTION_LEVEL);
    let _ = out.write_str("</section>\n");
    write_html_tail(&mut out);
    out
}

/// Append the change-history `<section>` under a level-2 heading, for
/// embedding in the aggregated `bca report --vcs` page (before its
/// closing tail). `headings` is the aggregated page's id/TOC collector, so
/// this section's `<h2>` joins the table-of-contents (issue #622).
pub(crate) fn push_html_section(out: &mut String, headings: &mut Headings, report: &Report) {
    let _ = out.write_str("<section>\n");
    // The section `<h2>` carries a slug id and is linked from the page TOC.
    headings.emit_h2(out, HEADING, HEADING);
    let _ = out.write_str("\n");
    // Section header is `<h2>`, so subsections start at `<h3>`.
    write_html_body(out, headings, report, EMBEDDED_SUBSECTION_LEVEL);
    let _ = out.write_str("</section>\n");
}

/// Provenance summary + sortable table (or the empty-set message). No
/// `<section>` wrapper or heading — the caller supplies those.
/// `subsection_level` is the heading depth for the Bus factor subsection.
fn write_html_body(
    out: &mut String,
    headings: &mut Headings,
    report: &Report,
    subsection_level: usize,
) {
    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(out, "<p>{}</p>", provenance(report));
    if report.truncated_shallow_clone {
        let _ = writeln!(out, "<p class=\"note\">{SHALLOW_NOTE}</p>");
    }
    let _ = out.write_str("</div>\n");
    let specs = active_specs(report);
    if report.files.is_empty() {
        let _ = writeln!(out, "<p>{EMPTY_MESSAGE}</p>");
    } else {
        let rows: Vec<Vec<String>> = cell_rows(report, &specs)
            .into_iter()
            .map(|row| row.into_iter().map(cell_text).collect())
            .collect();
        // Tint only the risk cell, by relative rank. `report.files` is
        // risk-ranked, so the row index is the rank; the Markdown path
        // (which never calls this) stays plain text. Omitting the trailing
        // Hotspot column never shifts the Risk column, so the index stays
        // valid against the active spec set.
        let risk_col = risk_column_index();
        let n_rows = rows.len();
        // The table arrives pre-ranked by Risk descending (`report.files`
        // is risk-ranked), so announce that as the initial sort on the Risk
        // column's `<th>` — the existing CSS arrow then shows on first
        // render and screen readers get `aria-sort` semantics (issue #622).
        let ranked = risk_col.map(|index| RankedColumn {
            index,
            dir: "descending",
        });
        write_html_table_classed(
            out,
            &headers(&specs),
            &aligns(&specs),
            &vcs_tooltips(&specs),
            ranked,
            &rows,
            |r, c| {
                if Some(c) == risk_col {
                    risk_heat_class(r, n_rows)
                } else {
                    None
                }
            },
        );
    }
    if let Some(aggregate) = &report.vcs_aggregate {
        write_html_bus_factor(out, headings, &aggregate.bus_factor, subsection_level);
    }
    // A visible legend so the column definitions survive print, mobile, and
    // screen readers (the `title=` tooltips are hover-only) — issue #611;
    // an omitted Hotspot column (issue #615) drops its legend entry too.
    if !report.files.is_empty() {
        write_legend_html(out, &legend_entries(&specs));
    }
}

/// Append the bus-factor subsection (repo sentence + per-directory table)
/// to the HTML body, delegating the table to the shared renderer (which
/// escapes cells).
fn write_html_bus_factor(
    out: &mut String,
    headings: &mut Headings,
    bf: &big_code_analysis::vcs::BusFactor,
    level: usize,
) {
    let _ = out.write_str("<section class=\"bus-factor\">\n");
    headings.emit_heading(out, level, "Bus factor", "Bus factor");
    let _ = writeln!(out, "<p class=\"summary\">{BUS_FACTOR_EXPLANATION}</p>");
    let _ = writeln!(
        out,
        "<p class=\"summary\">Repository: <strong>{}</strong> \
         (over {} {}, coverage threshold {:.2}).</p>",
        bf.repo.bus_factor,
        thousands(bf.repo.files as usize),
        files_plural(bf.repo.files),
        bf.coverage_threshold,
    );
    if !bf.by_directory.is_empty() {
        write_table_with_tooltips(
            out,
            &bus_factor_headers(),
            &bus_factor_aligns(),
            &bus_factor_tooltips(),
            // The bus-factor table is not pre-ranked by a single metric
            // column, so no header announces an initial sort.
            None,
            &bus_factor_rows(bf),
        );
    }
    let _ = out.write_str("</section>\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_code_analysis::wire;

    /// Build a `FileEntry` with the given path/risk and a couple of
    /// signal fields varied so the columns carry distinct, checkable
    /// values across rows.
    fn entry(path: &str, risk: f64, recent: u32, hotspot: Option<f64>) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            vcs: wire::Vcs {
                vcs_schema_version: 1,
                risk_score_version: 1,
                long_window_days: 365,
                recent_window_days: 90,
                commits_long: recent + 10,
                commits_recent: recent,
                churn_long: u64::from(recent) * 100,
                churn_recent: u64::from(recent) * 40,
                authors_long: 3,
                authors_recent: 2,
                ownership_top_share: 0.75,
                burst: 0.5,
                bug_fix_commits: 4,
                security_fix_commits: 1,
                revert_commits: 0,
                age_days: 200,
                last_modified_days: 5,
                change_entropy_long: 2.5,
                change_entropy_recent: 1.5,
                cochange_entropy_long: 1.2,
                cochange_entropy_recent: 0.8,
                risk_score: risk,
                hotspot_score: hotspot,
                author_ids: None,
            },
        }
    }

    /// A small ranked report: descending risk, one row with a hotspot
    /// score and one without (the `bca vcs` vs `metrics --vcs` split).
    fn rich_report() -> Report {
        Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: Some(sample_aggregate()),
            files: vec![
                entry("src/hot.rs", 9.4, 50, Some(123.0)),
                entry("src/warm.rs", 6.1, 20, None),
                entry("docs/with|pipe.md", 2.0, 1, None),
            ],
        }
    }

    /// A small, fixed bus-factor aggregate so the rich-report snapshots
    /// exercise the rendered `## Bus factor` / `<section>` blocks.
    fn sample_aggregate() -> big_code_analysis::vcs::VcsAggregate {
        use big_code_analysis::vcs::{
            BUS_FACTOR_SCHEMA_VERSION, BusFactor, DirectoryBusFactor, GroupBusFactor, VcsAggregate,
        };
        let group = |bus_factor, files, authors| GroupBusFactor {
            bus_factor,
            files,
            authors,
            key_author_ids: None,
        };
        VcsAggregate {
            bus_factor: BusFactor {
                bus_factor_schema_version: BUS_FACTOR_SCHEMA_VERSION,
                coverage_threshold: 0.5,
                doa_threshold: 0.75,
                repo: group(2, 3, 4),
                by_directory: vec![
                    DirectoryBusFactor {
                        directory: "docs".to_owned(),
                        group: group(1, 1, 1),
                    },
                    DirectoryBusFactor {
                        directory: "src".to_owned(),
                        group: group(2, 2, 3),
                    },
                ],
            },
        }
    }

    /// Inner text of every `<td …>…</td>` cell, in document order.
    fn html_cell_texts(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = html;
        while let Some(open) = rest.find("<td") {
            rest = &rest[open..];
            let Some(gt) = rest.find('>') else { break };
            rest = &rest[gt + 1..];
            let Some(close) = rest.find("</td>") else {
                break;
            };
            out.push(rest[..close].to_owned());
            rest = &rest[close + "</td>".len()..];
        }
        out
    }

    /// Unescaped File-column value of every Markdown data row, in order.
    /// Layout is `| Rank | File | … |`, so the File cell is field index 2.
    /// A GFM-escaped `\|` inside a cell is hidden before the naive split
    /// (otherwise it would be mistaken for a column separator) and
    /// restored after, so the returned path is the logical (unescaped)
    /// value.
    fn md_file_column(md: &str) -> Vec<String> {
        const PIPE_SENTINEL: char = '\u{0}';
        // Scope to the ranked-files table, before the bus-factor
        // subsection (whose own pipe table would otherwise be parsed as
        // extra file rows).
        let files_md = md.split("# Bus factor").next().unwrap_or(md);
        files_md
            .lines()
            .filter(|l| l.starts_with('|'))
            .skip(2) // header + separator
            .map(|l| {
                l.replace("\\|", &PIPE_SENTINEL.to_string())
                    .split('|')
                    .nth(2)
                    .unwrap_or("")
                    .trim()
                    .replace(PIPE_SENTINEL, "|")
            })
            .collect()
    }

    #[test]
    fn markdown_report_renders_all_rows() {
        let report = rich_report();
        let md = render_markdown(&report);
        // expected: heading, provenance, and one data row per file in
        // ranked order; the `|`-bearing path is GFM-escaped.
        assert!(md.starts_with("# Change-history risk\n"));
        // Provenance keeps the formula version but drops the wire-only
        // "schema vN" stamp and states the ordinal caveat (issue #618).
        assert!(md.contains("risk formula v1."));
        assert!(!md.contains("schema v"));
        assert!(md.contains("Risk is ordinal"));
        assert_eq!(
            md_file_column(&md),
            ["src/hot.rs", "src/warm.rs", "docs/with|pipe.md"],
        );
        // The `|` in the path must be GFM-escaped so it does not break the
        // table column structure.
        assert!(md.contains("docs/with\\|pipe.md"));
        insta::assert_snapshot!("vcs_report_markdown_rich", md);
    }

    #[test]
    fn html_report_is_self_contained_and_sortable() {
        let report = rich_report();
        let html = render_html(&report);
        // expected: shared scaffolding (doctype + inline sort JS + the
        // sortable hotspot table the AST report uses) so the page sorts
        // on header click exactly like report.html.
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<table class=\"hotspot\">"));
        assert!(html.contains("aria-sort")); // inline sort JS present
        // Standalone page heading comes from the shared scaffolding `<h1>`.
        assert!(html.contains("<h1>Change-history risk</h1>"));
        // The `|` in the path is a literal here (HTML needs no GFM
        // escaping), proving each format escapes for its own target.
        assert!(html.contains("docs/with|pipe.md"));
        insta::assert_snapshot!("vcs_report_html_rich", html);
    }

    /// `risk_column_index()` indexes the FULL `VCS_SPECS`, but the
    /// renderers consume the `active_specs()` view, which may omit the
    /// Hotspot column. That index math is sound only while Risk
    /// precedes Hotspot — the omission then never shifts Risk's
    /// position — so pin the ordering before anyone reorders the specs.
    #[test]
    fn risk_precedes_hotspot_so_omission_never_shifts_risk_index() {
        let risk = VCS_SPECS
            .iter()
            .position(|c| c.header == RISK_COLUMN_HEADER)
            .expect("Risk column in VCS_SPECS");
        let hotspot = VCS_SPECS
            .iter()
            .position(|c| c.header == HOTSPOT_HEADER)
            .expect("Hotspot column in VCS_SPECS");
        assert!(
            risk < hotspot,
            "Risk (idx {risk}) must precede the omittable Hotspot column \
             (idx {hotspot}); risk_column_index() is used against the \
             active_specs() view and relies on it"
        );
    }

    #[test]
    fn markdown_and_html_columns_match() {
        // The cross-format parity guard (mirrors the AST report's
        // `html_and_markdown_report_identical_section_membership`): both
        // renderers consume the one `VCS_SPECS`, so headers and row order
        // must be identical. A renderer that dropped or reordered a
        // column would diverge here.
        let report = rich_report();
        let md = render_markdown(&report);
        let html = render_html(&report);
        let specs = active_specs(&report);

        for header in headers(&specs) {
            assert!(
                md.contains(header),
                "Markdown missing column header {header:?}"
            );
            assert!(
                html.contains(&format!(">{header}</th>")),
                "HTML missing column header {header:?}"
            );
        }

        // File column appears at the same index in every row of both
        // formats, so the ordered File lists must match. Scope to the
        // ranked-files table, before the bus-factor section's own table.
        let files_html = html
            .split("<section class=\"bus-factor\">")
            .next()
            .unwrap_or(&html);
        let html_files: Vec<String> = html_cell_texts(files_html)
            .chunks(specs.len())
            .map(|row| row[1].clone())
            .collect();
        // Both extractors return the logical (unescaped) path, so the
        // parity check is about row order, not per-format escaping.
        let md_files = md_file_column(&md);
        assert_eq!(md_files, html_files);
        assert_eq!(md_files, ["src/hot.rs", "src/warm.rs", "docs/with|pipe.md"]);
    }

    #[test]
    fn empty_report_emits_message_not_table() {
        let report = Report {
            files: Vec::new(),
            // No matched files means no authorship to aggregate, so the
            // bus-factor section is absent too.
            vcs_aggregate: None,
            ..rich_report()
        };
        let md = render_markdown(&report);
        let html = render_html(&report);
        assert!(md.contains(EMPTY_MESSAGE));
        assert!(!md.contains("| Rank |"));
        assert!(html.contains(EMPTY_MESSAGE));
        assert!(!html.contains("<table class=\"hotspot\">"));
    }

    #[test]
    fn hotspot_column_renders_when_any_row_has_a_score() {
        // `rich_report` has one scored row and two `None` rows — the
        // join-present case (e.g. `report --vcs`). The Hotspot column and
        // its legend entry must render, and the scored value must appear.
        let report = rich_report();
        let specs = active_specs(&report);
        assert!(
            specs.iter().any(|c| c.header == HOTSPOT_HEADER),
            "a scored row must keep the Hotspot column"
        );
        let md = render_markdown(&report);
        let html = render_html(&report);
        assert!(md.contains("| Hotspot |"), "Markdown keeps Hotspot header");
        assert!(html.contains(">Hotspot</th>"), "HTML keeps Hotspot header");
        // `Some(123.0)` formats with one decimal place.
        assert!(md.contains("123.0"), "Markdown renders the hotspot value");
        assert!(html.contains("123.0"), "HTML renders the hotspot value");
        // The legend documents the rendered column.
        assert!(
            legend_entries(&specs)
                .iter()
                .any(|(h, _)| *h == HOTSPOT_HEADER),
            "legend keeps the Hotspot entry when the column renders"
        );
    }

    #[test]
    fn hotspot_column_omitted_when_no_row_has_a_score() {
        // Plain `bca vcs` (no AST join): every row's `hotspot_score` is
        // `None`. The all-blank column is dropped from both formats and the
        // legend, instead of a permanently-empty trailing column (#615).
        let report = Report {
            vcs_aggregate: None,
            files: vec![
                entry("src/a.rs", 9.0, 30, None),
                entry("src/b.rs", 4.0, 10, None),
            ],
            ..rich_report()
        };
        let specs = active_specs(&report);
        assert!(
            !specs.iter().any(|c| c.header == HOTSPOT_HEADER),
            "an all-None report must drop the Hotspot column"
        );
        let md = render_markdown(&report);
        let html = render_html(&report);
        assert!(
            !md.contains("Hotspot"),
            "Markdown must omit the Hotspot column entirely"
        );
        assert!(
            !html.contains("Hotspot"),
            "HTML must omit the Hotspot column entirely (header + legend)"
        );
        // A still-rendered column proves the table itself is intact.
        assert!(md.contains("| Rank |"));
        assert!(html.contains(">Risk</th>"));
    }

    #[test]
    fn every_vcs_header_carries_a_tooltip() {
        // Drive from the shared specs (mirrors the AST report's
        // `metric_headers_carry_tooltips`): every change-history column
        // tooltip — including the bus-factor table's own columns — must
        // render as a `title="…"` attribute, so a new column is required
        // to document itself without anyone remembering to update this
        // test. `legend_entries` is the single source the HTML tooltips
        // and both legends draw from.
        use crate::html_report::escape_html;
        let report = rich_report();
        let html = render_html(&report);
        for (header, tip) in legend_entries(&active_specs(&report)) {
            let needle = format!(
                " title=\"{}\">{}</th>",
                escape_html(tip),
                escape_html(header)
            );
            assert!(
                html.contains(&needle),
                "VCS header {header:?} should render with its tooltip; expected {needle:?}"
            );
        }
    }

    #[test]
    fn legend_renders_in_both_formats() {
        // Issue #611: the column definitions must reach a Markdown reader
        // (PR comment, pasted issue) and survive HTML print/mobile/screen
        // readers, drawn from the same `legend_entries` source so the two
        // formats cannot drift.
        let report = rich_report();
        let md = render_markdown(&report);
        let html = render_html(&report);
        // Standalone page title is `#`/h1, so the legend lands at `##`/h2
        // (issue #618 keeps the outline gap-free).
        assert!(md.contains("## Legend"), "Markdown legend heading missing");
        assert!(
            html.contains("<summary>Legend</summary>"),
            "HTML legend missing"
        );
        for (header, tip) in legend_entries(&active_specs(&report)) {
            assert!(
                md.contains(&format!("**{header}**")),
                "Markdown legend missing header {header:?}"
            );
            // The definition text (minus any escaping) reaches both.
            let snippet: String = tip.chars().take(20).collect();
            assert!(
                md.contains(&snippet),
                "Markdown legend missing definition for {header:?}"
            );
            assert!(
                html.contains(&snippet),
                "HTML legend missing definition for {header:?}"
            );
        }
    }

    #[test]
    fn bus_factor_files_tooltip_distinct_from_overview() {
        // Issue #610 deferred item: the bus-factor "Files" column means
        // files-per-directory, not "source files analysed". With tooltips
        // sourced positionally from the spec, the bus-factor table now
        // carries its own definition rather than inheriting the unrelated
        // overview one.
        let html = render_html(&rich_report());
        assert!(
            html.contains(&format!(" title=\"{BUS_FACTOR_FILES_TOOLTIP}\">Files</th>")),
            "bus-factor Files column should carry its own tooltip"
        );
        assert!(
            !html.contains("Number of source files analysed."),
            "the overview Files tooltip must not leak onto the VCS page"
        );
    }

    #[test]
    fn shallow_clone_note_appears_in_both_formats() {
        let report = Report {
            truncated_shallow_clone: true,
            ..rich_report()
        };
        assert!(render_markdown(&report).contains(SHALLOW_NOTE));
        assert!(render_html(&report).contains(SHALLOW_NOTE));
    }

    #[test]
    fn provenance_drops_schema_and_states_ordinal_caveat() {
        // Issue #618 (d): the wire-only `schema vN` stamp must not reach
        // either rendered format, and both must carry the ordinal caveat
        // so a reader does not treat the Risk magnitude as a scale. The
        // structured output keeps `vcs_schema_version` (tested elsewhere).
        let report = rich_report();
        let md = render_markdown(&report);
        let html = render_html(&report);
        for rendered in [&md, &html] {
            assert!(
                !rendered.contains("schema v"),
                "rendered formats must not leak the wire schema stamp"
            );
            assert!(rendered.contains("risk formula v1."));
            assert!(
                rendered.contains("Risk is ordinal"),
                "provenance must state the ordinal-only caveat"
            );
        }
        // The Risk tooltip / legend also carries the caveat.
        assert!(RISK_TOOLTIP.contains("Ordinal"));
        assert!(html.contains("Ordinal: only relative ranks"));
    }

    #[test]
    fn integer_cells_use_thousands_separators() {
        // Issue #618 (b): churn/commit cells render with comma separators
        // like the AST tables, not the bare `15973`. A row with churn in
        // the thousands proves the separator is applied; the HTML sort JS
        // strips commas before comparing, so this is display-only.
        let report = Report {
            vcs_aggregate: None,
            // churn_recent = 250 * 40 = 10,000; commits_long = 260.
            files: vec![entry("src/big.rs", 9.0, 250, None)],
            ..rich_report()
        };
        let md = render_markdown(&report);
        let html = render_html(&report);
        for rendered in [&md, &html] {
            assert!(
                rendered.contains("10,000"),
                "churn cell must render with a thousands separator"
            );
            assert!(
                !rendered.contains("10000"),
                "the unseparated form must not appear"
            );
        }
    }

    #[test]
    fn standalone_subsections_keep_heading_outline_gap_free() {
        // Issue #618 (c): the standalone page title is `#`/<h1>, so its
        // Bus factor and Legend subsections must be `##`/<h2> — a jump
        // straight to `###`/<h3> skips h2 and trips MD001 (the lint this
        // tool runs over its own Markdown) and breaks screen-reader
        // outlines.
        let report = rich_report();
        let md = render_markdown(&report);
        assert!(
            md.contains("\n## Bus factor\n"),
            "standalone bus factor at h2"
        );
        assert!(md.contains("\n## Legend\n"), "standalone legend at h2");
        assert!(
            !md.contains("### Bus factor"),
            "no h3 jump on standalone page"
        );

        let html = render_html(&report);
        // Headings carry a slug `id=` now (issue #622), so match text+close.
        assert!(
            html.contains(">Bus factor</h2>"),
            "standalone HTML bus factor at h2"
        );
        assert!(
            !html.contains(">Bus factor</h3>"),
            "no h3 jump in standalone HTML"
        );

        // Embedded under a `##`/<h2> section header, subsections deepen to
        // `###`/<h3> so the outline stays gap-free there too.
        let mut embedded = String::from("# Report\n");
        push_markdown_section(&mut embedded, &report);
        assert!(
            embedded.contains("\n### Bus factor\n"),
            "embedded bus factor at h3"
        );
        assert!(embedded.contains("\n### Legend\n"), "embedded legend at h3");
    }

    #[test]
    fn bus_factor_reads_as_plain_english_with_pluralization() {
        // Issue #618 (a): the bus-factor block must explain what the number
        // means and which direction is risky, and pluralize "file(s)".
        let report = rich_report();
        let md = render_markdown(&report);
        let html = render_html(&report);
        for rendered in [&md, &html] {
            assert!(
                rendered.contains("authors whose departure would orphan more than half"),
                "bus factor must carry the plain-English explanation"
            );
            assert!(rendered.contains("lower is riskier"));
            // `sample_aggregate` has repo.files = 3 -> plural "files", and
            // no literal placeholder-ese "file(s)".
            assert!(rendered.contains("(over 3 files,"), "plural files");
            assert!(
                !rendered.contains("file(s)"),
                "no placeholder pluralization"
            );
        }
        // Singular path: a one-file repo reads "1 file".
        assert_eq!(files_plural(1), "file");
        assert_eq!(files_plural(0), "files");
        assert_eq!(files_plural(2), "files");
    }

    /// The `risk-heat-N` class on each ranked row's Risk `<td>`, in
    /// document order, read straight off the rendered tag so the test is
    /// tied to the actual attribute. Scoped to the `<tbody>` of the
    /// ranked-files table (before the bus-factor section, which has no
    /// risk column).
    fn risk_heat_classes(html: &str) -> Vec<String> {
        let risk_col = risk_column_index().expect("VCS_SPECS has a Risk column");
        let files_html = html
            .split("<section class=\"bus-factor\">")
            .next()
            .unwrap_or(html);
        let mut rest = files_html.split_once("<tbody>").map_or("", |(_, b)| b);
        let mut classes = Vec::new();
        while let Some(open) = rest.find("<tr>") {
            let row = &rest[open + "<tr>".len()..];
            let Some(close) = row.find("</tr>") else {
                break;
            };
            // The risk cell looks like ` class="numeric risk-heat-0">9.4`.
            // Fail loudly on a structural miss (column reorder / markup
            // drift) rather than silently pushing an empty class.
            let cell = row[..close]
                .split("<td")
                .nth(risk_col + 1)
                .expect("rendered row has a Risk cell");
            let heat = cell
                .split_once("risk-heat-")
                .and_then(|(_, c)| c.split(['"', ' ', '>']).next())
                .map(|n| format!("risk-heat-{n}"))
                .unwrap_or_default();
            classes.push(heat);
            rest = &row[close + "</tr>".len()..];
        }
        classes
    }

    #[test]
    fn risk_cell_is_heated_by_rank_not_value() {
        // Raw scores are tightly clustered and all high (98..94) — an
        // *absolute*-threshold scheme would paint every row the same
        // severe band. Relativity demands the top row get the most-severe
        // class and the bottom the least, purely by rank. Five rows over
        // five bands map rank r -> band r.
        let report = Report {
            vcs_aggregate: None,
            files: (0..5)
                .map(|i| entry(&format!("f{i}.rs"), 98.0 - f64::from(i), 5 - i, None))
                .collect(),
            ..rich_report()
        };
        let html = render_html(&report);
        assert_eq!(
            risk_heat_classes(&html),
            (0..5).map(|i| format!("risk-heat-{i}")).collect::<Vec<_>>(),
        );
        // Exactly one heated cell per row — the band lands on a single
        // column. Scan the body (the `<style>` block also names classes).
        let body = html.rsplit_once("</style>").map_or("", |(_, b)| b);
        assert_eq!(body.matches("risk-heat-").count(), report.files.len());
        // The Markdown path carries no heat classes whatsoever.
        assert!(!render_markdown(&report).contains("risk-heat-"));

        // Invert value vs. position to assert (not merely construct) that
        // the band keys off rank alone: hand the rows in *ascending* raw
        // score (lowest first). If heat read the absolute value, the first
        // row's low score would land in a less-severe band; because it is
        // positional, the first row must still get the most-severe class.
        let ascending = Report {
            vcs_aggregate: None,
            files: (0..5)
                .map(|i| entry(&format!("f{i}.rs"), 10.0 + f64::from(i), i + 1, None))
                .collect(),
            ..rich_report()
        };
        assert_eq!(
            risk_heat_classes(&render_html(&ascending)),
            (0..5).map(|i| format!("risk-heat-{i}")).collect::<Vec<_>>(),
            "band must follow row position, not raw risk_score magnitude",
        );
    }

    #[test]
    fn risk_heat_band_edges_are_relative() {
        // Unit check of the rank->band mapping and documented edge cases.
        assert_eq!(risk_heat_class(0, 1), Some("risk-heat-4")); // lone row: least-severe
        assert_eq!(risk_heat_class(0, 5), Some("risk-heat-0")); // top: most-severe
        assert_eq!(risk_heat_class(4, 5), Some("risk-heat-4")); // bottom: least-severe
        // Remainder rows: 3 over 5 bands -> 0, 1, 3 (no div-by-zero, in range).
        assert_eq!(risk_heat_class(1, 3), Some("risk-heat-1"));
        assert_eq!(risk_heat_class(2, 3), Some("risk-heat-3"));
        // The rich report's three rows render bands 0, 1, 3 in order.
        assert_eq!(
            risk_heat_classes(&render_html(&rich_report())),
            ["risk-heat-0", "risk-heat-1", "risk-heat-3"],
        );
    }

    #[test]
    fn heat_classes_have_light_and_dark_css() {
        // Every `risk-heat-N` band needs a light-mode rule and a dark-mode
        // override so the value text stays WCAG-AA legible in both schemes.
        // Assert against the rendered `<style>`; the heat dark-mode rules
        // are the *last* `@media` adapter (the palette emits an earlier one).
        let html = render_html(&rich_report());
        let dark_block = html
            .rsplit_once("@media (prefers-color-scheme:dark){")
            .expect("dark-mode adapter present")
            .1;
        for class in HEAT_CLASSES {
            let light = format!("td.{class}{{background:");
            assert!(
                html.contains(&light),
                "missing light-mode CSS for {class:?}: expected substring {light:?}"
            );
            assert!(
                dark_block.contains(&light),
                "missing dark-mode override for {class:?}: expected {light:?} in @media block"
            );
        }
    }
}
