//! `Cyclomatic` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::bash::{case_arm, short_circuit, ternary};
        let kind = node.kind_id();
        match kind.into() {
            // Standard-only: each case arm the `CaseArm` row owns (matches
            // C-family `case:` treatment — only arms contribute, not the
            // container). The bare-wildcard arm `*)` is Bash's analogue of
            // the C-family `default:` and the row leaves it out, matching
            // every other switch-bearing language; a multi-value pattern
            // (`a|b)`, `*|b)`) is not bare and still counts. Closes #211.
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the case…esac container collapses all arms
            // into one decision point.
            Bash::CaseStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            Bash::IfStatement
            | Bash::ElifClause
            | Bash::ForStatement
            | Bash::CStyleForStatement
            | Bash::WhileStatement => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // `&&` / `||`, and the `-a` / `-o` that `[ … ]` spells them
            // as — a decision each like the `[ … ] && [ … ]` and
            // `[[ … && … ]]` twins (#1536): the `ShortCircuit` row's
            // payers, the test spelling read for its bytes by the row.
            // The arithmetic ternary inside `(( … ))` / `$(( … ))` — the
            // only ternary form Bash has, and a decision point in every
            // sibling that has one (#1268) — is the `Ternary` row's
            // owner; `bash_ternary_expression_alias_is_unreachable` pins
            // the second id the row lists defensively.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
                || ternary::owns_kind(kind, node, code, ancestors) =>
            {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
