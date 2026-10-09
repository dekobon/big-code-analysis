//! Elixir clause and guard predicates shared by the `Cyclomatic` and
//! `Abc` impls for `ElixirCode`.
//!
//! Elixir spells every control-flow construct as a `Call`, so which
//! `stab_clause`, `when` and `<-` nodes are decisions is a question of
//! position and source text rather than node kind. Both metrics must
//! answer it identically (grammar-dispatch §7), so the answer lives in
//! one module.

use crate::lang_helpers::elixir::{elixir_call_keyword, elixir_is_method_macro};
use crate::{Ancestors, Elixir, Node};

/// Whether `node` is a `binary_operator` applying `operator`.
///
/// `binary_operator` carries three kind aliases at this pin so it is
/// matched by rule name rather than by enumerating ids
/// (grammar-dispatch §1); `when` and `<-` each have exactly one id.
fn elixir_is_binary_operator(node: &Node, operator: Elixir) -> bool {
    const BINARY_OPERATOR: &str = "binary_operator";

    node.kind() == BINARY_OPERATOR
        && node
            .child_by_field_name("operator")
            .is_some_and(|token| token.kind_id() == operator as u16)
}

/// Whether `node` is a `when` `binary_operator` — the shape a repeated
/// guard's alternatives nest through.
fn elixir_is_when_operator(node: &Node) -> bool {
    elixir_is_binary_operator(node, Elixir::When)
}

/// Whether the two ancestors `up` yields first are a `with` Call's
/// `arguments` and the Call itself — the position of a `with` clause.
///
/// `arguments` carries five kind aliases at this pin, so it is matched
/// by rule name (grammar-dispatch §1).
fn elixir_is_with_argument<'a>(mut up: impl Iterator<Item = Node<'a>>, code: &'a [u8]) -> bool {
    up.next()
        .is_some_and(|arguments| arguments.kind() == "arguments")
        && up
            .next()
            .is_some_and(|call| elixir_call_keyword(&call, code) == Some("with"))
}

/// Whether `node`, a `<-` token, is the operator of one of a `with`'s
/// `pattern <- expr` clauses: a decision, since the value either
/// matches the pattern or short-circuits to the `else` clauses (#1535).
/// It is the shape a Rust `if let` or `?` has, both of which pay one
/// decision in standard and modified cyclomatic alike.
///
/// Only a direct argument of a `with` qualifies. The same token is a
/// `for` comprehension's generator (`for x <- xs`), which filters
/// rather than short-circuits and is deliberately left alone — `for`
/// already pays one decision for its loop — and a bitstring
/// generator (`for <<c <- s>>`) sits inside a `bitstring`, not under
/// `arguments`.
///
/// Position alone is the gate, so it needs no separate test that the
/// token is applied (#1534): in every shape probed where it is not —
/// named (`with &<-/2`) or stranded by error recovery (`with(<-)`,
/// `with a, <- b`) — it sits under an `operator_identifier`, whose
/// parent is never the `with`'s `arguments`.
///
/// A clause wrapped in parentheses after a space (`with (a <- b) do`)
/// parses through a `block` and is not recognised; Elixir drops the
/// parentheses, so it is a clause, but the spelling is rare.
///
/// Shared by the `Cyclomatic` and `Abc` impls for `ElixirCode`
/// (grammar-dispatch §7).
pub(super) fn elixir_arrow_is_with_clause<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    elixir_is_with_argument(ancestors.iter(node).skip(1).map(|(a, _)| a), code)
}

/// The one alternative a guard's `when` token introduces.
///
/// `head when a when b` parses right-associatively as
/// `head when (a when b)`, so each `when` operator owns exactly one
/// alternative: the nested operator's `left` where its `right` nests a
/// further `when`, and its own `right` otherwise. One alternative per
/// token is what holds the `Abc` slot count level with the `Cyclomatic`
/// decision count at every chain length (§8), and what stops a chain of
/// n alternatives being counted once per token that can see it (§5).
///
/// The `?` is infallible at the pinned grammar — `binary_operator`
/// declares `right` required — and is spelled as an `Option` because
/// `AGENTS.md` bans `expect` outside tests.
pub(super) fn elixir_when_alternative<'a>(when_operator: &Node<'a>) -> Option<Node<'a>> {
    let right = when_operator.child_by_field_name("right")?;
    if elixir_is_when_operator(&right) {
        right.child_by_field_name("left")
    } else {
        Some(right)
    }
}

