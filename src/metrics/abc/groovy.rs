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
    Abc, Stats, count_boolean_slot, count_each_operand, count_field_operands,
    count_negated_operand, peel, rows_own, wrapped_operand,
};
use crate::roles::groovy::{
    branch, case_arm, catch, comparison, null_safe, short_circuit, ternary,
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
// `instanceof` test (the `Comparison` row's owners, paid at their
// token or, for the negated membership forms, at the node), a ternary
// (its `?`) or an elvis (its `?:`, the `NullSafe` owner paid by that
// token — not the row's three safe navigations), or an `&&` /
// `||` chain (the `ShortCircuit` row's owner), whose operands each pay
// through `groovy_count_condition`. Neither owner gate reads ancestors,
// so the peeled descendant is asked with none.
//
// An assignment is not peeled, because the `AssignmentExpression` arm
// already scores its right-hand side as a negated operand: peeling
// `if (y = !b)` down to `b` would pay the slot for the negation that arm
// counts. It scores itself exactly when its value does, as Java's does.
// Whether `expr` is the elvis `a ?: b`, the `NullSafe` owner whose
// payer is the `?:` token — asked of the row's projection, so the owner
// and the token the arm pays agree by construction.
// role-bypass: allow(NullSafe) — which of the row's four owners this consumer scores, told by
// the projected token: the elvis, not the safe navigations
fn groovy_is_elvis(expr: &Node, code: &[u8]) -> bool {
    null_safe::owns_via(Groovy::QMARKCOLON as u16, expr, code, Ancestors::unknown())
}

fn groovy_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    use Groovy::*;

    // The elvis owner is the one `NullSafe` owner that is a boolean
    // test; the three safe navigations never reach a slot as a value
    // that proves boolean, and score nothing if one does.
    rows_own!(expr, code; comparison, short_circuit, ternary)
        || groovy_is_elvis(expr, code)
        || match expr.kind_id().into() {
            AssignmentExpression => expr.child_by_field_name("right").is_some_and(|right| {
                let (value, proves_boolean) = peel(&right, groovy_wrapper_operand);
                proves_boolean || groovy_condition_scores_itself(&value, code)
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
fn groovy_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        groovy_wrapper_operand,
        |expr| groovy_condition_scores_itself(expr, code),
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, an argument, a
// declarator or assignment value, a ternary branch — scores only when a
// `!` proves it boolean (see `count_negated_operand`).
fn groovy_count_negated(operand: &Node, code: &[u8], conditions: &mut f64) {
    count_negated_operand(
        operand,
        groovy_wrapper_operand,
        |expr| groovy_condition_scores_itself(expr, code),
        conditions,
    );
}

fn groovy_count_slot(slot: Option<Node>, code: &[u8], conditions: &mut f64) {
    if let Some(condition) = slot {
        groovy_count_condition(&condition, code, conditions);
    }
}

fn groovy_count_negated_slot(slot: Option<Node>, code: &[u8], conditions: &mut f64) {
    if let Some(operand) = slot {
        groovy_count_negated(&operand, code, conditions);
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
// groovy 0.3.0 parses a `final` local as an `ERROR` node, with or
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
            // role-bypass: allow(Member, Modifier) — the binding shape a `=`
            // initialises, a local or a field alike, and which modifier it
            // carries; not whether it is a container's member or a modifier
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

fn groovy_count_token_branch<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use crate::roles::groovy::call;
    use Groovy::*;
    let kind = node.kind_id();
    // The `Call` row's method invocations and command chains, and the
    // `new` the row deliberately leaves out — object construction, which
    // Fitzpatrick's rule counts and ABC reads as its own.
    let is_branch = match kind.into() {
        _ if call::owns_kind(kind, node, code, ancestors) || kind == New as u16 => true,
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
        // role-bypass: allow(DocTarget) — whether the constant calls a
        // constructor, an ABC branch reading, not whether it takes a doc comment
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
// (the `CaseArm` row Groovy's cyclomatic reads owns `Case` and never
// `Default`).
fn groovy_count_token_condition<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Groovy::*;
    let kind = node.kind_id();
    // The `Comparison` row's payers (`roles/groovy.rs`): the six
    // Java-style tokens, with `<` / `>` gated to a `binary_expression`
    // — the dekobon grammar emits a bare `<` / `>` from four
    // productions (`binary_expression`, `type_arguments`,
    // `type_parameters` and `method_type_parameters` for `def <U> U
    // m(U x)`), the previous denylist named only the first type one
    // (#1274), and an explicit type witness
    // (`Collections.<String>emptyList()`) the grammar cannot parse puts
    // its `<` under an `ERROR` only the positive form copes with — the
    // identity `===` / `!==`, regex `=~` / `==~` and spaceship `<=>`
    // tokens, each emitted by exactly one production (#1461), the `in`
    // of a membership test and `instanceof`, and the negated `!in` /
    // `!instanceof` nodes, which carry no token to pay (#1461). One pays
    // per test (§5). Fitzpatrick Rule 5 scores a relational operator
    // wherever it is written, so `def r = a <=> b` and
    // `def b = a in l` score as `def r = a == b` does. The `Ternary`
    // row's `?` likewise, gated by the row to the ternary — as in Java,
    // a bare `?` is also the head of a `wildcard` type argument, which
    // is no decision (#1274).
    // And the `CaseArm` row's `case` label; `default` is excluded on the
    // row (lesson 11, #469).
    if comparison::pays_at_leaf_kind(kind, node, code, ancestors)
        || ternary::pays_at_leaf_kind(kind, node, code, ancestors)
        || case_arm::pays_at_leaf_kind(kind, node, code, ancestors)
        || catch::pays_at_leaf_kind(kind, node, code, ancestors)
    {
        stats.conditions += 1.;
        return true;
    }
    match kind.into() {
        // Fitzpatrick's `else` and `try`, which no row owns; `catch` is
        // the `Catch` row's payer, read above (#1697).
        Else | Try => {
            stats.conditions += 1.;
        }
        // The elvis `a ?: c`, the one `NullSafe` payer that is a boolean
        // test: a short-circuit decision Groovy cyclomatic counts and the
        // Kotlin ABC arm counts for its identical token; without it a
        // method whose only branching is elvis chains reported cyclomatic
        // > 1 with zero conditions. One per token, the Kotlin reading,
        // rather than the C / PHP short-ternary reading that also walks
        // the left operand as a unary condition — it keeps
        // `abc.conditions` equal to `cyclomatic() - 1` on the chain
        // (grammar-dispatch §8). The row's three navigations are no
        // boolean test and ABC does not score them.
        // role-bypass: allow(NullSafe) — which of the row's four payers this consumer scores:
        // the elvis, not the safe navigations
        _ if kind == QMARKCOLON as u16
            && null_safe::pays_at_leaf_kind(kind, node, code, ancestors) =>
        {
            stats.conditions += 1.;
        }
        _ => return false,
    }
    true
}

fn groovy_walk_for_conditions<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    use Groovy::*;
    let conds = &mut stats.conditions;
    let kind = node.kind_id();
    // Each operand of an `&&` / `||` chain is a boolean slot; the
    // `ShortCircuit` row is read at its leaf only to find the chain,
    // the token's parent.
    if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) {
        if let Some(chain) = ancestors.parent(node) {
            count_field_operands(
                &chain,
                |operand, conds| groovy_count_condition(operand, code, conds),
                conds,
            );
        }
        return;
    }
    match kind.into() {
        ArgumentList => count_each_operand(
            node,
            |operand, conds| groovy_count_negated(operand, code, conds),
            conds,
        ),
        VariableDeclarator => {
            groovy_count_negated_slot(node.child_by_field_name("value"), code, conds);
        }
        AssignmentExpression => {
            groovy_count_negated_slot(node.child_by_field_name("right"), code, conds);
        }
        // The dekobon grammar inlines the parens of `if` / `while` /
        // `do … while` as anonymous tokens (tree-sitter-java wraps them
        // in a `parenthesized_expression`), so the slot holds the bare
        // condition. Read by grammar field: the fixed index it replaced
        // landed on a comment in `if (/*c*/ b)` (#1455).
        // Every `Branch` owner, and the `Loop` owners with a `condition`
        // field — the two `for` forms walk theirs below or have none.
        // role-bypass: allow(Loop) — the loop owners with a `condition`
        // slot, a subset the row cannot express; `Branch` is read in full
        _ if branch::owns_kind(kind, node, code, ancestors)
            || matches!(kind.into(), WhileStatement | DoWhileStatement) =>
        {
            groovy_count_slot(node.child_by_field_name("condition"), code, conds);
        }
        // `return value` names no field; the value is its only operand.
        // role-bypass: allow(Exit) — a return's value as a boolean slot,
        // not whether the node is an exit
        ReturnStatement => groovy_count_negated_slot(wrapped_operand(node), code, conds),
        // role-bypass: allow(Loop) — the one loop owner with a three-part
        // head, not whether the node is a loop
        ForStatement => groovy_walk_for_statement(node, code, stats),
        // The `ternary_expression` the `Ternary` row owns: its `?` paid
        // above, this walks its operand slots.
        _ if ternary::owns_kind(kind, node, code, ancestors) => {
            groovy_walk_ternary(node, code, stats);
        }
        _ => {}
    }
}

fn groovy_walk_ternary(node: &Node, code: &[u8], stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // By grammar FIELD, not index — see `java_walk_ternary` for why the
    // positional form dropped a negated branch operand behind a comment
    // (#1181).
    groovy_count_slot(node.child_by_field_name("condition"), code, conds);
    for field in ["consequence", "alternative"] {
        groovy_count_negated_slot(node.child_by_field_name(field), code, conds);
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
fn groovy_walk_for_statement(node: &Node, code: &[u8], stats: &mut Stats) {
    groovy_count_slot(
        node.child_by_field_name("condition"),
        code,
        &mut stats.conditions,
    );
}

impl Abc for GroovyCode {
    // See `impl Abc for JavaCode` for the short-circuit-chain rationale
    // and the cross-helper-exclusivity invariant.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        if groovy_count_token_assignment(node, ancestors, stats) {
            return;
        }
        if groovy_count_token_branch(node, code, ancestors, stats) {
            return;
        }
        if groovy_count_token_condition(node, code, ancestors, stats) {
            return;
        }
        groovy_walk_for_conditions(node, code, ancestors, stats);
    }
}
