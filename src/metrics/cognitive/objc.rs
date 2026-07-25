//! `Cognitive` implementation for Objective-C.
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

impl Cognitive for ObjcCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Objc::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let Nesting {
            conditional: mut nesting,
            function_depth: mut depth,
            mut lambda,
        } = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `@catch` (the `catch_clause` node) nests like a C++ catch.
            // Fast enumeration folds into `for_statement`, so `ForStatement`
            // already covers it.
            ForStatement
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            GotoStatement | Else /* else-if also */ => {
                increment_by_one(stats);
            }
            // Both `binary_expression` aliases are listed: a node carries
            // exactly one kind_id, so this cannot double-count, and it is
            // robust to which alias the grammar emits for `&&` / `||`
            // (Step 3.5 aliased-variant audit).
            BinaryExpression | BinaryExpression2 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            // ObjC blocks `^{ … }` are the language's closures.
            BlockLiteral => {
                lambda += 1;
            }
            // Functions and `@implementation` methods are definition
            // boundaries: reset structural nesting and bump the
            // function-depth surcharge when nested inside another (#696).
            FunctionDefinition | FunctionDefinition2 | MethodDefinition => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[FunctionDefinition, FunctionDefinition2, MethodDefinition],
                );
            }
            _ => {}
        }
        nesting_map.insert(
            node.id(),
            Nesting {
                conditional: nesting,
                function_depth: depth,
                lambda,
            },
        );
    }
}
