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
use crate::roles::c::{case_arm, comparison, short_circuit, ternary};
use crate::*;

// The row-keyed `ScoresItself` (see the Cpp twin): a
// `binary_expression` the `Comparison` or `ShortCircuit` row owns, or a
// ternary. Neither owner gate reads ancestors, so the peeled descendant
// is asked with none.
fn c_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    rows_own!(expr, code; comparison, short_circuit, ternary)
}

impl Abc for CCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic, halstead)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // C-grammar sibling. The effort is the table's vocabulary.
        use C::*;

        let scores: ScoresItself = &|expr| c_condition_scores_itself(expr, code);
        let kind = node.kind_id();
        match kind.into() {
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
            // `else` opens an alternative branch path; `case`
            // (non-default) adds one per switch arm; `?` opens a
            // ternary. C has no exceptions and no `<=>` spaceship, so —
            // unlike the C++ impl — there are no `try` / `catch` /
            // `LTEQGT` condition arms.
            //
            // `&&` / `||` are deliberately NOT counted (Fitzpatrick
            // Rule 7 in Figure 3; the unary-conditional
            // counterpart is Rule 9). See the module-level `Stats`
            // doc-comment for the cross-language policy (issue
            // #395, walker tracked in #403).
            Else => {
                stats.conditions += 1.;
            }
            // The six comparison tokens, the `Comparison` row's payers
            // (`roles/c.rs`): `<=` `>=` `==` `!=` ungated, since
            // tree-sitter-c 0.24.2 emits them only from a
            // `binary_expression` or a `preproc_binary_expression`
            // aliased onto it; `<` / `>` only under one, so a stranded
            // recovery token pays nothing. Fitzpatrick Rule 5 scores the
            // operator wherever it is written; the slot holding one
            // reads the row's owner so it does not pay again.
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
            // Fitzpatrick Rule 9 (Figure 3): each operand of a `&&` /
            // `||` chain is one condition (issue #403), and the token
            // pays nothing — the `ShortCircuit` row is read at its leaf
            // only to find the chain, which is the token's parent.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_chain_operands(&parent, scores, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots, read by grammar field
            // in `cpp_count_condition_slot`. The slot pays for any
            // predicate no other arm counts (#1526), so `if (true)`,
            // `if (-x)` and `return !x` each count one condition; bare
            // `return x` reports zero.
            IfStatement | WhileStatement | DoStatement => {
                cpp_count_condition_slot(node, scores, &mut stats.conditions);
            }
            ReturnStatement => {
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
