//! Attach change-history [`Stats`] to a [`FuncSpace`] tree and score each
//! block's `hotspot_score` (issue #1743).
//!
//! The one owner of the rule that chooses which cyclomatic figure a
//! hotspot is scored from: the file (root) space uses its subtree
//! `cyclomatic_sum()` so the file-level score matches the `bca vcs`
//! column, and every descendant — container or function — uses its own
//! `cyclomatic()` so nested complexity is not re-counted at each
//! enclosing level (#709). The public surface is three functions —
//! [`file_complexity`], [`attach_file_history`] and
//! [`attach_per_function_history`] — and the four sites that score a
//! hotspot all call in here: the CLI's `metrics --vcs` and
//! `--vcs-per-function` injectors, its `report --vcs` join, and the
//! Python bindings' `analyze(vcs=True)` / `vcs_per_function=True`.
//! Before #1743 each of the four carried its own copy of the rule and
//! they drifted twice (#709 fixed only the CLI, which became #1624;
//! #1742 was the CLI scoring from an uncomputed cyclomatic while the
//! bindings omitted it).
//!
//! The gate is [`crate::MetricSet::contains`]`(Metric::Cyclomatic)` on the
//! space's [`CodeMetrics::selected`]: the set the wire serializer
//! consults to elide a metric, so it is exactly "the serialized metrics
//! carry no `cyclomatic`". A space analysed with only `nargs` reads a
//! file-level sum of 0 and a default own value of 1, neither of which is
//! a complexity (#1742).
//
// The two front ends disagree on whether per-function attribution
// implies the file-level block — `bca metrics --vcs-per-function`
// implies `--vcs`, while the bindings' `vcs_per_function=True` is
// independent of `vcs=True` (documented in `_native.pyi`). That is
// front-end policy, deliberately: `attach_file_history` and
// `attach_per_function_history` are two calls, and each front end
// decides whether one entails the other.

use std::path::Path;

use crate::vcs::git::BlameSession;
use crate::vcs::{Error, HistoryIndex, LineSpan, Stats, hotspot};
use crate::{CodeMetrics, FuncSpace, Metric};

/// Which cyclomatic figure a space's hotspot is scored from. Crate-private
/// so no caller can score a root space by `Own` or a descendant by
/// `SubtreeSum`: the two attach functions are the only ways in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HotspotComplexity {
    /// `cyclomatic.sum`: the space plus everything nested in it. The
    /// file-level figure, so `bca metrics --vcs` matches `bca vcs`.
    SubtreeSum,
    /// `cyclomatic.value`: the space's own complexity. The per-function
    /// figure, so a nested function's branches are not re-counted at
    /// every enclosing level (#709).
    Own,
}

/// The one gate: `figure` of `metrics`, or `None` when cyclomatic was not
/// computed for the space (#1742). Cyclomatic counts stay far below
/// 2^53, so the cast is exact.
#[allow(clippy::cast_precision_loss)]
fn complexity(metrics: &CodeMetrics, figure: HotspotComplexity) -> Option<f64> {
    if !metrics.selected.contains(Metric::Cyclomatic) {
        return None;
    }
    let count = match figure {
        HotspotComplexity::SubtreeSum => metrics.cyclomatic.cyclomatic_sum(),
        HotspotComplexity::Own => metrics.cyclomatic.cyclomatic(),
    };
    Some(count as f64)
}

/// The complexity figure a file's hotspot is scored from — the figure
/// `bca report --vcs` joins and the file-level block carries: the subtree
/// cyclomatic sum, or `None` when cyclomatic was not computed for the
/// space, so a consumer that only has the figure (not the space) still
/// cannot score from an uncomputed one.
#[must_use]
pub fn file_complexity(metrics: &CodeMetrics) -> Option<f64> {
    complexity(metrics, HotspotComplexity::SubtreeSum)
}

/// Attach `stat` to `space` as its `metrics.vcs`, with `hotspot_score`
/// scored from the `figure` (complexity × recent churn).
///
/// The score is assigned `Some` iff [`Metric::Cyclomatic`] is in
/// `space.metrics.selected`, and explicitly `None` otherwise: the
/// unselected arm is an assignment, not a skip, so a `stat` that arrives
/// already scored — a [`HistoryIndex::new`] caller may seed it — never
/// leaks a stale value through (#1742).
pub(crate) fn attach_stats(space: &mut FuncSpace, mut stat: Stats, figure: HotspotComplexity) {
    stat.hotspot_score = complexity(&space.metrics, figure)
        .map(|complexity| hotspot::hotspot_score(complexity, stat.churn_recent));
    space.metrics.vcs = Some(stat);
}

