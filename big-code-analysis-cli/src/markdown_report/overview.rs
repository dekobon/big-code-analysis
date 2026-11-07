//! The Per-language overview table, defined once as a column spec.
//!
//! The overview is a per-language aggregate (one row per language), not
//! a per-function hotspot, so it cannot share [`hotspot::Column`]; but it
//! follows the same pattern — headers, alignments, cell projectors and
//! tooltips on one array — so the two renderers and both legends read
//! the same definition and a column cannot gain or lose a header, a
//! tooltip or an alignment on one side only.

use super::hotspot::{self, Cell};
use super::{Align, FunctionSummary, MiAverage, language_display_name, thousands};
use big_code_analysis::SpaceKind;

/// Tooltip for the Per-language overview's `Avg MI` column. Distinct from
/// [`hotspot::MI_TOOLTIP`] (the per-file hotspot column, which shows the
/// *clamped* `mi_visual_studio`): the overview average is SLOC-weighted
/// and derived from the *unclamped* value, so it can go negative for an
/// unmaintainable codebase instead of saturating at 0 (issue #725).
const AVG_MI_TOOLTIP: &str = "Average Maintainability Index, SLOC-weighted across the language's files so larger files dominate. Unlike the per-file MI column it uses the unclamped Visual Studio value, so a codebase of unmaintainable files reads negative rather than saturating at 0. GOOD \u{2265} 20, MODERATE \u{2265} 10, else LOW; any value below 10, however negative, is LOW.";
/// Tooltip for the Per-language overview's `Avg CC` column. Names the
/// population — every function, suppression markers ignored — because the
/// CC note under the cyclomatic hotspot table averages a *different* set
/// (suppressed functions excluded) and the two figures can disagree.
const AVG_CC_TOOLTIP: &str = "Mean Cyclomatic Complexity over every function and method in the language, ignoring suppression markers. The Average CC under the cyclomatic hotspot table covers every function eligible for that table before its top-N cut (CC above 0 and, by default, not suppressed), so the two can differ.";
/// Tooltip for the Per-language overview's `Avg Cognitive` column.
const AVG_COGNITIVE_TOOLTIP: &str = "Mean Cognitive Complexity over every function and method in the language, ignoring suppression markers.";
/// "Files" here means *source files analysed*; the bus-factor table's
/// like-named column means files-per-directory and supplies its own
/// tooltip (issue #610), so this entry never leaks onto it.
const FILES_TOOLTIP: &str = "Number of source files analysed.";
const FUNCTIONS_TOOLTIP: &str = "Number of functions and methods analysed.";

/// One language's overview aggregates: the quantities the seven columns
/// project. Computed once per language by [`LangOverview::from_summaries`].
pub(crate) struct LangOverview {
    display_name: String,
    files: usize,
    sloc: usize,
    functions: usize,
    /// `None` when no unit carried code to measure (see [`MiAverage`]).
    avg_mi: Option<f64>,
    avg_cc: f64,
    avg_cognitive: f64,
}

impl LangOverview {
    /// Fold one language's summaries: the file-level quantities over its
    /// `Unit` spaces and the averages over its `Function` spaces.
    pub(crate) fn from_summaries(lang_name: &str, summaries: &[&FunctionSummary]) -> Self {
        let (files, sloc, avg_mi) = unit_aggregates(summaries);
        let (functions, avg_cc, avg_cognitive) = function_aggregates(summaries);
        Self {
            display_name: language_display_name(lang_name).into_owned(),
            files,
            sloc,
            functions,
            avg_mi,
            avg_cc,
            avg_cognitive,
        }
    }

    /// The row, one [`Cell`] per column of [`OVERVIEW_SPECS`].
    pub(crate) fn cells(&self) -> Vec<Cell> {
        OVERVIEW_SPECS.iter().map(|c| (c.cell)(self)).collect()
    }
}

fn unit_aggregates(lang_summaries: &[&FunctionSummary]) -> (usize, usize, Option<f64>) {
    let mut count = 0usize;
    let mut sloc = 0usize;
    let mut mi = MiAverage::default();
    for s in lang_summaries.iter().filter(|s| s.kind == SpaceKind::Unit) {
        count += 1;
        sloc += s.sloc;
        mi.add(s);
    }
    (count, sloc, mi.average())
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

/// One Per-language overview column: header, alignment, the cell it
/// projects from the language's aggregates, and its tooltip (`None` for
/// the `Language` identity column). The tooltip is the single source of
/// the column's definition: the HTML renderer attaches it as `title=`
/// and both legends list it through [`legend`].
pub(crate) struct OverviewColumn {
    pub(crate) header: &'static str,
    pub(crate) align: Align,
    pub(crate) cell: fn(&LangOverview) -> Cell,
    pub(crate) tooltip: Option<&'static str>,
}

/// The seven overview columns, in render order.
pub(crate) const OVERVIEW_SPECS: &[OverviewColumn] = &[
    OverviewColumn {
        header: "Language",
        align: Align::Left,
        cell: |o| Cell::Text(o.display_name.clone()),
        tooltip: None,
    },
    OverviewColumn {
        header: "Files",
        align: Align::Right,
        cell: |o| Cell::Num(thousands(o.files)),
        tooltip: Some(FILES_TOOLTIP),
    },
    OverviewColumn {
        header: "SLOC",
        align: Align::Right,
        cell: |o| Cell::Num(thousands(o.sloc)),
        tooltip: Some(hotspot::SLOC_TOOLTIP),
    },
    OverviewColumn {
        header: "Functions",
        align: Align::Right,
        cell: |o| Cell::Num(thousands(o.functions)),
        tooltip: Some(FUNCTIONS_TOOLTIP),
    },
    OverviewColumn {
        header: "Avg MI",
        align: Align::Right,
        // An empty cell, not `n/a`, when no file had code to measure: the
        // HTML table sorter reads an empty cell as "no value" and sorts it
        // last in either direction; any other text would parse as NaN and
        // sort the same way, but read as a value in the cell.
        cell: |o| Cell::Num(o.avg_mi.map_or_else(String::new, |mi| format!("{mi:.1}"))),
        tooltip: Some(AVG_MI_TOOLTIP),
    },
    OverviewColumn {
        header: "Avg CC",
        align: Align::Right,
        cell: |o| Cell::Num(format!("{:.1}", o.avg_cc)),
        tooltip: Some(AVG_CC_TOOLTIP),
    },
    OverviewColumn {
        header: "Avg Cognitive",
        align: Align::Right,
        cell: |o| Cell::Num(format!("{:.1}", o.avg_cognitive)),
        tooltip: Some(AVG_COGNITIVE_TOOLTIP),
    },
];

pub(crate) fn headers() -> Vec<&'static str> {
    OVERVIEW_SPECS.iter().map(|c| c.header).collect()
}

pub(crate) fn aligns() -> Vec<Align> {
    OVERVIEW_SPECS.iter().map(|c| c.align).collect()
}

pub(crate) fn tooltips() -> Vec<Option<&'static str>> {
    OVERVIEW_SPECS.iter().map(|c| c.tooltip).collect()
}

/// The overview's legend entries, derived from the spec so a column
/// cannot exist without its definition being considered, nor the legend
/// name a column that does not render. Listed in both renderers' legends
/// so a reader who meets a negative `Avg MI` has the explanation on the
/// page rather than only on hover; `Language` carries none.
pub(crate) fn legend() -> Vec<(&'static str, &'static str)> {
    OVERVIEW_SPECS
        .iter()
        .filter_map(|c| c.tooltip.map(|t| (c.header, t)))
        .collect()
}
