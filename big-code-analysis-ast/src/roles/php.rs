//! Php's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::Php;
use crate::role_table;

role_table! {
    lang: Php;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator,
    // `instanceof` included; its `operator` field says which, so the
    // owner is gated on the field. The ten comparison tokens are the
    // four equalities, `<>` (the second spelling of `!=`), the four
    // orderings and the spaceship `<=>`, plus the `instanceof`
    // keyword, every one ungated: PHP has no generic brackets, and
    // the pinned grammar spells each only as a `binary_expression`'s
    // operator, so a token error recovery strands under an `ERROR`
    // pays as it always did.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [EQEQ, EQEQEQ, BANGEQ, BANGEQEQ, LTGT, LTEQ, GTEQ, LTEQGT, Instanceof, LT, GT]
        project "operator"
    }
    // The punctuation forms `&&` / `||` and the low-precedence keyword
    // forms `and` / `or`, each a token of its own inside a
    // `binary_expression`. `xor` is `EagerLogical`'s: it evaluates both
    // operands (#1536). `??` is `NullSafe`'s.
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
        pays [AMPAMP, PIPEPIPE, And, Or]
        project "operator"
    }
    // `xor` evaluates both operands, so it is a value as `^` is, not a
    // chain: no Rule-9 chain increment, no cyclomatic decision, no
    // boolean sequence, and the slot holding one pays once under the
    // ordinary slot rule (#1536). The token is a kind of its own the
    // grammar spells only as the operator — `[xor => 1]` and `$o->xor`
    // are a `name` — and Halstead bills it as the operator it is, as
    // before the row.
    EagerLogical {
        owns [BinaryExpression(holds_payer)]
        pays [Xor]
        project "operator"
    }
    // The `conditional_expression` owns it and its `?` pays, gated to
    // that owner — `?int` spells the same token as type syntax. The
    // short form `$a ?: $b` is the same node with its `body` field
    // empty, and pays once as the long form does; `??` is `NullSafe`'s.
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK under ConditionalExpression]
        project "operator"
    }
    // Two structural forms, each one decision per token (#436): the
    // null coalescing `a ?? b`, a `binary_expression` owned through its
    // `operator` field, and the nullsafe access `$o?->p` / call
    // `$o?->m()`, whose `?->` sits in the
    // `nullsafe_member_access_expression` (two aliased ids) or
    // `nullsafe_member_call_expression`. Cyclomatic pays each token
    // once per textual operator, across both nullsafe kinds and in a
    // chain; cognitive folds `??` into a boolean sequence (an operation
    // of its own, #1530) and reads `?->` not at all; ABC pays neither —
    // a nullsafe call is a branch as every call is. The compound `??=`
    // is an assignment to ABC, a sequence to cognitive and a decision to
    // cyclomatic (#236): the consumers' own, recorded here.
    NullSafe {
        owns [
            BinaryExpression(holds_payer),
            NullsafeMemberAccessExpression, NullsafeMemberAccessExpression2,
            NullsafeMemberCallExpression,
        ]
        pays [QMARKQMARK, QMARKDASHGT]
        project "operator"
        excludes [QMARKQMARKEQ "the compound `??=`, an assignment ABC pays as one and a decision the others pay as their own (#236)"]
    }
    // A `case_statement` of a `switch` and a `match_conditional_expression`
    // of a `match` each own a decision and pay at the node (the `case`
    // keyword bills as the keyword it is; a match arm has none); the
    // `default_statement` / `match_default_expression` and their
    // keyword are the fallback and pay nothing (lesson 11, #469, #473).
    CaseArm {
        owns [CaseStatement, MatchConditionalExpression]
        excludes [
            DefaultStatement "the fallback arm, no decision (lesson 11, #469)",
            MatchDefaultExpression "the fallback arm of a `match`, no decision (#473)",
            Case "the arm's keyword, billed as such; the arm pays",
            Default "the fallback label's keyword",
        ]
    }
    Guard { absent "PHP has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Each modifier is its keyword leaf, one kind apiece, under an
    // unlisted `*_modifier` wrapper or bare in its declaration. `var`
    // is the `var_modifier` node, the grammar's one spelling of the
    // PHP 4 property keyword. `static` and `const` are one kind each
    // for the modifier and the statement (`static $x`, `const K = 1`),
    // and `global` for the statement alone; all bill once per
    // spelling.
    Modifier {
        owns [Public, Protected, Private, Static, Final, Abstract, Readonly, VarModifier, Const, Global]
    }
    // The declaration keywords, each its leaf under an unlisted
    // `*_declaration`; `fn` opens an arrow function and `declare` a
    // `declare(strict_types=1)` block.
    DeclKeyword { owns [Function, Fn, Class, Interface, Trait, Enum, Namespace, Use, Declare] }
    // PHP bills a primitive as an *operand* — the type name is what a
    // declaration acts on, keyed by its text — and `getter/php.rs`
    // says so where it reads this row. `primitive_type` is the node
    // that carries it: the wrapper is childless for `callable`,
    // `iterable`, `mixed`, `void`, `false` and `true` (the grammar
    // emits no token node for those spellings), so the keyword leaves
    // it does hold (`int`, `bool`, `array`, `object`, `string`,
    // `float`, `null`) are the duplicate, suppressed by the getter
    // under this wrapper (#1293). `bottom_type` (`never`) and
    // `cast_type` (`(int)`) are leaves of their own and bill with it;
    // a cast never emits a keyword node (#1484).
    Primitive { owns [PrimitiveType, BottomType, CastType] }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "PHP documents with PHPDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;
    use crate::{Ancestors, PhpCode};

    // The `EagerLogical` row owns an expression exactly when its
    // operator is `xor`, projects to that token, and pays at it — and
    // the chain operators beside it are the `ShortCircuit` row's alone
    // (#1695).
    #[cfg(feature = "php")]
    #[test]
    fn eager_logical_owns_xor_and_nothing_beside_it() {
        // `xor` binds looser than `=`, so the owner is the assignment.
        let source = b"<?php $c = $a xor $b; $d = $a or $b; $e = [xor => 1]; $o->xor;\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<PhpCode>(source, |node, chain| {
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
        assert_eq!(owned, ["$c = $a xor $b"]);
        assert_eq!(paid, ["xor"]);
    }
}
