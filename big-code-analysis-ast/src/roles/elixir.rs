//! Elixir's role rows: the text-gated fixture (#1691). `def` / `defp` /
//! `defmacro` / `defmacrop` / `defmodule` are not grammar productions —
//! each is a `call` whose `target` identifier spells the keyword — so
//! the rows that name them read source bytes, and the one nested in a
//! `quote do … end` template reads the ancestor chain (#275, #310).

use crate::lang_helpers::elixir::{
    elixir_applying_operator, elixir_call_keyword, elixir_is_class_macro,
    elixir_is_inside_quote_block, elixir_is_method_macro,
};
use crate::lang_helpers::elixir_decisions::{
    elixir_clause_is_decision, elixir_is_when_operator, elixir_when_is_guard,
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

/// Whether a `stab_clause` is a decision arm — of a `case`, `cond`,
/// `with … else`, `receive` or a `try` handler — and not the fallback
/// (`_ ->`, `true ->`, a `receive`'s `after`), nor an anonymous
/// function's head (`elixir_decisions` has the shapes, #1454).
fn is_decision_clause<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    elixir_clause_is_decision(node, code, ancestors)
}

/// Whether a `binary_operator` is a `when` operator — a guard's, or a
/// typespec's binding clause (`@spec f(a) :: a when a: integer`), which
/// only the operator's position tells apart (#1454). That position is
/// read from the *token's* ancestor chain by the payer gate below; the
/// owner is the operator by kind, which is what the root's guard walker
/// reaches from the token.
fn is_when_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    elixir_is_when_operator(node)
}

/// Whether a `when` token is a guard's rather than a typespec's: the
/// operator joining a clause head or function head to its condition
/// (#1454). Reads the token's ancestor chain.
fn is_guard<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    elixir_when_is_guard(node, code, ancestors)
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
            BinaryOperator(holds_payer),
            BinaryOperator2(holds_payer),
            BinaryOperator3(holds_payer),
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
            BinaryOperator(holds_payer),
            BinaryOperator2(holds_payer),
            BinaryOperator3(holds_payer),
        ]
        pays [
            AMPAMP gated_by applied, PIPEPIPE gated_by applied,
            And gated_by applied, Or gated_by applied,
        ]
        project "operator"
    }
    EagerLogical { absent "Elixir's `and` / `or` and `&&` / `||` all short-circuit; `&&&` / `|||` are \
         bitwise" }
    Ternary { absent "Elixir has no ternary operator" }
    // Every multi-way branch — `case`, `cond`, `with … else`, `receive`,
    // a `try`'s `rescue` / `catch` / `else` — is a `Call` whose clauses
    // are `stab_clause`s, as are an anonymous function's heads; which of
    // them is a decision is a question of position and bytes the gate
    // answers (#1454, lesson 11): the owner pays at the node, since the
    // clause's `->` is punctuation. Which call the clause sits in is
    // read from the ancestor chain, so this owner is never asked with
    // `Ancestors::unknown()`.
    CaseArm { owns [StabClause(text_gated_by is_decision_clause)] }
    // A guard is the `when` operator joining a clause or function head
    // to its condition; the token pays, gated to that position by its
    // ancestor chain, since a typespec's `when a: integer` spells the
    // same token on type syntax (#1454). The `binary_operator` applying
    // `when` owns it: whether that operator is a guard's is the token's
    // question, so the owner is wider than the payer and no consumer
    // reads it at the wrapper — ABC and cyclomatic both count the token.
    Guard {
        owns [
            BinaryOperator(gated_by is_when_operator),
            BinaryOperator2(gated_by is_when_operator),
            BinaryOperator3(gated_by is_when_operator),
        ]
        pays [When gated_by is_guard]
        project "operator"
    }
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

    // The `Guard` row's owner is every `when` operator by kind, and its
    // `when` token pays only in a guard position: a typespec's `when a:
    // var` is the same operator and token, told apart by position alone
    // (#1454). The consumers read the row at the token, so only this
    // pins the owner side — and that the two projections sit on the
    // same nodes, the operator owning and its token paying.
    #[cfg(feature = "elixir")]
    #[test]
    fn guard_operators_own_and_their_tokens_pay_in_guard_position_only() {
        let source = b"defmodule M do\n  @spec f(a) :: a when a: var\n  def f(x) when is_integer(x), do: x\n  def g(x) do\n    case x do\n      y when y > 0 -> 1\n      _ -> 0\n    end\n  end\nend\n";
        let (mut owned, mut paid, mut tokens) = (Vec::new(), Vec::new(), Vec::new());
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if guard::owns(node, source, ancestors) {
                owned.push(text());
            }
            if node.kind_id() == Elixir::When as u16 {
                tokens.push(node.start_byte());
                if guard::pays_at_leaf(node, source, ancestors) {
                    paid.push(node.start_byte());
                    let operator = ancestors.parent(node).expect("a `when` has a parent");
                    assert!(guard::owns(&operator, source, ancestors), "{}", text());
                }
            }
        });
        // Every `when` operator owns — the typespec's included, since the
        // kind alone cannot tell it apart — and `y > 0` owns nothing.
        assert_eq!(
            owned,
            [
                "f(a) :: a when a: var",
                "f(x) when is_integer(x)",
                "y when y > 0"
            ]
        );
        // Three `when` tokens; the typespec's, first, pays nothing.
        assert_eq!(tokens.len(), 3);
        assert_eq!(paid, tokens[1..]);
    }
}
