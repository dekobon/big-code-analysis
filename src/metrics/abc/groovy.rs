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

use super::{
    Abc, Stats, count_boolean_slot, count_negated_operand, is_operand, peel, wrapped_operand,
};
use crate::*;

// One peel step for a Groovy boolean operand: given a wrapper node,
// returns the operand inside it plus whether the wrapper itself *proves*
// the operand sits in a boolean slot. `None` means the node is not a
// wrapper this walker unwraps, and `groovy_count_condition` asks exactly
// that rather than restating the kind list — the divergence #1459 fixed
// in Kotlin, where `unary_expression` was routed to the peel while the
// peel handled only its `!` spelling, so the slot read as covering a
// shape the peel dropped on the floor (`.claude/rules/grammar-dispatch.md`
// §7).
//
// The operand is read by grammar field (§3): `unary_expression` names
// `operator` and `operand`, so one read serves all four spellings and
// survives both a grammar re-order and an interposed `extra`
// (`if (! /*c*/ a)` scores, where the positional `child(1)` read scored
// the comment). `parenthesized_expression` names nothing — its only
// child in node-types.json is the unlabelled inner `_expression` — so it
// takes the first operand that is not an extra; the positional read it
// replaced scored `if ( /*c*/ a)` zero (#1455).
//
// A cast (`x as T` spelled `(T) x`, or `x as T`) evaluates to its
// `value`, so a cast of a comparison pays once, through the comparison.
//
// Every `?` below is infallible at the pinned grammar and is spelled
// that way because `AGENTS.md` bans `expect` outside tests — do not try
// to cover the `None` arms. `unary_expression` declares `operand` and
// `operator` as required fields, and a `parenthesized_expression` is
// `(` expr `)`, so an operand exists. Only error recovery on invalid
// Groovy can produce a shorter node, and pinning that would make the
// grammar's present over-permissiveness the contract (§6).
fn groovy_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Groovy::*;

    match node.kind_id().into() {
        // `(expr)` — the inner expression follows the `(` token.
        ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        CastExpression | ParenthesizedTypeCast => {
            node.child_by_field_name("value").map(|o| (o, false))
        }
        UnaryExpression => {
            let operand = node.child_by_field_name("operand")?;
            match node.child_by_field_name("operator")?.kind_id().into() {
                BANG => Some((operand, true)),
                // `~a`, `+a`, `-a`: bitwise / arithmetic, never a
                // boolean slot's operand, so the peel declines rather
                // than reaching a bare `identifier` and counting it.
                _ => None,
            }
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison, identity, regex, spaceship, membership or
// `instanceof` test, a ternary (its `?`) or an elvis (its `?:`) — each
// its own token or node arm — or an `&&` / `||` chain, whose operands
// each pay through `groovy_count_condition`.
//
// An assignment is not peeled, because the `AssignmentExpression` arm
// already scores its right-hand side as a negated operand: peeling
// `if (y = !b)` down to `b` would pay the slot for the negation that arm
// counts. It scores itself exactly when its value does, as Java's does.
fn groovy_condition_scores_itself(expr: &Node) -> bool {
    use Groovy::*;

    match expr.kind_id().into() {
        IdentityExpression | RegexFindExpression | RegexMatchExpression | SpaceshipExpression
        | MembershipExpression | InstanceofExpression | TernaryExpression | ElvisExpression => true,
        BinaryExpression => expr.child_by_field_name("operator").is_some_and(|op| {
            matches!(
                op.kind_id().into(),
                EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ | AMPAMP | PIPEPIPE
            )
        }),
        AssignmentExpression => expr.child_by_field_name("right").is_some_and(|right| {
            let (value, proves_boolean) = peel(&right, groovy_wrapper_operand);
            proves_boolean || groovy_condition_scores_itself(&value)
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `while` / `do-while` / `for`
// condition, a ternary condition, an operand of an `&&` / `||` chain
// (see `count_boolean_slot`). Groovy truth makes every value testable,
// so `if (-x)`, `if (x + 1)`, `if ((y = x))` and `if (a ==> b)` are each
// a decision, and each scored 0 while the slot paid only for a fixed
// list of terminal kinds (#1526).
fn groovy_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        groovy_wrapper_operand,
        groovy_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, an argument, a
// declarator or assignment value, a ternary branch — scores only when a
// `!` proves it boolean (see `count_negated_operand`).
fn groovy_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        groovy_wrapper_operand,
        groovy_condition_scores_itself,
        conditions,
    );
}

fn groovy_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        groovy_count_condition(&condition, conditions);
    }
}

fn groovy_count_negated_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(operand) = slot {
        groovy_count_negated(&operand, conditions);
    }
}

// ABC token-level helpers for Groovy. Mirrors the Java helper layout
// (assignments / branches / conditions / walked) with the dekobon
// Groovy grammar's specific deltas — `CommandChain` as a branch
// alongside `MethodInvocation` (#247); `DoWhileStatement` replacing
// Java's `DoStatement`; no `LambdaExpression` (Groovy closures take
// block bodies, no implicit-return arm); and `if (…)` / `while (…)` /
// `do { … } while (…)` parens inlined as token children rather than
// wrapped in `parenthesized_expression`, so the condition slot holds
// the bare expression and goes through `groovy_count_condition`.

// The Groovy spelling of `java_eq_initializes_final_binding`: the
// grammar puts `final` directly under the declaration rather than in a
// `modifiers` node. Only the field form is reachable — tree-sitter-
// groovy 0.2.2 parses a `final` local as an `ERROR` node, with or
// without a `;` (pinned by `groovy_final_local_is_an_error_at_the_pinned_grammar`)
// — but the local kind is listed so a grammar that starts parsing it
// classifies it like Java. Before this predicate a sentinel stack
// suppressed every `=` inside a `final` field's initializer, so a
// `final Closure c = { x = 1 }` hid the closure body's assignment.
fn groovy_eq_initializes_final_binding<'a>(
    eq_node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Groovy::*;
    eq_node.parent_grandparent_match(
        ancestors,
        |parent| parent.kind_id() == VariableDeclarator,
        |declaration| {
            matches!(
                declaration.kind_id().into(),
                FieldDeclaration | LocalVariableDeclaration
            ) && declaration
                .children()
                .take_while(|child| child.kind_id() != VariableDeclarator)
                .any(|child| child.kind_id() == Final)
        },
    )
}

