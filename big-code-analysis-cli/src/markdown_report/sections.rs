//! Per-section writers for [`super::write_language_section`].
//!
//! Each writer appends one Markdown section (heading + optional table)
//! to `out`. Empty inputs produce no output so the orchestrator can
//! call each writer unconditionally without per-section emptiness
//! checks. Filtering, sorting, and `top_n` truncation are localized
//! to each writer, matching the structure that lived inline in
//! `write_language_section` before the #357 P2 split.

use std::fmt::Write as _;

use big_code_analysis::SpaceKind;

use super::{
    Align, FunctionSummary, escape_cell, escape_name, is_class_like, mi_rating,
    sort_by_metric_asc, sort_by_metric_desc, thousands, write_table,
};
use crate::format_util::MetricScalar;

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

pub(super) fn write_mi_lowest(out: &mut String, units: &[&FunctionSummary], top_n: usize) {
    let mut mi_entries: Vec<&FunctionSummary> = units
        .iter()
        .filter(|s| s.mi_visual_studio > 0.0)
        .copied()
        .collect();
    if mi_entries.is_empty() {
        return;
    }
    sort_by_metric_asc(&mut mi_entries, |s| s.mi_visual_studio);
    let count = mi_entries.len().min(top_n);

    let _ = writeln!(
        out,
        "\n### Maintainability Index (lowest files, top-{top_n})\n"
    );
    let rows: Vec<Vec<String>> = mi_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_cell(&s.file),
                format!("{:.1}", s.mi_visual_studio),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
        &["File", "MI", "SLOC", "Tokens"],
        &[Align::Left, Align::Right, Align::Right, Align::Right],
        &rows,
    );
}

pub(super) fn write_cyclomatic_hotspots(
    out: &mut String,
    funcs: &[&FunctionSummary],
    top_n: usize,
) {
    let mut cc_entries: Vec<&FunctionSummary> = funcs
        .iter()
        .filter(|s| s.cyclomatic > 0.0)
        .copied()
        .collect();
    if cc_entries.is_empty() {
        return;
    }
    let stats = CyclomaticStats::from(cc_entries.as_slice());
    let avg_cc = stats.average();

    sort_by_metric_desc(&mut cc_entries, |s| s.cyclomatic);
    let count = cc_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Cyclomatic Complexity Hotspots\n");
    let rows: Vec<Vec<String>> = cc_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                MetricScalar(s.cyclomatic).to_string(),
                MetricScalar(s.cognitive).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Average CC: {avg_cc:.1} | Max: {max_cc:.0} | CC > 10: {count_gt10} functions | CC > 20: {count_gt20} functions",
        max_cc = stats.max,
        count_gt10 = stats.count_gt10,
        count_gt20 = stats.count_gt20,
    );
}

/// Aggregate counters for the cyclomatic hotspots section: sum, count,
/// max, and per-threshold tallies, all in one pass over the entries.
/// Lives next to `write_cyclomatic_hotspots` because no other section
/// needs this exact shape.
struct CyclomaticStats {
    sum: f64,
    count: usize,
    max: f64,
    count_gt10: usize,
    count_gt20: usize,
}

impl CyclomaticStats {
    fn from(entries: &[&FunctionSummary]) -> Self {
        let mut stats = Self {
            sum: 0.0,
            count: 0,
            max: f64::NAN,
            count_gt10: 0,
            count_gt20: 0,
        };
        for s in entries {
            let c = s.cyclomatic;
            stats.sum += c;
            stats.count += 1;
            stats.max = f64::max(stats.max, c);
            stats.count_gt10 += usize::from(c > 10.0);
            stats.count_gt20 += usize::from(c > 20.0);
        }
        stats
    }

    fn average(&self) -> f64 {
        if self.count > 0 {
            self.sum / self.count as f64
        } else {
            0.0
        }
    }
}

pub(super) fn write_cognitive_hotspots(
    out: &mut String,
    funcs: &[&FunctionSummary],
    top_n: usize,
) {
    let mut cog_entries: Vec<&FunctionSummary> = funcs
        .iter()
        .filter(|s| s.cognitive > 0.0)
        .copied()
        .collect();
    if cog_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut cog_entries, |s| s.cognitive);
    let count = cog_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Cognitive Complexity Hotspots\n");
    let rows: Vec<Vec<String>> = cog_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                MetricScalar(s.cognitive).to_string(),
                MetricScalar(s.cyclomatic).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
}

