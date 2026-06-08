// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// many-term score formula plus the flat output DTOs), not per-function
// logic complexity (cognitive/cyclomatic stay enforced) — mirrors the
// sibling `score.rs`.

//! Just-in-time (commit-level) defect-induction risk scoring (issue #331).
//!
//! Where [`score`](crate::vcs::score) ranks *files* at a ref, this module
//! scores a single *commit* at check-in time — the unit a CI gate
//! actually reviews. It is the static, rule-based counterpart to the
//! machine-learning just-in-time (JIT) defect-prediction models in the
//! literature; no model is trained or persisted, so there is nothing to
//! re-fit as a project ages.
//!
//! # Why static rules rather than a model
//!
//! The JIT defect-prediction literature (Kamei et al., *A Large-Scale
//! Empirical Study of Just-in-Time Quality Assurance*, IEEE TSE 39(6),
//! 2013; the systematic survey by Zhao et al. in *ACM Computing
//! Surveys* 55(4), 2022) is mature and consistently finds commit-level
//! prediction high-value at check-in. The survey's key tooling caveat is
//! that trained JIT models **lose predictive power within about a year**
//! and must be re-trained on recent data, so a rule-based scorer with no
//! model to drift is the most maintainable starting point. The signed
//! direction of every term below is taken from that literature, not
//! fitted, so the score needs no retraining.
//!
//! # Features (the Kamei change measures)
//!
//! Grouped exactly as Kamei et al. group them, with the open-source
//! replications [Commit Guru (Rosen, Grawi & Shihab, FSE 2015 tool
//! demo)] and [McIntosh & Kamei, *Are Fix-Inducing Changes a Moving
//! Target?*, IEEE TSE 44(5), 2018] confirming the directions on
//! independent corpora:
//!
//! - **Size** — lines added/deleted, files touched, diff hunks. Larger
//!   changes are more defect-prone (Kamei `LA`/`LD`/`NF`).
//! - **Diffusion** — distinct subsystems and directories touched, plus
//!   the within-commit change entropy. Scattered changes are riskier
//!   (Kamei `NS`/`ND`/`Entropy`).
//! - **History** — the touched files' priors: prior change count,
//!   distinct prior authors, prior bug- and security-fix counts, and the
//!   composite file-level [`risk_score`](crate::vcs::Stats::risk_score).
//!   Files with turbulent history induce more defects (Kamei
//!   `NDEV`/`NUC`; the file priors fold in the #328 composite).
//! - **Experience** — the author's prior commit count, long and recent.
//!   This term is **negatively** signed: experienced authors induce
//!   *fewer* defects (Kamei `EXP`/`REXP`, the one robustly protective
//!   signal in their models).
//! - **Purpose** — whether the commit is a fix (itself defect-prone in
//!   Kamei's `FIX`), a security fix (weighted higher here), or a revert
//!   (corrective, so dampened).
//!
//! # The score is ordinal
//!
//! [`score`] returns a non-negative composite plus its per-group
//! [`JitContributions`] (so a consumer sees *why* a commit scored as it
//! did). Like the file-level risk score it is **ordinal**: rank commits
//! by it, compare a commit against a project's own distribution, but do
//! not read the absolute magnitude as a probability. Any change to the
//! term set or weights **must** bump [`JIT_SCORE_VERSION`].
//!
//! # Scope (v1)
//!
//! Scoring an arbitrary unprovenanced diff (`--diff <file>`) is **not**
//! implemented: a bare diff carries no author, parent, or file history,
//! so only the size and diffusion groups would be computable and the
//! resulting partial score would mislead. v1 scores a real commit (which
//! has all five groups) and defers diff scoring to a follow-up. ML-based
//! JIT and server-side hook integration are out of scope per issue #331.

use serde::Serialize;

use super::score::ln1p;

/// Version of the composite JIT formula. Increment on any change to the
/// term set, weights, or bumps in [`score`]. Separate from the
/// file-level [`RISK_SCORE_VERSION`](crate::vcs::score::RISK_SCORE_VERSION)
/// so the two scores version independently.
pub const JIT_SCORE_VERSION: u32 = 1;

/// Output-shape version for a [`JitReport`]. Bump on any change to the
/// serialized field set.
pub const JIT_SCHEMA_VERSION: u32 = 1;

