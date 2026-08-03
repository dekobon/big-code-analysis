//! Per-section writers for [`super::write_language_section`].
//!
//! Each writer appends one Markdown section (heading + optional table)
//! to `out`. Empty inputs produce no output so the orchestrator can
//! call each writer unconditionally without per-section emptiness
//! checks. Filtering, sorting, and `top_n` truncation are localized
//! to each writer so the shared "filter → sort → take → emit table"
//! shape stays close to the per-section column layout it drives.

use std::fmt::Write as _;

use big_code_analysis::SuppressionPolicy;

use super::advisory::AdvisoryThresholds;
use super::hotspot::{Cell, CyclomaticStats, HotspotSpec};
use super::{Align, FunctionSummary, escape_cell, escape_name, mi_rating, thousands, write_table};

pub(super) fn write_summary(out: &mut String, units: &[&FunctionSummary]) {
    let (files, sloc, ploc, cloc, mi_numerator) = units.iter().fold(
        (0usize, 0usize, 0usize, 0usize, 0.0f64),
        |(f, sl, pl, cl, mi), s| {
            (
                f + 1,
                sl + s.sloc,
                pl + s.ploc,
                cl + s.cloc,
                mi + super::mi_weight_numerator(s),
            )
        },
    );
    let cr = if sloc > 0 {
        (cloc as f64 / sloc as f64) * 100.0
    } else {
        0.0
    };
    let avg_mi = super::sloc_weighted_avg_mi(mi_numerator, sloc);
    let rating = mi_rating(avg_mi);

    // A two-column (metric | value) table renders identically in every GFM
    // dialect; the earlier `|`-joined single-newline lines collapsed to a
    // run-on paragraph in spec-conformant GFM (issue #671).
    let rows: Vec<Vec<String>> = vec![
        vec!["Files".to_string(), thousands(files)],
        vec!["SLOC".to_string(), thousands(sloc)],
        vec!["PLOC".to_string(), thousands(ploc)],
        vec!["Comment ratio".to_string(), format!("{cr:.1}%")],
        vec![
            super::AVG_MI_LABEL.to_string(),
            format!("{avg_mi:.1} ({rating})"),
        ],
    ];

    let _ = writeln!(out, "### Summary\n");
    write_table(
        out,
        &["Metric", "Value"],
        &[Align::Left, Align::Right],
        &rows,
    );
}

/// Render one hotspot section: a `### {title}` heading + GFM table from a
/// shared [`HotspotSpec`] and its already-selected `rows`. The title is
/// written raw: it is a `### ` heading, not a table cell, so GFM-special
/// characters need no cell-escaping. No current concept carries a `>`
/// anyway (see [`super::hotspot::HotspotTitle::render`]), unlike HTML,
/// which escapes it defensively.
pub(super) fn emit_section_md(
    out: &mut String,
    spec: &HotspotSpec,
    top_n: usize,
    rows: &[&FunctionSummary],
) {
    let _ = writeln!(out, "\n### {}\n", spec.title.render(top_n));
    let headers: Vec<&str> = spec.columns.iter().map(|c| c.header).collect();
    let aligns: Vec<Align> = spec.columns.iter().map(|c| c.align).collect();
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|s| {
            spec.columns
                .iter()
                .map(|c| render_cell_md((c.cell)(s)))
                .collect()
        })
        .collect();
    write_table(out, &headers, &aligns, &table_rows);
}

/// Escape a [`Cell`] for Markdown: identifiers get backtick-wrapped
/// (`escape_name`), paths get GFM-escaped (`escape_cell`), pre-formatted
/// numerics pass through raw — reproducing the per-cell escaping the
/// hand-written section writers used.
pub(crate) fn render_cell_md(cell: Cell) -> String {
    match cell {
        Cell::Name(s) => escape_name(&s),
        Cell::Path(s) => escape_cell(&s),
        Cell::Num(s) => s,
    }
}

/// The cyclomatic summary note under the CC hotspot table: a blank line then
/// the caption over the same suppression-filtered set the table shows (see
/// [`super::hotspot::select_cc`]). When `policy` honors suppression the line
/// is captioned `(excluding suppressed functions)` so a reader can tell it
/// apart from the raw, suppression-independent CC count in the Actionable
/// Summary (issue #616).
pub(super) fn emit_cc_note_md(
    out: &mut String,
    stats: &CyclomaticStats,
    policy: SuppressionPolicy,
) {
    let _ = writeln!(out);
    let caption = super::hotspot::cc_note_caption(policy)
        .map(|c| format!(" ({c})"))
        .unwrap_or_default();
    // The bands use the resolved advisory CC cutoff and its severe multiple
    // (issue #630) — `> 10` / `> 20` by default, shifted when a manifest sets
    // `cyclomatic = N`.
    let _ = writeln!(
        out,
        "Average CC: {:.1} | Max: {:.0} | CC > {:.0}: {} functions | CC > {:.0}: {} functions{caption}",
        stats.avg(),
        stats.max,
        stats.primary_cutoff,
        stats.over_primary,
        stats.severe_cutoff,
        stats.over_severe,
    );
}

