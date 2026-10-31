//! `Cyclomatic` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/ruby.rs`, read at the projection the
// metric counts (#1695, #1697). Ruby counts statement nodes, so `Branch`
// / `Loop` / `Catch` / `Switch` are plain rows read at the owner — the
// block forms and the statement-modifier forms (`x if a`, `x while a`,
// `x rescue y`) each their own kind — and the `elsif` clause, the
// `ElseIf` row's self-paying owner, pays as the branch it is. `&&` /
// `||` / `and` / `or` from `ShortCircuit` at the token; the
// `conditional` from `Ternary` at its wrapper; a `when` / `in` clause's
// guard from `Guard` at the owner; the safe-navigation `&.` from
// `NullSafe` at the token; each `when` and `in` clause from `CaseArm` at
// the owner (the row leaves the `else` out); `case` and `case … in`
// from `Switch`, the modified tier's.
impl_cyclomatic_rows!(
    RubyCode,
    Ruby,
    roles = crate::roles::ruby,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        guard::owns_kind,
        null_safe::pays_at_leaf_kind,
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
