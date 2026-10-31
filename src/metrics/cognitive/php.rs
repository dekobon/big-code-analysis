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
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read or a
        // kind the rows do not own: the count is the construct list, not
        // branching a reader must hold, and the effort is the table's vocabulary
        // (#1695, #1697).
        use crate::roles::php::{
            branch, catch, closure, else_if, func_opener, r#loop, null_safe, short_circuit, switch,
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
            // Per SonarSource Cognitive Complexity §B2, `goto label;` is an
            // unstructured jump and adds +1 (matching C++/C#/Go/Perl/Lua
            // goto). PHP has no *labeled* `break`/`continue`; its only
            // non-default jump argument is the numeric level form
            // `break N;` / `continue N;`, which breaks out of N enclosing
            // loops. Those enclosing loops are already counted via nesting,
            // so the numeric form is a structured loop-level exit and adds
            // +0 — only `goto` is genuinely unstructured here.
            GotoStatement => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns, or one
            // the `NullSafe` row owns through PHP's null-coalescing `??`,
            // which short-circuits like `&&` / `||` and the word-form
            // `and` / `or` and so forms boolean sequences alongside them
            // (#1695). The word forms are keyed to their symbols; `??` is
            // an operation of its own (#1530). `xor` (the `EagerLogical`
            // row's) evaluates both operands, so like `^` it is a value,
            // not a sequence (#1536). Which payers share a key is this
            // consumer's reading of the rows; the `NullSafe` row's other
            // owners, the nullsafe accesses, are no `binary_expression`
            // and never reach the key.
            _ if short_circuit::owns_kind(kind, node, code, ancestors)
                || null_safe::owns_kind(kind, node, code, ancestors) =>
            {
                // role-bypass: allow(ShortCircuit, NullSafe) — the sequence key the rows' payers
                // share, not membership
                compute_booleans_keyed(node, stats, |id| match id.into() {
                    AMPAMP | And => Some(AMPAMP as u16),
                    PIPEPIPE | Or => Some(PIPEPIPE as u16),
                    QMARKQMARK => Some(id),
                    _ => None,
                });
            }
            AugmentedAssignmentExpression => {
                // PHP's `??=` is `x = x ?? y` and carries one boolean-
                // sequence decision, parallel to the cyclomatic fix
                // from #231. The token sits inside the augmented-
                // assignment container rather than a `BinaryExpression`,
                // so it needs its own arm (#236). PHP grammar has no
                // `&&=` / `||=`.
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKQMARKEQ));
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
