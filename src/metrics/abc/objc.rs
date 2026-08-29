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

use super::cpp::{
    cpp_count_unary_conditions, cpp_inspect_child, cpp_inspect_container, cpp_walk_for_statement,
    cpp_walk_ternary,
};
use super::{Abc, Stats};
use crate::*;

// Objective-C ABC. ObjC is C plus message sends and `@`-directives, so
// the walker is the C/C++ shape (it reuses the grammar-agnostic
// `cpp_inspect_container` / `cpp_inspect_child` / `cpp_count_unary_conditions`
// helpers, which match on node-kind strings) with two ObjC additions:
//   * the `B` (branch) dimension counts `message_expression`
//     (`[obj msg:x]`) alongside C `call_expression`s — a message send is
//     a call. ObjC has no `new` allocator (`[Foo alloc]` is itself a
//     message send), so there is no `NewExpression` arm.
//   * the `C` (condition) dimension counts `@try` (the `@try` keyword)
//     and each `@catch` handler (`catch_clause`), mirroring how the C++
//     impl counts `try` / `catch`. ObjC has no bare `catch` keyword token
//     (it is `@catch` + a `catch_clause` node) and no `<=>` spaceship.
//     `@throw` is not a condition (the C++ impl likewise does not count
//     `throw`).
impl Abc for ObjcCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // Objective-C sibling.
        use Objc::*;

        match node.kind_id().into() {
            AssignmentExpression | UpdateExpression => {
                stats.assignments += 1.;
            }
            InitDeclarator if node.first_child(|id| id == EQ as u16).is_some() => {
                stats.assignments += 1.;
            }
            // Every call counts: C `call_expression` (two aliased ids).
            CallExpression | CallExpression2 => {
                stats.branches += 1.;
            }
            // An ObjC message send `[obj msg:x]` is also a call (B). Its
            // arguments are direct children of `message_expression` (there
            // is no `argument_list` wrapper, unlike a C call), so unary
            // conditions in them (`[obj msg:!x]`, Fitzpatrick Rule 9) are
            // inspected here rather than via the `ArgumentList` arm below.
            MessageExpression => {
                stats.branches += 1.;
                cpp_count_unary_conditions(node, &mut stats.conditions);
            }
            // Comparison operators, `else` / `case` / `?` branch openers,
            // and the `@try` / `@catch` exception conditions. `&&` / `||`
            // are deliberately excluded (Fitzpatrick Rule 7; the
            // unary-conditional counterpart is Rule 9, handled below).
            LTEQ | GTEQ | EQEQ | BANGEQ | Else | Case | QMARK | ATtry | CatchClause => {
                stats.conditions += 1.;
            }
            // Plain `<` / `>` count only in comparison position (parent is
            // a `binary_expression`). ObjC has no templates, but the parent
            // check is kept for parity with the C/C++ impl.
            LT | GT
                if ancestors.parent(node).is_some_and(|p| {
                    matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2)
                }) =>
            {
                stats.conditions += 1.;
            }
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            IfStatement | WhileStatement => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    cpp_inspect_container(&cond, node, &mut stats.conditions);
                }
            }
            ReturnStatement => {
                cpp_inspect_child(node, 1, &mut stats.conditions);
            }
            DoStatement => {
                cpp_inspect_child(node, 3, &mut stats.conditions);
            }
            ArgumentList | ArgumentList2 => {
                cpp_count_unary_conditions(node, &mut stats.conditions);
            }
            // `a ? !b : !c` — the ternary's own `?` token is already
            // counted by the condition arm above; this walks the three
            // operand slots (issue #1102).
            ConditionalExpression => {
                cpp_walk_ternary(node, &mut stats.conditions);
            }
            // `for (init; cond; update)` — the condition slot, read by
            // grammar field (issue #1276). `for (;;)` has no condition
            // field and counts nothing.
            ForStatement => {
                cpp_walk_for_statement(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
