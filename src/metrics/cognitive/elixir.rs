//! `Cognitive` implementation for Elixir.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::elixir::{
    elixir_applying_remote_call, elixir_short_circuit_kind, elixir_short_circuit_name,
};
use crate::metrics::elixir_decisions::{elixir_pair_section, elixir_receive_dispatches};

/// Whether a Call spelling `keyword` takes `try`'s sections — `else`,
/// `rescue`, `catch` — in its keyword form: `try` itself, and the
/// method-defining macros, whose body is an implicit `try`
/// (`def f, do: g(), rescue: (e -> e)`).
fn elixir_takes_try_sections(keyword: &str) -> bool {
    keyword == "try" || elixir_is_method_macro(keyword)
}

/// The cost a keyword-form section carries, matching its block form.
enum ElixirSection {
    /// `else:`, which adds +1 as the `Else` token does.
    Else,
    /// `rescue:` / `catch:`, which add +nesting as `RescueBlock` /
    /// `CatchBlock` do.
    Handler,
}

/// The section `pair` spells, when its Call takes it (#1574). Only such
/// a Call scores it, because `else:` is an ordinary option name to any
/// other function; a `with`'s `else:` is that construct's clause
/// dispatch, which its own +nesting already paid.
fn elixir_section<'a>(
    pair: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<ElixirSection> {
    let (key, call) = elixir_pair_section(pair, code, ancestors)?;
    let keyword = elixir_call_keyword(&call, code)?;
    match key {
        "else" if matches!(keyword, "if" | "unless") || elixir_takes_try_sections(keyword) => {
            Some(ElixirSection::Else)
        }
        "rescue" | "catch" if elixir_takes_try_sections(keyword) => Some(ElixirSection::Handler),
        _ => None,
    }
}

/// Folds an Elixir `BinaryOperator`'s short-circuit operator children
/// into the boolean-sequence counter — Elixir has four (`&&`, `||`,
/// `and`, `or`). Each word form is keyed to its symbol (see
/// [`compute_booleans_keyed`] and [`elixir_short_circuit_kind`]).
fn compute_elixir_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_keyed(node, stats, |id| {
        elixir_short_circuit_kind(id).map(|key| key as u16)
    });
}

/// [`compute_elixir_booleans`] for a short-circuit `Kernel` remote call
/// (`Kernel.||(a, b)`, `Kernel.or(a, b)`), whose operator sits in the
/// call's name rather than among a `binary_operator`'s children. The
/// sequence is keyed on the call's span, as a `binary_operator`'s is on
/// its own, and on the same symbol, so `a or Kernel.||(b, c)` is one run.
fn compute_elixir_remote_booleans<'a>(
    op: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    if let Some(remote) = elixir_applying_remote_call(op, code, ancestors)
        && let Some(key) = elixir_short_circuit_name(op, code)
    {
        stats.structural = stats.boolean_seq.eval_based_on_prev(
            key as u16,
            remote.call.end_byte(),
            stats.structural,
        );
    }
}

