//! `Loc` implementation for Java.
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

impl Loc for JavaCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Java::*;

        let start = init(node, stats, is_func_space);
        let kind_id: Java = node.kind_id().into();
        // LLOC in Java is counted for statements only
        // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/expressions.html
        match kind_id {
            Program => {}
            LineComment | BlockComment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
            }
            // A Java text block (`"""…"""`) is a `string_literal` whose body
            // is a `multiline_string_fragment` spanning several rows; credit
            // every spanned row to PLOC to match Python's #415 decision (#778).
            StringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            AssertStatement | BreakStatement | ContinueStatement | DoStatement
            | EnhancedForStatement | ExpressionStatement | ForStatement | IfStatement
            | ReturnStatement | SwitchExpression | ThrowStatement | TryStatement
            | WhileStatement => {
                stats.lloc.count_logical_line();
            }
            LocalVariableDeclaration => {
                // The declaration in a `for` header belongs to the loop's
                // logical line and adds none.
                // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/for.html
                //
                // The header is the parent's `init` field, which the grammar
                // gives `for_statement` alone. An enclosing `ForStatement` is
                // not the test: a declaration in a `switch` group under a
                // brace-less `for` body has one with no `block` between, and
                // is a real statement (#1887).
                let is_for_header = ancestors
                    .parent(node)
                    .and_then(|parent| parent.child_by_field_name("init"))
                    .is_some_and(|init| init.id() == node.id());
                if !is_for_header {
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
