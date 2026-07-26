//! `Loc` implementation for Php.
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

impl Loc for PhpCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use Php::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // A PHP double-quoted (`encapsed_string`) or single-quoted
            // (`string`) literal can span several rows; credit every spanned
            // row to PLOC to match Python's #415 decision (#778). Heredoc /
            // nowdoc bodies already reach PLOC through their inner statement
            // nodes, so they are not routed here.
            EncapsedString | String => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            // Statement kinds that contribute one logical line each.
            ExpressionStatement
            | EchoStatement
            | EmptyStatement
            | IfStatement
            | SwitchStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | TryStatement
            | ReturnStatement
            | BreakStatement
            | ContinueStatement
            | GotoStatement
            | UnsetStatement
            | DeclareStatement
            | NamespaceUseDeclaration
            | GlobalDeclaration
            | FunctionStaticDeclaration
            | ConstDeclaration
            | ConstDeclaration2
            | PropertyDeclaration
            | NamedLabelStatement => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
