//! `Checker` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for ElixirCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Elixir::Comment
    }

    // Elixir has no syntactic function-definition node: `def`/`defp` /
    // `defmacro`/`defmacrop` / `defmodule` are ordinary `Call` nodes
    // with the macro identifier in the `target` field. The byte-less
    // [`is_func_space`] and [`is_func`] cannot distinguish them from
    // any other `Call`, so they conservatively return zero (only the
    // file root and explicit anonymous functions surface as func
    // spaces). The text-aware [`is_func_space_with_code`] /
    // [`is_func_with_code`] overrides below promote the macro-shaped
    // declarations to first-class function / class spaces (#275). The
    // walker passes the source bytes through, so the metrics
    // attributed to a `def`'s body now correctly nest under a Function
    // space and `Wmc` / `Npm` / `Npa` see a `defmodule` Class.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::Source | Elixir::AnonymousFunction
        )
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        if Self::is_func_space(node) {
            return true;
        }
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        // A `def` / `defp` / `defmacro` / `defmacrop` nested inside a
        // `quote do … end` template does NOT declare a method of any
        // enclosing module — the syntax tree there is a code template
        // emitted later, on macro expansion (#310).
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn is_func_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_inside_quote_block, elixir_is_method_macro,
        };
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn promotes_to_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        // Cheap path: byte-less `is_func_space` matches `Source` and
        // `AnonymousFunction` without needing any text inspection.
        if Self::is_func_space(node) {
            return true;
        }
        // Otherwise one `elixir_call_keyword` lookup answers the
        // combined question instead of the two the default impl would
        // have made via `is_func_with_code || is_func_space_with_code`.
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Elixir::AnonymousFunction
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Elixir::Call
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::LPAREN | Elixir::LPAREN2 | Elixir::RPAREN | Elixir::COMMA
        )
    }

    impl_simple_is_string!(Elixir, String, Charlist, Sigil);

    // Elixir lacks an `else if` chain construct. Multi-way branching uses
    // `cond do ... end` (a `Call` whose `do_block` holds many
    // `stab_clause`s, each `+1` nesting in cognitive) or nested
    // `if/else`. No tail-recursive chain to collapse here.
    #[inline]
    fn is_else_if(_: &Node) -> bool {
        false
    }
}
