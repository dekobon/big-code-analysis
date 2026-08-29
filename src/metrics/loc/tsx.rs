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
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
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
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            // `StatementBlock` is deliberately absent — see MozjsCode::compute
            // (#777). It is a brace grouping, not a logical statement.
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
            //     which has its own arm. The header is identified by its
            //     grammar role — the `initializer` field of the parent
            //     `for_statement` — rather than by kind, because a
            //     brace-less body (`for (…) var s = i;`) is a declaration
            //     with the *same* parent and is a real logical line.
            //     `for (const x of …)` and `for (var k in …)` need no
            //     carve-out — the grammar inlines the `const` / `var`
            //     keyword into `for_in_statement` and emits no
            //     declaration node at all.
            //   * `export const a = 1;`: the declaration nests *inside*
            //     the `ExportStatement`, which already counted the line.
            //     This is an ancestor walk rather than a parent check
            //     because the grammar interposes an `ambient_declaration`
            //     for `export declare const y: string;`, and the four
            //     JS-family modules stay identical. `StatementBlock` stops
            //     the walk, so a declaration in an exported function's
            //     body still counts.
            //   * `declare const x: number;` and anything else under an
            //     `ambient_declaration` (`declare namespace N { const x:
            //     T; }`, `declare module "m" { … }`): ambient code is a
            //     type-level claim with no initializer to run, so it is
            //     no logical line at all. Ambience is transitive, which
            //     is why this walk does not stop at a `StatementBlock`.
            VariableDeclaration | LexicalDeclaration => {
                let is_for_header = ancestors.parent(node).is_some_and(|parent| {
                    parent.kind_id() == ForStatement
                        && parent
                            .child_by_field_name("initializer")
                            .is_some_and(|initializer| initializer.id() == node.id())
                });
                let is_exported = node.count_specific_ancestors::<TsxCode>(
                    ancestors,
                    |node| node.kind_id() == ExportStatement,
                    |node| node.kind_id() == StatementBlock,
                ) > 0;
                let is_ambient = node.count_specific_ancestors::<TsxCode>(
                    ancestors,
                    |node| node.kind_id() == AmbientDeclaration,
                    |_| false,
                ) > 0;
                if !is_for_header && !is_exported && !is_ambient {
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
