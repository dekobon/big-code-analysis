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
use crate::lang_helpers::documents_where_it_sits;

impl Loc for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Rust::*;

        let start = init(node, stats, is_func_space);

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
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            BlockComment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
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
                // Only a doc comment carries the newline, so the one
                // `Checker::is_doc_comment` answer decides both the row span
                // and, with where the comment sits, the `dloc` billing —
                // asked once rather than again through `add_comment_lines`.
                // The row discount follows the spelling alone: an in-body
                // `///` swallows its newline too.
                //
                // `saturating_sub` is exact under the `end > start` guard —
                // `end > start` and `start >= 0` give `end >= 1` — and is
                // belt-and-braces on the one line in this crate known to
                // have underflowed.
                let doc = Self::is_doc_comment(node, code);
                let end = node.end_row();
                let end = if doc && end > start {
                    end.saturating_sub(1)
                } else {
                    end
                };
                let documentation = doc && documents_where_it_sits::<Self>(node, code, ancestors);
                add_comment_rows(stats, start, end, documentation);
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
