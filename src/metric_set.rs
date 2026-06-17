//! Per-metric selection: the [`Metric`] enum and the
//! [`MetricSet`] bitfield it gates.
//!
//! Used by [`MetricsOptions::with_only`](crate::MetricsOptions::with_only)
//! to restrict which metrics are computed during a walk, and by
//! [`CodeMetrics`](crate::CodeMetrics)'s `Serialize` impl to elide
//! fields the caller did not select.

use std::fmt;
use std::str::FromStr;

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
///
/// `Ord` follows declaration order, not the [`Display`](fmt::Display)
/// spelling. It exists so [`Metric`] can key a `BTreeSet` — notably the
/// suppression scope (`SuppressionScope::Some`) — with a deterministic,
/// stable iteration order across runs. Do not rely on the ordering being
/// alphabetical; reorder the variants only with a deliberate review of
/// every serialized `BTreeSet<Metric>` snapshot.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
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
    Nargs,
    /// Exit-point count ([`crate::nexits::Stats`]).
    Nexits,
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
    //
    // Returns `u32` to match [`MetricSet`]'s storage width: at `u16`
    // the bitfield would overflow once a 17th variant landed (debug
    // panic / release wrap), and `Metric` is `#[non_exhaustive]`
    // specifically so new variants can land additively.
    #[inline]
    const fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// Returns the slice of metrics this metric depends on.
    ///
    /// Derived and averaged metrics consume the outputs of other
    /// metrics during the finalize step; selecting one without its
    /// dependencies would leave the dependency's `Stats` at default
    /// (zero) values and silently corrupt the result. Callers
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
            // Cognitive, Nexits, and Nargs each expose a per-function
            // average whose divisor is the function/closure count
            // sourced from Nom (see `spaces::compute_averages`).
            // Without Nom the divisor would be the `Stats` default
            // (zero), producing inf/NaN averages (#428).
            Self::Cognitive | Self::Nexits | Self::Nargs => &[Self::Nom],
            _ => &[],
        }
    }

    /// Canonical user-facing name for each metric — the single
    /// source of truth shared by the Python bindings'
    /// `bca.METRIC_NAMES` constant, the `unknown metric: <bad>;
    /// valid: …` error message, and any downstream Rust consumer
    /// that parses user input into a [`MetricSet`].
    ///
    /// Each entry round-trips through [`Metric::from_str`]. Every
    /// metric uses one canonical spelling end-to-end:
    /// [`Metric::Nexits`] is `"nexits"` in `Display`, in this table,
    /// and as the JSON output key (the `CodeMetrics::Serialize` impl
    /// in `src/spaces.rs`).
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

    /// Every metric except [`Metric::Tokens`], in declaration order.
    ///
    /// `tokens` is the one metric with no configurable threshold, so it
    /// is the one metric that cannot be named in a suppression marker
    /// (`bca: suppress(tokens)` is rejected). This is the single source
    /// of truth for the suppressible vocabulary: the suppression
    /// parser's "known metrics" hint and the threshold-name resolver
    /// both derive from it rather than hardcoding the list.
    pub fn suppressible() -> impl Iterator<Item = Metric> {
        Self::ALL.iter().copied().filter(|m| *m != Self::Tokens)
    }

    /// Every [`Metric`] variant, in declaration order. Drives
    /// [`Metric::suppressible`] and any consumer that needs to iterate
    /// the full set without re-deriving it from `NAMES`.
    const ALL: &'static [Self] = &[
        Self::Cognitive,
        Self::Cyclomatic,
        Self::Halstead,
        Self::Loc,
        Self::Nom,
        Self::Tokens,
        Self::Nargs,
        Self::Nexits,
        Self::Abc,
        Self::Npm,
        Self::Npa,
        Self::Mi,
        Self::Wmc,
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
            Self::Nargs => "nargs",
            Self::Nexits => "nexits",
            Self::Abc => "abc",
            Self::Npm => "npm",
            Self::Npa => "npa",
            Self::Mi => "mi",
            Self::Wmc => "wmc",
        };
        f.write_str(s)
    }
}

/// Error returned by [`Metric::from_str`] when the input
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

impl ParseMetricError {
    /// The rejected input that failed to parse as a [`Metric`] name.
    ///
    /// Lets callers recover the offending string programmatically
    /// rather than scraping it out of the [`Display`](fmt::Display)
    /// output.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ParseMetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown metric: {}", self.0)
    }
}

