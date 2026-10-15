//! Elixir clause and guard predicates shared by the `Cyclomatic` and
//! `Abc` impls for `ElixirCode`.
//!
//! Elixir spells every control-flow construct as a `Call`, so which
//! `stab_clause`, `when` and `<-` nodes are decisions is a question of
//! position and source text rather than node kind. Both metrics must
//! answer it identically (grammar-dispatch §7), so the answer lives in
//! one module.

use crate::lang_helpers::elixir::{elixir_call_keyword, elixir_is_method_macro};
use crate::metrics::abc::is_operand;
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
/// A `receive` gets exactly one free arm, in every spelling. Its
/// `after` clause is the timeout, the path taken when no message clause
/// matched in time, which makes it `receive`'s default arm — the role a
/// bare `_ ->` plays in a `case` (#1581). But a bare `_ ->` in the `do`
/// section is free under `receive` as well, and with one present the
/// timeout is a path of its own: `receive do {:a, y} -> y; _ -> 0 after
/// 10 -> :t end` reaches a matched message, any other message, and an
/// empty mailbox timing out — three paths, the three-clause `case`'s
/// two decisions. So the `after` clause is free only when the `do`
/// section has no bare catch-all ([`elixir_receive_after_is_decision`]).
/// It is never a dispatch in its own right: the compiler accepts
/// exactly one `after` clause, and its head is a timeout value, not a
/// pattern that can fail. No other construct puts a clause there, since
/// `try`'s `after` takes a plain body, so the section is not among
/// those [`DECISION_SECTION_KEYS`] names.
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
/// counts when it is the value of a pair whose key names a clause
/// section ([`DECISION_SECTION_KEYS`]).
///
/// That pair is not asked which Call owns it, so a clause block passed
/// under a section key to *any* call counts: `f(x, rescue: (e -> e))`
/// scores one decision (#1583). Only a macro can take such an argument
/// — the compiler rejects it for a function ("cannot handle clauses with
/// the -> operator because it is not a macro", Elixir 1.17) — and a
/// user macro's clauses mean whatever it makes of them, so gating on the
/// owner would change no valid program's score in a way either answer
/// could defend. A dispatching DSL is the likelier reading, and is the
/// one kept. Cognitive gates its keyword sections on the owner for a
/// different reason: it scores the pair whatever its value holds, and
/// `else:` is an ordinary option to any function (`f(x, else: 1)`).
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

    if let Some(receive) = elixir_after_section_receive(node, code, ancestors) {
        return elixir_receive_after_is_decision(&receive, code);
    }
    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    let in_section = up.next().is_some_and(|parent| {
        matches!(
            parent.kind_id().into(),
            E::DoBlock | E::ElseBlock | E::RescueBlock | E::CatchBlock | E::AnonymousFunction
        ) || (parent.kind_id() == E::Block as u16
            && up.next().is_some_and(|pair| {
                elixir_pair_key(&pair, code).is_some_and(|key| DECISION_SECTION_KEYS.contains(&key))
            }))
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
/// Spelled in any form: the block form's `else_block`, or the keyword
/// form's `else:` pair whose value is a parenthesised `block`, bare or
/// bracketed ([`elixir_trailing_keywords`]).
///
/// In a `with`'s `else` the default-clause rule reduces to the bare
/// `_`: `true ->` is free only under a `cond`, and the head skip only
/// under a `fn` or `reduce:`, so [`elixir_clause_is_decision`] agrees
/// clause by clause with the test below.
pub(super) fn elixir_with_else_dispatches(call: &Node, code: &[u8]) -> bool {
    elixir_section_has_clause(call, code, Section::Else, |clause| {
        !elixir_is_catch_all(clause, code)
    })
}

/// Whether `clause` (a `stab_clause`) is a bare, unguarded `_ ->`.
fn elixir_is_catch_all(clause: &Node, code: &[u8]) -> bool {
    elixir_sole_unguarded_pattern(clause)
        .is_some_and(|pattern| elixir_is_bare_wildcard(&pattern, code))
}

/// Whether a `receive`'s `after` clause is a decision: whether its `do`
/// section already holds the free bare `_ ->`, leaving the timeout as a
/// path of its own. Without one the timeout is the construct's single
/// free arm ([`elixir_clause_is_decision`]).
fn elixir_receive_after_is_decision(receive: &Node, code: &[u8]) -> bool {
    elixir_section_has_clause(receive, code, Section::Do, |clause| {
        elixir_is_catch_all(clause, code)
    })
}

/// The `receive` Call whose `after` section holds `node` (a
/// `stab_clause`): the block form's `after_block` inside the Call's
/// `do_block`, or the keyword form's `after: (…)` block, bare or
/// bracketed. `None` for a clause anywhere else, and for an `after`
/// section of any other Call.
fn elixir_after_section_receive<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    use Elixir as E;

    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    let call = match up.next().map(|parent| parent.kind_id().into()) {
        Some(E::AfterBlock) => up
            .next()
            .filter(|do_block| do_block.kind_id() == E::DoBlock as u16)
            .and_then(|_| up.next()),
        Some(E::Block) => {
            let pair = up.next()?;
            elixir_pair_section(&pair, code, up)
                .filter(|(key, _)| *key == "after")
                .map(|(_, call)| call)
        }
        _ => None,
    }?;
    (elixir_call_keyword(&call, code) == Some("receive")).then_some(call)
}

/// Whether a `receive` Call dispatches over the mailbox: whether its
/// `do` section holds a message clause, in any spelling. One holding
/// only an `after` timeout (`receive do after 100 -> :ok end`, the
/// idiomatic sleep) is a timed wait that picks between nothing, so it
/// pays neither modified cyclomatic's container decision nor cognitive's
/// +nesting — as a `with` whose `else` picks nothing pays no modified
/// decision ([`elixir_with_else_dispatches`]). Any clause counts, a bare
/// `_ ->` included, because `case x do _ -> 0 end` pays its container
/// too (#1581).
pub(super) fn elixir_receive_dispatches(call: &Node, code: &[u8]) -> bool {
    elixir_section_has_clause(call, code, Section::Do, |_| true)
}

/// A clause section [`elixir_section_has_clause`] reads.
#[derive(Clone, Copy)]
enum Section {
    Do,
    Else,
}

/// Whether `call`'s `section` holds a `stab_clause` that `accept`s: the
/// block form's `do_block`, or the `else_block` inside it, or the
/// keyword form's `do:` / `else:` value, bare or bracketed
/// ([`elixir_trailing_keywords`]). The block form's `else_block` is the
/// `do_block`'s child, never the Call's.
fn elixir_section_has_clause(
    call: &Node,
    code: &[u8],
    section: Section,
    accept: impl Fn(&Node) -> bool,
) -> bool {
    use Elixir as E;

    let key = match section {
        Section::Do => "do",
        Section::Else => "else",
    };
    call.children().any(|child| {
        let holder = match (child.kind_id().into(), section) {
            (E::DoBlock, Section::Do) => Some(child),
            (E::DoBlock, Section::Else) => child
                .children()
                .find(|c| c.kind_id() == E::ElseBlock as u16),
            _ if child.kind() == ARGUMENTS => elixir_keyword_section(&child, code, key),
            _ => None,
        };
        holder.is_some_and(|holder| {
            holder
                .children()
                .any(|clause| clause.kind_id() == E::StabClause as u16 && accept(&clause))
        })
    })
}

/// The keyword spellings of the clause sections whose clauses are
/// decisions, which a `do … end` block spells as `do_block` /
/// `else_block` / `rescue_block` / `catch_block`. Not `after`: see
/// [`elixir_clause_is_decision`].
const DECISION_SECTION_KEYS: &[&str] = &["do", "else", "rescue", "catch"];

/// The key a keyword-list `pair` spells, without its colon: `do` for
/// both `do: x` and the quoted `"do": x`, which Elixir reads as the same
/// atom. The key token's text carries its colon and the whitespace after
/// it. `None` for any other node, and for a quoted key that interpolates
/// (`"do#{k}": x`), whose atom is not known until run time — valid in an
/// ordinary call's options, but never a section.
///
/// The grammar's key is a `keyword` token, which has no named parts, or a
/// `quoted_keyword` around its `quoted_content` and any interpolations.
fn elixir_pair_key<'a>(pair: &Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    let key = Some(pair)
        .filter(|pair| pair.kind_id() == Elixir::Pair as u16)
        .and_then(|pair| pair.child_by_field_name("key"))?;
    let mut parts = key.children().filter(Node::is_named);
    match (parts.next(), parts.next()) {
        (None, _) => key
            .utf8_text(code)
            .and_then(|text| text.trim_end().strip_suffix(':')),
        (Some(content), None) if content.kind() == "quoted_content" => content.utf8_text(code),
        _ => None,
    }
}

