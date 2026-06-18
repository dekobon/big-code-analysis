//! `Cyclomatic` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Python::*;

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
        match node.kind_id().into() {
            If | Elif | For | While | Except | Assert | And | Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            CaseClause
                if crate::metrics::npa::python_case_clause_counts(node, UNDERSCORE as u16) =>
            {
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
