//! `Cyclomatic` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/bash.rs`, read at the projection the
// metric counts (#1695, #1697). Bash counts statement nodes, so `Branch`
// / `Loop` are plain rows read at the owner, and the `elif` clause —
// the `ElseIf` row's self-paying owner — pays as the branch it is.
// `&&` / `||`, and the `-a` / `-o` that `[ … ]` spells them as — a
// decision each like the `[ … ] && [ … ]` and `[[ … && … ]]` twins
// (#1536) — from `ShortCircuit` at the token, the test spelling read for
// its bytes by the row; the arithmetic ternary inside `(( … ))` /
// `$(( … ))` — the only ternary form Bash has, a decision in every
// sibling that has one (#1268) — from `Ternary` at its owner; each
// `case` arm from `CaseArm` at the owner — the bare-wildcard `*)` is
// Bash's `default:` and the row leaves it out, a multi-value pattern
// (`a|b)`) still counts (#211); the `case … esac` container from
// `Switch`, the modified tier's.
impl_cyclomatic_rows!(
    BashCode,
    Bash,
    roles = crate::roles::bash,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        else_if::pays_at_leaf_kind,
    ],
    standard = [case_arm::owns_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [],
);