/// The keyword list `arguments` ends with: a bare `keywords` node
/// (`if c, do: x`), or the same list in brackets (`if(c, [do: x])`),
/// which Elixir reads as the identical trailing keyword list — the
/// keyword syntax is sugar for exactly that last-argument literal.
///
/// Only the *last* argument qualifies, and only a list holding nothing
/// but the `keywords` node: `[a: 1]` in any other position is an
/// ordinary list argument, and `[1, do: x]` is a list with a keyword
/// tail, not a keyword list. An empty `[]` holds no `keywords` at all.
/// The grammar ends a list with its `keywords`, so a list whose first
/// item is one holds nothing else.
///
/// Matched by rule name: `keywords` carries two kind ids at this pin
/// (grammar-dispatch §1). Comments are skipped as the extras they are.
pub(super) fn elixir_trailing_keywords<'a>(arguments: &Node<'a>) -> Option<Node<'a>> {
    let last = arguments.children().filter(is_operand).last()?;
    if last.kind() == KEYWORDS {
        return Some(last);
    }
    if last.kind_id() != Elixir::List as u16 {
        return None;
    }
    last.children()
        .find(is_operand)
        .filter(|keywords| keywords.kind() == KEYWORDS)
}

/// The value of the `key:` pair in `arguments`' trailing keyword list
/// ([`elixir_trailing_keywords`]), or `None` when it has no such pair.
pub(super) fn elixir_keyword_section<'a>(
    arguments: &Node<'a>,
    code: &'a [u8],
    key: &str,
) -> Option<Node<'a>> {
    elixir_trailing_keywords(arguments)?
        .children()
        .find(|pair| elixir_pair_key(pair, code) == Some(key))?
        .child_by_field_name("value")
}

