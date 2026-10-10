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

use super::{
    Abc, Stats, count_boolean_slot, count_each_operand, count_field_operands,
    count_negated_operand, is_operand, wrapped_operand,
};
use crate::metrics::cyclomatic::cpp_operator_is_applied;
use crate::*;

// The C-family slot rule, shared by the `CCode`, `CppCode`, `MozcppCode`
// and `ObjcCode` ABC impls (#720). Every kind is matched by NAME: the
// Mozilla fork assigns different kind_ids to the same kinds (#732,
// mirroring the npa fix in #731), and tree-sitter-cpp emits second ids
// for `binary_expression`, `parenthesized_expression` and
// `unary_expression` that all render to one base name, so a single
// string arm covers each family (grammar-dispatch §1).
//
// One step of the value peel (see `PeelStep`). The `if` / `while` head
// wrappers — C++'s `condition_clause`, whose condition is its `value`
// field after an optional init-statement, and the C grammars'
// `parenthesized_expression` — evaluate to what they hold; so do a
// comma expression (its `right`), a cast (its `value`), a plain `=` (its
// `right`, so `if (y = x > 1)` tests the comparison its own arm already
// counts) and a C++ condition declaration (`if (bool y = x > 1)`, its
// `value`, or `if (bool y{x > 1})`, whose one-element brace list holds
// it). A compound assignment is not peeled: the slot
// tests the updated value, not its right-hand side.
//
// `!` is the one wrapper that proves its operand boolean; `not` is its
// ISO C++ alternative token ([lex.digraph]), which the C++ grammars give
// a kind of its own. tree-sitter-c and tree-sitter-objc have no such
// token — there `not` is an `<iso646.h>` macro the parser sees as an
// identifier — so the extra name is inert for them. The other unary
// operators (`-x`, `~x`, `*p`, `&v`) yield a value rather than wrapping a
// test, so the peel stops on them and the slot pays.
//
// Every operand is read by role, never at child(1): a comment may sit
// there (`if (/*c*/ b)`, #1455).
fn cpp_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    let operator_is = |accept: &[&str]| {
        node.child_by_field_name("operator")
            .is_some_and(|op| accept.contains(&op.kind()))
    };
    let value = match node.kind() {
        "parenthesized_expression" => wrapped_operand(node),
        "condition_clause" | "cast_expression" | "declaration" => node.child_by_field_name("value"),
        "initializer_list" => sole_operand(node),
        "comma_expression" => node.child_by_field_name("right"),
        "assignment_expression" if operator_is(&["="]) => node.child_by_field_name("right"),
        "unary_expression" if operator_is(&["!", "not"]) => {
            return node.child_by_field_name("argument").map(|o| (o, true));
        }
        _ => None,
    };
    value.map(|o| (o, false))
}

// The only operand of a one-element brace list; `None` for any other
// length, which holds no single value to test.
fn sole_operand<'a>(list: &Node<'a>) -> Option<Node<'a>> {
    let mut operands = list.children().filter(is_operand);
    operands.next().filter(|_| operands.next().is_none())
}

// The comparison operators the C-family comparison-token arms count when
// applied (`not_eq` and `<=>` exist in the C++ grammars only).
const CPP_COMPARISONS: &[&str] = &["<", ">", "<=", ">=", "==", "!=", "not_eq", "<=>"];

// Whether an arm of the C-family `compute` impls already charges `expr`
// (already peeled) as a condition: a ternary (its `?`), an applied
// comparison, or an `&&` / `||` chain, whose operands each pay through
// `cpp_count_condition`. A C++ fold over any of those operators is
// charged by its `operator` field token as one application of it (see
// `cpp_count_chain_operands`), so the slot holding it does not pay.
fn cpp_condition_scores_itself(expr: &Node) -> bool {
    let operator_in = |accept: &[&str]| {
        expr.child_by_field_name("operator")
            .is_some_and(|op| accept.contains(&op.kind()))
    };
    match expr.kind() {
        "conditional_expression" => true,
        "binary_expression" | "fold_expression" => {
            operator_in(CPP_COMPARISONS) || operator_in(&["&&", "||", "and", "or"])
        }
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `while` / `do` / `for` condition,
// a ternary condition, an operand of an `&&` / `||` chain (see
// `count_boolean_slot`). C and C++ test any scalar for non-zero, so
// `if (-x)`, `if (*p)`, `if (x + 1)`, `if (this)` and `if (sizeof x)` are
// each a decision, and each scored 0 while the slot paid only for a
// fixed list of terminal kinds (#1526).
fn cpp_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        cpp_wrapper_operand,
        cpp_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call or
// message argument, a ternary branch — scores only when a `!` proves it
// boolean (see `count_negated_operand`), which keeps `(a > 0) ? b : -b`
// at 2 (the `?` and the `>`).
fn cpp_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        cpp_wrapper_operand,
        cpp_condition_scores_itself,
        conditions,
    );
}

