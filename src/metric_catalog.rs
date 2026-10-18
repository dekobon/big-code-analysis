//! Single source of truth for the metric catalog.
//!
//! Before this module existed, the same set of offender metric ids was
//! hand-maintained in three places — [`output::sarif`]'s rule
//! descriptions, the CLI's threshold extractor table, and a third copy
//! inside a "does every extractor have a description" test — plus a
//! fourth, differently-shaped table powering `bca list-metrics`. Those
//! tables drifted: ten of twenty-one rule-description keys once failed
//! to match any real offender id and went unnoticed for two model
//! versions.
//!
//! [`METRICS`](crate::metric_catalog::METRICS) is now the canonical
//! list of offender sub-metric ids (`halstead.volume`, `mi.original`,
//! …) together with their long-form sentences and
//! [`Direction`](crate::metric_catalog::Direction).
//! [`FAMILIES`](crate::metric_catalog::FAMILIES) is the canonical view
//! that `bca list-metrics` renders. The library's offender formatters
//! ([`output::sarif`], [`output::code_climate`]) read `METRICS`; the
//! CLI's threshold engine keys its extractor table off the same ids and
//! a parity test pins the two id-sets together, so a new metric cannot
//! ship with a half-updated catalog.
//!
//! Each entry also records its [`Scale`](crate::metric_catalog::Scale):
//! ratio for the counts, interval for the Maintainability Index family,
//! whose zero is arbitrary and whose values go negative.
//! [`Threshold`](crate::metric_catalog::Threshold) builds on direction
//! and scale together. It is a validated limit that answers every value
//! question a gate asks: whether a value breaches, by how much, how two
//! breaches rank, where the soft tier sits, and how far a value has
//! moved from its baseline. Those answers then mean the same thing on
//! every surface (#1621).
//!
//! [`resolve_selector`](crate::metric_catalog::resolve_selector) reads
//! both tables to turn a *selector* — a name saying what to compute — into
//! the [`Metric`] family that computes it. `bca metrics --metrics` and the
//! Python bindings' `metrics=` both resolve through it, so a name copied
//! from `bca list-metrics` or `bca check --threshold` works in either or
//! neither (#1614).
//!
//! [`output::sarif`]: crate::output
//! [`output::code_climate`]: crate::output

#![allow(clippy::doc_markdown)]

use std::cmp::Ordering;
use std::fmt;
use std::sync::LazyLock;

use crate::Metric;
use crate::spaces::SpaceKind;
use crate::suggestion::{closest_names, render_suggestion};

mod threshold;
pub use threshold::{Change, Checked, LimitError, LimitWarning, Threshold};

/// The space kind a metric's threshold is meaningful on (issue #969).
///
/// A threshold gate (`bca check`, the Python `to_sarif` binding) walks
/// every [`crate::FuncSpace`] — the file-level [`SpaceKind::Unit`] root,
/// every container (class / impl / ...), and every individual function.
/// For the subtree-summed accessors a metric's value at any space that
/// owns children is a *sum across many functions*, so a per-function
/// limit would fire on every non-trivial file and multi-method `impl`.
/// Scope records the kind each metric actually measures so the front-ends
/// gate it there and nowhere else — keeping the CLI gate and the binding
/// in lockstep, the same way [`Direction`] keeps their breach direction
/// aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricScope {
    /// Gate only the whole-file [`SpaceKind::Unit`] root — the `loc.*`
    /// size family, whose limit is a per-file ceiling.
    File,
    /// Gate only individual function spaces ([`SpaceKind::Function`] —
    /// free functions, methods, closures). The per-function complexity
    /// metrics (cognitive, cyclomatic, abc, mi.*) and the subtree sums
    /// that describe one function (halstead.*, nargs, nexits, tokens)
    /// live here.
    ///
    /// Whether "one function" includes its nested closures is per
    /// metric, not per scope. `halstead.*`, `nexits` and `tokens` read
    /// subtree sums, because a closure's tokens and exits really are
    /// part of the enclosing body a reader must follow. `nargs` reads
    /// the space's own parameters instead (#1196): a closure that opens
    /// its own space is gated on its own row, and summing its arguments
    /// into the enclosing signature made the offender's number describe
    /// something its remediation could not change.
    Function,
    /// Gate only container spaces that own methods (class / struct /
    /// trait / impl / namespace / interface) — the object-oriented size
    /// metrics `nom`, `wmc`, `npm`, `npa`.
    Container,
}

impl MetricScope {
    /// Whether a threshold with this scope is evaluated against `kind`.
    ///
    /// The single source of truth for the kind-filtering both the CLI
    /// gate and the Python binding apply, so the two cannot drift on
    /// which space kinds a metric gates.
    #[must_use]
    pub fn admits(self, kind: SpaceKind) -> bool {
        match self {
            Self::File => matches!(kind, SpaceKind::Unit),
            Self::Function => matches!(kind, SpaceKind::Function),
            Self::Container => matches!(
                kind,
                SpaceKind::Class
                    | SpaceKind::Struct
                    | SpaceKind::Trait
                    | SpaceKind::Impl
                    | SpaceKind::Namespace
                    | SpaceKind::Interface
            ),
        }
    }
}

