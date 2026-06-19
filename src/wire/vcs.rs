//! VCS (change-history) wire structs and their projections — the
//! `vcs-git`-gated arm of the wire shape (see the parent [`super`]
//! module doc for the single-source-of-truth rationale).

use super::*;

/// Wire form of [`crate::vcs::Stats`] — per-file change-history metrics.
///
/// Always-slim by design (issue #635): the row carries only the metrics
/// that vary per file. The four constant stamps that hold across every
/// row of a single response — `vcs_schema_version`, `risk_score_version`,
/// `long_window_days`, `recent_window_days` — live exactly once in the
/// enclosing envelope (`bca vcs`'s `Report`, `POST /vcs`'s response, and
/// the `/vcs/trend` document), never repeated per row or per trend point.
///
/// The remaining field names are the nested `vcs` object's output keys
/// verbatim (issue #684). All scores are ordinal. `hotspot_score` and
/// `author_ids` are elided when absent (no AST metrics alongside, and
/// `--emit-author-details` off, respectively). Gated behind the
/// `vcs-git` backend feature.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Vcs {
    /// Distinct commits in the long window.
    pub commits_long: u32,
    /// Distinct commits in the recent window.
    pub commits_recent: u32,
    /// Σ(added + deleted) lines in the long window.
    pub churn_long: u64,
    /// Σ(added + deleted) lines in the recent window.
    pub churn_recent: u64,
    /// Distinct authors in the long window.
    pub authors_long: u32,
    /// Distinct authors in the recent window.
    pub authors_recent: u32,
    /// Top-author edit share in `[0, 1]`.
    #[serde(default = "nan_default", with = "non_finite")]
    pub ownership_top_share: f64,
    /// `commits_recent / commits_long`, clamped to `[0, 1]`.
    #[serde(default = "nan_default", with = "non_finite")]
    pub burst: f64,
    /// Long-window bug-fix commit count.
    pub bug_fix_commits: u32,
    /// Long-window security-fix commit count.
    pub security_fix_commits: u32,
    /// Long-window revert commit count.
    pub revert_commits: u32,
    /// Days since the file's first in-window commit (capped at window).
    pub age_days: u32,
    /// Days since the file's most recent in-window commit.
    pub last_modified_days: u32,
    /// Change entropy (bits) over the long window — Hassan 2009 History
    /// Complexity Metric; higher means more scattered changes.
    #[serde(default = "nan_default", with = "non_finite")]
    pub change_entropy_long: f64,
    /// Change entropy (bits) over the recent window.
    #[serde(default = "nan_default", with = "non_finite")]
    pub change_entropy_recent: f64,
    /// Co-change graph entropy (bits) over the long window — arXiv
    /// 2504.18511; higher means changes ripple across more partners.
    /// `0.0` is computed (no co-changes), not missing.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cochange_entropy_long: f64,
    /// Co-change graph entropy (bits) over the recent window.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cochange_entropy_recent: f64,
    /// Ordinal composite risk score.
    #[serde(default = "nan_default", with = "non_finite")]
    pub risk_score: f64,
    /// Complexity × recent-churn hotspot score, when AST metrics were
    /// computed alongside the history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_score: Option<f64>,
    /// SHA-256-hashed canonical author identities, under
    /// `--emit-author-details`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_ids: Option<Vec<String>>,
}

#[cfg(feature = "vcs-git")]
impl From<&crate::vcs::Stats> for Vcs {
    fn from(s: &crate::vcs::Stats) -> Self {
        Self {
            commits_long: s.commits_long,
            commits_recent: s.commits_recent,
            churn_long: s.churn_long,
            churn_recent: s.churn_recent,
            authors_long: s.authors_long,
            authors_recent: s.authors_recent,
            ownership_top_share: s.ownership_top_share,
            burst: s.burst,
            bug_fix_commits: s.bug_fix_commits,
            security_fix_commits: s.security_fix_commits,
            revert_commits: s.revert_commits,
            age_days: s.age_days,
            last_modified_days: s.last_modified_days,
            change_entropy_long: s.change_entropy_long,
            change_entropy_recent: s.change_entropy_recent,
            cochange_entropy_long: s.cochange_entropy_long,
            cochange_entropy_recent: s.cochange_entropy_recent,
            risk_score: s.risk_score,
            hotspot_score: s.hotspot_score,
            author_ids: s.author_ids.clone(),
        }
    }
}

