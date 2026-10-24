//! Elixir's role rows: the text-gated fixture (#1691). `def` / `defp` /
//! `defmacro` / `defmacrop` / `defmodule` are not grammar productions —
//! each is a `call` whose `target` identifier spells the keyword — so
//! the rows that name them read source bytes, and the one nested in a
//! `quote do … end` template reads the ancestor chain (#275, #310).

use crate::lang_helpers::elixir::{
    elixir_applying_operator, elixir_call_keyword, elixir_is_class_macro,
    elixir_is_inside_quote_block, elixir_is_method_macro,
};
use crate::role_table;
use crate::{Ancestors, Elixir, Node};

/// Whether an operator token applies its operator: its parent is a
/// `binary_operator`. A token also *names* the operator — `&==/2`,
/// `&and/2` and `Kernel.||(a, b)` hold it inside an
/// `operator_identifier`, and a single-token `atom` / `keyword` spells
/// it too — and under error recovery its parent can be an `ERROR` with
/// no operands, so every decision metric keys on the parent (#1531,
/// #1534). A `Kernel` remote call that names a short-circuit operator
/// applies it too, but has no `binary_operator`: that spelling is
/// text-keyed on the receiver and stays the consumers' named extra
/// (#1574, #1582).
fn applied<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    elixir_applying_operator(node, ancestors).is_some()
}

/// Whether a `binary_operator`'s `operator` field is one of the
/// `Comparison` payers; the three aliased ids carry every binary
/// operator.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_operator`'s `operator` field is a short-circuit
/// payer.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

/// A `call` whose target spells a method-defining macro, outside any
/// `quote` block — `ElixirCode::is_func_with_code`'s rule.
fn method_macro<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    elixir_call_keyword(node, code).is_some_and(elixir_is_method_macro)
        && !elixir_is_inside_quote_block(node, code, ancestors)
}

/// A `call` whose target spells `defmodule`.
fn class_macro<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    elixir_call_keyword(node, code).is_some_and(elixir_is_class_macro)
}