/// Which direction of a metric's value is unhealthy.
///
/// Most metrics grow worse as they grow larger; the Maintainability
/// Index family is the inverse — a *lower* value is worse. [`Threshold`]
/// reads it for every breach, severity and soft-tier question, and the
/// rule sentences use it to pick "exceeds" vs "falls below" phrasing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// A higher value is worse (cyclomatic, halstead.*, loc.*, …).
    HigherIsWorse,
    /// A lower value is worse (the `mi.*` Maintainability Index family).
    LowerIsWorse,
}

/// The measurement scale of a metric's value: which arithmetic on it
/// means anything (issue #1621).
///
/// Stevens' classification of measurement scales (*On the Theory of
/// Scales of Measurement*, Science 103, 1946) is the reason this is a
/// separate axis from [`Direction`]. On a **ratio** scale zero means
/// "none of it", so `value / limit` and "N percent worse" are
/// meaningful: 30 branches is twice 15. On an **interval** scale the
/// zero is arbitrary, so only *differences* are meaningful: a
/// Maintainability Index of -10 is not "twice as bad" as -5, and a drop
/// from 0.001 to -0.0097 is a hundredth of a point, not a 1069 % fall.
/// Every surface that ranks, scales or reports a change reads this
/// instead of assuming ratio arithmetic; [`Threshold`] is where that
/// arithmetic lives.
///
/// `#[non_exhaustive]`, as is the `Interval` variant, so a scale or a
/// field can be added in a minor release. Match with a wildcard arm and
/// `Interval { unit, .. }`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Scale {
    /// True zero, values `>= 0`: every count and count-derived metric
    /// (`cyclomatic`, `halstead.*`, `loc.*`, …). Severity is the plain
    /// `value / limit` ratio and a change is a percentage.
    Ratio,
    /// Arbitrary zero: the Maintainability Index family. Only
    /// differences are meaningful, so severity and change are measured
    /// in multiples of `unit`.
    #[non_exhaustive]
    Interval {
        /// Smallest value the metric can take (may be `-INFINITY`).
        min: f64,
        /// Largest value the metric can take (may be `INFINITY`).
        max: f64,
        /// One step of badness, in the metric's own points: the
        /// distance that plays the role a `Ratio` metric's limit plays
        /// in severity, soft-tier scaling and drift. Always finite and
        /// positive.
        unit: f64,
    },
}

/// One Visual Studio maintainability band, in Visual Studio points.
///
/// Visual Studio rates a method green at 20–100, yellow at 10–19 and
/// red at 0–9
/// (<https://learn.microsoft.com/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning>);
/// radon's A/B/C ranks use the same 10/20 edges
/// (<https://radon.readthedocs.io/en/latest/intro.html>). One band is
/// therefore 10 points.
const MI_VISUAL_STUDIO_UNIT: f64 = 10.0;

/// One Visual Studio band expressed on the original 171-point scale.
///
/// `mi.visual_studio` is `mi.original · 100 / 171` (clamped at 0), so a
/// 10-point Visual Studio band is `10 · 171 / 100 = 17.1` original
/// points. `mi.sei` keeps the original formula's 171-point scale and
/// adds a comment term; it takes the same unit by convention rather
/// than by derivation.
const MI_ORIGINAL_UNIT: f64 = MI_VISUAL_STUDIO_UNIT * 171.0 / 100.0;

/// `mi.original` / `mi.sei`: unbounded and routinely negative.
const MI_UNBOUNDED: Scale = Scale::Interval {
    min: f64::NEG_INFINITY,
    max: f64::INFINITY,
    unit: MI_ORIGINAL_UNIT,
};

/// `mi.visual_studio`: the original formula rescaled to and clamped
/// into `[0, 100]`.
const MI_PERCENT: Scale = Scale::Interval {
    min: 0.0,
    max: 100.0,
    unit: MI_VISUAL_STUDIO_UNIT,
};

/// Catalog entry for one offender-emitting sub-metric id.
///
/// The `id` is the dotted key the threshold engine emits for an
/// offender (`halstead.volume`); `family` groups ids under a top-level
/// metric (`halstead`) and must match a [`MetricFamily::name`].
///
/// `#[non_exhaustive]`: these are read-only records the library
/// constructs (downstream consumers read fields, never build them), so
/// a new field can be added in a future minor without a SemVer break.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricInfo {
    /// Dotted offender id, e.g. `"halstead.volume"` or `"cognitive"`.
    pub id: &'static str,
    /// Top-level family this id belongs to, e.g. `"halstead"`.
    pub family: &'static str,
    /// Long-form sentence for SARIF `rule.shortDescription.text` and
    /// the Code Climate `description` prefix.
    pub long_description: &'static str,
    /// Whether a higher or lower value is the unhealthy direction.
    pub direction: Direction,
    /// Whether the metric's JSON headline is an aggregate across
    /// descendant spaces (a `sum`/`*_sum` field) that does **not** match
    /// the CLI threshold accessor's per-space scalar at any interior
    /// space.
    ///
    /// `true` for the five metrics whose serialized headline diverges
    /// from the per-space accessor — `cognitive`, `cyclomatic`,
    /// `cyclomatic.modified`, `abc` (#441) and, since #1196 moved its
    /// gate onto the callable's own parameter list, `nargs`. The
    /// aggregate is at least as large everywhere and equal only where
    /// the descendants contribute nothing — always at a leaf space (no
    /// descendant function/closure spaces), and incidentally at an
    /// interior one whose descendants all score zero.
    ///
    /// This flag describes the `sum`/`*_sum`/`total` *aggregate* field,
    /// which still diverges. As of #958 (and #1236 for `nargs`) the wire
    /// shape **also** serializes each of these metrics' per-space own
    /// value (`cyclomatic.value`, `cyclomatic.modified.value`,
    /// `cognitive.value`, `abc.value`, `nargs.value`), so a JSON-walking
    /// front-end no longer needs this flag to stay correct: it reads the
    /// own value directly and emits at every space exactly like the CLI.
    /// The Python `to_sarif` binding was switched to that path in #958;
    /// before it, the binding emitted these only at leaf spaces to avoid
    /// subtree-wide values masquerading as per-space findings the CLI
    /// never produces (#855). The flag name retains its original
    /// unit-only framing.
    ///
    /// The flag is **not** derivable from the JSON path string: `nexits`
    /// also serialises a `sum` field, but its CLI accessor (`nexits_sum()`)
    /// reads that same aggregate, so it does not diverge and is `false`.
    /// The divergence is between the JSON field and the CLI accessor,
    /// which only this registry now records once for both front-ends to
    /// share (#442).
    pub skip_at_unit: bool,
    /// The space kind this metric's threshold gates (issue #969). Both
    /// the CLI threshold engine and the Python `to_sarif` binding read
    /// this to skip spaces a metric does not measure, so a metric's
    /// file-wide or `impl`-wide aggregate never fires as a per-function
    /// limit. See [`MetricScope`].
    pub scope: MetricScope,
    /// The measurement scale of the metric's value (issue #1621):
    /// [`Scale::Ratio`] for every count, [`Scale::Interval`] for the
    /// `mi.*` family. See [`Scale`].
    pub scale: Scale,
}

