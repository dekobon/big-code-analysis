//! `Npa` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Elixir Npa (#275). `defmodule` is treated as a class via source-aware
// Checker dispatch; `defstruct` is its closest analog to a field-set
// declaration. When entering a `defmodule` Class space we look for a
// direct-child `defstruct` Call in the `do_block` and count its
// field arguments. Three syntactic forms are accepted, matching the
// Elixir docs (https://hexdocs.pm/elixir/Kernel.html#defstruct/1):
//
// - `defstruct [:a, :b]` — bracketed list of atoms.
// - `defstruct a: 1, b: 2` — bare keyword list (the most common form).
// - `defstruct [a: 1, b: 2]` — bracketed keyword list.
//
// All fields are counted as public (`class_npa`); Elixir struct fields
// have no Java-style visibility modifier and the runtime exposes every
// field via pattern matching and `Map.get/2`.
impl Npa for ElixirCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use crate::metrics::cognitive::{elixir_call_keyword, elixir_do_block_call_children};

        // `Ancestors::unknown()`: `Npm`/`Npa::compute` take no
        // ancestor chain, so this pays `Node::parent`'s `O(depth)`
        // per step for a quoted `def`. Widening the two traits' 20-odd
        // impls is tracked as #1088.
        if !stats.is_disabled() || !Self::is_func_space_with_code(node, code, Ancestors::unknown())
        {
            return;
        }
        if !matches!(elixir_call_keyword(node, code), Some("defmodule")) {
            return;
        }

        stats.is_class_space = true;

        for stmt in elixir_do_block_call_children(node) {
            if matches!(elixir_call_keyword(&stmt, code), Some("defstruct")) {
                let fields = count_defstruct_fields(&stmt);
                stats.class_na += fields;
                stats.class_npa += fields;
            }
        }
    }
}

// Counts the field entries of an Elixir `defstruct` Call's arguments.
// `defstruct` accepts three syntactic forms:
//   * `defstruct [:a, :b]` — a `List` of atoms.
//   * `defstruct a: 1, b: 2` — a bare `Keywords` keyword list, which
//     in the tree-sitter-elixir grammar appears directly inside
//     `Arguments` without an extra wrapper.
//   * `defstruct [a: 1, b: 2]` — a `List` wrapping a `Keywords`.
// We descend through the `Arguments` / `List` / `Keywords` wrapper
// nodes (skipping the leading `target` Identifier that names the
// macro itself) and tally `Atom` leaves (bare-list form) and `Pair`s
// (keyword form). `defstruct nil` and an empty `defstruct` correctly
// return 0.
fn count_defstruct_fields(call: &Node) -> usize {
    use Elixir as E;

    // `Arguments` is the wrapper around the macro's positional
    // arguments. `List` is the bracketed form. Keyword pairs without
    // brackets appear directly inside `Arguments` (no `Keywords`
    // wrapper) in the tree-sitter-elixir grammar. The leading
    // `target` Identifier is never one of these kinds, so no
    // explicit target-skip filter is needed.
    call.children()
        .filter(|child| matches!(child.kind_id().into(), E::Arguments | E::List | E::Keywords))
        .map(|child| count_field_entries(&child))
        .sum()
}

fn count_field_entries(node: &Node) -> usize {
    use Elixir as E;

    node.children()
        .map(|child| match child.kind_id().into() {
            // Bare-list form (`defstruct [:a, :b]`): each atom is a
            // field. Keyword form (`defstruct a: 1, b: 2`): each
            // `Pair` is a field.
            E::Atom | E::QuotedAtom | E::Atom2 | E::Pair => 1,
            // A `List` or `Keywords` may wrap the entries one level
            // deeper (`defstruct [a: 1, b: 2]` puts a `List` inside
            // `Arguments`, which then contains a `Keywords`).
            E::List | E::Keywords => count_field_entries(&child),
            _ => 0,
        })
        .sum()
}
