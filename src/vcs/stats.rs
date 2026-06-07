// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// 20-field Stats assembly in `finalize`), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! Per-file change-history statistics: the public [`Stats`] output and
//! the internal [`Accumulator`] that the backend feeds during a walk.

use std::collections::{HashMap, HashSet};

use super::classify::Classification;
use super::identity::AuthorId;
use super::options::{Options, SECONDS_PER_DAY};
use super::score::{self, RISK_SCORE_VERSION, ScoreInput};

/// Output-shape version for the `vcs` block. Bump on any change to the
/// serialized field set.
pub const VCS_SCHEMA_VERSION: u32 = 1;

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
    /// Per-identity edit credits in the long window (ownership + count).
    author_edits_long: HashMap<AuthorId, u32>,
    /// Identities credited within the recent window (count only).
    authors_recent: HashSet<AuthorId>,
    bug_fix_commits: u32,
    security_fix_commits: u32,
    revert_commits: u32,
    oldest_touch: Option<i64>,
    newest_touch: Option<i64>,
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
        self.commits_long += 1;
        self.churn_long += change.churn;
        self.bug_fix_commits += u32::from(change.class.bug_fix);
        self.security_fix_commits += u32::from(change.class.security_fix);
        self.revert_commits += u32::from(change.class.revert);
        for id in change.authors {
            // Avoid cloning the identity on the common repeat-author path;
            // only a first-seen author allocates a map key.
            if let Some(count) = self.author_edits_long.get_mut(id) {
                *count += 1;
            } else {
                self.author_edits_long.insert(id.clone(), 1);
            }
        }
        self.oldest_touch = Some(match self.oldest_touch {
            Some(t) => t.min(change.commit_time),
            None => change.commit_time,
        });
        self.newest_touch = Some(match self.newest_touch {
            Some(t) => t.max(change.commit_time),
            None => change.commit_time,
        });
        if change.in_recent {
            self.commits_recent += 1;
            self.churn_recent += change.churn;
            self.authors_recent.extend(change.authors.iter().cloned());
        }
    }

    /// Collapse the accumulator into the serializable [`Stats`].
    ///
    /// `now` is the reference timestamp (wall clock or `--as-of`). The
    /// resulting risk score uses the weighted formula; percentile
    /// re-ranking is a whole-set pass applied later by the backend.
    #[must_use]
    pub fn finalize(&self, now: i64, options: &Options) -> Stats {
        let long_window_days = options.long_window_days();
        let authors_long = u32::try_from(self.author_edits_long.len()).unwrap_or(u32::MAX);
        let authors_recent = u32::try_from(self.authors_recent.len()).unwrap_or(u32::MAX);

        let total_edits: u32 = self.author_edits_long.values().copied().sum();
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
        });

        let author_ids = options.emit_author_details.then(|| {
            let mut ids: Vec<String> = self
                .author_edits_long
                .keys()
                .map(AuthorId::hashed)
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