impl MetricInfo {
    /// Order two values of this metric by badness: [`Ordering::Greater`]
    /// means `a` is worse than `b`.
    ///
    /// The comparison is oriented by [`direction`](Self::direction) and
    /// is a total order, so it is safe to hand to a sort. A NaN sorts as
    /// the **worst** value in either direction (and two NaNs compare
    /// equal), matching how a NaN measurement is treated as a regression
    /// rather than silently passing. `-0.0` and `0.0` compare equal, as
    /// they do under [`Threshold::breaches`].
    #[must_use]
    pub fn cmp_worse(&self, a: f64, b: f64) -> Ordering {
        // Neither operand is NaN in the last arm, so `partial_cmp` is
        // total there; `total_cmp` would rank `-0.0` below `0.0` and read
        // an MI of `-0.0` against a record of `0.0` as a regression.
        let numeric = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
        match (a.is_nan(), b.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => match self.direction {
                Direction::HigherIsWorse => numeric(a, b),
                Direction::LowerIsWorse => numeric(b, a),
            },
        }
    }
}

/// A `bca list-metrics` row: the bare name printed in `names` mode and
/// the one-line summary printed in `descriptions` mode.
///
/// `#[non_exhaustive]` for the same forward-compat reason as
/// [`MetricInfo`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricRow {
    /// Bare name printed one-per-line by `list-metrics`, e.g.
    /// `"halstead"` or `"sloc"`. Downstream tooling (`bca diff`, which
    /// buckets per-file metric deltas by these names) relies on them, so
    /// they are an external contract.
    pub name: &'static str,
    /// One-line description printed in `list-metrics descriptions` mode.
    pub summary: &'static str,
}

/// A top-level metric family as surfaced by `bca list-metrics`.
///
/// Most families render as a single [`MetricRow`] whose name equals
/// [`name`](Self::name). `loc` is the exception: it renders one row per
/// sub-measurement (`sloc`, `ploc`, …) because those bare names are an
/// external grep contract.
///
/// `#[non_exhaustive]` for the same forward-compat reason as
/// [`MetricInfo`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct MetricFamily {
    /// Family key, e.g. `"halstead"`, `"loc"`. Matches
    /// [`MetricInfo::family`].
    pub name: &'static str,
    /// `list-metrics` rows for this family, in display order.
    pub rows: &'static [MetricRow],
}

