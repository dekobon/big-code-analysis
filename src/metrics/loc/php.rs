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
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Php::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // Every PHP literal that can span rows, so its interior rows
            // reach PLOC instead of being claimed by
            // `blank = sloc - ploc - cloc` (#778, #1396) — the decision #415
            // took for Python and #1260 took for the last four languages.
            //
            // #778 routed the quoted forms and excluded the heredoc, on the
            // premise that its body "already reaches PLOC through its inner
            // statement nodes". Half true, and it cost a phantom blank row
            // per spelling: tree-sitter-php emits a body child only for a row
            // that *has* text. Heredoc drops just the row empty inside the
            // literal; nowdoc is worse, emitting one `nowdoc_string` for the
            // first line and a single multi-row one for the rest, whose
            // interior rows the catch-all's start-row insertion all lost
            // whether or not any of them was empty.
            //
            // The wrapper is routed rather than `HeredocBody` / `NowdocBody`
            // because a body of one empty row emits no body node at all —
            // `heredoc` is the node present for every spelling
            // (`.claude/rules/grammar-dispatch.md` section 6). Its span runs
            // from `<<<` to the closing marker, so its interior is the body
            // rows plus that marker's row, which is code either way.
            //
            // `ShellCommandExpression` (`` `…` ``) is the fifth form and had
            // the nowdoc shape exactly: one multi-row `string_content` child,
            // so every interior row was lost. It is routed here for the same
            // reason, which makes this arm agree with
            // `PhpCode::is_string` (`big-code-analysis-ast/src/checker/php.rs`)
            // on every kind that grammar can span rows with — section 7's
            // parity cross-walk.
            //
            // Aliases (section 1): none of the five routed kinds has a
            // numeric-suffix variant. `String2` is the anonymous `string`
            // *type* keyword and `String3` is the hidden `_string` supertype
            // the parser never emits (section 2) — neither is a literal, so
            // neither belongs here.
            EncapsedString | String | Heredoc | Nowdoc | ShellCommandExpression => {
                add_multiline_string_ploc(node, ancestors, stats, start);
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
                stats.lloc.count_logical_line();
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
