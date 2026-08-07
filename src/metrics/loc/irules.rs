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
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // Same row-terminator quirk as the Tcl impl this dialect
            // forked from: `LF` is a token child of the root whose start
            // row is the row it terminates, so the `_` catch-all below
            // credited comment-only and whitespace-only rows to PLOC
            // (#1135).
            Irules::SourceFile | Irules::LF => {}

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
                stats.lloc.count_logical_line();
            }

            // `expr` and a bare command are logical lines at statement
            // level only; inside `[...]` each is a sub-expression, which
            // is why the two share one guard. The rationale sits above
            // the arm rather than between the alternatives because a
            // comment *inside* a match pattern makes rustfmt emit the
            // whole match verbatim while `cargo fmt --check` still exits
            // 0 — see `.claude/rules/formatting.md`.
            Irules::ExprCmd | Irules::Command
                if ancestors
                    .parent(node)
                    .is_none_or(|p| p.kind_id() != Irules::CommandSubstitution) =>
            {
                stats.lloc.count_logical_line();
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
