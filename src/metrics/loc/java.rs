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
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use Java::*;

        let (start, end) = init(node, stats, is_func_space);
        let kind_id: Java = node.kind_id().into();
        // LLOC in Java is counted for statements only
        // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/expressions.html
        match kind_id {
            Program => {}
            LineComment | BlockComment => {
                add_cloc_lines(stats, start, end);
            }
            // A Java text block (`"""…"""`) is a `string_literal` whose body
            // is a `multiline_string_fragment` spanning several rows; credit
            // every spanned row to PLOC to match Python's #415 decision (#778).
            StringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            AssertStatement | BreakStatement | ContinueStatement | DoStatement
            | EnhancedForStatement | ExpressionStatement | ForStatement | IfStatement
            | ReturnStatement | SwitchExpression | ThrowStatement | TryStatement
            | WhileStatement => {
                stats.lloc.logical_lines += 1;
            }
            LocalVariableDeclaration => {
                if node.count_specific_ancestors::<JavaCode>(
                    |node| node.kind_id() == ForStatement,
                    |node| node.kind_id() == Block,
                ) == 0
                {
                    // The initializer, condition, and increment in a for loop are expressions.
                    // Don't count the variable declaration if in a ForStatement.
                    // https://docs.oracle.com/javase/tutorial/java/nutsandbolts/for.html
                    stats.lloc.logical_lines += 1;
                }
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