/// The MI note under the MI hotspot table: a blank line then the shared
/// [`super::hotspot::MI_NOTE`] text in italics, explaining the rendered
/// variant, rating bands, and 0-100 clamping caveat so a table of all-0.0
/// clamped values is not misread as catastrophic (issue #627).
pub(super) fn emit_mi_note_md(out: &mut String) {
    let _ = writeln!(out, "\n_{}_", super::hotspot::MI_NOTE);
}

/// Emits the advisory "functions over threshold" roll-up.
///
/// Unlike the per-metric hotspot tables, this summary intentionally
/// counts raw measurements regardless of suppression policy: it is a
/// whole-codebase health indicator, not a gate, so a `bca: suppress`
/// marker that silences a function in one metric's hotspot table does
/// not erase it from the aggregate concern count (#501). The bullets are
/// therefore policy-independent; `policy` only selects the suppressed-function
/// figure named in the caption (issue #616), never which functions are
/// counted in the bullets.
pub(super) fn write_actionable_summary(
    out: &mut String,
    funcs: &[&FunctionSummary],
    policy: SuppressionPolicy,
    advisory: AdvisoryThresholds,
) {
    // Cutoffs come from the resolved advisory thresholds (issue #630): the
    // built-in defaults, or the manifest `[thresholds]` values when present, so
    // a project gating at `cyclomatic = 15` is not scolded at `CC > 10`. The
    // counting is single-sourced with the HTML report via `count_over`.
    let counts = advisory.count_over(funcs);

    let _ = writeln!(out, "\n### Actionable Summary\n");
    // Provenance so the cutoffs are always attributable (issue #630).
    let _ = writeln!(out, "_{}_\n", advisory.provenance_line());
    let breakdown = super::hotspot::suppressed_metric_breakdown(funcs, policy, advisory);
    let _ = writeln!(
        out,
        "{}\n",
        super::hotspot::actionable_summary_caption(&breakdown)
    );
    if counts.all_clear() {
        let _ = writeln!(out, "No major quality concerns detected.");
        return;
    }
    if counts.cc > 0 {
        let _ = writeln!(
            out,
            "- **{}** functions with CC > {:.0}",
            counts.cc, advisory.cc
        );
    }
    if counts.cognitive > 0 {
        let _ = writeln!(
            out,
            "- **{}** functions with cognitive complexity > {:.0}",
            counts.cognitive, advisory.cognitive
        );
    }
    if counts.sloc > 0 {
        let _ = writeln!(
            out,
            "- **{}** functions with SLOC > {}",
            counts.sloc, advisory.sloc
        );
    }
    if counts.nargs > 0 {
        let _ = writeln!(
            out,
            "- **{}** functions with more than {} parameters",
            counts.nargs, advisory.nargs
        );
    }
    if counts.bugs > 0 {
        let _ = writeln!(
            out,
            "- **{}** functions with estimated Halstead bugs > {:.1}",
            counts.bugs, advisory.bugs
        );
    }
}

/// Emit the section heading followed by the "table omitted: all N matching
/// functions suppressed" caption, in place of a hotspot table that was
/// rendered empty *solely because* suppression hid every matching row
/// (`count > 0`). The heading is emitted exactly as the non-suppressed path
/// does so anchors and heading structure stay stable across suppression
/// states (issue #681). Keeps an Actionable-Summary bullet from pointing at a
/// table absent from the document (issue #616). A no-op when `count == 0`
/// (the metric was genuinely absent, so silence is correct).
pub(super) fn emit_fully_suppressed_note_md(out: &mut String, title: &str, count: usize) {
    if count == 0 {
        return;
    }
    // Emit the section heading exactly as `emit_section_md` does, so a
    // fully-suppressed section keeps its place in the heading structure and
    // its anchor stays stable across suppression states (issue #681). The
    // omission note is the section body.
    let _ = writeln!(out, "\n### {title}\n");
    let _ = writeln!(
        out,
        "_{}_",
        super::hotspot::fully_suppressed_caption(title, count)
    );
}
