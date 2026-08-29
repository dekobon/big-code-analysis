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
use crate::macros::java_bool_terminal_kinds;
use crate::*;

// Inspects the content of Java parenthesized expressions
// and `Not` operators to find unary conditional expressions
fn java_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Java::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    // Initializes the flag to true if the container is known to contain a boolean value
    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement => true,
        TernaryExpression => parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()),
        _ => false,
    };

    // Looks inside parenthesized expressions and `Not` operators to find what they contain
    loop {
        // Checks if the node is a parenthesized expression or a `Not` operator
        // The child node of index 0 contains the unary expression operator (we look for the `!` operator)
        let is_parenthesised_exp = matches!(node_kind, ParenthesizedExpression);
        let is_not_operator = matches!(node_kind, UnaryExpression)
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        // Stops the exploration if the node is neither
        // a parenthesized expression nor a `Not` operator
        if !is_parenthesised_exp && !is_not_operator {
            break;
        }

        // Sets the flag to true if a `Not` operator is found
        // This is used to prove if a variable or a value returned by a method is actually boolean
        // e.g. `return (!x);`
        if !has_boolean_content && is_not_operator {
            has_boolean_content = true;
        }

        // Parenthesized expressions and `Not` operators nodes
        // always store their expressions in the children nodes of index one
        // https://github.com/tree-sitter/tree-sitter-java/blob/master/src/grammar.json#L2472
        // https://github.com/tree-sitter/tree-sitter-java/blob/master/src/grammar.json#L2150
        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        // Stops the exploration when the content is found. The terminal
        // set includes `FieldAccess` (`obj.flag`), `CastExpression`
        // (`(boolean)v`), `ArrayAccess` (`flags[0]`), and
        // `InstanceofExpression` (`x instanceof Foo`) — every kind whose
        // evaluated value is implicitly boolean in idiomatic Java, mirroring
        // the C# fix in #372 (lesson #19).
        if matches!(node_kind, java_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Inspects a list of elements and counts any unary conditional expression found
fn java_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Java::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    // Scans the immediate children nodes of the argument node
    if cursor.goto_first_child() {
        loop {
            // Gets the current child node and its kind
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            // Checks if the node is a unary condition. The terminal set
            // includes `FieldAccess`, `CastExpression`, `ArrayAccess`,
            // and `InstanceofExpression` so that bool-evaluating
            // operands of `&&` / `||` chains are not silently zeroed
            // out (mirrors the C# fix in #372; lesson #19).
            if matches!(node_kind, java_bool_terminal_kinds!())
                && matches!(list_kind, BinaryExpression)
            {
                *conditions += 1.;
            } else {
                // Checks if the node is a unary condition container
                java_inspect_container(&node, list_node, conditions);
            }

            // Moves the cursor to the next sibling node of the current node
            // Exits the scan if there is no next sibling node
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ABC token-level helpers for Java. Each helper covers one of the four
// categories ABC tracks (assignments / branches / conditions / walked
// unary conditions). Each returns `true` when it owns the node so the
// dispatcher in `impl Abc for JavaCode::compute` can short-circuit and
// avoid re-matching the same kind across categories. The arms are
// mutually exclusive in the source language so a short-circuit chain
// reproduces the original `match` semantics bit-for-bit.

// Shared helper: passes `node.child(idx)` to `java_inspect_container`.
// The container helper is a no-op on kinds other than
// `ParenthesizedExpression` / `!`-prefixed `UnaryExpression`, so no
// `matches!` guard is needed at the call site.
fn java_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        java_inspect_container(&child, node, conditions);
    }
}

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
    if matches!(
        node.kind_id().into(),
        MethodInvocation | New | ExplicitConstructorInvocation
    ) {
        stats.branches += 1.;
        return true;
    }
    false
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
        GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Try | Catch => {
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
        // Unary conditions in elements separated by `&&` / `||`.
        AMPAMP | PIPEPIPE => {
            if let Some(parent) = ancestors.parent(node) {
                java_count_unary_conditions(&parent, conds);
            }
        }
        // Unary conditions among method arguments.
        ArgumentList => java_count_unary_conditions(node, conds),
        // Child 1: `if (cond) ...`, `while (cond) ...`, `return value;`.
        IfStatement | WhileStatement | ReturnStatement => java_inspect_child(node, 1, conds),
        // Child 2: assignment / declarator RHS, lambda body
        // (`params -> body`).
        VariableDeclarator | AssignmentExpression | LambdaExpression => {
            java_inspect_child(node, 2, conds);
        }
        // Child 3: the `while (cond)` condition of `do { ... } while (...);`.
        DoStatement => java_inspect_child(node, 3, conds),
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
        java_count_condition(&condition, node, conds);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            java_inspect_container(&branch, node, conds);
        }
    }
}

// Classifies one condition-slot expression: a bare boolean terminal
// counts directly, a `(...)` / `!...` wrapper is offered to the unwrap
// chain, and anything else (a `binary_expression`, whose operator token
// the dispatcher already counted) contributes nothing. The terminal set
// mirrors `java_inspect_container` (issue #372 / lesson #19):
// FieldAccess / CastExpression / ArrayAccess / InstanceofExpression all
// evaluate to a boolean in idiomatic Java condition slots. Mirrors
// `csharp_count_condition` / `groovy_count_condition`.
fn java_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Java::*;
    match condition.kind_id().into() {
        java_bool_terminal_kinds!() => *conditions += 1.,
        ParenthesizedExpression | UnaryExpression => {
            java_inspect_container(condition, parent, conditions);
        }
        _ => {}
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
        java_count_condition(&condition, node, &mut stats.conditions);
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
