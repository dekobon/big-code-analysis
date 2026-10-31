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
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read
        // or a kind the rows do not own: the count is the construct list,
        // not branching a reader must hold, and the effort is the table's
        // vocabulary (#1695, #1697).
        use crate::roles::python::{
            branch, case_arm, catch, else_if, r#loop, short_circuit, switch, ternary,
        };
        use Python::*;

        let kind = node.kind_id();
        // Every decision is a row of `roles/python.rs`, read at the
        // projection the metric counts (#1694, #1695, #1697). Python
        // counts keyword tokens, so `Branch` / `Loop` / `Catch` / `ElseIf`
        // are read at the leaf — `if` (declined under the ternary by the
        // row's gate), `assert`, `for` (the comprehension clause's too,
        // #417), `while`, `except`, `elif` — `and` / `or` from
        // `ShortCircuit` at the token (which ABC deliberately does not
        // count, #395), and the `conditional_expression` from `Ternary`
        // at its owner.
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || branch::pays_at_leaf_kind(kind, node, code, ancestors)
            || r#loop::pays_at_leaf_kind(kind, node, code, ancestors)
            || catch::pays_at_leaf_kind(kind, node, code, ancestors)
            || else_if::pays_at_leaf_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        // Python's `match`/`case` (PEP 634, 3.10+) is treated like Rust's
        // `match` and the C-family `switch`: each `case` clause the
        // `CaseArm` row owns counts toward standard CCN — a bare `case _:`
        // with no guard is the `match`'s `default:` and the row leaves it
        // out; `case _ if g:` still counts — and the `match_statement`
        // the `Switch` row owns adds the modified count.
        //
        // `with` (and `async with`) is deliberately absent from every
        // row: it is unconditional resource management, not a branch.
        // Standard McCabe does not count it, and the C-family `using` /
        // try-with-resources siblings are likewise uncounted, so counting
        // it here would be an undocumented divergence. The
        // `__exit__`-can-suppress-an-exception argument is rejected for
        // parity with those siblings and with the textbook definition.
        // See #418.
        if case_arm::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
        } else if switch::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic_modified += 1.;
        }
        // Python's `for/else`, `while/else`, and `try/except/else`
        // attach an `else_clause` whose body runs only on the "normal"
        // completion path (loop finishes without `break`; try block
        // finishes without raising). That conditional execution is a
        // distinct decision point, so count it toward both standard and
        // modified cyclomatic. Plain `if/else` is unconditional once the
        // `if` has been counted, so this must NOT fire for `else_clause`
        // parents of `if_statement` — see #229. A consumer policy on the
        // `else` keyword, which no row owns: the `Catch` row is the
        // handler, not the `else` that runs when none fired.
        // role-bypass: allow(Loop) — which construct the `else` belongs to,
        // read off the grandparent (`try_statement` is no row's); not
        // whether the grandparent is a loop
        if kind == Else as u16
            && node.parent_grandparent_match(
                ancestors,
                |parent| parent.kind_id() == ElseClause,
                |grand| {
                    matches!(
                        grand.kind_id().into(),
                        ForStatement | WhileStatement | TryStatement
                    )
                },
            )
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
    }
}
