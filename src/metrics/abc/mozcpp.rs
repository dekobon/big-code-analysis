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

impl Abc for MozcppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // Mozilla-fork clone.
        use Mozcpp::*;

        match node.kind_id().into() {
            // `assignment_expression` covers both plain `=` and every
            // compound form (`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`,
            // `^=`, `<<=`, `>>=`); the grammar lifts them all into a
            // single named node so we count once per
            // `assignment_expression`. `update_expression` covers both
            // prefix and postfix `++` / `--`.
            AssignmentExpression | AssignmentExpression2 | UpdateExpression => {
                stats.assignments += 1.;
            }
            // `int x = expr;` parses as a `declaration` carrying an
            // `init_declarator` of the form `declarator = value`. Per
            // Fitzpatrick (1997), every `=` operator increments A; the
            // JS impl already counts `let x = 5;` (and excludes
            // `const`). We follow the literal reading for C++ too and
            // count every `init_declarator` whose body contains an
            // explicit `=` token. `const int x = 5;` is counted along
            // with `int x = 5;` — distinguishing them would diverge
            // from the JS rule's "let counted, const not" mapping
            // because C++ `const` semantics are unlike JS `const` (a
            // C++ `const int x` binding is the canonical "one
            // assignment to initialise" — closer to Rust's
            // non-`mut` `let` than to JS's hoisted reference binding).
            // `int x;` parses as a plain declarator inside the
            // `declaration`, not an `init_declarator`, so this arm
            // never fires for un-initialised declarations. The second
            // `init_declarator` grammar form `int x(5);` / `int x{5};`
            // (paren / brace init) carries no `=` token and stays out
            // — only the `=` operator counts.
            InitDeclarator if node.first_child(|id| id == EQ as u16).is_some() => {
                stats.assignments += 1.;
            }
            // Every call counts (method calls fold in as
            // `call_expression` with a `field_expression` callee). The
            // C++ grammar exposes two aliased `call_expression` ids.
            // `new T(...)` allocations count as a branch — they invoke
            // a constructor, mirroring Java's `New` and C#'s
            // `ObjectCreationExpression` rule.
            CallExpression | CallExpression2 | NewExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`. The C++20 spaceship `<=>` (`LTEQGT`)
            // is a comparison operator and counts once per use.
            // `else` opens an alternative branch path; `case`
            // (non-default) adds one per switch arm; `?` opens a
            // ternary; `try` / `catch` count per Fitzpatrick (and
            // Java's rule). `Try2` is the second token-id alias the
            // C++ grammar emits for `try` (it appears under
            // structured-exception forms).
            //
            // `&&` / `||` are deliberately NOT counted (Fitzpatrick
            // Rule 7 in Figure 3 for C++; the unary-conditional
            // counterpart is Rule 9). See the module-level `Stats`
            // doc-comment for the cross-language policy (issue
            // #395, walker tracked in #403).
            LTEQ | GTEQ | EQEQ | BANGEQ | LTEQGT | Else | Case | QMARK | Try | Try2 | Catch => {
                stats.conditions += 1.;
            }
            // Plain `<` / `>` doubles as template-argument and
            // template-parameter delimiter (`std::vector<int>`,
            // `template <typename T>`). The `binary_expression` parent
            // check disambiguates without inspecting siblings — only
            // comparison uses of `<` / `>` count. Both kind-id aliases
            // (`BinaryExpression`, `BinaryExpression2`) are accepted
            // because the C++ grammar emits the same node under two
            // production-rule paths.
            LT | GT
                if ancestors.parent(node).is_some_and(|p| {
                    matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2)
                }) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 (C++ in Figure 3): each operand of a
            // `&&` / `||` chain is one condition (issue #403).
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots. C++ wraps every
            // `if (...)` / `while (...)` / `do {…} while (...)` /
            // `return value` in a paren / parenthesized expression
            // (return is unparenthesized but its child(1) is the
            // expression). `cpp_inspect_container` handles the
            // `(...)` / `!...` unwrap so `if (true)` and `return !x`
            // each count one condition; bare `return x` reports zero.
            // Use `child_by_field_name("condition")` for if/while so
            // the `if constexpr (cond)` form (where child(1) is the
            // `constexpr` keyword, not the condition_clause) is
            // handled correctly. Return uses positional child(1)
            // — its value field is always at index 1, no optional
            // attribute precedes it.
            IfStatement | WhileStatement => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    cpp_inspect_container(&cond, node, &mut stats.conditions);
                }
            }
            ReturnStatement => {
                cpp_inspect_child(node, 1, &mut stats.conditions);
            }
            // `do { ... } while (cond);` — children: `do`, body,
            // `while`, condition (parenthesized). Condition at child(3).
            DoStatement => {
                cpp_inspect_child(node, 3, &mut stats.conditions);
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
