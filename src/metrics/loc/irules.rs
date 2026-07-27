//! `Loc` implementation for Irules.
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

impl Loc for IrulesCode {
    fn compute(node: &Node, _ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            Irules::SourceFile => {}

            Irules::Comment => {
                add_cloc_lines(stats, start, end);
            }

            // Compound-statement headers and dedicated statement productions
            // each count once. `when_event` / `on_handler` / `trap_handler`
            // are the iRules handler headers; `for` / `dict_for` are loops
            // Tcl lacks; `switch` counts once (its arms' bodies count their
            // own commands — `switch_arm` is a pattern/body pair, not itself
            // an executable line). `else` is intentionally excluded (it has no
            // condition), matching the Tcl impl above.
            Irules::Procedure
            | Irules::WhenEvent
            | Irules::OnHandler
            | Irules::TrapHandler
            | Irules::If
            | Irules::Elseif
            | Irules::For
            | Irules::Foreach
            | Irules::While
            | Irules::DictFor
            | Irules::DictUpdate
            | Irules::DictWith
            | Irules::Switch
            | Irules::Set
            | Irules::Global
            | Irules::Namespace
            | Irules::Try
            | Irules::Catch
            | Irules::Regexp => {
                stats.lloc.logical_lines += 1;
            }

            // `expr` at statement level is a logical line; inside [...] it is a
            // sub-expression and is not counted (same semantics as Command).
            Irules::ExprCmd
            // Commands inside [...] are sub-expressions, not top-level statements.
            | Irules::Command
                if node
                    .parent()
                    .is_none_or(|p| p.kind_id() != Irules::CommandSubstitution) =>
            {
                stats.lloc.logical_lines += 1;
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
