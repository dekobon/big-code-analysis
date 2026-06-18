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
use crate::*;

impl Abc for BashCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
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
            // Two condition signals share this arm:
            //
            // - Comparison operators inside `[[ … ]]` and `(( … ))`, plus
            //   the prefix test operators `-z`, `-n`, `-eq`, `-lt`, … which
            //   the grammar emits as `Bash::TestOperator`.
            // - Control-flow branches (`if`/`elif`/`while`). A Bash predicate
            //   is a command, so the branch keyword itself is the only
            //   condition signal. These branch keywords mirror the matching
            //   Bash cyclomatic decisions (`if`/`elif`/`while`; not `for`/
            //   `&&`/`||`), lifting ABC off 0 for `if cmd; then … elif … fi`.
            Bash::EQEQ
            | Bash::BANGEQ
            | Bash::LT
            | Bash::GT
            | Bash::LTEQ
            | Bash::GTEQ
            | Bash::EQTILDE
            | Bash::TestOperator
            | Bash::IfStatement
            | Bash::ElifClause
            | Bash::WhileStatement => {
                stats.conditions += 1.;
            }
            // Case arms are conditions too, but counted per-arm like
            // cyclomatic, excluding the bare-`*)` wildcard (the Bash analogue
            // of `default:`) (#696).
            Bash::CaseItem | Bash::CaseItem2
                if !crate::metrics::cyclomatic::bash_case_item_is_bare_wildcard(node, code) =>
            {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}