/// Wire form of one ranked file in a [`VcsReport`]: its repository-
/// relative path plus the always-slim [`Vcs`] block, nested under a
/// `vcs` key like every other metric group (issue #684).
// Serialize-only (no `Deserialize`): `VcsAggregate` and its bus-factor
// family are serialize-only upstream, so `VcsReport` cannot round-trip
// through `Deserialize` the way [`Vcs`] / [`VcsTrend`] do. The only
// consumer (`bca vcs` / `POST /vcs` / Python `vcs.rank()`) serializes
// outward; nothing reads a report back in.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VcsReportFile {
    /// Repository-relative path.
    pub path: String,
    /// The file's change-history metrics.
    pub vcs: Vcs,
}

/// Wire form of the file-ranking change-history report (issue #328) —
/// the single serialized shape shared by `bca vcs`, `POST /vcs`, and the
/// Python `vcs.rank()` (#664).
///
/// The four constant stamps (`long_window_days`, `recent_window_days`,
/// `risk_score_version`, `vcs_schema_version`) sit once at the top level
/// rather than per row (issue #635); each `files` row carries only the
/// per-file metrics under a nested `vcs` key (issue #684). `vcs_aggregate`
/// is the directory-/repo-level bus-factor summary, omitted when not
/// computed.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VcsReport {
    /// Long window length, in days (constant across rows).
    pub long_window_days: u32,
    /// Recent window length, in days (constant across rows).
    pub recent_window_days: u32,
    /// Composite-formula version.
    pub risk_score_version: u32,
    /// Per-row metric-block shape version.
    pub vcs_schema_version: u32,
    /// Whether the history came from a shallow clone.
    pub truncated_shallow_clone: bool,
    /// Directory-/repo-level bus-factor aggregate, when computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs_aggregate: Option<crate::vcs::VcsAggregate>,
    /// Files ranked by descending `vcs.risk_score`.
    pub files: Vec<VcsReportFile>,
}

/// Wire form of one sampled point in a historical metric trend (issue
/// #333): the sample timestamp plus the file's VCS block at that moment.
/// `as_of` leads; the metrics sit under a nested `vcs` key (issue #684),
/// the same always-slim [`Vcs`] row every other endpoint emits. The four
/// constant stamps are carried once on the enclosing [`VcsTrend`], never
/// repeated per point (issue #635).
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendPoint {
    /// Unix-second timestamp this point was sampled at.
    pub as_of: i64,
    /// The file's change-history metrics at `as_of`.
    pub vcs: Vcs,
}

/// Wire form of one file's risk-score movement across the trend.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendDelta {
    /// Repository-relative path.
    pub path: String,
    /// Timestamp of the file's earliest present point.
    pub first_as_of: i64,
    /// Timestamp of the file's latest present point.
    pub last_as_of: i64,
    /// `risk_score` at the earliest present point.
    pub first_risk_score: f64,
    /// `risk_score` at the latest present point.
    pub last_risk_score: f64,
    /// `last_risk_score - first_risk_score`; negative means improved.
    pub delta: f64,
}

#[cfg(feature = "vcs-git")]
impl VcsTrendDelta {
    /// Project a compute-side [`crate::vcs::TrendDelta`], dropping a
    /// non-UTF-8 path (which cannot be a JSON key) by returning `None` —
    /// the same path policy the file map uses.
    fn from_delta(d: &crate::vcs::TrendDelta) -> Option<Self> {
        Some(Self {
            path: d.path.to_str()?.to_owned(),
            first_as_of: d.first_as_of,
            last_as_of: d.last_as_of,
            first_risk_score: d.first_risk_score,
            last_risk_score: d.last_risk_score,
            delta: d.delta,
        })
    }
}

/// Wire form of the improving / regressing delta summary.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendDeltas {
    /// Files whose risk fell, most-improved first.
    pub improved: Vec<VcsTrendDelta>,
    /// Files whose risk rose, most-regressed first.
    pub regressed: Vec<VcsTrendDelta>,
}

