// bca: suppress-file(halstead, nargs, nom)
// The `SPECS` table is declarative data: ~70 capture-free cell-projector
// closures inflate the file-level nom/nargs/halstead sums (each closure is a
// one-arg "function"), the same string-formatting / many-fn aggregation
// artifact the sibling report renderers suppress — not per-function logic.

//! Format-neutral source of truth for the per-language hotspot sections
//! shared by the Markdown (`super`) and HTML (`crate::html_report`)
//! report renderers.
//!
//! This module decides WHICH functions appear in each hotspot section, in
//! what order, with what suppression, plus the per-section columns and the
//! cyclomatic summary stats. It does NOT decide escaping or table markup —
//! each renderer keeps its own `write_table` and maps a [`Cell`] to its own
//! format. Sharing the [`SPECS`] table is what keeps the two reports from
//! diverging (the "same data" guarantee; see the book's report
//! "Format consistency" section). Despite living under `markdown_report`,
//! nothing here is Markdown-specific.

use std::borrow::Cow;

use big_code_analysis::{Metric, SuppressionPolicy};

use super::{FunctionSummary, is_class_like, sort_by_metric_asc, sort_by_metric_desc, thousands};
use crate::format_util::MetricScalar;

/// Cell text alignment. Drives the Markdown separator (`--:`/`:--`) and the
/// HTML `data-numeric` / `class="numeric"` attributes.
#[derive(Clone, Copy)]
pub(crate) enum Align {
    Left,
    Right,
}

impl Align {
    pub(crate) fn is_numeric(self) -> bool {
        matches!(self, Self::Right)
    }
}

/// Sort direction for a hotspot's ranking metric.
#[derive(Clone, Copy)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

/// Which per-language slice a section ranks over.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// File-level `Unit` spaces (the MI table).
    Units,
    /// `Function` spaces (most hotspots).
    Funcs,
    /// The full per-language slice incl. class-likes (WMC).
    All,
}

/// A rendered cell carrying its semantic kind so each format escapes it
/// correctly. The payload is already value-formatted; only escaping differs:
/// HTML escapes every kind uniformly via `escape_html`; Markdown wraps a
/// [`Cell::Name`] in backticks (`escape_name`), GFM-escapes a [`Cell::Path`]
/// (`escape_cell`), and emits a [`Cell::Num`] raw.
#[derive(Clone)]
pub(crate) enum Cell {
    /// A function or class identifier.
    Name(String),
    /// A file path.
    Path(String),
    /// Pre-formatted numeric text (escaping is a no-op in either format).
    Num(String),
}

/// One column: header, alignment, and a capture-free projector to a [`Cell`].
#[derive(Clone, Copy)]
pub(crate) struct Column {
    pub(crate) header: &'static str,
    pub(crate) align: Align,
    pub(crate) cell: fn(&FunctionSummary) -> Cell,
}

/// A section title. Static for every section except the MI table, whose
/// title interpolates `top_n`.
#[derive(Clone, Copy)]
pub(crate) enum HotspotTitle {
    Static(&'static str),
    MiLowest,
}

impl HotspotTitle {
    /// The logical (unescaped) title; the MI variant fills in `top_n`. Each
    /// renderer escapes the result for its format (HTML escapes the `>` in
    /// the many-parameters title; Markdown emits it raw).
    pub(crate) fn render(self, top_n: usize) -> Cow<'static, str> {
        match self {
            Self::Static(s) => Cow::Borrowed(s),
            Self::MiLowest => {
                // `--top 0` shows every file, so the title says "all" rather
                // than the misleading "top-0" (issue #602).
                let suffix = cap(top_n).map_or_else(|| "all".to_owned(), |n| format!("top-{n}"));
                Cow::Owned(format!("Maintainability Index (lowest files, {suffix})"))
            }
        }
    }
}