/// Whether a `when` operator token spells a real guard — a function
/// head's (`def f(x) when g do`) or a clause's (`x when g -> …`) —
/// rather than a typespec's `when` binding clause
/// (`@spec f(a) :: a when a: integer`), which is type syntax and no
/// decision at all.
///
/// Shared by the `Cyclomatic` and `Abc` impls for `ElixirCode` so the
/// two cannot disagree about what a guard is (grammar-dispatch §7).
/// Elixir has no dedicated guard production — `x when g` is an ordinary
/// `binary_operator` — so the position it sits in is the only thing that
/// tells a guard from a typespec, and the allowlist below is that
/// position set at the pinned grammar: the `left` slot of a
/// `stab_clause` (`case` / `cond` / `fn` / `receive` / `with`'s `else`
/// / `try`'s handlers), the pattern of a `with` clause
/// (`{:ok, a} when is_integer(a) <- g(x)`, #1535), or an argument of a
/// definition Call that takes a guarded head.
///
/// `arguments` carries five kind aliases at this pin and
/// `binary_operator` three, so both are matched by rule name rather
/// than by enumerating ids (grammar-dispatch §1).
///
/// Repeated guards (`when a when b`) are an or-chain: Elixir tries each
/// `when` expression in turn and moves to the next when the previous one
/// is false *or raises*, so the construct carries one decision per
/// alternative, level with the `when a or b` spelling. They parse
/// right-associatively into nested `when` operators, and only the
/// outermost sits on the anchor — so a nested one is a guard too, and
/// the climb below walks the `when` operators between it and the anchor
/// before asking the position question. [`elixir_when_alternative`]
/// names the one alternative each token introduces, which is how the
/// matching `Abc` slot stays one-per-token rather than double counting
/// the chain (grammar-dispatch §5).
///
/// The climb stops at the first non-`when` ancestor, so an ordinary
/// single guard pays no extra step and a chain pays one per alternative
/// it lists — a bound set by the guard, not by the tree's depth.
///
/// Both `None`s the climb answers `false` to are unreachable at the
/// pin: a `when` *token*'s chain always holds at least the
/// `binary_operator` it belongs to, and that operator always sits under
/// something, because the Elixir root is `source` and no `when`
/// operator can be it. They are `is_some_and`s rather than `expect`s
/// because `AGENTS.md` bans the `expect` outside tests.
pub(super) fn elixir_when_is_guard<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let mut chain = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    // The token's parent is the `when` operator node itself; the first
    // ancestor above the chain of `when` operators is the position that
    // decides.
    chain.next().is_some_and(|mut operator| {
        let mut above = chain.next();
        while let Some(enclosing) = above.filter(elixir_is_when_operator) {
            operator = enclosing;
            above = chain.next();
        }
        above.is_some_and(|parent| elixir_is_guard_position(&operator, &parent, chain, code))
    })
}

