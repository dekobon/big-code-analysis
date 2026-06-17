//! Composite risk-score formulas.
//!
//! Two scores are offered. The default **weighted** score is a
//! log-scaled weighted sum with categorical multiplicative bumps; the
//! **percentile** score re-ranks every signal to its position within
//! the analyzed set and averages. Both are *ordinal*: only relative
//! ranks carry meaning, never the absolute magnitude.
//!
//! # Literature
//!
//! The term weights and thresholds are grounded in the defect- and
//! vulnerability-prediction literature synthesised on issue #328:
//!
//! - Recent churn and recent commit count carry the highest weight —
//!   Nagappan & Ball's relative-churn measures and the just-in-time
//!   defect-prediction line both find recent change activity the
//!   strongest single signal (Firefox `LinesChanged` PD 85,
//!   `NumChanges` PD 86).
//! - The author factor is multiplied by an ownership-dilution term
//!   `(1 - ownership_top_share)`: diffuse ownership predicts defects
//!   (Avelino DoA / truck-factor; Bird et al.).
//! - Categorical developer-count bumps encode the RHEL4 finding that
//!   files touched by ≥9 developers were ~16× more likely to harbour a
//!   vulnerability, with a softer bump at the 6-developer mark.
//! - A new-file bump reflects the Chromium observation that newly
//!   added features carry elevated risk.
//! - Bug-fix and security-fix commit counts feed a log-scaled additive
//!   term, security fixes double-weighted (Sentence-Level VFC studies;
//!   PySecDB).
//! - File size enters as `ln1p(sloc)^2 / 100` with a full coefficient of
//!   1.0, so it is a meaningful contributor rather than a tiny tie-breaker:
//!   ~0.33 at 300 SLOC, ~0.48 at 1k, ~0.85 at 10k, and >1.0 past ~50k SLOC.
//!   Those magnitudes are comparable to the churn terms (a recency-churn
//!   "point" is `0.30*ln1p(50)` ≈ 1.18) and dwarf the entropy (≤0.15) and
//!   long-churn (≈0.27 at heavy churn) terms. Large files are only weakly
//!   correlated with defects, but the squared-log scaling keeps size a
//!   first-class additive signal across realistic file sizes.
//! - **v2** adds two recent-window process-entropy terms (issue #330):
//!   *change entropy* (Hassan 2009; file-level Pearson 0.54 with defects
//!   on Apache projects) and *co-change graph entropy* (arXiv 2504.18511,
//!   2025; combining the two improved AUROC in 82.5% of cases). Both
//!   enter additively, weighted below recent churn/commits but on par
//!   with the fix term — they are bounded per file and complement, rather
//!   than restate, the v1 churn/commit signals.
//!
//! Bumping *either* formula in any way **must** increment
//! [`RISK_SCORE_VERSION`] so downstream consumers can detect the
//! change. Both the weighted sum ([`weighted`]) and the percentile
//! blend ([`apply_percentile`]) stamp and are cache-keyed on that one
//! constant, so it versions both — see its doc for the full contract.

use super::stats::Stats;

/// Version of the risk-score computation, covering **both** formulas.
/// A single `risk_score_version` is stamped on every file's output
/// regardless of the active [`super::RiskFormula`] (the weighted sum or
/// the percentile blend), and the persistent VCS cache keys reuse on it
/// (see [`super::cache`]). Increment on any change that can alter an
/// emitted `risk_score`, including:
///
/// - the weighted formula ([`weighted`]): its term set, weights, or
///   categorical bumps below; and
/// - the percentile blend ([`apply_percentile`]): its extractor set,
///   mid-rank scaling, or normalization.
///
/// `2` added the change- and co-change-entropy terms to both formulas
/// (issue #330).
pub const RISK_SCORE_VERSION: u32 = 2;

