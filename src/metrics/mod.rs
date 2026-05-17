//! Per-metric implementations.
//!
//! Each submodule defines one maintainability metric, its per-language
//! traits, and its `Stats` accumulator. See the crate-level docs for an
//! overview of the metric suite.

/// Assignment / Branch / Condition counts.
pub mod abc;
/// Cognitive complexity.
pub mod cognitive;
/// Cyclomatic complexity.
pub mod cyclomatic;
/// Exit-point counting.
pub mod exit;
/// Halstead suite (operators, operands, volume, difficulty, effort).
pub mod halstead;
/// Lines-of-code variants (SLOC, PLOC, LLOC, CLOC, blank).
pub mod loc;
/// Maintainability Index.
pub mod mi;
/// Number of arguments per function.
pub mod nargs;
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
