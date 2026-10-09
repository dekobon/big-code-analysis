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

use super::{Abc, Stats, last_operand, wrapped_operand};
use crate::lang_helpers::elixir::elixir_call_keyword;
use crate::*;

/// The grammar rule an applied binary operator hangs off, as opposed to
/// `operator_identifier`, which is how one is *named*
/// (`&</2`, `Kernel.<(a, b)`).
///
/// Matched by name rather than by `kind_id` because the grammar aliases
/// this one rule to three ids (`Elixir::BinaryOperator` through
/// `BinaryOperator3`).
const BINARY_OPERATOR: &str = "binary_operator";

// One step of the value peel: the operand a wrapper evaluates to, or
// `None` for anything that is not a wrapper this peel descends. A
// wrapper adds no decision of its own, so a slot looks through it to
// what is actually tested.
//
// Both of Elixir's negations, not just `!`: the keyword `not` raises on
// a non-boolean operand where `!` accepts any truthy value, but ABC
// counts the negation, not its strictness. The other `unary_operator`s
// (`-x`, `@attr`, `&f/1`, `^pin`) yield a value rather than wrapping a
// test, so the peel declines them and the slot pays for them.
//
// A parenthesised `block` evaluates to its *last* expression
// (`(a; b)` is `b`), and a match `=` to its `right` side — so
// `if (y = x > 1)` tests the comparison, which its own arm already
// counts. Every slot is read by role (field, or last operand), never by
// position, because a comment is named and may sit before the operand
// (`(# c⏎ b)`, #1455).
fn elixir_wrapper_operand<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    use Elixir as E;

    match node.kind_id().into() {
        E::Block => last_operand(node),
        E::UnaryOperator => node
            .child_by_field_name("operator")
            .filter(|op| matches!(op.kind_id().into(), E::BANG | E::Not))
            .and_then(|_| node.child_by_field_name("operand")),
        _ if elixir_binary_operator_is(node, |op| op == E::EQ) => node.child_by_field_name("right"),
        _ => None,
    }
}

// Whether `node` is a `binary_operator` whose operator token satisfies
// `accept`.
fn elixir_binary_operator_is(node: &Node, accept: impl Fn(Elixir) -> bool) -> bool {
    node.kind() == BINARY_OPERATOR
        && node
            .child_by_field_name("operator")
            .is_some_and(|op| accept(op.kind_id().into()))
}

// Whether another arm of `compute` already charges `expr` as a
// condition, looking through the wrappers `elixir_wrapper_operand`
// descends. True for a comparison or membership test (the two token
// arms) and for an `&&` / `||` / `and` / `or` chain, whose walker scores
// each operand through `elixir_count_condition` — so every operand
// either pays there or is one of these, and a chain always carries at
// least one condition of its own.
//
// The operator list must agree with the token arms in `compute`: an
// operator listed here and counted nowhere would leave its slot at zero,
// and one counted there but missing here would pay twice.
fn elixir_condition_scores_itself(expr: &Node) -> bool {
    use Elixir as E;

    let mut node = *expr;
    while let Some(operand) = elixir_wrapper_operand(&node) {
        node = operand;
    }
    elixir_binary_operator_is(&node, |op| {
        matches!(
            op,
            E::EQEQ
                | E::EQEQEQ
                | E::BANGEQ
                | E::BANGEQEQ
                | E::LTEQ
                | E::GTEQ
                | E::LT
                | E::GT
                | E::In
                | E::Notin
                | E::AMPAMP
                | E::PIPEPIPE
                | E::And
                | E::Or
        )
    })
}

// Scores one boolean slot — an `if` / `unless` predicate, a `cond`
// clause's condition, a guard alternative, or an operand of an `&&` /
// `||` chain — as Fitzpatrick's "unary conditional expression" (Rule
// 6 / 7 / 9): one condition, unless another arm already charged the
// same decision. The port of `ruby_count_condition` (#1520, #1529).
//
// Elixir's slots used to pay only for an occupant that peeled down to a
// fixed list of terminal kinds, and the `if` / `unless` / `cond` Calls
// did not use a slot at all: they paid a flat one on top of whatever
// their predicate scored, so `if x > 5` cost two against Ruby's and
// Java's one, and `if a && b` three against their two (#1527). Asking
// whether the decision is already paid for, rather than whether the
// occupant is a known terminal, also scores what the list never held —
// `a && x + 1`, `a && @flag`, `a && -x` each scored one below `a && b`.
fn elixir_count_condition(condition: &Node, conditions: &mut f64) {
    if !elixir_condition_scores_itself(condition) {
        *conditions += 1.;
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
    if let Some(alternative) = npa::elixir_when_alternative(when_operator) {
        elixir_count_condition(&alternative, conditions);
    }
}

// One arm of a `cond` (#1527). `cond` is an `if` / `else if` chain
// spelled as clauses, so each clause's condition is an `if` predicate
// slot, and `cond do x > 5 -> …; b -> … end` scores what its nested-`if`
// twin does. It paid a flat one per construct instead, which
// double-counted a comparison clause and undercounted every clause
// after the first.
//
// The unguarded `true ->` catch-all is `cond`'s `default:` and scores
// nothing, by the gate `Cyclomatic` uses for the same clause (§7, §8).
// A clause with no sole unguarded condition (`x when g ->`, `a, b ->`)
// does not compile, so it scores nothing here: only invalid input could
// tell any choice apart (grammar-dispatch §6). A guarded one still pays
// its guard through the `when` arm.
fn elixir_count_cond_clause<'a>(
    clause: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    conditions: &mut f64,
) {
    if npa::elixir_is_default_clause(clause, code, ancestors) {
        return;
    }
    if let Some(condition) = npa::elixir_sole_unguarded_pattern(clause) {
        elixir_count_condition(&condition, conditions);
    }
}

