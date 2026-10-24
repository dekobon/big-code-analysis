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
        use crate::roles::bash::short_circuit;
        let kind = node.kind_id();
        match kind.into() {
            // Standard-only: individual case arms (matches C-family `case:`
            // treatment — only arms contribute, not the container). The
            // bare-wildcard arm `*)` is Bash's analogue of the C-family
            // `default:` and is excluded from the standard count, matching
            // every other switch-bearing language. A multi-value pattern
            // (`a|b)`, `*|b)`) is NOT bare and still counts. Closes #211.
            Bash::CaseItem | Bash::CaseItem2 if !bash_case_item_is_bare_wildcard(node, code) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the case…esac container collapses all arms
            // into one decision point.
            Bash::CaseStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified. `TernaryExpression` is the
            // arithmetic ternary inside `(( … ))` / `$(( … ))` — the only
            // ternary form Bash has, and a decision point in every sibling
            // that has one (#1268). The pinned grammar emits only kind 223
            // in every arithmetic context; the `TernaryExpression2` alias
            // is listed defensively per grammar-dispatch §1, following the
            // ObjC `BinaryExpression | BinaryExpression2` precedent, and
            // pinned unreachable by
            // `bash_ternary_expression_alias_is_unreachable`.
            Bash::IfStatement
            | Bash::ElifClause
            | Bash::ForStatement
            | Bash::CStyleForStatement
            | Bash::WhileStatement
            | Bash::TernaryExpression
            | Bash::TernaryExpression2 => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // `&&` / `||`, and the `-a` / `-o` that `[ … ]` spells them
            // as — a decision each like the `[ … ] && [ … ]` and
            // `[[ … && … ]]` twins (#1536): the `ShortCircuit` row's
            // payers, the test spelling read for its bytes by the row.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
