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

use super::{Abc, Stats, for_each_named_child, wrapped_operand};
use crate::macros::go_bool_terminal_kinds;
use crate::*;

// One step of the `(...)` / `!` peel: the operand a wrapper wraps, and
// whether the wrapper itself proves that operand boolean. `None` for
// anything this peel does not descend, which is also the answer
// `go_count_condition` asks for, so the slot and the peel cannot
// disagree about which kinds are wrappers (#1470; the Kotlin, Groovy
// and C# instances of that disagreement were #1459, #1466 and #1463).
// Each operand is read by role rather than at child(1) after the `(` or
// the operator token, where a comment may sit (`if ! /*c*/ b`, #1455):
// `unary_expression` names its `operand`, and a parenthesis holds only
// its operand. A `unary_expression` spelled with any other operator
// (`-x`, `^x`, `*p`, `&v`, `<-ch`) wraps no boolean, so the peel stops
// there; `*p` and `<-ch` are boolean leaves (`go_is_bool_operand`).
fn go_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Go as G;

    match node.kind_id().into() {
        G::ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        G::UnaryExpression if node.child(0).is_some_and(|c| c.kind_id() == G::BANG as u16) => {
            node.child_by_field_name("operand").map(|o| (o, true))
        }
        _ => None,
    }
}

// A leaf a boolean slot can hold: the terminal kinds, plus a pointer
// dereference (`*p` of a `*bool`) or a channel receive (`<-ch` of a
// `chan bool`). Those two are `unary_expression`s, so they cannot join
// the kind set, and no other arm counts them, so without this `if *p`
// and a tagless `case <-ch:` scored 0 against a cyclomatic decision of
// 1 (#1526). `-x` and `^x` are never boolean and stay out.
fn go_is_bool_operand(node: &Node) -> bool {
    use Go as G;

    matches!(node.kind_id().into(), go_bool_terminal_kinds!())
        || (node.kind_id() == G::UnaryExpression
            && node
                .child(0)
                .is_some_and(|op| matches!(op.kind_id().into(), G::STAR | G::LTDASH)))
}

// Go ABC unary-conditional walker (issue #403; see `rust_inspect_container`
// for the cross-language rationale). Terminal-bool kinds include calls,
// selector access (`r.Field`), index access (`xs[i]`), and type
// assertions (`x.(*T)`) — every kind whose evaluated value is implicitly
// boolean in idiomatic Go for `if` / `for` conditions.
fn go_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Go as G;

    let mut node = *container_node;
    // `ExpressionCase` reaches here only from a tagless switch, whose
    // case expressions are boolean exactly as an `if` predicate is.
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        G::BinaryExpression | G::IfStatement | G::ForStatement | G::ExpressionCase
    );

    while let Some((operand, proves_boolean)) = go_wrapper_operand(&node) {
        has_boolean_content |= proves_boolean;
        node = operand;

        if go_is_bool_operand(&node) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B (issue #403): condition-slot dispatcher for Go.
fn go_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    if go_is_bool_operand(condition) {
        *conditions += 1.;
    } else if go_wrapper_operand(condition).is_some() {
        // Asking the peel itself which kinds it unwraps, rather than
        // restating the list here (#1470): a restated list that gained a
        // kind the peel lacked would read as covering a shape the peel
        // then dropped (`.claude/rules/grammar-dispatch.md` §7).
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
// It is the first named child that is neither the body — always a
// `block`, the one kind a header can never be — nor a comment.
// `node.child(1)` was the body for a bare `for {}`, and tree-sitter
// counts comments among a node's children, so `for /* n */ a {}`
// shifted it (the #1181 failure the field-addressed siblings no longer
// have).
fn go_walk_for_statement(node: &Node, conditions: &mut f64) {
    use Go as G;
    let Some(header) = node
        .children()
        .find(|child| child.is_named() && !matches!(child.kind_id().into(), G::Comment | G::Block))
    else {
        return;
    };
    let slot = if matches!(header.kind_id().into(), G::ForClause) {
        header.child_by_field_name("condition")
    } else {
        Some(header)
    };
    if let Some(condition) = slot {
        go_count_condition(&condition, node, conditions);
    }
}

// Scores one `expression_case` (#1523, transferring #1453's Ruby rule).
// Both switch shapes contribute the single decision cyclomatic counts per
// case, but they pay for it in different places.
//
// A tagged case (`switch x { case 1: }`) lists values compared with
// `x == 1`: that comparison is written nowhere in the source, so the case
// itself is the condition.
//
// A tagless switch is `switch true`, so each case expression is an
// ordinary boolean evaluated exactly as an `if` predicate, and is scored
// by the same slot `IfStatement` uses. A comparison, chain or negated
// comparison is then paid by the arm that owns its operator; a bare
// `case b:`, `(b)` or `!b` pays through the slot. The former blanket +1
// counted `case x > 5:` twice against its `if x > 5` twin's once.
//
// A case may list several expressions (`case a, b:`): an implicit `||`,
// so each is one operand of that chain, scored as its `if` slot would
// score it. `case a, b:` / `case a, x > 1:` therefore read 2, exactly
// their `if a || b` / `if a || x > 1` analogues, independent of order.
// Cyclomatic scores the case as one decision, so these sit one above
// `conditions == cyclomatic - 1`, as Ruby's `when a, b` does.
//
// A case whose parent is not a switch occurs only under error recovery;
// it keeps the per-case count rather than guessing.
fn go_count_expression_case<'a>(
    case: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    conditions: &mut f64,
) {
    // The tag is the `value` field, not a position: `switch x := f(); {`
    // carries an `initializer` and is still tagless.
    let tagless = ancestors.parent(case).is_some_and(|switch| {
        switch.kind_id() == Go::ExpressionSwitchStatement
            && switch.child_by_field_name("value").is_none()
    });
    if !tagless {
        *conditions += 1.;
        return;
    }
    // A `,` or comment in the list is neither terminal nor wrapper, so the
    // slot scores it 0 without a filter.
    for expr in case
        .child_by_field_name("value")
        .into_iter()
        .flat_map(|list| list.children())
    {
        go_count_condition(&expr, case, conditions);
    }
}

fn go_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Go as G;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            if go_is_bool_operand(&node) && matches!(list_kind, G::BinaryExpression) {
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
        // bca: suppress(halstead, cyclomatic) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the
        // cyclomatic count is the number of node kinds the Go grammar can
        // hand us, and `halstead.effort` counts the distinct enum operands
        // those arms name, neither being reasoning a reader must do.
        // Cyclomatic was already baselined past its limit; the
        // `ExpressionCase` arm (#1523) took effort past its own. Each arm
        // is independent; there is no boundary to split on.
        //
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
            // `binary_expression`, `else`, and each non-default
            // type-switch / select arm all contribute one condition
            // (an expression-switch case is scored below).
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
            | G::TypeCase
            | G::CommunicationCase => {
                stats.conditions += 1.;
            }
            G::ExpressionCase => {
                go_count_expression_case(node, ancestors, &mut stats.conditions);
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
            // `expression_list`, the statement's only operand (not
            // child(1), where `return /*c*/ !x` puts a comment —
            // #1455). Iterate the list's
            // children and route each through `inspect_container`
            // (NOT the terminal-at-top form): `return !x` counts
            // the wrapped Identifier once, while `return x` (bare
            // identifier in the return slot) reports zero
            // conditions. Matches Java's policy in
            // `java_return_without_conditions`. Bare `return`
            // (no values) has no operand.
            G::ReturnStatement => {
                if let Some(expr_list) = wrapped_operand(node) {
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
