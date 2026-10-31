//! `Cyclomatic` implementation for iRules.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/irules.rs`, read at the projection
// the metric counts (#1695, #1697). Unlike Tcl, iRules gives `if` /
// `switch` / `for` / `foreach` / `while` / `dict for` dedicated kinds,
// so `Branch` / `Loop` / `Catch` / `Switch` are plain rows read at the
// owner — a `try`'s `on error` / `trap` handler groups are wrapped in
// dedicated nodes and each is one decision (#1266), matching `catch`
// and the C-family `CatchClause`; `try` and `finally` stay free — and
// the `elseif` node, the `ElseIf` row's self-paying owner, pays as the
// branch it is. The short-circuit operators, symbolic and the iRules
// keyword forms (`and` / `or`), from `ShortCircuit` at the token; the
// `ternary_expr` from `Ternary` at its wrapper; each `switch_arm` but
// `default` from `CaseArm` at the owner; the `switch` from `Switch`,
// the modified tier's.
impl_cyclomatic_rows!(
    IrulesCode,
    Irules,
    roles = crate::roles::irules,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
        else_if::pays_at_leaf_kind,
    ],
    standard = [case_arm::owns_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [],
);
