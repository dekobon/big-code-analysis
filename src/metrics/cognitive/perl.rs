//! `Cognitive` implementation for Perl.
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

/// Folds a Perl `binary_expression`'s short-circuit operator children
/// into the boolean-sequence counter — Perl has five bare forms (`&&`,
/// `||`, `//`, `and`, `or`) plus three compound short-circuit
/// assignments (`&&=`, `||=`, `//=`). The grammar exposes each `op=`
/// as a distinct operator token inside the same `binary_expression`,
/// so they fold into the same predicate (issue #249).
fn compute_perl_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Perl::AMPAMP
                | Perl::PIPEPIPE
                | Perl::SLASHSLASH
                | Perl::And
                | Perl::Or
                | Perl::AMPAMPEQ
                | Perl::PIPEPIPEEQ
                | Perl::SLASHSLASHEQ
        )
    });
}

impl Cognitive for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Perl as P;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // tree-sitter-perl parses `elsif_clause` as a direct child of
            // the surrounding `if_statement` (not as a nested `if`), so the
            // `IfStatement` arm here always increases nesting and the
            // `Else | ElsifClause` arm below carries the flat +1.
            P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::TernaryExpression
            // Postfix conditional / loop forms (`return 1 if $cond;`) — the
            // condition is a real cognitive branch and contributes nesting
            // even though the body is a single expression.
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement => {
                increase_nesting(stats, &mut nesting);
            }
            // `else` and `elsif` each contribute a flat +1.
            P::Else | P::ElsifClause => {
                increment_by_one(stats);
            }
            // SonarSource §B2: `goto` is a non-local jump and adds +1.
            // `goto LABEL;` parses as `goto_expression` wrapping the
            // anonymous `goto` keyword token; the walker visits both, so
            // matching only the `GotoExpression` statement node counts the
            // jump once (matching `P::Goto` too would double-count — #450).
            P::GotoExpression => {
                increment_by_one(stats);
            }
            // SonarSource §B2: labeled `last LABEL` / `next LABEL` /
            // `redo LABEL` each add +1 for breaking structured control
            // flow; bare `last;` / `next;` / `redo;` are +0. The jump
            // target is carried as an `Identifier` child of
            // `loop_control_statement` (`Label` is the loop-*definition*
            // node `OUTER:`, never the target — gating on it was a dead
            // arm, #450).
            P::LoopControlStatement if node.is_child(P::Identifier as u16) => {
                increment_by_one(stats);
            }
            P::BinaryExpression => {
                compute_perl_booleans(node, stats);
            }
            P::FunctionDefinition | P::FunctionDefinitionWithoutSub => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[P::FunctionDefinition, P::FunctionDefinitionWithoutSub],
                );
            }
            P::AnonymousFunction => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
