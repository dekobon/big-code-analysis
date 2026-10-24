//! Elixir clause and guard predicates shared by the `CaseArm` and
//! `Guard` rows (`roles/elixir.rs`) and the root crate's `Cyclomatic`
//! and `Abc` impls for `ElixirCode`.
//!
//! Elixir spells every control-flow construct as a `Call`, so which
//! `stab_clause`, `when` and `<-` nodes are decisions is a question of
//! position and source text rather than node kind. Every reader must
//! answer it identically (grammar-dispatch §7), so the answer lives in
//! one module — here, beside the rows that gate on it (#1695; it was
//! `src/metrics/elixir_decisions.rs` in the root until then).

use crate::lang_helpers::elixir::{elixir_call_keyword, elixir_is_method_macro};
use crate::node::MemoQuestion;
use crate::{Ancestors, Elixir, Node};

// The guard-position checks `elixir_when_is_guard` reaches, counted so
// a test can see that a walk lending a `FoldCache` reaches it once per
// repeated-guard chain whichever order it visits the tokens in (#1695):
// the answers are identical either way, and only the count tells the
// two apart.
crate::observation::counter!(guard_position_checks, cfg(test));

/// Whether `child` can occupy an operand slot: named, and not an
/// `extra`. The root's `metrics::abc::is_operand`, which the rows
/// cannot reach.
fn is_operand(child: &Node) -> bool {
    child.is_named() && !child.as_tree_sitter().is_extra()
}

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
#[must_use]
pub(crate) fn elixir_is_when_operator(node: &Node) -> bool {
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
#[must_use]
pub fn elixir_arrow_is_with_clause<'a>(
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
#[must_use]
pub fn elixir_when_alternative<'a>(when_operator: &Node<'a>) -> Option<Node<'a>> {
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
/// it lists — a bound set by the guard, not by the tree's depth. Asked
/// once per token, that is quadratic in a chain's length; a walk that
/// lends a memo records the answer for every `when` operator the climb
/// passes, and a nested operator's answer is its parent operator's, so
/// a chain is climbed once however its tokens are visited (#1695).
///
/// Both `None`s the climb answers `false` to are unreachable at the
/// pin: a `when` *token*'s chain always holds at least the
/// `binary_operator` it belongs to, and that operator always sits under
/// something, because the Elixir root is `source` and no `when`
/// operator can be it. They are `is_some_and`s rather than `expect`s
/// because `AGENTS.md` bans the `expect` outside tests.
#[must_use]
pub(crate) fn elixir_when_is_guard<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let mut chain = ancestors.iter(node);
    // The token's parent is the `when` operator node itself; the first
    // ancestor above the chain of `when` operators is the position that
    // decides.
    let Some((operator, _)) = chain.next() else {
        return false;
    };
    ancestors.memoised(MemoQuestion::GuardOperator, &operator, || {
        let memo = ancestors.node_memo(MemoQuestion::GuardOperator);
        let mut operator = operator;
        let mut above = chain.next();
        // The operators the climb passes, to record the answer on each:
        // the enclosing operator's answer is every nested one's.
        let mut passed = Vec::new();
        let answer = loop {
            let Some((enclosing, _)) = above.filter(|(up, _)| elixir_is_when_operator(up)) else {
                guard_position_checks::record();
                break above.is_some_and(|(parent, _)| {
                    elixir_is_guard_position(&operator, &parent, chain.map(|(up, _)| up), code)
                });
            };
            if let Some(known) = memo.and_then(|memo| memo.get(&enclosing)) {
                break known;
            }
            passed.push(enclosing);
            operator = enclosing;
            above = chain.next();
        };
        if let Some(memo) = memo {
            for operator in passed {
                memo.set(&operator, answer);
            }
        }
        answer
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
#[must_use]
pub fn elixir_sole_unguarded_pattern<'a>(node: &Node<'a>) -> Option<Node<'a>> {
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
/// (`elixir_is_default_clause`).
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
/// section has no bare catch-all ([`elixir_receive_timeout_clause`]).
/// It is never a dispatch in its own right: the compiler accepts
/// exactly one `after` clause, and its head is a timeout value, not a
/// pattern that can fail. No other construct puts a clause there, since
/// `try`'s `after` takes a plain body, so the section is not among
/// those `DECISION_SECTION_KEYS` names.
/// The owning Call scores the timeout once; scoring it per clause would
/// repeatedly scan the message section on compiler-invalid multi-timeout input.
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
/// section (`DECISION_SECTION_KEYS`).
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
#[must_use]
pub(crate) fn elixir_clause_is_decision<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Elixir as E;

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
/// bracketed (`elixir_trailing_keywords`).
///
/// In a `with`'s `else` the default-clause rule reduces to the bare
/// `_`: `true ->` is free only under a `cond`, and the head skip only
/// under a `fn` or `reduce:`, so `elixir_clause_is_decision` agrees
/// clause by clause with the test below.
#[must_use]
pub fn elixir_with_else_dispatches(call: &Node, code: &[u8]) -> bool {
    elixir_section_has_clause(call, code, Section::Else, |clause| {
        !elixir_is_catch_all(clause, code)
    })
}

/// Whether `clause` (a `stab_clause`) is a bare, unguarded `_ ->`.
fn elixir_is_catch_all(clause: &Node, code: &[u8]) -> bool {
    elixir_sole_unguarded_pattern(clause)
        .is_some_and(|pattern| elixir_is_bare_wildcard(&pattern, code))
}

/// A `receive`'s `after` clause when it is a decision: when its `do`
/// section already holds the free bare `_ ->`, leaving the timeout as a
/// path of its own. Without one the timeout is the construct's single
/// free arm (`elixir_clause_is_decision`).
/// Called once per receive, not once per timeout clause.
#[must_use]
pub fn elixir_receive_timeout_clause<'a>(receive: &Node<'a>, code: &'a [u8]) -> Option<Node<'a>> {
    let timeout = receive.children().find_map(|child| {
        elixir_section_holder(&child, code, Section::After)?
            .children()
            .find(|clause| clause.kind_id() == Elixir::StabClause as u16)
    })?;
    elixir_section_has_clause(receive, code, Section::Do, |clause| {
        elixir_is_catch_all(clause, code)
    })
    .then_some(timeout)
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
#[must_use]
pub fn elixir_receive_dispatches(call: &Node, code: &[u8]) -> bool {
    elixir_section_has_clause(call, code, Section::Do, |_| true)
}

/// A section of a Call, as `elixir_section_holder` finds it.
#[derive(Clone, Copy)]
pub enum Section {
    /// The `do` block.
    Do,
    /// The `else` section of a `with` / `try` / `receive`.
    Else,
    /// The `after` section of a `receive` / `try`.
    After,
}

impl Section {
    /// The section's keyword-form spelling, without its colon.
    const fn key(self) -> &'static str {
        match self {
            Self::Do => "do",
            Self::Else => "else",
            Self::After => "after",
        }
    }
}

/// Whether `call`'s `section` holds a `stab_clause` that `accept`s.
fn elixir_section_has_clause(
    call: &Node,
    code: &[u8],
    section: Section,
    accept: impl Fn(&Node) -> bool,
) -> bool {
    call.children().any(|child| {
        elixir_section_holder(&child, code, section).is_some_and(|holder| {
            holder
                .children()
                .any(|clause| clause.kind_id() == Elixir::StabClause as u16 && accept(&clause))
        })
    })
}

/// Whether `call` has a `section`, in either spelling
/// (`elixir_section_holder`).
#[must_use]
pub fn elixir_call_has_section(call: &Node, code: &[u8], section: Section) -> bool {
    call.children()
        .any(|child| elixir_section_holder(&child, code, section).is_some())
}

/// The node holding the `section` that `child`, a child of a Call,
/// spells: the block form's `do_block`, or the `else_block` inside it,
/// or the keyword form's `do:` / `else:` value in the trailing keyword
/// list of the `arguments` `child` is, bare or bracketed
/// (`elixir_trailing_keywords`). The block form's `else_block` is the
/// `do_block`'s child, never the Call's.
fn elixir_section_holder<'a>(
    child: &Node<'a>,
    code: &'a [u8],
    section: Section,
) -> Option<Node<'a>> {
    use Elixir as E;

    match (child.kind_id().into(), section) {
        (E::DoBlock, Section::Do) => Some(*child),
        (E::DoBlock, Section::Else) => child
            .children()
            .find(|c| c.kind_id() == E::ElseBlock as u16),
        (E::DoBlock, Section::After) => child
            .children()
            .find(|c| c.kind_id() == E::AfterBlock as u16),
        _ if child.kind() == ARGUMENTS => elixir_keyword_section(child, code, section.key()),
        _ => None,
    }
}

