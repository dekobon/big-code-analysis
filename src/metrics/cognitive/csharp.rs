//! `Cognitive` implementation for C#.
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

impl Cognitive for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Csharp::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | SwitchExpression
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting);
            }
            // `else` is an anonymous keyword token. Each occurrence carries
            // a flat +1 for the alternative branch (matches Java's `Else`
            // handling).
            Else => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, any `goto` (including
            // `goto label`, `goto case x`, `goto default`) is an unstructured
            // jump and adds +1. C#'s grammar does not allow labeled
            // `break`/`continue` (those forms are syntactically rejected), so
            // the only labeled-jump form to handle here is `goto_statement`.
            GotoStatement => {
                increment_by_one(stats);
            }
            BinaryExpression => {
                // C#'s null-coalescing `??` short-circuits like `&&` /
                // `||` and forms boolean sequences alongside them.
                // Mirrors the C# cyclomatic operator set.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | QMARKQMARK)
                });
            }
            AssignmentExpression => {
                // C#'s compound null-coalescing assignment `??=` is
                // semantically `x = x ?? y` and carries one boolean-
                // sequence decision, parallel to the cyclomatic fix
                // from #231. The operator token sits inside the
                // `assignment_expression` node rather than a
                // `BinaryExpression`, so it needs its own arm (#236).
                // C# grammar does not provide `&&=` or `||=`, so only
                // `??=` matters here.
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKQMARKEQ));
            }
            LambdaExpression | AnonymousMethodExpression => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) function boundary, reset structural
            // nesting to zero and bump the function-depth surcharge when
            // this declaration is itself nested inside another — matching
            // Rust and the 9-of-13 sibling families. C# local
            // functions are the acute case: a local function declared
            // inside an `if` previously inherited `nesting = 1`, inflating
            // every control-flow statement in its body by one. The grammar
            // emits both `local_function_statement` and the aliased
            // `local_function_declaration`; both are boundaries (#696).
            MethodDeclaration
            | ConstructorDeclaration
            | DestructorDeclaration
            | OperatorDeclaration
            | ConversionOperatorDeclaration
            | AccessorDeclaration
            | LocalFunctionStatement
            | LocalFunctionDeclaration => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        MethodDeclaration,
                        ConstructorDeclaration,
                        DestructorDeclaration,
                        OperatorDeclaration,
                        ConversionOperatorDeclaration,
                        AccessorDeclaration,
                        LocalFunctionStatement,
                        LocalFunctionDeclaration,
                    ],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