/// Security fixes weigh twice a plain bug fix in the history term, matching
/// the file-level formula's double weight on security-fix history.
const SECURITY_FIX_WEIGHT: f64 = 2.0;
/// File-level `risk_score` magnitudes land in roughly `[0, 15]`; dividing
/// by this keeps the file-prior term on par with the `ln1p` count terms.
const FILE_RISK_SCALE: f64 = 10.0;

/// Size of the change (Kamei `LA`/`LD`/`NF`, plus diff hunks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JitSize {
    /// Lines added across all touched text files.
    pub lines_added: u64,
    /// Lines deleted across all touched text files.
    pub lines_deleted: u64,
    /// Distinct text files the commit touched.
    pub files_touched: u32,
    /// Diff hunks across all touched text files.
    pub hunks: u32,
}

/// How widely the change is spread (Kamei `NS`/`ND`/`Entropy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct JitDiffusion {
    /// Distinct top-level subsystems (first path component) touched.
    pub subsystems: u32,
    /// Distinct directories (full parent paths) touched.
    pub directories: u32,
    /// Within-commit change entropy in bits — the Shannon entropy of the
    /// commit's churn distribution across its files (Hassan 2009; reused
    /// from [`crate::vcs::entropy`]). `0.0` for a single-file commit.
    pub entropy: f64,
}

/// Priors of the touched files, measured from history *before* the scored
/// commit (Kamei `NDEV`/`NUC` plus the #328 file composite).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct JitHistory {
    /// Σ prior in-window commits over the touched files (`NUC`).
    pub prior_changes: u32,
    /// Largest distinct-prior-author count among the touched files — a
    /// lower-bound proxy for Kamei `NDEV` (a true cross-file union of
    /// author identities is not available from the per-file index, which
    /// exposes counts, not identity sets). Reported as a feature; it does
    /// not feed the composite score.
    pub prior_distinct_authors: u32,
    /// Σ prior bug-fix commits over the touched files.
    pub prior_bug_fix_commits: u32,
    /// Σ prior security-fix commits over the touched files.
    pub prior_security_fix_commits: u32,
    /// Max composite file-level `risk_score` over the touched files.
    pub file_risk_max: f64,
    /// Mean composite file-level `risk_score` over the touched files.
    pub file_risk_mean: f64,
    /// Touched files absent from history before this commit (new files;
    /// their priors are all zero, by definition).
    pub new_files: u32,
}

/// The author's prior activity (Kamei `EXP`/`REXP`). Higher means more
/// experience, which **lowers** the score.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JitExperience {
    /// Author's prior commits in the long window, before this commit.
    pub author_prior_commits: u32,
    /// Author's prior commits in the recent window, before this commit.
    pub author_recent_commits: u32,
}

/// Keyword classification of the commit message (Kamei `FIX`, plus the
/// security and revert refinements this crate already detects).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JitPurpose {
    /// The message matched a bug-fix keyword.
    pub is_fix: bool,
    /// The message matched a security-fix keyword.
    pub is_security_fix: bool,
    /// The commit is a revert / rollback.
    pub is_revert: bool,
}

/// Every numeric feature of one commit, grouped as Kamei groups them. The
/// score's *purpose* term is supplied separately ([`JitPurpose`]) so this
/// struct is the pure numeric feature vector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct JitFeatures {
    /// Size of the change.
    pub size: JitSize,
    /// Spread of the change.
    pub diffusion: JitDiffusion,
    /// Touched-file priors.
    pub history: JitHistory,
    /// Author experience.
    pub experience: JitExperience,
}

/// Per-group contributions to the composite score. They sum to the score
/// before the non-negative floor (so `experience` is typically negative);
/// surfaced so a consumer can see which group drove the result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct JitContributions {
    /// Size term (≥ 0).
    pub size: f64,
    /// Diffusion term (≥ 0).
    pub diffusion: f64,
    /// History / file-prior term (≥ 0).
    pub history: f64,
    /// Purpose term (fix/security add, revert subtracts).
    pub purpose: f64,
    /// Experience term (≤ 0 — experience lowers risk).
    pub experience: f64,
}