// The condition slot of an `if` / `while` / `do` statement, read by the
// grammar's `condition` field, never by index: `if constexpr (cond)`
// puts the keyword at child(1), and a comment shifts every later child
// (`do {} while /*c*/ (b);` — #1455).
pub(super) fn cpp_count_condition_slot(statement: &Node, conditions: &mut f64) {
    if let Some(condition) = statement.child_by_field_name("condition") {
        cpp_count_condition(&condition, conditions);
    }
}

// The value of a `return` statement, which names no field: it is the
// statement's only operand (`return /*c*/ !b;` — #1455). The bare
// `return;` has none.
pub(super) fn cpp_count_return(statement: &Node, conditions: &mut f64) {
    if let Some(value) = wrapped_operand(statement) {
        cpp_count_negated(&value, conditions);
    }
}

// Each operand of a call's `argument_list` or of an Objective-C message
// is a negated operand (`f(!a, !b)`).
pub(super) fn cpp_count_arguments(list: &Node, conditions: &mut f64) {
    count_each_operand(list, cpp_count_negated, conditions);
}

// Fitzpatrick Rule 9 (C++ in Figure 3, #403): each operand of an `&&` /
// `||` chain is a boolean slot. `a && b || c` is a left-nested chain of
// `binary_expression`s, so an operand that is itself a chain is paid by
// its own operator's visit, and operands are read by field so a comment
// beside the operator does not pay.
//
// A C++ fold is the one other parent an applied operator token has, and
// it applies its operator once — cyclomatic counts it once — whatever
// the pack's size. So it scores one application, as the comparison fold
// `(... == a)` scores what `a == b` does (#1533): each written operand is
// a slot, so `(true && ... && a)` scores what `true && a` does, and a
// unary fold's unwritten side is one more operand, so `(... && a)` and
// `(... && (a > 0))` score what `a && a` and `(a > 0) && (a > 0)` do.
//
// Nothing else applies the operator. The C and Objective-C `&&` arms are
// ungated, but their grammars spell `&&` in no other production, so a
// token reparented by error recovery reaches here and, as the gated C++
// arms do, scores nothing.
pub(super) fn cpp_count_chain_operands(chain: &Node, conditions: &mut f64) {
    match chain.kind() {
        "binary_expression" => count_field_operands(chain, cpp_count_condition, conditions),
        "fold_expression" => {
            let mut written = 0;
            for operand in chain.children().filter(is_operand) {
                cpp_count_condition(&operand, conditions);
                written += 1;
            }
            if written == 1 {
                *conditions += 1.;
            }
        }
        _ => {}
    }
}

// Phase-2B (issues #403 / #1102): a ternary's condition is a boolean
// slot and each branch a negated operand, exactly as `java_walk_ternary`
// counts them. Without this the C family scored `a ? !b : !c` as 1 (the
// `?` token alone) against Java's 4.
//
// Slots are addressed by grammar FIELD, not by child index: the C-family
// `conditional_expression` marks `consequence` optional to admit the GNU
// elision `a ?: b`, which shifts the alternative from child(4) to
// child(3).
pub(super) fn cpp_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        cpp_count_condition(&condition, conditions);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            cpp_count_negated(&branch, conditions);
        }
    }
}

