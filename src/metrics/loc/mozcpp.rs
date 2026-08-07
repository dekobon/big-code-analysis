//! `Loc` implementation for Mozcpp.
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

impl Loc for MozcppCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Mozcpp::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            DeclarationList | FieldDeclarationList | TranslationUnit => {}
            // A `raw_string_literal` (`R"(…)"`) or a `string_literal` written
            // across adjacent lines can span multiple rows; credit every
            // spanned row to PLOC to match Python's #415 decision (#778).
            RawStringLiteral | StringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
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
                if node.count_specific_ancestors::<MozcppCode>(
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
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // As reported here: https://github.com/tree-sitter/tree-sitter-cpp/issues/276
                // `tree-sitter-cpp` doesn't expand macros, providing a single `PreprocArg` node for the entire macro argument.
                // Therefore, all lines from `start_row` to `end_row` must be added to PLOC to account for the unexpanded macro content
                if let PreprocArg = node.kind_id().into() {
                    (node.start_row().saturating_add(1)..=node.end_row()).for_each(|line| {
                        stats.ploc.lines.insert(line);
                    });
                }
            }
        }
    }
}
