//! `Cyclomatic` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// Returns `true` when `node` is the first `stab_clause` child of an
/// `anonymous_function` parent — i.e. the closure's head clause rather
/// than a pattern-dispatch branch.
///
/// The grammar shape is `anonymous_function → fn stab_clause+ end`, so
/// the parent's children include the `fn`/`end` keyword tokens and one
/// or more `stab_clause`s. We locate the first child whose kind is
/// `stab_clause` (skipping the `fn` token and any other non-clause
/// sibling) and report whether it is `node`. Multi-clause `fn`s thus
/// skip only their first clause; `case`/`cond`/`with` arms have a
/// `do_block` parent and never match here (issue #776).
fn elixir_is_anonymous_fn_head_clause<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    use Elixir as E;

    let Some(parent) = ancestors.parent(node) else {
        return false;
    };
    if parent.kind_id() != E::AnonymousFunction as u16 {
        return false;
    }
    parent
        .children()
        .find(|child| child.kind_id() == E::StabClause as u16)
        .is_some_and(|first| first.id() == node.id())
}

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
    // `with`/`try`) contribute modified. Single-branch keyword Calls
    // (`if`/`unless`/`for`/`while`) contribute to both. Short-circuit
    // booleans (`&&`, `||`, `and`, `or`) contribute to both.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Elixir as E;

        match node.kind_id().into() {
            // Per-arm decisions: each `stab_clause` is one arm of a
            // `case`/`cond`/`with`/anonymous-fn body or a `rescue`/
            // `catch` handler. Standard-only — modified counts the
            // container Call once.
            //
            // The exception is the *first* `stab_clause` of an
            // `anonymous_function` (`fn … -> … end`): it is the
            // closure's head/definition, not a pattern-dispatch
            // decision. The closure already opens its own function
            // space seeded with base cyclomatic 1 (see
            // `getter::elixir` → `SpaceKind::Function`), so counting the
            // head clause too over-reports a trivial `fn x -> x end` as
            // 2 (issue #776). Only the 2nd+ clauses of a multi-clause
            // `fn` are real branches. `case`/`cond`/`with` arms have a
            // `do_block` parent — not `anonymous_function` — so they are
            // unaffected and keep counting.
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
                if elixir_is_anonymous_fn_head_clause(node, ancestors)
                    || crate::metrics::npa::elixir_is_default_clause(node, code, ancestors) => {}
            E::StabClause => {
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
            // Short-circuit booleans add a decision point in both
            // metrics.
            E::AMPAMP | E::PIPEPIPE | E::And | E::Or => {
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
                        "case" | "cond" | "with" | "try" => {
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
