//! Per-metric implementations.
//!
//! Each submodule defines one maintainability metric, its per-language
//! traits, and its `Stats` accumulator. See the crate-level docs for an
//! overview of the metric suite.

use crate::SpaceKind;

/// Assignment / Branch / Condition counts.
pub mod abc;
/// Cognitive complexity.
pub mod cognitive;
/// Cyclomatic complexity.
pub mod cyclomatic;
/// Halstead suite (operators, operands, volume, difficulty, effort).
pub mod halstead;
/// Lines-of-code variants (SLOC, PLOC, LLOC, CLOC, blank).
pub mod loc;
/// Maintainability Index.
pub mod mi;
/// Number of arguments per function.
pub mod nargs;
/// Exit-point counting.
pub mod nexits;
/// Number of methods (functions + closures).
pub mod nom;
/// Number of public attributes.
pub mod npa;
/// Number of public methods.
pub mod npm;
/// Token count.
pub mod tokens;
/// Weighted Methods per Class.
pub mod wmc;

// Gated on the union of the features its `FIXTURES` rows carry, so a
// build enabling none of them drops the module rather than tripping
// `assert_fixtures_present` — a failure that reads as a defect in
// whatever was being changed (`.claude/rules/testing.md`, #1286). The
// tests the module already gates individually all name a subset of this
// list.
#[cfg(test)]
#[cfg(any(
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "typescript"
))]
#[path = "container_scope_tests.rs"]
mod container_scope_tests;

/// Divides a metric sum by a count, guarding the divisor with `.max(1)`.
///
/// Every "average over a count" metric routes through this helper so the
/// divide-by-zero guard added for [#428] is applied uniformly rather than
/// per call site. A `count` of `0` degrades to `sum / 1` (the sum itself)
/// instead of producing `inf`/`NaN`, so a never-observed or count-less
/// space still serializes a finite number.
///
/// The *meaning* of `count` is the caller's choice of denominator
/// convention; the project uses two:
///
/// - **Per-function** averages (`cognitive`, `cyclomatic`, `exit`,
///   `nargs`) divide by the function/closure count of the subtree, so the
///   value reads as "average complexity per function". `cognitive`/
///   `exit`/`nargs` source this count from `Nom`; `cyclomatic` counts its
///   own function/closure *spaces* (equal in the common case, but
///   independent of whether `Nom` is selected — see
///   `cyclomatic::Stats::function_spaces`).
/// - **Per-space** averages (`nom`, `loc`, `abc`, `tokens`) divide by the
///   total number of spaces (functions, closures, classes, the file unit,
///   …). These measure a property of each space rather than of each
///   function, so a per-function denominator would not match their
///   meaning (and for `nom` it would be circular — it *is* the function
///   count).
///
/// [#428]: https://github.com/dekobon/big-code-analysis/issues/428
#[inline]
#[must_use]
// `count as f64` is exact for any realistic space count; the cast mirrors
// the per-metric modules' module-level allowance for count-to-float casts.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn average(sum: f64, count: usize) -> f64 {
    sum / count.max(1) as f64
}

/// Whether the object-oriented member metrics — `wmc`, `npm`, `npa` —
/// are meaningful on a space of a given [`SpaceKind`].
///
/// An extension trait rather than an inherent method because
/// `SpaceKind` is defined in `big-code-analysis-ast`, which knows
/// nothing about metrics: "does `wmc` apply here" is a question only
/// this crate can ask, so it is answered here (#1376).
pub(crate) trait MemberScopeExt {
    /// True for every container (which owns methods and attributes) and
    /// for the file [`SpaceKind::Unit`] (which aggregates its
    /// containers' counts into a whole-file roll-up). False for a
    /// function space, which owns neither, and for
    /// [`SpaceKind::Unknown`].
    ///
    /// This is the single definition the three metrics share. `wmc`
    /// carried it alone as an inline `matches!`; `npm` and `npa` enabled
    /// themselves from `Checker::is_func_space` instead, which admits
    /// function spaces and so gave a Kotlin `<get>` or a JavaScript
    /// method an all-zero block its sibling method did not have (#1197).
    ///
    /// Phrased as an exclusion so that a future [`SpaceKind`] variant —
    /// the enum is `#[non_exhaustive]` — defaults to *carrying* the
    /// metrics. A new kind is far likelier to be another container than
    /// another callable, and an extra roll-up is a milder wrong answer
    /// than a silently missing one.
    ///
    /// Takes `&self` rather than a by-value `Copy`: the receiver is
    /// cost-free either way, and a trait cannot tell
    /// `clippy::wrong_self_convention` that every implementor is `Copy`,
    /// so by-value would need a carve-out that buys nothing.
    fn is_member_scope(&self) -> bool;
}

impl MemberScopeExt for SpaceKind {
    #[inline]
    fn is_member_scope(&self) -> bool {
        !matches!(*self, SpaceKind::Function | SpaceKind::Unknown)
    }
}