/// Whether `parent` — the first ancestor above the chain of `when`
/// operators that `operator` tops — holds that chain in a guard
/// position; `up` yields the ancestors above `parent`. The second step
/// of [`elixir_when_is_guard`], after the climb.
fn elixir_is_guard_position<'a>(
    operator: &Node<'a>,
    parent: &Node<'a>,
    mut up: impl Iterator<Item = Node<'a>>,
    code: &'a [u8],
) -> bool {
    use Elixir as E;

    const ARGUMENTS: &str = "arguments";

    if parent.kind_id() == E::StabClause as u16 {
        return parent
            .child_by_field_name("left")
            .is_some_and(|left| left.id() == operator.id());
    }
    // A guard on the pattern of a `with` clause. `when` binds tighter
    // than `<-`, so it parses as the clause's `left`; a `when` on the
    // right would be a guard outside any guard position, which Elixir
    // rejects, so the side is not checked. The guard of a `for`
    // generator (`for x when x > 1 <- xs`) is a filter, which the
    // comprehension's own decision already covers exactly as a bare
    // filter (`for x <- xs, x > 1`) is — so it is not a guard here.
    if elixir_is_binary_operator(parent, E::LTDASH) {
        return elixir_is_with_argument(up, code);
    }
    parent.kind() == ARGUMENTS
        && up.next().is_some_and(|call| {
            elixir_call_keyword(&call, code).is_some_and(|keyword| {
                elixir_is_method_macro(keyword) || matches!(keyword, "defguard" | "defguardp")
            })
        })
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
/// string comparison over enumerating them. A zero-arity clause carries
/// no `left` field at all (`fn -> … end`), which the same `filter` folds
/// into "no plain pattern list" — the two spellings of "nothing to
/// inspect" are one question, so they share one exit. Named-children
/// filtering skips the anonymous `(` `)` tokens a parenthesised clause
/// head carries, so an empty pair (`fn () -> … end`) also yields zero
/// patterns. Clauses with zero or several patterns return `None`.
pub(super) fn elixir_sole_unguarded_pattern<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let left = node
        .child_by_field_name("left")
        .filter(|left| left.kind() == "arguments")?;
    let mut patterns = left.children().filter(Node::is_named);
    let sole = patterns.next()?;
    patterns.next().is_none().then_some(sole)
}

/// Whether `pattern` is a bare `_`. Elixir has no wildcard token — `_`
/// parses as an ordinary `identifier` — so the bytes decide.
fn elixir_is_bare_wildcard(pattern: &Node, code: &[u8]) -> bool {
    pattern.kind_id() == Elixir::Identifier as u16 && pattern.utf8_text(code) == Some("_")
}

/// Returns `true` when `node` is a construct's free default clause:
/// a bare `_ ->` catch-all outside a head-skipping construct (an
/// anonymous fn or a `for … reduce:`), or an unguarded `true ->`
/// directly under a `cond`.
///
/// Both shapes hinge on the clause's sole unguarded pattern, and
/// [`elixir_sole_unguarded_pattern`]'s child scan allocates a cursor
/// (the #1112 malloc) — so the pattern is extracted once here and the
/// two shapes branch on its (kind, text), instead of every counted
/// `stab_clause` paying the extraction twice through two independent
/// predicates.
///
/// Bare `_ ->`: Elixir has no dedicated wildcard token — `_` parses
/// as an ordinary `identifier` — so the bytes decide (grammar-dispatch
/// §10). A named discard (`_x ->`) binds a value the body can read
/// and keeps counting, matching Rust's bare-`_`-only `MatchArm` rule;
/// guarded wildcards never reach the text check because
/// [`elixir_sole_unguarded_pattern`] rejects them. The exclusion does
/// NOT apply when `skips_head` — the clause's container is an
/// `anonymous_function` or a `for … reduce:`: such a multi-clause
/// dispatch is like `case` — n clauses are n−1 decisions — and its free
/// base path is already granted by the head-clause skip (#776, #1535),
/// so excluding a trailing `_ ->` too would leave
/// `fn 0 -> :a; _ -> :b end` at zero decisions while the identical
/// `case` reports one.
///
/// `true ->` under `cond`: the exclusion is *shape-based* — any
/// unguarded `true ->` whose parent is the `do_block` of a `Call`
/// spelling `cond`, whatever the arm's position. That deliberately
/// matches the sibling convention: Rust's bare-`_` `MatchArm`
/// exclusion is equally position-blind. The same clause under `case`
/// is an ordinary boolean pattern match, so the exclusion is anchored
/// to the owning construct (grammar-dispatch §8); a guarded
/// `true when g ->` is a real decision and never reaches the
/// container check.
fn elixir_is_default_clause<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    skips_head: bool,
) -> bool {
    use Elixir as E;

    let Some(pattern) = elixir_sole_unguarded_pattern(node) else {
        return false;
    };
    match pattern.kind_id().into() {
        // Bare `_` catch-all — free everywhere except where the
        // head-clause skip already provides the free base path.
        E::Identifier => !skips_head && elixir_is_bare_wildcard(&pattern, code),
        // Unguarded `true` — free only as `cond`'s designated
        // default; the O(1) parent/grandparent checks run after the
        // text compare so non-`true` booleans bail early.
        E::Boolean => {
            pattern.utf8_text(code) == Some("true")
                && elixir_do_section_call(node, code, ancestors)
                    .is_some_and(|call| elixir_call_keyword(&call, code) == Some("cond"))
        }
        _ => false,
    }
}

