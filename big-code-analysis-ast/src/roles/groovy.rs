//! Groovy's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::getter::node_text;
use crate::lang_helpers::doc_comment::node_bytes;
use crate::role_table;
use crate::{Ancestors, Groovy, Node};

/// Whether a `membership_expression` or `instanceof_expression` spells
/// its keyword as a token. The negated forms `a !in b` and
/// `o !instanceof T` emit no operator token at all — the node holds its
/// two operands and nothing between them — so the owner pays for
/// itself there and the positive form's keyword pays otherwise.
fn has_no_keyword_token<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_none()
}

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

/// Whether a `type_identifier` spells `var` as the declaration
/// placeholder (#1706).
///
/// The pinned grammar emits `var` as its keyword leaf only under a
/// `local_variable_declaration`. In a field (`var field = 1`, `static
/// var s = 2`), a parameter (`def m(var x)`) and the recovery of a
/// modified local (`final var q = 5`, which the grammar cannot parse
/// and fails into an `ERROR` holding the word) it is a
/// `type_identifier` directly under the declaration, the parameter or
/// the `ERROR`: the three parents admitted. Groovy cannot declare a
/// type named `var` (`class var {}` is a compile error), but the word
/// is a legal *segment* of a qualified type (`a.var.B`, with `package
/// a.var`) and a legal import alias (`import java.lang.String as var`),
/// and both spell a `type_identifier` that names a real type wherever a
/// type is written out — a `qualified_type`, a cast, an `instanceof`, a
/// type argument, an array or generic type — so a `var` under any of
/// those stays the operand it was. An aliased `var` used bare as a
/// field's or parameter's type is the one spelling the shape cannot
/// tell from the placeholder, and it bills as the keyword; a `var` used
/// as a name is an `identifier` and never reaches this gate.
///
/// A `method_declaration` parent is not admitted. The invalid return
/// type `var m() {}` lands there, and so does one valid shape the
/// grammar recovers that way: a script-level `final var s = 9` followed
/// by another statement reads as a method head, where the same source
/// alone recovers as an `ERROR` and is claimed. Whether that `var`
/// bills therefore depends on what follows it — a gap of the pinned
/// grammar's recovery, recorded rather than pinned.
fn spells_var<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    use Groovy::*;

    node_bytes(node, code) == b"var"
        && ancestors.parent(node).is_some_and(|parent| {
            parent.is_error()
                || matches!(parent.kind_id().into(), FieldDeclaration | FormalParameter)
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
    // `command_chain` is the dekobon grammar's distinct node for Groovy's
    // command-style juxtaposed calls (`foo bar baz`), which the prior
    // amaanq grammar mis-modelled as `juxt_function_call`; it is a
    // genuine method-call form. `ObjectCreationExpression` (`new Foo()`)
    // is deliberately absent: the row follows the Java-family convention
    // (Java's `MethodInvocation`, C#'s `InvocationExpression`) of
    // counting method / function call sites only — constructor
    // invocations are an ABC concern, where Groovy's `branches` counts
    // `New` on its own (`groovy_count_token_branch`).
    Call { owns [MethodInvocation, CommandChain] }
    // One `string_literal` kind for every quoting. A GString `wraps` an
    // interpolation in either form — the long `${expr}` and the short
    // `$name` / `$obj.field` — whose inner expression is billed on its
    // own; a plain literal has neither child and is one operand. The
    // `string_fragment*` aliases are the body text the dump keeps
    // verbatim, not literals of their own.
    StringLit {
        owns [StringLiteral]
        wraps [GstringBraceInterpolation, GstringDollarInterpolation]
    }
    // The worked example of a node / token pairing (#1695). A
    // `binary_expression` owns a comparison through its `operator`
    // field, where `<` / `>` are gated to that parent (the grammar
    // also spells them as generic brackets). The dekobon grammar gives
    // Groovy's own comparisons nodes of their own, each wrapping its
    // token: `spaceship_expression` / `<=>`, `identity_expression` /
    // `===` and `!==`, `regex_find_expression` / `=~`,
    // `regex_match_expression` / `==~`, `membership_expression` /
    // `in` (gated: the same keyword opens a `for (x in xs)` header) and
    // `instanceof_expression` / `instanceof` (gated: a keyword error
    // recovery strands under `ERROR` is under no test). The negated
    // forms `!in` and `!instanceof` emit no token at all, so those two
    // owners pay for themselves exactly when they hold no keyword, and
    // once per test either way.
    Comparison {
        owns [
            BinaryExpression(holds_payer),
            SpaceshipExpression, IdentityExpression, RegexFindExpression, RegexMatchExpression,
            MembershipExpression(pays_itself_when has_no_keyword_token),
            InstanceofExpression(pays_itself_when has_no_keyword_token),
        ]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT under BinaryExpression, GT under BinaryExpression,
            LTEQGT, EQEQEQ, BANGEQEQ, EQTILDE, EQEQTILDE,
            In under MembershipExpression, Instanceof under InstanceofExpression,
        ]
        project "operator"
    }
    // `&&` / `||`, which the grammar emits only under a
    // `binary_expression`. The elvis `?:` is `NullSafe`'s.
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
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
        pays [QMARK under TernaryExpression]
        project "operator"
    }
    // Four structural forms, each one decision per token (#452): the
    // elvis `a ?: b` (`elvis_expression` / `?:`), the safe navigation
    // `a?.b` (`safe_navigation_expression` / `?.`), the safe chain
    // `a??.b` (`safe_chain_dot_expression` / `??.`) and the safe
    // subscript `a?[0]` (`safe_subscript_expression` / `?[`). Cyclomatic
    // pays each token; cognitive folds the elvis into a boolean
    // sequence and reads the others not at all; ABC pays the elvis token
    // as a condition — once per token, the Kotlin reading, rather than
    // the C / PHP short-ternary reading that also walks the left
    // operand (grammar-dispatch §8) — and the navigations not at all.
    NullSafe {
        owns [
            ElvisExpression(holds_payer),
            SafeNavigationExpression(holds_payer),
            SafeChainDotExpression(holds_payer),
            SafeSubscriptExpression(holds_payer),
        ]
        pays [QMARKCOLON, QMARKDOT, QMARKQMARKDOT, QMARKLBRACK]
        project "operator"
    }
    // A `switch_case` owns a decision and its `case` keyword pays; the
    // `switch_default` and its `default` keyword are the fallback and
    // pay nothing (lesson 11, #469).
    CaseArm {
        owns [SwitchCase(holds_payer)]
        pays [Case]
        excludes [
            SwitchDefault "the fallback arm, no decision (lesson 11, #469)",
            Default "the fallback label's keyword",
        ]
    }
    Guard { absent "Groovy has no guard clause on a case or a function head" }
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
    // `[var: 1]`) is an `identifier`. A field's `var field = 1`, a
    // parameter's `m(var x)` and a `final var q = 5` parse the word as
    // a `type_identifier`, which the `spells_var` gate claims as the
    // same keyword where it is the placeholder (#1706);
    // `Checker::keys_operator_by_text` then keys both spellings by
    // their bytes so Halstead counts one `var`, where the operand
    // reading had been Java's before #1560 gated its own.
    DeclKeyword {
        owns [
            Class, Interface, Enum, Def, Var(gated_by contextual_keyword_owns),
            TypeIdentifier(text_gated_by spells_var),
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
    // A method and a constructor are the callables; `static { … }`
    // opens a space without being one (#1184). A closure opens no
    // space: the walk never promoted one (`is_func_space` left it out),
    // though the byte-less `get_space_kind` labelled it `Function` if
    // asked — the "pair that fails quietly" of grammar-dispatch §7; the
    // row records the walk's answer, as Java's does for its lambda. An
    // anonymous class opens nothing either: the pinned dekobon grammar
    // parses `new Runnable() { … }` as a bare
    // `object_creation_expression` followed by a separate `closure`, an
    // upstream-grammar limitation, not a row's.
    FuncOpener {
        owns [
            MethodDeclaration(callable opens Function),
            ConstructorDeclaration(callable opens Function),
            StaticInitializer(opens Function),
        ]
    }
    Closure { owns [Closure] }
    // A trait is an interface with default method bodies (#247), so
    // it opens `Interface` as an annotation type does (#280).
    Container {
        owns [
            SourceFile(opens Unit),
            ClassDeclaration(opens Class),
            EnumDeclaration(opens Class),
            RecordDeclaration(opens Class),
            InterfaceDeclaration(opens Interface),
            TraitDeclaration(opens Interface),
            AnnotationTypeDeclaration(opens Interface),
        ]
    }
    // A field declaration — `def field` included, whose `Def` modifier
    // the consumer reads for visibility — in a class or enum body.
    Member { owns [FieldDeclaration] }
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
