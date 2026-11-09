//! Groovy's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::getter::node_text;
use crate::lang_helpers::doc_comment::{java_doc, node_bytes, token_spelling};
use crate::role_table;
use crate::roles::control_flow::else_if_prev_sibling_gate;
use crate::{Ancestors, Groovy, GroovyCode, Node};

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

/// The type a `record_component` or a constructor's `formal_parameter`
/// declares — the `type` field's tokens, comments and whitespace
/// removed — or `None` for an untyped `formal_parameter` (`def` is a
/// `type_identifier`, so only a bare name has none; a bare name in a
/// `record_component` is a `type_identifier` to the pinned grammar,
/// which requires the type there and recovers an untyped list as one
/// component — `groovy_untyped_record_components_recover_as_one`) and
/// for a child that is no parameter.
fn groovy_parameter_type(param: &Node<'_>, code: &[u8]) -> Option<Vec<u8>> {
    param
        .child_by_field_name("type")
        .map(|ty| token_spelling::<GroovyCode>(&ty, code))
}

/// Whether `ctor`, a `constructor_declaration` in `record`'s body,
/// declares the record's canonical constructor: named after the record,
/// with as many formal parameters as the record `header` has
/// components, each spelling the same type as its component (Java's
/// rule, JLS 8.10.4.1 and 8.8, which Groovy records follow). Parameter
/// names and annotations do not take part. The same spelling, not the
/// same type — the tokens, so a comment inside the type is nothing —
/// so `java.util.List<T>` and `List<T>` are two spellings here, as in
/// `roles/java.rs`. The name test is for the grammar, not the language:
/// the pinned grammar has no nested class, so `record R(int a) { class
/// Inner { Inner(int a) { } } }` lands `Inner`'s constructor as a direct
/// `constructor_declaration` of `R`'s body, where arity and types alone
/// would read it as `R`'s canonical constructor.
///
/// Two grammar limitations shape what reaches this. The pinned
/// grammar parses a variable-arity component (`String... xs`) with the
/// `...` in an `ERROR` outside the `type` field while the constructor's
/// parameter has a `varargs_type`, so the two never agree and such a
/// record's header opens a space beside its canonical constructor;
/// and a compact constructor (`R { … }`) does not parse at all: the
/// `record_declaration` ends at its header's `)`, so a record that
/// writes one is read through its `ERROR` nodes as having no body, and
/// its members land at file level.
fn is_record_canonical_constructor(
    ctor: &Node<'_>,
    record: &Node<'_>,
    header: &Node<'_>,
    code: &[u8],
) -> bool {
    let (Some(ctor_name), Some(record_name), Some(params)) = (
        ctor.child_by_field_name("name"),
        record.child_by_field_name("name"),
        ctor.child_by_field_name("parameters"),
    ) else {
        return false;
    };
    if node_bytes(&ctor_name, code) != node_bytes(&record_name, code) {
        return false;
    }
    let declared = params
        .children()
        .filter(|p| p.kind_id() == Groovy::FormalParameter as u16)
        .map(|p| groovy_parameter_type(&p, code));
    let components = header
        .children()
        .filter(|p| p.kind_id() == Groovy::RecordComponent as u16)
        .map(|p| groovy_parameter_type(&p, code));
    declared.eq(components)
}

