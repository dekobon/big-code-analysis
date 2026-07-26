//! `Loc` implementation for C.
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

impl Loc for CCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use C::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            DeclarationList | FieldDeclarationList | TranslationUnit => {}
            // A `string_literal` continued across rows (`"…\` + newline) can
            // span multiple rows; credit every spanned row to PLOC to match
            // Python's #415 decision (#778).
            StringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // C has no `throw` / `try` statements.
            WhileStatement | SwitchStatement | CaseStatement | IfStatement | ForStatement
            | ReturnStatement | BreakStatement | ContinueStatement | GotoStatement
            | ExpressionStatement | ExpressionStatement2 | LabeledStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            Declaration => {
                if node.count_specific_ancestors::<CCode>(
                    |node| {
                        matches!(
                            node.kind_id().into(),
                            WhileStatement | ForStatement | IfStatement
                        )
                    },
                    |node| node.kind_id() == CompoundStatement,
                ) == 0
                {
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);

                // As reported here: https://github.com/tree-sitter/tree-sitter-cpp/issues/276
                // `tree-sitter-cpp` doesn't expand macros, providing a single `PreprocArg` node for the entire macro argument.
                // Therefore, all lines from `start_row` to `end_row` must be added to PLOC to account for the unexpanded macro content
                if let PreprocArg = node.kind_id().into() {
                    (node.start_row() + 1..=node.end_row()).for_each(|line| {
                        stats.ploc.lines.insert(line);
                    });
                }
            }
        }
    }
}