fn groovy_count_token_assignment<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Groovy::*;
    match node.kind_id().into() {
        STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | AMPEQ | PIPEEQ
        | CARETEQ | GTGTGTEQ | PLUSPLUS | DASHDASH => {
            stats.assignments += 1.;
        }
        EQ => {
            if !groovy_eq_initializes_final_binding(node, ancestors) {
                stats.assignments += 1.;
            }
        }
        _ => return false,
    }
    true
}

fn groovy_count_token_branch(node: &Node, stats: &mut Stats) -> bool {
    use Groovy::*;
    let is_branch = match node.kind_id().into() {
        MethodInvocation | CommandChain | New => true,
        // An enum constant carrying constructor arguments — `A(1)` in
        // `enum E { A(1), B, C(2) }` — invokes the enum's constructor, so
        // it is an object construction under Fitzpatrick's "function
        // invocation or object construction" rule. It scored zero until
        // #1407, which is the inconsistent position once #1279 decided
        // Java's `super(…)` / `this(…)` — already counted here through
        // `MethodInvocation` — and #1384 decided Kotlin's `class Sub :
        // Base(1, 2)`: all three are a constructor call the source spells
        // out.
        //
        // The counter-argument is that an enum constant is a declaration,
        // not a call site a reader navigates to. It loses because the same
        // is true of a superclass delegation, and because the arguments
        // still have to be understood as a constructor's, which is the
        // effort ABC is measuring.
        //
        // The child gate is what makes this a §6 narrowing rather than a
        // new node: a bare `B` is `enum_constant > identifier` with no
        // `argument_list` child and must stay at zero. Verified with `bca
        // dump`: the dekobon Groovy grammar gives the constant an
        // `identifier` plus an optional `argument_list`. Java's annotated
        // spelling has no analogue to guard against here — this grammar
        // cannot parse `@Deprecated A(1)` inside an enum body at all and
        // recovers into `ERROR` nodes, so no `enum_constant` is produced.
        // An argument that is itself a call (`A(f())`) scores 2:
        // `argument_list` is not a branch node, so the inner
        // `method_invocation` is the only other node counted (§5).
        EnumConstant => node.is_child(ArgumentList as u16),
        _ => false,
    };
    if is_branch {
        stats.branches += 1.;
    }
    is_branch
}

