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

// One step of the value peel (see `PeelStep`): a parenthesis, a cast
// (its `value`) and a `!` (its `operand`, proving it boolean). `(x)` and
// `(boolean) x` preserve the type of what they wrap; `!x` is boolean
// whatever `x` is. A `unary_expression` spelled `-x`, `+x` or `~x` yields
// a value rather than wrapping a test, so the peel stops on it.
//
// The operand is read by role, not at child index one after the `(` or
// the operator token: a comment may sit there (`(/*c*/ b)`, `! /*c*/ b`)
// and scored the condition zero (#1455). `parenthesized_expression`
// names nothing, so it takes the first operand that is not an extra.
fn java_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Java::*;

    match node.kind_id().into() {
        ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        CastExpression => node.child_by_field_name("value").map(|o| (o, false)),
        UnaryExpression if node.child(0).is_some_and(|c| c.kind_id() == BANG as u16) => {
            node.child_by_field_name("operand").map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison, a ternary (its `?`), an `instanceof` test
// (their token arms) or an `&&` / `||` chain, whose operands each pay
// through `java_count_condition`.
//
// An assignment is not peeled, because the `AssignmentExpression` arm
// already scores its right-hand side as a negated operand: peeling
// `if (y = !b)` down to `b` would pay the slot for the negation that arm
// counts. So it scores itself exactly when its value does — through that
// arm, or through its own operator's arm (`if (y = x > 1)`) — and a bare
// `if ((y = b))` pays through the slot.
fn java_condition_scores_itself(expr: &Node) -> bool {
    use Java::*;

    match expr.kind_id().into() {
        TernaryExpression | InstanceofExpression => true,
        BinaryExpression => expr.child_by_field_name("operator").is_some_and(|op| {
            matches!(
                op.kind_id().into(),
                EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ | AMPAMP | PIPEPIPE
            )
        }),
        AssignmentExpression => expr.child_by_field_name("right").is_some_and(|right| {
            let (value, proves_boolean) = peel(&right, java_wrapper_operand);
            proves_boolean || java_condition_scores_itself(&value)
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `while` / `do` / `for` condition,
// a ternary condition, a `case … when` guard, an operand of an `&&` /
// `||` chain (see `count_boolean_slot`). A Java slot only admits a
// `boolean`, and the terminal-kind list it used to pay for missed valid
// ones: `if ((y = b))`, `if (a & b)`, `if (a ^ b)`, `if (a | b)` and a
// boolean `switch` expression each scored 0 conditions against a
// cyclomatic decision of 1 (#1526). It also pays for an ill-typed
// `if (-x)`, which no valid program can tell apart (grammar-dispatch §6).
fn java_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        java_wrapper_operand,
        java_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, an argument, a
// declarator or assignment value, a lambda body, a ternary branch —
// scores only when a `!` proves it boolean (see `count_negated_operand`).
fn java_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        java_wrapper_operand,
        java_condition_scores_itself,
        conditions,
    );
}

// Fitzpatrick Rule 9 (#403): each operand of an `&&` / `||` chain is a
// boolean slot. `a && b || c` is a left-nested chain of
// `binary_expression`s, so an operand that is itself a chain is paid by
// its own operator's visit. Read by field: a comment beside the operator
// is a named child too, and must not pay.
fn java_count_chain_operands(chain: &Node, conditions: &mut f64) {
    for field in ["left", "right"] {
        if let Some(operand) = chain.child_by_field_name(field) {
            java_count_condition(&operand, conditions);
        }
    }
}

fn java_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        java_count_condition(&condition, conditions);
    }
}

fn java_count_negated_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(operand) = slot {
        java_count_negated(&operand, conditions);
    }
}

// ABC token-level helpers for Java. Each helper covers one of the four
// categories ABC tracks (assignments / branches / conditions / walked
// unary conditions). Each returns `true` when it owns the node so the
// dispatcher in `impl Abc for JavaCode::compute` can short-circuit and
// avoid re-matching the same kind across categories. The arms are
// mutually exclusive in the source language so a short-circuit chain
// reproduces the original `match` semantics bit-for-bit.

// Whether `eq_node` initialises a `final` binding, whose initializer is
// part of the declaration and therefore not an ABC assignment: its
// parent is a `variable_declarator` whose parent is a local or field
// declaration whose leading `modifiers` node holds `final`. The
// structural form replaces a sentinel stack that was pushed on the
// declaration, promoted on `final` and cleared only on the next `;`,
// so every `=` *inside* a `final` initializer — a lambda body's
// `x = 1`, an array initializer's — was suppressed with it (the #1277
// defect in its Java spelling). Only the declarator's own `=` is part
// of the declaration; `final int[] a = { x = 1 };` counts one.
//
// The modifiers precede the declarators, so the scan stops at the
// first `variable_declarator` and a wide `int a0 = 0, a1 = 1, …` stays
// linear; both hops read the ancestor chain the walk already
// descended through (#1096).
fn java_eq_initializes_final_binding<'a>(eq_node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    use Java::*;
    eq_node.parent_grandparent_match(
        ancestors,
        |parent| parent.kind_id() == VariableDeclarator,
        |declaration| {
            matches!(
                declaration.kind_id().into(),
                LocalVariableDeclaration | FieldDeclaration
            ) && declaration
                .children()
                .take_while(|child| child.kind_id() != VariableDeclarator)
                .any(|child| child.kind_id() == Modifiers && child.is_child(Final as u16))
        },
    )
}

