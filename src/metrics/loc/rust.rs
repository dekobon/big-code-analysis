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
use crate::lang_helpers::bound_doc_run;

/// The rows a Rust comment renders on. tree-sitter-rust's
/// `process_line_doc_content` (see `src/scanner.c` in the pinned grammar
/// crate) consumes a line doc comment's trailing newline into its
/// `DocComment`, so the node spans one row more than it renders — except
/// at EOF, where it returns without consuming one. The row is discounted
/// only when the node really spans it; discounting unconditionally
/// underflowed (#1051). Only a doc comment carries the newline, and the
/// discount follows the spelling alone: an in-body `///` swallows its
/// newline too. A block comment never spans a row past its `*/`.
///
/// `saturating_sub` is exact under the `end > start` guard and is
/// belt-and-braces on the one line in this crate known to have
/// underflowed.
fn rendered_rows(node: &Node, doc: bool) -> (usize, usize) {
    let (start, end) = (node.start_row(), node.end_row());
    if doc && node.kind_id() == Rust::LineComment && end > start {
        (start, end.saturating_sub(1))
    } else {
        (start, end)
    }
}

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
            // A line doc comment renders on fewer rows than its node spans
            // (`rendered_rows`), so both kinds bill their rendered rows here
            // rather than through `add_comment_lines`, and a doc comment
            // bound to an item is billed across the same rows.
            // `is_doc_comment` opens a cursor over the node's children, so
            // it is asked once here and its answer handed down; a member
            // `bound_doc_run` credits has already passed that test.
            BlockComment | LineComment => {
                let doc = Self::is_doc_comment(node, code);
                let (first, last) = rendered_rows(node, doc);
                let documentation = doc && documents_by_placement::<Self>(node, code, ancestors);
                add_comment_rows(stats, first, last, documentation);
                bound_doc_run::<Self>(node, code, ancestors, |member| {
                    let (first, last) = rendered_rows(member, true);
                    stats.dloc.lines.insert_range(first, last);
                });
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
