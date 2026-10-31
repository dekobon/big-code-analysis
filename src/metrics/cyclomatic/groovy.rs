//! `Cyclomatic` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/groovy.rs`, read at the projection
// the metric counts (#1694, #1695, #1697). `&&` / `||` from
// `ShortCircuit` at the token; the ternary from `Ternary` at its
// wrapper; `Guard` is absent and not read; the Elvis `?:`, the
// safe-navigation `?.` / `??.` and the safe-subscript `?[` from
// `NullSafe` at the token — each a short-circuit decision, one per
// textual operator (so `a?.b?.c` is +2; #246, #281, #452, #1471), which
// the dekobon grammar gives real lexer tokens where amaanq's emitted
// ERROR nodes; the `if` / `for` / `while` / `catch` keyword tokens from
// `Branch` / `Loop` / `Catch` (#284); the `case` label from `CaseArm` at
// the token, its row excluding `default`; the `switch` keyword from
// `Switch` at the token, once per switch (#300). `assert` is the one
// extra: a runtime check that branches on its condition, counted as
// Sonar's standard CCN does.
impl_cyclomatic_rows!(
    GroovyCode,
    Groovy,
    roles = crate::roles::groovy,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        null_safe::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::pays_at_leaf_kind],
    extra = [Assert],
    constraints = [],
);
