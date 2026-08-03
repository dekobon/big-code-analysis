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

use super::{Abc, DeclKind, Stats};
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

fn groovy_count_token_assignment(node: &Node, stats: &mut Stats) -> bool {
    use Groovy::*;
    match node.kind_id().into() {
        STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | AMPEQ | PIPEEQ
        | CARETEQ | GTGTGTEQ | PLUSPLUS | DASHDASH => {
            stats.assignments += 1.;
        }
        FieldDeclaration | LocalVariableDeclaration => {
            stats.declaration.push(DeclKind::Var);
        }
        Final => {
            if let Some(DeclKind::Var) = stats.declaration.last() {
                stats.declaration.push(DeclKind::Const);
            }
        }
        SEMI => {
            if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                stats.declaration.clear();
            }
        }
        EQ => {
            if stats
                .declaration
                .last()
                .is_none_or(|decl| matches!(decl, DeclKind::Var))
            {
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
        GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | QMARK | Try | Catch => {
            stats.conditions += 1.;
        }
        // Excludes `<` / `>` used for generic types (e.g. `List<String>`).
        GT | LT => {
            if let Some(parent) = ancestors.parent(node)
                && !matches!(parent.kind_id().into(), TypeArguments)
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

// Two shapes: a present condition lives at child(3); an empty condition
// shows up as a bare `SEMI` token at child(3) with the next slot
// (child(4)) holding either the update expression or `;` / `)` for
// `for(;;)`-style loops, which still count as a single condition slot.
fn groovy_walk_for_statement(node: &Node, stats: &mut Stats) {
    use Groovy::*;
    let Some(condition) = node.child(3) else {
        return;
    };
    if !matches!(condition.kind_id().into(), SEMI) {
        groovy_count_condition(&condition, node, &mut stats.conditions);
        return;
    }
    let Some(cond) = node.child(4) else { return };
    if matches!(cond.kind_id().into(), SEMI | RPAREN) {
        stats.conditions += 1.;
    } else {
        groovy_count_condition(&cond, node, &mut stats.conditions);
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
        if groovy_count_token_assignment(node, stats) {
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
