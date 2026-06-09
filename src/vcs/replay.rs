// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the fold +
// finalize + replay pipeline), not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

//! The per-commit fold shared by the live walk and the cache replay, plus
//! the replay itself (issue #334).
//!
//! Both the live `vcs-git` walk and a cache replay reduce a stream of
//! commits to the same per-file [`Accumulator`]s and co-change
//! [`CochangeGraph`]. Keeping that fold in *one* place — [`fold_commit`] —
//! is what lets a cache hit reproduce a fresh walk bit-for-bit: there is no
//! second implementation to drift. [`replay`] then drives the fold from a
//! cached [`CommitEvent`] log and finalises it at the *current* reference
//! time, so windowing tracks wall-clock `now` rather than freezing at the
//! time the cache was written.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::bus_factor::{self, BusFactor, FileAuthorship};
use super::cache::CommitEvent;
use super::classify::Classification;
use super::entropy::{self, CochangeGraph};
use super::identity::AuthorId;
use super::options::{Options, RiskFormula};
use super::score;
use super::stats::{Accumulator, ChangeRecord, Stats};

/// Per-commit facts shared by every file the commit touched — the single
/// context type threaded through [`fold_commit`] by both the live git walk
/// and the cache replay.
pub(crate) struct FoldContext<'a> {
    /// Commit timestamp in Unix seconds, already clamped to the reference
    /// `now` (a future-dated commit reads as "today").
    pub commit_time: i64,
    /// Whether the commit falls inside the recent window.
    pub in_recent: bool,
    /// Keyword classification of the commit message.
    pub class: Classification,
    /// Non-empty, bot-filtered participant identities for this commit.
    pub authors: &'a [AuthorId],
}

/// Upper bound on rename-alias chain length; far beyond any real rename
/// history, so hitting it implies a cycle in malformed history.
const MAX_ALIAS_DEPTH: usize = 10_000;

/// Follow the rename-alias chain from a historical path to the path the
/// file carries at the target ref. The depth guard defends against a
/// pathological cycle in malformed history.
pub(crate) fn resolve_alias<'a>(
    alias: &'a HashMap<PathBuf, PathBuf>,
    path: &'a Path,
) -> Cow<'a, Path> {
    let mut current: &'a Path = path;
    for _ in 0..MAX_ALIAS_DEPTH {
        match alias.get(current) {
            Some(next) => current = next.as_path(),
            None => break,
        }
    }
    // Always borrowed: both `path` and the alias values outlive `'a`, so
    // no `PathBuf` is allocated unless a caller later needs ownership.
    Cow::Borrowed(current)
}

/// Fold one commit's touched-file list (already alias-resolved to
/// target-ref paths) into the accumulators and the co-change graph.
///
/// The commit's **change entropy** (Hassan 2009) is the Shannon entropy
/// of its churn distribution across `touched`; each file is credited its
/// churn share of that entropy (`pᵢ·H`). The distribution spans *every*
/// touched text file — including ones absent at the target ref — so a
/// dropped file does not inflate the survivors' shares. The **co-change
/// graph** is fed the same full path list so edges reflect what truly
/// changed together.
///
/// The `churn as f64` casts are exact for any realistic line count (well
/// under 2^53), and the entropy share is ordinal, so the precision lint
/// is allowed for the whole fold.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn fold_commit(
    touched: &[(PathBuf, u64)],
    options: &Options,
    accumulators: &mut HashMap<PathBuf, Accumulator>,
    graph: &mut CochangeGraph,
    ctx: &FoldContext<'_>,
) {
    // Churn distribution of the commit, fed to the entropy core directly
    // (no intermediate Vec). The total is summed once for the per-file
    // share denominator below.
    let churn_bits = || touched.iter().map(|&(_, churn)| churn as f64);
    let total: f64 = churn_bits().sum();
    let commit_entropy = entropy::shannon_entropy(churn_bits());

    for (path, churn) in touched {
        // A file's share of the commit's entropy; zero when the commit
        // has no churn (mode-only change) or this file added no lines.
        let change_entropy = if total > 0.0 {
            (*churn as f64 / total) * commit_entropy
        } else {
            0.0
        };
        let accumulator = match accumulators.get_mut(path) {
            Some(acc) => acc,
            None if options.include_deleted => accumulators
                .entry(path.clone())
                .or_insert_with(|| Accumulator::new(0)),
            // A file not present at the target ref and not opted in via
            // --include-deleted: skip recording, but it still counted
            // toward the entropy distribution and the co-change graph
            // above/below.
            None => continue,
        };
        accumulator.record(&ChangeRecord {
            churn: *churn,
            commit_time: ctx.commit_time,
            in_recent: ctx.in_recent,
            class: ctx.class,
            authors: ctx.authors,
            change_entropy,
        });
    }

    // Co-change edges over the full touched set (cap enforced inside).
    // The graph borrows each path; the interner clones only on a path's
    // first appearance, so no per-commit PathBuf clone happens here.
    let paths: Vec<&Path> = touched.iter().map(|(path, _)| path.as_path()).collect();
    graph.record_commit(&paths, ctx.in_recent);
}