/// Whether `node`'s clauses-owner grants a free head clause: an
/// `anonymous_function` (#776), or a `for … reduce:` comprehension
/// (#1535), whose accumulator clauses are an anonymous fn in all but
/// spelling — a single `acc -> …` always matches, and only the 2nd+
/// clauses dispatch.
///
/// A `for` holds clauses in its `do` section only under `reduce:`;
/// without it the section is a plain body. So the Call's keyword alone
/// identifies the shape, with no need to find the `reduce:` pair.
fn elixir_clauses_skip_head<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Elixir::AnonymousFunction as u16)
        || elixir_do_section_call(node, code, ancestors)
            .is_some_and(|call| elixir_call_keyword(&call, code) == Some("for"))
}

/// Returns `true` when `node` is its parent's first `stab_clause` —
/// the head clause of a construct [`elixir_clauses_skip_head`] names.
///
/// The parent is the `anonymous_function` (`fn stab_clause+ end`), the
/// `do_block` (`do stab_clause+ end`) or the keyword form's
/// parenthesised `block`; in each the first child whose kind is
/// `stab_clause` (skipping the `fn` / `do` / `(` token) is the head.
fn elixir_is_first_clause<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .and_then(|parent| {
            parent
                .children()
                .find(|child| child.kind_id() == Elixir::StabClause as u16)
        })
        .is_some_and(|first| first.id() == node.id())
}

/// Whether `node` (a `stab_clause`) is one decision of the construct
/// that owns it: every clause of a `case` / `cond` / `with`'s `else` /
/// `receive` / `try` handler and every clause after a multi-clause
/// `fn`'s first, except the construct's free default clause
/// ([`elixir_is_default_clause`]).
///
/// An anonymous fn's head clause is the closure's definition, not a
/// dispatch: the closure opens its own function space whose base path
/// already covers it, so counting it too would score a trivial
/// `fn x -> x end` as a decision (#776). A `for … reduce:`'s first
/// accumulator clause is skipped for the same reason: the `for` already
/// pays its one decision, and a single `acc -> …` clause always matches
/// (#1535).
///
/// The parent is allowlisted because the grammar puts a `stab_clause`
/// in one more position: a parenthesised `block`, which is how a
/// typespec spells a function type (`@spec f((any -> any)) :: list`).
/// That is type syntax and branches on nothing, the same reason
/// [`elixir_when_is_guard`] excludes a typespec's `when` (#1531). The
/// keyword form of a clause construct (`case(x, do: (1 -> :a; …))`)
/// holds its clauses in the same parenthesised `block`, so a `block`
/// counts when it is the value of a section pair
/// ([`elixir_is_section_pair`]).
///
/// Shared by the `Cyclomatic` and `Abc` impls for `ElixirCode`, which
/// both score a clause construct per clause and must agree on which
/// clauses those are (grammar-dispatch §7, #1531).
pub(super) fn elixir_clause_is_decision<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Elixir as E;

    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    let in_section = up.next().is_some_and(|parent| {
        matches!(
            parent.kind_id().into(),
            E::DoBlock
                | E::ElseBlock
                | E::AfterBlock
                | E::RescueBlock
                | E::CatchBlock
                | E::AnonymousFunction
        ) || (parent.kind_id() == E::Block as u16
            && up
                .next()
                .is_some_and(|pair| elixir_is_section_pair(&pair, code, SECTION_KEYS)))
    });
    if !in_section {
        return false;
    }
    let skips_head = elixir_clauses_skip_head(node, code, ancestors);
    if skips_head && elixir_is_first_clause(node, ancestors) {
        return false;
    }
    !elixir_is_default_clause(node, code, ancestors, skips_head)
}

