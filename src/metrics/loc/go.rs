//! `Loc` implementation for Go.
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

impl Loc for GoCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            G::SourceFile | G::InterpretedStringLiteral => {}
            // A Go raw string literal (`` `…` ``) can span several rows;
            // credit every spanned row to PLOC to match Python's #415
            // decision (#778).
            G::RawStringLiteral => {
                add_multiline_string_ploc(node, stats, start, end);
            }
            G::Comment => {
                add_cloc_lines(stats, start, end);
            }
            G::FallthroughStatement
            | G::BreakStatement
            | G::ContinueStatement
            | G::GotoStatement
            | G::ReturnStatement
            | G::GoStatement
            | G::DeferStatement
            | G::IfStatement
            | G::ForStatement
            | G::ExpressionSwitchStatement
            | G::TypeSwitchStatement
            | G::SelectStatement
            | G::LabeledStatement => {
                stats.lloc.logical_lines += 1;
            }
            G::ExpressionStatement
            | G::SendStatement
            | G::IncStatement
            | G::DecStatement
            | G::AssignmentStatement
            | G::ShortVarDeclaration
            | G::VarDeclaration
            | G::ConstDeclaration => {
                // Skip simple statements / declarations that appear inside a
                // for-clause init or update slot (e.g. `for i := 0; i < n; i++`);
                // the surrounding `for_statement` already counts as one
                // logical line.
                if node.count_specific_ancestors::<GoCode>(
                    |n| n.kind_id() == G::ForClause,
                    |n| n.kind_id() == G::Block,
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
