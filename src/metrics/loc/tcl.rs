//! `Loc` implementation for Tcl.
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

impl Loc for TclCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // Tcl is the only grammar family here that surfaces the row
            // terminator as a token child of the root rather than as
            // extra. `LF`'s start row is the row it *terminates*, so the
            // `_` catch-all below credited every terminated row to PLOC —
            // comment-only and whitespace-only rows included (#1135). An
            // LF after real code is redundant anyway: the code node on
            // that row already inserted it.
            Tcl::SourceFile | Tcl::LF => {}

            Tcl::Comment => {
                add_cloc_lines(stats, start, end);
            }

            Tcl::Procedure
            | Tcl::If
            | Tcl::Elseif
            | Tcl::Foreach
            | Tcl::While
            | Tcl::Set
            | Tcl::Global
            | Tcl::Namespace
            | Tcl::Try
            | Tcl::Catch
            | Tcl::Regexp => {
                stats.lloc.logical_lines += 1;
            }

            // `expr` and a bare command are logical lines at statement
            // level only; inside `[...]` each is a sub-expression, which
            // is why the two share one guard. The rationale sits above
            // the arm rather than between the alternatives because a
            // comment *inside* a match pattern makes rustfmt emit the
            // whole match verbatim while `cargo fmt --check` still exits
            // 0 — see `.claude/rules/formatting.md`.
            Tcl::ExprCmd | Tcl::Command
                if ancestors
                    .parent(node)
                    .is_none_or(|p| p.kind_id() != Tcl::CommandSubstitution) =>
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