/// Canonical offender sub-metric catalog. Long-form sentences and the
/// `mi.*` lower-is-worse direction moved here verbatim from the former
/// `output::rule_descriptions` table.
///
/// `#[rustfmt::skip]`: the one-row-per-entry layout keeps the table
/// scannable; rustfmt would otherwise wrap each struct over many lines.
#[rustfmt::skip]
pub const METRICS: &[MetricInfo] = &[
    MetricInfo { id: "cognitive",           family: "cognitive",  long_description: "Cognitive Complexity exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "cyclomatic",          family: "cyclomatic", long_description: "Cyclomatic Complexity exceeds the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "cyclomatic.modified", family: "cyclomatic", long_description: "Modified Cyclomatic Complexity exceeds the configured threshold.", direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "halstead.volume",     family: "halstead",   long_description: "Halstead volume exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "halstead.difficulty", family: "halstead",   long_description: "Halstead difficulty exceeds the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "halstead.effort",     family: "halstead",   long_description: "Halstead effort exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "halstead.time",       family: "halstead",   long_description: "Halstead time-to-program exceeds the configured threshold.",      direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "halstead.bugs",       family: "halstead",   long_description: "Estimated Halstead bugs exceed the configured threshold.",         direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "loc.sloc",            family: "loc",        long_description: "Source lines of code exceed the configured threshold.",            direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "loc.ploc",            family: "loc",        long_description: "Physical lines of code exceed the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "loc.lloc",            family: "loc",        long_description: "Logical lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "loc.cloc",            family: "loc",        long_description: "Comment lines of code exceed the configured threshold.",           direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "loc.blank",           family: "loc",        long_description: "Blank lines of code exceed the configured threshold.",             direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "loc.dloc",            family: "loc",        long_description: "Documentation lines of code exceed the configured threshold.",     direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::File,      scale: Scale::Ratio },
    MetricInfo { id: "nom",                 family: "nom",        long_description: "Number of methods/functions exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container, scale: Scale::Ratio },
    MetricInfo { id: "tokens",              family: "tokens",     long_description: "Number of tokens exceeds the configured threshold.",               direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "nexits",              family: "nexits",     long_description: "Number of exit points exceeds the configured threshold.",          direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "nargs",               family: "nargs",      long_description: "Number of function arguments exceeds the configured threshold.",   direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "mi.original",         family: "mi",         long_description: "Maintainability Index falls below the configured threshold.",      direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function,  scale: MI_UNBOUNDED },
    MetricInfo { id: "mi.sei",              family: "mi",         long_description: "Maintainability Index (SEI) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function,  scale: MI_UNBOUNDED },
    MetricInfo { id: "mi.visual_studio",    family: "mi",         long_description: "Maintainability Index (Visual Studio) falls below the configured threshold.", direction: Direction::LowerIsWorse,  skip_at_unit: false, scope: MetricScope::Function,  scale: MI_PERCENT },
    MetricInfo { id: "abc",                 family: "abc",        long_description: "ABC magnitude exceeds the configured threshold.",                  direction: Direction::HigherIsWorse, skip_at_unit: true,  scope: MetricScope::Function,  scale: Scale::Ratio },
    MetricInfo { id: "wmc",                 family: "wmc",        long_description: "Weighted Methods per Class exceeds the configured threshold.",     direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container, scale: Scale::Ratio },
    MetricInfo { id: "npm",                 family: "npm",        long_description: "Number of public methods exceeds the configured threshold.",       direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container, scale: Scale::Ratio },
    MetricInfo { id: "npa",                 family: "npa",        long_description: "Number of public attributes exceeds the configured threshold.",    direction: Direction::HigherIsWorse, skip_at_unit: false, scope: MetricScope::Container, scale: Scale::Ratio },
];

/// Canonical `bca list-metrics` view. Family summaries moved here
/// verbatim from the CLI's former hand-maintained catalog. Declaration
/// order is the `list-metrics` print order.
///
/// Only `loc` expands to multiple rows; every other family is a single
/// row whose name equals the family name.
pub const FAMILIES: &[MetricFamily] = &[
    MetricFamily {
        name: "cognitive",
        rows: &[MetricRow {
            name: "cognitive",
            summary: "Cognitive Complexity: how difficult code is to understand.",
        }],
    },
    MetricFamily {
        name: "cyclomatic",
        rows: &[MetricRow {
            name: "cyclomatic",
            summary: "Cyclomatic Complexity: linearly independent paths through the code; the modified variant collapses switch/match/when arms in a single switch statement into one decision point.",
        }],
    },
    MetricFamily {
        name: "halstead",
        rows: &[MetricRow {
            name: "halstead",
            summary: "Halstead suite: vocabulary, length, volume, difficulty, effort, time, bugs.",
        }],
    },
    MetricFamily {
        name: "loc",
        rows: &[
            MetricRow {
                name: "sloc",
                summary: "Source lines of code: total lines in a source file.",
            },
            MetricRow {
                name: "ploc",
                summary: "Physical lines of code: instruction lines.",
            },
            MetricRow {
                name: "lloc",
                summary: "Logical lines of code: statement count.",
            },
            MetricRow {
                name: "cloc",
                summary: "Comment lines of code.",
            },
            MetricRow {
                name: "blank",
                summary: "Blank lines.",
            },
            MetricRow {
                name: "dloc",
                summary: "Documentation lines of code: doc comments, docstrings and POD, a subset of cloc.",
            },
        ],
    },
    MetricFamily {
        name: "nom",
        rows: &[MetricRow {
            name: "nom",
            summary: "Number of methods and closures.",
        }],
    },
    MetricFamily {
        name: "tokens",
        rows: &[MetricRow {
            name: "tokens",
            summary: "Per-function token count: AST leaves excluding comments.",
        }],
    },
    MetricFamily {
        name: "nexits",
        rows: &[MetricRow {
            name: "nexits",
            summary: "Number of exit points from a function or method.",
        }],
    },
    MetricFamily {
        name: "nargs",
        rows: &[MetricRow {
            name: "nargs",
            summary: "Number of arguments to a function or method.",
        }],
    },
    MetricFamily {
        name: "mi",
        rows: &[MetricRow {
            name: "mi",
            summary: "Maintainability Index suite.",
        }],
    },
    MetricFamily {
        name: "abc",
        rows: &[MetricRow {
            name: "abc",
            summary: "ABC: assignments, branches, and conditions.",
        }],
    },
    MetricFamily {
        name: "wmc",
        rows: &[MetricRow {
            name: "wmc",
            summary: "Weighted Methods per Class.",
        }],
    },
    MetricFamily {
        name: "npm",
        rows: &[MetricRow {
            name: "npm",
            summary: "Number of public methods of a class.",
        }],
    },
    MetricFamily {
        name: "npa",
        rows: &[MetricRow {
            name: "npa",
            summary: "Number of public attributes of a class.",
        }],
    },
];

/// Catalog entry for a known offender id, or `None`. Callers pick their
/// own fallback for unknown ids (SARIF emits the raw id; Code Climate
/// falls through to its default message).
///
/// Public so out-of-crate consumers (the CLI threshold engine) can read
/// a metric's [`Direction`] — the `mi.*` family is lower-is-worse, so
/// the gate and the offender wording must consult it rather than
/// assuming a higher value is always the violation (#698).
#[must_use]
pub fn lookup(id: &str) -> Option<&'static MetricInfo> {
    METRICS.iter().find(|m| m.id == id)
}

/// Whether a lower value of the metric `id` is the unhealthy direction
/// (the `mi.*` Maintainability Index family). An id the catalog does not
/// know defaults to higher-is-worse (#698).
///
/// Deprecated since 2.4.0 (#1621): a bare direction bool cannot say how
/// far past a limit a value is, and every consumer that took one
/// re-derived that arithmetic itself. No caller in this workspace uses
/// it any more, and the root `clippy.toml` keeps it that way. Read
/// [`MetricInfo::direction`] via [`lookup`], or build a [`Threshold`]
/// and ask it. Removal is reserved for 3.0.
#[deprecated(
    since = "2.4.0",
    note = "read `MetricInfo::direction` via `lookup`, or build a `Threshold`"
)]
#[must_use]
pub fn lower_is_worse(id: &str) -> bool {
    lookup(id).is_some_and(|m| matches!(m.direction, Direction::LowerIsWorse))
}

