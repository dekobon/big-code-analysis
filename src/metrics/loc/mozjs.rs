//! `Loc` implementation for Mozjs.
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

impl Loc for MozjsCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Mozjs::*;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            String | DQUOTE | Program => {}
            // `HtmlComment` is the Annex-B `<!-- -->` comment kind; count
            // its rows as CLOC rather than letting them fall to the `_`
            // arm and over-count PLOC (#697).
            Comment | HtmlComment => {
                add_cloc_lines(stats, start, end);
            }
            // A `template_string` (`` `…` ``) can span multiple rows; credit
            // every spanned row to PLOC to match Python's #415 decision (#778).
            TemplateString => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            // `StatementBlock` is a syntactic `{ … }` brace grouping, not a
            // logical statement, so it is deliberately absent here (#777). It
            // falls through to the `_` PLOC catch-all, matching every other
            // language: Rust's `Block`, C/C++'s `CompoundStatement`, and
            // Java/Groovy/C#'s `Block` all contribute 0 lloc. Counting it
            // inflated JS/TS lloc by one per brace block.
            ExpressionStatement | ExportStatement | ImportStatement | IfStatement
            | SwitchStatement | ForStatement | ForInStatement | WhileStatement | DoStatement
            | TryStatement | WithStatement | BreakStatement | ContinueStatement
            | DebuggerStatement | ReturnStatement | ThrowStatement | EmptyStatement
            | StatementIdentifier => {
                stats.lloc.count_logical_line();
            }
            // A declaration is an executable statement — the initializer
            // runs — which is the rationale Java's `LocalVariableDeclaration`
            // arm cites. Two enclosing constructs already count a logical
            // line for the same row, so counting the declaration as well
            // would double-count it:
            //
            //   * a classic `for (let i = 0; …)` header: the initializer,
            //     condition and increment belong to the `ForStatement`,
            //     which has its own arm. `for (const x of …)` and
            //     `for (var k in …)` need no carve-out — the grammar
            //     inlines the `const` / `var` keyword into
            //     `for_in_statement` and emits no declaration node at all.
            //   * `export const a = 1;`: the declaration nests *inside*
            //     the `ExportStatement`, which already counted the line.
            //     This is an ancestor walk rather than a parent check
            //     because the TypeScript and TSX grammars interpose an
            //     `ambient_declaration` for `export declare const y: string;`,
            //     and the four JS-family modules stay identical.
            //
            // `StatementBlock` stops the walk, so a declaration in a
            // function or loop body under either construct still counts.
            //
            // `using_declaration` (`using r = open();`, TC39 explicit resource
            // management) is the third member of the grammar's `declaration`
            // supertype that executes an initializer. The TypeScript and TSX
            // grammars pinned here emit no such node, which is why only the
            // two JavaScript modules list it.
            VariableDeclaration | LexicalDeclaration | UsingDeclaration => {
                if node.count_specific_ancestors::<MozjsCode>(
                    ancestors,
                    |node| node.kind_id() == ForStatement || node.kind_id() == ExportStatement,
                    |node| node.kind_id() == StatementBlock,
                ) == 0
                {
                    stats.lloc.count_logical_line();
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
