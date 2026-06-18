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
use crate::macros::cpp_bool_terminal_kinds;
use crate::*;

// C++ ABC unary-conditional walker (Fitzpatrick Rule 9 in Figure 3;
// see `rust_inspect_container` for the cross-language rationale).
// Matches on node-kind NAMES so the helper is correct for every C-family
// grammar: it is shared by the `CppCode` and `MozcppCode` ABC impls (#720),
// and the Mozilla fork assigns different kind_ids to the same kinds (#732,
// mirroring the npa fix in #731). Aliased kinds — `binary_expression`,
// `parenthesized_expression`, `unary_expression` each have a second token-id
// in the C++ grammar (under structured-binding / requires-clause production
// rules) — all render to one base name, so a single string arm covers each
// family: equivalent to the former `Cpp`-enum match for Cpp, and
// grammar-agnostic for Mozcpp.
pub(super) fn cpp_inspect_container(container_node: &Node, conditions: &mut f64) {
    let mut node = *container_node;
    let mut node_kind = node.kind();
    let Some(parent) = node.parent() else { return };
    let parent_kind = parent.kind();
    let mut has_boolean_content = matches!(
        parent_kind,
        "binary_expression" | "if_statement" | "while_statement" | "do_statement" | "for_statement"
    ) || (parent_kind == "conditional_expression"
        && node
            .previous_sibling()
            .is_none_or(|prev| !matches!(prev.kind(), "?" | ":")));

    loop {
        // `condition_clause` is the C++-grammar wrapper around an
        // `if (...)` / `while (...)` head — same `(`, content, `)`
        // shape as `parenthesized_expression`, so it unwraps the
        // same way at child(1). `do { ... } while (...)`'s trailing
        // condition is a plain `parenthesized_expression`.
        let is_parens = matches!(node_kind, "parenthesized_expression" | "condition_clause");
        let is_not =
            node_kind == "unary_expression" && node.child(0).is_some_and(|c| c.kind() == "!");

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind();

        if matches!(node_kind, cpp_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B helpers (issue #403): condition-slot dispatcher for C++.
// `if (cond)` / `while (cond)` / `return value` slots are
// paren-wrapped in C++; `cpp_inspect_container` already handles the
// `(...)` / `!...` unwrap chain and the boolean-context seed from
// the parent kind. No top-level terminal counter is needed because
// the paren wrapper provides the unwrap step.
pub(super) fn cpp_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        cpp_inspect_container(&child, conditions);
    }
}

pub(super) fn cpp_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    let list_kind = list_node.kind();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind();

            if matches!(node_kind, cpp_bool_terminal_kinds!()) && list_kind == "binary_expression" {
                *conditions += 1.;
            } else if node.is_named() {
                cpp_inspect_container(&node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for CppCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Cpp::*;

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
                if node.parent().is_some_and(|p| {
                    matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2)
                }) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 (C++ in Figure 3): each operand of a
            // `&&` / `||` chain is one condition (issue #403).
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = node.parent() {
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
                    cpp_inspect_container(&cond, &mut stats.conditions);
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
            _ => {}
        }
    }
}