// Each operand of an `&&` / `||` / `and` / `or` chain is a boolean slot
// (Fitzpatrick Rule 9). tree-sitter-elixir parses `a && b || c` as a
// left-nested chain of `binary_operator`s, so an operand that is itself
// a chain is paid by its own operator's visit. Read by field: a comment
// beside the operator is a named child of the `binary_operator` too,
// and must not pay.
fn elixir_count_chain_operands(chain: &Node, conditions: &mut f64) {
    for field in ["left", "right"] {
        if let Some(operand) = chain.child_by_field_name(field) {
            elixir_count_condition(&operand, conditions);
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
    match keyword {
        // The predicate is the Call's first argument in both the block
        // (`if p do … end`) and keyword (`if p, do: …`) forms, and is a
        // slot (#1527). `arguments` carries five kind aliases at this
        // pin, so it is matched by rule name (grammar-dispatch §1).
        Some("if" | "unless") => {
            if let Some(predicate) = node
                .children()
                .find(|c| c.kind() == "arguments")
                .and_then(|args| wrapped_operand(&args))
            {
                elixir_count_condition(&predicate, &mut stats.conditions);
            }
        }
        // A `case` / `with` pays once per construct, not per clause; a
        // `cond` pays per clause through its `StabClause` arm.
        Some("case" | "with") => stats.conditions += 1.,
        _ => {}
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
    // Conditions cover the comparison and membership operator tokens
    // (`==`, `===`, `!=`, `!==`, `<`, `>`, `<=`, `>=`, `in`, `not in`),
    // the boolean slots scored by `elixir_count_condition` (`if` /
    // `unless` predicates, `cond` clauses, guards, chain operands), and
    // one per `case` / `with` construct.
    // `for` / `while` are looping forms — not condition-shaped per
    // the issue body's literal list — so we omit them.
    //
    // Limitations:
    // - `case` / `with` are counted once on the container, not once per
    //   arm (`stab_clause`), matching the Rust impl's "MatchExpression
    //   once" rule. `cond` left that rule in #1527: its clauses are
    //   boolean predicates, not patterns, so each is an `if` slot.
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
        // bca: suppress(halstead, cyclomatic) — exhaustive kind dispatch table
        // One arm per grammar kind, like `GoCode::compute`: the count is
        // the number of node kinds the Elixir grammar can hand us, and
        // `halstead.effort` counts the distinct enum operands those arms
        // name, neither being reasoning a reader must do. The `cond`
        // clause arm (#1527) took both past their limits; each arm is
        // independent and there is no boundary to split on.
        use Elixir as E;

        match node.kind_id().into() {
            // A `BinaryOperator` whose operator token is `EQ` is a
            // pattern-match assignment. Read through the `operator`
            // field rather than at child index 1, where a comment
            // before the operator (`a # c⏎ = 1`) sits instead (#1455);
            // a field read also avoids an `any()` scan of all children
            // on an arm that fires on every Elixir binary op.
            E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3
                if node
                    .child_by_field_name("operator")
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
            // the read.
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
            // Fitzpatrick Rule 9 walker: each operand of a `&&` / `||` /
            // `and` / `or` chain is a boolean slot (issue #557, #1527).
            // The short-circuit operators are not counted directly
            // (cross-language policy, #395). Under error recovery the
            // token's parent can be an `ERROR` node, which has no
            // operands to score.
            E::AMPAMP | E::PIPEPIPE | E::And | E::Or => {
                if let Some(chain) = ancestors
                    .parent(node)
                    .filter(|parent| parent.kind() == BINARY_OPERATOR)
                {
                    elixir_count_chain_operands(&chain, &mut stats.conditions);
                }
            }
            E::StabClause if npa::elixir_is_cond_clause(node, code, ancestors) => {
                elixir_count_cond_clause(node, code, ancestors, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