role_table! {
    lang: Elixir;

    // Both rows gate the same `Call` kind on different target text, so
    // one kind sits in two rows of two *different* roles — allowed; the
    // same-role duplicate is what the macro rejects. `AnonymousFunction`
    // (`fn … end`) opens a space and is a closure (`checker/elixir.rs`).
    FuncOpener {
        owns [
            Call(text_gated_by method_macro opens Function),
            AnonymousFunction(opens Function),
        ]
    }
    Container {
        owns [
            Source(opens Unit),
            Call(text_gated_by class_macro opens Class),
        ]
    }
    Closure { owns [AnonymousFunction] }
    Call { owns [Call] }
    StringLit { owns [String, Charlist, Sigil] wraps [Interpolation] }

    NullSafe { absent "Elixir has no `?.` or `??`" }
    ElseIf { absent "Elixir has no `else if`; multi-way branching is `cond`" }
    Modifier { absent "Elixir's `defp` is a declaration keyword, not a modifier" }
    Primitive { absent "Elixir has no built-in type names in syntax" }
    DocComment { absent "Elixir documents with `@doc` attributes, not comments" }
    // The `binary_operator` node — three aliased ids — carries every
    // binary operator; its `operator` field says which, so each row's
    // owner is gated on the field, and every payer on applying its
    // operator under one (`applied`), since a token also names the
    // operator in a capture, an atom or a `Kernel.op(a, b)` call. The
    // comparison payers are the four equalities, the four orderings
    // and the membership tests `in` / `not in` (the compound is one
    // token).
    Comparison {
        owns [
            BinaryOperator(gated_by comparison_operator),
            BinaryOperator2(gated_by comparison_operator),
            BinaryOperator3(gated_by comparison_operator),
        ]
        pays [
            EQEQ gated_by applied, EQEQEQ gated_by applied,
            BANGEQ gated_by applied, BANGEQEQ gated_by applied,
            LTEQ gated_by applied, GTEQ gated_by applied,
            LT gated_by applied, GT gated_by applied,
            In gated_by applied, Notin gated_by applied,
        ]
        project "operator"
    }
    // `&&` / `||` and the strict-boolean `and` / `or`, each a kind of
    // its own. A `Kernel.||(a, b)` / `Kernel.and(a, b)` remote call
    // applies the same macro and decides as the operator does; it has
    // no `binary_operator` and its `and` / `or` are a plain
    // `identifier`, so it is the consumers' text-keyed extra, not this
    // row's.
    ShortCircuit {
        owns [
            BinaryOperator(gated_by short_circuit_operator),
            BinaryOperator2(gated_by short_circuit_operator),
            BinaryOperator3(gated_by short_circuit_operator),
        ]
        pays [
            AMPAMP gated_by applied, PIPEPIPE gated_by applied,
            And gated_by applied, Or gated_by applied,
        ]
        project "operator"
    }
    EagerLogical { pending R05 }
    Ternary { absent "Elixir has no ternary operator" }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    Exit { pending R07 }
    // `fn` is the one declaration keyword the grammar gives a kind of
    // its own. `def` / `defp` / `defmodule` / `defmacro` are not
    // keywords: each is the `identifier` target of a `Call`, told from
    // any other call by its text, and is `FuncOpener` / `Container`
    // territory (R06); the `Call` row (R08) bills the call.
    DeclKeyword { owns [Fn] }
    Member { pending R06 }
    Comment { pending R09 }
    Docstring { pending R09 }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;
    use crate::{Checker, ElixirCode, Getter, SpaceKind};

    // The rows restate `checker/elixir.rs` and `getter/elixir.rs`; this
    // pins the restatement node by node (lesson 35), against the
    // `_with_code` spellings the walk reads (grammar-dispatch §7). The
    // per-shape tests below do not cover membership: emptying `Call` or
    // dropping `Sigil` from `StringLit` failed none of them.
    #[cfg(feature = "elixir")]
    #[test]
    fn membership_rows_agree_with_the_live_classifiers() {
        let source = b"defmodule M do\n  def f(x), do: fn y -> y end\n  defp g, do: ~s(a)\n  defmacro m do\n    quote do\n      def h, do: 'c'\n    end\n  end\n  IO.puts(\"a #{b}\")\nend\n";
        let mut seen_kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            assert_eq!(
                call::owns(node, source, ancestors),
                ElixirCode::is_call_with_code(node, source, ancestors),
                "{kind}: Call"
            );
            assert_eq!(
                string_lit::owns(node, source, ancestors),
                ElixirCode::is_string_with_code(node, source, ancestors),
                "{kind}: StringLit"
            );
            assert_eq!(
                closure::owns(node, source, ancestors),
                ElixirCode::is_closure(node, ancestors),
                "{kind}: Closure"
            );
            assert_eq!(
                func_opener::owns(node, source, ancestors)
                    || container::owns(node, source, ancestors),
                ElixirCode::promotes_to_func_space_with_code(node, source, ancestors),
                "{kind}: space promotion"
            );
            assert_eq!(
                opens_space(node, source, ancestors),
                ElixirCode::get_space_kind_with_code(node, source, ancestors),
                "{kind}: SpaceKind"
            );
            seen_kinds.insert(node.kind_id());
        });
        for kinds in [
            call::OWNERS,
            string_lit::OWNERS,
            closure::OWNERS,
            func_opener::OWNERS,
            container::OWNERS,
        ] {
            for &k in kinds {
                assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
            }
        }
    }

    // Every `call` in the fixture, with what the two text-gated rows
    // make of it: the `defmodule` is a container, the `def` a function
    // opener, the `def` inside `quote` neither — the chain says so, and
    // the same bytes outside a `quote` say the opposite. `IO.puts` is a
    // remote call with no keyword target, `quote` itself is neither, and
    // the `f(x)` head of `def f(x)` is a plain call nested in the `def`.
    #[cfg(feature = "elixir")]
    #[test]
    fn text_gated_rows_read_the_target_bytes_and_the_chain() {
        let source = b"defmodule M do\n  def f(x), do: x\n  defmacro m do\n    quote do\n      def g, do: 1\n    end\n  end\n  IO.puts(1)\nend\n";
        let mut calls = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() != Elixir::Call as u16 {
                return;
            }
            let ancestors = Ancestors::known(chain);
            let target = node
                .child_by_field_name("target")
                .and_then(|t| t.utf8_text(source))
                .unwrap_or("<none>")
                .to_owned();
            calls.push((
                target,
                func_opener::owns(node, source, ancestors),
                container::owns(node, source, ancestors),
                opens_space(node, source, ancestors),
            ));
        });
        assert_eq!(
            calls,
            [
                ("defmodule".to_owned(), false, true, SpaceKind::Class),
                ("def".to_owned(), true, false, SpaceKind::Function),
                ("f".to_owned(), false, false, SpaceKind::Unknown),
                ("defmacro".to_owned(), true, false, SpaceKind::Function),
                ("quote".to_owned(), false, false, SpaceKind::Unknown),
                ("def".to_owned(), false, false, SpaceKind::Unknown),
                ("IO.puts".to_owned(), false, false, SpaceKind::Unknown),
            ]
        );
    }

    // The same `def` bytes with no `quote` ancestor: the row answers
    // from the chain, not from the text alone.
    #[cfg(feature = "elixir")]
    #[test]
    fn the_quote_gate_depends_on_the_ancestor_chain() {
        let source = b"def g, do: 1\n";
        let mut defs = 0;
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() == Elixir::Call as u16 {
                assert!(func_opener::owns(node, source, Ancestors::known(chain)));
                defs += 1;
            }
        });
        assert_eq!(defs, 1);
    }

    // The anonymous function opens a space and is a closure; the `def`
    // opens a space and is not; `Call` is in both space rows without
    // being named twice in either.
    #[cfg(feature = "elixir")]
    #[test]
    fn closure_and_opener_are_two_declarations() {
        let source = b"def f, do: fn x -> x end\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let closure = closure::owns(node, source, ancestors);
            let space = opens_space(node, source, ancestors);
            if closure || space != SpaceKind::Unknown {
                seen.push((node.kind(), closure, space));
            }
        });
        assert_eq!(
            seen,
            [
                ("source", false, SpaceKind::Unit),
                ("call", false, SpaceKind::Function),
                ("anonymous_function", true, SpaceKind::Function),
            ]
        );
        assert_eq!(
            func_opener::OWNERS,
            &[Elixir::Call as u16, Elixir::AnonymousFunction as u16]
        );
        assert_eq!(
            container::OWNERS,
            &[Elixir::Source as u16, Elixir::Call as u16]
        );
    }
}
