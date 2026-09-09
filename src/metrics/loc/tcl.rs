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

            // A quoted word (`"…"`) is the one Tcl literal that can span
            // rows and is unambiguously data. The grammar gives it no child
            // per row — only the two `"` tokens and any embedded
            // substitution — so its interior rows reached neither PLOC nor
            // CLOC and `blank = sloc - ploc - cloc` mislabelled them as
            // blank (#1260). Credit every spanned row to PLOC, the decision
            // #778 took for eighteen other languages and #415 took for
            // Python.
            //
            // `braced_word` is deliberately *not* routed here. Tcl spells a
            // script body and a braced literal with the same kind, and the
            // grammar parses both as scripts — `puts {a\n\nb}` yields
            // `command` children exactly as a `proc` body does (#1318 had to
            // separate the two roles out-of-band, by the enclosing command's
            // leading word). Routing it would turn every blank line inside
            // every procedure body into code.
            //
            // The `LF` no-op above is unaffected: `LF` is a token child of
            // the root and of `braced_word`, never of a quoted word, so the
            // two arms cannot see the same node (#1135).
            Tcl::QuotedWord => {
                add_multiline_string_ploc(node, ancestors, stats, start);
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
                stats.lloc.count_logical_line();
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
                stats.lloc.count_logical_line();
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
