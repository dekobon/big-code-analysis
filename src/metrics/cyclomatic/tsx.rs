//! `Cyclomatic` implementation for TSX.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/tsx.rs`, read at the projection
// the metric counts (#1695, #1697): `&&` / `||` from `ShortCircuit` at
// the token; the ternary from `Ternary` at its wrapper; `??` and the
// optional chain's `?.` from `NullSafe` at the token — one decision
// per occurrence, the token in every grammar (`optional_chain` itself in
// the JavaScript grammars, `?.` under the `optional_chain` wrapper in
// the TypeScript ones, #281); the `if` / `for` / `while` / `catch`
// *keyword tokens* from `Branch` / `Loop` / `Catch` (#284: the `while`
// of a `do … while` and the `for` of a `for … of` each fire once); the
// `case` label from `CaseArm` at the token, its row excluding
// `default`; and the `switch_statement` from `Switch` at the owner,
// the modified tier's container. The compound short-circuit assignments
// `&&=` / `||=` / `??=` — `x = x op y`, one decision edge each (#226,
// #231, #248) — are excluded on the rows as the consumers' own and are
// the dialect's `extra`.
impl_cyclomatic_rows!(
    TsxCode,
    Tsx,
    roles = crate::roles::tsx,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        null_safe::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::owns_kind],
    extra = [AMPAMPEQ, PIPEPIPEEQ, QMARKQMARKEQ],
    constraints = [],
);
