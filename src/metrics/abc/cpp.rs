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
pub(super) fn cpp_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // One loop peels `(...)` / `!...` layers while carrying a single
    // boolean-context flag, terminating on a terminal-bool kind. The flag
    // must be readable at every step, so any split would have to thread it
    // back out; the parts have no names a reader would draw. The siblings
    // that also breach carry the same marker; the ones sitting at the limit
    // are baselined instead, so growth still trips the gate (#1143).
    let mut node = *container_node;
    let mut node_kind = node.kind();
    let parent_kind = parent.kind();
    let mut has_boolean_content = matches!(
        parent_kind,
        "binary_expression" | "if_statement" | "while_statement" | "do_statement" | "for_statement"
    ) || (parent_kind == "conditional_expression"
        && parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()));

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
        cpp_inspect_container(&child, node, conditions);
    }
}

// Phase-2B (issues #403 / #1102): a ternary's condition and its two
// branch operands are each a Fitzpatrick Rule 9 unary condition, exactly
// as `java_walk_ternary` already counts them. Without this the C family
// scored `a ? !b : !c` as 1 (the `?` token alone) against Java's 4, and
// `cpp_inspect_container`'s `conditional_expression` boolean-context
// seed was unreachable.
//
// Slots are addressed by grammar FIELD, not by child index: the C-family
// `conditional_expression` marks `consequence` optional to admit the GNU
// elision `a ?: b`, which shifts the alternative from child(4) to
// child(3). String-kind-based like its neighbours so the Mozilla fork's
// differing kind_ids (#732) stay covered by this one implementation.
//
// The condition goes through `cpp_count_condition`, whose top-level
// terminal check is what stops a bare `a ? … : …` scoring zero:
// `cpp_inspect_container` alone only counts *after* unwrapping a
// `(...)` / `!...` layer.
pub(super) fn cpp_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        cpp_count_condition(&condition, node, conditions);
    }
    // Branch operands carry no terminal check: an unnegated branch is
    // type-free and contributes nothing, which is what keeps
    // `(a > 0) ? b : -b` at 2 (the `?` and the `>`).
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            cpp_inspect_container(&branch, node, conditions);
        }
    }
}

// Classifies one condition-slot expression: a bare boolean terminal
// counts directly, anything else is offered to the `(...)` / `!...`
// unwrap chain. Shared by the ternary condition slot and the `for`
// header's condition slot, which are the two places a C-family
// condition arrives *unwrapped* — `if` / `while` / `do` hand
// `cpp_inspect_container` a `condition_clause` or a
// `parenthesized_expression` that supplies the unwrap step itself, so
// they must NOT take the top-level terminal count (it is what keeps
// `(a > 0) ? b : -b` at 2). Mirrors `csharp_count_condition`.
fn cpp_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    if matches!(condition.kind(), cpp_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else {
        cpp_inspect_container(condition, parent, conditions);
    }
}

