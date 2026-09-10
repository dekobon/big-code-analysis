//! `Checker` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

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

    impl_simple_is_string!(Tcl, QuotedWord, BracedWord, BracedWordSimple);

    // Tcl grammar has a dedicated `elseif` named node, not a nested `if`.
    impl_is_else_if_clause!(Tcl, Elseif);
}
