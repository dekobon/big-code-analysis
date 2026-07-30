//! `Cognitive` implementation for Go.
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

impl Cognitive for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Go as G;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            G::IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            G::ForStatement
            | G::ExpressionSwitchStatement
            | G::TypeSwitchStatement
            | G::SelectStatement => {
                increase_nesting(stats, &mut nesting);
            }
            G::Else | G::GotoStatement => {
                increment_by_one(stats);
            }
            G::BreakStatement | G::ContinueStatement if node.is_child(G::LabelName as u16) => {
                increment_by_one(stats);
            }
            G::BinaryExpression => {
                compute_booleans(node, stats, G::AMPAMP, G::PIPEPIPE);
            }
            G::FunctionDeclaration | G::MethodDeclaration => {
                nesting.conditional = 0;
                increment_function_depth(
                    &mut nesting.function_depth,
                    node,
                    ancestors,
                    &[G::FunctionDeclaration, G::MethodDeclaration],
                );
            }
            G::FuncLiteral => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
