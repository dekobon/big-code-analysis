//! `Cognitive` implementation for Java.
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

impl Cognitive for JavaCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Java::*;

        let Nesting {
            conditional: mut nesting,
            function_depth: mut depth,
            mut lambda,
        } = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement
            | EnhancedForStatement
            | WhileStatement
            | DoStatement
            | SwitchBlock
            | CatchClause
            | TernaryExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else /* else-if also */ => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, labeled `break LABEL`
            // and `continue LABEL` each add +1 for breaking the structured
            // control flow. Plain `break;` / `continue;` are not penalized.
            BreakStatement | ContinueStatement
                if node.is_child(Identifier as u16) =>
            {
                increment_by_one(stats);
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            LambdaExpression => {
                lambda += 1;
            }
            // At a (possibly nested) method / constructor boundary, reset
            // structural nesting to zero and bump the function-depth
            // surcharge when this declaration is itself nested inside
            // another — matching Rust and the 9-of-13 sibling
            // families. Without this, a method declared inside a control
            // construct (Java local / member classes) inherited the
            // enclosing nesting and every nested method missed the
            // SonarSource B-nesting amplification (#696).
            MethodDeclaration | ConstructorDeclaration => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[MethodDeclaration, ConstructorDeclaration],
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
