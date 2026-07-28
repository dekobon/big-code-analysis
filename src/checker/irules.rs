//! `Checker` implementation for iRules.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for IrulesCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Irules::Comment
    }

    // iRules code is dominated by `when EVENT { … }` event handlers; real
    // scripts frequently contain no `proc` at all. Treating `when` handlers
    // and `proc` definitions as function spaces is what makes per-function
    // metrics meaningful here — under a proc-only model a typical
    // handler-only file would collapse to a single Unit space. The handler
    // bodies are recursively-parsed `braced_word`s, so nested control flow
    // is attributed correctly.
    //
    // `OnHandler` / `TrapHandler` are defensive arms (lesson #34): the
    // grammar declares dedicated `on_handler` / `trap_handler` nodes, but at
    // statement level a bare `on …` / `trap …` parses as a generic `command`
    // (the `command` rule wins), so these kinds do not surface in practice
    // and have no dedicated test. They are listed here so that if a future
    // grammar revision starts emitting them they are handled as spaces
    // rather than silently dropped — never removed in favour of an
    // `unreachable!` (lesson #5).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Irules::SourceFile
                | Irules::Procedure
                | Irules::WhenEvent
                | Irules::OnHandler
                | Irules::TrapHandler
        )
    }

    // Handlers count as functions (not closures): `nom.functions` on a
    // typical iRules file is then the handler count, the intuitive metric.
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Irules::Procedure | Irules::WhenEvent | Irules::OnHandler | Irules::TrapHandler
        )
    }

    // iRules has no anonymous lambda node (`apply` is an ordinary command).
    fn is_closure<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Irules::Command
    }

    // iRules arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    impl_simple_is_string!(Irules, QuotedWord, BracedWord, BracedWordSimple);

    // iRules grammar has a dedicated `elseif` named node (id 145), not a
    // nested `if`. `Elseif2` (id 92) is the `elseif` keyword token, not
    // the clause, so it is intentionally excluded here (lesson #34
    // defensive-arm marker).
    impl_is_else_if_clause!(Irules, Elseif);
}
