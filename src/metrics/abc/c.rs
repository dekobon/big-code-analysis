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
    cpp_count_unary_conditions, cpp_inspect_slot, cpp_walk_for_statement, cpp_walk_ternary,
};
use super::{Abc, Stats, wrapped_operand};
use crate::*;

impl Abc for CCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // C-grammar sibling.
        use C::*;

        match node.kind_id().into() {
            // `assignment_expression` covers both plain `=` and every
            // compound form (`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`,
            // `^=`, `<<=`, `>>=`); the grammar lifts them all into a
            // single named node so we count once per
            // `assignment_expression`. `update_expression` covers both
            // prefix and postfix `++` / `--`.
            AssignmentExpression | UpdateExpression => {
                stats.assignments += 1.;
            }
            // `int x = expr;` parses as a `declaration` carrying an
            // `init_declarator` of the form `declarator = value`. Per
            // Fitzpatrick (1997), every `=` operator increments A, so we
            // count every `init_declarator` whose body contains an
            // explicit `=` token (`const int x = 5;` counts like
            // `int x = 5;`). `int x;` parses as a plain declarator inside
            // the `declaration`, not an `init_declarator`, so this arm
            // never fires for un-initialised declarations.
            InitDeclarator if node.first_child(|id| id == EQ as u16).is_some() => {
                stats.assignments += 1.;
            }
            // Every call counts. The C grammar exposes two aliased
            // `call_expression` ids. C has no `new` allocations, so
            // (unlike the C++ impl) there is no `NewExpression` branch.
            CallExpression | CallExpression2 => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`. `else` opens an alternative branch
            // path; `case` (non-default) adds one per switch arm; `?`
            // opens a ternary. C has no exceptions and no `<=>`
            // spaceship, so — unlike the C++ impl — there are no
            // `try` / `catch` / `LTEQGT` condition arms.
            //
            // `&&` / `||` are deliberately NOT counted (Fitzpatrick
            // Rule 7 in Figure 3; the unary-conditional
            // counterpart is Rule 9). See the module-level `Stats`
            // doc-comment for the cross-language policy (issue
            // #395, walker tracked in #403).
            //
            // Unlike C++ (#1448), `<=` `>=` `==` `!=` need no parent
            // gate: tree-sitter-c 0.24.2's `grammar.json` emits them
            // only from `binary_expression` and
            // `preproc_binary_expression`, both comparisons, and C has
            // no operator overloading to declare them elsewhere.
            LTEQ | GTEQ | EQEQ | BANGEQ | Else | Case | QMARK => {
                stats.conditions += 1.;
            }
            // Plain `<` / `>` are comparison operators (C has no
            // templates, so there is no template-delimiter ambiguity to
            // resolve). The `binary_expression` parent check ensures only
            // comparison uses count. Both kind-id aliases
            // (`BinaryExpression`, `BinaryExpression2`) are accepted
            // because the grammar emits the node under two
            // production-rule paths.
            LT | GT
                if ancestors.parent(node).is_some_and(|p| {
                    matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2)
                }) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 (Figure 3): each operand of a
            // `&&` / `||` chain is one condition (issue #403).
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots. `if (...)` /
            // `while (...)` / `do {…} while (...)` wrap their condition
            // in a `parenthesized_expression` that
            // `cpp_inspect_container` unwraps, so `if (true)` and
            // `return !x` each count one condition; bare `return x`
            // reports zero. Every slot is read by role, never by index:
            // a comment shifts every later child (`do {} while /*c*/
            // (b);`, `return /*c*/ !b;` — #1455). `return` names no
            // field; its value is its only operand.
            IfStatement | WhileStatement | DoStatement => {
                cpp_inspect_slot(
                    node.child_by_field_name("condition"),
                    node,
                    &mut stats.conditions,
                );
            }
            ReturnStatement => {
                cpp_inspect_slot(wrapped_operand(node), node, &mut stats.conditions);
            }
            // `f(!a, !b)` — argument list walker. Two aliases —
            // `argument_list` is emitted as ArgumentList or
            // ArgumentList2 depending on production rule path.
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