/// The finalized output of a replay: per-file stats plus the optional
/// bus-factor aggregate. The backend wraps these into a
/// [`HistoryIndex`](crate::vcs::HistoryIndex) with the working-tree
/// metadata it alone holds.
pub(crate) struct ReplayOutput {
    /// Per-file change-history statistics, keyed by repo-relative path.
    pub files: HashMap<PathBuf, Stats>,
    /// Directory- / repo-level bus factor, when opted in via
    /// [`Options::compute_bus_factor`].
    pub bus_factor: Option<BusFactor>,
}

/// Replay a cached [`CommitEvent`] log into per-file [`Stats`](super::stats::Stats) at the
/// reference time `now`.
///
/// `seed_files` maps each file present at the target ref to its SLOC; the
/// accumulators are pre-seeded from it so a tracked file with no in-window
/// activity still emits a zeroed record. Events are folded newest-first
/// (the order they were walked and stored), so the floating-point entropy
/// sums match a fresh walk exactly. Commits are clamped to `now` and
/// pruned to the current long window here — *not* at cache-write time — so
/// the windowing reflects the moment of this call.
pub(crate) fn replay(
    seed_files: HashMap<PathBuf, u64>,
    events: &[CommitEvent],
    options: &Options,
    now: i64,
) -> ReplayOutput {
    let mut accumulators: HashMap<PathBuf, Accumulator> = seed_files
        .into_iter()
        .map(|(path, sloc)| (path, Accumulator::new(sloc)))
        .collect();
    let long_boundary = now - options.long_window_secs;
    let recent_boundary = now - options.recent_window_secs;
    let mut graph = CochangeGraph::new();
    // Rebuilt newest-first across the whole log, exactly as the live walk
    // builds it — so a rename in a newer (possibly incremental) commit
    // re-homes edits in older cached commits, matching production.
    let mut alias: HashMap<PathBuf, PathBuf> = HashMap::new();

    for event in events {
        // Clamp to the *current* now (future-dated commit → "today"),
        // then prune anything now outside the long window.
        let commit_time = event.time.min(now);
        if commit_time < long_boundary {
            continue;
        }
        for (source, dest) in &event.renames {
            alias.insert(source.clone(), dest.clone());
        }
        let authors = event.author_ids();
        let touched: Vec<(PathBuf, u64)> = event
            .touched
            .iter()
            .map(|(location, churn)| (resolve_alias(&alias, location).into_owned(), *churn))
            .collect();
        let ctx = FoldContext {
            commit_time,
            in_recent: commit_time >= recent_boundary,
            class: event.classification(),
            authors: &authors,
        };
        fold_commit(&touched, options, &mut accumulators, &mut graph, &ctx);
    }

    let bus_factor = options
        .compute_bus_factor
        .then(|| bus_factor_aggregate(&accumulators, options))
        .flatten();
    let files = finalize(accumulators, &graph, options, now);
    ReplayOutput { files, bus_factor }
}

/// Build the directory- / repo-level bus-factor aggregate from the
/// per-file accumulators (issue #332). Files with no in-window authorship
/// contribute no signal and are excluded from the denominator; when *no*
/// file has any (an empty repo, or one whose only commits were bot-only or
/// out-of-window) the aggregate is `None` so front ends omit the
/// `vcs_aggregate` block rather than emit a meaningless "0 over 0 files".
pub(crate) fn bus_factor_aggregate(
    accumulators: &HashMap<PathBuf, Accumulator>,
    options: &Options,
) -> Option<BusFactor> {
    let authorship: Vec<FileAuthorship> = accumulators
        .iter()
        .filter_map(|(path, acc)| {
            acc.authorship().map(|contributions| FileAuthorship {
                path: path.clone(),
                contributions,
            })
        })
        .collect();
    if authorship.is_empty() {
        return None;
    }
    Some(bus_factor::compute(
        &authorship,
        options.bus_factor_threshold,
        options.emit_author_details,
    ))
}

/// Finalise every accumulator into a [`Stats`](super::stats::Stats) record, joining in each
/// file's co-change graph entropy (the one signal the per-file
/// accumulator cannot compute alone) and applying the percentile
/// re-ranking pass when that formula is selected.
pub(crate) fn finalize(
    accumulators: HashMap<PathBuf, Accumulator>,
    graph: &CochangeGraph,
    options: &Options,
    now: i64,
) -> HashMap<PathBuf, Stats> {
    let (paths, mut stats): (Vec<PathBuf>, Vec<Stats>) = accumulators
        .into_iter()
        .map(|(path, acc)| {
            let (cochange_long, cochange_recent) = graph.entropy(&path);
            let stats = acc.finalize(now, options, cochange_long, cochange_recent);
            (path, stats)
        })
        .unzip();

    if options.risk_formula == RiskFormula::Percentile {
        score::apply_percentile(&mut stats);
    }

    paths.into_iter().zip(stats).collect()
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