impl Cognitive for ElixirCode {
    // Elixir control flow is macro-shaped: `if`, `unless`, `case`,
    // `cond`, `with`, `receive`, `for`, `while`, and `try` each surface as a
    // `Call` node whose `target` Identifier text spells the keyword.
    // We classify the Call once on entry (raising nesting), then let
    // the structural `Else` token and `Rescue` / `Catch` blocks inside
    // the do_block contribute their own cost without double-counting.
    //
    // Mapping (mirrors the Java/Kotlin SonarSource interpretation for
    // switch-like constructs):
    // - `if` / `unless` / `for` / `while`: single-branch control flow,
    //   `+nesting`. Their `else` (token `Elixir::Else` inside an
    //   `ElseBlock`) adds `+1` without nesting, matching Java.
    // - `case` / `cond` / `with` / `receive`: switch-/multi-arm,
    //   `+nesting` once on the container. Individual `stab_clause` arms
    //   do NOT add extra cost (matches Java `SwitchBlock` / `case:`
    //   rule).
    // - `try`: `+0` on the container itself — it is a wrapper, and its
    //   `rescue` / `catch` arms surface as `RescueBlock` / `CatchBlock`
    //   and each one adds `+nesting`, matching Java's `CatchClause`
    //   treatment. See the arm comment below.
    // - `def` / `defp` / `defmacro` / `defmacrop`: method-defining
    //   macros. Nesting resets at the boundary, as in Bash's
    //   `FunctionDefinition` rule, but the function depth deliberately
    //   does NOT bump — see the arm comment below for why.
    // - `AnonymousFunction` (`fn x -> y end`): lambda nesting bumps.
    // - `&&` / `||` / `and` / `or`: boolean sequence cost, as is a
    //   `Kernel.&&` / `Kernel.||` / `Kernel.and` / `Kernel.or` remote
    //   call.
    //
    // Limitations:
    // - `Enum.reduce` / `Enum.map` and friends are higher-order function
    //   calls (`RemoteCallWithParentheses`), not syntactic control flow.
    //   Cognitive complexity per the SonarSource spec does NOT count
    //   function calls; we follow suit.
    // - Recursion detection is intentionally omitted. The SonarSource
    //   spec scores recursion at +1, but reliably detecting recursion
    //   needs symbol-table awareness (the body of `def foo do foo() end`
    //   must compare names) which is out of scope for this fix. See
    //   the issue body's explicit "skip if too complex" guidance.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — kind dispatch table; each gated arm is one rule
        use crate::roles::elixir::short_circuit;
        use Elixir as E;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            E::Call => match elixir_call_keyword(node, code) {
                Some("if" | "unless" | "for" | "while" | "case" | "cond" | "with") => {
                    increase_nesting(stats, &mut nesting);
                }
                // A `case` over the mailbox, paying as one (#1581) when
                // it has message clauses; one holding only an `after`
                // timeout is a timed wait, the idiomatic sleep, and
                // branches on nothing. The `after` adds nothing either
                // way: like `case`, the container pays and its arms do
                // not, a timeout beside a bare `_ ->` included.
                Some("receive") if elixir_receive_dispatches(node, code) => {
                    increase_nesting(stats, &mut nesting);
                }
                // `try` is intentionally absent: it is a wrapper for
                // `rescue` / `catch` arms (each of which earns its own
                // +nesting via `RescueBlock` / `CatchBlock`). Adding the
                // `try` itself would double-count, matching Java /
                // C#'s "try is a wrapper, only catch counts" rule.
                Some(kw) if elixir_is_method_macro(kw) => {
                    // Method-defining macros reset nesting at the
                    // function boundary, mirroring Bash's
                    // `FunctionDefinition` rule. We deliberately do
                    // NOT call `increment_function_depth` here: that
                    // helper matches `Call` ancestors by `kind_id`
                    // alone, and EVERY `def` inside a `defmodule` has
                    // a `Call` ancestor (the defmodule Call) which
                    // would falsely raise the function depth. Elixir
                    // does not allow `def` nested inside another
                    // `def` (defs only live at module top level), so
                    // truly nested method definitions are not a
                    // concern — the lambda channel via
                    // `AnonymousFunction` handles the analogous
                    // higher-order case. Elixir is therefore the one
                    // language that takes the resets without the depth
                    // bump, so it resets inline instead of calling the
                    // shared `enter_function_boundary` the other
                    // eighteen modules use (#1103).
                    //
                    // Both channels, not just the conditional one. `def`
                    // inside `fn` is invalid Elixir but the grammar
                    // parses it, and skipping the lambda reset here is
                    // exactly the accidental opt-out that #1187 moved
                    // the line into the shared helper to prevent — so
                    // the one language that cannot use the helper has to
                    // stay in step by hand.
                    nesting.conditional = 0;
                    nesting.lambda = 0;
                }
                _ => {}
            },
            // `else` keyword inside an `else_block` (else arm of `if`
            // / `unless` / `try`). Matches Java/Kotlin's `Else` rule: +1
            // without raising nesting.
            //
            // Not a `with`'s: its `else` holds the clauses that dispatch
            // over the value a `<-` failed to match, the arms of the
            // `case` the construct is in all but spelling, and `case`'s
            // arms add nothing past the +nesting its container paid.
            // Ruby draws the same line for the `else` of `case/when`
            // (#451); the `if` alternative and `try`'s no-exception
            // branch, which no container paid for, keep their +1. The
            // keyword form `with(…, do: …, else: (…))` scored 1 all
            // along; this puts the block form level with it (#1574).
            // The `else` token's parents are the `else_block`, then the
            // `do_block`, then the Call.
            E::Else
                if ancestors
                    .iter(node)
                    .nth(2)
                    .is_none_or(|(call, _)| elixir_call_keyword(&call, code) != Some("with")) =>
            {
                increment_by_one(stats);
            }
            // The keyword spellings of the sections above, for the
            // owners `elixir_section` names: `if(c, do: 1, else: 2)`,
            // `try(do: f(), rescue: (…))`, bare or in brackets. A user
            // macro's `foo(do: 1, else: 2)` scores nothing here while
            // its `foo do 1 else 2 end` scores the `Else` arm above.
            E::Pair => match elixir_section(node, code, ancestors) {
                Some(ElixirSection::Else) => increment_by_one(stats),
                Some(ElixirSection::Handler) => increase_nesting(stats, &mut nesting),
                None => {}
            },
            // `rescue` / `catch` arms of a `try` Call each add +nesting,
            // matching Java's `CatchClause` treatment.
            E::RescueBlock | E::CatchBlock => {
                increase_nesting(stats, &mut nesting);
            }
            // Anonymous functions are Elixir's lambdas. Increment the
            // lambda depth so the cost of control flow inside them is
            // amplified, matching Kotlin's `LambdaLiteral` rule.
            E::AnonymousFunction => {
                nesting.lambda += 1;
            }
            // Short-circuit booleans (token-form `&&` / `||` and word-
            // form `and` / `or`) contribute one structural cost per
            // operator sequence. Single-pass helper (see
            // `compute_elixir_booleans`) collapses the four operator
            // kinds in one walk of `node.children()` — the previous
            // shape called `compute_booleans` twice, walking children
            // twice per BinaryOperator.
            // A `binary_operator` the `ShortCircuit` row owns (#1695).
            _ if short_circuit::owns(node, code, ancestors) => {
                compute_elixir_booleans(node, stats);
            }
            // A short-circuit `Kernel` remote call applies the operator
            // as `a || b` does, so it opens or continues a sequence the
            // same way (#1574), keyed on the call's span as a
            // `binary_operator`'s operator is keyed on the operator's.
            // `Foo.||(a, b)` is an ordinary call and is not matched; the
            // token inside a `binary_operator` is the arm above's.
            // `Kernel.and` / `Kernel.or` name the operator with an
            // `identifier` instead of a token (#1582).
            // role-bypass: allow(ShortCircuit) — the remote-call extra: which kinds can name a
            // `Kernel` short-circuit call, decided on the receiver's text, not membership
            E::AMPAMP | E::PIPEPIPE | E::Identifier => {
                compute_elixir_remote_booleans(node, code, ancestors, stats);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
