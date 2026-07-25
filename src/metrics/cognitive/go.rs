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
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Go as G;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            G::IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            G::ForStatement
            | G::ExpressionSwitchStatement
            | G::TypeSwitchStatement
            | G::SelectStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
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
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[G::FunctionDeclaration, G::MethodDeclaration],
                );
            }
            G::FuncLiteral => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