/// RHEL4 high-developer-count threshold (~16× vulnerability likelihood).
const HIGH_DEV_THRESHOLD: u32 = 9;
/// RHEL4 softer developer-count threshold.
const MID_DEV_THRESHOLD: u32 = 6;
/// Multiplicative bump applied at or above [`HIGH_DEV_THRESHOLD`].
const HIGH_DEV_BONUS: f64 = 0.35;
/// Multiplicative bump applied at or above [`MID_DEV_THRESHOLD`].
const MID_DEV_BONUS: f64 = 0.15;
/// Multiplicative bump for a file first seen within the recent window.
const NEW_FILE_BONUS: f64 = 0.15;
/// Absolute tolerance for treating two percentile signals as tied. All
/// signals are integer counts or ratios in `[0, 1]`, so `1e-9` is far
/// below any meaningful difference yet absorbs floating-point ratio drift.
const TIE_TOLERANCE: f64 = 1e-9;

/// Raw signals consumed by the weighted formula. Decoupled from
/// [`Stats`] so the formula is unit-testable on synthetic inputs
/// without constructing a full stats record.
#[derive(Clone, Copy, Debug)]
pub struct ScoreInput {
    /// Lines added + deleted in the recent window.
    pub churn_recent: u64,
    /// Lines added + deleted in the long window.
    pub churn_long: u64,
    /// Distinct commits in the recent window.
    pub commits_recent: u32,
    /// Distinct commits in the long window.
    pub commits_long: u32,
    /// Distinct authors in the long window.
    pub authors_long: u32,
    /// Top-author share of edits in `[0, 1]`.
    pub ownership_top_share: f64,
    /// Long-window bug-fix commit count.
    pub bug_fix_commits: u32,
    /// Long-window security-fix commit count.
    pub security_fix_commits: u32,
    /// Source lines of the file at the target ref (tie-breaker).
    pub sloc: u64,
    /// Days since the file's first in-window commit (capped at window).
    pub age_days: u32,
    /// Recent-window length in days (new-file threshold).
    pub recent_window_days: u32,
    /// Recent-window change entropy (Hassan HCM share, in bits).
    pub change_entropy_recent: f64,
    /// Recent-window co-change graph entropy (in bits).
    pub cochange_entropy_recent: f64,
}

/// Compute the weighted composite risk score for one file.
///
/// See the module docs for the term-by-term literature grounding. The
/// `f64` casts of count fields are exact for every realistic input
/// (counts never exceed 2^53) and the score is ordinal, so the lints
/// are allowed locally with that justification.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn weighted(input: &ScoreInput) -> f64 {
    let recency_churn = ln1p(input.churn_recent as f64);
    let long_churn = ln1p(input.churn_long as f64);
    let recency_count = ln1p(f64::from(input.commits_recent));
    let long_count = ln1p(f64::from(input.commits_long));
    let author_factor = ln1p(f64::from(input.authors_long));
    let dilution = (1.0 - input.ownership_top_share).clamp(0.0, 1.0);
    let fix_factor =
        ln1p(f64::from(input.bug_fix_commits) + 2.0 * f64::from(input.security_fix_commits));
    // Squared log over 100, entering `base` with coefficient 1.0. This is a
    // meaningful contributor, not a tie-breaker: ~0.85 at 10k SLOC and >1.0
    // past ~50k SLOC, comparable to the recency-churn term (≈1.18 per
    // churn-point) and well above the entropy (≤0.15) and long-churn terms.
    let size_factor = ln1p(input.sloc as f64).powi(2) / 100.0;
    // Recent-window process entropy (v2): scattered changes (Hassan) and
    // wide co-change blast radius (arXiv 2504.18511) both predict defects.
    // Already log-scaled (bits), so they enter linearly; the `.max(0.0)`
    // is defensive against an upstream negative.
    let entropy_factor =
        0.10 * input.change_entropy_recent.max(0.0) + 0.05 * input.cochange_entropy_recent.max(0.0);

    let new_file_bonus = if input.age_days < input.recent_window_days {
        NEW_FILE_BONUS
    } else {
        0.0
    };
    let dev_bonus = if input.authors_long >= HIGH_DEV_THRESHOLD {
        HIGH_DEV_BONUS
    } else if input.authors_long >= MID_DEV_THRESHOLD {
        MID_DEV_BONUS
    } else {
        0.0
    };

    let base = 0.30 * recency_churn
        + 0.25 * recency_count
        + 0.15 * long_count
        + 0.15 * author_factor * (1.0 + dilution)
        + 0.10 * fix_factor
        + 0.05 * long_churn
        + entropy_factor
        + size_factor;

    base * (1.0 + dev_bonus + new_file_bonus)
}