// Phase-2B (issues #403 / #1276): the `for (init; condition; update)`
// condition slot, exactly like the `if` / `while` slots. Without this the
// C family scored `for (; a; ) {}` zero where `if (a) {}` scores one.
//
// The slot is addressed by grammar FIELD. All three header slots are
// optional, so every child index moves with the shape written, and a
// comment inside the header moves them again (#1181). An empty condition
// (`for (;;)`) exposes no `condition` field, so it counts zero with no
// special case — see the `Stats` doc comment's cross-language
// empty-`for`-condition policy.
pub(super) fn cpp_walk_for_statement(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        cpp_count_condition(&condition, conditions);
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

        // A requires clause or requires-expression is compile-time and
        // pays no condition, parenthesised or not; cyclomatic and
        // cognitive agree (#1533).
        let opens = matches!(node.kind_id().into(), RequiresClause | RequiresExpression);
        let in_constraint = stats.constraint.covers(node, opens);

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
            // Inside a constraint only the A and B arms above apply: a
            // call is still a branch, as it is in the unparenthesised
            // `requires A<T> && (f<T>())` and in `static_assert`.
            _ if in_constraint => {}
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
            Else | Case | QMARK | Try | Try2 | Catch => {
                stats.conditions += 1.;
            }
            // The seven comparison tokens, counted only when applied —
            // their parent is a `binary_expression`. A `grammar.json`
            // sweep of tree-sitter-cpp 0.23.4 (and the vendored
            // tree-sitter-mozcpp) finds each of them in these
            // productions:
            //
            // - `binary_expression` — a comparison; counts.
            // - `preproc_binary_expression` — `#if A <= B`, legal C++
            //   and a real decision. The grammar aliases it onto
            //   `binary_expression`, so it counts through the same
            //   allowlist.
            // - `operator_name` — `bool operator<=(…)` *declares* the
            //   operator rather than applying it. Each of the seven
            //   overloads has `cyclomatic()` 1; before #1448 only
            //   `<` / `>` were gated, so the other five scored one
            //   condition each (#1420 is the C# instance).
            // - `_fold_operator` / `_binary_fold_operator` (`<` `>` `<=`
            //   `>=` `==` `!=` only) — the parent is `fold_expression`.
            //   A fold applies its comparison, so it counts once per
            //   fold: the token must be the fold's `operator` field. A
            //   binary fold `(0 == ... == a)` spells its one operator
            //   twice, and the field names only one of the two
            //   spellings. The idiomatic `((a == 0) && ...)` keeps its
            //   `==` inside a `binary_expression` and counts there.
            // - `template_argument_list` / `template_parameter_list` /
            //   `system_lib_string` (`<` `>` only) — delimiters
            //   (`std::vector<int>`, `#include <vector>`).
            //
            // Allowlist polarity so a grammar bump adding a production
            // fails closed (`.claude/rules/grammar-dispatch.md` §1). A
            // token reparented under `{ERROR}` by recovery stops
            // counting too, as `<` / `>` always have.
            //
            // The test is `cpp_operator_is_applied`, shared with Mozcpp.
            //
            // `NotEq` is `not_eq`, the ISO alternative token for `!=`. It
            // occurs in exactly the productions `!=` does bar the
            // preprocessor one, so the same allowlist decides it.
            LT | GT | LTEQ | GTEQ | EQEQ | BANGEQ | NotEq | LTEQGT
                if cpp_operator_is_applied(node, ancestors) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 (C++ in Figure 3): each operand of a
            // `&&` / `||` chain is one condition (issue #403). `and` /
            // `or` are the same operators spelled as ISO alternative
            // tokens, which the grammar gives kinds of their own.
            //
            // The same applied-operator gate as cyclomatic: a
            // `constraint_conjunction` scores nothing (a parenthesised
            // constraint never reaches here), and a binary fold
            // `(0 || ... || !a)` spells its one operator twice, so only
            // the `operator` field token walks the fold — the second
            // spelling paid `!a` again.
            AMPAMP | PIPEPIPE | And | Or if cpp_operator_is_applied(node, ancestors) => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_chain_operands(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots, read by grammar field
            // in `cpp_count_condition_slot`. The slot pays for any
            // predicate no other arm counts (#1526), so `if (true)`,
            // `if (-x)` and `return !x` each count one condition; bare
            // `return x` reports zero.
            IfStatement | WhileStatement | DoStatement => {
                cpp_count_condition_slot(node, &mut stats.conditions);
            }
            // `co_return` is a coroutine's `return` (#1547), so its value
            // is read the same way: `co_return !x` counts one. `co_yield`
            // hands back a value without returning, as the JS family's
            // `yield` does, and like it scores no value slot.
            ReturnStatement | CoReturnStatement => {
                cpp_count_return(node, &mut stats.conditions);
            }
            // `f(!a, !b)` — argument list walker. Two aliases —
            // `argument_list` is emitted as ArgumentList or
            // ArgumentList2 depending on production rule path.
            ArgumentList | ArgumentList2 => {
                cpp_count_arguments(node, &mut stats.conditions);
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
    use super::{cpp_count_chain_operands, cpp_count_condition_slot, cpp_count_negated};
    use crate::traits::ParserTrait;
    use crate::{CppParser, Node};

    // The `pub(super)` helpers in this file are the shared C-family ABC
    // condition walker: the `CCode`, `ObjcCode`, and `MozcppCode` ABC
    // impls all import and route through them (`use super::cpp::{…}` in
    // c.rs / objc.rs / mozcpp.rs). A regression here silently mis-counts
    // the ABC `C` (conditions) component across four languages at once, so
    // these tests exercise the helpers directly rather than only through
    // the per-language `compute` paths.

    #[cfg(feature = "cpp")]
    fn parse(src: &str) -> CppParser {
        CppParser::new(
            src.as_bytes().to_vec(),
            std::path::Path::new("seam.cpp"),
            None,
        )
    }

    // First node in pre-order (document order) whose kind name is `kind`.
    #[cfg(feature = "cpp")]
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

    // `a && b` / `a && -b`: each operand of the chain is one boolean
    // slot, whatever kind it is, and the `&&` token itself pays nothing.
    #[cfg(feature = "cpp")]
    #[test]
    fn chain_operands_each_pay_one_slot() {
        for src in [
            "int f(int a, int b) { return a && b; }",
            "int f(int a, int b) { return a && -b; }",
        ] {
            let p = parse(src);
            let bin = first_of_kind(p.root(), "binary_expression")
                .expect("`a && b` parses to a binary_expression");
            let mut conditions = 0.;
            cpp_count_chain_operands(&bin, &mut conditions);
            assert_eq!(conditions, 2., "{src}");
        }
    }

    // `if (a)`, `if (((a)))`, `if (!a)`, `if (-a)`: the slot peels every
    // parenthesis and negation layer and pays once — not once per layer.
    // `if (a > 1)` pays nothing here: the comparison token's own arm owns
    // that decision.
    #[cfg(feature = "cpp")]
    #[test]
    fn condition_slot_pays_once_unless_counted() {
        for (src, expected) in [
            ("void f(int a) { if (a) {} }", 1.),
            ("void f(int a) { if (((a))) {} }", 1.),
            ("void f(int a) { if (!a) {} }", 1.),
            ("void f(int a) { if (-a) {} }", 1.),
            ("void f(int a) { if (a > 1) {} }", 0.),
        ] {
            let p = parse(src);
            let statement =
                first_of_kind(p.root(), "if_statement").expect("the fixture holds an `if`");
            let mut conditions = 0.;
            cpp_count_condition_slot(&statement, &mut conditions);
            assert_eq!(conditions, expected, "{src}");
        }
    }

    // `int x = (a);` / `int x = !a;`: outside a slot only a negation
    // makes its operand a condition.
    #[cfg(feature = "cpp")]
    #[test]
    fn negated_operand_counts_only_behind_a_negation() {
        for (src, kind, expected) in [
            (
                "int g(int a) { int x = (a); return x; }",
                "parenthesized_expression",
                0.,
            ),
            (
                "int g(int a) { int x = !a; return x; }",
                "unary_expression",
                1.,
            ),
        ] {
            let p = parse(src);
            let operand = first_of_kind(p.root(), kind).expect("the fixture holds the operand");
            let mut conditions = 0.;
            cpp_count_negated(&operand, &mut conditions);
            assert_eq!(conditions, expected, "{src}");
        }
    }
}
