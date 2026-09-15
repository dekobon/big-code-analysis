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
use crate::lang_helpers::elixir::elixir_call_keyword;
use crate::macros::elixir_bool_terminal_kinds;
use crate::*;

/// The grammar rule an applied binary operator hangs off, as opposed to
/// `operator_identifier`, which is how one is *named*
/// (`&</2`, `Kernel.<(a, b)`).
///
/// Matched by name rather than by `kind_id` because the grammar aliases
/// this one rule to three ids (`Elixir::BinaryOperator` through
/// `BinaryOperator3`).
const BINARY_OPERATOR: &str = "binary_operator";

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
        // Both of Elixir's negations, not just `!`. The keyword `not` is
        // the stricter of the two — it raises on a non-boolean operand,
        // where `!` accepts any truthy value — so it is at least as good
        // a proof that what it wraps is boolean. Listing only `BANG`
        // scored `a && not b` one condition against `a && !b`'s two, and
        // would have dropped `when not is_nil(y)` to zero once the guard
        // became a slot below.
        let is_not = matches!(node_kind, E::UnaryOperator)
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), E::BANG | E::Not));

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

// The Elixir sibling of `java_count_condition` / `ruby_count_condition`:
// classifies one boolean *slot* — a position whose occupant is evaluated
// for truth — and adds at most one condition for it.
//
// A slot adds nothing for an operator-spelled occupant: `y > 5` is a
// `binary_operator`, absent from `elixir_bool_terminal_kinds!()`, and
// the `>` token arm in `compute` already owns that one. Counting it here
// too is the `.claude/rules/grammar-dispatch.md` §5 double count, and is
// what made an Elixir guard's score depend on its spelling. `Block`
// (`(y)`) and `UnaryOperator` (`!y`, `not y`) are wrappers rather than
// occupants, so they are peeled by `elixir_inspect_container`.
fn elixir_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Elixir as E;

    let kind = condition.kind_id().into();
    if matches!(kind, elixir_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, E::Block | E::UnaryOperator) {
        elixir_inspect_container(condition, parent, conditions);
    }
}

