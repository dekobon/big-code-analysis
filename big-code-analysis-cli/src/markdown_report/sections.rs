// bca: suppress-file(halstead, nargs, nexits, nom)
// Markdown report section builders; the offenders are string-formatting-volume
// and many-fn aggregation artifacts, not per-function logic complexity.

//! Per-section writers for [`super::write_language_section`].
//!
//! Each writer appends one Markdown section (heading + optional table)
//! to `out`. Empty inputs produce no output so the orchestrator can
//! call each writer unconditionally without per-section emptiness
//! checks. Filtering, sorting, and `top_n` truncation are localized
//! to each writer so the shared "filter → sort → take → emit table"
//! shape stays close to the per-section column layout it drives.

use std::fmt::Write as _;

use big_code_analysis::{SpaceKind, SuppressionPolicy};

use super::hotspot::{Cell, CyclomaticStats, HotspotSpec};
use super::{Align, FunctionSummary, escape_cell, escape_name, mi_rating, thousands, write_table};

pub(super) fn write_summary(out: &mut String, units: &[&FunctionSummary]) {
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

    let _ = writeln!(out, "### Summary\n");
    let _ = writeln!(
        out,
        "Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%",
        thousands(files),
        thousands(sloc),
        thousands(ploc),
    );
    let _ = writeln!(out, "Average MI: {avg_mi:.1} ({rating})");
}

/// Render one hotspot section: a `### {title}` heading + GFM table from a
/// shared [`HotspotSpec`] and its already-selected `rows`. The title is
/// written raw (the `>` in the many-parameters heading stays literal in
/// Markdown, unlike HTML which escapes it).
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
    let _ = writeln!(
        out,
        "Average CC: {:.1} | Max: {:.0} | CC > 10: {} functions | CC > 20: {} functions{caption}",
        stats.avg(),
        stats.max,
        stats.gt10,
        stats.gt20,
    );
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
) {
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

    let _ = writeln!(out, "\n### Actionable Summary\n");
    let suppressed = super::hotspot::suppressed_func_count(funcs, policy);
    let _ = writeln!(
        out,
        "{}\n",
        super::hotspot::actionable_summary_caption(suppressed)
    );
    if cc_gt10 == 0 && cog_gt15 == 0 && sloc_gt100 == 0 && nargs_gt3 == 0 && bugs_gt1 == 0 {
        let _ = writeln!(out, "No major quality concerns detected.");
        return;
    }
    if cc_gt10 > 0 {
        let _ = writeln!(out, "- **{cc_gt10}** functions with CC > 10");
    }
    if cog_gt15 > 0 {
        let _ = writeln!(
            out,
            "- **{cog_gt15}** functions with cognitive complexity > 15"
        );
    }
    if sloc_gt100 > 0 {
        let _ = writeln!(out, "- **{sloc_gt100}** functions with SLOC > 100");
    }
    if nargs_gt3 > 0 {
        let _ = writeln!(
            out,
            "- **{nargs_gt3}** functions with more than 3 parameters"
        );
    }
    if bugs_gt1 > 0 {
        let _ = writeln!(
            out,
            "- **{bugs_gt1}** functions with estimated Halstead bugs > 1.0"
        );
    }
}

/// Emit the "table omitted: all N matching functions suppressed" caption in
/// place of a hotspot table that was rendered empty *solely because*
/// suppression hid every matching row (`count > 0`). Keeps an
/// Actionable-Summary bullet from pointing at a table absent from the
/// document (issue #616). A no-op when `count == 0` (the metric was genuinely
/// absent, so silence is correct).
pub(super) fn emit_fully_suppressed_note_md(out: &mut String, title: &str, count: usize) {
    if count == 0 {
        return;
    }
    let _ = writeln!(
        out,
        "\n_{}_",
        super::hotspot::fully_suppressed_caption(title, count)
    );
}

/// Partition `entries` by `SpaceKind` into (units, functions). The
/// `units` slice drives the file-level summary and MI section; the
/// `funcs` slice drives all per-function hotspot tables.
pub(super) fn split_units_and_functions<'a>(
    entries: &[&'a FunctionSummary],
) -> (Vec<&'a FunctionSummary>, Vec<&'a FunctionSummary>) {
    let units: Vec<&FunctionSummary> = entries
        .iter()
        .filter(|s| s.kind == SpaceKind::Unit)
        .copied()
        .collect();
    let funcs: Vec<&FunctionSummary> = entries
        .iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .copied()
        .collect();
    (units, funcs)
}
