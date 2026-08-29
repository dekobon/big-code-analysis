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

use super::{Abc, Stats};
use crate::macros::groovy_bool_terminal_kinds;
use crate::*;

fn groovy_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Groovy::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoWhileStatement | ForStatement => true,
        TernaryExpression => parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()),
        _ => false,
    };

    loop {
        let is_parenthesised_exp = matches!(node_kind, ParenthesizedExpression);
        let is_not_operator = matches!(node_kind, UnaryExpression)
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        if !is_parenthesised_exp && !is_not_operator {
            break;
        }

        if !has_boolean_content && is_not_operator {
            has_boolean_content = true;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        // `BooleanLiteral` is the dekobon tree-sitter-groovy
        // grammar's named wrapper for `true` / `false` — see the
        // doc comment on `groovy_count_condition`. The remaining
        // bool-evaluating terminals (`FieldAccess`, `CastExpression`,
        // `ParenthesizedTypeCast`, `InstanceofExpression`) mirror
        // the C# fix in #372 (lesson #19).
        if matches!(node_kind, groovy_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

fn groovy_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Groovy::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            // Terminal set mirrors `groovy_inspect_container` —
            // bool-evaluating kinds (`FieldAccess`, `CastExpression`,
            // `ParenthesizedTypeCast`, `InstanceofExpression`) added
            // per issue #372 (lesson #19).
            if matches!(node_kind, groovy_bool_terminal_kinds!())
                && matches!(list_kind, BinaryExpression)
            {
                *conditions += 1.;
            } else {
                groovy_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ABC token-level helpers for Groovy. Mirrors the Java helper layout
// (assignments / branches / conditions / walked) with the dekobon
// Groovy grammar's specific deltas — `CommandChain` as a branch
// alongside `MethodInvocation` (#247); `DoWhileStatement` replacing
// Java's `DoStatement`; no `LambdaExpression` (Groovy closures take
// block bodies, no implicit-return arm); and `if (…)` / `while (…)` /
// `do { … } while (…)` parens inlined as token children rather than
// wrapped in `parenthesized_expression`, so the condition sits at a
// different child index and goes through `groovy_count_condition`.

// Groovy mirror of `java_inspect_child`: passes `node.child(idx)` to
// `groovy_inspect_container`, which is a no-op on kinds other than
// `ParenthesizedExpression` / `!`-prefixed `UnaryExpression`.
fn groovy_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        groovy_inspect_container(&child, node, conditions);
    }
}

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
    if matches!(node.kind_id().into(), MethodInvocation | CommandChain | New) {
        stats.branches += 1.;
        return true;
    }
    false
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
        GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Try | Catch | QMARKCOLON => {
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
        AMPAMP | PIPEPIPE => {
            if let Some(parent) = ancestors.parent(node) {
                groovy_count_unary_conditions(&parent, conds);
            }
        }
        ArgumentList => groovy_count_unary_conditions(node, conds),
        VariableDeclarator | AssignmentExpression => groovy_inspect_child(node, 2, conds),
        // dekobon `if_statement` / `while_statement` shape:
        // [keyword, `(`, condition, `)`, body, …]. Condition lives at
        // child index 2 (not 1 as under tree-sitter-java, where parens
        // wrap the condition in a `parenthesized_expression`).
        IfStatement | WhileStatement => {
            if let Some(condition) = node.child(2) {
                groovy_count_condition(&condition, node, conds);
            }
        }
        // dekobon shape: [`do`, body, `while`, `(`, condition, `)`].
        // Condition is at child index 4.
        DoWhileStatement => {
            if let Some(condition) = node.child(4) {
                groovy_count_condition(&condition, node, conds);
            }
        }
        ReturnStatement => groovy_inspect_child(node, 1, conds),
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
    if let Some(condition) = node.child_by_field_name("condition") {
        groovy_count_condition(&condition, node, conds);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            groovy_inspect_container(&branch, node, conds);
        }
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
    if let Some(condition) = node.child_by_field_name("condition") {
        groovy_count_condition(&condition, node, &mut stats.conditions);
    }
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

fn groovy_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Groovy::*;
    // Terminal set mirrors the C# fix in #372 (lesson #19):
    // `FieldAccess` (`obj.flag`), `CastExpression` (`v as Boolean` — the
    // Groovy-idiomatic form), `ParenthesizedTypeCast` (`(boolean) v` —
    // the Java-style form, which the dekobon Groovy grammar represents
    // as its own kind rather than nesting `cast_expression` inside
    // `parenthesized_expression`), and `InstanceofExpression`
    // (`x instanceof Foo`) all evaluate to a boolean. The dekobon
    // Groovy grammar has no `await` or `array_access` analogues, so
    // those collapse out of the five-kind C# set.
    match condition.kind_id().into() {
        groovy_bool_terminal_kinds!() => {
            *conditions += 1.;
        }
        ParenthesizedExpression | UnaryExpression => {
            groovy_inspect_container(condition, parent, conditions);
        }
        _ => {}
    }
}
