// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// 20-field Stats assembly in `finalize`), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! Per-file change-history statistics: the public [`Stats`] output and
//! the internal [`Accumulator`] that the backend feeds during a walk.

use std::collections::{HashMap, HashSet};

use super::bus_factor::AuthorContribution;
use super::classify::Classification;
use super::identity::AuthorId;
use super::options::{Options, SECONDS_PER_DAY};
use super::score::{self, RISK_SCORE_VERSION, ScoreInput};

/// Output-shape version for the `vcs` block. Bump on any change to the
/// serialized field set. `2` added the change- and co-change-entropy
/// fields (issue #330).
pub const VCS_SCHEMA_VERSION: u32 = 2;

/// Per-file change-history metrics.
///
/// One record per tracked, non-binary, non-symlink file present at the
/// target ref. A tracked file with no activity in the window is emitted
/// with zero counts — distinct from an untracked file, which has no
/// `vcs` block at all.
///
/// All scores are *ordinal*: rank files by them, do not read absolute
/// magnitudes. The serialized shape lives in [`crate::wire::Vcs`]; this
/// struct is the compute-side source the wire form is projected from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Output-shape version ([`VCS_SCHEMA_VERSION`]).
    pub vcs_schema_version: u32,
    /// Composite-formula version ([`RISK_SCORE_VERSION`]).
    pub risk_score_version: u32,
    /// Long window length, in days.
    pub long_window_days: u32,
    /// Recent window length, in days.
    pub recent_window_days: u32,
    /// Distinct commits touching the file in the long window.
    pub commits_long: u32,
    /// Distinct commits touching the file in the recent window.
    pub commits_recent: u32,
    /// Σ(added + deleted) lines in the long window.
    pub churn_long: u64,
    /// Σ(added + deleted) lines in the recent window.
    pub churn_recent: u64,
    /// Distinct canonical author identities in the long window.
    pub authors_long: u32,
    /// Distinct canonical author identities in the recent window.
    pub authors_recent: u32,
    /// Top-author share of edits in the long window, in `[0, 1]`.
    pub ownership_top_share: f64,
    /// `commits_recent / commits_long`, clamped to `[0, 1]`.
    pub burst: f64,
    /// Long-window commits whose message matched a bug-fix keyword.
    pub bug_fix_commits: u32,
    /// Long-window commits whose message matched a security keyword.
    pub security_fix_commits: u32,
    /// Long-window commits whose subject is a revert / rollback.
    pub revert_commits: u32,
    /// Days since the file's first in-window commit (capped at window).
    pub age_days: u32,
    /// Days since the file's most recent in-window commit.
    pub last_modified_days: u32,
    /// Change entropy over the long window (Hassan 2009 History
    /// Complexity Metric, in bits): the file's accumulated share
    /// `Σ pᵢ·Hᵢ` of the churn-distribution entropy of every commit it
    /// took part in. Higher = the file participates in more scattered,
    /// distributed changes. `0.0` means it only ever changed alone.
    pub change_entropy_long: f64,
    /// Change entropy restricted to the recent window.
    pub change_entropy_recent: f64,
    /// Co-change graph entropy over the long window (arXiv 2504.18511,
    /// 2025, in bits): the Shannon entropy of the file's co-change
    /// edge-weight distribution. Higher = its changes ripple across many
    /// different partners. `0.0` is *computed*, not missing — the file
    /// has no co-change neighbours (only single-file commits).
    pub cochange_entropy_long: f64,
    /// Co-change graph entropy restricted to the recent window.
    pub cochange_entropy_recent: f64,
    /// Composite risk score (weighted or percentile, per options).
    pub risk_score: f64,
    /// Complexity × recent-churn hotspot score; `Some` only when AST
    /// metrics were computed alongside the history.
    pub hotspot_score: Option<f64>,
    /// SHA-256-hashed canonical author identities, sorted; `Some` only
    /// under `--emit-author-details`.
    pub author_ids: Option<Vec<String>>,
}

