//! `Loc` implementation for Rust.
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

impl Loc for RustCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Rust::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            Block
            | SourceFile
            | SLASH
            | SLASHSLASH
            | SLASHSTAR
            | STARSLASH
            | OuterDocCommentMarker
            | OuterDocCommentMarker2
            | DocComment
            | InnerDocCommentMarker
            | BANG => {}
            // A `string_literal` / `raw_string_literal` can span multiple
            // rows; credit every spanned row to PLOC to match Python's #415
            // decision (#778).
            StringLiteral | RawStringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            LineComment => {
                // Exclude the last line for `LineComment` containing a `DocComment`,
                // since the `DocComment` includes the newline,
                // as explained here: https://github.com/tree-sitter/tree-sitter-rust/blob/2eaf126458a4d6a69401089b6ba78c5e5d6c1ced/src/scanner.c#L194-L195
                let end = if node.is_child(DocComment as u16) {
                    end - 1
                } else {
                    end
                };
                add_cloc_lines(stats, start, end);
            }
            Statement
            | EmptyStatement
            | ExpressionStatement
            | LetDeclaration
            | AssignmentExpression
            | CompoundAssignmentExpr => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
