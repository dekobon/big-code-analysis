//! `Checker` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::tcl::BRACED_WORD_KINDS;

impl Checker for TclCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Tcl::Comment
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(node.kind_id().into(), Tcl::SourceFile | Tcl::Procedure)
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Tcl::Procedure
    }

    // Tcl closures (`apply`) are ordinary commands; the grammar has no distinct closure node.
    fn is_closure<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tcl::Command
    }

    // Tcl arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    // `BracedWord` is listed because it *is* the literal of
    // `lappend x {a b}` — the grammar reserves `BracedWordSimple` for the
    // handful of commands it models (`set`, `foreach`, `regexp`, …) and
    // spells every other command's braced argument `BracedWord`. The same
    // kind is also the `proc` / `if` body, which this table cannot
    // exclude; `is_string_with_code` below does, and is what the walk
    // calls.
    impl_simple_is_string!(Tcl, QuotedWord, BracedWord, BracedWordSimple);

    // The half of the rule that needs the source bytes (#1381), the twin
    // of `TclCode::get_op_type_with_code`. `is_value_braced_word` lives
    // on `Getter`, and `TclCode` implements both traits, so the two
    // classifiers answer from one predicate over one kinds table rather
    // than from two copies (grammar-dispatch §7).
    fn is_string_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        Self::is_string(node)
            && !<Self as Getter>::is_braced_script_word(node, code, ancestors, &BRACED_WORD_KINDS)
    }

    // Tcl grammar has a dedicated `elseif` named node, not a nested `if`.
    impl_is_else_if_clause!(Tcl, Elseif);
}