/// Attach the file-level block: look `path` up in `index` and attach its
/// [`Stats`] to `space` scored by the subtree cyclomatic sum (the
/// [`file_complexity`] figure, so `None` when cyclomatic was not computed).
/// Returns whether a block was attached; `false` means the file is
/// untracked or outside the index's work tree, and `space` is untouched.
///
/// `path` is canonicalized first, unconditionally. [`HistoryIndex::get_for_path`]
/// is a lexical strip against the canonical workdir, and a tracked path
/// may itself be a symlink to another tracked file whose history differs
/// — a lexical-first lookup attached the link's own history to bytes that
/// came from the target (#1764). A path that cannot be canonicalized is
/// looked up as spelled.
pub fn attach_file_history(space: &mut FuncSpace, path: &Path, index: &HistoryIndex) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(stat) = index.get_for_path(&canonical) else {
        return false;
    };
    attach_stats(space, stat.clone(), HotspotComplexity::SubtreeSum);
    true
}

/// Blame `path` once through `session` and attach one block, scored by
/// the descendant's *own* cyclomatic (`None` when cyclomatic was not
/// computed), to every descendant of `space` in pre-order (issue #329).
/// The root — the file space, whose block
/// [`attach_file_history`] owns — is left untouched. A `space` with no
/// descendants is a no-op that never blames.
///
/// # Errors
///
/// The blame's error — an untracked file, a path outside the work tree, a
/// backend failure — is returned with no descendant touched, so each
/// front end decides whether to warn or skip silently; neither aborts
/// the walk on one unblameable file.
pub fn attach_per_function_history(
    space: &mut FuncSpace,
    path: &Path,
    session: &mut BlameSession,
) -> Result<(), Error> {
    let mut spans = Vec::new();
    collect_child_spans(space, &mut spans);
    if spans.is_empty() {
        return Ok(());
    }
    let stats = session.per_function(path, &spans)?;
    // `per_function` returns exactly one `Stats` per span, and
    // `assign_child_stats` replays the identical pre-order, so the
    // iterator must be fully consumed; a leftover means the two
    // traversals drifted out of lockstep (a bug to catch in tests).
    debug_assert_eq!(stats.len(), spans.len());
    let mut stats = stats.into_iter();
    assign_child_stats(space, &mut stats);
    debug_assert!(
        stats.next().is_none(),
        "per-function stats outnumbered the spaces they attach to"
    );
    Ok(())
}

/// Collect the 1-based inclusive line span of every descendant space, in
/// pre-order. Saturates a span line past `u32::MAX` (no real source file
/// reaches that line count) rather than wrapping.
///
/// Walks with an explicit stack of child iterators rather than recursing:
/// this runs before serialization, which is where the space depth is
/// bounded (`MAX_SPACE_SERIALIZE_DEPTH`), so a recursive walk would
/// overflow the thread stack — an abort, not a reportable error — on
/// nesting the serializer would have refused with its error (#1770).
fn collect_child_spans(space: &FuncSpace, out: &mut Vec<LineSpan>) {
    let mut pending = vec![space.spaces.iter()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        let start = u32::try_from(child.start_line).unwrap_or(u32::MAX);
        let end = u32::try_from(child.end_line).unwrap_or(u32::MAX);
        out.push(LineSpan::new(start, end));
        pending.push(child.spaces.iter());
    }
}

/// Replay the [`collect_child_spans`] pre-order, attaching one blame
/// [`Stats`] to each descendant space scored by its *own* cyclomatic.
/// Walks iteratively for the reason [`collect_child_spans`] gives.
fn assign_child_stats(space: &mut FuncSpace, stats: &mut impl Iterator<Item = Stats>) {
    let mut pending = vec![space.spaces.iter_mut()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if let Some(stat) = stats.next() {
            attach_stats(child, stat, HotspotComplexity::Own);
        }
        pending.push(child.spaces.iter_mut());
    }
}

#[cfg(test)]
#[path = "attach_tests.rs"]
mod tests;