pub(super) fn write_halstead_hotspots(
    out: &mut String,
    funcs: &[&FunctionSummary],
    top_n: usize,
) {
    let mut hal_entries: Vec<&FunctionSummary> = funcs
        .iter()
        .filter(|s| s.halstead_effort > 0.0)
        .copied()
        .collect();
    if hal_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut hal_entries, |s| s.halstead_effort);
    let count = hal_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Halstead Effort Hotspots\n");
    let rows: Vec<Vec<String>> = hal_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                MetricScalar(s.halstead_effort).to_string(),
                MetricScalar(s.halstead_volume).to_string(),
                format!("{:.2}", s.halstead_bugs),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
}

pub(super) fn write_largest_by_sloc(
    out: &mut String,
    funcs: &[&FunctionSummary],
    top_n: usize,
) {
    let mut sloc_entries: Vec<&FunctionSummary> =
        funcs.iter().filter(|s| s.sloc > 0).copied().collect();
    if sloc_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut sloc_entries, |s| s.sloc as f64);
    let count = sloc_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Largest Functions by SLOC\n");
    let rows: Vec<Vec<String>> = sloc_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
                MetricScalar(s.cyclomatic).to_string(),
                MetricScalar(s.cognitive).to_string(),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
}

pub(super) fn write_many_params(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let mut nargs_entries: Vec<&FunctionSummary> =
        funcs.iter().filter(|s| s.nargs > 3).copied().collect();
    if nargs_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut nargs_entries, |s| s.nargs as f64);
    let count = nargs_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Functions With Many Parameters (>3)\n");
    let rows: Vec<Vec<String>> = nargs_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.nargs.to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
        &["Function", "File", "Args", "SLOC", "Tokens"],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        &rows,
    );
}

pub(super) fn write_actionable_summary(out: &mut String, funcs: &[&FunctionSummary]) {
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

pub(super) fn write_wmc_hotspots(out: &mut String, entries: &[&FunctionSummary], top_n: usize) {
    let mut class_entries: Vec<&FunctionSummary> = entries
        .iter()
        .filter(|s| is_class_like(s.kind) && s.wmc > 0.0)
        .copied()
        .collect();
    if class_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut class_entries, |s| s.wmc);
    let count = class_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Class/Trait/Impl Hotspots (WMC)\n");
    let rows: Vec<Vec<String>> = class_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                MetricScalar(s.wmc).to_string(),
                s.nom.to_string(),
                MetricScalar(s.npa).to_string(),
                MetricScalar(s.npm).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
}

pub(super) fn write_nexits_hotspots(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let mut nexits_entries: Vec<&FunctionSummary> =
        funcs.iter().filter(|s| s.nexits > 0).copied().collect();
    if nexits_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut nexits_entries, |s| s.nexits as f64);
    let count = nexits_entries.len().min(top_n);

    let _ = writeln!(out, "\n### Functions with the most exit points (NEXITS)\n");
    let rows: Vec<Vec<String>> = nexits_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                s.nexits.to_string(),
                MetricScalar(s.cyclomatic).to_string(),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
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
        &rows,
    );
}

pub(super) fn write_abc_hotspots(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let mut abc_entries: Vec<&FunctionSummary> =
        funcs.iter().filter(|s| s.abc > 0.0).copied().collect();
    if abc_entries.is_empty() {
        return;
    }
    sort_by_metric_desc(&mut abc_entries, |s| s.abc);
    let count = abc_entries.len().min(top_n);

    let _ = writeln!(out, "\n### ABC Magnitude Hotspots\n");
    let rows: Vec<Vec<String>> = abc_entries[..count]
        .iter()
        .map(|s| {
            vec![
                escape_name(&s.name),
                escape_cell(&s.file),
                s.start_line.to_string(),
                format!("{:.1}", s.abc),
                thousands(s.sloc),
                thousands(s.tokens),
            ]
        })
        .collect();
    write_table(
        out,
        &["Function", "File", "Line", "ABC", "SLOC", "Tokens"],
        &[
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        &rows,
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