// The guard slot of an anchored `when` operator.
//
// Repeated guards (`when a when b`) are an or-chain — Elixir tries each
// alternative in turn, moving on when the previous one is false or
// raises — so the construct carries one slot per alternative, level
// with the `when a or b` spelling. They nest right-associatively, and
// `elixir_when_is_guard` anchors every `when` token in the chain, so
// the slot this fills is the single alternative *this* token
// introduces: one per token, never the whole chain once per token
// (grammar-dispatch §5).
//
// `elixir_when_alternative` answers `None` only when a `when`
// `binary_operator` has no `right` child, which the grammar declares
// required — so the `if let`'s else is unreachable at the pin rather
// than untested.
fn elixir_count_guard(when_operator: &Node, conditions: &mut f64) {
    if let Some((alternative, owner)) = npa::elixir_when_alternative(when_operator) {
        elixir_count_condition(&alternative, &owner, conditions);
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

// What an Elixir `Call` contributes. The classification is by keyword
// text rather than by kind, so it is a paragraph of policy rather than a
// dispatch arm — the same split `java_count_token_branch` and
// `csharp_count_token_assignment` make in the two largest sibling impls.
fn elixir_count_call(node: &Node, code: &[u8], stats: &mut Stats) {
    let keyword = elixir_call_keyword(node, code);
    let is_definition_or_directive = matches!(
        keyword,
        Some(
            "def"
                | "defp"
                | "defmacro"
                | "defmacrop"
                | "defmodule"
                | "defstruct"
                | "defprotocol"
                | "defimpl"
                | "alias"
                | "import"
                | "require"
                | "use"
        )
    );
    if !is_definition_or_directive {
        stats.branches += 1.;
    }
    // Keyword-shaped control-flow Calls also contribute one condition.
    if matches!(keyword, Some("if" | "unless" | "case" | "cond" | "with")) {
        stats.conditions += 1.;
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
                if node.child(1).is_some_and(|c| c.kind_id() == E::EQ as u16) =>
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
            E::Call => elixir_count_call(node, code, stats),
            E::EQEQ | E::EQEQEQ | E::BANGEQ | E::BANGEQEQ | E::LTEQ | E::GTEQ => {
                stats.conditions += 1.;
            }
            // Guard `when` token: introduces the guard clause of a
            // function head or `case` / `fn` / `receive` arm. The guard
            // is a condition *slot*, so every spelling contributes
            // exactly one — the condition-slot model #1422 gave C# and
            // #1454 gave Java, Rust, Python and Ruby.
            //
            // Elixir was the one language of the six that did not get
            // the slot. It added a flat one for the `when` token *on top
            // of* whatever the guard's sub-structure already scored, so
            // `when y > 5` cost two where `when is_integer(y)` and
            // `when y` cost one — precisely the spelling-dependence the
            // slot exists to remove, and a §5 double count with the `>`
            // token arm below. Routing the guard expression through
            // `elixir_count_condition` puts all four spellings at one:
            // a value-bearing guard scores in the slot, an
            // operator-spelled one scores through the operator's own
            // arm.
            //
            // By grammar FIELD, not index (grammar-dispatch §3): Elixir
            // has no `guard` production — `when` is a `binary_operator`
            // whose `left` is the head being guarded and whose `right`
            // is the guard — so a comment between the two cannot shift
            // the read. That the operator *is* a `binary_operator` is
            // also why `elixir_inspect_container` needs no new
            // `has_boolean_content` seed for this slot the way its Java,
            // Rust and Ruby siblings did: the seed list already opens
            // with the three `BinaryOperator` aliases, so `when (y)` and
            // `when !y` are proven boolean for free.
            //
            // The gate is #1454's, shared with the `Cyclomatic` impl
            // that carries the matching decision (§7). Elixir has no
            // dedicated guard production, and a typespec's binding
            // clause (`@spec f(a) :: a when a: integer`) spells the same
            // token: it scored a condition here against no decision
            // anywhere, on type syntax that branches on nothing.
            //
            // The `if let` cannot take its else: the gate already walked
            // this token's ancestor chain and returns false when it is
            // empty, so reaching the body proves the parent exists. It
            // is not re-plumbed out of the predicate because that
            // predicate's whole job (§7) is to be one boolean the `Abc`
            // and `Cyclomatic` impls share.
            E::When if npa::elixir_when_is_guard(node, code, ancestors) => {
                if let Some(operator) = ancestors.parent(node) {
                    elixir_count_guard(&operator, &mut stats.conditions);
                }
            }
            // `in` / `not in` are Elixir's membership and type tests
            // (`x in [1, 2]`, `rescue e in RuntimeError`) — relational
            // operators, which Fitzpatrick Rule 5 scores by use. #1461
            // moved exactly this class onto an unconditional arm in C#,
            // Java, Groovy, Kotlin and Ruby but did not reach Elixir, so
            // `x in y` scored zero where `x == y` scored one. The gap
            // stayed invisible while the `when` arm above paid a flat
            // one for every guard; as a slot, `when x in [1, 2]` would
            // have fallen to zero, so the arm the slot model presumes —
            // every operator-spelled guard is owned by an operator arm —
            // has to exist for `in` as it already does for `>`.
            //
            // Sharing the `<` / `>` gate rather than standing alone
            // because `in` has the same three grammar positions: a
            // `grammar.json` sweep of the pinned tree-sitter-elixir
            // finds it in `binary_operator`, in `operator_identifier`
            // (`&in/2`) and in `_remote_dot` (`Kernel.in(a, b)`), the
            // last two being how an operator is *named* rather than
            // applied. `not in` lexes as one token and so has no inner
            // `not` leaf to double count (§5).
            //
            // Counts all four only as the operator token of a
            // `binary_operator`, the allowlist polarity the rest of the
            // workspace moved to in #1274 and #1297. The previous
            // denylist excluded a sigil delimiter (`~s<hi>`, #1256) and
            // nothing else, on the reasoning that "Elixir has no
            // Go-style generic-instantiation brackets, so what remains
            // is a genuine comparison" — a coverage claim the grammar
            // contradicts. A `grammar.json` sweep of the pinned
            // tree-sitter-elixir finds a bare `<` / `>` in
            // `binary_operator`, the two quoted-angle sigil rules, and
            // `operator_identifier`, which is how an operator is
            // *named* rather than applied: `&</2` and `Kernel.<(a, b)`
            // each scored a condition against zero decisions, the same
            // shape as the C# `operator <` declaration #1297 fixed.
            // (`:<` atoms and `<:` keywords lex as single tokens and
            // never reach here, as do `<=` / `>=` and `<<` / `>>`.)
            //
            // Matched by rule name rather than by `kind_id`: this
            // grammar aliases `binary_operator` to three ids
            // (`E::BinaryOperator`, `BinaryOperator2`,
            // `BinaryOperator3`), and a name comparison stays correct
            // when a bump adds a fourth
            // (`.claude/rules/grammar-dispatch.md` §1, the same call
            // `QUOTED_CONTENT` makes in `src/metrics/loc/elixir.rs`).
            // The runtime cost that trade buys there is not paid here:
            // the guard runs only for one of these four tokens, not for
            // every node.
            E::LT | E::GT | E::In | E::Notin
                if ancestors
                    .parent(node)
                    .is_some_and(|parent| parent.kind() == BINARY_OPERATOR) =>
            {
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