/// One hotspot section's full data contract.
pub(crate) struct HotspotSpec {
    pub(crate) title: HotspotTitle,
    pub(crate) source: Source,
    pub(crate) keep: fn(&FunctionSummary) -> bool,
    pub(crate) metric: fn(&FunctionSummary) -> f64,
    pub(crate) dir: SortDir,
    pub(crate) metric_kind: Metric,
    pub(crate) columns: &'static [Column],
    /// Cyclomatic is the one section with a trailing summary note.
    pub(crate) cc_note: bool,
}

// Reusable column descriptors. Each `cell` carries the SAME value formatting
// the two renderers used before unification; only the `Cell` *kind* (which
// drives per-format escaping) is new.
const COL_FUNCTION: Column = Column {
    header: "Function",
    align: Align::Left,
    cell: |s| Cell::Name(s.name.clone()),
};
const COL_FILE: Column = Column {
    header: "File",
    align: Align::Left,
    cell: |s| Cell::Path(s.file.clone()),
};
const COL_LINE: Column = Column {
    header: "Line",
    align: Align::Right,
    cell: |s| Cell::Num(s.start_line.to_string()),
};
const COL_CC: Column = Column {
    header: "CC",
    align: Align::Right,
    cell: |s| Cell::Num(MetricScalar(s.cyclomatic).to_string()),
};
const COL_COGNITIVE: Column = Column {
    header: "Cognitive",
    align: Align::Right,
    cell: |s| Cell::Num(MetricScalar(s.cognitive).to_string()),
};
const COL_SLOC: Column = Column {
    header: "SLOC",
    align: Align::Right,
    cell: |s| Cell::Num(thousands(s.sloc)),
};
const COL_TOKENS: Column = Column {
    header: "Tokens",
    align: Align::Right,
    cell: |s| Cell::Num(thousands(s.tokens)),
};

