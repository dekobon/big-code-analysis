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
use crate::lang_helpers::bash::{bash_test_connective, bash_test_eq_count};
use crate::*;

impl Abc for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
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
            // Three condition signals share this arm:
            //
            // - Comparison operators inside `[[ … ]]` and `(( … ))`, plus
            //   the test operators `-z`, `-n`, `-eq`, `-lt`, … which the
            //   grammar emits as `Bash::TestOperator`. The `-a` / `-o`
            //   connectives share that kind and are excluded: they join
            //   two tests the way `&&` / `||` do, and neither pays
            //   (#1536).
            // - Control-flow branches (`if`/`elif`/`while`). A Bash predicate
            //   is a command, so the branch keyword itself is the only
            //   condition signal. These branch keywords mirror the matching
            //   Bash cyclomatic decisions (`if`/`elif`/`while`; not `for`/
            //   `&&`/`||`), lifting ABC off 0 for `if cmd; then … elif … fi`.
            // - The arithmetic ternary, Bash's only ternary form, matching
            //   the C-family `ConditionalExpression` and the Bash
            //   cyclomatic / cognitive arms that count it (#1268).
            //   `TernaryExpression2` is listed defensively per
            //   grammar-dispatch §1, as it is in those two siblings.
            Bash::EQEQ
            | Bash::BANGEQ
            | Bash::LTEQ
            | Bash::GTEQ
            | Bash::EQTILDE
            | Bash::IfStatement
            | Bash::ElifClause
            | Bash::WhileStatement
            | Bash::TernaryExpression
            | Bash::TernaryExpression2 => {
                stats.conditions += 1.;
            }
            Bash::TestOperator
                if ancestors
                    .parent(node)
                    .is_none_or(|parent| bash_test_connective(node, &parent, code).is_none()) =>
            {
                stats.conditions += 1.;
            }
            // `=` compares strings inside `[ … ]` / `[[ … ]]`, exactly as
            // `==` does, and assigns inside `(( … ))` (#1536). Each test
            // counts its own, once, from the top.
            Bash::TestCommand => {
                stats.conditions += bash_test_eq_count(node) as f64;
            }
            // `<` and `>` are comparisons only inside a `binary_expression`.
            // The same two tokens spell an I/O redirection (`cmd > out`,
            // `read x < in`), which the grammar parents under
            // `file_redirect` — so ungated they scored every redirect in a
            // script as a condition. This is the Bash instance of the
            // positive-parent gate #1280 applied to Ruby, and the polarity
            // Rust / Go / C / C++ already use. At tree-sitter-bash 0.25.1
            // only `BinaryExpression3` wraps a comparison; the four
            // siblings are listed defensively per grammar-dispatch §1.
            Bash::LT | Bash::GT
                if ancestors.parent(node).is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        Bash::BinaryExpression
                            | Bash::BinaryExpression2
                            | Bash::BinaryExpression3
                            | Bash::BinaryExpression4
                            | Bash::BinaryExpression5
                    )
                }) =>
            {
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