/// The [`MetricScope`] of metric `id` — the space kind its threshold
/// gates (issue #969). `None` for an id the catalog does not know; both
/// front-ends treat an unknown id as a usage error before reaching here,
/// so the `None` arm is only a defensive fallback.
#[must_use]
pub fn scope(id: &str) -> Option<MetricScope> {
    lookup(id).map(|m| m.scope)
}

/// The [`Scale`] of metric `id` (issue #1621), or `None` for an id the
/// catalog does not know. The free-function twin of [`scope`].
#[must_use]
pub fn scale(id: &str) -> Option<Scale> {
    lookup(id).map(|m| m.scale)
}

/// The names [`resolve_selector`] advertises: every family name (`loc`,
/// `halstead`, …) and every `bca list-metrics` row name (`sloc`, `dloc`,
/// …), sorted and deduplicated.
///
/// The dotted [`METRICS`] ids (`cyclomatic.modified`, `mi.original`)
/// resolve too but are not listed: each names a field inside a family
/// these names already cover, and listing all of them would bury the
/// families in an error message.
#[must_use]
pub fn selector_names() -> &'static [&'static str] {
    // Built once: `UnknownSelectorError`'s `Display` and its suggestion
    // search both read the list, and it is fixed at compile time.
    static NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
        let mut names: Vec<&'static str> = FAMILIES
            .iter()
            .flat_map(|family| {
                std::iter::once(family.name).chain(family.rows.iter().map(|row| row.name))
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    });
    &NAMES
}

/// Resolve a metric selector to the [`Metric`] family that computes it.
///
/// A selector is any name `bca metrics --metrics` accepts:
///
/// - a family name (`loc`, `cyclomatic`), the [`Metric`] `Display`
///   spelling;
/// - a `bca list-metrics` row name (`sloc`, `dloc`, …), which selects
///   the family that owns the row;
/// - a dotted [`METRICS`] id (`cyclomatic.modified`, `loc.sloc`,
///   `mi.original`), which selects its family.
///
/// Selection is family-granular, as the walk computes whole families:
/// `"dloc"` and `"loc"` both select [`Metric::Loc`], and nothing narrows
/// the output to the named field. Matching is strict lowercase, like
/// [`Metric`]'s `FromStr`.
///
/// # Errors
///
/// [`UnknownSelectorError`] when `name` is none of the above. It carries
/// the closest [`selector_names`] as a did-you-mean hint.
pub fn resolve_selector(name: &str) -> Result<Metric, UnknownSelectorError> {
    FAMILIES
        .iter()
        .find(|family| family.name == name || family.rows.iter().any(|row| row.name == name))
        .map(|family| family.name)
        .or_else(|| lookup(name).map(|info| info.family))
        // Every family name is a `Metric` spelling
        // (`every_selector_name_resolves` pins it), so this drops nothing.
        .and_then(|family| family.parse::<Metric>().ok())
        .ok_or_else(|| UnknownSelectorError::new(name))
}

/// Error returned by [`resolve_selector`] for a name that is no metric
/// selector.
///
/// Carries the rejected input and the did-you-mean candidates, drawn
/// from [`selector_names`]. `Display` renders
/// `unknown metric: <input>[; did you mean …]; known metrics: <names>`,
/// the message the Python bindings raise; its exact wording is not
/// SemVer-protected. A front end that wants its own wording composes it
/// from [`input`](Self::input), [`suggestions`](Self::suggestions) and
/// [`selector_names`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSelectorError {
    input: String,
    suggestions: Vec<&'static str>,
}

impl UnknownSelectorError {
    fn new(input: &str) -> Self {
        Self {
            input: input.to_owned(),
            suggestions: closest_names(input, selector_names()),
        }
    }

    /// The rejected input, verbatim.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The [`selector_names`] closest to the input, best first; empty
    /// when nothing is close. At most three, all tied on closeness.
    #[must_use]
    pub fn suggestions(&self) -> &[&'static str] {
        &self.suggestions
    }
}