/// The hotspot sections in canonical render order. Both renderers iterate
/// this; the Actionable Summary is spliced just before
/// [`ACTIONABLE_SUMMARY_INDEX`].
pub(crate) const SPECS: &[HotspotSpec] = &[
    // 0 — Maintainability Index (lowest files). `keep` mirrors
    // `mi::Stats::inputs_are_empty` so a clamped-to-0 worst file still shows.
    HotspotSpec {
        title: HotspotTitle::MiLowest,
        source: Source::Units,
        keep: |s| s.halstead_volume > 0.0 && s.sloc > 0,
        metric: |s| s.mi_visual_studio,
        dir: SortDir::Asc,
        metric_kind: Metric::Mi,
        columns: &[
            COL_FILE,
            Column {
                header: "MI",
                align: Align::Right,
                cell: |s| Cell::Num(format!("{:.1}", s.mi_visual_studio)),
            },
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 1 — Cyclomatic Complexity (carries the summary note).
    HotspotSpec {
        title: HotspotTitle::Static("Cyclomatic Complexity Hotspots"),
        source: Source::Funcs,
        keep: |s| s.cyclomatic > 0.0,
        metric: |s| s.cyclomatic,
        dir: SortDir::Desc,
        metric_kind: Metric::Cyclomatic,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            COL_LINE,
            COL_CC,
            COL_COGNITIVE,
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: true,
    },
    // 2 — Cognitive Complexity.
    HotspotSpec {
        title: HotspotTitle::Static("Cognitive Complexity Hotspots"),
        source: Source::Funcs,
        keep: |s| s.cognitive > 0.0,
        metric: |s| s.cognitive,
        dir: SortDir::Desc,
        metric_kind: Metric::Cognitive,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            COL_LINE,
            COL_COGNITIVE,
            COL_CC,
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 3 — Halstead Effort.
    HotspotSpec {
        title: HotspotTitle::Static("Halstead Effort Hotspots"),
        source: Source::Funcs,
        keep: |s| s.halstead_effort > 0.0,
        metric: |s| s.halstead_effort,
        dir: SortDir::Desc,
        metric_kind: Metric::Halstead,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            Column {
                header: "Effort",
                align: Align::Right,
                cell: |s| Cell::Num(MetricScalar(s.halstead_effort).to_string()),
            },
            Column {
                header: "Volume",
                align: Align::Right,
                cell: |s| Cell::Num(MetricScalar(s.halstead_volume).to_string()),
            },
            Column {
                header: "Est. Bugs",
                align: Align::Right,
                cell: |s| Cell::Num(format!("{:.2}", s.halstead_bugs)),
            },
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 4 — Largest Functions by SLOC.
    HotspotSpec {
        title: HotspotTitle::Static("Largest Functions by SLOC"),
        source: Source::Funcs,
        keep: |s| s.sloc > 0,
        metric: |s| s.sloc as f64,
        dir: SortDir::Desc,
        metric_kind: Metric::Loc,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            COL_LINE,
            COL_SLOC,
            COL_TOKENS,
            COL_CC,
            COL_COGNITIVE,
        ],
        cc_note: false,
    },
    // 5 — Functions With Many Parameters (>3). Title `>` is logical here;
    // HTML escapes it to `&gt;`, Markdown writes it raw.
    HotspotSpec {
        title: HotspotTitle::Static("Functions With Many Parameters (>3)"),
        source: Source::Funcs,
        keep: |s| s.nargs > 3,
        metric: |s| s.nargs as f64,
        dir: SortDir::Desc,
        metric_kind: Metric::NArgs,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            Column {
                header: "Args",
                align: Align::Right,
                cell: |s| Cell::Num(s.nargs.to_string()),
            },
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 6 — Class/Trait/Impl (WMC). Drawn from the FULL slice (`Source::All`),
    // since class-likes are excluded from both the unit and function buckets.
    // The Actionable Summary is emitted immediately before this section.
    HotspotSpec {
        title: HotspotTitle::Static("Class/Trait/Impl Hotspots (WMC)"),
        source: Source::All,
        keep: |s| is_class_like(s.kind) && s.wmc > 0.0,
        metric: |s| s.wmc,
        dir: SortDir::Desc,
        metric_kind: Metric::Wmc,
        columns: &[
            Column {
                header: "Class",
                align: Align::Left,
                cell: |s| Cell::Name(s.name.clone()),
            },
            COL_FILE,
            COL_LINE,
            Column {
                header: "WMC",
                align: Align::Right,
                cell: |s| Cell::Num(MetricScalar(s.wmc).to_string()),
            },
            Column {
                header: "Methods",
                align: Align::Right,
                cell: |s| Cell::Num(s.nom.to_string()),
            },
            Column {
                header: "NPA",
                align: Align::Right,
                cell: |s| Cell::Num(MetricScalar(s.npa).to_string()),
            },
            Column {
                header: "NPM",
                align: Align::Right,
                cell: |s| Cell::Num(MetricScalar(s.npm).to_string()),
            },
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 7 — Functions with the most exit points (NEXITS). `Metric::Nexits`
    // is the canonical spelling shared by suppression and the threshold
    // engine (post-#555 unification).
    HotspotSpec {
        title: HotspotTitle::Static("Functions with the most exit points (NEXITS)"),
        source: Source::Funcs,
        keep: |s| s.nexits > 0,
        metric: |s| s.nexits as f64,
        dir: SortDir::Desc,
        metric_kind: Metric::Nexits,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            COL_LINE,
            Column {
                header: "Exits",
                align: Align::Right,
                cell: |s| Cell::Num(s.nexits.to_string()),
            },
            COL_CC,
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
    // 8 — ABC Magnitude.
    HotspotSpec {
        title: HotspotTitle::Static("ABC Magnitude Hotspots"),
        source: Source::Funcs,
        keep: |s| s.abc > 0.0,
        metric: |s| s.abc,
        dir: SortDir::Desc,
        metric_kind: Metric::Abc,
        columns: &[
            COL_FUNCTION,
            COL_FILE,
            COL_LINE,
            Column {
                header: "ABC",
                align: Align::Right,
                cell: |s| Cell::Num(format!("{:.1}", s.abc)),
            },
            COL_SLOC,
            COL_TOKENS,
        ],
        cc_note: false,
    },
];

/// Caption appended to the cyclomatic summary note, naming the population it
/// covers: the same suppression-filtered set the CC hotspot table shows. The
/// raw, suppression-independent CC count lives in the Actionable Summary, so
/// without this caption a reader sees two different "CC > 10" figures with no
/// explanation (issue #616). Shared verbatim by both renderers.
pub(crate) const CC_NOTE_SUPPRESSED_CAPTION: &str = "excluding suppressed functions";

/// Logical (unescaped) lead-in for the Actionable Summary, captioning it as a
/// raw whole-codebase roll-up that — unlike the hotspot tables — counts
/// functions regardless of suppression policy (issue #501, #616). `suppressed`
/// is the number of functions carrying a marker (see
/// [`suppressed_func_count`]); when it is `0` the parenthetical is dropped.
/// Both renderers feed the result through their own escaper.
pub(crate) fn actionable_summary_caption(suppressed: usize) -> Cow<'static, str> {
    if suppressed == 0 {
        Cow::Borrowed("Raw counts across all functions, ignoring suppression markers.")
    } else {
        Cow::Owned(format!(
            "Raw counts across all functions, including {suppressed} suppressed \
             (re-run with --no-suppress to list them)."
        ))
    }
}

/// Logical (unescaped) caption emitted in place of a hotspot table that was
/// dropped *because suppression hid every matching row* (see
/// [`fully_suppressed_count`]). Keeps an Actionable-Summary bullet from
/// dangling when its detail table is silently absent (issue #616).
pub(crate) fn fully_suppressed_caption(metric_label: &str, count: usize) -> String {
    format!("{metric_label} table omitted: all {count} matching functions suppressed.")
}

/// Index into [`SPECS`] before which the Actionable Summary is emitted
/// (i.e. after Many-Parameters, before WMC), so both renderers interleave
/// it identically.
pub(crate) const ACTIONABLE_SUMMARY_INDEX: usize = 6;

/// Cyclomatic summary stats for the CC note, computed over the FULL
/// suppression-filtered set (before top-N truncation), as both renderers did.
pub(crate) struct CyclomaticStats {
    pub(crate) sum: f64,
    pub(crate) count: usize,
    pub(crate) max: f64,
    pub(crate) gt10: usize,
    pub(crate) gt20: usize,
}

impl CyclomaticStats {
    /// Entries are already `cyclomatic > 0.0` (the CC `keep`), so no internal
    /// guard is needed — matching the pre-unification stats in both formats.
    fn from_entries(entries: &[&FunctionSummary]) -> Self {
        let mut s = Self {
            sum: 0.0,
            count: 0,
            max: f64::NAN,
            gt10: 0,
            gt20: 0,
        };
        for f in entries {
            let c = f.cyclomatic;
            s.sum += c;
            s.count += 1;
            s.max = f64::max(s.max, c);
            s.gt10 += usize::from(c > 10.0);
            s.gt20 += usize::from(c > 20.0);
        }
        s
    }

    pub(crate) fn avg(&self) -> f64 {
        if self.count > 0 {
            self.sum / self.count as f64
        } else {
            0.0
        }
    }
}

/// Translate a `--top` value into an optional row cap under the unified
/// `0 = all` semantics (issue #602): `0` means "no cap" (`None`), any other
/// `n` caps to `n` rows (`Some(n)`). The single definition the report
/// renderers share so the three `--top`-family flags can't drift again.
pub(crate) fn cap(top_n: usize) -> Option<usize> {
    (top_n != 0).then_some(top_n)
}

/// In-place: keep the `top_n` highest-by-`metric` survivors of `v`, sorted
/// descending. `top_n == 0` means "no cap" — keep every survivor (the unified
/// `0 = all` semantics, issue #602). `O(N + k·log k)` via
/// `select_nth_unstable_by` over the same total-order comparator as
/// [`sort_by_metric_desc`]. Carries the `n < len` guard so it never partitions
/// needlessly.
fn partial_top_n_desc<M: Fn(&FunctionSummary) -> f64>(
    v: &mut Vec<&FunctionSummary>,
    top_n: usize,
    metric: M,
) {
    // `cap()` collapses `0 = all` to the actual length, so the partition and
    // truncate below keep every row when no cap was requested.
    let n = cap(top_n).map_or(v.len(), |c| v.len().min(c));
    if n < v.len() {
        // Same comparator as `sort_by_metric_desc` (metric desc + shared
        // `tiebreak`), so the partition selects exactly the rows the final
        // sort will keep.
        v.select_nth_unstable_by(n - 1, |a, b| {
            metric(b)
                .total_cmp(&metric(a))
                .then_with(|| super::tiebreak(a, b))
        });
        v.truncate(n);
    }
    sort_by_metric_desc(v, metric);
}

/// Filter `entries`, keep the `top_n` highest-by-`metric` survivors sorted
/// descending (`top_n == 0` keeps all — issue #602). `None` when the filter
/// drops everything so callers can skip an empty heading.
pub(crate) fn top_n_desc<'a, F, M>(
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
    partial_top_n_desc(&mut filtered, top_n, metric);
    Some(filtered)
}

/// One section's membership predicate: the spec's own `keep` plus the
/// suppression filter for its metric. The single definition shared by
/// [`select`] and [`select_cc`] so the two cannot diverge on what a section
/// includes.
fn section_keep(
    spec: &HotspotSpec,
    policy: SuppressionPolicy,
) -> impl Fn(&FunctionSummary) -> bool + '_ {
    move |s| (spec.keep)(s) && !s.is_hidden_for(spec.metric_kind, policy)
}

/// Suppression-filter + sort + top-N for one section. The single entry point
/// both renderers use so section membership and order are provably identical.
pub(crate) fn select<'a>(
    spec: &HotspotSpec,
    base: &[&'a FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) -> Vec<&'a FunctionSummary> {
    let keep = section_keep(spec, policy);
    match spec.dir {
        SortDir::Desc => top_n_desc(base, top_n, keep, spec.metric).unwrap_or_default(),
        SortDir::Asc => {
            let mut v: Vec<&FunctionSummary> = base.iter().copied().filter(|s| keep(s)).collect();
            sort_by_metric_asc(&mut v, spec.metric);
            // `cap()` yields `None` for `top_n == 0` (show all); otherwise cap.
            if let Some(c) = cap(top_n) {
                v.truncate(v.len().min(c));
            }
            v
        }
    }
}

/// Whether a section's table was rendered empty *because suppression hid
/// every matching row*, as opposed to no row matching the section's own
/// `keep` predicate. Distinguishes "table omitted: all N functions
/// suppressed" (a caption the renderers emit so a summary bullet never
/// dangles) from "metric genuinely absent" (stay silent).
///
/// Returns the count of rows that match `spec.keep` but are hidden by the
/// metric's suppression under `policy`; `0` when nothing matched `keep` at
/// all, or when no matching row is suppressed. Under
/// [`SuppressionPolicy::Ignore`] (`--no-suppress`) nothing is hidden, so this
/// is always `0`.
pub(crate) fn fully_suppressed_count(
    spec: &HotspotSpec,
    base: &[&FunctionSummary],
    policy: SuppressionPolicy,
) -> usize {
    let mut matched = 0usize;
    let mut hidden = 0usize;
    for s in base.iter().filter(|s| (spec.keep)(s)) {
        matched += 1;
        if s.is_hidden_for(spec.metric_kind, policy) {
            hidden += 1;
        }
    }
    // Only a *fully* suppressed table earns the caption: every keep-matching
    // row was hidden, and at least one row matched in the first place.
    if matched > 0 && hidden == matched {
        hidden
    } else {
        0
    }
}

/// Number of distinct functions in `funcs` carrying *any* suppression marker
/// under `policy` — the "N suppressed" figure the raw Actionable Summary
/// cites so a reader can reconcile its counts against the suppression-filtered
/// hotspot tables. `0` under [`SuppressionPolicy::Ignore`], since
/// `--no-suppress` honors no markers.
pub(crate) fn suppressed_func_count(
    funcs: &[&FunctionSummary],
    policy: SuppressionPolicy,
) -> usize {
    if matches!(policy, SuppressionPolicy::Ignore) {
        return 0;
    }
    funcs.iter().filter(|s| !s.suppressed.is_empty()).count()
}

/// Like [`select`] but also returns the cyclomatic stats over the FULL
/// suppression-filtered set (before truncation), for the CC note.
pub(crate) fn select_cc<'a>(
    spec: &HotspotSpec,
    base: &[&'a FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
) -> (Vec<&'a FunctionSummary>, CyclomaticStats) {
    let keep = section_keep(spec, policy);
    let mut v: Vec<&FunctionSummary> = base.iter().copied().filter(|s| keep(s)).collect();
    let stats = CyclomaticStats::from_entries(&v);
    partial_top_n_desc(&mut v, top_n, spec.metric);
    (v, stats)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use big_code_analysis::{LANG, SpaceKind};

    /// Minimal `FunctionSummary` builder: every numeric field is set to the
    /// same `metric` so one argument exercises every per-section sort.
    fn summary(name: &str, file: &str, start_line: usize, metric: f64) -> FunctionSummary {
        FunctionSummary {
            file: file.to_string(),
            name: name.to_string(),
            kind: SpaceKind::Function,
            language: LANG::Rust,
            suppressed: big_code_analysis::SuppressionScope::default(),
            start_line,
            end_line: start_line + 10,
            sloc: metric as usize,
            ploc: metric as usize,
            lloc: metric as usize,
            cloc: 0,
            tokens: 30,
            cyclomatic: metric,
            cognitive: metric,
            halstead_volume: metric,
            halstead_difficulty: metric,
            halstead_effort: metric,
            halstead_bugs: 0.1,
            halstead_time: 28.0,
            mi_original: 80.0,
            mi_sei: 85.0,
            mi_visual_studio: 50.0,
            nargs: metric as usize,
            nexits: metric as usize,
            nom: 1,
            abc: metric,
            wmc: metric,
            npa: 0.0,
            npm: 0.0,
        }
    }

    fn rust_funcs() -> Vec<FunctionSummary> {
        super::super::rich_fixture()
            .into_iter()
            .filter(|s| s.language == LANG::Rust && s.kind == SpaceKind::Function)
            .collect()
    }

    // The canonical spec order both renderers depend on.
    #[test]
    fn specs_count_and_actionable_splice() {
        assert_eq!(SPECS.len(), 9);
        assert!(matches!(SPECS[5].metric_kind, Metric::NArgs)); // Many-Params
        assert!(matches!(
            SPECS[ACTIONABLE_SUMMARY_INDEX].metric_kind,
            Metric::Wmc
        ));
        assert!(
            SPECS[1].cc_note,
            "only the cyclomatic spec carries the note"
        );
        assert_eq!(SPECS.iter().filter(|s| s.cc_note).count(), 1);
    }

    #[test]
    fn select_truncates_and_drops_suppressed() {
        let fx = rust_funcs();
        let refs: Vec<&FunctionSummary> = fx.iter().collect();
        // 7 non-suppressed cyclomatic funcs + 1 suppressed (secret_internal).
        let rows = select(&SPECS[1], &refs, 5, SuppressionPolicy::Honor);
        assert_eq!(rows.len(), 5, "top-5 truncation");
        assert!(
            !rows.iter().any(|s| s.name == "secret_internal"),
            "suppressed function dropped from the CC table"
        );
        assert_eq!(rows[0].name, "process_request", "highest CC first");
    }

    #[test]
    fn select_cc_stats_cover_full_filtered_set() {
        let fx = rust_funcs();
        let refs: Vec<&FunctionSummary> = fx.iter().collect();
        let (rows, stats) = select_cc(&SPECS[1], &refs, 5, SuppressionPolicy::Honor);
        assert_eq!(rows.len(), 5, "display truncated to top-5");
        // Stats over all 7 non-suppressed (25,20,15,12,8,5,3): 4 > 10, 1 > 20.
        assert_eq!(stats.count, 7);
        assert_eq!(stats.gt10, 4);
        assert_eq!(stats.gt20, 1);
        assert_eq!(stats.max, 25.0);
    }

    #[test]
    fn select_cc_note_excludes_suppressed_but_audit_includes_it() {
        let fx = rust_funcs();
        let refs: Vec<&FunctionSummary> = fx.iter().collect();
        let (_, stats) = select_cc(&SPECS[1], &refs, 20, SuppressionPolicy::Honor);
        assert_eq!(stats.max, 25.0, "note excludes the suppressed cc=99");
        let (_, audit) = select_cc(&SPECS[1], &refs, 20, SuppressionPolicy::Ignore);
        assert_eq!(audit.max, 99.0);
    }

    #[test]
    fn mi_select_keeps_clamped_zero_but_drops_empty() {
        // SPECS[0] is the MI table; its keep mirrors `mi::Stats::inputs_are_empty`
        // (`halstead_volume > 0 && sloc > 0`). A clamped-to-0 worst file
        // (volume>0, sloc>0) is kept; every file failing EITHER half of the
        // conjunction is dropped — including the asymmetric volume-only and
        // sloc-only cases, which pins the `&&` against a future `||` flip.
        let mut worst = summary("big", "src/big.rs", 1, 10.0);
        worst.mi_visual_studio = 0.0;
        worst.halstead_volume = 5_000.0;
        worst.sloc = 2_000;
        let mut empty = summary("empty", "src/empty.rs", 1, 0.0);
        empty.mi_visual_studio = 0.0;
        empty.halstead_volume = 0.0;
        empty.sloc = 0;
        let mut vol_only = summary("vol_only", "src/vol.rs", 1, 0.0);
        vol_only.mi_visual_studio = 0.0;
        vol_only.halstead_volume = 1_000.0;
        vol_only.sloc = 0;
        let mut sloc_only = summary("sloc_only", "src/sloc.rs", 1, 0.0);
        sloc_only.mi_visual_studio = 0.0;
        sloc_only.halstead_volume = 0.0;
        sloc_only.sloc = 50;
        let refs = vec![&worst, &empty, &vol_only, &sloc_only];
        let rows = select(&SPECS[0], &refs, 20, SuppressionPolicy::Honor);
        let kept: Vec<&str> = rows.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(
            kept,
            ["src/big.rs"],
            "only the volume>0 && sloc>0 file is kept (mirrors inputs_are_empty)"
        );
    }

    #[test]
    fn top_n_desc_returns_none_when_filter_drops_every_entry() {
        let entries = [summary("a", "f.rs", 1, 5.0)];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        assert!(top_n_desc(&refs, 10, |_| false, |s| s.cyclomatic).is_none());
    }

    #[test]
    fn top_n_desc_top_n_zero_keeps_all_sorted() {
        // Issue #602: `top_n == 0` means "no cap" — every survivor is kept,
        // still sorted descending. `--top` no longer clamps to `range(1..)`.
        let entries = [
            summary("low", "f.rs", 1, 1.0),
            summary("high", "f.rs", 2, 10.0),
            summary("mid", "f.rs", 3, 5.0),
        ];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        let got = top_n_desc(&refs, 0, |_| true, |s| s.cyclomatic).expect("Some");
        assert_eq!(got.len(), 3);
        assert_eq!(
            got.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["high", "mid", "low"]
        );
    }

    #[test]
    fn cap_zero_is_no_cap() {
        assert_eq!(cap(0), None);
        assert_eq!(cap(1), Some(1));
        assert_eq!(cap(20), Some(20));
    }

    #[test]
    fn select_top_n_zero_keeps_all_rows() {
        // Issue #602: `0 = all` flows through both sort directions. The CC
        // (Desc) and MI (Asc) sections must list every survivor, not zero.
        let fx = rust_funcs();
        let refs: Vec<&FunctionSummary> = fx.iter().collect();
        // 7 non-suppressed cyclomatic funcs survive `keep`; `--top 0` keeps all.
        let desc = select(&SPECS[1], &refs, 0, SuppressionPolicy::Honor);
        assert_eq!(desc.len(), 7, "Desc: top 0 keeps every survivor");
        let trunc = select(&SPECS[1], &refs, 3, SuppressionPolicy::Honor);
        assert_eq!(trunc.len(), 3, "nonzero still truncates");

        let (cc_all, _) = select_cc(&SPECS[1], &refs, 0, SuppressionPolicy::Honor);
        assert_eq!(cc_all.len(), 7, "select_cc: top 0 keeps every survivor");
    }

    #[test]
    fn mi_title_says_all_when_uncapped() {
        // `--top 0` renders "all", not the misleading "top-0".
        assert_eq!(
            HotspotTitle::MiLowest.render(0),
            "Maintainability Index (lowest files, all)"
        );
        assert_eq!(
            HotspotTitle::MiLowest.render(5),
            "Maintainability Index (lowest files, top-5)"
        );
    }

    #[test]
    fn top_n_desc_top_n_larger_than_len_returns_all_sorted() {
        let entries = [
            summary("low", "f.rs", 1, 1.0),
            summary("high", "f.rs", 2, 10.0),
            summary("mid", "f.rs", 3, 5.0),
        ];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        let got = top_n_desc(&refs, 100, |_| true, |s| s.cyclomatic).expect("Some");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].name, "high");
        assert_eq!(got[1].name, "mid");
        assert_eq!(got[2].name, "low");
    }

    #[test]
    fn top_n_desc_top_n_less_than_len_keeps_only_highest_n() {
        let entries = [
            summary("low", "f.rs", 1, 1.0),
            summary("high", "f.rs", 2, 10.0),
            summary("mid", "f.rs", 3, 5.0),
        ];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        let got = top_n_desc(&refs, 2, |_| true, |s| s.cyclomatic).expect("Some");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "high");
        assert_eq!(got[1].name, "mid");
    }

    #[test]
    fn top_n_desc_breaks_ties_by_file_then_line_then_name() {
        // All four share the metric value, so the comparator falls through to
        // file (asc) → start_line (asc) → name (asc); the unstable partition
        // could otherwise pick an arbitrary subset of the ties.
        let entries = [
            summary("z", "b.rs", 1, 5.0),
            summary("y", "a.rs", 2, 5.0),
            summary("x", "a.rs", 1, 5.0),
            summary("w", "a.rs", 1, 5.0),
        ];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        let got = top_n_desc(&refs, 4, |_| true, |s| s.cyclomatic).expect("Some");
        assert_eq!(got[0].file, "a.rs");
        assert_eq!(got[0].start_line, 1);
        assert_eq!(got[0].name, "w");
        assert_eq!(got[1].file, "a.rs");
        assert_eq!(got[1].start_line, 1);
        assert_eq!(got[1].name, "x");
        assert_eq!(got[2].file, "a.rs");
        assert_eq!(got[2].start_line, 2);
        assert_eq!(got[3].file, "b.rs");
    }

    #[test]
    fn top_n_desc_applies_filter_before_sort() {
        let entries = [
            summary("low", "f.rs", 1, 1.0),
            summary("high", "f.rs", 2, 10.0),
            summary("mid", "f.rs", 3, 5.0),
        ];
        let refs: Vec<&FunctionSummary> = entries.iter().collect();
        let got = top_n_desc(&refs, 10, |s| s.cyclomatic > 3.0, |s| s.cyclomatic).expect("Some");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "high");
        assert_eq!(got[1].name, "mid");
    }
}
