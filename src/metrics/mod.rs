//! Per-metric implementations.
//!
//! Each submodule defines one maintainability metric, its per-language
//! traits, and its `Stats` accumulator. See the crate-level docs for an
//! overview of the metric suite.

use crate::{Node, SpaceKind};

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

#[cfg(test)]
#[path = "bash_test_connective_tests.rs"]
mod bash_test_connective_tests;
// Gated on the union of every feature any test in the module names —
// the `FIXTURES` rows plus the `bash` / `lua` / `c` row set that
// `a_language_with_no_member_construct_emits_neither_block` uses. The
// narrower `FIXTURES` union belongs on the four tests that read
// `FIXTURES`, not here: a module gate is an outer `cfg`, so naming only
// the `FIXTURES` features would delete that Bash/Lua/C test from a build
// enabling exactly the languages it exists to cover
// (`docs/development/testing.md`, #1286).
#[cfg(test)]
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
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
#[cfg(test)]
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
#[path = "cpp_alternative_tokens_tests.rs"]
mod cpp_alternative_tokens_tests;
#[cfg(test)]
#[path = "elixir_named_operator_tests.rs"]
mod elixir_named_operator_tests;
#[cfg(test)]
#[path = "elixir_spelling_parity_tests.rs"]
mod elixir_spelling_parity_tests;
// Metric-arm parity derived from the role tables (#1700): the
// root-crate half of `roles/derived_parity_tests.rs`. A full-enum sweep
// with an `is_enabled()` filter and a non-vacuity assertion, so absent
// rather than failing with no language enabled; every item inherits.
// test-lang-gates: hand-written(bash,c,cpp,csharp,elixir,go,groovy,irules,java,javascript,kotlin,lua,mozcpp,mozjs,objc,perl,php,python,ruby,rust,tcl,typescript)
//     — the sweep dispatches on `LANG` through a macro naming every
//     `Code` tag and filters at run time, so it needs the union
#[cfg(all(
    test,
    any(
        feature = "bash",
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "elixir",
        feature = "go",
        feature = "groovy",
        feature = "irules",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "lua",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "perl",
        feature = "php",
        feature = "python",
        feature = "ruby",
        feature = "rust",
        feature = "tcl",
        feature = "typescript",
    )
))]
#[path = "derived_parity_tests.rs"]
mod derived_parity_tests;
#[cfg(test)]
#[path = "function_population_tests.rs"]
mod function_population_tests;
#[cfg(test)]
#[path = "perl_keyword_key_tests.rs"]
mod perl_keyword_key_tests;

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
/// - **Per-function** averages (`cognitive`, `cyclomatic`, `nexits`,
///   `nargs`) divide by a function count, so the value reads as "average
///   complexity per function". `cognitive`, `cyclomatic` and `nexits`
///   count the [`SpaceKind::Function`] spaces of the subtree — the same
///   population their numerator and their `min` / `max` fold, so the
///   three agree (#1284) — independently of whether `Nom` is selected.
///   `nargs` keeps two channels, functions and closures, and sources
///   both counts from `Nom`.
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

/// A value a [`FunctionPopulation`] folds: the metric's own-value type.
pub(crate) trait PopulationValue: Copy + Default + PartialOrd + std::ops::AddAssign {
    fn to_f64(self) -> f64;
}

impl PopulationValue for f64 {
    fn to_f64(self) -> f64 {
        self
    }
}

impl PopulationValue for usize {
    #[allow(clippy::cast_precision_loss)]
    fn to_f64(self) -> f64 {
        self as f64
    }
}

/// The per-function aggregate of one metric: numerator, divisor and
/// extrema folded over the same population — the
/// [`SpaceKind::Function`][crate::SpaceKind] spaces of a subtree — so
/// `min <= average <= max` holds by construction (#1284). One `fold`
/// per function space, one `merge` per child; the all-space `sum` the
/// metrics also publish is kept apart, in the metric's own field. An
/// empty population has no `min`, and reports `0` for it (#428).
///
/// This is the one place the rule is spelled in the tree; the book's
/// "Aggregate population" section under Cyclomatic Complexity is the
/// user-facing definition and the accessors link here. What it
/// replaced: the numerator and extrema folded every space while the
/// divisor counted functions, so `fn f() {}` reported cyclomatic
/// `average 2.0` against `min 1, max 1` and the unit's own `0` pinned
/// file-level cognitive `min`; and cognitive / nexits divided by
/// `nom.total()`, which counts a closure that opens no space (a Python
/// `lambda`, whose increments are part of the enclosing function's
/// own value), so a function holding one could report `average 7.0`
/// against `min 14, max 14`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct FunctionPopulation<T: PopulationValue> {
    sum: T,
    count: usize,
    min: Option<T>,
    max: T,
}

impl<T: PopulationValue> FunctionPopulation<T> {
    /// Folds one function space's own value.
    #[inline]
    pub(crate) fn fold(&mut self, own: T) {
        self.sum += own;
        self.count += 1;
        self.min = Some(
            self.min
                .map_or(own, |min| if own < min { own } else { min }),
        );
        if own > self.max {
            self.max = own;
        }
    }

    /// Merges a child subtree's population into this one.
    #[inline]
    pub(crate) fn merge(&mut self, other: &Self) {
        self.sum += other.sum;
        self.count += other.count;
        self.min = match (self.min, other.min) {
            (Some(a), Some(b)) => Some(if b < a { b } else { a }),
            (a, b) => a.or(b),
        };
        if other.max > self.max {
            self.max = other.max;
        }
    }

    /// `sum / count`, `0.0` for an empty population (#428).
    #[inline]
    pub(crate) fn average(&self) -> f64 {
        average(self.sum.to_f64(), self.count)
    }

    /// The smallest own value, or the type's zero for an empty population.
    #[inline]
    pub(crate) fn min(&self) -> T {
        self.min.unwrap_or_default()
    }

    /// The largest own value, the type's zero for an empty population.
    #[inline]
    pub(crate) fn max(&self) -> T {
        self.max
    }
}

/// How far the C++ `requires` clause or `requires` expression the walk
/// is inside reaches, so `cyclomatic`, `cognitive` and `abc` can leave
/// it unscored in `O(1)` per node.
///
/// A constraint is checked at compile time, and a requires-expression's
/// requirements are never evaluated at all, so neither branches. The
/// grammar parses `requires A<T> && B<T>` as a `constraint_conjunction`,
/// which no metric arm matches, but `requires (A<T> && B<T>)` as an
/// ordinary `binary_expression`. Asking each operator whether it sits in
/// a clause by climbing to one made a long `&&` chain quadratic (#1533),
/// so the answer is carried down the walk instead.
///
/// It is exact because the walk is pre-order: a clause is visited before
/// its descendants, and every node visited after the clause starts
/// before the clause's end byte if and only if it is a descendant. A
/// nested clause ends within the outer one, so the outer reach already
/// covers it. Each space keeps its own reach in its metric `Stats`;
/// C++ lambdas open no space, so a lambda written in a constraint
/// scores nothing either. Never serialized or merged: it is walk state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConstraintReach {
    end_byte: usize,
}

impl ConstraintReach {
    /// Whether `node` lies in a constraint, recording the reach of the
    /// one `node` opens when `opens_constraint` says it is a
    /// `requires_clause` or `requires_expression`.
    #[inline]
    pub(crate) fn covers(&mut self, node: &Node, opens_constraint: bool) -> bool {
        if node.start_byte() < self.end_byte {
            return true;
        }
        if opens_constraint {
            self.end_byte = node.end_byte();
        }
        opens_constraint
    }
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