/// Structural facts about the scored commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JitCommit {
    /// Resolved commit id (full hex).
    pub id: String,
    /// Number of parents (≥ 2 means a merge; `0` a root commit).
    pub parent_count: u32,
    /// Whether the commit is a merge (`parent_count > 1`).
    pub is_merge: bool,
    /// Keyword classification of the commit message.
    pub purpose: JitPurpose,
}

/// The full result of scoring one commit: the resolved commit, its
/// features, the per-group contributions, and the composite score.
///
/// Field order keeps every top-level scalar before the nested tables so
/// the report serializes cleanly to TOML (which requires values to
/// precede tables); JSON / YAML readers are order-insensitive.
#[derive(Clone, Debug, Serialize)]
pub struct JitReport {
    /// Output-shape version ([`JIT_SCHEMA_VERSION`]).
    pub jit_schema_version: u32,
    /// Composite-formula version ([`JIT_SCORE_VERSION`]).
    pub jit_score_version: u32,
    /// Long observation window, in days (priors / experience).
    pub long_window_days: u32,
    /// Recent observation window, in days (recent experience).
    pub recent_window_days: u32,
    /// Ordinal composite risk score (≥ 0). Rank commits by it; do not
    /// read it as an absolute probability.
    pub score: f64,
    /// The scored commit.
    pub commit: JitCommit,
    /// The numeric feature vector.
    pub features: JitFeatures,
    /// Per-group contributions to [`score`](JitReport::score).
    pub contributions: JitContributions,
}

/// Compute the composite JIT risk score and its per-group breakdown.
///
/// The term weights and signs come from the JIT literature (see the
/// module docs): size, diffusion, and history are positively signed;
/// experience is negative (experienced authors induce fewer defects);
/// purpose adds for fixes and subtracts for reverts. The contributions
/// sum to the score before the final non-negative floor.
///
/// The `f64` casts of count fields are exact for every realistic input
/// (counts never approach 2^53) and the score is ordinal, so the
/// precision lint is allowed locally.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn score(features: &JitFeatures, purpose: JitPurpose) -> (f64, JitContributions) {
    let s = &features.size;
    let d = &features.diffusion;
    let h = &features.history;
    let e = &features.experience;

    let churn = ln1p((s.lines_added + s.lines_deleted) as f64);
    let size =
        0.30 * churn + 0.15 * ln1p(f64::from(s.files_touched)) + 0.05 * ln1p(f64::from(s.hunks));

    // Subsystem / directory *spread*: a change confined to one location
    // contributes nothing (`saturating_sub(1)` → 0), and entropy captures
    // the within-commit scatter on top of that.
    let diffusion = 0.15 * ln1p(f64::from(d.subsystems.saturating_sub(1)))
        + 0.10 * ln1p(f64::from(d.directories.saturating_sub(1)))
        + 0.15 * d.entropy.max(0.0);

    let fix_history = f64::from(h.prior_bug_fix_commits)
        + SECURITY_FIX_WEIGHT * f64::from(h.prior_security_fix_commits);
    let history = 0.10 * ln1p(f64::from(h.prior_changes))
        + 0.15 * ln1p(fix_history)
        + 0.15 * (h.file_risk_max.max(0.0) / FILE_RISK_SCALE);

    // Experienced authors induce fewer defects (Kamei `EXP`/`REXP`), so
    // this group subtracts.
    let experience = -0.10 * ln1p(f64::from(e.author_prior_commits))
        - 0.05 * ln1p(f64::from(e.author_recent_commits));

    let purpose_term = purpose_contribution(purpose);

    let contributions = JitContributions {
        size,
        diffusion,
        history,
        purpose: purpose_term,
        experience,
    };
    let total = (size + diffusion + history + purpose_term + experience).max(0.0);
    (total, contributions)
}

/// Additive adjustments for `FIX` / security / revert (Kamei `FIX` is
/// itself defect-prone, so fixes add; a revert is corrective, so it
/// subtracts).
fn purpose_contribution(purpose: JitPurpose) -> f64 {
    let mut term = 0.0;
    if purpose.is_fix {
        term += 0.15;
    }
    if purpose.is_security_fix {
        term += 0.30;
    }
    if purpose.is_revert {
        term -= 0.20;
    }
    term
}

#[cfg(test)]
#[path = "jit_tests.rs"]
mod tests;