/// Whether a `with` Call's `else` clauses dispatch: whether at least one
/// of them is a decision rather than the free bare `_ ->` default.
///
/// Modified cyclomatic collapses a clause construct's arms into one
/// decision, which for `case` / `cond` / `try` is the container paying
/// once. A `with`'s decisions are its `<-` clauses, each paid on its own
/// in both tiers as a Rust `if let` is; its `else` is the dispatch over
/// the failed value, which collapses into one decision exactly when it
/// holds one — so `else _ -> 0`, which picks nothing, stays free in
/// modified as it is in standard, like Rust's `if let … else` (#1535).
/// Spelled in either form: the block form's `else_block`, or the
/// keyword form's `else:` pair whose value is a parenthesised `block`.
///
/// In a `with`'s `else` the default-clause rule reduces to the bare
/// `_`: `true ->` is free only under a `cond`, and the head skip only
/// under a `fn` or `reduce:`, so [`elixir_clause_is_decision`] agrees
/// clause by clause with the test below.
pub(super) fn elixir_with_else_dispatches(call: &Node, code: &[u8]) -> bool {
    use Elixir as E;

    call.children().any(|child| {
        let section = match child.kind_id().into() {
            E::DoBlock => child
                .children()
                .find(|c| c.kind_id() == E::ElseBlock as u16),
            _ if child.kind() == "arguments" => child
                .children()
                .filter(|c| c.kind_id() == E::Keywords as u16)
                .flat_map(|keywords| keywords.children())
                .find(|pair| elixir_is_section_pair(pair, code, &["else:"]))
                .and_then(|pair| pair.child_by_field_name("value")),
            _ => None,
        };
        section.is_some_and(|section| {
            section.children().any(|clause| {
                clause.kind_id() == E::StabClause as u16
                    && !elixir_sole_unguarded_pattern(&clause)
                        .is_some_and(|pattern| elixir_is_bare_wildcard(&pattern, code))
            })
        })
    })
}

/// The keyword spellings of the clause sections a `do … end` block
/// spells as `do_block` / `else_block` / `after_block` / `rescue_block`
/// / `catch_block`.
const SECTION_KEYS: &[&str] = &["do:", "else:", "after:", "rescue:", "catch:"];

/// Whether `pair` is a keyword-form section (`do: (…)`) whose key is one
/// of `keys`. The key token's text carries its colon and the whitespace
/// after it, so it is trimmed before comparing.
fn elixir_is_section_pair(pair: &Node, code: &[u8], keys: &[&str]) -> bool {
    pair.kind_id() == Elixir::Pair as u16
        && pair
            .child_by_field_name("key")
            .and_then(|key| key.utf8_text(code))
            .is_some_and(|key| keys.contains(&key.trim_end()))
}

/// The `Call` whose `do` section holds `node` (a `stab_clause`): the
/// block form's `do_block` (whose parent is the call) or the keyword
/// form's `do: (…)` block (`block` → `pair` → `keywords` → `arguments`
/// → call). The caller asks the Call's keyword (`cond`, `for`).
fn elixir_do_section_call<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    use Elixir as E;

    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    match up.next().map(|parent| parent.kind_id().into()) {
        Some(E::DoBlock) => up.next(),
        Some(E::Block) => up
            .next()
            .filter(|pair| elixir_is_section_pair(pair, code, &["do:"]))
            .and_then(|_| up.nth(2)),
        _ => None,
    }
}
