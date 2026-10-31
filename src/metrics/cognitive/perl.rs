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
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read or a
        // kind the rows do not own: the count is the construct list, not
        // branching a reader must hold, and the effort is the table's vocabulary
        // (#1695, #1697).
        use crate::roles::perl::{branch, closure, else_if, func_opener, r#loop, ternary};
        use Perl as P;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // Every branch the `Branch` row owns and every loop the
            // `Loop` row owns — the block forms and the postfix forms
            // (`return 1 if $cond;`), whose condition is a real cognitive
            // branch even though the body is one expression. tree-sitter-
            // perl parses `elsif_clause` as a direct child of the
            // `if_statement`, never as a nested `if`, so no `ElseIf` guard
            // is needed here: an `if_statement` always opens a level and
            // the clause carries the flat +1 below (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors)
                || r#loop::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `else` and the `elsif` clause the `ElseIf` row pays at —
            // its leaf, the clause itself — each contribute a flat +1.
            // `increment_by_one`, not `increment_branch_extension`: of
            // the dedicated-clause languages — Bash, Lua, iRules, Ruby
            // and Tcl all reset — Perl is the one that has never reset
            // the boolean sequence at a clause, and the row migration
            // moved the occurrence, not the action (#1697). One arm: the row read
            // is a guard, so the `else` token joins it as a kind compare
            // rather than a pattern.
            _ if kind == P::Else as u16
                || else_if::pays_at_leaf_kind(kind, node, code, ancestors) =>
            {
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
            // The two kinds the `ShortCircuit` and `NullSafe` rows own
            // through their operators — tree-sitter-perl files
            // low-precedence `and` under `unary_expression` (with `not`),
            // while `or` / `xor` are a `binary_expression`; without the
            // second kind every `and` scored nothing here while
            // cyclomatic and ABC counted it (#1530) — and that spell the
            // compound forms the rows exclude as the consumers' own.
            // Unguarded: `compute_perl_booleans` asks the rows of each
            // child and folds nothing for a node holding no payer (an
            // auto-quoted `and` key included, #1539), so a guard here
            // would only scan the children a second time.
            // role-bypass: allow(ShortCircuit, NullSafe) — the rows' owner kinds, read per child
            // inside for the keys their payers share
            P::BinaryExpression | P::UnaryExpression => {
                compute_perl_booleans(node, code, stats);
            }
            // A named function — the `FuncOpener` row's callable owners —
            // resets structural nesting and bumps the function-depth
            // surcharge when nested in another (#696); the stop list is
            // the row's callable kinds.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            // A closure — the `Closure` row — charges the lambda channel
            // and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;

    /// The `elsif` arm is `increment_by_one`, not
    /// `increment_branch_extension`: this module has never reset the
    /// boolean sequence at a clause, and #1697 moved the occurrence onto
    /// the `ElseIf` row without moving the action (#1697). No output
    /// can tell the two apart — a sequence opened in the `if`'s
    /// condition ends with that condition's span before the `elsif` is
    /// visited — so the pin is the state: seed an open sequence, run
    /// `compute` on the clause, and the sequence is still open.
    #[cfg(feature = "perl")]
    #[test]
    fn elsif_adds_one_and_leaves_an_open_boolean_sequence_alone() {
        let source = b"sub f { if ($a) { 1 } elsif ($b) { 2 } }\n";
        let mut seen = 0;
        for_each_node_with_chain::<PerlCode>(source, |node, chain| {
            if node.kind_id() != Perl::ElsifClause as u16 {
                return;
            }
            seen += 1;
            let mut stats = Stats {
                structural: 5,
                boolean_seq: BoolSequence {
                    boolean_op: Some((Perl::AMPAMP as u16, usize::MAX)),
                },
                ..Stats::default()
            };
            let mut map = NestingMap::default();
            PerlCode::compute(node, source, Ancestors::known(chain), &mut stats, &mut map);
            assert_eq!(stats.structural, 6, "one flat increment");
            assert_eq!(
                stats.boolean_seq.boolean_op,
                Some((Perl::AMPAMP as u16, usize::MAX)),
                "the clause does not reset the sequence"
            );
        });
        assert_eq!(seen, 1);
    }
}
