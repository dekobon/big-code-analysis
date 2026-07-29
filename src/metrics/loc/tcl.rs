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
            Tcl::SourceFile => {}

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

            // `expr` at statement level is a logical line; inside [...] it is a
            // sub-expression and should not be counted (same semantics as Command).
            Tcl::ExprCmd
            // Commands inside [...] are sub-expressions, not top-level statements.
            | Tcl::Command
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