/// The keyword spellings of the clause sections whose clauses are
/// decisions, which a `do … end` block spells as `do_block` /
/// `else_block` / `rescue_block` / `catch_block`. Not `after`: see
/// `elixir_clause_is_decision`.
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
    if pair.kind_id() != Elixir::Pair as u16 {
        return None;
    }
    let key = pair.child_by_field_name("key")?;
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
fn elixir_trailing_keywords<'a>(arguments: &Node<'a>) -> Option<Node<'a>> {
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
/// (`elixir_trailing_keywords`), or `None` when it has no such pair.
fn elixir_keyword_section<'a>(arguments: &Node<'a>, code: &'a [u8], key: &str) -> Option<Node<'a>> {
    elixir_trailing_keywords(arguments)?
        .children()
        .find(|pair| elixir_pair_key(pair, code) == Some(key))?
        .child_by_field_name("value")
}

/// The section key of `pair` and the `Call` whose trailing keyword list
/// holds it — `("else", if …)` for the `else:` of `if(c, do: 1, else: 2)`
/// or of `if(c, [do: 1, else: 2])` — or `None` when `pair` is not a
/// keyword argument of a Call. `ancestors` is `pair`'s chain.
///
/// The climb is `keywords`, an optional bracketing `list`, `arguments`,
/// then the Call, and the keyword list must be the trailing one
/// `elixir_trailing_keywords` describes: no operand follows it among
/// the arguments, and a bracketing list holds nothing before it. So a
/// `[do: x]` in any other argument position, a map's keyword pairs and a
/// list nested in a list (`f([[do: x]])`, whose outer list is where the
/// `arguments` should be) are not sections.
///
/// Each step reads one sibling off the chain rather than the argument
/// list from its front, so the pairs of one call cost it a scan each
/// only once: every pair asks this in cognitive, and every keyword-form
/// clause in cyclomatic and ABC, and a call holding thousands of
/// positional arguments beside thousands of pairs or clauses was
/// quadratic in them (5.4 s for 16 000 arguments in a release build;
/// `cognitive/elixir-mixed-arguments` prices the shape).
#[must_use]
pub fn elixir_pair_section<'a>(
    pair: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<(&'a str, Node<'a>)> {
    let mut up = ancestors.iter(pair);
    let (keywords, keywords_ancestors) = up.next().filter(|(node, _)| node.kind() == KEYWORDS)?;
    // The argument the keyword list is: itself, or the list bracketing it.
    let (bracketed, (argument, argument_ancestors)) = match up.next()? {
        (list, list_ancestors) if list.kind_id() == Elixir::List as u16 => {
            (true, (list, list_ancestors))
        }
        _ => (false, (keywords, keywords_ancestors)),
    };
    let mut above = argument_ancestors.iter(&argument);
    let (arguments, _) = above.next()?;
    let (call, _) = above.next()?;
    // The grammar ends a list with its `keywords`, so a bracketing list
    // that holds nothing before them holds nothing else.
    let is_section = arguments.kind() == ARGUMENTS
        && call.kind_id() == Elixir::Call as u16
        && argument_ancestors
            .siblings_after(&argument)
            .all(|sibling| !is_operand(&sibling))
        && (!bracketed
            || keywords_ancestors
                .siblings_before(&keywords)
                .all(|sibling| !is_operand(&sibling)));
    is_section
        .then(|| elixir_pair_key(pair, code))
        .flatten()
        .map(|key| (key, call))
}

