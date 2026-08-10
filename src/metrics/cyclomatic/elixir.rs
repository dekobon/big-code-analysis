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

/// Returns the sole pattern of a `stab_clause` whose left-hand side
/// carries no `when` guard, or `None` otherwise.
///
/// The grammar's `left` field is an `arguments` node for a plain
/// pattern list, but a guarded clause (`_ when g ->`) re-shapes it
/// into a `binary_operator` wrapping the patterns and the guard — so
/// checking the field's kind answers "is there a guard" for free. The
/// kind is compared as a string because the grammar aliases several
/// internal rules to `arguments` with distinct kind ids
/// (`Arguments2`..`Arguments5`); grammar-dispatch §1 prefers the one
/// string comparison over enumerating them. Named-children filtering
/// skips the anonymous `(` `)` tokens a parenthesised clause head
/// carries. Clauses with zero (`fn -> … end`) or several patterns
/// return `None`.
fn elixir_sole_unguarded_pattern<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let left = node.child_by_field_name("left")?;
    if left.kind() != "arguments" {
        return None;
    }
    let mut patterns = left.children().filter(Node::is_named);
    let sole = patterns.next()?;
    patterns.next().is_none().then_some(sole)
}

/// Returns `true` when `node` is a bare catch-all `stab_clause`: a
/// single `_` pattern with no `when` guard (`_ -> …`).
///
/// Elixir has no dedicated wildcard token — `_` parses as an ordinary
/// `identifier` — so the bytes decide (grammar-dispatch §10). A named
/// discard (`_x ->`) binds a value the body can read and keeps
/// counting, matching Rust's bare-`_`-only `MatchArm` rule; guarded
/// wildcards never reach the text check because
/// [`elixir_sole_unguarded_pattern`] rejects them.
fn elixir_is_bare_catchall_clause<'a>(node: &Node<'a>, code: &'a [u8]) -> bool {
    use Elixir as E;

    elixir_sole_unguarded_pattern(node).is_some_and(|pattern| {
        pattern.kind_id() == E::Identifier as u16 && pattern.utf8_text(code) == Some("_")
    })
}

/// Returns `true` when `node` is the idiomatic `true -> …` default
/// clause of a `cond` container.
///
/// `cond`'s final `true ->` arm is the construct's designated default
/// — the direct analogue of `if`/`elif`/`else`'s free `else` — but
/// the same clause under `case` is an ordinary boolean pattern match,
/// so the exclusion is anchored to the owning construct
/// (grammar-dispatch §8): the clause's parent must be the `do_block`
/// of a `Call` whose target spells `cond`. A guarded `true when g ->`
/// is a real decision and never reaches the container check.
fn elixir_is_cond_default_clause<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Elixir as E;

    let is_literal_true = elixir_sole_unguarded_pattern(node).is_some_and(|pattern| {
        pattern.kind_id() == E::Boolean as u16 && pattern.utf8_text(code) == Some("true")
    });
    if !is_literal_true {
        return false;
    }
    let mut chain = ancestors.iter(node);
    let Some((parent, _)) = chain.next() else {
        return false;
    };
    if parent.kind_id() != E::DoBlock as u16 {
        return false;
    }
    chain.next().is_some_and(|(grandparent, _)| {
        crate::metrics::cognitive::elixir_call_keyword(&grandparent, code) == Some("cond")
    })
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
            // (issue #1272, lesson 11): a bare `_ ->` catch-all
            // (any container — `case`, `receive`, and a multi-clause
            // `fn`'s dispatch all use it as their default; a `rescue`
            // arm's `_ ->` is likewise free, a deliberate divergence
            // from C-family `catch (...)` — the bare-`_` rescue form
            // is vanishingly rare and the `try` container still pays
            // modified), and `cond`'s
            // idiomatic `true ->` final arm (only under a `cond`
            // container; `true ->` under `case` is an ordinary pattern
            // and keeps counting). Guarded forms (`_ when g ->`,
            // `true when g ->`) and named discards (`_x ->`) are real
            // decisions and still count.
            E::StabClause
                if elixir_is_anonymous_fn_head_clause(node, ancestors)
                    || elixir_is_bare_catchall_clause(node, code)
                    || elixir_is_cond_default_clause(node, code, ancestors) => {}
            E::StabClause => {
                stats.cyclomatic += 1.;
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