// The `default` arm of a `switch` is excluded (issue #469): it is the
// unconditional fallthrough, so cyclomatic counts only the `Case` arms
// (Groovy shares Java's `impl_cyclomatic_java_like!`, which matches
// `Case` and never `Default`).
fn groovy_count_token_condition<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Groovy::*;
    match node.kind_id().into() {
        // `QMARKCOLON` is the elvis operator `a ?: c`, a short-circuit
        // decision Groovy cyclomatic already counts
        // (`src/metrics/cyclomatic/groovy.rs`) and the Kotlin ABC arm
        // counts for its identical token; without it a method whose only
        // branching is elvis chains reported cyclomatic > 1 with zero
        // conditions. One per token, the Kotlin reading, rather than the
        // C / PHP short-ternary reading that also walks the left operand
        // as a unary condition — it keeps `abc.conditions` equal to
        // `cyclomatic() - 1` on the chain (grammar-dispatch §8).
        // `EQEQEQ` / `BANGEQEQ` (`===`, `!==`) and `EQTILDE` /
        // `EQEQTILDE` (`=~`, `==~`) are Groovy's identity and regex
        // comparisons. They sit in this ungated arm beside `==` / `!=`
        // for the reason those do, and a `grammar.json` sweep of
        // dekobon-tree-sitter-groovy 0.2.2 is what makes ungating
        // safe: each of the four tokens is emitted by exactly one
        // production — `identity_expression` for the first pair,
        // `regex_find_expression` / `regex_match_expression` for the
        // second — so unlike `GT` / `LT` there is no type-argument or
        // loop-header spelling to exclude.
        //
        // Their own expression kinds are therefore **not** counted as
        // nodes; counting both would score each twice (§5). The token is
        // the better half of that choice
        // because it scores outside a boolean slot as well, where
        // `def r = (a == b)` already scored 1 and `def r = (a === b)`
        // scored 0 — a within-language asymmetry between two
        // equality operators. It is also how every other language here
        // spells these: `EQEQEQ | BANGEQEQ` are token arms in Kotlin,
        // the JS family, PHP and Elixir, and `EQTILDE` in Perl, Ruby
        // and Bash. `getter/groovy.rs` already classifies all four as
        // Halstead operators.
        //
        // `LTEQGT` is the spaceship `<=>`, added in #1461. It yields
        // -1 / 0 / 1 rather than a boolean, which is why it was not
        // listed in the boolean-operand kind set the slots paid for
        // until #1526 — that set held operands, and `<=>` is not one —
        // but it *is* a relational
        // operator, and Fitzpatrick Rule 5 counts those by use
        // regardless of result type — and `<=>` is the whole of a
        // three-way decision, not a fragment of one. Every sibling
        // language with a spaceship already counted it: `LTEQGT` is a
        // condition token in Ruby, PHP, C++ and Mozcpp, and Perl adds
        // its word spelling `cmp` beside it. Groovy was the outlier,
        // scoring `def r = a <=> b` zero against `a == b`'s one. The
        // same dekobon-tree-sitter-groovy 0.2.2 `grammar.json` sweep
        // finds `<=>` in `spaceship_expression` alone, so it is ungated
        // for the reason `EQEQEQ` is, and the expression node itself is
        // counted nowhere (§5).
        // Groovy's two relational forms with no usable operator token
        // join them in #1461,
        // scored by use rather than by slot (#1461). Both sat in the
        // slots' boolean-operand kind set until then, which counted only
        // inside a boolean slot: `def b = a in l` and
        // `def b = a instanceof String` scored zero where the
        // `def b = a == 1` beside them scored one.
        //
        // Matched as nodes rather than as tokens because neither
        // construct has a token this arm could use: `in` is shared with
        // the `for (x in l)` header, and the negated spellings `!in` /
        // `!instanceof` emit no operator token at all — `bca dump`
        // shows `membership_expression` with two `identifier` children
        // and nothing between them. One node covers both spellings of
        // each construct, and neither token is counted anywhere, so
        // exactly one arm fires per test (§5).
        //
        // They share the arm rather than sitting beside it because the
        // arm's meaning is "this node is a condition, with nothing to
        // gate on" — which is as true of a production as of a token.
        GTEQ | LTEQ | LTEQGT | EQEQ | BANGEQ | EQEQEQ | BANGEQEQ | EQTILDE | EQEQTILDE | Else
        | Case | Try | Catch | QMARKCOLON | MembershipExpression | InstanceofExpression => {
            stats.conditions += 1.;
        }
        // As in Java: a bare `?` is either a ternary head or the head of
        // a `wildcard` type argument (`List<? extends T>`), and only the
        // first is a decision (#1274).
        QMARK => {
            if ancestors
                .parent(node)
                .is_some_and(|parent| matches!(parent.kind_id().into(), TernaryExpression))
            {
                stats.conditions += 1.;
            }
        }
        // Counts `<` / `>` only as the operator token of a
        // `binary_expression` — see `java_count_token_condition` for
        // the polarity rationale. The dekobon Groovy grammar emits a
        // bare `<` / `>` from four productions: `binary_expression`,
        // `type_arguments`, `type_parameters`, and — unlike
        // tree-sitter-java — a separate `method_type_parameters` for
        // `def <U> U m(U x)`. The previous denylist named only
        // `type_arguments`, leaving both generic-declaration forms
        // worth two conditions (#1274). The positive form also copes
        // with a construct this grammar cannot parse: an explicit type
        // witness (`Collections.<String>emptyList()`) puts its `<`
        // under an `ERROR` node that no denylist can name, so only the
        // trailing `>` — which error recovery does hang off a
        // `binary_expression` — is still counted. Groovy's `<=>` is a
        // `spaceship_expression` carrying its own token, so it never
        // reaches this arm.
        GT | LT => {
            if ancestors
                .parent(node)
                .is_some_and(|parent| matches!(parent.kind_id().into(), BinaryExpression))
            {
                stats.conditions += 1.;
            }
        }
        _ => return false,
    }
    true
}