impl fmt::Display for UnknownSelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown metric: {}{}; known metrics: {}",
            self.input,
            render_suggestion(&self.suggestions),
            selector_names().join(", ")
        )
    }
}

impl std::error::Error for UnknownSelectorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn metric_ids_are_unique() {
        let mut seen = HashSet::new();
        for m in METRICS {
            assert!(seen.insert(m.id), "duplicate metric id {:?}", m.id);
        }
    }

    #[test]
    fn family_names_are_unique() {
        let mut seen = HashSet::new();
        for f in FAMILIES {
            assert!(seen.insert(f.name), "duplicate family name {:?}", f.name);
        }
    }

    #[test]
    fn every_metric_family_is_declared() {
        let families: HashSet<&str> = FAMILIES.iter().map(|f| f.name).collect();
        for m in METRICS {
            assert!(
                families.contains(m.family),
                "metric {:?} references undeclared family {:?}",
                m.id,
                m.family,
            );
        }
    }

    #[test]
    fn every_family_has_a_metric() {
        let metric_families: HashSet<&str> = METRICS.iter().map(|m| m.family).collect();
        for f in FAMILIES {
            assert!(
                metric_families.contains(f.name),
                "family {:?} has no METRICS entry",
                f.name,
            );
        }
    }

    #[test]
    fn lookup_round_trips_and_rejects_unknown() {
        for m in METRICS {
            assert_eq!(lookup(m.id).map(|i| i.id), Some(m.id));
        }
        assert!(lookup("not.a.metric").is_none());
    }

    /// `mi.*` is the only lower-is-worse family. This pins the data that
    /// replaced the former `is_lower_is_worse` prefix predicate; if the
    /// `Direction` of an `mi.*` row is flipped (or a non-`mi` row is
    /// marked `LowerIsWorse`), Code Climate's breach-ratio inversion
    /// silently flips with it.
    #[test]
    fn lower_is_worse_iff_mi_family() {
        for m in METRICS {
            let expect_lower = m.family == "mi";
            assert_eq!(
                matches!(m.direction, Direction::LowerIsWorse),
                expect_lower,
                "metric {:?} has the wrong direction",
                m.id,
            );
        }
    }

    // Pins the deprecated helper's contract until 3.0 removes it.
    #[allow(deprecated, clippy::disallowed_methods)]
    #[test]
    fn lower_is_worse_helper_matches_catalog_and_defaults_false() {
        assert!(lower_is_worse("mi.original"), "mi.* is lower-is-worse");
        assert!(
            !lower_is_worse("cyclomatic"),
            "cyclomatic is higher-is-worse"
        );
        // An id the catalog does not know defaults to higher-is-worse, so
        // the shared gate never flags an unknown metric on the wrong side.
        assert!(!lower_is_worse("not_a_metric"));
    }

    /// `mi.*` sentences phrase the breach as "falls below"; every other
    /// metric phrases it as "exceeds"/"exceed". This pins the wording to
    /// the direction so a copy-paste sentence with the wrong verb is
    /// caught.
    #[test]
    fn sentence_phrasing_matches_direction() {
        for m in METRICS {
            match m.direction {
                Direction::LowerIsWorse => assert!(
                    m.long_description.contains("falls below"),
                    "{:?} should use `falls below`: {:?}",
                    m.id,
                    m.long_description,
                ),
                Direction::HigherIsWorse => assert!(
                    m.long_description.contains("exceed"),
                    "{:?} should use `exceed(s)`: {:?}",
                    m.id,
                    m.long_description,
                ),
            }
        }
    }

    /// `skip_at_unit` is `true` for exactly the metrics whose serialized
    /// JSON headline at the file-level `unit` space is an aggregate over
    /// descendant spaces that does not match the CLI threshold
    /// accessor's per-space scalar (#441).
    ///
    /// Since #958 the flag has no consumer: both front-ends read a
    /// per-space `value` field instead, and
    /// `big-code-analysis-py/src/sarif.rs` pins its own JSON paths
    /// (`metric_field_paths_are_pinned`) rather than deriving them from
    /// this flag. So what this enumeration buys is a review tripwire,
    /// not a derived contract — which is exactly what it failed to be
    /// between #1196 and #1236: #1196 moved the `nargs` gate from
    /// `total()` (a subtree sum) to the callable's own parameter list
    /// while `nargs.total` kept its meaning, and the row here stayed
    /// `false`, leaving the registry asserting a parity that no longer
    /// held. Editing the set means re-deriving it against the CLI
    /// `EXTRACTORS` accessors, one row at a time.
    ///
    /// The property is deliberately enumerated rather than derived from
    /// the id string: `nexits` also serialises a `sum` field but reads
    /// that same aggregate via its CLI accessor, so it does not diverge.
    #[test]
    fn skip_at_unit_is_the_sum_vs_per_space_divergence_set() {
        let mut skip: Vec<&str> = METRICS
            .iter()
            .filter(|m| m.skip_at_unit)
            .map(|m| m.id)
            .collect();
        skip.sort_unstable();
        assert_eq!(
            skip,
            [
                "abc",
                "cognitive",
                "cyclomatic",
                "cyclomatic.modified",
                "nargs"
            ],
            "skip_at_unit set drifted from the JSON-aggregate-vs-CLI-accessor \
             property; review against the CLI EXTRACTORS accessors before editing",
        );
    }

    /// The per-metric [`MetricScope`] partition (#969): `loc.*` gates the
    /// file root, the OO size metrics gate containers, everything else
    /// gates leaf functions. Enumerated so a new metric must be placed
    /// deliberately rather than defaulting silently — both the CLI gate
    /// and the Python binding derive their kind-filtering from this.
    #[test]
    fn scope_partitions_metrics_by_measured_kind() {
        let by_scope = |want: MetricScope| {
            let mut ids: Vec<&str> = METRICS
                .iter()
                .filter(|m| m.scope == want)
                .map(|m| m.id)
                .collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(
            by_scope(MetricScope::File),
            [
                "loc.blank",
                "loc.cloc",
                "loc.dloc",
                "loc.lloc",
                "loc.ploc",
                "loc.sloc"
            ],
            "only the loc.* size family is File-scoped",
        );
        assert_eq!(
            by_scope(MetricScope::Container),
            ["nom", "npa", "npm", "wmc"],
            "only the OO size metrics are Container-scoped",
        );
        // Everything else is per-function; spot-check the representatives
        // and confirm the partition is total (no metric left unscoped).
        for id in [
            "cognitive",
            "cyclomatic",
            "halstead.effort",
            "nargs",
            "nexits",
            "abc",
            "mi.original",
        ] {
            assert_eq!(
                scope(id),
                Some(MetricScope::Function),
                "{id} should be Function-scoped"
            );
        }
        let counted = by_scope(MetricScope::File).len()
            + by_scope(MetricScope::Function).len()
            + by_scope(MetricScope::Container).len();
        assert_eq!(
            counted,
            METRICS.len(),
            "every metric must have exactly one scope"
        );
    }

    /// Exactly the three `mi.*` rows are interval-scale (#1621), each
    /// with a finite positive unit and an ordered domain; every count is
    /// ratio-scale.
    #[test]
    #[allow(clippy::float_cmp)] // The derived units are exact constants.
    fn scale_is_interval_for_exactly_the_mi_family() {
        let mut interval: Vec<&str> = Vec::new();
        for m in METRICS {
            match m.scale {
                Scale::Interval { min, max, unit } => {
                    assert_eq!(m.family, "mi", "{} is interval-scale", m.id);
                    assert!(unit.is_finite() && unit > 0.0, "{} unit {unit}", m.id);
                    assert!(min <= max, "{} domain [{min}, {max}]", m.id);
                    interval.push(m.id);
                }
                Scale::Ratio => assert_ne!(m.family, "mi", "{} is ratio-scale", m.id),
            }
        }
        interval.sort_unstable();
        assert_eq!(interval, ["mi.original", "mi.sei", "mi.visual_studio"]);

        // One Visual Studio band (10 points) on the 171-point scale.
        assert_eq!(MI_ORIGINAL_UNIT, 17.1);
        for id in ["mi.original", "mi.sei"] {
            assert_eq!(
                scale(id),
                Some(Scale::Interval {
                    min: f64::NEG_INFINITY,
                    max: f64::INFINITY,
                    unit: 17.1
                }),
                "{id}"
            );
        }
        assert_eq!(
            scale("mi.visual_studio"),
            Some(Scale::Interval {
                min: 0.0,
                max: 100.0,
                unit: 10.0
            })
        );
    }

    #[test]
    fn cmp_worse_orients_by_direction() {
        let cyc = lookup("cyclomatic").expect("known");
        assert_eq!(cyc.cmp_worse(20.0, 10.0), Ordering::Greater);
        assert_eq!(cyc.cmp_worse(10.0, 20.0), Ordering::Less);
        assert_eq!(cyc.cmp_worse(3.0, 3.0), Ordering::Equal);

        let mi = lookup("mi.original").expect("known");
        assert_eq!(mi.cmp_worse(-20.0, 10.0), Ordering::Greater);
        assert_eq!(mi.cmp_worse(10.0, -20.0), Ordering::Less);
        // Signed zeros are one value, as they are to `breaches`: an MI of
        // `-0.0` against a record of `0.0` is not a regression.
        for info in [cyc, mi] {
            assert_eq!(info.cmp_worse(-0.0, 0.0), Ordering::Equal, "{}", info.id);
            assert_eq!(info.cmp_worse(0.0, -0.0), Ordering::Equal, "{}", info.id);
        }
    }

    /// NaN sorts as the worst value in either direction, so a sort by
    /// `cmp_worse` puts a NaN measurement with the regressions.
    #[test]
    fn cmp_worse_ranks_nan_worst_in_both_directions() {
        for id in ["cyclomatic", "mi.original"] {
            let info = lookup(id).expect("known");
            for finite in [f64::NEG_INFINITY, -5.0, 0.0, 1e9, f64::INFINITY] {
                assert_eq!(info.cmp_worse(f64::NAN, finite), Ordering::Greater, "{id}");
                assert_eq!(info.cmp_worse(finite, f64::NAN), Ordering::Less, "{id}");
            }
            assert_eq!(info.cmp_worse(f64::NAN, -f64::NAN), Ordering::Equal, "{id}");
            let mut values = [3.0, f64::NAN, -2.0, 7.0];
            values.sort_by(|a, b| info.cmp_worse(*a, *b));
            assert!(values[3].is_nan(), "{id}: {values:?}");
        }
    }

    /// [`MetricScope::admits`] gates exactly the intended kinds: File only
    /// the `Unit` root, Function only `Function`, Container the
    /// method-owning kinds — and nothing admits `Unknown`.
    #[test]
    fn scope_admits_only_its_kinds() {
        assert!(MetricScope::File.admits(SpaceKind::Unit));
        assert!(!MetricScope::File.admits(SpaceKind::Function));
        assert!(!MetricScope::File.admits(SpaceKind::Class));

        assert!(MetricScope::Function.admits(SpaceKind::Function));
        assert!(!MetricScope::Function.admits(SpaceKind::Unit));
        assert!(!MetricScope::Function.admits(SpaceKind::Impl));

        for kind in [
            SpaceKind::Class,
            SpaceKind::Struct,
            SpaceKind::Trait,
            SpaceKind::Impl,
            SpaceKind::Namespace,
            SpaceKind::Interface,
        ] {
            assert!(
                MetricScope::Container.admits(kind),
                "{kind:?} is a container"
            );
        }
        assert!(!MetricScope::Container.admits(SpaceKind::Unit));
        assert!(!MetricScope::Container.admits(SpaceKind::Function));

        for scope in [
            MetricScope::File,
            MetricScope::Function,
            MetricScope::Container,
        ] {
            assert!(
                !scope.admits(SpaceKind::Unknown),
                "{scope:?} must not admit Unknown"
            );
        }
    }

    /// The advertised vocabulary, pinned whole: the thirteen family names
    /// plus the six `loc` row names, which is also the list the CLI's
    /// `--metrics` error has printed since #1597 added `dloc`.
    #[test]
    fn selector_names_are_families_plus_rows() {
        assert_eq!(
            selector_names(),
            [
                "abc",
                "blank",
                "cloc",
                "cognitive",
                "cyclomatic",
                "dloc",
                "halstead",
                "lloc",
                "loc",
                "mi",
                "nargs",
                "nexits",
                "nom",
                "npa",
                "npm",
                "ploc",
                "sloc",
                "tokens",
                "wmc",
            ]
        );
    }

    /// No advertised name is rejected: the resolver's `parse` step would
    /// drop a catalog family with no `Metric` variant, and this is what
    /// says so.
    #[test]
    fn every_selector_name_resolves() {
        for name in selector_names() {
            assert!(resolve_selector(name).is_ok(), "{name} must resolve");
        }
    }

    #[test]
    fn resolve_selector_takes_family_names() {
        assert_eq!(resolve_selector("loc"), Ok(Metric::Loc));
        assert_eq!(resolve_selector("cyclomatic"), Ok(Metric::Cyclomatic));
        for name in Metric::NAMES {
            assert_eq!(
                resolve_selector(name).map(|m| m.to_string()),
                Ok((*name).to_owned())
            );
        }
    }

    /// A `list-metrics` row name selects the family that owns the row,
    /// so every `loc` sub-measurement selects the whole `loc` family.
    #[test]
    fn resolve_selector_maps_row_names_to_their_family() {
        for row in ["sloc", "ploc", "lloc", "cloc", "blank", "dloc"] {
            assert_eq!(resolve_selector(row), Ok(Metric::Loc), "{row}");
        }
    }

    /// A dotted threshold id selects its family. None of these is a
    /// family or row name, so only the `lookup` fallback can answer.
    #[test]
    fn resolve_selector_maps_dotted_ids_to_their_family() {
        assert_eq!(
            resolve_selector("cyclomatic.modified"),
            Ok(Metric::Cyclomatic)
        );
        assert_eq!(resolve_selector("halstead.volume"), Ok(Metric::Halstead));
        assert_eq!(resolve_selector("mi.original"), Ok(Metric::Mi));
        assert_eq!(resolve_selector("loc.dloc"), Ok(Metric::Loc));
        for info in METRICS {
            assert_eq!(
                resolve_selector(info.id).map(|m| m.to_string()),
                Ok(info.family.to_owned()),
                "{}",
                info.id
            );
        }
    }

    #[test]
    fn unknown_selector_suggests_the_closest_name() {
        let err = resolve_selector("dlocc").expect_err("not a selector");
        assert_eq!(err.input(), "dlocc");
        assert_eq!(err.suggestions(), ["dloc"]);
        assert_eq!(
            err.to_string(),
            "unknown metric: dlocc; did you mean `dloc`?; known metrics: abc, blank, cloc, \
             cognitive, cyclomatic, dloc, halstead, lloc, loc, mi, nargs, nexits, nom, npa, \
             npm, ploc, sloc, tokens, wmc"
        );
    }

    /// Two names tied on closeness are both offered.
    #[test]
    fn unknown_selector_lists_tied_suggestions() {
        let err = resolve_selector("npz").expect_err("not a selector");
        assert_eq!(err.suggestions(), ["npa", "npm"]);
        assert!(
            err.to_string()
                .starts_with("unknown metric: npz; did you mean one of `npa`, `npm`?; known"),
            "{err}"
        );
    }

    #[test]
    fn unknown_selector_without_a_close_name_omits_the_hint() {
        let err = resolve_selector("xyzq").expect_err("not a selector");
        assert_eq!(err.suggestions(), [] as [&str; 0]);
        assert_eq!(
            err.to_string(),
            format!(
                "unknown metric: xyzq; known metrics: {}",
                selector_names().join(", ")
            )
        );
    }

    /// Strict lowercase, as `Metric::from_str` is: a capitalised family
    /// name is rejected, and the hint names the spelling that works.
    #[test]
    fn resolve_selector_is_case_sensitive() {
        let err = resolve_selector("Loc").expect_err("case-sensitive");
        assert_eq!(err.suggestions(), ["loc"]);
    }
}
