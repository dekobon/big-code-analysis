// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// Trend accessors + delta projection + validation, each with its own
// early returns / `?`), not per-function logic complexity
// (cognitive/cyclomatic stay enforced) — mirrors the sibling vcs modules.

//! Historical metric trend: the change-history (VCS) metrics computed at
//! several points in time, so a consumer can see whether a file's risk is
//! improving or degrading rather than only its risk *now* (issue #333).
//!
//! This module is backend-agnostic. It owns the time-point math
//! ([`timestamps`]), the parameter validation ([`validate_points`]), the
//! assembled [`Trend`] container, and the improving/regressing
//! [`delta summary`](Trend::deltas). The actual repeated history walks
//! live in the backend (`git::build_trend`), which resolves the
//! repository tip at each point in time and folds the per-snapshot
//! [`Stats`] into a [`Trend`] via [`Trend::from_snapshots`].

use std::collections::HashMap;
use std::path::PathBuf;

use super::error::Error;
use super::stats::Stats;

/// Output-shape version for the trend document. Bumped on any
/// shape-breaking change to the serialized time series so consumers can
/// detect an incompatible schema (separate from the per-file
/// [`VCS_SCHEMA_VERSION`](super::stats::VCS_SCHEMA_VERSION), which
/// versions each point's metric block).
pub const TREND_SCHEMA_VERSION: u32 = 1;

/// Minimum number of time points. A trend needs at least two snapshots to
/// express a direction; a single point is just `bca vcs --as-of`.
pub const MIN_TREND_POINTS: usize = 2;

/// Maximum number of time points. Each point is a full history walk, so
/// the count is capped to bound the worst-case runtime on deep histories
/// (issue #333's "very deep histories: cap point count"). 120 points is a
/// decade of monthly snapshots — far beyond any practical trend chart.
pub const MAX_TREND_POINTS: usize = 120;

/// Validate a requested point count against the [`MIN_TREND_POINTS`] /
/// [`MAX_TREND_POINTS`] bounds. The single source of truth shared by
/// every front end so the accepted range cannot drift between them.
///
/// # Errors
///
/// Returns [`Error::InvalidTrend`] when `points` is below the minimum or
/// above the maximum.
pub fn validate_points(points: usize) -> Result<usize, Error> {
    if points < MIN_TREND_POINTS {
        return Err(Error::InvalidTrend(format!(
            "at least {MIN_TREND_POINTS} points are required for a trend (got {points})"
        )));
    }
    if points > MAX_TREND_POINTS {
        return Err(Error::InvalidTrend(format!(
            "at most {MAX_TREND_POINTS} points are supported (got {points})"
        )));
    }
    Ok(points)
}

/// The `points` Unix-second timestamps a trend samples, evenly spaced
/// across `span_secs` and ending at `end` (the most recent point is
/// exactly `end`, the oldest is `end - span_secs`). Returned oldest-first.
///
/// Integer division places the interior points; the two endpoints are
/// exact regardless of rounding. `points` is assumed already validated by
/// [`validate_points`] (`>= MIN_TREND_POINTS`), so the `points - 1`
/// divisor is always positive.
#[must_use]
pub fn timestamps(end: i64, span_secs: i64, points: usize) -> Vec<i64> {
    // Defensive: a single (or zero) point degenerates to just the
    // endpoint, side-stepping a divide-by-zero even though the public
    // entry points validate `points >= 2` first.
    if points <= 1 {
        return vec![end];
    }
    let start = end.saturating_sub(span_secs);
    // `points` is small (validated `<= MAX_TREND_POINTS`); `try_from`
    // keeps the conversion total and lint-clean without an `as` wrap.
    let divisor = i64::try_from(points - 1).unwrap_or(i64::MAX);
    (0..points)
        .map(|i| {
            // `span_secs * i / divisor`: 0 at i=0 (→ start) and span_secs
            // at i=divisor (→ end), so both endpoints are hit exactly.
            let i = i64::try_from(i).unwrap_or(i64::MAX);
            start + span_secs.saturating_mul(i) / divisor
        })
        .collect()
}

/// The change-history metrics of a set of files sampled at several points
/// in time (issue #333).
///
/// Each file maps to a vector aligned 1:1 with [`as_of_points`](Self::as_of_points):
/// element `i` is `Some(stats)` when the file existed and was tracked at
/// point `i`, or `None` when it did not exist yet (or was untracked /
/// binary) at that point.
///
/// # Rename limitation
///
/// Each snapshot keys files by their path *at that snapshot's tip*, with
/// renames followed only *within* that snapshot's walk. A file renamed
/// *between* two sampled points therefore appears as two separate path
/// series (its old name, then its new name), not one continuous series.
/// Cross-snapshot rename stitching is deferred; document this for
/// consumers reading the series.
#[derive(Clone, Debug)]
pub struct Trend {
    as_of_points: Vec<i64>,
    files: HashMap<PathBuf, Vec<Option<Stats>>>,
    long_window_days: u32,
    recent_window_days: u32,
    truncated_shallow_clone: bool,
}

