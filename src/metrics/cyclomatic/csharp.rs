//! `Cyclomatic` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Csharp::*;

        match node.kind_id().into() {
            // Standard-only: individual switch statement arms. The `case`
            // keyword token is what is matched here; `default:` uses a
            // distinct `Default` token and is correctly excluded.
            Case => {
                stats.cyclomatic += 1.;
            }
            // Standard-only: switch expression arms, except the bare
            // discard arm `_ =>` (and `var _ =>`), which is C#'s analogue
            // of `default:` and must NOT contribute to standard CCN
            // (issue #282 / lesson 11). A guarded discard
            // (`_ when g => …`) still counts because the guard introduces
            // a non-trivial decision, mirroring Rust's `_ if g` rule.
            SwitchExpressionArm if !csharp_switch_expression_arm_is_bare_discard(node) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the switch statement and switch expression
            // containers each collapse to one decision point.
            SwitchStatement | SwitchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            IfStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | CatchClause
            | ConditionalExpression
            | ConditionalAccessExpression
            | AMPAMP
            | PIPEPIPE
            | QMARKQMARK
            | QMARKQMARKEQ => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
