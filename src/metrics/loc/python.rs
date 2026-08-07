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

impl Loc for PythonCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Python::*;

        let (start, end) = init(node, stats, is_func_space);

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
            StringStart | StringEnd | StringContent | Module => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            String => {
                let Some(parent) = ancestors.parent(node) else {
                    return;
                };
                if let ExpressionStatement = parent.kind_id().into() {
                    // A bare string statement is treated as a docstring and
                    // counted as comment lines (see issue #415 for the related
                    // classification caveat, intentionally left unchanged).
                    add_cloc_lines(stats, start, end);
                } else {
                    // A non-docstring string literal (assigned or passed as an
                    // argument). Its interior physical rows are real code, not
                    // blank lines (#415). The opening row is inserted only when
                    // the parent statement begins on an earlier row, otherwise
                    // that row is already attributed to the enclosing statement.
                    if parent.start_row() != start {
                        check_comment_ends_on_code_line(stats, start);
                        stats.ploc.lines.insert(start);
                    }
                    (start.saturating_add(1)..=end).for_each(|line| {
                        stats.ploc.lines.insert(line);
                    });
                }
            }
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
