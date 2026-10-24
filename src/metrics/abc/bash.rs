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
use crate::lang_helpers::bash::bash_test_eq_count;
use crate::roles::bash::{case_arm, comparison, ternary};
use crate::*;

impl Abc for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        let kind = node.kind_id();
        match kind.into() {
            // Each `variable_assignment` is one assignment regardless of
            // operator (`=`, `+=`, `-=`, …) — counting the parent node
            // avoids double-counting `Bash::EQ`, which is also produced
            // for the `=` inside `[ a = b ]` test expressions.
            Bash::VariableAssignment | Bash::VariableAssignment2 => {
                stats.assignments += 1.;
            }
            // Every command invocation is a branch in the ABC sense
            // (function-call / message-pass). `return` and `exit` builtins
            // are also `Bash::Command` nodes and count here too.
            Bash::Command => {
                stats.branches += 1.;
            }
            // Control-flow branches (`if`/`elif`/`while`). A Bash predicate
            // is a command, so the branch keyword itself is the only
            // condition signal. These branch keywords mirror the matching
            // Bash cyclomatic decisions (`if`/`elif`/`while`; not `for`/
            // `&&`/`||`), lifting ABC off 0 for `if cmd; then … elif … fi`.
            Bash::IfStatement | Bash::ElifClause | Bash::WhileStatement => {
                stats.conditions += 1.;
            }
            // The arithmetic ternary, Bash's only ternary form and the
            // `Ternary` row's owner, read at the wrapper: one condition,
            // matching the C-family `conditional_expression` and the Bash
            // cyclomatic / cognitive arms that count it (#1268).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // The comparison operators, the `Comparison` row's payers
            // (`roles/bash.rs`): the symbols inside `[[ … ]]` and
            // `(( … ))`, `<` / `>` gated to a `binary_expression` since
            // the same tokens spell a redirection (#1280), and the test
            // operators `-z`, `-n`, `-eq`, `-lt`, … which the grammar
            // emits as one `test_operator` kind shared with the `-a` /
            // `-o` connectives — those join two tests the way `&&` /
            // `||` do, and the row's text gate leaves them out (#1536).
            // `&&` / `||` themselves are command-list separators, so
            // Fitzpatrick Rule 9 does not map onto this grammar and ABC
            // reads the `ShortCircuit` row for nothing.
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // `=` compares strings inside `[ … ]` / `[[ … ]]`, exactly as
            // `==` does, and assigns inside `(( … ))` (#1536). Each test
            // counts its own, once, from the top.
            Bash::TestCommand => {
                stats.conditions += bash_test_eq_count(node) as f64;
            }
            // Case arms are conditions too, counted per arm like
            // cyclomatic: each `case_item` the `CaseArm` row owns, which
            // leaves out the bare `*)` wildcard, the Bash analogue of
            // `default:` (#696).
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}
