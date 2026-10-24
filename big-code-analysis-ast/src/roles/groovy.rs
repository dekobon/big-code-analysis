//! Groovy's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::getter::node_text;
use crate::role_table;
use crate::{Ancestors, Groovy, Node};

/// Whether a contextual keyword leaf (`trait`, `record`, `sealed`,
/// `permits`, `var`) sits directly under the construct it introduces.
///
/// Valid Groovy may use any of the five as a name. Inside a class the
/// grammar parses that name as an `identifier`, but a script-level
/// statement that opens with one (`record = 3`, `String record`,
/// `void record() {}`, `sealed.call()`) fails into an `ERROR` holding
/// the keyword leaf, and so does a statement opening with `var` as a
/// name anywhere (`var = 4`, `var++`). Billing the leaf there would make
/// a variable an operator (#1552, #1565, #1704). The pinned grammar also
/// misreads a *nested* `sealed class` as a field typed `sealed`, so that
/// `sealed` bills as a type-name operand.
fn contextual_keyword_owns<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Groovy::*;

    ancestors.parent(node).is_some_and(|parent| {
        matches!(
            (node.kind_id().into(), parent.kind_id().into()),
            (Trait, TraitDeclaration)
                | (Record, RecordDeclaration)
                | (Permits, PermitsClause)
                | (
                    Sealed,
                    ClassDeclaration | InterfaceDeclaration | TraitDeclaration
                )
                | (Var, LocalVariableDeclaration)
        )
    })
}

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers; the one kind carries every Java-style binary
/// operator.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_expression`'s `operator` field is `&&` or `||`.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

/// Whether a bare `<` / `>` is the operator of a `binary_expression`.
/// The grammar emits the tokens from four productions —
/// `binary_expression`, `type_arguments`, `type_parameters` and, unlike
/// tree-sitter-java, a separate `method_type_parameters` for
/// `def <U> U m(U x)` — and error recovery strands either (#1274).
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Groovy::BinaryExpression as u16)
}

/// Whether a `?` is a ternary's rather than a wildcard bound's
/// (`List<? extends T>`), the one other production the dekobon Groovy grammar
/// spells the token in (#1274).
fn under_ternary<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Groovy::TernaryExpression as u16)
}

/// Whether an `in` keyword is a membership test rather than the `in`
/// of a `for (x in xs)` loop header, where it is a loop keyword.
fn under_membership_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Groovy::MembershipExpression as u16)
}

/// Whether a `membership_expression` or `instanceof_expression` spells
/// its keyword as a token. The negated forms `a !in b` and
/// `o !instanceof T` emit no operator token at all — the node holds its
/// two operands and nothing between them — so the owner pays for
/// itself there and the positive form's keyword pays otherwise.
fn has_no_keyword_token<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_none()
}

/// Java's eight primitive type keywords and `void`, which Groovy
/// reserves as Java does.
const PRIMITIVE_TYPES: &[&str] = &[
    "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
];

/// Whether `node` spells a primitive type, which bills as a text-keyed
/// operator as Java's primitive keywords do (#1568).
///
/// The pinned grammar has no primitive node: a declared type is a
/// `type_identifier` (`int x = 1`, `void m()`), and a class literal's
/// receiver (`int.class`) or an uninitialised local (`int t`, which
/// parses as two expression statements) is a plain `identifier`. Groovy
/// reserves these keywords, so a name can spell one only as a member
/// selector after a `.`-family operator (`x.int`, `x?.int`, `x::int`) or
/// as a map or named-argument label (`[int: 1]`, `f(int: 1)`); those
/// stay operands.
fn spells_primitive_type<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    use Groovy::*;

    if !node_text(code, node).is_some_and(|text| PRIMITIVE_TYPES.contains(&text)) {
        return false;
    }
    ancestors.parent(node).is_none_or(|parent| {
        let is_first_child = parent.child(0).is_some_and(|first| first.id() == node.id());
        match parent.kind_id().into() {
            FieldAccess
            | SafeNavigationExpression
            | SafeChainDotExpression
            | SpreadDotExpression
            | DirectFieldAccessExpression
            | MethodPointerExpression
            | MethodReferenceExpression => is_first_child,
            MapEntry | NamedArgument => !is_first_child,
            _ => true,
        }
    })
}

/// Whether a `static` qualifies a declaration rather than `import
/// static`, whose `static` bills nothing as `import` does not; Java
/// makes the same call, and the reasoning (#1600) is on its
/// `not_in_a_directive`.
fn not_in_an_import<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent_has_kind(node, Groovy::ImportDeclaration as u16)
}

