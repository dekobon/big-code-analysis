//! Per-metric implementations.
//!
//! Each submodule defines one maintainability metric, its per-language
//! traits, and its `Stats` accumulator. See the crate-level docs for an
//! overview of the metric suite.

use crate::checker::Checker;
use crate::getter::Getter;
use crate::node::Ancestors;
use crate::node::Node;

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

/// Whether `node` opens a space at which the object-oriented member
/// metrics are meaningful — a container, or the file unit that rolls its
/// containers up. See [`SpaceKind::is_member_scope`].
///
/// [`Checker::is_func_space`] answers a strictly wider question — "does
/// this node open a space at all" — and stood in for this one until the
/// gap showed. It lists ordinary methods in the JS family, C#, PHP and
/// Ruby; [#1184] then added Kotlin property accessors and `init { … }` /
/// `static { … }` blocks. Each of those function spaces grew an all-zero
/// `npm` / `npa` block that the plain method beside it did not have
/// ([#1197]).
///
/// Classifies through the **source-aware** [`Checker::is_func_space_with_code`]
/// and [`Getter::get_space_kind_with_code`], because those are what the
/// walker itself uses to build and label the space tree
/// (`spaces::compute`). The byte-less forms disagree for Elixir, whose
/// `defmodule` is a `Class` only to `get_space_kind_with_code` — routing
/// Elixir through here with the plain forms would silently emit nothing
/// for every module, with no test or diagnostic to catch it.
///
/// The `is_func_space_with_code` conjunct is a precondition rather than a
/// live filter: no language currently classifies a member scope on a node
/// that opens no space. It keeps the flag pinned to the space's own root
/// node, so a container kind appearing on a non-space node could never
/// enable the *enclosing* space by accident.
///
/// [#1184]: https://github.com/dekobon/big-code-analysis/issues/1184
/// [#1197]: https://github.com/dekobon/big-code-analysis/issues/1197
#[inline]
fn opens_member_scope<'a, L: Checker + Getter>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    L::is_func_space_with_code(node, code, ancestors)
        && L::get_space_kind_with_code(node, code, ancestors).is_member_scope()
}
