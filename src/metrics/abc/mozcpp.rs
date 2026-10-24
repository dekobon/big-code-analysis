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
    cpp_count_arguments, cpp_count_chain_operands, cpp_count_condition_slot, cpp_count_return,
    cpp_walk_for_statement, cpp_walk_ternary,
};
use super::{Abc, ScoresItself, Stats, rows_own};
use crate::roles::mozcpp::{case_arm, comparison, short_circuit, ternary};
use crate::*;

// The row-keyed `ScoresItself` of the Mozilla fork — the Cpp twin's,
// against this dialect's rows and ids.
fn mozcpp_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    rows_own!(expr, code; comparison, short_circuit, ternary)
}

impl Abc for MozcppCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic, halstead)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // Mozilla-fork clone.
        use Mozcpp::*;

        // A requires clause pays no condition, and the rows are read
        // under its reach — see the Cpp twin (#1533).
        let opens = matches!(node.kind_id().into(), RequiresClause | RequiresExpression);
        let in_constraint = stats.constraint.covers(node, opens);
        let scores: ScoresItself = &|expr| mozcpp_condition_scores_itself(expr, code);

        let kind = node.kind_id();
        match kind.into() {
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
            // The seven comparison tokens count only when applied, through
            // the `Comparison` row; the vendored grammar has the same
            // productions for them as tree-sitter-cpp, and the derivation
            // is on the matching arm of `CppCode::compute`.
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // A ternary's `?`, the `Ternary` row's payer: one decision,
            // paid at the token so `a ? b : c` scores one outside any
            // slot; the owner arm below walks its operand slots.
            _ if ternary::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // A `case` label, the `CaseArm` row's payer — one per label,
            // as cyclomatic counts them; `default` is excluded on the row
            // (lesson 11, #469).
            _ if case_arm::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 (C++ in Figure 3): each operand of a
            // `&&` / `||` chain is one condition (issue #403); the
            // `ShortCircuit` row's gate keeps a `constraint_conjunction` and
            // a binary fold's second spelling out, and the derivation is on
            // the Cpp twin.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_chain_operands(&parent, scores, &mut stats.conditions);
                }
            }
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
            Else | Try | Try2 | Catch => {
                stats.conditions += 1.;
            }
            // Phase-2B (issue #403): condition slots, read by grammar field
            // in `cpp_count_condition_slot`. The slot pays for any
            // predicate no other arm counts (#1526), so `if (true)`,
            // `if (-x)` and `return !x` each count one condition; bare
            // `return x` reports zero.
            IfStatement | WhileStatement | DoStatement => {
                cpp_count_condition_slot(node, scores, &mut stats.conditions);
            }
            // `co_return` is a coroutine's `return` (#1547), so its value
            // is read the same way: `co_return !x` counts one. `co_yield`
            // hands back a value without returning, as the JS family's
            // `yield` does, and like it scores no value slot.
            ReturnStatement | CoReturnStatement => {
                cpp_count_return(node, scores, &mut stats.conditions);
            }
            // `f(!a, !b)` — argument list walker. Two aliases —
            // `argument_list` is emitted as ArgumentList or
            // ArgumentList2 depending on production rule path.
            ArgumentList | ArgumentList2 => {
                cpp_count_arguments(node, scores, &mut stats.conditions);
            }
            // `a ? !b : !c` — the ternary's own `?` token is already
            // paid by the `Ternary` arm above; this walks the three
            // operand slots (issue #1102).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                cpp_walk_ternary(node, scores, &mut stats.conditions);
            }
            // `for (init; cond; update)` — the condition slot, read by
            // grammar field (issue #1276). `for (;;)` has no condition
            // field and counts nothing.
            ForStatement => {
                cpp_walk_for_statement(node, scores, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
