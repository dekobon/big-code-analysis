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
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Rust::*;

        let (start, end) = init(node, stats, is_func_space);

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
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            LineComment => {
                // tree-sitter-rust's `process_line_doc_content` (see
                // `src/scanner.c` in the pinned grammar crate) consumes the
                // trailing newline into the `DocComment`, so the node spans
                // one row more than it renders — except at EOF, where it
                // returns without consuming one. Discount that row only when
                // the node really spans it; discounting unconditionally
                // underflowed (#1051).
                //
                // Cheap operand first: `end > start` is false for every plain
                // line comment, short-circuiting `is_child`'s child walk.
                //
                // `saturating_sub` is exact under that guard — `end > start`
                // and `start >= 0` give `end >= 1` — and is belt-and-braces
                // on the one line in this crate known to have underflowed.
                let end = if end > start && node.is_child(DocComment as u16) {
                    end.saturating_sub(1)
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
                stats.lloc.count_logical_line();
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
