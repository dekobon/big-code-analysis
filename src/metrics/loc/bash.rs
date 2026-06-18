//! `Loc` implementation for Bash.
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

impl Loc for BashCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool, is_unit: bool) {
        use Bash::*;

        let (start, end) = init(node, stats, is_func_space, is_unit);

        match node.kind_id().into() {
            Program => {}
            Comment => {
                add_cloc_lines(stats, start, end);
            }
            // LLOC: leaf statement nodes. Pipeline, Subshell, and
            // RedirectedStatement are excluded because they wrap inner
            // Command nodes that are already counted here.
            Command | VariableAssignment | DeclarationCommand | UnsetCommand | IfStatement
            | ForStatement | CStyleForStatement | WhileStatement | CaseStatement
            | FunctionDefinition => {
                stats.lloc.logical_lines += 1;
            }
            _ => {
                if node.child_count() == 0 {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
        }
    }
}