/// Whether a `record_components` list is a record's header in a record
/// that declares no canonical constructor, so the header stands for
/// the implicitly declared one (#1842): the `components` field of a
/// `record_declaration`, a sibling of its `class_body`, checked by
/// field as Java's is so a grammar bump that reuses the kind elsewhere
/// under the record opens nothing there. A written canonical
/// constructor ([`is_record_canonical_constructor`]) owns the
/// components instead; a non-canonical one is a callable of its own
/// and leaves the header open. The gate reads the record's body, so it
/// is a `fn` rather than `under [RecordDeclaration]`, and the body's
/// constructor types as bytes, so the row marks it `text_gated_by`:
/// handed no bytes, every same-arity constructor reads as canonical.
fn opens_record_header<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    let Some(record) = ancestors.parent(node) else {
        return false;
    };
    if record.kind_id() != Groovy::RecordDeclaration as u16
        || record
            .child_by_field_name("components")
            .is_none_or(|p| p.id() != node.id())
    {
        return false;
    }
    let Some(body) = record.child_by_field_name("body") else {
        return true;
    };
    !body.children().any(|member| {
        member.kind_id() == Groovy::ConstructorDeclaration as u16
            && is_record_canonical_constructor(&member, &record, node, code)
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
/// var s = 2`), a parameter (`def m(var x)`), a try-with-resources
/// `resource` (`try (var r = open())`), a `catch` parameter (`catch
/// (var e)`) and the recovery of a modified local (`final var q = 5`,
/// which the grammar cannot parse and fails into an `ERROR` holding the
/// word) it is a `type_identifier` directly under the declaration, the
/// parameter, the resource, the catch parameter or the `ERROR`: the
/// five parents admitted. Groovy cannot declare a
/// type named `var` (`class var {}` is a compile error), but the word
/// is a legal *segment* of a qualified type (`a.var.B`, with `package
/// a.var`) and a legal import alias (`import java.lang.String as var`),
/// and both spell a `type_identifier` that names a real type wherever a
/// type is written out — a `qualified_type`, a cast, an `instanceof`, a
/// type argument, an array or generic type — so a `var` under any of
/// those stays the operand it was. An aliased `var` used bare as a
/// field's, parameter's or resource's type is the one spelling the
/// shape cannot tell from the placeholder, and it bills as the keyword;
/// a `var` used as a name is an `identifier` and never reaches this
/// gate. The batch review found the resource and catch parents missing
/// from the first list — both were in the evidence dump and not acted
/// on.
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
                || matches!(
                    parent.kind_id().into(),
                    FieldDeclaration | FormalParameter | Resource | CatchFormalParameter
                )
        })
}

// `else if` is an `if_statement` whose previous sibling is the bare `else`
// token — no clause wrapper — so a nested `if` under `else { … }` follows
// `{` instead (#1697).
else_if_prev_sibling_gate!(Groovy, Else);

/// The container a `switch` nests its arms in, the `Switch` owner's
/// child: where cognitive opens its level (see Java's twin, #1697).
/// A consumer's structural projection of an owner's child, not a row:
/// the `Switch` row records the kind under `excludes`, and
/// `is_else_if_is_the_row_and_the_switch_body_is_the_owners_child`
/// pins the parent/child relation (#1701). Java's twin is instead
/// claimed by its `DocBodyResidual` row, because javadoc reads a doc
/// comment inside a `switch_block` in a field initializer; whether
/// Groovydoc does the same was not measured in #1699, so this
/// language's `DocBodyResidual` row does not list the kind and the
/// exclusion here is a coverage-accounting fact, not a Groovydoc
/// one. Deciding it is the row-membership question #1797 holds.
#[inline]
#[must_use]
pub fn is_switch_body_kind(kind: u16) -> bool {
    kind == Groovy::SwitchBlock as u16
}

