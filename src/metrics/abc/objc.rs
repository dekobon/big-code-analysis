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
use crate::roles::objc::{case_arm, comparison, short_circuit, ternary};
use crate::*;

// The row-keyed `ScoresItself` (see the Cpp twin): a
// `binary_expression` the `Comparison` or `ShortCircuit` row owns, or a
// ternary. Neither owner gate reads ancestors, so the peeled descendant
// is asked with none.
fn objc_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    rows_own!(expr, code; comparison, short_circuit, ternary)
}

// Objective-C ABC. ObjC is C plus message sends and `@`-directives, so
// the walker is the C/C++ shape (it reuses the grammar-agnostic C-family
// slot helpers in `cpp.rs`, which match on node-kind strings) with two
// ObjC additions:
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
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic, halstead)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`, of which this is the
        // Objective-C sibling. The effort is the table's vocabulary plus
        // the scorer every shared helper now takes (#1694).
        use Objc::*;

        let scores: ScoresItself = &|expr| objc_condition_scores_itself(expr, code);
        let kind = node.kind_id();
        match kind.into() {
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
                cpp_count_arguments(node, scores, &mut stats.conditions);
            }
            // `else` / `case` / `?` branch openers and the `@try` /
            // `@catch` exception conditions. `&&` / `||` are deliberately
            // excluded (Fitzpatrick Rule 7; the unary-conditional
            // counterpart is Rule 9, handled below).
            Else | ATtry | CatchClause => {
                stats.conditions += 1.;
            }
            // The six comparison tokens, the `Comparison` row's payers
            // (`roles/objc.rs`): `<=` `>=` `==` `!=` ungated, since
            // tree-sitter-objc 3.0.2 emits them only from a
            // `binary_expression` or a `preproc_binary_expression`; `<` /
            // `>` only under one — a `grammar.json` sweep finds the bare
            // tokens in `generic_specifier`, `protocol_reference_list`
            // (`id<NSCopying>`), `parameterized_arguments` and
            // `argument_list` too, every one of which the gate excludes.
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
            // Fitzpatrick Rule 9: each operand of a `&&` / `||` chain is
            // one condition; the `ShortCircuit` row is read at its leaf
            // only to find the chain, the token's parent.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(parent) = ancestors.parent(node) {
                    cpp_count_chain_operands(&parent, scores, &mut stats.conditions);
                }
            }
            // By role, never by index — see `impl Abc for CppCode` (#1455).
            IfStatement | WhileStatement | DoStatement => {
                cpp_count_condition_slot(node, scores, &mut stats.conditions);
            }
            ReturnStatement => {
                cpp_count_return(node, scores, &mut stats.conditions);
            }
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
