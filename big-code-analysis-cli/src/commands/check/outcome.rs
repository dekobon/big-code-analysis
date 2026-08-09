//! `bca check` exit-code outcome classification (#385).

use super::super::*;
use super::*;

/// Severity category of a `bca check` run, used to derive the process
/// exit code (#385). The variants are *not* the exit codes — the
/// mapping depends on `--exit-codes=tiered` (see [`Self::exit_code`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckOutcome {
    /// No violations survived filtering.
    Clean,
    /// Violations exist, but none is a baseline regression and none
    /// breaches the hard limit under `--tier=soft`. Also the bucket for
    /// every violation when no `--baseline` was supplied (nothing is
    /// baselined, so nothing can have "regressed").
    NewOnly,
    /// Every kept violation matched a baseline entry that worsened.
    RegressionOnly,
    /// A mix of new offenders and baseline regressions.
    Mixed,
    /// At least one `--tier=soft` violation also exceeds the hard
    /// limit — escalated above the new/regression split because a true
    /// breach is more urgent than soft-band encroachment.
    HardBreach,
}

impl CheckOutcome {
    /// Map the outcome to a process exit code. In the default contract
    /// (`tiered == false`) every non-clean run collapses to exit `2`,
    /// preserving the stable 0/1/2 behaviour every existing integration
    /// relies on. In tiered mode (`--exit-codes=tiered`) each category
    /// gets its own code (2-5). Returns `None` for a clean run, where
    /// the caller exits 0 implicitly by returning.
    // The parameter used to be called `strict`, predating the
    // `--strict` gate profile, which is unrelated to exit codes.
    pub(crate) fn exit_code(self, tiered: bool) -> Option<i32> {
        let code = match self {
            Self::Clean => return None,
            Self::NewOnly => 2,
            Self::RegressionOnly => 3,
            Self::Mixed => 4,
            Self::HardBreach => 5,
        };
        // The default contract collapses every violation category to
        // exit 2; only `--exit-codes=tiered` surfaces the 3/4/5 split.
        Some(if tiered { code } else { 2 })
    }
}

/// Categorise the kept violations for the exit-code contract (#385).
///
/// Each violation carries `hard_limit`, the resolved hard-tier ceiling
/// for its metric *under the table that gated its file* — which since
/// #1141 may be a `[thresholds.lang.<slug>]` override rather than the
/// project-wide one, and which is `None` for a metric with a soft limit
/// but no hard one (nothing to breach). It is consulted only at the soft
/// tier, where a violation whose value also exceeds that ceiling
/// escalates to [`CheckOutcome::HardBreach`]. At the hard tier every
/// violation already exceeds it, so the escalation is suppressed (it
/// would otherwise swallow the new/regr split) and only baseline
/// coverage drives the result.
///
/// Reading the ceiling off the violation rather than a global map is
/// what keeps a loosened language from reporting exit 5 for every
/// function between the project limit and its own: a C function at
/// cognitive 24 under `[thresholds.lang.c] cognitive = 25` is a
/// soft-band encroachment, not a hard breach, even though the project
/// table says 15.
pub(crate) fn classify_check_outcome(
    pairs: &[(Violation, Option<Coverage>)],
    tier: Tier,
) -> CheckOutcome {
    if pairs.is_empty() {
        return CheckOutcome::Clean;
    }
    let mut has_new = false;
    let mut has_regression = false;
    let mut has_hard_breach = false;
    for (v, coverage) in pairs {
        // Direction-aware hard-breach escalation (#837): mirror the
        // gate's breach test so the lower-is-worse `mi.*` family escalates
        // on a value *below* the hard floor, not above it. A NaN value
        // (degenerate Halstead on a trivial function) fails both
        // comparisons in `breaches_limit`, so it never escalates to a hard
        // breach; it falls to the new/regr split below, mirroring how
        // `Baseline::classify` treats a NaN as `Regressed` rather than a
        // magnitude. A NaN has no meaningful distance from the ceiling.
        if tier == Tier::Soft
            && let Some(hard) = v.hard_limit
            && breaches_limit(v.value, hard, v.lower_is_worse)
        {
            has_hard_breach = true;
        }
        match coverage {
            Some(Coverage::Regressed { .. }) => has_regression = true,
            // `Coverage::New`, or `None` when no `--baseline` was given.
            // `Coverage::Covered` never reaches here: by default
            // `filter_by_baseline` drops it, and under `--report-suppressed`
            // `run_check` partitions it into the `suppressed` set — so the
            // `active` slice this fn classifies is always Covered-free.
            _ => has_new = true,
        }
    }
    if has_hard_breach {
        CheckOutcome::HardBreach
    } else if has_new && has_regression {
        CheckOutcome::Mixed
    } else if has_regression {
        CheckOutcome::RegressionOnly
    } else {
        CheckOutcome::NewOnly
    }
}
