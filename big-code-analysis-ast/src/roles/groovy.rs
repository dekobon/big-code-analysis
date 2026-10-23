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
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { pending R05 }
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
    // that leaf: a field's `var field = 1` and a `final var q = 5` parse
    // the word as a `type_identifier`, where the grammar reads it as a
    // type name, and a `var` used as a name elsewhere (`def var = 3`,
    // `[var: 1]`) is an `identifier`.
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