/// The rule name of a keyword list (two kind ids at this pin).
const KEYWORDS: &str = "keywords";

/// The rule name of an argument list (five kind ids at this pin).
const ARGUMENTS: &str = "arguments";

/// The `Call` whose `do` section holds `node` (a `stab_clause`), in
/// either spelling: the block form's `do_block`, or a trailing `do:`
/// keyword pair. The caller asks the Call's keyword (`cond`, `for`).
fn elixir_do_section_call<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    use Elixir as E;

    let mut up = ancestors.iter(node);
    let (holder, _) = up.next()?;
    let (next, next_ancestors) = up.next()?;
    match holder.kind_id().into() {
        E::DoBlock => Some(next),
        E::Block => elixir_pair_section(&next, code, next_ancestors)
            .filter(|(key, _)| *key == "do")
            .map(|(_, call)| call),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElixirCode;
    use crate::test_support::for_each_node_with_chain;

    // A repeated guard's `when` tokens each ask whether their operator
    // sits in a guard position; a walk lending a `FoldCache` reaches
    // the position check once per chain, in whichever order the tokens
    // are asked — innermost first here, the order that leaves nothing
    // memoised above the first asker, so only recording the answer on
    // every operator the climb passes keeps it to one. Dropping that
    // record makes the first assertion read the chain's length.
    #[cfg(feature = "elixir")]
    #[test]
    fn a_repeated_guard_chain_is_climbed_once_whichever_token_asks_first() {
        use crate::node::FoldCache;
        let depth = 32;
        let guards = (0..depth).fold(String::new(), |mut s, i| {
            use std::fmt::Write as _;
            let _ = write!(s, " when is_integer(x{i})");
            s
        });
        let source = format!("def f(x){guards}, do: x\n");
        let source = source.as_bytes();
        let tree = crate::node::Tree::new::<ElixirCode>(source);
        let root = tree.get_root();
        assert!(!root.has_error());
        // Every `when` token with its ancestor chain, root first.
        let mut tokens = Vec::new();
        let mut stack = vec![(root, Vec::new())];
        while let Some((node, chain)) = stack.pop() {
            if node.kind_id() == Elixir::When as u16 {
                tokens.push((node, chain.clone()));
            }
            let mut below = chain;
            below.push(node);
            stack.extend(node.children().map(|child| (child, below.clone())));
        }
        assert_eq!(tokens.len(), depth);
        // Innermost first.
        tokens.sort_by_key(|(token, _)| std::cmp::Reverse(token.start_byte()));

        let cache = FoldCache::default();
        let before = guard_position_checks::observed();
        let cached: Vec<bool> = tokens
            .iter()
            .map(|(token, chain)| {
                elixir_when_is_guard(token, source, Ancestors::known(chain).with_folds(&cache))
            })
            .collect();
        assert_eq!(
            guard_position_checks::observed() - before,
            1,
            "a walk lending a FoldCache checks the chain's position once"
        );

        let before = guard_position_checks::observed();
        let uncached: Vec<bool> = tokens
            .iter()
            .map(|(token, chain)| elixir_when_is_guard(token, source, Ancestors::known(chain)))
            .collect();
        assert_eq!(guard_position_checks::observed() - before, depth);
        assert_eq!(cached, uncached);
        assert!(
            cached.iter().all(|&guard| guard),
            "every `when` of the chain is a guard"
        );
    }

    #[cfg(feature = "elixir")]
    #[test]
    fn empty_call_arguments_have_no_trailing_keywords() {
        let source = b"f()\ng(else: 1)\n";
        let mut found = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, _| {
            if node.kind() == ARGUMENTS {
                found.push(elixir_trailing_keywords(node).map(|keywords| {
                    keywords
                        .utf8_text(source)
                        .expect("keywords are UTF-8")
                        .to_owned()
                }));
            }
        });
        assert_eq!(found, [None, Some("else: 1".to_owned())]);
    }

    #[cfg(feature = "elixir")]
    #[test]
    fn standalone_keyword_list_has_no_owning_call() {
        let source = b"[else: 1]\nf(else: 2)\n";
        let mut found = Vec::new();
        for_each_node_with_chain::<ElixirCode>(source, |node, chain| {
            if node.kind_id() == Elixir::Pair as u16 {
                found.push(
                    elixir_pair_section(node, source, Ancestors::known(chain)).map(
                        |(key, call)| {
                            (
                                key.to_owned(),
                                call.utf8_text(source).expect("call is UTF-8").to_owned(),
                            )
                        },
                    ),
                );
            }
        });
        assert_eq!(
            found,
            [None, Some(("else".to_owned(), "f(else: 2)".to_owned()))]
        );
    }
}