/// `ln(1 + x)` guarding against a negative argument from clock skew or
/// an upstream miscount; the domain of every caller is non-negative, so
/// the clamp is purely defensive. Shared with the JIT formula
/// ([`super::jit`]) so the two scores log-scale counts identically.
pub(super) fn ln1p(x: f64) -> f64 {
    (1.0 + x.max(0.0)).ln()
}

/// Recompute every file's `risk_score` as the mean percentile rank of
/// its signals within the analyzed set (the `--risk-formula percentile`
/// mode).
///
/// Each signal is ranked independently using the *mid-rank* of ties
/// (so identical values share a percentile), scaled to `[0, 100]`, and
/// the per-file mean across signals becomes the score. With fewer than
/// two files percentiles are undefined, so the weighted score is left
/// in place.
///
/// The `u64 → f64` signal casts are exact for every realistic churn
/// count (well under 2^53) and the result is ordinal, so the precision
/// lint is allowed for the whole pass.
///
/// The resulting `risk_score` is stamped with and cache-keyed on
/// [`RISK_SCORE_VERSION`] (the same constant as the weighted formula),
/// so any change to this blend — the extractor set, the mid-rank
/// scaling, or the `/ signal_count * 100` normalization — **must**
/// increment that constant.
#[allow(clippy::cast_precision_loss)]
pub fn apply_percentile(stats: &mut [Stats]) {
    if stats.len() < 2 {
        return;
    }
    // Signals contributing to the percentile blend. Ownership enters as
    // its dilution `(1 - share)` so "more diffuse" ranks higher, matching
    // the weighted formula's direction. The two recent-window entropy
    // signals join in v2 (issue #330), so both formulas read the new data.
    let extractors: [fn(&Stats) -> f64; 10] = [
        |s| s.churn_recent as f64,
        |s| s.churn_long as f64,
        |s| f64::from(s.commits_recent),
        |s| f64::from(s.commits_long),
        |s| f64::from(s.authors_long),
        |s| 1.0 - s.ownership_top_share,
        |s| f64::from(s.bug_fix_commits),
        |s| f64::from(s.security_fix_commits),
        |s| s.change_entropy_recent.max(0.0),
        |s| s.cochange_entropy_recent.max(0.0),
    ];

    let n = stats.len();
    let mut blended = vec![0.0_f64; n];
    for extract in extractors {
        let values: Vec<f64> = stats.iter().map(extract).collect();
        for (i, &v) in values.iter().enumerate() {
            // Tie tolerance: integer-derived signals compare bit-exact,
            // but the dilution signal `1 - top/total` can land ~1 ULP
            // apart for mathematically-equal ratios (e.g. 2/6 vs 1/3).
            // `f64::EPSILON` is the ULP at magnitude 1.0 (too tight here),
            // so use a small absolute tolerance — and apply it to BOTH
            // filters so a near-equal value is counted as tied, never as
            // both "less" and "equal" (which could push `pct` past 1.0).
            let less = values.iter().filter(|&&o| o < v - TIE_TOLERANCE).count();
            let equal = values
                .iter()
                .filter(|&&o| (o - v).abs() < TIE_TOLERANCE)
                .count();
            // Mid-rank: average position among equal values, in [0, 1].
            let pct = (less as f64 + (equal as f64 - 1.0) / 2.0) / (n as f64 - 1.0);
            blended[i] += pct.clamp(0.0, 1.0);
        }
    }
    let signal_count = extractors.len() as f64;
    for (s, blend) in stats.iter_mut().zip(blended) {
        s.risk_score = (blend / signal_count) * 100.0;
    }
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;
