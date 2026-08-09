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
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        match node.kind_id().into() {
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
            | Bash::TernaryExpression2
            | Bash::AMPAMP
            | Bash::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
