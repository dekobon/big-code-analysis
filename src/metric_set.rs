//! Per-metric selection: the [`Metric`] enum and the
//! [`MetricSet`] bitfield it gates.
//!
//! Used by [`MetricsOptions::with_only`](crate::MetricsOptions::with_only)
//! to restrict which metrics are computed during a walk, and by
//! [`CodeMetrics`](crate::CodeMetrics)'s `Serialize` impl to elide
//! fields the caller did not select.

use std::fmt;

/// One metric computed by the analysis walker.
///
/// Pass a slice of these to
/// [`MetricsOptions::with_only`](crate::MetricsOptions::with_only) to
/// restrict computation to the listed metrics.
///
/// `#[non_exhaustive]` so future metrics can land additively. Use
/// `match` against the existing variants and either a wildcard arm or
/// the `m if !MetricSet::all().contains(m)` guard to stay
/// forwards-compatible.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Metric {
    /// Cognitive complexity ([`crate::cognitive::Stats`]).
    Cognitive,
    /// Cyclomatic complexity ([`crate::cyclomatic::Stats`]).
    Cyclomatic,
    /// Halstead ([`crate::halstead::Stats`]).
    Halstead,
    /// LoC family ([`crate::loc::Stats`]).
    Loc,
    /// Number of methods ([`crate::nom::Stats`]).
    Nom,
    /// Token counts ([`crate::tokens::Stats`]).
    Tokens,
    /// Number of arguments ([`crate::nargs::Stats`]).
    NArgs,
    /// Exit-point count ([`crate::exit::Stats`]).
    Exit,
    /// ABC ([`crate::abc::Stats`]).
    Abc,
    /// Number of public methods ([`crate::npm::Stats`]).
    Npm,
    /// Number of public attributes ([`crate::npa::Stats`]).
    Npa,
    /// Maintainability index ([`crate::mi::Stats`]). Derived metric:
    /// selecting only `Mi` via
    /// [`MetricsOptions::with_only`](crate::MetricsOptions::with_only)
    /// also pulls in [`Metric::Loc`], [`Metric::Cyclomatic`], and
    /// [`Metric::Halstead`].
    Mi,
    /// Weighted methods per class ([`crate::wmc::Stats`]). Derived
    /// metric: selecting `Wmc` also pulls in [`Metric::Cyclomatic`]
    /// and [`Metric::Nom`].
    Wmc,
}

impl Metric {
    // Bit position used inside [`MetricSet`]. The ordering is
    // intentionally arbitrary — the only contract is that each
    // variant maps to a distinct bit.
    #[inline]
    const fn bit(self) -> u16 {
        1 << (self as u32)
    }

    /// Returns the slice of metrics this metric depends on.
    ///
    /// Derived metrics (`Mi`, `Wmc`) consume the outputs of other
    /// metrics during the finalize step; selecting one without its
    /// dependencies would leave the dependency's `Stats` at default
    /// (zero) values and silently corrupt the derived value. Callers
    /// typically reach this through
    /// [`MetricsOptions::with_only`](crate::MetricsOptions::with_only),
    /// which auto-resolves the closure transparently.
    #[must_use]
    pub const fn dependencies(self) -> &'static [Metric] {
        match self {
            // Mi = function(Loc, Cyclomatic, Halstead). All three must
            // be computed for the MI formula to be meaningful.
            Self::Mi => &[Self::Loc, Self::Cyclomatic, Self::Halstead],
            // Wmc aggregates per-method cyclomatic complexity and
            // needs Nom to count those methods.
            Self::Wmc => &[Self::Cyclomatic, Self::Nom],
            _ => &[],
        }
    }
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Cognitive => "cognitive",
            Self::Cyclomatic => "cyclomatic",
            Self::Halstead => "halstead",
            Self::Loc => "loc",
            Self::Nom => "nom",
            Self::Tokens => "tokens",
            Self::NArgs => "nargs",
            Self::Exit => "exit",
            Self::Abc => "abc",
            Self::Npm => "npm",
            Self::Npa => "npa",
            Self::Mi => "mi",
            Self::Wmc => "wmc",
        };
        f.write_str(s)
    }
}

/// Bitfield of selected metrics.
///
/// Stored on [`MetricsOptions`](crate::MetricsOptions) (controls
/// which metrics the walker computes) and on
/// [`CodeMetrics`](crate::CodeMetrics) (controls which fields the
/// `Serialize` impl emits).
///
/// `MetricSet::all()` is the default: every metric enabled, matching
/// the pre-#257 behaviour.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct MetricSet(u16);

impl MetricSet {
    // All-metrics mask: OR together every variant's bit. Kept
    // explicit (rather than `(1 << N) - 1`) so adding a new variant
    // requires a deliberate edit here and surfaces in code review.
    const ALL_BITS: u16 = Metric::Cognitive.bit()
        | Metric::Cyclomatic.bit()
        | Metric::Halstead.bit()
        | Metric::Loc.bit()
        | Metric::Nom.bit()
        | Metric::Tokens.bit()
        | Metric::NArgs.bit()
        | Metric::Exit.bit()
        | Metric::Abc.bit()
        | Metric::Npm.bit()
        | Metric::Npa.bit()
        | Metric::Mi.bit()
        | Metric::Wmc.bit();

    /// Empty set (no metrics selected).
    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Full set (every metric selected). This is the default for
    /// [`MetricsOptions`](crate::MetricsOptions), preserving the
    /// pre-#257 "compute everything" behaviour.
    #[inline]
    #[must_use]
    pub const fn all() -> Self {
        Self(Self::ALL_BITS)
    }

