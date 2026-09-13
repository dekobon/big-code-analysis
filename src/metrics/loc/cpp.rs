//! `Loc` implementation for Cpp.
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

impl Loc for CppCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Cpp::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            DeclarationList | FieldDeclarationList | TranslationUnit => {}
            // A `raw_string_literal` (`R"(…)"`) or a `string_literal` written
            // across adjacent lines can span multiple rows; credit every
            // spanned row to PLOC to match Python's #415 decision (#778).
            RawStringLiteral | StringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            WhileStatement | SwitchStatement | CaseStatement | IfStatement | ForStatement
            | ReturnStatement | BreakStatement | ContinueStatement | GotoStatement
            | ThrowStatement | TryStatement | TryStatement2 | ExpressionStatement
            | ExpressionStatement2 | LabeledStatement | StatementIdentifier => {
                stats.lloc.count_logical_line();
            }
            Declaration => {
                if node.count_specific_ancestors::<CppCode>(
                    ancestors,
                    |node| {
                        matches!(
                            node.kind_id().into(),
                            WhileStatement | ForStatement | IfStatement
                        )
                    },
                    |node| node.kind_id() == CompoundStatement,
                ) == 0
                {
                    stats.lloc.count_logical_line();
                }
            }
            kind => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // As reported here: https://github.com/tree-sitter/tree-sitter-cpp/issues/276
                // `tree-sitter-cpp` doesn't expand macros, providing a single
                // `PreprocArg` node for the entire macro argument, so every row
                // that node spans is PLOC rather than blank.
                //
                // Bounded by `add_string_interior_ploc`, and therefore by
                // `Node::end_line` rather than the raw end row: a body whose
                // last continuation is a dangling backslash ends at column 0 of
                // the row below, which the node does not occupy (#1423).
                if let PreprocArg = kind {
                    add_string_interior_ploc(node, stats, start);
                }
            }
        }
    }
}
