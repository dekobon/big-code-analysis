//! `Loc` implementation for Csharp.
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

impl Loc for CsharpCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Csharp::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);
        let kind_id: Csharp = node.kind_id().into();
        match kind_id {
            CompilationUnit => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // A C# verbatim (`@"…"`) or raw (`"""…"""`) string literal can span
            // several rows; credit every spanned row to PLOC to match Python's
            // #415 decision (#778).
            VerbatimStringLiteral | RawStringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            BreakStatement | CheckedStatement | ContinueStatement | DoStatement
            | ExpressionStatement | FixedStatement | ForStatement | ForeachStatement
            | GotoStatement | IfStatement | LabeledStatement | LockStatement | ReturnStatement
            | SwitchStatement | ThrowStatement | TryStatement | UnsafeStatement
            | UsingStatement | WhileStatement | YieldStatement => {
                stats.lloc.logical_lines += 1;
            }
            LocalDeclarationStatement => {
                // Variable declarations inside a `for_statement` init/condition/update
                // (e.g. `for (int i = 0; i < n; i++)`) shouldn't bump LLOC; the
                // surrounding `for_statement` already counts.
                if node.count_specific_ancestors::<CsharpCode>(
                    |n| n.kind_id() == ForStatement,
                    |n| n.kind_id() == Block,
                ) == 0
                {
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
