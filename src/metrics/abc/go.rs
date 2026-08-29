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

use super::{Abc, Stats, for_each_named_child};
use crate::macros::go_bool_terminal_kinds;
use crate::*;

// Go ABC unary-conditional walker (issue #403; see `rust_inspect_container`
// for the cross-language rationale). Terminal-bool kinds include calls,
// selector access (`r.Field`), index access (`xs[i]`), and type
// assertions (`x.(*T)`) — every kind whose evaluated value is implicitly
// boolean in idiomatic Go for `if` / `for` conditions.
fn go_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Go as G;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        G::BinaryExpression | G::IfStatement | G::ForStatement
    );

    loop {
        let is_parens = matches!(node_kind, G::ParenthesizedExpression);
        let is_not = matches!(node_kind, G::UnaryExpression)
            && node.child(0).is_some_and(|c| c.kind_id() == G::BANG as u16);

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, go_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B (issue #403): condition-slot dispatcher for Go.
fn go_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Go as G;
    let kind = condition.kind_id().into();
    if matches!(kind, go_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, G::ParenthesizedExpression | G::UnaryExpression) {
        go_inspect_container(condition, parent, conditions);
    }
}

// Resolves and counts the loop condition of Go's one loop keyword,
// which carries four unrelated shapes under `for_statement`:
//   `for cond {}`          → the header slot is the condition itself
//   `for init; cond; post` → the slot is a `for_clause` whose
//                            `condition` field holds it
//   `for range xs {}`      → the slot is a `range_clause`, no condition
//   `for {}`               → no header slot; only the body
//
// The three-clause form was the gap (#1276): letting the `for_clause`
// fall through to `go_count_condition` — which filters non-terminal /
// non-paren / non-unary kinds — scored `for i := 0; a; i++ {}` zero
// while `for a {}` scored one. A `range_clause` is rejected the same
// way, and a `for_clause` with no `condition` field contributes
// nothing; see the `Stats` doc comment's cross-language
// empty-`for`-condition policy.
//
// Go is the one grammar here whose `for_statement` exposes no
// `condition` field, so the header slot has to be located structurally.
// It is the first named child that is neither the body (which *does*
// have a field) nor a comment — `node.child(1)` would be the body for
// a bare `for {}`, and tree-sitter counts comments among a node's
// children, so `for /* n */ a {}` shifted it (the #1181 failure the
// field-addressed siblings no longer have).
fn go_walk_for_statement(node: &Node, conditions: &mut f64) {
    use Go as G;
    let body_id = node.child_by_field_name("body").map(|body| body.id());
    let Some(header) = node.children().find(|child| {
        child.is_named()
            && !matches!(child.kind_id().into(), G::Comment)
            && Some(child.id()) != body_id
    }) else {
        return;
    };
    let slot = if matches!(header.kind_id().into(), G::ForClause) {
        header.child_by_field_name("condition")
    } else {
        Some(header)
    };
    let Some(condition) = slot else { return };
    go_count_condition(&condition, node, conditions);
}

fn go_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Go as G;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, go_bool_terminal_kinds!())
                && matches!(list_kind, G::BinaryExpression)
            {
                *conditions += 1.;
            } else if node.is_named() {
                go_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides
        // with the bare enum name in pattern position under
        // `use Go::*;` (same workaround as in cyclomatic / cognitive).
        use Go as G;

        match node.kind_id().into() {
            // Plain `=`, augmented `+=`, `-=`, … all parse as
            // `assignment_statement`. `:=` is a short variable
            // declaration. `x++` / `x--` rebind too.
            G::AssignmentStatement | G::ShortVarDeclaration | G::IncStatement | G::DecStatement => {
                stats.assignments += 1.;
            }
            // `var x = 5` and `x := 5` are the same binding spelled two
            // ways, and the cross-language policy counts an initialized
            // declaration: Java the `EQ` in `int x = 5;`, C++ an
            // `InitDeclarator` carrying `EQ`, Rust a `LetDeclaration` with
            // a `value` field, JS a `let` / `var` initializer (#1278). The
            // `value` field is what distinguishes `var z int` (no
            // initializer, no assignment) position-independently across
            // both `var x = 5` and `var y int = 6`. Matching the spec
            // rather than the declaration counts each line of a grouped
            // `var ( a = 1 \n b int )` block on its own; a multi-name
            // `var p, q = 1, 2` is one spec and so scores 1, matching
            // `p, q := 1, 2`. `const` stays excluded, as everywhere else.
            G::VarSpec if node.child_by_field_name("value").is_some() => {
                stats.assignments += 1.;
            }
            // Every call expression — including method calls
            // (`r.Method()` parses as `call_expression` whose callee is
            // a `selector_expression`) — contributes one branch.
            // Composite literals (`Point{X: 1}`) are NOT calls.
            G::CallExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`, `else`, and each non-default switch
            // / type-switch / select arm all contribute one condition.
            // `<` / `>` double as type-argument delimiters in generic
            // instantiations (`f[T any]`, `List[int]`); the
            // `BinaryExpression` parent guard filters those out
            // without inspecting siblings. `default_case` is
            // intentionally excluded — like Java / C# `default:`, it
            // does not introduce a new decision point.
            G::EQEQ
            | G::BANGEQ
            | G::LTEQ
            | G::GTEQ
            | G::Else
            | G::ExpressionCase
            | G::TypeCase
            | G::CommunicationCase => {
                stats.conditions += 1.;
            }
            G::LT | G::GT
                if ancestors
                    .parent(node)
                    .is_some_and(|p| matches!(p.kind_id().into(), G::BinaryExpression)) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 7: each operand of a `&&` / `||` chain is
            // one condition (issue #403). The walker iterates immediate
            // children of the parent `binary_expression`.
            G::AMPAMP | G::PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    go_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): Rule 6 / 7 condition slots.
            // `if true {}` / `if !a {}` / `if (a) {}` count once.
            // Use `child_by_field_name("condition")` so the
            // `if x := f(); x { ... }` init-statement form is
            // handled correctly — its `condition` field is at
            // child(2) (not child(1), which is the init slot).
            G::IfStatement => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    go_count_condition(&cond, node, &mut stats.conditions);
                }
            }
            // Phase-2B follow-up (findings.md #1): Go's `for` is its
            // only loop with a bare-condition slot. Children:
            //   `for cond {}`           → child(1) = condition
            //   `for init; cond; post`  → child(1) = `for_clause`
            //   `for range items {}`    → child(1) = `range_clause`
            //
            G::ForStatement => {
                go_walk_for_statement(node, &mut stats.conditions);
            }
            // `return value` — Go wraps the return values in an
            // `expression_list` at child(1). Iterate the list's
            // children and route each through `inspect_container`
            // (NOT the terminal-at-top form): `return !x` counts
            // the wrapped Identifier once, while `return x` (bare
            // identifier in the return slot) reports zero
            // conditions. Matches Java's policy in
            // `java_return_without_conditions`. Bare `return`
            // (no values) has no child(1).
            G::ReturnStatement => {
                if let Some(expr_list) = node.child(1) {
                    for_each_named_child(&expr_list, &mut stats.conditions, go_inspect_container);
                }
            }
            // Method-argument-list walker for `f(!a, !b)`. Two
            // aliases — `argument_list` is emitted as ArgumentList
            // or ArgumentList2 depending on production rule path.
            G::ArgumentList | G::ArgumentList2 => {
                go_count_unary_conditions(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