role_table! {
    lang: Groovy;
    Call { pending R08 }
    StringLit { pending R08 }
    // The worked example of a node / token pairing (#1695). A
    // `binary_expression` owns a comparison through its `operator`
    // field, where `<` / `>` are gated to that parent (the grammar
    // also spells them as generic brackets). The dekobon grammar gives
    // Groovy's own comparisons nodes of their own, each wrapping its
    // token: `spaceship_expression` / `<=>`, `identity_expression` /
    // `===` and `!==`, `regex_find_expression` / `=~`,
    // `regex_match_expression` / `==~`, `membership_expression` /
    // `in` (gated: the same keyword opens a `for (x in xs)` header) and
    // `instanceof_expression` / `instanceof`. The negated forms `!in`
    // and `!instanceof` emit no token at all, so those two owners pay
    // for themselves exactly when they hold no keyword, and once per
    // test either way.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            SpaceshipExpression, IdentityExpression, RegexFindExpression, RegexMatchExpression,
            MembershipExpression(pays_itself_when has_no_keyword_token),
            InstanceofExpression(pays_itself_when has_no_keyword_token),
        ]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
            LTEQGT, EQEQEQ, BANGEQEQ, EQTILDE, EQEQTILDE,
            In gated_by under_membership_expression, Instanceof,
        ]
        project "operator"
    }
    // `&&` / `||`, which the grammar emits only under a
    // `binary_expression`. The elvis `?:` is `NullSafe`'s.
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    // `&` / `|` / `^` on boolean operands evaluate both sides, but the
    // grammar emits them as the same `binary_expression` as the integral
    // forms, so the syntax alone cannot tell them apart; no consumer
    // treats them as a decision, and the three tokens bill as the
    // operators they are. A recognition boundary, recorded as the
    // kinds considered rather than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }
    // The `ternary_expression` owns it and its `?` pays, gated to that
    // owner: the grammar also spells `?` as a `wildcard` bound, which is
    // type syntax (#1274). The elvis `?:` is its own token under its own
    // `elvis_expression`, and is `NullSafe`'s.
    Ternary {
        owns [TernaryExpression]
        pays [QMARK gated_by under_ternary]
        project "operator"
    }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Every modifier (#1565). The grammar has no `modifiers` wrapper —
    // each modifier leaf sits directly under its declaration — so each
    // bills once. `sealed` and `permits` are contextual and bill only
    // under the construct they introduce; `non-sealed` cannot be spelled
    // as a name (`non-sealed` as a subtraction parses as `non`, `-` and
    // an identifier `sealed`). `default` is not here: it is one kind for
    // a `switch` label and an interface method's modifier, and bills
    // with the control keywords.
    Modifier {
        owns [
            Public, Private, Protected, Static(gated_by not_in_an_import), Abstract, Final,
            Native, Strictfp, Synchronized, Transient, Volatile, NonDASHsealed,
            Sealed(gated_by contextual_keyword_owns), Permits(gated_by contextual_keyword_owns),
        ]
    }
    // The type-declaration keywords (#1552): no `*Declaration` wrapper
    // is classified, so the keyword leaf is the only node that can bill
    // one. Unlike Java's, `class` needs no class-literal gate:
    // `String.class` parses as a `field_access` whose `class` is a plain
    // `identifier`. `trait` and `record` are contextual, as `sealed` is.
    // `def` declares a method, a field or a local, as Kotlin's `fun` /
    // `val` and Perl's `my` do; it is one kind in every position. `var`
    // is the same kind of leaf under a `local_variable_declaration`, as
    // Kotlin's `var` is, and bills with it (#1704) — gated, as `trait`
    // and `record` are, because a statement that opens with `var` as a
    // name (`var = 4`) fails into an `ERROR` holding the same leaf. Only
    // that leaf: a `var` used as a name elsewhere (`def var = 3`,
    // `[var: 1]`) is an `identifier`, and a field's `var field = 1` or
    // a `final var q = 5` parses the word as a `type_identifier`, where
    // it still bills as a type-name *operand* — the reading Java's
    // `get_op_type_with_code` rejects for its own `var`. FIXME(#1706):
    // that spelling wants the same operator, keyed by its text.
    DeclKeyword {
        owns [
            Class, Interface, Enum, Def, Var(gated_by contextual_keyword_owns),
            Trait(gated_by contextual_keyword_owns), Record(gated_by contextual_keyword_owns),
        ]
    }
    // The pinned grammar has no primitive node (#1568): a primitive is
    // an `identifier` or `type_identifier` spelling one of Java's eight
    // keywords or `void`, told from a name by the bytes and by position
    // — hence `text_gated_by`. `Checker::is_primitive` keys both kinds
    // by text for the same reason.
    Primitive {
        owns [
            Identifier(text_gated_by spells_primitive_type),
            TypeIdentifier(text_gated_by spells_primitive_type),
        ]
    }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Groovy documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GroovyCode;
    use crate::test_support::for_each_node_with_chain;

    // A node / token pairing with a negated spelling that has no token
    // (#1695): `a in b` is owned by its node and paid by its `in`;
    // `a !in b` is owned and paid by the node, since nothing else is
    // there to pay. One pays per test either way, and the `in` of a
    // `for` header pays nothing.
    #[cfg(feature = "groovy")]
    #[test]
    fn membership_pays_at_its_keyword_or_at_itself_when_negated() {
        let source = b"def f(a, b, o, xs) { def n = a in b; def nn = a !in b; def i = o instanceof String; def ni = o !instanceof String; for (x in xs) { } }\n";
        let mut paid = Vec::new();
        for_each_node_with_chain::<GroovyCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if comparison::pays_at_leaf(node, source, ancestors) {
                paid.push(node.utf8_text(source).expect("fixture is UTF-8").to_owned());
            }
        });
        assert_eq!(
            paid,
            ["in", "a !in b", "instanceof", "o !instanceof String"]
        );
    }
}