/// The section key of `pair` and the `Call` whose trailing keyword list
/// holds it — `("else", if …)` for the `else:` of `if(c, do: 1, else: 2)`
/// or of `if(c, [do: 1, else: 2])` — or `None` when `pair` is not a
/// keyword argument of a Call. `up` yields the ancestors above `pair`.
///
/// The climb is `keywords`, an optional bracketing `list`, `arguments`,
/// then the Call; the keyword list must be the one
/// [`elixir_trailing_keywords`] finds, so a `[do: x]` in any other
/// argument position, a map's keyword pairs and a nested list are not
/// sections. That identity check is also what rejects a list nested in
/// a list (`f([[do: x]])`), whose inner list the climb steps over.
fn elixir_pair_section<'a>(
    pair: &Node<'a>,
    code: &'a [u8],
    mut up: impl Iterator<Item = Node<'a>>,
) -> Option<(&'a str, Node<'a>)> {
    let key = elixir_pair_key(pair, code)?;
    let keywords = up.next().filter(|keywords| keywords.kind() == KEYWORDS)?;
    let arguments = up.find(|node| node.kind_id() != Elixir::List as u16)?;
    up.next()
        .filter(|call| {
            call.kind_id() == Elixir::Call as u16
                && arguments.kind() == ARGUMENTS
                && elixir_trailing_keywords(&arguments).map(|found| found.id())
                    == Some(keywords.id())
        })
        .map(|call| (key, call))
}

/// [`elixir_pair_section`] for a `pair` the walker is visiting, its
/// ancestors read off the walker's chain.
pub(super) fn elixir_keyword_pair_section<'a>(
    pair: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<(&'a str, Node<'a>)> {
    elixir_pair_section(
        pair,
        code,
        ancestors.iter(pair).map(|(ancestor, _)| ancestor),
    )
}

/// The rule name of a keyword list (two kind ids at this pin).
const KEYWORDS: &str = "keywords";

/// The rule name of an argument list (five kind ids at this pin).
const ARGUMENTS: &str = "arguments";

/// The `Call` whose `do` section holds `node` (a `stab_clause`): the
/// block form's `do_block` (whose parent is the call) or the keyword
/// form's `do: (…)` block, bare or bracketed (`block` → `pair` → the
/// climb [`elixir_pair_section`] makes). The caller asks the Call's
/// keyword (`cond`, `for`).
fn elixir_do_section_call<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    use Elixir as E;

    let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
    match up.next().map(|parent| parent.kind_id().into()) {
        Some(E::DoBlock) => up.next(),
        Some(E::Block) => {
            let pair = up.next()?;
            elixir_pair_section(&pair, code, up)
                .filter(|(key, _)| *key == "do")
                .map(|(_, call)| call)
        }
        _ => None,
    }
}
