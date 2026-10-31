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
    // The space tree is declared in `roles/irules.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::irules::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // Event handlers count as functions (not closures): `nom.functions` on
    // a typical iRules file is then the handler count, the intuitive
    // metric. `try` handlers are excluded — see `is_func_space`.
    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::irules::func_opener::is_callable(node, &[], ancestors)
    }

    // iRules has no anonymous lambda node (`apply` is an ordinary command).
    fn is_closure<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    // `Call` and `StringLit` are the rows of `roles/irules.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(irules, call, is_call, is_call_with_code);
    impl_row_predicates!(irules, string_lit, is_string, is_string_with_code);

    // iRules arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    // The `ElseIf` row of `roles/irules.rs`: the dedicated `elseif` node
    // itself is the else-if. `Elseif2` is the `elseif` keyword token, not
    // the clause, and the row leaves it out (#1697).
    impl_else_if_from_row!(irules);
}
