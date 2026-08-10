#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{Abc, Stats};
use crate::macros::elixir_bool_terminal_kinds;
use crate::*;

// Elixir ABC unary-conditional walker (Fitzpatrick Rule 9; issue #557).
// tree-sitter-elixir parses `a && b || c` as a left-nested chain of
// `binary_operator` nodes (aliased `BinaryOperator`..`BinaryOperator3`
// per lesson #2) carrying `&&` / `||` / `and` / `or` operator tokens.
// Negation surfaces as `unary_operator` whose child(0) is the `!` token;
// parenthesised operands parse as `block`. Both are unwrapped by
// `elixir_inspect_container`.
fn elixir_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // See `cpp_inspect_container` for the shared rationale: one loop peels
    // `(...)` / `!...` layers while carrying a single boolean-context flag.
    use Elixir as E;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3
    );

    loop {
        let is_block = matches!(node_kind, E::Block);
        let is_not = matches!(node_kind, E::UnaryOperator)
            && node.child(0).is_some_and(|c| c.kind_id() == E::BANG as u16);

        if !is_block && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // A `!` unary stores its operand at child index 1 (after the `!`
        // token); a parenthesised `block` carries its inner expression as
        // the first named child.
        let next = if is_not {
            node.child(1)
        } else {
            node.children().find(Node::is_named)
        };
        let Some(child) = next else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, elixir_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Counts each non-comparison operand of an Elixir `&&` / `||` chain once.
// Comparison operands are nested `binary_operator` nodes (absent from
// `elixir_bool_terminal_kinds!()`) and so contribute nothing.
fn elixir_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Elixir as E;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, elixir_bool_terminal_kinds!())
                && matches!(
                    list_kind,
                    E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3
                )
            {
                *conditions += 1.;
            } else if node.is_named() {
                elixir_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for ElixirCode {
    // Elixir's pattern-match `=` is a `BinaryOperator` whose middle
    // child is an `EQ` token. The same wrapper node also hosts `+=`-
    // style augmented assignments, but Elixir is purely functional —
    // augmented assignment does not exist in the grammar; `EQ` is the
    // only assignment-shaped operator. `|>` (`PIPEGT`) is a
    // BinaryOperator too but its operator token differs, so the EQ
    // child check is what filters assignments from pipelines and from
    // comparison operators that share the wrapper.
    //
    // Branches cover `|>` (the pipe operator dispatches one call per
    // step) and every `Call` node (function / method / macro
    // invocation). `RemoteCallWithParentheses` and `LocalCallWith*`
    // variants are subordinate nodes to `Call`, so the single `Call`
    // match captures every dispatch site.
    //
    // Conditions cover `when` (guard token `Elixir::When`), the six
    // comparison operator tokens (`==`, `===`, `!=`, `!==`, `<`, `>`,
    // `<=`, `>=`), and the keyword-shaped `Call`s that introduce a
    // decision point (`if`, `unless`, `case`, `cond`, `with`).
    // `for` / `while` are looping forms — not condition-shaped per
    // the issue body's literal list — so we omit them.
    //
    // Limitations:
    // - `case` is counted once on the container, not once per arm
    //   (`stab_clause`). The issue body says "conditions = case,
    //   cond, if, with, guard when" — i.e. one condition per
    //   construct, not per arm. Matches the Rust impl's "MatchExpression
    //   once" rule.
    // - Higher-order calls like `Enum.reduce` are `RemoteCallWithParentheses`
    //   nodes; they are still `Call` nodes and so contribute one branch
    //   each, matching the issue's "branches = `|>`, function calls"
    //   instruction.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Elixir as E;

        match node.kind_id().into() {
            // A `BinaryOperator` whose operator token is `EQ` is a
            // pattern-match assignment. The grammar shape is
            // `(left, operator, right)`, so the operator token is
            // always at child index 1 — looking it up directly is
            // O(1) vs. an `any()` scan of all children. This arm
            // fires on every Elixir binary op (comparisons, pipes,
            // boolean ops, arithmetic) so the constant-time check
            // matters.
            E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3
                if node
                    .child(1)
                    .is_some_and(|c| c.kind_id() == E::EQ as u16) =>
            {
                stats.assignments += 1.;
            }
            // `|>` pipeline operator: every step in `foo |> bar |> baz`
            // is one branch (the pipe dispatches one call per step).
            E::PIPEGT => {
                stats.branches += 1.;
            }
            // Every Call (function, method, macro, sigil-call) is one
            // branch — `RemoteCallWith*`, `LocalCallWith*`,
            // `AnonymousCall`, and `DoubleCall` are all subordinate
            // node kinds underneath the top-level `Call` wrapper, so
            // matching `Call` alone captures every dispatch site.
            //
            // Method-defining macros (`def`/`defp`/`defmacro`/`defmacrop`)
            // and module/struct/protocol declarations (`defmodule`/
            // `defstruct`/`defprotocol`/`defimpl`) are *not* runtime
            // dispatch and must not inflate `branches` — they parse as
            // `Call` nodes because Elixir's grammar uses the same
            // shape for all keyword-introduced forms. Aliasing/import
            // directives (`alias`, `import`, `require`, `use`) are
            // similarly declarative and excluded.
            //
            // Cognitive's `elixir_call_keyword` lookup is reused to
            // identify the target keyword. Note: Cognitive only acts
            // on a subset of these keywords (the four method-definers
            // for nesting reset, plus the 7 control-flow keywords for
            // +nesting); Abc's broader filter additionally drops the
            // module/struct/protocol declarators and aliasing
            // directives that Cognitive ignores entirely. Filter sets
            // are intentionally different — both impls use the same
            // helper to look up the keyword, but apply different
            // policies on top.
            E::Call => {
                let keyword = super::cognitive::elixir_call_keyword(node, code);
                let is_definition_or_directive = matches!(
                    keyword,
                    Some(
                        "def" | "defp" | "defmacro" | "defmacrop"
                        | "defmodule" | "defstruct" | "defprotocol" | "defimpl"
                        | "alias" | "import" | "require" | "use"
                    )
                );
                if !is_definition_or_directive {
                    stats.branches += 1.;
                }
                // Keyword-shaped control-flow Calls also contribute
                // one condition.
                if matches!(keyword, Some("if" | "unless" | "case" | "cond" | "with")) {
                    stats.conditions += 1.;
                }
            }
            // Sigil delimiter `<` / `>` (`~s<hi>`) are spelling, not
            // comparisons — suppressed with the same parent-is-`Sigil`
            // guard the Halstead getter uses (#1256). Of the other
            // delimiter kinds the getter guards (`SLASH` / `LPAREN` /
            // `LBRACE` / `LBRACK` / `PIPE`), none has an arm in this
            // impl, so `LT` / `GT` are the only overlap to guard.
            E::LT | E::GT
                if ancestors
                    .parent(node)
                    .is_some_and(|p| p.kind_id() == E::Sigil as u16) => {}
            // Comparison operator tokens. `Elixir::LT` / `Elixir::GT`
            // reach here only outside a sigil (the guard arm above
            // consumes the delimiter case); Elixir has no Go-style
            // generic-instantiation brackets, so what remains is a
            // genuine comparison.
            E::EQEQ | E::EQEQEQ | E::BANGEQ | E::BANGEQEQ
            | E::LT | E::GT | E::LTEQ | E::GTEQ
            // Guard `when` token: introduces the guard clause of a
            // function head or `case` arm.
            | E::When => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each non-comparison operand of a
            // `&&` / `||` / `and` / `or` chain is one condition (issue
            // #557). The short-circuit operators are not counted directly
            // (cross-language policy, #395); the keyword forms `and` / `or`
            // get the same treatment as `&&` / `||`. Combined with the
            // `if` Call already contributing one condition, `if a && b ||
            // c` reports 4 — consistent with the cyclomatic count.
            E::AMPAMP | E::PIPEPIPE | E::And | E::Or => {
                if let Some(parent) = ancestors.parent(node) {
                    elixir_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}
