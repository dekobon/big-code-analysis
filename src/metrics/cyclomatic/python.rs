//! `Cyclomatic` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::python::{case_arm, short_circuit, ternary};
        use Python::*;

        let kind = node.kind_id();
        // `and` / `or`, the `ShortCircuit` row's payers: cyclomatic counts
        // the token, which ABC deliberately does not (#395) — the same
        // row, two policies (#1694). The `conditional_expression` the
        // `Ternary` row owns is one decision, read at the node: its `if`
        // is the statement keyword's kind, so the keyword arm below
        // declines the one the ternary spells (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }

        // Python's `match`/`case` (PEP 634, 3.10+) is treated like Rust's
        // `match` and the C-family `switch`: each non-bare-wildcard arm
        // counts toward standard CCN, and the containing `match_statement`
        // adds the modified count. A bare `case _:` (no guard) is skipped,
        // mirroring Rust's `MatchArm` filter and Java/C#'s `default:`
        // exclusion. A guard (`case _ if g:`) still escapes the filter.
        // `with` (and `async with`) is deliberately absent from the
        // decision-point arm below: it is unconditional resource
        // management, not a branch. Standard McCabe does not count it,
        // and the C-family `using` / try-with-resources siblings are
        // likewise uncounted, so counting it here would be an
        // undocumented divergence. The `__exit__`-can-suppress-an-
        // exception argument is rejected for parity with those
        // siblings and with the textbook definition. Both plain
        // `with` and `async with` surface the same `with` keyword
        // token (`With`), so omitting it stops counting both. See #418.
        match kind.into() {
            If if ancestors
                .parent(node)
                .is_some_and(|p| ternary::owns(&p, code, ancestors)) => {}
            If | Elif | For | While | Except | Assert => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // Each `case` clause the `CaseArm` row owns: a bare `case _:`
            // with no guard is the `match`'s `default:` and the row
            // leaves it out; `case _ if g:` still counts.
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            MatchStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Python's `for/else`, `while/else`, and `try/except/else`
            // attach an `else_clause` whose body runs only on the
            // "normal" completion path (loop finishes without `break`;
            // try block finishes without raising). That conditional
            // execution is a distinct decision point, so count it
            // toward both standard and modified cyclomatic. Plain
            // `if/else` is unconditional once the `if` has been
            // counted, so we must NOT fire for `else_clause` parents
            // of `if_statement` — see #229.
            Else if node.parent_grandparent_match(
                ancestors,
                |parent| parent.kind_id() == ElseClause,
                |grand| {
                    matches!(
                        grand.kind_id().into(),
                        ForStatement | WhileStatement | TryStatement
                    )
                },
            ) =>
            {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