    /// Returns `true` if `metric` is in the set.
    #[inline]
    #[must_use]
    pub const fn contains(self, metric: Metric) -> bool {
        (self.0 & metric.bit()) != 0
    }

    /// Returns a new set with `metric` inserted.
    #[inline]
    #[must_use]
    pub const fn with(self, metric: Metric) -> Self {
        Self(self.0 | metric.bit())
    }

    /// Returns the union of two sets.
    #[inline]
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Insert `metric` (in place).
    #[inline]
    pub fn insert(&mut self, metric: Metric) {
        self.0 |= metric.bit();
    }

    // Build a `MetricSet` from a slice, auto-adding the transitive
    // dependencies of each selected metric. This is the workhorse
    // behind `MetricsOptions::with_only` — the caller-facing builder
    // enforces the full dependency closure so a request for `Mi`
    // alone still computes `Loc + Cyclomatic + Halstead`. We use a
    // worklist (rather than a single pass) so a future derived metric
    // whose dependency is itself derived still resolves the complete
    // closure. The loop terminates because each iteration either
    // inserts a new bit or the worklist drains; the bitfield is
    // bounded at `Metric` variant count.
    pub(crate) fn from_slice_with_deps(metrics: &[Metric]) -> Self {
        let mut set = Self::empty();
        let mut worklist: Vec<Metric> = metrics.to_vec();
        while let Some(m) = worklist.pop() {
            if set.contains(m) {
                continue;
            }
            set.insert(m);
            for &dep in m.dependencies() {
                if !set.contains(dep) {
                    worklist.push(dep);
                }
            }
        }
        set
    }
}

impl Default for MetricSet {
    /// Default = every metric selected, matching the pre-#257
    /// behaviour of [`MetricsOptions::default`](crate::MetricsOptions::default).
    #[inline]
    fn default() -> Self {
        Self::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_contains_nothing() {
        let set = MetricSet::empty();
        assert!(!set.contains(Metric::Loc));
        assert!(!set.contains(Metric::Halstead));
        assert!(!set.contains(Metric::Mi));
    }

    #[test]
    fn all_contains_every_variant() {
        let set = MetricSet::all();
        for m in [
            Metric::Cognitive,
            Metric::Cyclomatic,
            Metric::Halstead,
            Metric::Loc,
            Metric::Nom,
            Metric::Tokens,
            Metric::NArgs,
            Metric::Exit,
            Metric::Abc,
            Metric::Npm,
            Metric::Npa,
            Metric::Mi,
            Metric::Wmc,
        ] {
            assert!(set.contains(m), "MetricSet::all() must contain {m}");
        }
    }

    #[test]
    fn with_dependencies_pulls_in_mi_inputs() {
        let set = MetricSet::from_slice_with_deps(&[Metric::Mi]);
        assert!(set.contains(Metric::Mi));
        assert!(set.contains(Metric::Loc), "Mi depends on Loc");
        assert!(set.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
        assert!(set.contains(Metric::Halstead), "Mi depends on Halstead");
        // Unrelated metrics stay out.
        assert!(!set.contains(Metric::Abc));
        assert!(!set.contains(Metric::Tokens));
    }

    #[test]
    fn with_dependencies_pulls_in_wmc_inputs() {
        let set = MetricSet::from_slice_with_deps(&[Metric::Wmc]);
        assert!(set.contains(Metric::Wmc));
        assert!(
            set.contains(Metric::Cyclomatic),
            "Wmc depends on Cyclomatic"
        );
        assert!(set.contains(Metric::Nom), "Wmc depends on Nom");
    }

    // The closure must follow transitive dependencies, not just a
    // single hop. We exercise this by feeding the slice an entry
    // whose deps' own deps would be missed by a one-pass loop.
    // Today no metric has a derived dependency, so we simulate the
    // shape by listing Mi alongside an unrelated metric and asserting
    // the worklist still terminates at the same closure as
    // `&[Metric::Mi]` alone.
    #[test]
    fn closure_is_idempotent_for_mixed_input() {
        let a = MetricSet::from_slice_with_deps(&[Metric::Mi, Metric::Loc]);
        let b = MetricSet::from_slice_with_deps(&[Metric::Mi]);
        // Loc was already in Mi's closure; explicitly adding it is a
        // no-op and must not corrupt or duplicate state.
        assert_eq!(a, b);
    }

    // The closure must terminate even when the input contains
    // duplicates; the worklist algorithm guards against this by
    // skipping bits already set.
    #[test]
    fn closure_handles_duplicate_input() {
        let set = MetricSet::from_slice_with_deps(&[Metric::Mi, Metric::Mi, Metric::Mi]);
        assert_eq!(set, MetricSet::from_slice_with_deps(&[Metric::Mi]));
    }

    #[test]
    fn empty_slice_yields_empty_set() {
        assert_eq!(MetricSet::from_slice_with_deps(&[]), MetricSet::empty());
    }

    #[test]
    fn distinct_bits_per_variant() {
        // Each variant must map to a distinct bit; otherwise the
        // bitfield silently aliases two metrics and gating one
        // toggles the other.
        let mut seen: u16 = 0;
        for m in [
            Metric::Cognitive,
            Metric::Cyclomatic,
            Metric::Halstead,
            Metric::Loc,
            Metric::Nom,
            Metric::Tokens,
            Metric::NArgs,
            Metric::Exit,
            Metric::Abc,
            Metric::Npm,
            Metric::Npa,
            Metric::Mi,
            Metric::Wmc,
        ] {
            let bit = m.bit();
            assert_eq!(seen & bit, 0, "duplicate bit for {m}: {bit:#b}");
            seen |= bit;
        }
        assert_eq!(seen, MetricSet::ALL_BITS);
    }
}
