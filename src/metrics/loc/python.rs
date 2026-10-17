//! `Loc` implementation for Python.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::python::{
    python_documentation_string_run, python_is_bare_statement_value, python_is_bare_string,
    python_sphinx_doc_comment,
};

impl Loc for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Python::*;

        let start = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // `block` (both aliased kind_ids — `Block` 135, the dead
            // hidden supertype, and `Block2` 160, the only one emitted
            // today) is a structural wrapper that contributes no LOC of
            // its own: its opening row is always shared with the first
            // statement it wraps, which inserts that row via its own
            // arm. Route through `python_is_block` so both aliases
            // no-op identically at one normalization point — previously
            // `Block2` fell through to the `_` arm and re-inserted an
            // already-covered row (a HashSet duplicate); making the
            // no-op explicit is behaviour-preserving (issue #419).
            _ if python_is_block(node) => {}
            // `escape_sequence` and `escape_interpolation` are leaves inside
            // a string's content that spell no code of their own; their row
            // is billed by the string's arm. Left to the catch-all they
            // credited a docstring row to PLOC (#1597).
            StringStart | StringEnd | StringContent | EscapeSequence | EscapeInterpolation
            | Module => {}
            Comment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
                add_sphinx_doc_comment(node, code, ancestors, stats);
            }
            // An implicitly concatenated bare string (`"""doc""" "more"`)
            // is one string to CPython and to Halstead (#695): bill the
            // whole literal here, once; its fragments are skipped below. A
            // concatenated value falls to the catch-all.
            ConcatenatedString if python_is_bare_statement_value(node, ancestors) => {
                add_bare_string_lines(node, code, ancestors, stats);
            }
            String => {
                if python_is_bare_string(node, ancestors) {
                    // A bare string statement is counted as comment lines
                    // (see issue #415 for the related classification caveat,
                    // intentionally left unchanged), and as documentation
                    // too when it sits where Python reads documentation. A
                    // fragment of a concatenated string was billed by the
                    // arm above.
                    if !ancestors.parent_has_kind(node, ConcatenatedString as u16) {
                        add_bare_string_lines(node, code, ancestors, stats);
                    }
                } else if ancestors.parent_has_kind(node, ExpressionStatement as u16) {
                    // A bare statement's literal that is not a docstring, such
                    // as an element of the discarded tuple `"""a\nb""", x`.
                    // The statement's arm credits no row, so the literal
                    // credits its own opening row instead of trusting
                    // `add_multiline_string_ploc`'s parent gate (#1597).
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                    add_string_interior_ploc(node, stats, start);
                } else {
                    // A non-docstring string literal (assigned or passed as an
                    // argument). Its interior physical rows are real code, not
                    // blank lines (#415). The opening row is inserted only when
                    // the parent statement begins on an earlier row, otherwise
                    // that row is already attributed to the enclosing statement.
                    //
                    // This arm is the rule `add_multiline_string_ploc` was
                    // extracted from (#778), and stayed open-coded until #1423
                    // swept the last copies of the pattern onto the helper. The
                    // parent gate is unchanged — a `string` always has a parent
                    // here, so the helper's `is_none_or` reduces to the same
                    // test — and the interior bound moves from the raw end row
                    // to `Node::end_line`. That is a no-op in Python today: a
                    // `string` node always closes on a `string_end` quote, so
                    // its end column is never 0, and an unterminated literal
                    // parses to a bare `string_start` under an `ERROR` rather
                    // than to a `string` node at all.
                    add_multiline_string_ploc(node, ancestors, stats, start);
                }
            }
            // `match` (PEP 634) and `type` aliases (PEP 695) are real
            // statements that parse cleanly and previously fell through
            // to the `_` arm, contributing 0 logical lines (#462). Count
            // `MatchStatement` like `IfStatement`/`TryStatement` — the
            // construct itself adds one lloc; its `case_clause` children
            // are deliberately omitted, mirroring how `elif_clause`,
            // `else_clause`, `except_clause`, and `finally_clause` are
            // absent here (clauses do not add lloc; the statements they
            // contain do via their own arms). `TypeAliasStatement` is a
            // leaf statement, counted like an assignment expression.
            Statement
            | SimpleStatements
            | ImportStatement
            | FutureImportStatement
            | ImportFromStatement
            | PrintStatement
            | AssertStatement
            | ReturnStatement
            | DeleteStatement
            | RaiseStatement
            | PassStatement
            | BreakStatement
            | ContinueStatement
            | IfStatement
            | ForStatement
            | WhileStatement
            | TryStatement
            | MatchStatement
            | TypeAliasStatement
            | WithStatement
            | GlobalStatement
            | NonlocalStatement
            | ExecStatement
            | ExpressionStatement => {
                stats.lloc.count_logical_line();
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}

/// Bills a bare string statement's rows: as comment rows always (#415),
/// and as documentation rows (`dloc`) only in a docstring or attribute
/// docstring position (#1597) — so a triple-quoted string wrapping
/// commented-out code is charged by MI like a `#` comment.
fn add_bare_string_lines<'a>(
    value: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    add_comment_rows(stats, value.start_row(), value.end_row(), false);
    python_documentation_string_run(value, code, ancestors, |first, last| {
        stats.dloc.lines.insert_range(first, last);
    });
}

/// Credits a Sphinx `#:` doc comment, which documents the assignment
/// beside or below it, as `dloc`. Its rows are already comment rows, so
/// `dloc <= cloc` holds (#1597).
fn add_sphinx_doc_comment<'a>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    python_sphinx_doc_comment(comment, code, ancestors, |first, last| {
        stats.dloc.lines.insert_range(first, last);
    });
}