/// Wire form of a historical metric trend (issue #333) — the single
/// serialized shape shared by `bca vcs trend`, `POST /vcs/trend`, and the
/// Python `vcs_trend()`.
///
/// `as_of_points` lists the sample timestamps oldest-first; every file's
/// array in `files` aligns to it 1:1, with a `null` element where the file
/// did not exist at that point. `files` is keyed by repository-relative
/// path and ordered lexicographically for deterministic output.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrend {
    /// Trend-document shape version
    /// ([`crate::vcs::TREND_SCHEMA_VERSION`]).
    pub trend_schema_version: u32,
    /// Per-point metric-block shape version.
    pub vcs_schema_version: u32,
    /// Composite-formula version.
    pub risk_score_version: u32,
    /// Long window length, in days (constant across points).
    pub long_window_days: u32,
    /// Recent window length, in days (constant across points).
    pub recent_window_days: u32,
    /// Whether any sampled snapshot came from a shallow clone.
    pub truncated_shallow_clone: bool,
    /// Sample timestamps, oldest-first.
    pub as_of_points: Vec<i64>,
    /// Per-file time series; `null` marks a point where the file was
    /// absent.
    pub files: std::collections::BTreeMap<String, Vec<Option<VcsTrendPoint>>>,
    /// The most-improved / most-regressed files by risk delta.
    pub deltas: VcsTrendDeltas,
}

#[cfg(feature = "vcs-git")]
impl VcsTrend {
    /// Project a compute-side [`crate::vcs::Trend`] into the wire shape,
    /// keeping the `top_files` highest-risk files (by their most-recent
    /// present `risk_score`; `0` keeps all) and the `top_deltas`
    /// strongest movers in each delta list.
    #[must_use]
    pub fn from_trend(trend: &crate::vcs::Trend, top_files: usize, top_deltas: usize) -> Self {
        let as_of_points = trend.as_of_points().to_vec();

        // Rank files by most-recent present risk so `top_files` keeps the
        // currently-riskiest. Reuse the shared `rank_by_risk` so the
        // descending-risk + path tie-break and the `top` truncation match
        // `bca vcs` / `POST /vcs` exactly (a non-UTF-8 path sorts as "" and
        // is dropped below).
        let mut ranked: Vec<(&std::path::PathBuf, &[Option<crate::vcs::Stats>], f64)> = trend
            .iter()
            .map(|(path, points)| (path, points, latest_present_risk(points)))
            .collect();
        crate::vcs::rank_by_risk(&mut ranked, top_files, |entry| {
            (entry.0.to_str().unwrap_or(""), entry.2)
        });

        let files = ranked
            .into_iter()
            .filter_map(|(path, points, _)| {
                // A non-UTF-8 path cannot be a JSON object key; drop it,
                // matching the per-file endpoints' policy.
                let key = path.to_str()?.to_owned();
                let series = points
                    .iter()
                    .zip(&as_of_points)
                    .map(|(stats, &as_of)| {
                        stats.as_ref().map(|s| VcsTrendPoint {
                            as_of,
                            vcs: Vcs::from(s),
                        })
                    })
                    .collect();
                Some((key, series))
            })
            .collect();

        let compute_deltas = trend.deltas(top_deltas);
        let deltas = VcsTrendDeltas {
            improved: compute_deltas
                .improved
                .iter()
                .filter_map(VcsTrendDelta::from_delta)
                .collect(),
            regressed: compute_deltas
                .regressed
                .iter()
                .filter_map(VcsTrendDelta::from_delta)
                .collect(),
        };

        Self {
            trend_schema_version: crate::vcs::TREND_SCHEMA_VERSION,
            vcs_schema_version: crate::vcs::stats::VCS_SCHEMA_VERSION,
            risk_score_version: crate::vcs::score::RISK_SCORE_VERSION,
            long_window_days: trend.long_window_days(),
            recent_window_days: trend.recent_window_days(),
            truncated_shallow_clone: trend.truncated_shallow_clone(),
            as_of_points,
            files,
            deltas,
        }
    }
}
