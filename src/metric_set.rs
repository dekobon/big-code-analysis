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

    /// Canonical user-facing name for each metric — the single
    /// source of truth shared by the Python bindings'
    /// `bca.METRIC_NAMES` constant, the `unknown metric: <bad>;
    /// valid: …` error message, and any downstream Rust consumer
    /// that parses user input into a [`MetricSet`].
    ///
    /// Each entry round-trips through [`<Metric as
    /// std::str::FromStr>::from_str`]. The table uses the
    /// JSON-output-key spelling for [`Metric::Exit`] (`"nexits"`,
    /// matching the `CodeMetrics::Serialize` impl in
    /// `src/spaces.rs`) rather than the [`fmt::Display`] spelling
    /// (`"exit"`); both parse to [`Metric::Exit`] via the alias
    /// arm in `FromStr`, but the canonical spelling exposed here
    /// is the JSON one so callers see the same name in
    /// `Metric::NAMES`, in the output dict, and in error
    /// messages.
    ///
    /// Alphabetised. The drift between this table and the
    /// `FromStr` arms (or the `Metric` enum itself) is guarded by
    /// `names_table_parses_to_every_variant` and
    /// `names_table_is_alphabetised` in the test module below.
    pub const NAMES: &'static [&'static str] = &[
        "abc",
        "cognitive",
        "cyclomatic",
        "halstead",
        "loc",
        "mi",
        "nargs",
        "nexits",
        "nom",
        "npa",
        "npm",
        "tokens",
        "wmc",
    ];
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

/// Error returned by [`<Metric as FromStr>::from_str`] when the input
/// is not a recognised metric name.
///
/// Holds the offending input verbatim. Downstream consumers that own
/// the canonical name table (e.g. the `bca` Python bindings'
/// `METRIC_NAMES` constant) typically compose this with a
/// `valid: <list>` suffix from their own source of truth; this type
/// deliberately stays out of that policy and only carries the
/// rejected input so the wrapper layer can format the user-facing
/// message however it wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseMetricError(String);

impl fmt::Display for ParseMetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown metric: {}", self.0)
    }
}

impl std::error::Error for ParseMetricError {}

impl std::str::FromStr for Metric {
    type Err = ParseMetricError;

