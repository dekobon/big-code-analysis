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

/// Folds an Elixir `BinaryOperator`'s short-circuit operator children
/// into the boolean-sequence counter — Elixir has four (`&&`, `||`,
/// `and`, `or`). Single-pass walk over `node.children()` avoids the
/// 2x cost of calling the two-operator `compute_booleans` twice.
fn compute_elixir_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Elixir::AMPAMP | Elixir::PIPEPIPE | Elixir::And | Elixir::Or
        )
    });
}

impl Cognitive for ElixirCode {
    // Elixir control flow is macro-shaped: `if`, `unless`, `case`,
    // `cond`, `with`, `for`, `while`, and `try` each surface as a
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
    // - `case` / `cond` / `with` / `try`: switch-/multi-arm, `+nesting`
    //   once on the container. Individual `stab_clause` arms do NOT
    //   add extra cost (matches Java `SwitchBlock` / `case:` rule).
    //   `try`'s `rescue` / `catch` arms surface as `RescueBlock` /
    //   `CatchBlock` and each one adds `+nesting`, matching Java's
    //   `CatchClause` treatment.
    // - `def` / `defp` / `defmacro` / `defmacrop`: method-defining
    //   macros. Treated like Bash's `FunctionDefinition` — nesting
    //   resets, function depth bumps so nested functions amplify cost.
    // - `AnonymousFunction` (`fn x -> y end`): lambda nesting bumps.
    // - `&&` / `||` / `and` / `or`: boolean sequence cost.
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
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Elixir as E;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            E::Call => match elixir_call_keyword(node, code) {
                Some("if" | "unless" | "for" | "while" | "case" | "cond" | "with") => {
                    increase_nesting(stats, &mut nesting, depth, lambda);
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
                    // higher-order case.
                    nesting = 0;
                }
                _ => {}
            },
            // `else` keyword inside an `else_block` (else arm of `if`
            // / `unless` / `with` / `try`). Matches Java/Kotlin's
            // `Else` rule: +1 without raising nesting.
            E::Else => {
                increment_by_one(stats);
            }
            // `rescue` / `catch` arms of a `try` Call each add +nesting,
            // matching Java's `CatchClause` treatment.
            E::RescueBlock | E::CatchBlock => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // Anonymous functions are Elixir's lambdas. Increment the
            // lambda depth so the cost of control flow inside them is
            // amplified, matching Kotlin's `LambdaLiteral` rule.
            E::AnonymousFunction => {
                lambda += 1;
            }
            // Short-circuit booleans (token-form `&&` / `||` and word-
            // form `and` / `or`) contribute one structural cost per
            // operator sequence. Single-pass helper (see
            // `compute_elixir_booleans`) collapses the four operator
            // kinds in one walk of `node.children()` — the previous
            // shape called `compute_booleans` twice, walking children
            // twice per BinaryOperator.
            E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3 => {
                compute_elixir_booleans(node, stats);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
