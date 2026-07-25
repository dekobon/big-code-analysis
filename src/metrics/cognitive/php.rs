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
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Php::*;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // The two-word `else if` form parses as `else_clause →
            // if_statement`; `Self::is_else_if` flags that nested
            // `IfStatement` so it is not counted again against the wrapping
            // `else_clause`'s branch extension and does not inflate nesting
            // for later arms (#529). The one-word `elseif` keyword is a
            // dedicated `ElseIfClause` node handled by the branch-extension
            // arm below.
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | MatchExpression
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
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
            BinaryExpression => {
                // PHP's null-coalescing `??` short-circuits like `&&` /
                // `||` and the word-form `and` / `or` / `xor`, so it
                // forms boolean sequences alongside them. Mirrors the
                // PHP cyclomatic operator set minus the assignment
                // form `??=`, which is not a `BinaryExpression`.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | And | Or | Xor | QMARKQMARK)
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
            AnonymousFunction | ArrowFunction => {
                lambda += 1;
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
            FunctionDefinition | MethodDeclaration => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[FunctionDefinition, MethodDeclaration],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
