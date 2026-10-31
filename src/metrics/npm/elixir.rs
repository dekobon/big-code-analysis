//! `Npm` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Elixir Npm (#275). The defmodule Call opens a Class space via
// source-aware Checker dispatch. When we enter that Class space we
// scan its `do_block` body for direct-child `def`/`defp`/`defmacro`/
// `defmacrop` Calls and tally them. `def` and `defmacro` are public
// (Elixir's default — only `defp` / `defmacrop` are private and
// scoped to the module). This mirrors the Java InterfaceBody /
// ClassBody pattern but unrolled because Elixir lacks a dedicated
// "class body" grammar production.
impl Npm for ElixirCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::lang_helpers::elixir::{
            ElixirCallTarget, elixir_call_keyword, elixir_call_target,
            elixir_do_block_call_children,
        };

        // The space-opening node for a `defmodule` Call is the node
        // itself, and the walker pushes that space before running any
        // metric against it, so a nested `defmodule` is counted by its
        // own module and never reaches this one's stats.
        //
        // `is_func_space_with_code` is not consulted: it is implied.
        // The `Container` row's gate is exactly the `defmodule` word, so
        // it answers `true` for every node this check lets through, and
        // for the `def`-shaped calls where it would have consulted the
        // ancestor chain — the `quote`-template lookup — this check
        // rejects the node anyway. Calling it cost a source-text keyword
        // scan per node plus, before #1088, an `O(depth)` climb, and its
        // answer was discarded either way (#1088). The word is the
        // walk's cached one (#1697).
        if elixir_call_target(node, code, ancestors) != ElixirCallTarget::Defmodule {
            return;
        }

        // Direct-child method Calls of the module's do_block, read by
        // keyword rather than through the `FuncOpener` row: the row
        // declines a `def` inside a `quote` template (#310), where it
        // opens no space, but a `defmodule` quoted whole is still a
        // class whose `def`s are its methods
        // (`elixir_npm_counts_a_quoted_defmodule_as_a_class`), so this
        // consumer counts the macro wherever the module sits. We do not
        // descend deeper — methods nested inside another `defmodule`
        // are attributed to that inner module via its own pass. Which of
        // the four macros is public is this consumer's reading too.
        // The uncached read, deliberately: each statement is another
        // node than the one the walk is visiting, so the cached read
        // would evict the module's own word from the slot before NPA
        // asks for it, and the statements miss when they are visited in
        // turn anyway (`elixir_call_keyword`'s doc).
        for stmt in elixir_do_block_call_children(node) {
            match elixir_call_keyword(&stmt, code)
                .map_or(ElixirCallTarget::Other, ElixirCallTarget::from_text)
            {
                ElixirCallTarget::Def | ElixirCallTarget::Defmacro => {
                    stats.class_nm += 1;
                    stats.class_npm += 1;
                }
                // `defp` / `defmacrop` are methods but not public, so
                // they bump `class_nm` only.
                ElixirCallTarget::Defp | ElixirCallTarget::Defmacrop => {
                    stats.class_nm += 1;
                }
                _ => {}
            }
        }
    }
}
