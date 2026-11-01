//! `Checker` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for ElixirCode {
    // The one `Comment` row of `roles/elixir.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(elixir);

    // Elixir has no syntactic function-definition node: `def` / `defp`
    // / `defmacro` / `defmacrop` / `defmodule` are ordinary `call`s with
    // the macro identifier in the `target` field, so the rows that name
    // them (`roles/elixir.rs`) read source bytes and, for a `def` inside
    // a `quote` template (#310), the chain. The byte-less `is_func_space`
    // and `is_func` cannot read either and so answer the kind table
    // alone: the file root and an anonymous function. The `_with_code`
    // spellings read the rows, and the walk promotes through
    // `promotes_to_func_space_with_code` below, which reads the one
    // keyword lookup both rows share rather than each row in turn.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::Source | Elixir::AnonymousFunction
        )
    }

    fn is_func<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::elixir::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::elixir::func_opener::is_callable(node, code, ancestors)
    }

    fn promotes_to_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        use crate::lang_helpers::elixir::elixir_declaration;
        // The default is `is_func_with_code || is_func_space_with_code`,
        // which would scan a `call`'s target text once per row; the two
        // rows' gates read `elixir_declaration`, so the walk reads it
        // once and promotes on either answer (#310 follow-on perf). The
        // kind-only owners — the file root, an anonymous function — are
        // the byte-less table. Spelling the rule out a second time is
        // what let this override and `is_func_space_with_code` drift
        // apart before (#1130); `roles/elixir.rs`'s tests hold the two
        // to each other node by node.
        Self::is_func_space(node) || elixir_declaration(node, code, ancestors).is_some()
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::elixir::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/elixir.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(elixir, call, is_call, is_call_with_code);
    impl_row_predicates!(elixir, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::LPAREN | Elixir::LPAREN2 | Elixir::RPAREN | Elixir::COMMA
        )
    }

    // The `ElseIf` row of `roles/elixir.rs` is absent: Elixir has no
    // `else if` chain construct. Multi-way branching uses `cond do … end`
    // (a `Call` whose `do_block` holds many `stab_clause`s, each `+1`
    // nesting in cognitive) or nested `if/else`. No tail-recursive chain
    // to collapse here, so the default `false` stands (#1697).
}