// Phase-2B (issues #403 / #1276): the `for (init; condition; update)`
// condition slot is a Fitzpatrick Rule 9 unary condition, exactly like
// the `if` / `while` slots the dispatchers already walk. Without this
// the C family scored `for (; a; ) {}` zero where `if (a) {}` scores
// one, and `cpp_inspect_container`'s `for_statement` boolean-context
// seed was unreachable. Comparison-shaped conditions (`i < n`) were
// never affected — the `<` token arm counts those.
//
// The slot is addressed by grammar FIELD. All three header slots are
// optional, so every child index moves with the shape written, and a
// comment inside the header moves them again (#1181).
//
// An empty condition (`for (;;)`) exposes no `condition` field, so it
// counts zero with no special case — see the `Stats` doc comment's
// cross-language empty-`for`-condition policy.
pub(super) fn cpp_walk_for_statement(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        cpp_count_condition(&condition, node, conditions);
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
                cpp_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for CppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic)
        // Exhaustive one-arm-per-grammar-kind dispatch table: the
        // cyclomatic count here is the number of node kinds the C++
        // grammar can hand us, not branching a reader must hold in
        // their head. Every arm is independent and self-describing, and
        // the table only ever grows as the grammar does (#1102 added
        // the ternary arm). Splitting it would scatter one lookup
        // across helpers with no semantic boundary to split on.
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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::{cpp_count_unary_conditions, cpp_inspect_container};
    use crate::traits::ParserTrait;
    use crate::{CppParser, Node};

    // The three `pub(super)` helpers in this file are the shared C-family
    // ABC condition walker: the `CCode`, `ObjcCode`, and `MozcppCode` ABC
    // impls all import and route through them (`use super::cpp::{…}` in
    // c.rs / objc.rs / mozcpp.rs). A regression here silently mis-counts
    // the ABC `C` (conditions) component across four languages at once, so
    // these tests exercise the helpers directly rather than only through
    // the per-language `compute` paths — which also pins behaviour the
    // whole-source integration tests reach only transitively.

    fn parse(src: &str) -> CppParser {
        CppParser::new(
            src.as_bytes().to_vec(),
            std::path::Path::new("seam.cpp"),
            None,
        )
    }

    // `cpp_inspect_container` takes the container's parent explicitly
    // rather than resolving it, because `Node::parent` costs `O(depth)`
    // on the metric walk (#1096). These tests reach their container by
    // search rather than by descent, so the authoritative lookup is what
    // supplies it here.
    fn parent_of<'a>(node: &Node<'a>) -> Node<'a> {
        node.parent()
            .expect("every fixture below places its container under a parent node")
    }

    // First node in pre-order (document order) whose kind name is `kind`.
    fn first_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if n.kind() == kind {
                return Some(n);
            }
            for i in (0..n.child_count()).rev() {
                if let Some(c) = n.child(i) {
                    stack.push(c);
                }
            }
        }
        None
    }

    // `a && b`: `cpp_count_unary_conditions` walks the `binary_expression`
    // and counts each boolean-terminal operand once. `a` and `b` are both
    // `identifier`s (members of `cpp_bool_terminal_kinds!`) and the `&&`
    // token is anonymous, so the count is exactly 2.
    #[test]
    fn count_unary_conditions_counts_each_boolean_operand() {
        let p = parse("int f(int a, int b) { return a && b; }");
        let bin = first_of_kind(p.root(), "binary_expression")
            .expect("`a && b` parses to a binary_expression");
        let mut conditions = 0.;
        cpp_count_unary_conditions(&bin, &mut conditions);
        assert_eq!(conditions, 2.);
    }

    // `if (a)`: the `condition_clause` wraps `( a )`. `cpp_inspect_container`
    // seeds boolean context from the `if_statement` parent, unwraps the
    // parens to the `a` identifier terminal, and counts it once.
    #[test]
    fn inspect_container_counts_parenthesized_condition() {
        let p = parse("void f(int a) { if (a) {} }");
        let cond = first_of_kind(p.root(), "condition_clause")
            .expect("`if (...)` produces a condition_clause");
        let mut conditions = 0.;
        cpp_inspect_container(&cond, &parent_of(&cond), &mut conditions);
        assert_eq!(conditions, 1.);
    }

    // `if (((a)))`: the unwrap loop strips every parenthesis layer and
    // counts the single terminal `a` exactly once — not once per paren.
    #[test]
    fn inspect_container_unwraps_nested_parens_once() {
        let p = parse("void f(int a) { if (((a))) {} }");
        let cond = first_of_kind(p.root(), "condition_clause")
            .expect("`if (...)` produces a condition_clause");
        let mut conditions = 0.;
        cpp_inspect_container(&cond, &parent_of(&cond), &mut conditions);
        assert_eq!(conditions, 1.);
    }

    // `if (!a)`: the leading `!` drives the `is_not` branch, which marks the
    // unwrap chain as boolean content before reaching the `a` terminal, so
    // the negated operand is counted once.
    #[test]
    fn inspect_container_counts_negated_condition() {
        let p = parse("void f(int a) { if (!a) {} }");
        let cond = first_of_kind(p.root(), "condition_clause")
            .expect("`if (...)` produces a condition_clause");
        let mut conditions = 0.;
        cpp_inspect_container(&cond, &parent_of(&cond), &mut conditions);
        assert_eq!(conditions, 1.);
    }

    // `int x = (a);`: the `(a)` parenthesized_expression sits in an
    // initializer, not a condition, so the `has_boolean_content` guard
    // stays false and the unwrapped `a` terminal is NOT counted. This
    // guard branch is awkward to reach through the full `compute` path.
    #[test]
    fn inspect_container_ignores_non_boolean_context() {
        let p = parse("int g(int a) { int x = (a); return x; }");
        let paren = first_of_kind(p.root(), "parenthesized_expression")
            .expect("`(a)` parses to a parenthesized_expression");
        let mut conditions = 0.;
        cpp_inspect_container(&paren, &parent_of(&paren), &mut conditions);
        assert_eq!(conditions, 0.);
    }
}
