//! `Cyclomatic` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/php.rs`, read at the projection the
// metric counts (#1695, #1697). PHP counts statement nodes, so `Branch`
// / `Loop` / `Catch` / `Switch` are plain rows read at the owner, and
// the one-word `elseif` clause (both aliases) — the `ElseIf` row's
// self-paying owners — pays as the branch it is; the two-word `else if`
// is the row's third owner, an `if_statement` under an `else_clause`,
// which pays nothing here because its own statement is `Branch`'s.
// `&&` / `||` / `and` / `or` from `ShortCircuit` at the token; the
// `conditional_expression` from `Ternary` at its wrapper; `??` and the
// nullsafe `?->` from `NullSafe` at the token; each `case` from
// `CaseArm` at the owner (the row leaves `default` out); `switch` and
// the PHP 8 `match` from `Switch`, the modified tier's. The compound
// `??=` is excluded on the row as the consumers' own and is the one
// extra.
impl_cyclomatic_rows!(
    PhpCode,
    Php,
    roles = crate::roles::php,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        null_safe::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
        else_if::pays_at_leaf_kind,
    ],
    standard = [case_arm::owns_kind],
    modified = [switch::owns_kind],
    extra = [QMARKQMARKEQ],
    constraints = [],
);