// Whether a `groovydoc_comment` opens with the marker Groovy's own
// `GroovydocManager` reads: the grammar marks `/** … */` as its own
// kind but also gives the empty `/**/` that kind, which documents
// nothing; a `/***` one is read, exactly as javac reads Javadoc.
crate::roles::byte_gate!(opens_with_groovydoc_marker, java_doc);

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
    // subscript `a?[0]` (`safe_subscript_expression` / `?[`). Which of
    // the four each consumer scores is the consumer's to say
    // (`src/metrics/{abc,cyclomatic}/groovy.rs`).
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
    // The `if_statement` owns the branch; its `if` keyword pays, once per
    // `if` and inside an `else if` chain too (#284, #1697).
    Branch { owns [IfStatement] pays [If] }
    // The `switch_expression` is the grammar's one node for the statement
    // and expression forms; its `switch` keyword pays once per switch,
    // which keeps the modified count aligned with the `case` arms (#300).
    // The `switch_block` — the arms' container, where cognitive opens its
    // level so a ternary or closure in the *subject* does not inherit
    // one — is the owner's child, not a construct of its own: no row
    // claims it, and it is excluded here rather than left as coverage
    // debt. The consumer's projection onto it is the named
    // `is_switch_body_kind` beside this table (#1701).
    Switch {
        owns [SwitchExpression]
        pays [Switch]
        excludes [SwitchBlock "the owner's child; cognitive's projection is the named `is_switch_body_kind` beside the table"]
    }
    // Every loop statement, its keyword paying: `for` once in the classic
    // and the `for (x in xs)` form, `while` once in a `while` and once in
    // a `do … while` (#284).
    Loop {
        owns [ForStatement, ForInStatement, WhileStatement, DoWhileStatement]
        pays [For, While]
    }
    Catch { owns [CatchClause] pays [Catch] }
    // Groovy spells `else if` as Java does: an `if_statement` whose
    // previous sibling is the bare `else` token. Recognition only; the
    // `Else` token pays cognitive's +1 and the `if` keyword `Branch`'s.
    ElseIf { owns [IfStatement(gated_by follows_else_keyword)] pays [] }
    // `return`, `throw`, and the Java 14+ switch-expression `yield`.
    // Implicit-return-from-closure is not an exit (consistent with Java).
    Exit { owns [ReturnStatement, ThrowStatement, YieldStatement] }
    // Every modifier (#1565). The grammar has no `modifiers` wrapper —
    // each modifier leaf sits directly under its declaration — so each
    // bills once. `sealed` and `permits` are contextual and bill only
    // under the construct they introduce; `non-sealed` cannot be spelled
    // as a name (`non-sealed` as a subtraction parses as `non`, `-` and
    // an identifier `sealed`). `default` is not here: it is one kind for
    // a `switch` label and an interface method's modifier, and bills
    // with the control keywords. The `static` of `import static` sits
    // under the `import_declaration`, a directive that qualifies no
    // declaration; Java's row reads the same parent, and the reasoning
    // (#1600) is in the comment above it.
    Modifier {
        owns [
            Public, Private, Protected, Static(not_under [ImportDeclaration]), Abstract, Final,
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
    // A method and a constructor are the callables, and so is a
    // record's header — its `record_components`, the `components`
    // field of the `record_declaration` and a sibling of the
    // `class_body` — where the body writes no canonical constructor
    // and the header stands for the implicit one (#1842, Java's shape
    // in `roles/java.rs`; the grammar has no compact-constructor kind,
    // see `opens_record_header`). `static { … }`
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
            RecordComponents(callable opens Function text_gated_by opens_record_header),
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
    // The grammar tells Groovydoc apart by kind: `groovydoc_comment` is
    // the `/** … */` form, a distinct node (Groovy 150) from the `//`
    // and `/* */` comments, so it sits on `DocComment` alone — under a
    // byte gate, since the kind also takes the empty `/**/` — and
    // `Comment` holds the other two. `is_comment` is the two kind tables
    // as one, which is what keeps the empty `/**/` a comment (#697,
    // #1699).
    Comment { owns [LineComment, BlockComment] }
    DocComment { owns [GroovydocComment(text_gated_by opens_with_groovydoc_marker no_chain)] }
    Docstring { absent "Groovy documents with comments, not string literals" }
    // The nodes `AstBuilder` hands to `GroovydocManager::handle`: a
    // type, a method or constructor, a field or property, an enum
    // constant. The enum constant is the one target outside the
    // space-tree rows — the parity exception `roles/doc_tests/parity.rs`
    // records.
    DocTarget {
        owns [
            ClassDeclaration,
            TraitDeclaration,
            InterfaceDeclaration,
            EnumDeclaration,
            RecordDeclaration,
            AnnotationTypeDeclaration,
            MethodDeclaration,
            ConstructorDeclaration,
            FieldDeclaration,
            EnumConstant,
        ]
    }
    DocRunOpener { absent "Groovydoc attaches every doc comment to a declaration; none documents by its own content" }
    DocRunDirective { absent "Groovydoc reads no directive comment" }
    // Beyond the callables and the closure: a `static { … }` initializer
    // opens a function space without being a callee, and Groovydoc
    // reads a doc comment in it no more than one in a method body.
    DocBodyResidual { owns [StaticInitializer] }
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
