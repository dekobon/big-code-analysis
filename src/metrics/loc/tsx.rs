//! `Loc` implementation for Tsx.
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

impl Loc for TsxCode {
    fn compute(node: &Node, _ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Tsx::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            // See MozjsCode::compute — `HtmlComment` counts as CLOC (#697).
            Comment | HtmlComment => {
                add_cloc_lines(stats, start, end);
            }
            // A `template_string` (`` `…` ``) can span multiple rows; credit
            // every spanned row to PLOC to match Python's #415 decision (#778).
            TemplateString => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            // `StatementBlock` is deliberately absent — see MozjsCode::compute
            // (#777). It is a brace grouping, not a logical statement.
            ExpressionStatement | ExportStatement | ImportStatement | IfStatement
            | SwitchStatement | ForStatement | ForInStatement | WhileStatement | DoStatement
            | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