impl Trend {
    /// Assemble a [`Trend`] from one [`HistoryIndex`](super::HistoryIndex)
    /// per time point.
    ///
    /// `as_of_points` are the sampled timestamps (oldest-first).
    /// `per_point` is the matching per-file `Stats` map for each point, in
    /// the same order; a point that resolved to no commit (the repository
    /// did not exist yet) contributes an empty map, leaving every file
    /// `None` there. The two slices must be the same length — a backend
    /// invariant, so a mismatch is a programming error rather than a
    /// user-facing one.
    #[must_use]
    pub fn from_snapshots(
        as_of_points: Vec<i64>,
        per_point: Vec<HashMap<PathBuf, Stats>>,
        long_window_days: u32,
        recent_window_days: u32,
        truncated_shallow_clone: bool,
    ) -> Self {
        let n = as_of_points.len();
        // Backend invariant: one snapshot map per sampled point.
        debug_assert_eq!(per_point.len(), n, "one snapshot per as-of point");
        let mut files: HashMap<PathBuf, Vec<Option<Stats>>> = HashMap::new();
        for (i, snapshot) in per_point.into_iter().enumerate().take(n) {
            for (path, stats) in snapshot {
                // `i < n` (enumerate over a `take(n)`) and the seeded
                // vector is length `n`, so the slot always exists.
                if let Some(slot) = files
                    .entry(path)
                    .or_insert_with(|| vec![None; n])
                    .get_mut(i)
                {
                    *slot = Some(stats);
                }
            }
        }
        Self {
            as_of_points,
            files,
            long_window_days,
            recent_window_days,
            truncated_shallow_clone,
        }
    }

    /// The sampled timestamps, oldest-first, aligned 1:1 with each file's
    /// point vector.
    #[must_use]
    pub fn as_of_points(&self) -> &[i64] {
        &self.as_of_points
    }

    /// Iterate `(repo-relative path, per-point stats)` pairs. Each inner
    /// slice is aligned with [`as_of_points`](Self::as_of_points);
    /// elements are `None` where the file did not exist.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &[Option<Stats>])> {
        self.files.iter().map(|(path, points)| (path, &points[..]))
    }

    /// Number of files tracked across the whole trend (the union over all
    /// points).
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the trend tracked no files at any point.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Long window length, in days (constant across all points).
    #[must_use]
    pub fn long_window_days(&self) -> u32 {
        self.long_window_days
    }

    /// Recent window length, in days (constant across all points).
    #[must_use]
    pub fn recent_window_days(&self) -> u32 {
        self.recent_window_days
    }

    /// `true` when any sampled snapshot came from a shallow clone, so the
    /// per-point counts are lower bounds.
    #[must_use]
    pub fn truncated_shallow_clone(&self) -> bool {
        self.truncated_shallow_clone
    }

    /// The most-improved and most-regressed files by change in
    /// `risk_score` between each file's earliest and latest *present*
    /// points.
    ///
    /// A file is only eligible when it is present at two or more points
    /// (otherwise there is no direction to report). `delta = last_risk -
    /// first_risk`: negative means risk fell (improved), positive means it
    /// rose (regressed). `top` truncates each list (`0` keeps all). Ties
    /// break on the file path so the output is deterministic.
    #[must_use]
    pub fn deltas(&self, top: usize) -> TrendDeltas {
        let mut improved: Vec<TrendDelta> = Vec::new();
        let mut regressed: Vec<TrendDelta> = Vec::new();
        for (path, points) in &self.files {
            let Some(delta) = self.file_delta(path, points) else {
                continue;
            };
            if delta.delta < 0.0 {
                improved.push(delta);
            } else if delta.delta > 0.0 {
                regressed.push(delta);
            }
            // A zero delta is neither improving nor regressing; omit it.
        }
        // Improved: most-negative first. Regressed: most-positive first.
        improved.sort_by(|a, b| {
            a.delta
                .partial_cmp(&b.delta)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        regressed.sort_by(|a, b| {
            b.delta
                .partial_cmp(&a.delta)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        if top > 0 {
            improved.truncate(top);
            regressed.truncate(top);
        }
        TrendDeltas {
            improved,
            regressed,
        }
    }

    /// Compute one file's risk-score delta between its earliest and latest
    /// present points, or `None` when it is present at fewer than two
    /// points.
    fn file_delta(&self, path: &std::path::Path, points: &[Option<Stats>]) -> Option<TrendDelta> {
        let mut present = points
            .iter()
            .enumerate()
            .filter_map(|(i, stats)| stats.as_ref().map(|s| (i, s)));
        let (first_i, first) = present.next()?;
        // `next_back` (not `last`) avoids re-walking the whole iterator;
        // it yields `None` when only the first point was present.
        let (last_i, last) = present.next_back()?;
        Some(TrendDelta {
            path: path.to_path_buf(),
            first_as_of: self.as_of_points[first_i],
            last_as_of: self.as_of_points[last_i],
            first_risk_score: first.risk_score,
            last_risk_score: last.risk_score,
            delta: last.risk_score - first.risk_score,
        })
    }
}

/// One file's risk-score movement across the trend (see
/// [`Trend::deltas`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TrendDelta {
    /// Repository-relative path.
    pub path: PathBuf,
    /// Timestamp of the file's earliest present point.
    pub first_as_of: i64,
    /// Timestamp of the file's latest present point.
    pub last_as_of: i64,
    /// `risk_score` at the earliest present point.
    pub first_risk_score: f64,
    /// `risk_score` at the latest present point.
    pub last_risk_score: f64,
    /// `last_risk_score - first_risk_score`; negative = improved.
    pub delta: f64,
}

/// The improving / regressing split returned by [`Trend::deltas`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrendDeltas {
    /// Files whose risk fell, most-improved first.
    pub improved: Vec<TrendDelta>,
    /// Files whose risk rose, most-regressed first.
    pub regressed: Vec<TrendDelta>,
}

#[cfg(test)]
#[path = "trend_tests.rs"]
mod tests;
