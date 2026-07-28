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
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use crate::metrics::cognitive::{elixir_call_keyword, elixir_do_block_call_children};

        // The space-opening node for a `defmodule` Call is the node
        // itself, so this triggers exactly once per Class.
        //
        // `is_func_space_with_code` is not consulted: it is implied.
        // `elixir_is_class_macro` is exactly `kw == "defmodule"`, so it
        // answers `true` for every node this check lets through, and for
        // the `def`-shaped calls where it would have consulted the
        // ancestor chain — the `quote`-template lookup — this check
        // rejects the node anyway. Calling it cost a source-text keyword
        // scan per node plus, before #1088, an `O(depth)` climb, and its
        // answer was discarded either way (#1088).
        if !stats.is_disabled() || !matches!(elixir_call_keyword(node, code), Some("defmodule")) {
            return;
        }

        stats.is_class_space = true;

        // Direct-child method Calls of the module's do_block. We do
        // not descend deeper — methods nested inside another
        // `defmodule` are attributed to that inner module via its own
        // pass.
        for stmt in elixir_do_block_call_children(node) {
            match elixir_call_keyword(&stmt, code) {
                Some("def" | "defmacro") => {
                    stats.class_nm += 1;
                    stats.class_npm += 1;
                }
                // `defp` / `defmacrop` are methods but not public, so
                // they bump `class_nm` only.
                Some("defp" | "defmacrop") => {
                    stats.class_nm += 1;
                }
                _ => {}
            }
        }
    }
}
