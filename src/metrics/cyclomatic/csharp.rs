//! `Cyclomatic` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
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
            //
            // `WhenClause` and `CatchFilterClause` are C#'s two guard
            // spellings, and each is a decision the enclosing construct
            // does not already pay for (#1422): a guarded arm fails two
            // ways — the pattern does not match, or it matches and the
            // guard is false — while contributing one decision, and
            // `catch (E e) when (c)` tests the filter after the type.
            // Rust already counts a match guard — `rust.rs`'s `If` arm
            // catches the guard's own `if` token — so C# was the
            // outlier, not the convention.
            //
            // The clause *nodes*, not the `when` keyword they share:
            // `When` (119) is also a `_reserved_identifier` at this pin,
            // so `int when = 1;` emits it under an `identifier` and the
            // token would score a decision per mention of the variable
            // (`bca dump`, not inferred). One `when_clause` serves both
            // the `switch_expression_arm` and `switch_section` guards;
            // neither kind carries a numeric-suffix alias.
            //
            // A guarded discard (`_ when g => …`) therefore scores two,
            // and that is the intended reading: the guard is already why
            // `csharp_switch_expression_arm_is_bare_discard` keeps the
            // arm out of the `default:` exclusion, so the arm is no
            // longer an unconditional fallthrough and the guard that
            // makes it conditional is its own decision.
            IfStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | CatchClause
            | WhenClause
            | CatchFilterClause
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
