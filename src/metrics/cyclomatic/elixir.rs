//! `Cyclomatic` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for ElixirCode {
    // Elixir's control-flow constructs are not distinct grammar
    // productions: `if`/`unless`/`for`/`while`/`with`/`case`/`cond`/`try`
    // all surface as `Call` nodes whose `target` field is an
    // `Identifier` whose text spells the keyword. We must consult the
    // source bytes (mirroring `impl Exit for ElixirCode`) to identify
    // them.
    //
    // The split between standard and modified CCN mirrors the C-family
    // case/switch treatment: per-arm `stab_clause` nodes contribute
    // standard, while the multi-arm container Calls (`case`/`cond`/
    // `try`) contribute modified. Single-branch keyword Calls
    // (`if`/`unless`/`for`/`while`) contribute to both, as do a `with`'s
    // `<-` clauses and short-circuit booleans (`&&`, `||`, `and`, `or`).
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic, halstead) — kind dispatch table; each gated arm is one rule
        use Elixir as E;

        match node.kind_id().into() {
            // Per-arm decisions: each `stab_clause` is one arm of a
            // `case`/`cond`/`with`/anonymous-fn body or a `rescue`/
            // `catch` handler. Standard-only: modified counts a
            // `case`/`cond`/`try` container Call once, a `with` once
            // only when its `else` dispatches (the `"with"` arm below),
            // and gives a multi-clause `fn` or `reduce` nothing.
            //
            // The exception is the *first* `stab_clause` of an
            // `anonymous_function` (`fn … -> … end`): it is the
            // closure's head/definition, not a pattern-dispatch
            // decision. The closure already opens its own function
            // space seeded with base cyclomatic 1 (see
            // `getter::elixir` → `SpaceKind::Function`), so counting the
            // head clause too over-reports a trivial `fn x -> x end` as
            // 2 (issue #776). Only the 2nd+ clauses of a multi-clause
            // `fn` are real branches. A `for … reduce:`'s accumulator
            // clauses take the same skip (#1535): a single `acc -> …`
            // always matches. `case`/`cond`/`with` arms keep counting.
            //
            // Two further clause shapes are the construct's default arm
            // and are excluded to match the sibling family — Rust's
            // `_ =>`, Python's `case _:`, Ruby's `in _`, Kotlin's
            // `else ->`, C#'s `_ =>`, Bash's `*)`, C-family `default:`
            // (issue #1272, lesson 11): a bare `_ ->` catch-all under
            // `case` / `receive` / `rescue` (a `rescue` arm's `_ ->`
            // being free is a deliberate divergence from C-family
            // `catch (...)` — the bare-`_` rescue form is vanishingly
            // rare and the `try` container still pays modified), and
            // any unguarded `true ->` directly under a `cond` container
            // — shape-based, whatever the arm's position, matching
            // Rust's position-blind bare-`_` rule; `true ->` under
            // `case` is an ordinary pattern and keeps counting. A
            // multi-clause `fn`'s trailing `_ ->` is NOT excluded: the
            // head-clause skip above already grants the closure's free
            // base path, so each 2nd+ clause — the bare catch-all
            // included — is a real dispatch decision, keeping
            // `fn 0 -> :a; _ -> :b end` in parity with the identical
            // `case`. Guarded forms (`_ when g ->`, `true when g ->`)
            // and named discards (`_x ->`) are real decisions and
            // still count.
            E::StabClause
                if crate::metrics::npa::elixir_clause_is_decision(node, code, ancestors) =>
            {
                stats.cyclomatic += 1.;
            }
            // A guard is a decision the construct it guards does not
            // already pay for (#1454, transferring #1422's C# rule): a
            // guarded clause fails two ways — the pattern does not
            // match, or it matches and the guard is false — and a
            // guarded function head is one alternative among the
            // clauses. Both standard and modified, because no container
            // collapses it: a `case`'s arms collapse into the container
            // for modified, the guard on an arm does not, exactly as
            // C#'s `when_clause` counts in both tiers.
            //
            // Elixir's ABC counted the `when` token from the start; it
            // was cyclomatic that had no arm, so a guard read as a
            // condition with no decision behind it. The #1422 order
            // applies — fix cyclomatic, then re-derive ABC — and the
            // re-derivation is that ABC's existing count is already the
            // slot model (one per guard, sub-structure kept) and needs
            // only this gate, which it now shares.
            //
            // No double count (§5): the token fires once per `when`,
            // and the `binary_operator` that wraps it is not matched by
            // any arm here.
            E::When if crate::metrics::npa::elixir_when_is_guard(node, code, ancestors) => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // A `with`'s `pattern <- expr` clause either matches or
            // short-circuits to the `else`, so each is a decision
            // (#1535) — the Rust `if let` / `?` twin pays one in both
            // tiers. The clauses run in sequence rather than being
            // alternatives of one dispatch, so modified does not
            // collapse them. A `for` generator's `<-` is excluded by
            // the gate: the `for` already pays its loop decision.
            E::LTDASH
                if crate::metrics::npa::elixir_arrow_is_with_clause(node, code, ancestors) =>
            {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // Short-circuit booleans add a decision point in both
            // metrics — where applied. `&and/2`, `&||/2` and
            // `Kernel.||(a, b)` only name the operator and decide
            // nothing (#1534); ABC and cognitive gate on the same parent.
            E::AMPAMP | E::PIPEPIPE | E::And | E::Or
                if crate::lang_helpers::elixir::elixir_applying_operator(node, ancestors)
                    .is_some() =>
            {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            E::Call => {
                if let Some(target) = node.child_by_field_name("target")
                    && target.kind_id() == E::Identifier
                    && let Some(name) = target.utf8_text(code)
                {
                    match name {
                        // Single-branch constructs: count for both.
                        // There are no per-arm `stab_clause`s exposing
                        // themselves separately, so the Call itself
                        // must carry the decision point.
                        "if" | "unless" | "for" | "while" => {
                            stats.cyclomatic += 1.;
                            stats.cyclomatic_modified += 1.;
                        }
                        // Multi-arm containers: count once for modified
                        // (the container collapses to a single decision).
                        // Per-arm `stab_clause`s already contribute to
                        // standard above.
                        "case" | "cond" | "try" => {
                            stats.cyclomatic_modified += 1.;
                        }
                        // A `with`'s decisions are its `<-` clauses,
                        // counted above in both tiers. What modified
                        // collapses is its `else`, the dispatch over
                        // the failed value — once, and only when that
                        // dispatch picks between anything (#1535).
                        "with" if crate::metrics::npa::elixir_with_else_dispatches(node, code) => {
                            stats.cyclomatic_modified += 1.;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