impl std::error::Error for ParseMetricError {}

impl FromStr for Metric {
    type Err = ParseMetricError;

    /// Parse a [`Metric`] from its [`fmt::Display`] spelling.
    ///
    /// Strict lowercase: `"Loc"` is rejected. Every metric has exactly
    /// one accepted spelling, matching its `Display` form, `NAMES`
    /// entry, and JSON output key.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cognitive" => Ok(Self::Cognitive),
            "cyclomatic" => Ok(Self::Cyclomatic),
            "halstead" => Ok(Self::Halstead),
            "loc" => Ok(Self::Loc),
            "nom" => Ok(Self::Nom),
            "tokens" => Ok(Self::Tokens),
            "nargs" => Ok(Self::Nargs),
            "nexits" => Ok(Self::Nexits),
            "abc" => Ok(Self::Abc),
            "npm" => Ok(Self::Npm),
            "npa" => Ok(Self::Npa),
            "mi" => Ok(Self::Mi),
            "wmc" => Ok(Self::Wmc),
            _ => Err(ParseMetricError(s.to_owned())),
        }
    }
}

// Serialize/Deserialize are hand-written rather than derived so the wire
// form is the canonical [`Display`] spelling (`nargs`, `nexits`,
// `tokens`, …) — the same vocabulary used in JSON output keys, error
// messages, and `Metric::NAMES`. A `#[derive(Serialize)]` with
// `rename_all = "snake_case"` would emit `n_args` / `n_exits` instead,
// diverging from every other surface. Routing through `Display`/`FromStr`
// keeps the spelling single-sourced. `Metric` reaches the wire as the
// element type of `SuppressionScope::Some`'s `BTreeSet`.
impl serde::Serialize for Metric {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Metric {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `Cow<str>` borrows from self-describing, zero-copy formats
        // (JSON without escapes) and owns from reader-based or
        // non-borrowing ones (CBOR, YAML, TOML, JSON with escapes).
        // `<&str>` would reject every non-borrowing format, which is
        // exactly how `BTreeSet<Metric>` reaches the wire inside a
        // `SuppressionScope::Some` CBOR/YAML/TOML payload.
        let s = std::borrow::Cow::<str>::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
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
pub struct MetricSet(u32);

impl MetricSet {
    // All-metrics mask: OR together every variant's bit. Kept
    // explicit (rather than `(1 << N) - 1`) so adding a new variant
    // requires a deliberate edit here and surfaces in code review.
    const ALL_BITS: u32 = Metric::Cognitive.bit()
        | Metric::Cyclomatic.bit()
        | Metric::Halstead.bit()
        | Metric::Loc.bit()
        | Metric::Nom.bit()
        | Metric::Tokens.bit()
        | Metric::Nargs.bit()
        | Metric::Nexits.bit()
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
        for &m in metrics {
            set.insert(m);
        }
        set.resolved()
    }

    /// Returns this set closed under [`Metric::dependencies`].
    ///
    /// Every selected metric's transitive dependencies are added so a
    /// set carrying a derived metric (e.g. [`Metric::Mi`]) also carries
    /// the inputs that metric's finalize step consumes
    /// ([`Metric::Loc`], [`Metric::Cyclomatic`], [`Metric::Halstead`]).
    /// Resolving an already-closed set is a no-op, so the operation is
    /// idempotent: `set.resolved().resolved() == set.resolved()`.
    ///
    /// This is the set-in/set-out counterpart of
    /// [`MetricSet::from_slice_with_deps`] and is what
    /// [`MetricsOptions::with_metric_set`](crate::MetricsOptions::with_metric_set)
    /// applies so a caller-supplied set can never select a derived
    /// metric without its prerequisites (#743).
    ///
    /// Implementation note: uses a worklist rather than a single pass
    /// so a future derived metric whose dependency is itself derived
    /// still resolves the complete closure. The loop terminates
    /// because each iteration either inserts a new bit or the worklist
    /// drains; the bitfield is bounded at `Metric` variant count.
    #[must_use]
    pub fn resolved(self) -> Self {
        let mut set = self;
        let mut worklist: Vec<Metric> = Metric::ALL
            .iter()
            .copied()
            .filter(|&m| self.contains(m))
            .collect();
        while let Some(m) = worklist.pop() {
            for &dep in m.dependencies() {
                if !set.contains(dep) {
                    set.insert(dep);
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
#[path = "metric_set_tests.rs"]
mod tests;