// Counts assignment tokens; a plain `=` counts unless it initialises a
// `final` binding.
fn java_count_token_assignment<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Java::*;
    match node.kind_id().into() {
        STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | AMPEQ | PIPEEQ
        | CARETEQ | GTGTGTEQ | PLUSPLUS | DASHDASH => {
            stats.assignments += 1.;
        }
        EQ => {
            if !java_eq_initializes_final_binding(node, ancestors) {
                stats.assignments += 1.;
            }
        }
        _ => return false,
    }
    true
}

// Counts branch tokens: every method call, `new` allocation, and
// constructor delegation.
// `ExplicitConstructorInvocation` is the `super(…)` / `this(…)` delegation
// at the head of a constructor — a call by Fitzpatrick's rule, and one
// Groovy already counted for identical source (#1279). It is a distinct
// production that does not wrap a `MethodInvocation` for the delegation
// itself, so listing it adds no double count; calls in its argument list
// are separate nodes and still count on their own.
fn java_count_token_branch(node: &Node, stats: &mut Stats) -> bool {
    use Java::*;
    let is_branch = match node.kind_id().into() {
        MethodInvocation | New | ExplicitConstructorInvocation => true,
        // An enum constant carrying constructor arguments — `A(1)` in
        // `enum E { A(1), B, C(2); }` — invokes the enum's constructor, so
        // it is an object construction under Fitzpatrick's "function
        // invocation or object construction" rule. It scored zero until
        // #1407, which is the inconsistent position once #1279 decided
        // `super(…)` / `this(…)` above: both are a constructor call the
        // source spells out, and Kotlin's `class Sub : Base(1, 2)` was
        // settled the same way in #1384.
        //
        // The counter-argument is that an enum constant is a declaration,
        // not a call site a reader navigates to. It loses because the same
        // is true of the delegation forms already counted, and because the
        // arguments still have to be understood as a constructor's, which
        // is the effort ABC is measuring.
        //
        // The child gate is what makes this a §6 narrowing rather than a
        // new node: a bare `B` is `enum_constant > identifier` with no
        // `argument_list` child and must stay at zero. The gate is
        // specifically on `argument_list`, so an annotated constant
        // (`@Deprecated A` / `@Foo(1) A`) cannot satisfy it — an
        // annotation's arguments are a distinct `annotation_argument_list`
        // production, and they hang off the constant's `modifiers` child
        // rather than off the constant directly. Both verified with `bca
        // dump`. An argument that is itself a call (`A(f())`) scores 2:
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

// Counts condition tokens: comparison operators, control-flow keywords,
// and the two tokens Java's generic syntax shares with an operator —
// `<` / `>` count only in comparison position and `?` only as a ternary
// head, each gated on its parent kind. The `default` arm of a
// `switch` is excluded: it is the unconditional fallthrough, so
// cyclomatic counts only the `Case` arms (issue #469). Java's classic
// statement switch (`default:`) and arrow switch (`default ->`) both
// emit the same `Default` token under `switch_label`, so omitting it
// here covers both forms.
fn java_count_token_condition<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Java::*;
    match node.kind_id().into() {
        // `x instanceof Foo` joins them in #1461, scored by use rather
        // than by slot.
        // It sat in `java_bool_terminal_kinds!()` until then, which
        // counts only inside a boolean slot: `boolean b = x instanceof
        // String;` scored zero where the `boolean b = x == 1;` beside it
        // scored one, because `EQEQ` is a token arm and `instanceof` was
        // not. Fitzpatrick Rule 5 scores a relational operator wherever
        // it is written.
        //
        // Matched as the node rather than as the `instanceof` token
        // because one node spans both spellings — the plain test and
        // Java 16's pattern form `x instanceof String s` — and no arm
        // counts the keyword, so exactly one fires per test (§5).
        //
        // It shares the arm rather than sitting beside it because the
        // arm's meaning is "this node is a condition, with nothing to
        // gate on" — which is as true of a production as of a token.
        GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Try | Catch | InstanceofExpression => {
            stats.conditions += 1.;
        }
        // `?` opens a ternary, but tree-sitter-java also emits it bare
        // as the head of a `wildcard` type argument
        // (`List<? extends T>`), which is type syntax and no more a
        // decision than the `<` / `>` around it (#1274). Those two
        // productions are the only ones that emit a bare `?`, so the
        // same allowlist polarity used below settles it.
        QMARK => {
            if ancestors
                .parent(node)
                .is_some_and(|parent| matches!(parent.kind_id().into(), TernaryExpression))
            {
                stats.conditions += 1.;
            }
        }
        // Counts `<` / `>` only as the operator token of a
        // `binary_expression` — the polarity C / C++ / Rust / Go use.
        // tree-sitter-java emits a bare `<` / `>` from exactly three
        // productions (`binary_expression`, `type_arguments`,
        // `type_parameters`), so this is the inverse of denying the two
        // generic-type contexts, and it does not have to be revisited
        // when a grammar bump adds a fourth type-syntax one. The
        // previous denylist named only `type_arguments`, leaving every
        // generic *declaration* — `class Gen<T>`, `<T> void m()`, both
        // `type_parameters` — worth two conditions (#1274). A nested
        // generic (`Map<String, List<T>>`) closes with two separate `>`
        // tokens under their own `type_arguments`, not one `>>`; the
        // shifts are distinct tokens and never reach this arm.
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

fn java_walk_for_conditions<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
    use Java::*;
    let conds = &mut stats.conditions;
    match node.kind_id().into() {
        // Each operand of an `&&` / `||` chain is a boolean slot.
        AMPAMP | PIPEPIPE => {
            if let Some(chain) = ancestors
                .parent(node)
                .filter(|p| p.kind_id() == BinaryExpression)
            {
                java_count_chain_operands(&chain, conds);
            }
        }
        // Negated operands among method arguments.
        ArgumentList => {
            for argument in node.children().filter(is_operand) {
                java_count_negated(&argument, conds);
            }
        }
        // `if (cond)`, `while (cond)`, `do … while (cond);`, by grammar
        // field: a fixed index lands on a comment before the slot
        // (`if /*c*/ (b)`) and scores the condition zero (#1455).
        IfStatement | WhileStatement | DoStatement => {
            java_count_slot(node.child_by_field_name("condition"), conds);
        }
        // `return value;` names no field; the value is its only operand.
        ReturnStatement => java_count_negated_slot(wrapped_operand(node), conds),
        // The Java 21 pattern-switch guard (`case Integer i when g ->`),
        // modelled as a condition slot exactly like the `if` / `while`
        // slots above (#1454, transferring #1422's C# rule). Before
        // this, a guard scored whatever operator happened to sit inside
        // it: `when i > 5` counted one via the comparison-token arm
        // while `when isEven(i)` and `when b` counted zero, so three
        // semantically identical guards produced two different numbers.
        // As a slot every spelling contributes exactly one, and a
        // compound guard (`when a > 1 && b < 2`) keeps its
        // sub-structure rather than collapsing to one.
        //
        // By role, not index (`.claude/rules/grammar-dispatch.md` §3):
        // `guard` is `seq('when', expression)` and node-types.json gives
        // it no field, so the expression is the clause's first operand
        // that is not an `extra` — `when /*c*/ g` puts a comment before
        // it, which would pay as a slot of its own.
        Guard => java_count_slot(wrapped_operand(node), conds),
        // Declarator / assignment RHS and lambda body (`params -> body`),
        // by field for the same reason as the condition slots above.
        VariableDeclarator => java_count_negated_slot(node.child_by_field_name("value"), conds),
        AssignmentExpression => java_count_negated_slot(node.child_by_field_name("right"), conds),
        LambdaExpression => java_count_negated_slot(node.child_by_field_name("body"), conds),
        TernaryExpression => java_walk_ternary(node, stats),
        ForStatement => java_walk_for_statement(node, stats),
        _ => {}
    }
}

fn java_walk_ternary(node: &Node, stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // Slots are addressed by grammar FIELD, not by index. The positional
    // form read children 0 / 2 / 4, and tree-sitter counts comments among
    // a node's children, so `a ? /*n*/ !b : c` put the comment at index 2
    // and the negated operand went uninspected — the ternary scored 2
    // where the same expression without the comment scores 3 (#1181).
    // That is the mirror image of the over-count the token-based seed
    // produced in the C family, from the same cause.
    if let Some(condition) = node.child_by_field_name("condition") {
        java_count_condition(&condition, conds);
    }
    for field in ["consequence", "alternative"] {
        java_count_negated_slot(node.child_by_field_name(field), conds);
    }
}

// The `for (init; condition; update)` condition slot, addressed by
// grammar FIELD. This replaces a positional cascade that read child(3),
// and child(4) when child(3) was the `;` an expression initializer
// leaves behind. Two things that cascade got wrong, both fixed here by
// construction (#1276):
//
//   * A comment anywhere in the header shifted every index, so
//     `for (; /* n */ a; )` scored zero where `for (; a; )` scores one
//     — the same positional failure #1181 removed from the ternary.
//   * `SEMI` / `RPAREN` landing at child(4) was counted as a
//     vacuously-true condition, so `for (;;)` scored one. Java and
//     Groovy were the only two impls doing that; see the `Stats` doc
//     comment's cross-language empty-`for`-condition policy.
fn java_walk_for_statement(node: &Node, stats: &mut Stats) {
    if let Some(condition) = node.child_by_field_name("condition") {
        java_count_condition(&condition, &mut stats.conditions);
    }
}

// Fitzpatrick, Jerry (1997). "Applying the ABC metric to C, C++ and Java". C++ Report.
// Source: https://www.softwarerenovation.com/Articles.aspx
// ABC Java rules: (page 8, figure 4)
// ABC Java example: (page 15, listing 4)
impl Abc for JavaCode {
    // Short-circuit chain across four mutually-exclusive category
    // helpers. Each helper returns `true` when it owns the node, so
    // the dispatcher early-exits to avoid re-matching the same kind in
    // a later helper. The original pre-refactor `match` enforced
    // one-arm-per-kind by construction; this chain preserves the same
    // semantics only as long as no node kind is matched by more than
    // one helper. If you add a new arm covering a kind already matched
    // by an earlier helper, the earlier helper's `return` will silently
    // hide it — split the kinds across helpers explicitly instead.
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        if java_count_token_assignment(node, ancestors, stats) {
            return;
        }
        if java_count_token_branch(node, stats) {
            return;
        }
        if java_count_token_condition(node, ancestors, stats) {
            return;
        }
        java_walk_for_conditions(node, ancestors, stats);
    }
}
