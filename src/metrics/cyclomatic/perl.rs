//! `Cyclomatic` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/perl.rs`, read at the projection
// the metric counts (#1695, #1697). Perl counts statement *nodes*, not
// keywords, so the `Branch` / `Loop` / `ElseIf` rows are plain (their
// owners are their leaves) and the block forms and the statement-
// modifier forms (`return 1 if $c;`, `$a++ while $c;`, `print for
// @l;`) are each one decision at their own kind.
//
// - `&&` / `||` / `or`, and an `and` that is the operator rather than
//   an auto-quoted key (#1539): `ShortCircuit`'s payers, at the leaf.
// - The `ternary_expression` from `Ternary` at its wrapper — not its
//   ungated `?`, which recovery can strand (`my $x = a ?;`).
// - The `when` statement from `CaseArm` at its **owner**, scoring
//   **both** tiers: Perl's `given` is not a production of the pinned
//   grammar (its `Switch` row is absent, with the dump that shows why),
//   so there is no container for the modified count to collapse the
//   arms onto, and `when_simple_statement` — reached only by error
//   recovery at this pin — pays as the branch it is. Perl is the only
//   language that reads `CaseArm` in `both`; Lua, the other language
//   without a switch, has no `CaseArm` row to read.
// - The defined-or `//` from `NullSafe` at its leaf.
// - The `elsif` clause from `ElseIf` at its leaf: the grammar gives it
//   a node of its own, a direct child of the `if_statement`, so the
//   clause pays for itself (the row marks it `pays_itself`).
// - The compound short-circuit assignments `&&=`, `||=`, `//=` — the
//   rows exclude them as the consumers' own — are the dialect's
//   `extra`: each is `x = x op y`, one decision edge, parallel to the
//   JS-family fix in #248 (#249).
impl_cyclomatic_rows!(
    PerlCode,
    Perl,
    roles = crate::roles::perl,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        null_safe::pays_at_leaf_kind,
        case_arm::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        else_if::pays_at_leaf_kind,
    ],
    standard = [],
    modified = [],
    extra = [AMPAMPEQ, PIPEPIPEEQ, SLASHSLASHEQ],
    constraints = [],
);
