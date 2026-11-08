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
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Mozcpp::*;

        let start = init(node, stats, is_func_space);
        // Translation phase 2 splices a comment row's trailing `\` too.
        credit_bare_continuation_rows(
            node,
            code,
            ancestors,
            stats,
            BareRowComments {
                is_comment: crate::roles::mozcpp::comment::lists_kind,
                continued: true,
            },
        );

        match node.kind_id().into() {
            DeclarationList | FieldDeclarationList | TranslationUnit => {}
            // A `raw_string_literal` (`R"(…)"`) or a `string_literal` written
            // across adjacent lines can span multiple rows; credit every
            // spanned row to PLOC to match Python's #415 decision (#778). A
            // `char_literal` is spliced the same way, and its
            // `escape_sequence` starts on the row above the bare one, so the
            // leaf credit alone left that row to `blank` (#1816).
            RawStringLiteral | StringLiteral | CharLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            Comment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
            }
            WhileStatement | SwitchStatement | CaseStatement | IfStatement | ForStatement
            | ReturnStatement | BreakStatement | ContinueStatement | GotoStatement
            | ThrowStatement | TryStatement | TryStatement2 | ExpressionStatement
            | ExpressionStatement2 | LabeledStatement | StatementIdentifier => {
                stats.lloc.count_logical_line();
            }
            Declaration => {
                if !ancestors.has_ancestor_before_stop::<MozcppCode>(
                    node,
                    GateLane::InsideHeader,
                    |node| {
                        matches!(
                            node.kind_id().into(),
                            WhileStatement | ForStatement | IfStatement
                        )
                    },
                    |node| node.kind_id() == CompoundStatement,
                ) {
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
