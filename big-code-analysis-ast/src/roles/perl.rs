//! Perl's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::lang_helpers::perl::perl_and_is_operator;
use crate::role_table;
use crate::{Ancestors, Node, Perl};

/// Whether an expression holds one of the `Comparison` payers among its
/// tokens. tree-sitter-perl names an `operator` field on some binary
/// expressions and none on a comparison, so the row's projection reads
/// the field where there is one — holding whatever operator the node
/// applies — and the first payer child otherwise; the gate asks whether
/// what it finds is a payer.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether an expression holds one of the `ShortCircuit` payers among
/// its tokens, with an `and` counted only where it is the operator and
/// not an auto-quoted key (see `applied_and`).
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    node.children().any(|token| {
        let kind = token.kind_id();
        if kind == Perl::And as u16 {
            perl_and_is_operator(&token, node)
        } else {
            short_circuit::PAYERS.contains(&kind)
        }
    })
}

/// Whether an expression holds the `EagerLogical` payer among its
/// tokens — `xor`, which tree-sitter-perl spells only as an operator.
fn eager_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    eager_logical::project(node).is_some_and(|op| eager_logical::PAYERS.contains(&op.kind_id()))
}

/// Whether a bare `<` / `>` is the operator of a `binary_expression`. A
/// `grammar.json` sweep of tree-sitter-perl 1.1.2 finds the two tokens
/// in exactly three productions — `binary_expression` and the readlines
/// `standard_input_to_identifier` (`<FH>`) and
/// `standard_input_to_variable` (`<$fh>`), each a plain three-token
/// sequence whose brackets are the same bare tokens — so the gate is
/// closed. `<STDIN>` is a single `standard_input` token, which is why
/// #1297's sweep measured Perl at 0 and wrongly cleared it.
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Perl::BinaryExpression as u16)
}

/// Whether an `and` token is the operator of its chain rather than an
/// auto-quoted hash key (`(and => 1)`), which tree-sitter-perl parses
/// as a `unary_expression` whose first child is the token (#1539).
fn applied_and<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| perl_and_is_operator(node, &parent))
}

role_table! {
    lang: Perl;
    Call { pending R08 }
    StringLit { pending R08 }
    // tree-sitter-perl names no `operator` field, so a
    // `binary_expression` owns a comparison when one of the row's
    // payers is among its children and the projection is the lookup.
    // The payers are the numeric `== != < > <= >= <=>`, the string
    // `eq ne lt gt le ge cmp`, and the bindings `=~` / `!~`, which bind
    // a match, a test or a rewrite — the rewrite (`$x =~ s///`) is an
    // edit and no test, a reading the ABC consumer keeps as its own
    // extra (#1540), as it keeps the bare `/re/` match against `$_`
    // (#1467), which has no token at all.
    Comparison {
        owns [BinaryExpression(gated_by comparison_operator)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ, LTEQGT, Eq, Ne, Lt, Gt, Le, Ge, Cmp, EQTILDE, BANGTILDE,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
        ]
        project "operator"
    }
    // `&&` / `||` and the low-precedence `and` / `or`; tree-sitter-perl
    // spells `and` as a two-operand `unary_expression`, so that kind
    // owns a chain too, and an `and` that is an auto-quoted key pays
    // nothing (#1539). The defined-or `//` is `NullSafe`'s and `xor`
    // is `EagerLogical`'s: it does not short-circuit (#1536).
    ShortCircuit {
        owns [
            BinaryExpression(gated_by short_circuit_operator),
            UnaryExpression(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE, Or, And gated_by applied_and]
        project "operator"
    }
    // `xor` evaluates both operands, so it is a value as `^` is, not a
    // chain: no Rule-9 chain increment, and the slot holding one pays
    // once under the ordinary slot rule (#1536). The token is a kind of
    // its own that the grammar spells only as the operator — a hash key
    // `(xor => 1)` or `$h{xor}` is a bareword `identifier` — and Halstead
    // bills it as the operator it is, as before the row.
    EagerLogical {
        owns [BinaryExpression(gated_by eager_operator)]
        pays [Xor]
        project "operator"
    }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "Perl has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "the pinned tree-sitter-perl grammar has no try or catch production; `eval { }` \
         parses as a block and `die` as a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Perl has no modifier keywords; `my` / `our` are declarators" }
    // `sub` and `package` (the keyword kinds; `SUB` / `PACKAGE` are the
    // `__SUB__` / `__PACKAGE__` literals, operands) and the four
    // declarators `my` / `our` / `local` / `state`, each the leaf of an
    // unlisted `*_declaration` / `variable_declaration`.
    DeclKeyword { owns [Sub, Package, My, Our, Local, State] }
    Primitive { absent "Perl has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Perl documents with POD, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PerlCode;
    use crate::test_support::for_each_node_with_chain;

    // The `EagerLogical` row owns an expression exactly when its
    // operator is `xor`, projects to that token, and pays at it — and
    // the chain operators beside it are the `ShortCircuit` row's alone
    // (#1695).
    #[cfg(feature = "perl")]
    #[test]
    fn eager_logical_owns_xor_and_nothing_beside_it() {
        // `xor` binds looser than `=`, so the owner is the assignment.
        let source = b"my $c = $a xor $b; my $d = $a or $b; my %h = (xor => 1);\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<PerlCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if eager_logical::owns(node, source, ancestors) {
                let op = eager_logical::project(node).expect("an owner projects to its token");
                assert_eq!(op.utf8_text(source), Some("xor"), "{}", text());
                owned.push(text());
            }
            if eager_logical::pays_at_leaf(node, source, ancestors) {
                paid.push(text());
            }
            assert!(
                !(eager_logical::owns(node, source, ancestors)
                    && short_circuit::owns(node, source, ancestors)),
                "{}",
                text()
            );
        });
        assert_eq!(owned, ["my $c = $a xor $b"]);
        assert_eq!(paid, ["xor"]);
    }
}
