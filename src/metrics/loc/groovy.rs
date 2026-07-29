//! `Loc` implementation for Groovy.
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

impl Loc for GroovyCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Groovy::*;

        let (start, end) = init(node, stats, is_func_space);
        let kind_id: Groovy = node.kind_id().into();
        // LLOC counts statements only — same definition as Java.
        // Groovy additions over Java's list:
        //   - `YieldStatement` for the Java-14+ switch-expression form.
        //   - `CommandChain` for Groovy's parens-less call as a top-
        //     level statement (the dekobon grammar's distinct node;
        //     the prior amaanq grammar called this `juxt_function_call`).
        //   - `ForInStatement` (the dekobon grammar's name for the
        //     `for (x in xs)` / `for (Foo x : xs)` shape; the prior
        //     amaanq grammar called this `enhanced_for_statement`).
        //   - `PipelineStatement` is the dekobon grammar's distinct
        //     node for a Jenkinsfile `pipeline { … }` block, treated
        //     here as a single statement.
        match kind_id {
            SourceFile => {}
            LineComment | BlockComment | GroovydocComment => {
                add_cloc_lines(stats, start, end);
            }
            // A Groovy triple-quoted string (`"""…"""` / `'''…'''`) is a
            // `string_literal` spanning several rows; credit every spanned row
            // to PLOC to match Python's #415 decision (#778).
            StringLiteral => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            // An `ExpressionStatement` whose only child is a bare
            // `Closure` is a Groovy-specific grammar artifact: the
            // alternative branch of `if (x) { … } else { … }` wraps
            // the brace-block as `expression_statement (closure)`
            // even though the user wrote it as part of the surrounding
            // `if`. Skipping the wrapper avoids double-counting the
            // else-branch as a separate LLOC. Real expression
            // statements like `expression_statement (command_chain)`
            // for `println(x)` keep firing because their child is not
            // a bare `Closure`.
            ExpressionStatement if node.child(0).is_some_and(|c| c.kind_id() == Closure) => {
                // No-op: do not count as LLOC.
            }
            AssertStatement | BreakStatement | CommandChain | ContinueStatement
            | DoWhileStatement | ExpressionStatement | ForInStatement | ForStatement
            | IfStatement | PipelineStatement | ReturnStatement | SwitchExpression
            | ThrowStatement | TryStatement | WhileStatement | YieldStatement => {
                stats.lloc.logical_lines += 1;
            }
            LocalVariableDeclaration => {
                if node.count_specific_ancestors::<GroovyCode>(
                    ancestors,
                    |node| node.kind_id() == ForStatement,
                    |node| node.kind_id() == Block,
                ) == 0
                {
                    // Skip the initializer slot of a classic `for` loop —
                    // same reason as Java's impl.
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
