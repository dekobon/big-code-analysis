//! Per-section writers for [`super::write_language_section`].
//!
//! Each writer appends one Markdown section (heading + optional table)
//! to `out`. Empty inputs produce no output so the orchestrator can
//! call each writer unconditionally without per-section emptiness
//! checks. Filtering, sorting, and `top_n` truncation are localized
//! to each writer so the shared "filter → sort → take → emit table"
//! shape stays close to the per-section column layout it drives.

use std::fmt::Write as _;

use big_code_analysis::SpaceKind;

use super::{
    Align, FunctionSummary, escape_cell, escape_name, is_class_like, mi_rating, sort_by_metric_asc,
    sort_by_metric_desc, thousands, write_table,
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
    let stats = CyclomaticStats::from_entries(cc_entries.as_slice());
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
    fn from_entries(entries: &[&FunctionSummary]) -> Self {
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

/// Filter `entries`, keep the `top_n` highest-by-`metric` survivors,
/// and return them sorted descending. Returns `None` when no entry
/// passes the filter so callers can early-out before emitting an
/// empty heading.
///
/// Two section writers bypass this helper:
/// - `write_cyclomatic_hotspots` needs summary stats over the **full**
///   filtered set before truncation, so it can't use a top-N adapter.
/// - `write_mi_lowest` sorts ascending (lowest MI first), not descending.
///
/// Implementation: uses `select_nth_unstable_by` to partition the head
/// in `O(N)` average, then stable-sorts only the head — overall
/// `O(N + k log k)` instead of `O(N log N)` for a full sort that
/// throws away `N − k` entries (#358). The comparator matches
/// `sort_by_metric_desc` so the resulting top-N set is identical to
/// the full-sort approach for any total-order input; per-writer
/// filters narrow `kind`, so `(file, start_line, name)` tie-breaking
/// uniquely orders the survivors in practice.
fn top_n_desc<'a, F, M>(
    entries: &[&'a FunctionSummary],
    top_n: usize,
    filter: F,
    metric: M,
) -> Option<Vec<&'a FunctionSummary>>
where
    F: Fn(&FunctionSummary) -> bool,
    M: Fn(&FunctionSummary) -> f64,
{
    let mut filtered: Vec<&FunctionSummary> =
        entries.iter().filter(|s| filter(s)).copied().collect();
    if filtered.is_empty() {
        return None;
    }
    let n = filtered.len().min(top_n);
    if n == 0 {
        return Some(Vec::new());
    }
    if n < filtered.len() {
        filtered.select_nth_unstable_by(n - 1, |a, b| {
            metric(b)
                .total_cmp(&metric(a))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.start_line.cmp(&b.start_line))
                .then_with(|| a.name.cmp(&b.name))
        });
        filtered.truncate(n);
    }
    sort_by_metric_desc(&mut filtered, metric);
    Some(filtered)
}

pub(super) fn write_cognitive_hotspots(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let Some(entries) = top_n_desc(funcs, top_n, |s| s.cognitive > 0.0, |s| s.cognitive) else {
        return;
    };

    let _ = writeln!(out, "\n### Cognitive Complexity Hotspots\n");
    let rows: Vec<Vec<String>> = entries
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

pub(super) fn write_halstead_hotspots(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let Some(entries) = top_n_desc(
        funcs,
        top_n,
        |s| s.halstead_effort > 0.0,
        |s| s.halstead_effort,
    ) else {
        return;
    };

    let _ = writeln!(out, "\n### Halstead Effort Hotspots\n");
    let rows: Vec<Vec<String>> = entries
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

pub(super) fn write_largest_by_sloc(out: &mut String, funcs: &[&FunctionSummary], top_n: usize) {
    let Some(entries) = top_n_desc(funcs, top_n, |s| s.sloc > 0, |s| s.sloc as f64) else {
        return;
    };

    let _ = writeln!(out, "\n### Largest Functions by SLOC\n");
    let rows: Vec<Vec<String>> = entries
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
    let Some(entries) = top_n_desc(funcs, top_n, |s| s.nargs > 3, |s| s.nargs as f64) else {
        return;
    };

    let _ = writeln!(out, "\n### Functions With Many Parameters (>3)\n");
    let rows: Vec<Vec<String>> = entries
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
    let Some(classes) = top_n_desc(
        entries,
        top_n,
        |s| is_class_like(s.kind) && s.wmc > 0.0,
        |s| s.wmc,
    ) else {
        return;
    };

    let _ = writeln!(out, "\n### Class/Trait/Impl Hotspots (WMC)\n");
    let rows: Vec<Vec<String>> = classes
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
    let Some(entries) = top_n_desc(funcs, top_n, |s| s.nexits > 0, |s| s.nexits as f64) else {
        return;
    };

    let _ = writeln!(out, "\n### Functions with the most exit points (NEXITS)\n");
    let rows: Vec<Vec<String>> = entries
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
    let Some(entries) = top_n_desc(funcs, top_n, |s| s.abc > 0.0, |s| s.abc) else {
        return;
    };

    let _ = writeln!(out, "\n### ABC Magnitude Hotspots\n");
    let rows: Vec<Vec<String>> = entries
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

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
