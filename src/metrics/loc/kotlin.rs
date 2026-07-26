//! `Loc` implementation for Kotlin.
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

impl Loc for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use Kotlin::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            SourceFile => {}
            LineComment | BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            // A Kotlin raw string (`"""…"""`) is a `multiline_string_literal`
            // spanning several rows; credit every spanned row to PLOC to match
            // Python's #415 decision (#778).
            MultilineStringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            ForStatement | WhileStatement | DoWhileStatement | IfExpression | WhenExpression
            | TryExpression | ThrowExpression | ReturnExpression | Assignment
            | PropertyDeclaration => {
                stats.lloc.logical_lines += 1;
            }
            // Bare expression statements (e.g. `println(x)`) have no
            // ExpressionStatement wrapper in tree-sitter-kotlin-ng. Count
            // them as lloc when they appear as direct children of a block;
            // otherwise fall through to ploc so nested calls still count
            // as physical lines.
            CallExpression | NavigationExpression => {
                if let Some(parent) = node.parent()
                    && matches!(
                        parent.kind_id().into(),
                        Block | FunctionBody | SourceFile | CatchBlock | FinallyBlock
                    )
                {
                    stats.lloc.logical_lines += 1;
                } else {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