/// Mutable per-file accumulator threaded through the history walk.
///
/// Holds the raw sets and counters; [`Accumulator::finalize`] collapses
/// them into a [`Stats`]. Author *edits* credit every participant of a
/// touching commit (author plus `Co-authored-by` trailers) one edit, so
/// the distinct-author count and the ownership ratio both account for
/// co-authorship.
#[derive(Clone, Debug, Default)]
pub struct Accumulator {
    sloc: u64,
    commits_long: u32,
    commits_recent: u32,
    churn_long: u64,
    churn_recent: u64,
    /// Accumulated change-entropy contribution `Σ pᵢ·Hᵢ` over the long
    /// window; the recent counterpart sums only recent-window commits.
    change_entropy_long: f64,
    change_entropy_recent: f64,
    /// Per-identity edit credits in the long window (ownership + count).
    author_edits_long: HashMap<AuthorId, u32>,
    /// Identities credited within the recent window (count only).
    authors_recent: HashSet<AuthorId>,
    bug_fix_commits: u32,
    security_fix_commits: u32,
    revert_commits: u32,
    oldest_touch: Option<i64>,
    newest_touch: Option<i64>,
    /// Participants of the file's earliest observed in-window commit (the
    /// `FirstAuthorship` input for the bus factor, issue #332). Tracked
    /// against the minimum timestamp rather than walk order, so an
    /// out-of-order (clock-skewed) commit cannot misattribute creation.
    first_authors: Vec<AuthorId>,
}

/// One commit's effect on one file, as handed to [`Accumulator::record`].
pub struct ChangeRecord<'a> {
    /// Added + deleted lines this commit applied to the file.
    pub churn: u64,
    /// Commit timestamp (Unix seconds, clamped to `now` for skew).
    pub commit_time: i64,
    /// Whether the commit falls inside the recent window.
    pub in_recent: bool,
    /// Keyword classification of the commit message.
    pub class: Classification,
    /// Non-empty, bot-filtered participant identities for this commit.
    pub authors: &'a [AuthorId],
    /// This commit's change-entropy contribution to *this* file: the
    /// file's churn share of the commit times the commit's churn-
    /// distribution entropy (`pᵢ·H`), in bits. Zero for a single-file
    /// commit (`H = 0`) or a zero-churn touch (`pᵢ = 0`). The backend
    /// computes it once the whole commit's churn distribution is known.
    pub change_entropy: f64,
}

impl Accumulator {
    /// Start an accumulator for a file of `sloc` source lines.
    #[must_use]
    pub fn new(sloc: u64) -> Self {
        Self {
            sloc,
            ..Self::default()
        }
    }

    /// Fold one commit's effect on this file into the running totals.
    pub fn record(&mut self, change: &ChangeRecord<'_>) {
        // `saturating_add` everywhere, matching every other vcs accumulator:
        // a real history never approaches these bounds, but a debug build's
        // overflow check would panic, and the crate forbids panics in
        // non-test code (AGENTS.md).
        self.commits_long = self.commits_long.saturating_add(1);
        self.churn_long = self.churn_long.saturating_add(change.churn);
        self.change_entropy_long += change.change_entropy;
        self.bug_fix_commits = self
            .bug_fix_commits
            .saturating_add(u32::from(change.class.bug_fix));
        self.security_fix_commits = self
            .security_fix_commits
            .saturating_add(u32::from(change.class.security_fix));
        self.revert_commits = self
            .revert_commits
            .saturating_add(u32::from(change.class.revert));
        for id in change.authors {
            // Avoid cloning the identity on the common repeat-author path;
            // only a first-seen author allocates a map key.
            if let Some(count) = self.author_edits_long.get_mut(id) {
                *count = count.saturating_add(1);
            } else {
                self.author_edits_long.insert(id.clone(), 1);
            }
        }
        // A strictly-earlier commit (re)sets first authorship; tracking
        // the minimum timestamp directly keeps this correct even if the
        // walk visits an older-named or clock-skewed commit out of order.
        let is_new_oldest = self.oldest_touch.is_none_or(|t| change.commit_time < t);
        if is_new_oldest {
            self.oldest_touch = Some(change.commit_time);
            self.first_authors.clear();
            self.first_authors.extend_from_slice(change.authors);
        }
        self.newest_touch = Some(match self.newest_touch {
            Some(t) => t.max(change.commit_time),
            None => change.commit_time,
        });
        if change.in_recent {
            self.commits_recent = self.commits_recent.saturating_add(1);
            self.churn_recent = self.churn_recent.saturating_add(change.churn);
            self.change_entropy_recent += change.change_entropy;
            self.authors_recent.extend(change.authors.iter().cloned());
        }
    }

    /// The per-developer authorship inputs for the bus-factor aggregate
    /// (issue #332), or `None` when the file saw no in-window activity (no
    /// authorship signal, so it is excluded from the bus-factor
    /// denominator).
    ///
    /// `deliveries` is each developer's participation count (`DL`);
    /// accepted-changes (`AC`) is derived per file by the aggregate, and
    /// `first_authorship` flags participants of the earliest observed
    /// commit. Identities are cloned because the aggregate outlives the
    /// accumulator the walk discards.
    #[must_use]
    pub(crate) fn authorship(&self) -> Option<Vec<AuthorContribution>> {
        if self.author_edits_long.is_empty() {
            return None;
        }
        Some(
            self.author_edits_long
                .iter()
                .map(|(author, &deliveries)| AuthorContribution {
                    author: author.clone(),
                    deliveries,
                    first_authorship: self.first_authors.contains(author),
                })
                .collect(),
        )
    }

