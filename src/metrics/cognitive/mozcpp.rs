//! `Cognitive` implementation for mozcpp.
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

impl Cognitive for MozcppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Mozcpp::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);
        // A requires clause scores nothing — see the Cpp twin (#1533).
        let opens = matches!(node.kind_id().into(), RequiresClause | RequiresExpression);
        let in_constraint = stats.constraint.covers(node, opens);

        match node.kind_id().into() {
            _ if in_constraint => {}
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement
            | ForRangeLoop
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both.
            GotoStatement | Else => {
                increment_by_one(stats);
            }
            // `and` / `or` keyed to `&&` / `||`, and a fold one sequence —
            // see the Cpp twin (#1522, #1533).
            BinaryExpression2 | FoldExpression => {
                compute_booleans_keyed(node, stats, |id| match id.into() {
                    AMPAMP | And => Some(AMPAMP as u16),
                    PIPEPIPE | Or => Some(PIPEPIPE as u16),
                    _ => None,
                });
            }
            LambdaExpression => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) function-definition boundary, reset
            // structural nesting to zero and bump the function-depth
            // surcharge when this definition is itself nested inside
            // another — matching Rust and the 9-of-13 sibling
            // families. Without this, a method defined inside a control
            // construct inherited the enclosing nesting and every nested
            // definition missed the SonarSource B-nesting amplification
            // (#696).
            FunctionDefinition | FunctionDefinition2 | FunctionDefinition3
            | FunctionDefinition4 => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        FunctionDefinition,
                        FunctionDefinition2,
                        FunctionDefinition3,
                        FunctionDefinition4,
                    ],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