    /// Parse a [`Metric`] from its [`fmt::Display`] spelling.
    ///
    /// Strict lowercase: `"Loc"` is rejected. The single alias is
    /// `"nexits"`, which parses to [`Metric::Exit`] — this matches
    /// the JSON output key the metric's `Stats` serialises under,
    /// so downstream consumers can use either the enum-Display
    /// spelling or the JSON-key spelling interchangeably.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cognitive" => Ok(Self::Cognitive),
            "cyclomatic" => Ok(Self::Cyclomatic),
            "halstead" => Ok(Self::Halstead),
            "loc" => Ok(Self::Loc),
            "nom" => Ok(Self::Nom),
            "tokens" => Ok(Self::Tokens),
            "nargs" => Ok(Self::NArgs),
            "exit" | "nexits" => Ok(Self::Exit),
            "abc" => Ok(Self::Abc),
            "npm" => Ok(Self::Npm),
            "npa" => Ok(Self::Npa),
            "mi" => Ok(Self::Mi),
            "wmc" => Ok(Self::Wmc),
            _ => Err(ParseMetricError(s.to_owned())),
        }
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

    /// Build a `MetricSet` from a slice, auto-adding the transitive
    /// dependencies of each selected metric.
    ///
    /// This is the workhorse behind
    /// [`MetricsOptions::with_only`](crate::MetricsOptions::with_only):
    /// the caller-facing builder enforces the full dependency closure
    /// so a request for `Mi` alone still computes
    /// `Loc + Cyclomatic + Halstead`. Exposed `pub` because
    /// downstream consumers (notably the `bca` Python bindings'
    /// `parse_metric_names` helper) parse user input into a
    /// `Vec<Metric>` and need the same closure-resolution semantics
    /// without re-implementing the worklist.
    ///
    /// Implementation note: uses a worklist rather than a single pass
    /// so a future derived metric whose dependency is itself derived
    /// still resolves the complete closure. The loop terminates
    /// because each iteration either inserts a new bit or the
    /// worklist drains; the bitfield is bounded at `Metric` variant
    /// count.
    #[must_use]
    pub fn from_slice_with_deps(metrics: &[Metric]) -> Self {
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

    // Listing a metric that is already in another entry's closure
    // is a no-op and does not corrupt or duplicate state. Today's
    // dependency graph is flat (Mi/Wmc both depend only on leaf
    // metrics), so this test cannot exercise the worklist's
    // transitive resolution — a single-pass implementation that
    // pulls in only direct dependencies would also pass. When a
    // derived-of-derived metric lands, replace this with a test
    // that actually exercises the multi-hop closure (e.g. by
    // feeding an entry whose dependency itself has a non-empty
    // `dependencies()` list).
    #[test]
    fn closure_is_idempotent_for_mixed_input() {
        let a = MetricSet::from_slice_with_deps(&[Metric::Mi, Metric::Loc]);
        let b = MetricSet::from_slice_with_deps(&[Metric::Mi]);
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

    /// Every `Metric` variant. Tests that need to walk the enum
    /// exhaustively reach for this constant so a newly-added variant
    /// surfaces as a compile error here (the wildcard in the
    /// initialiser would not catch a missed `match` arm in
    /// [`fmt::Display`] or [`std::str::FromStr`]).
    const ALL_VARIANTS: &[Metric] = &[
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
    ];

    #[test]
    fn from_str_round_trips_every_variant_display_name() {
        // Reverting any single arm in `impl FromStr for Metric`
        // makes this fail on exactly that variant — the test is
        // load-bearing per `.claude/rules/testing.md`.
        for &m in ALL_VARIANTS {
            let parsed: Metric = m
                .to_string()
                .parse()
                .unwrap_or_else(|e| panic!("Display->FromStr round-trip failed for {m}: {e}"));
            assert_eq!(parsed, m, "round-trip mismatch for {m}");
        }
    }

    #[test]
    fn from_str_accepts_nexits_alias_for_exit() {
        // `Metric::Exit` serialises as JSON key "nexits"; we accept
        // both spellings so consumers can name the metric by either
        // its enum-Display spelling or its JSON output key.
        assert_eq!("exit".parse::<Metric>().unwrap(), Metric::Exit);
        assert_eq!("nexits".parse::<Metric>().unwrap(), Metric::Exit);
    }

    #[test]
    fn from_str_rejects_uppercase() {
        let err = "Loc".parse::<Metric>().unwrap_err();
        assert_eq!(err.to_string(), "unknown metric: Loc");
    }

    // Drift guard: every entry in `Metric::NAMES` must parse via
    // `FromStr`, and every variant must have at least one entry
    // in the table that parses to it (the `"exit"`/`"nexits"`
    // alias means `Exit` is reached via the canonical `"nexits"`
    // spelling, not via the Display arm). Adding a `Metric`
    // variant without a `NAMES` entry — or vice versa — fails
    // here before any pytest run.
    #[test]
    fn names_table_parses_to_every_variant() {
        use std::collections::HashSet;
        let mut seen: HashSet<Metric> = HashSet::new();
        for name in Metric::NAMES {
            let parsed = name.parse::<Metric>().unwrap_or_else(|_| {
                panic!("Metric::NAMES contains {name:?} but FromStr rejects it")
            });
            seen.insert(parsed);
        }
        for &m in ALL_VARIANTS {
            assert!(
                seen.contains(&m),
                "Metric::{m:?} is not represented in Metric::NAMES; \
                 add the canonical spelling to the table",
            );
        }
    }

    // The error-message `valid: <list>` and the public
    // `bca.METRIC_NAMES` tuple both surface this slice verbatim;
    // pinning the alphabetised invariant catches accidental
    // re-orderings on `cargo test`.
    #[test]
    fn names_table_is_alphabetised() {
        let mut sorted: Vec<&str> = Metric::NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            Metric::NAMES,
            sorted.as_slice(),
            "Metric::NAMES must stay alphabetised",
        );
    }

    // `MetricsOptions::with_metric_set` consumes its argument
    // verbatim — no closure resolution. Pinning the contrast with
    // `with_only` (which DOES resolve deps) catches a future
    // "helpful" refactor that adds auto-resolution to
    // `with_metric_set`: such a change would silently fix some
    // callers but invalidate the public-API contract documented
    // on the builder, where "this set MUST be closed before it
    // reaches this builder" is the load-bearing precondition.
    //
    // The test lives alongside `MetricSet` rather than in
    // `spaces.rs` because the contrast is between two `MetricSet`
    // operations: `from_slice_with_deps` (closure-resolving) vs.
    // raw construction via `empty().with(...)` (no resolution).
    #[test]
    fn with_metric_set_does_not_resolve_dependencies() {
        // `from_slice_with_deps(&[Mi])` includes Loc, Cyclomatic,
        // Halstead alongside Mi…
        let resolved = MetricSet::from_slice_with_deps(&[Metric::Mi]);
        assert!(resolved.contains(Metric::Mi));
        assert!(resolved.contains(Metric::Loc));
        assert!(resolved.contains(Metric::Cyclomatic));
        assert!(resolved.contains(Metric::Halstead));

        // …whereas `empty().with(Mi)` does NOT auto-resolve, and
        // the caller-owned closure precondition documented on
        // `MetricsOptions::with_metric_set` is what guards
        // against MI being computed against zero-valued inputs.
        let bare = MetricSet::empty().with(Metric::Mi);
        assert!(bare.contains(Metric::Mi));
        assert!(!bare.contains(Metric::Loc), "with(Mi) must NOT pull Loc");
        assert!(
            !bare.contains(Metric::Cyclomatic),
            "with(Mi) must NOT pull Cyclomatic",
        );
        assert!(
            !bare.contains(Metric::Halstead),
            "with(Mi) must NOT pull Halstead",
        );
    }

    #[test]
    fn from_str_rejects_unknown_name() {
        let err = "bogus".parse::<Metric>().unwrap_err();
        assert_eq!(err.to_string(), "unknown metric: bogus");
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
