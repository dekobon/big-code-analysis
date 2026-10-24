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

/// Folds a Perl `binary_expression`'s (or, for low-precedence `and`,
/// `unary_expression`'s) short-circuit operator children into the
/// boolean-sequence counter: the `ShortCircuit` row's payers, read
/// through the row so an auto-quoted `and` key (#1539) is left out by
/// its `applied_and` gate, the `NullSafe` row's defined-or `//`, and
/// the three compound short-circuit assignments (`&&=`, `||=`, `//=`),
/// which the rows exclude as the consumers' own. The grammar exposes
/// each `op=` as a distinct operator token inside the same
/// `binary_expression`, so they fold into the same predicate (issue
/// #249). The word forms `and` / `or` are keyed to their symbols (see
/// [`compute_booleans_keyed`]); `//` and each compound assignment keep
/// keys of their own. Which payers share a key is this consumer's
/// reading of the rows.
fn compute_perl_booleans(node: &Node, code: &[u8], stats: &mut Stats) {
    use crate::roles::perl::{null_safe, short_circuit};
    // Each child's parent is `node`, which is all the row's `applied_and`
    // gate reads.
    let chain = [*node];
    let under_node = Ancestors::known(&chain);
    compute_booleans_by_node(node, node.end_byte(), stats, |child| {
        let kind = child.kind_id();
        // role-bypass: allow(ShortCircuit) — the sequence key the row's payers share, not
        // membership: the rows are read first, and the `op=` keys are the consumers' own
        if short_circuit::pays_at_leaf_kind(kind, child, code, under_node) {
            return Some(match kind.into() {
                Perl::And => Perl::AMPAMP as u16,
                Perl::Or => Perl::PIPEPIPE as u16,
                _ => kind,
            });
        }
        if null_safe::pays_at_leaf_kind(kind, child, code, under_node) {
            return Some(kind);
        }
        matches!(
            kind.into(),
            Perl::AMPAMPEQ | Perl::PIPEPIPEEQ | Perl::SLASHSLASHEQ
        )
        .then_some(kind)
    });
}

impl Cognitive for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the grammar can hand us, not branching
        // a reader must hold, and the effort is the table's vocabulary;
        // the row reads (#1695) are arms like the kind arms they replaced.
        use crate::roles::perl::{null_safe, short_circuit, ternary};
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
            // The `ternary_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns(node, code, ancestors) => {
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
            // A `binary_expression` or `unary_expression` the
            // `ShortCircuit` row owns — tree-sitter-perl files
            // low-precedence `and` under `unary_expression` (with `not`),
            // while `or` / `xor` are a `binary_expression`; without the
            // second kind every `and` scored nothing here while
            // cyclomatic and ABC counted it (#1530), and the row's gate
            // leaves out the `unary_expression` an auto-quoted `and` key
            // (`(and => 1)`) is parsed as (FIXME #1539 upstream) — or one
            // the `NullSafe` row owns through the defined-or `//`, or one
            // carrying the compound forms the same two kinds spell, which
            // the rows exclude as the consumers' own.
            // role-bypass: allow(ShortCircuit, NullSafe) — the `op=` owners, excluded on the rows
            // as the consumers' own; the rows are read on the arm's guard
            P::BinaryExpression | P::UnaryExpression
                if short_circuit::owns(node, code, ancestors)
                    || null_safe::owns(node, code, ancestors)
                    || node.children().any(|c| {
                        matches!(
                            c.kind_id().into(),
                            P::AMPAMPEQ | P::PIPEPIPEEQ | P::SLASHSLASHEQ
                        )
                    }) =>
            {
                compute_perl_booleans(node, code, stats);
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