    /// Collapse the accumulator into the serializable [`Stats`].
    ///
    /// `now` is the reference timestamp (wall clock or `--as-of`).
    /// `cochange_long` / `cochange_recent` are the file's co-change graph
    /// entropies (computed by the backend from the whole-walk graph, which
    /// the per-file accumulator cannot see), folded into the score here so
    /// the weighted formula stays the single risk-score authority. The
    /// resulting risk score uses the weighted formula; percentile
    /// re-ranking is a whole-set pass applied later by the backend.
    #[must_use]
    pub fn finalize(
        &self,
        now: i64,
        options: &Options,
        cochange_long: f64,
        cochange_recent: f64,
    ) -> Stats {
        let long_window_days = options.long_window_days();
        let authors_long = u32::try_from(self.author_edits_long.len()).unwrap_or(u32::MAX);
        let authors_recent = u32::try_from(self.authors_recent.len()).unwrap_or(u32::MAX);

        // The per-author counts are already saturating_add-clamped; the
        // cross-author sum must be too — std `Sum<u32>` panics on overflow
        // in debug, and the vcs crate forbids panics in non-test code.
        let total_edits: u32 = self
            .author_edits_long
            .values()
            .copied()
            .fold(0u32, u32::saturating_add);
        let top_edits = self.author_edits_long.values().copied().max().unwrap_or(0);
        let ownership_top_share = if total_edits > 0 {
            f64::from(top_edits) / f64::from(total_edits)
        } else {
            0.0
        };

        let burst = if self.commits_long > 0 {
            (f64::from(self.commits_recent) / f64::from(self.commits_long)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Age is capped at the long window: the walk only observes
        // in-window commits, so a file older than the window reports
        // the window length rather than its true creation date (true
        // first-commit detection is the per-function/full-history
        // follow-up, #329).
        let age_days = self.oldest_touch.map_or(long_window_days, |t| {
            days_between(t, now).min(long_window_days)
        });
        let last_modified_days = self
            .newest_touch
            .map_or(long_window_days, |t| days_between(t, now));

        let risk_score = score::weighted(&ScoreInput {
            churn_recent: self.churn_recent,
            churn_long: self.churn_long,
            commits_recent: self.commits_recent,
            commits_long: self.commits_long,
            authors_long,
            ownership_top_share,
            bug_fix_commits: self.bug_fix_commits,
            security_fix_commits: self.security_fix_commits,
            sloc: self.sloc,
            age_days,
            recent_window_days: options.recent_window_days(),
            change_entropy_recent: self.change_entropy_recent,
            cochange_entropy_recent: cochange_recent,
        });

        let author_ids = options.emit_author_details.then(|| {
            let key = options.author_hash_key.as_ref();
            let mut ids: Vec<String> = self
                .author_edits_long
                .keys()
                .map(|author| author.emit_hashed(key))
                .collect();
            ids.sort_unstable();
            ids
        });

        Stats {
            vcs_schema_version: VCS_SCHEMA_VERSION,
            risk_score_version: RISK_SCORE_VERSION,
            long_window_days,
            recent_window_days: options.recent_window_days(),
            commits_long: self.commits_long,
            commits_recent: self.commits_recent,
            churn_long: self.churn_long,
            churn_recent: self.churn_recent,
            authors_long,
            authors_recent,
            ownership_top_share,
            burst,
            bug_fix_commits: self.bug_fix_commits,
            security_fix_commits: self.security_fix_commits,
            revert_commits: self.revert_commits,
            age_days,
            last_modified_days,
            change_entropy_long: self.change_entropy_long,
            change_entropy_recent: self.change_entropy_recent,
            cochange_entropy_long: cochange_long,
            cochange_entropy_recent: cochange_recent,
            risk_score,
            hotspot_score: None,
            author_ids,
        }
    }
}

/// Whole days between an earlier timestamp and `now`, clamped at zero so
/// a future-dated commit (clock skew) reads as "today" rather than a
/// negative age.
fn days_between(earlier: i64, now: i64) -> u32 {
    let delta = (now - earlier).max(0);
    u32::try_from(delta / SECONDS_PER_DAY).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
