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
    // `OnHandler` / `TrapHandler` are deliberately NOT spaces. The grammar
    // references `on_handler` / `trap_handler` from the `try` rule alone,
    // so they surface only as `try` error-handler clauses — the `catch`
    // analogue, not the `when`-style event handlers their names suggest.
    // Listing them here fabricated an anonymous nested function space (and
    // a `nom` function) per handler; they are branch points, counted by
    // the Cognitive / Cyclomatic impls instead (issue #1266).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Irules::SourceFile | Irules::Procedure | Irules::WhenEvent
        )
    }

    // Event handlers count as functions (not closures): `nom.functions` on
    // a typical iRules file is then the handler count, the intuitive
    // metric. `try` handlers are excluded — see `is_func_space`.
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(node.kind_id().into(), Irules::Procedure | Irules::WhenEvent)
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
