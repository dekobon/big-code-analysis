//! `Cognitive` implementation for PHP.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cognitive for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // One arm per construct the grammar can hand us, each a row read or a
        // kind the rows do not own: the count is the construct list, not
        // branching a reader must hold, and the effort is the table's vocabulary
        // (#1695, #1697).
        use crate::roles::php::{
            branch, catch, closure, comment, else_if, func_opener, r#loop, short_circuit, switch,
            ternary,
        };
        use Php::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if` the `Branch` row owns opens a level — unless the
            // `ElseIf` row recognises it as the `if` of a two-word `else
            // if` (`else_clause → if_statement`), where the `Else` arm
            // below pays the +1 and no level opens, so the nested `if` is
            // not counted again and does not inflate nesting for later
            // arms (#529, #1697). The one-word `elseif` keyword is a
            // dedicated `ElseIfClause` the row also owns, handled by the
            // branch-extension arm below.
            _ if branch::owns_kind(kind, node, code, ancestors)
                && !else_if::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // Every loop, switch and handler the rows own (#1697).
            _ if r#loop::owns_kind(kind, node, code, ancestors)
                || switch::owns_kind(kind, node, code, ancestors)
                || catch::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // The `conditional_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `else` clauses, which no row owns, and the one-word
            // `elseif` clauses the `ElseIf` row pays at (its self-paying
            // owners); the two-word `else if`'s nested `if_statement` is
            // the row's third owner and pays nothing — its enclosing
            // `else_clause` is what reaches this arm.
            ElseClause | ElseClause2 => {
                increment_branch_extension(stats);
            }
            _ if else_if::pays_at_leaf_kind(kind, node, code, ancestors) => {
                increment_branch_extension(stats);
            }
            // Per SonarSource Cognitive Complexity §B1, `goto label;` is an
            // unstructured jump and adds +1 (matching C++/C#/Go/Perl/Lua
            // goto). PHP has no *labeled* `break`/`continue`; its only
            // non-default jump argument is the numeric level form
            // `break N;` / `continue N;`, B1's `break NUMBER` /
            // `continue NUMBER`, which adds +1 whenever an argument is
            // written — `break 1;` included, as sonar-php increments on
            // `tree.argument() != null` (#1765). The grammar admits any
            // expression there — the one named child a jump can hold
            // beside the anonymous keyword and `;`, a comment extra
            // aside — so the gate is a named, non-comment child, not
            // the `integer` kind. A bare `break;` / `continue;` adds +0.
            GotoStatement => {
                increment_by_one(stats);
            }
            BreakStatement | ContinueStatement
                if node
                    .children()
                    .any(|c| c.is_named() && !comment::lists_kind(c.kind_id())) =>
            {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns (#1695).
            // The word forms are keyed to their symbols (#1530); `xor`
            // (the `EagerLogical` row's) evaluates both operands, so
            // like `^` it is a value, not a sequence (#1536). The
            // `NullSafe` row's `??` and the compound `??=` are not read:
            // the paper ignores null coalescing (the module header of
            // `cognitive.rs`, #1800). PHP has no `&&=` / `||=`, so no
            // assignment arm is left.
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                // role-bypass: allow(ShortCircuit) — the sequence key the row's payers
                // share, not membership
                compute_booleans_keyed(node, stats, |id| match id.into() {
                    AMPAMP | And => Some(AMPAMP as u16),
                    PIPEPIPE | Or => Some(PIPEPIPE as u16),
                    _ => None,
                });
            }
            // A closure — the `Closure` row — charges the lambda channel
            // and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // A named function or method — the `FuncOpener` row's callable
            // owners — resets structural nesting and bumps the
            // function-depth surcharge when nested in another, matching
            // Java and the sibling families; without it a declaration
            // inside a control construct inherited the enclosing nesting
            // and a nested one missed the SonarSource B-nesting
            // amplification (#775, the #696 gap). The stop list is the
            // row's callable kinds. Closures (`AnonymousFunction` /
            // `ArrowFunction`) take the lambda arm above and deliberately
            // do *not* reset nesting, as in the siblings.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
