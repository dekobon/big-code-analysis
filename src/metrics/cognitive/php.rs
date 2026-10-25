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
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the grammar can hand us, not branching
        // a reader must hold, and the effort is the table's vocabulary;
        // the row reads (#1695) are arms like the kind arms they replaced.
        use crate::roles::php::{closure, func_opener, null_safe, short_circuit, ternary};
        use Php::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // The two-word `else if` form parses as `else_clause →
            // if_statement`; `Self::is_else_if` flags that nested
            // `IfStatement` so it is not counted again against the wrapping
            // `else_clause`'s branch extension and does not inflate nesting
            // for later arms (#529). The one-word `elseif` keyword is a
            // dedicated `ElseIfClause` node handled by the branch-extension
            // arm below.
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement | ForeachStatement | WhileStatement | DoStatement | SwitchStatement
            | MatchExpression | CatchClause => {
                increase_nesting(stats, &mut nesting);
            }
            // The `conditional_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ElseClause | ElseClause2 | ElseIfClause | ElseIfClause2 => {
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
            _ if closure::owns(node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) named-function / method boundary, reset
            // structural nesting to zero and bump the function-depth
            // surcharge when this definition is itself nested inside another
            // — matching Java and the sibling families. Without this, a PHP
            // function/method declared inside a control construct inherited
            // the enclosing nesting and every nested definition missed the
            // SonarSource B-nesting amplification (#775, the #696 gap).
            // Closures (`AnonymousFunction` / `ArrowFunction`) are handled by
            // the lambda arm above and intentionally do *not* reset nesting,
            // mirroring how the siblings treat lambda vs named-function arms.
            // A named function — the `FuncOpener` row's callable owners —
            // resets structural nesting and bumps the function-depth
            // surcharge when nested in another (#696); the stop list is
            // the row's callable kinds.
            _ if func_opener::is_callable(node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