fn groovy_walk_for_conditions<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    use Groovy::*;
    let conds = &mut stats.conditions;
    match node.kind_id().into() {
        // Each operand of an `&&` / `||` chain is a boolean slot, read by
        // field so a comment beside the operator does not pay.
        AMPAMP | PIPEPIPE => {
            if let Some(chain) = ancestors
                .parent(node)
                .filter(|p| p.kind_id() == BinaryExpression)
            {
                for field in ["left", "right"] {
                    groovy_count_slot(chain.child_by_field_name(field), conds);
                }
            }
        }
        ArgumentList => {
            for argument in node.children().filter(is_operand) {
                groovy_count_negated(&argument, conds);
            }
        }
        VariableDeclarator => groovy_count_negated_slot(node.child_by_field_name("value"), conds),
        AssignmentExpression => {
            groovy_count_negated_slot(node.child_by_field_name("right"), conds);
        }
        // The dekobon grammar inlines the parens of `if` / `while` /
        // `do … while` as anonymous tokens (tree-sitter-java wraps them
        // in a `parenthesized_expression`), so the slot holds the bare
        // condition. Read by grammar field: the fixed index it replaced
        // landed on a comment in `if (/*c*/ b)` (#1455).
        IfStatement | WhileStatement | DoWhileStatement => {
            groovy_count_slot(node.child_by_field_name("condition"), conds);
        }
        // `return value` names no field; the value is its only operand.
        ReturnStatement => groovy_count_negated_slot(wrapped_operand(node), conds),
        TernaryExpression => groovy_walk_ternary(node, stats),
        ForStatement => groovy_walk_for_statement(node, stats),
        _ => {}
    }
}

fn groovy_walk_ternary(node: &Node, stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // By grammar FIELD, not index — see `java_walk_ternary` for why the
    // positional form dropped a negated branch operand behind a comment
    // (#1181).
    groovy_count_slot(node.child_by_field_name("condition"), conds);
    for field in ["consequence", "alternative"] {
        groovy_count_negated_slot(node.child_by_field_name(field), conds);
    }
}

// The `for (init; condition; update)` condition slot, addressed by
// grammar FIELD. This replaces a positional cascade that read child(3),
// and child(4) when child(3) was the `;` an expression initializer
// leaves behind. Two things that cascade got wrong, both fixed here by
// construction (#1276) — see `java_walk_for_statement`, whose identical
// cascade had the identical pair of defects:
//
//   * A comment anywhere in the header shifted every index.
//   * `SEMI` / `RPAREN` landing at child(4) was counted as a
//     vacuously-true condition, so `for (;;)` scored one; see the
//     `Stats` doc comment's cross-language empty-`for`-condition
//     policy.
fn groovy_walk_for_statement(node: &Node, stats: &mut Stats) {
    groovy_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
}

impl Abc for GroovyCode {
    // See `impl Abc for JavaCode` for the short-circuit-chain rationale
    // and the cross-helper-exclusivity invariant.
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        if groovy_count_token_assignment(node, ancestors, stats) {
            return;
        }
        if groovy_count_token_branch(node, stats) {
            return;
        }
        if groovy_count_token_condition(node, ancestors, stats) {
            return;
        }
        groovy_walk_for_conditions(node, ancestors, stats);
    }
}
