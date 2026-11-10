//! Java's role rows: one row of each shape the macro accepts, so the
//! shapes were proven against a real grammar before any consumer read
//! them (#1691). The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::checker::java_anonymous_class_body;
use crate::lang_helpers::doc_comment::{java_doc, node_bytes, token_spelling};
use crate::role_table;
use crate::roles::control_flow::else_if_prev_sibling_gate;
use crate::{Ancestors, Checker, Java, JavaCode, Node};

// Whether a comment opens with javac's doc marker (`/**`, not `/**/`;
// a `///` Markdown line, JEP 467), read from the bytes.
crate::roles::byte_gate!(opens_with_javadoc_marker, java_doc);

/// Whether a `package` declaration takes the file's first doc comment:
/// javac's `attach` gives it the comment, javadoc reads a package's
/// only from `package-info.java`, and javac's lint calls it dangling
/// once a declaration follows — so a package documents when no
/// declaration follows it before the next package, which is that
/// file's shape (`package-info.java`), and a file of repeated packages
/// — invalid, but parsed without error — is read in disjoint stretches
/// rather than to its end from each one (#1446). The scan reads the
/// row's *kind table* (`lists_kind`), never `owns`: asking `owns` of a
/// sibling package would re-enter this gate from inside its own scan,
/// quadratic on repeated packages.
fn package_takes_the_file_doc<'a>(node: &Node<'a>, _: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .siblings_after(node)
        .find(|n| doc_target::lists_kind(n.kind_id()))
        .is_none_or(|n| n.kind_id() == Java::PackageDeclaration as u16)
}

/// Whether an `object_creation_expression` carries a `class_body`, which
/// makes it an anonymous class rather than a plain `new Foo()` (#463).
fn has_class_body(node: &Node<'_>, _code: &[u8]) -> bool {
    java_anonymous_class_body(node).is_some()
}

// `else if` is an `if_statement` whose previous sibling is the bare `else`
// token — no clause wrapper — so a nested `if` under `else { … }` follows
// `{` instead (#1697).
else_if_prev_sibling_gate!(Java, Else);

/// The type a record component or a constructor's formal parameter
/// declares, as the canonical-constructor rule compares it: the source
/// spelling as its tokens — comments and whitespace removed — and
/// whether the parameter is a variable-arity one.
#[derive(PartialEq, Eq)]
struct ParameterType {
    spelling: Vec<u8>,
    variadic: bool,
}

/// The [`ParameterType`] of a record component or a constructor's
/// formal parameter; `None` for a child that is no parameter
/// (punctuation, a comment, a receiver parameter).
///
/// A `formal_parameter` holds its annotations in a separate `modifiers`
/// child and its type in the `type` field, with C-style trailing
/// `dimensions` (`int a[]`) in a field of their own; a
/// `spread_parameter` (`int... a`) has no fields, so its type is the
/// first named child that is not an annotation, the `modifiers` or a
/// comment — a `block_comment` is a named extra, and one between the
/// modifiers and the type (`final /* c */ int... xs`) sits exactly
/// where the type is sought.
fn java_parameter_type(param: &Node<'_>, code: &[u8]) -> Option<ParameterType> {
    match param.kind_id().into() {
        Java::FormalParameter => {
            let mut spelling =
                token_spelling::<JavaCode>(&param.child_by_field_name("type")?, code);
            if let Some(dims) = param.child_by_field_name("dimensions") {
                spelling.extend(token_spelling::<JavaCode>(&dims, code));
            }
            Some(ParameterType {
                spelling,
                variadic: false,
            })
        }
        Java::SpreadParameter => param
            .children()
            .find(|c| {
                c.is_named()
                    && !JavaCode::is_comment(c)
                    && !matches!(
                        c.kind_id().into(),
                        Java::Modifiers | Java::Annotation | Java::MarkerAnnotation
                    )
            })
            .map(|ty| ParameterType {
                spelling: token_spelling::<JavaCode>(&ty, code),
                variadic: true,
            }),
        _ => None,
    }
}

/// Whether `ctor`, a `constructor_declaration` in `record`'s body,
/// declares the record's canonical constructor (JLS 8.10.4.1): it is
/// named after the record (JLS 8.8, which every constructor must be —
/// the check is for a body the grammar fills with another class's
/// constructor, as tree-sitter-groovy's nested-class recovery does and
/// as tree-sitter-java accepts for the invalid `record R(int a) { Q(int
/// a) { } }`), and its formal parameters and the record `header`'s
/// components have the same arity and, pairwise, the same declared type
/// and the same variable-arity-ness. Parameter names and annotations
/// may differ and do not take part.
///
/// "Same type" is read as the same spelling — the type's tokens, with
/// comments and whitespace removed, so `List</* c */ String>` and
/// `List<String>` agree — since the row has no symbol table:
/// `java.util.List<T>` and `List<T>` are one type to javac and two
/// spellings here, as are `List<@Nullable String>` and `List<String>`
/// (a type-use annotation is a token of the type), and `int...` and
/// `int[]` (JLS 8.4.1 makes the pair one type, and the canonical rule
/// then requires the arity marker to match anyway). A record whose
/// explicit canonical constructor respells a component's type that way
/// is read as having none, and its header opens a space of its own
/// beside the constructor's. The generic form is exact: `R<T>(T a)`
/// and `R(T a)` spell `T` the same.
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
        .filter_map(|p| java_parameter_type(&p, code));
    let components = header
        .children()
        .filter_map(|p| java_parameter_type(&p, code));
    declared.eq(components)
}

/// Whether a `formal_parameters` list is a record's header — the
/// component list hanging directly off a `record_declaration`, a
/// sibling of its `class_body` — in a record that declares no canonical
/// constructor, so the header stands for the implicitly declared one
/// (JLS 8.10.4; #1842). A record that writes one, compact
/// (`R { … }`) or normal (`R(int a, int b) { … }`, recognised by
/// [`is_record_canonical_constructor`]), owns the components there
/// instead; a non-canonical constructor (`R(int a) { this(a, 0); }`)
/// is a callable of its own and leaves the header open. The gate reads
/// the record's body, which a parent-kind list cannot, so it is a `fn`
/// rather than `under [RecordDeclaration]`; and it reads the body's
/// constructor *types* as bytes, so the row marks it `text_gated_by`:
/// handed no bytes, every same-arity constructor spells the empty type
/// and reads as canonical.
fn opens_record_header<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    let Some(record) = ancestors.parent(node) else {
        return false;
    };
    if record.kind_id() != Java::RecordDeclaration as u16
        || record
            .child_by_field_name("parameters")
            .is_none_or(|p| p.id() != node.id())
    {
        return false;
    }
    let Some(body) = record.child_by_field_name("body") else {
        return true;
    };
    !body.children().any(|member| match member.kind_id().into() {
        Java::CompactConstructorDeclaration => true,
        Java::ConstructorDeclaration => {
            is_record_canonical_constructor(&member, &record, node, code)
        }
        _ => false,
    })
}

/// The container a `switch` nests its arms in: the `switch_block`, the
/// `Switch` owner's child — a structural fact about the construct the
/// row recognises, pinned by
/// `switch_body_is_the_owners_child_and_not_the_switch_rows`, not an
/// owner of that row. The kind is claimed by `DocBodyResidual` (a
/// `switch_block` holds a doc comment, #1699), which is a different
/// question. Which consumer opens a level here (cognitive, so a ternary
/// or lambda in the switch's *subject* does not inherit one, #1697) is
/// that consumer's policy, and a consumer's structural projection of an
/// owner's child lives as a named `fn` beside the table, as here
/// (#1701); a DSL `body` attribute on the `Switch` row would be the
/// declared form if a third grammar needs it.
#[inline]
#[must_use]
pub fn is_switch_body_kind(kind: u16) -> bool {
    kind == Java::SwitchBlock as u16
}

role_table! {
    lang: Java;

    // Projected and gated. A `binary_expression` owns a comparison only
    // when its `operator` field holds one; the equality and ordering
    // tokens pay unconditionally except `<` / `>`, which the grammar also
    // emits as generic-type brackets. An `instanceof_expression` owns
    // the type test — one node spans both the plain test and the Java
    // 16 pattern form `x instanceof String s` — and its `instanceof`
    // keyword pays, gated to that owner: the grammar spells the keyword
    // in no other production, so one pays per test (#1461), and a
    // keyword error recovery strands under an `ERROR` pays for no
    // test. `?` is excluded here
    // rather than left unmentioned: a ternary's `?` is a `Ternary`
    // matter, a wildcard bound's is type syntax, and neither is a
    // comparison.
    Comparison {
        owns [
            BinaryExpression(holds_payer),
            InstanceofExpression,
        ]
        pays [
            EQEQ,
            BANGEQ,
            LTEQ,
            GTEQ,
            LT under BinaryExpression,
            GT under BinaryExpression,
            Instanceof under InstanceofExpression,
        ]
        project "operator"
        excludes [QMARK "a `?` is a ternary's decision or a wildcard bound; `Ternary` owns it"]
    }

    // The same `binary_expression` owns an `&&` / `||` chain when its
    // `operator` field holds one; the two tokens occur in no other
    // production, so they pay ungated. `a && b || c` is a left-nested
    // chain of these nodes, each owning one operator.
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }

    // `wraps`: a `string_literal` holding a `string_interpolation`
    // child is a Java 21 string template (`STR."a \{x}"`), whose inner
    // expressions are walked on their own. The set is what
    // `Getter::string_operand_type` reads in `getter/java.rs` to leave
    // such a wrapper unbilled (#420, wired by #1703).
    // `MultilineStringLiteral` is the hidden `_multiline_string_literal`
    // rule; the parser never emits it today, and the drift marker in the
    // tests below pins that (grammar-dispatch §2).
    StringLit {
        owns [StringLiteral, MultilineStringLiteral]
        wraps [StringInterpolation]
    }

    // Mapping. Every function opener is a `Function` space. The four
    // `callable` owners are what `is_func` answers — a method, a
    // constructor, a record's compact constructor (#1160) and a
    // record's header where it stands for the implicit canonical
    // constructor (#1842) — each a function someone names at a call
    // site. `static { … }` is not:
    // it opens a space (#1184) and is nobody's callee, so it carries no
    // `callable` and `nom` / `nargs` / `npm` never see it as a method.
    // The lambda is *not* here: the walk promotes on
    // `promotes_to_func_space_with_code`, and Java's `is_func` and
    // `is_func_space` both leave `lambda_expression` out, so no lambda
    // opens a space today — even though the byte-less `get_space_kind`
    // labelled one `Function` before #1696. The row records the walk's
    // answer, and the parity test below pins it against the walk's
    // predicate.
    //
    // A record's canonical constructor has no node of its own unless
    // written: the grammar hangs the component list, a
    // `formal_parameters`, directly off the `record_declaration` as its
    // `parameters` field, a sibling of the `class_body` — the one place
    // a `formal_parameters` is not a callable's `parameters` field.
    // That list is the constructor where the body declares none
    // (`opens_record_header`), as C#'s positional record's
    // `parameter_list` is (#1828).
    FuncOpener {
        owns [
            MethodDeclaration(callable opens Function),
            ConstructorDeclaration(callable opens Function),
            CompactConstructorDeclaration(callable opens Function),
            FormalParameters(callable opens Function text_gated_by opens_record_header),
            StaticInitializer(opens Function),
        ]
    }

    // A closure that opens no space (contract 2 of #1690): recognised
    // by `is_closure` for the arity and count metrics, promoted by
    // nothing — see `FuncOpener` above.
    Closure { owns [LambdaExpression] }

    // Gated mapping: `new Runnable() { … }` opens a `Class` space only
    // when it carries a body (#463). Enum, record and annotation types
    // are class-like; annotation types map to `Interface` (#280).
    Container {
        owns [
            Program(opens Unit),
            ClassDeclaration(opens Class),
            EnumDeclaration(opens Class),
            RecordDeclaration(opens Class),
            InterfaceDeclaration(opens Interface),
            AnnotationTypeDeclaration(opens Interface),
            ObjectCreationExpression(opens Class gated_by has_class_body no_chain),
        ]
    }

    Call { owns [MethodInvocation] }

    // A `case X when g ->` guard (Java 21) is the `guard` node, one
    // decision paid at the node — cyclomatic counts it, ABC walks its
    // expression as a boolean slot — and its `when` keyword bills
    // nothing: a contextual keyword the grammar emits only here, which
    // Halstead has never billed. R01's proof row said `pays never`,
    // which was the Halstead reading; the counting consumers pay the
    // node, so the owner is the paying projection (#1695).
    Guard { owns [Guard] excludes [When "a contextual keyword Halstead has never billed"] }

    NullSafe { absent "Java has no optional-navigation or null-coalescing operator" }
    // JLS §15.22.2 makes `&` / `|` / `^` on boolean operands eager
    // logical operators, and the grammar emits them as the same
    // `binary_expression` as the integral forms, so the syntax alone
    // cannot tell them apart; no consumer treats them as a decision,
    // and the three tokens bill as the operators they are. A
    // recognition boundary, recorded as the kinds considered rather
    // than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }

    // Filled in by the packages that migrate their consumers.
    // The `ternary_expression` owns it and its `?` pays, gated to that
    // owner: the grammar also spells `?` as a `wildcard` bound, which is
    // type syntax (#1274). The `Comparison` row's exclusion above says
    // the same.
    Ternary {
        owns [TernaryExpression]
        pays [QMARK under TernaryExpression]
        project "operator"
    }
    // A `switch_label` is the grammar's node for `case X` and `default`
    // alike, in the colon form (`switch_block_statement_group`) and the
    // arrow form (`switch_rule`); the label owns a decision when it
    // holds the `case` keyword, which pays — one per label, so a
    // fall-through run `case 2: case 3:` is two and a multi-pattern
    // `case 2, 3 ->` is one. `default` is the fallback and pays nothing
    // (lesson 11, #469); Halstead bills it as the keyword it is.
    CaseArm {
        owns [SwitchLabel(holds_payer)]
        pays [Case]
        excludes [Default "the fallback label, no decision (lesson 11, #469)"]
    }
    // The `if_statement` owns the branch; its `if` keyword pays. The
    // keyword is the paying node because cyclomatic counts keyword
    // tokens here (#284): the token fires once per `if`, inside an
    // `else if` chain too, where the statement is the `ElseIf` row's
    // and must not open a second nesting level in cognitive.
    Branch { owns [IfStatement] pays [If] }
    // The `switch_expression` is the grammar's one node for the
    // statement and the Java 14 expression form alike; its `switch`
    // keyword pays, once per switch, which keeps the modified count
    // aligned with the `case` arms (#300). The `switch_block` — the
    // arms' container, where cognitive opens its nesting level so a
    // ternary or lambda in the *subject* does not inherit one (#1697:
    // moving the level to this owner drifted) — is the owner's child,
    // not this row's: `DocBodyResidual` claims the kind, for the doc
    // comment its body can hold, and cognitive names it as the body of
    // the construct this row recognises (`is_switch_body_kind`, #1701).
    Switch { owns [SwitchExpression] pays [Switch] }
    // Every loop statement, with its keyword paying: `for` fires once
    // in a classic and an enhanced `for` alike, `while` once in a
    // `while` and once in a `do … while` (#284).
    Loop {
        owns [ForStatement, EnhancedForStatement, WhileStatement, DoStatement]
        pays [For, While]
    }
    // The `catch_clause`, paid at its `catch` keyword; `try` and
    // `finally` are unconditional and pay nothing (#416).
    Catch { owns [CatchClause] pays [Catch] }
    // Java spells `else if` as an `if_statement` whose previous sibling
    // is the bare `else` token — there is no clause wrapper — so the
    // chained `if` is told from a nested one (`else { if … }`, whose
    // previous sibling is `{`) by that sibling. Recognition only: the
    // `Else` token already pays cognitive's +1 and the `if` keyword
    // pays cyclomatic's through `Branch`, so no owner here pays at the
    // leaf — a split row with no payers (#1697).
    ElseIf { owns [IfStatement(gated_by follows_else_keyword)] pays [] }
    // `return`, `throw` and a `switch` expression's `yield`, each a
    // statement kind paid at the node.
    Exit { owns [ReturnStatement, ThrowStatement, YieldStatement] }
    // Every modifier, billed as C# and Kotlin bill theirs (#1565). Each
    // is its keyword leaf, one kind apiece; the `modifiers` wrapper
    // stays unlisted, so a modifier bills once and an annotation inside
    // it adds only its own tokens. `default` is not here: it is one
    // kind for a `switch` label and an interface method's modifier, and
    // bills with the control keywords. `sealed` and `permits`
    // (`Permits2`; `Permits` is the clause) are contextual, and the
    // grammar parses each used as a name (`var sealed = 1`,
    // `int permits`) as an `identifier`, so they need no gate. The
    // `static` of `import static` and `requires static` sits under an
    // `import_declaration` / `requires_modifier`, a directive that
    // qualifies no declaration, and is not a modifier; what the
    // Halstead consumer makes of directive words is its own
    // (`java_modifiers_are_operators_1565`, #1600).
    Modifier {
        owns [
            Public, Private, Protected, Static(not_under [ImportDeclaration, RequiresModifier]),
            Abstract, Final,
            Native, Strictfp, Synchronized, Transient, Volatile, Sealed, NonDASHsealed, Permits2,
        ]
    }
    // The type-declaration keywords, billed as C# bills its own
    // (#1552). Only the leaves: no `*_declaration` wrapper is listed, so
    // a declaration bills its keyword once. A `class` under a
    // `class_literal` (`C.class`) ends the literal and declares nothing:
    // JLS §15.8.2 makes the literal a primary expression, not a literal
    // (§3.10), so its parts are what they are anywhere else (#1566).
    DeclKeyword {
        owns [Class(not_under [ClassLiteral]), Interface, Enum, Record, ATinterface]
    }
    // Each primitive is a kind of its own (`int`, `boolean_type`, …), so
    // none is keyed by text and `Checker::is_primitive` stays the
    // default. `void` bills with them.
    Primitive { owns [Byte, Short, Int, Long, Char, Float, Double, BooleanType, VoidType] }
    // A field (`int a, b;` — one declaration, two `variable_declarator`
    // children, which the attribute count walks) in a class, enum or
    // record body, and an interface's or annotation type's constant.
    // Recognition only: whether a member is public, and which body it
    // sits in, is the consumers' reading (`npa/java.rs`). An annotation
    // type's `int v() default 1;` (`annotation_type_element_declaration`)
    // is an abstract method at the bytecode level and is counted by the
    // method consumer, not here.
    Member { owns [FieldDeclaration, ConstantDeclaration] }
    // The two comment kinds; either can open with javac's doc marker,
    // so both are `DocComment` owners under the byte gate and
    // `is_comment` is the two rows' kind tables as one (#1699).
    Comment { owns [LineComment, BlockComment] }
    DocComment {
        owns [
            LineComment(text_gated_by opens_with_javadoc_marker no_chain),
            BlockComment(text_gated_by opens_with_javadoc_marker no_chain),
        ]
    }
    Docstring { absent "Java documents with comments, not string literals" }
    // What javac attaches a doc comment to (`JavacParser`'s `attach`
    // calls): a type, a member, an enum constant and a module. An
    // import or a statement takes none, and an initializer's is
    // dropped (`ignoreDanglingComments`). The package declaration is
    // gated on the file's shape (#1446). The space-tree rows the
    // targets overlap are `FuncOpener` / `Container` / `Member`; the
    // targets outside those — the module, the enum constant, an
    // annotation type's element (an abstract method at the bytecode
    // level, counted by the method consumer and not a member here) —
    // are the parity exceptions `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            PackageDeclaration(gated_by package_takes_the_file_doc),
            ModuleDeclaration,
            ClassDeclaration,
            InterfaceDeclaration,
            EnumDeclaration,
            RecordDeclaration,
            AnnotationTypeDeclaration,
            MethodDeclaration,
            ConstructorDeclaration,
            CompactConstructorDeclaration,
            FieldDeclaration,
            ConstantDeclaration,
            AnnotationTypeElementDeclaration,
            EnumConstant,
        ]
    }
    DocRunOpener { absent "javadoc attaches every doc comment to a declaration; none documents by its own content" }
    DocRunDirective { absent "javadoc reads no directive comment" }
    // Beyond the callables and the lambda: a `static { … }` initializer
    // (a `FuncOpener` that is nobody's callee, #1184), an instance
    // initializer's bare `block`, and a `switch_block` — every one
    // executable code, a field initializer's `switch` expression
    // holding one outside any method — so javadoc reads a doc comment
    // in each no more than one in a method body. The full answer is
    // `func_opener::is_callable ∪ closure::owns ∪` this row, composed
    // in `checker/java.rs`.
    DocBodyResidual { owns [StaticInitializer, Block, SwitchBlock] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ast_has_kind_id, for_each_node_with_chain};
    use crate::{Checker, Getter, JavaCode, JavaParser, ParserTrait, SpaceKind};
    use std::path::Path;

    /// Every occurrence of each kind the `Comparison` row names, with
    /// the row's three answers: `(kind, text, owns, leaf, wrapper)`.
    #[cfg(feature = "java")]
    fn comparison_occurrences(source: &[u8]) -> Vec<(Java, String, bool, bool, bool)> {
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let kind: Java = node.kind_id().into();
            if !matches!(
                kind,
                Java::BinaryExpression
                    | Java::InstanceofExpression
                    | Java::Instanceof
                    | Java::LT
                    | Java::GT
                    | Java::EQEQ
                    | Java::QMARK
            ) {
                return;
            }
            let ancestors = Ancestors::known(chain);
            seen.push((
                kind,
                node.utf8_text(source).expect("fixture is UTF-8").to_owned(),
                comparison::owns(node, source, ancestors),
                comparison::pays_at_leaf(node, source, ancestors),
                comparison::pays_at_wrapper(node, source, ancestors),
            ));
        });
        seen
    }

    // Until #1696 the rows restated `checker/java.rs` and `getter/java.rs`
    // and this compared the two; now the classifiers *read* the rows, so
    // a comparison would be a forwarding check. The expected answer per
    // kind is pinned independently here instead — on a fixture holding
    // every kind the membership rows name, so a kind dropped from a row,
    // or a classifier that stops reading its row, fails here rather than
    // drifting silently (lesson 35). The per-shape tests above do not
    // cover membership: dropping `YieldStatement` from `Exit` or
    // `EnumDeclaration` from `Container` failed none of them.
    /// `(is_func_with_code, is_closure, promoted, SpaceKind, is_member)`:
    /// the five answers the space-tree and member consumers read of one
    /// node.
    #[cfg(feature = "java")]
    type Answers = (bool, bool, bool, SpaceKind, bool);

    #[cfg(feature = "java")]
    #[test]
    #[allow(clippy::too_many_lines)]
    fn classifiers_read_the_rows_and_the_rows_say_what_the_walk_did() {
        let source = b"enum E { A; int f; }\nrecord R(int a) { R { } int g; }\nrecord Q(int q) { }\ninterface I { int K = 1; void im(); }\n@interface An { int v() default 1; int C = 2; }\nclass C {\n  static { }\n  C() { }\n  String s = \"x\";\n  void m() { Runnable r = () -> m(); Object o = new Object() { }; Object p = new Object(); throw new RuntimeException(); }\n  int y(int s) { return switch (s) { default -> { yield 1; } }; }\n}\n";
        // Per kind, as the classifiers answered before the rows and must
        // answer after. A kind absent here answers all-false.
        let expected: &[(&str, Answers)] = &[
            ("program", (false, false, true, SpaceKind::Unit, false)),
            (
                "class_declaration",
                (false, false, true, SpaceKind::Class, false),
            ),
            (
                "enum_declaration",
                (false, false, true, SpaceKind::Class, false),
            ),
            (
                "record_declaration",
                (false, false, true, SpaceKind::Class, false),
            ),
            (
                "interface_declaration",
                (false, false, true, SpaceKind::Interface, false),
            ),
            (
                "annotation_type_declaration",
                (false, false, true, SpaceKind::Interface, false),
            ),
            (
                "method_declaration",
                (true, false, true, SpaceKind::Function, false),
            ),
            (
                "constructor_declaration",
                (true, false, true, SpaceKind::Function, false),
            ),
            (
                "compact_constructor_declaration",
                (true, false, true, SpaceKind::Function, false),
            ),
            (
                "static_initializer",
                (false, false, true, SpaceKind::Function, false),
            ),
            (
                "lambda_expression",
                (false, true, false, SpaceKind::Unknown, false),
            ),
            (
                "field_declaration",
                (false, false, false, SpaceKind::Unknown, true),
            ),
            (
                "constant_declaration",
                (false, false, false, SpaceKind::Unknown, true),
            ),
        ];
        let none = (false, false, false, SpaceKind::Unknown, false);
        let mut seen_kinds = std::collections::BTreeSet::new();
        let mut anonymous = Vec::new();
        let mut headers = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            let promoted = JavaCode::promotes_to_func_space_with_code(node, source, ancestors);
            let got = (
                JavaCode::is_func_with_code(node, source, ancestors),
                JavaCode::is_closure(node, ancestors),
                promoted,
                JavaCode::get_space_kind_with_code(node, source, ancestors),
                member::owns(node, source, ancestors),
            );
            if kind == "object_creation_expression" {
                // Gated on the body: `new Object() { }` opens a class;
                // `new Object()` and `new RuntimeException()` nothing
                // (#463), told apart by the gate; neither spelling is a
                // function, a closure or a member.
                anonymous.push(got);
            } else if kind == "formal_parameters" {
                // Gated on the parent and its body (#1842): `Q`'s header
                // is the implicit canonical constructor; `R`'s body
                // writes a compact one, and every other list is a
                // callable's `parameters` field.
                headers.push((node.utf8_text(source).unwrap_or("?").to_owned(), got));
            } else {
                let want = expected
                    .iter()
                    .find(|(k, _)| *k == kind)
                    .map_or(none, |(_, v)| *v);
                assert_eq!(got, want, "{kind}");
            }
            // Promotion and labelling are the two halves of one answer.
            assert_eq!(
                got.3 != SpaceKind::Unknown,
                promoted,
                "{kind}: opens_space vs promotion"
            );
            assert_eq!(
                JavaCode::is_func_space_with_code(node, source, ancestors),
                promoted,
                "{kind}: is_func_space is what the walk promotes"
            );
            // The byte-less spellings are the kind table: only the
            // record header's gate reads the chain and the bytes, so
            // they agree with the `_with_code` ones on every other node
            // (`java_byte_less_spellings_omit_the_record_header` pins
            // the header apart).
            if kind != "formal_parameters" {
                assert_eq!(
                    (
                        JavaCode::is_func(node, ancestors),
                        JavaCode::is_func_space(node),
                        JavaCode::get_space_kind(node)
                    ),
                    (got.0, promoted, got.3),
                    "{kind}: the byte-less spellings are the kind table"
                );
            }
            assert_eq!(
                call::owns(node, source, ancestors),
                kind == "method_invocation",
                "{kind}: Call"
            );
            assert_eq!(
                string_lit::owns(node, source, ancestors),
                kind == "string_literal",
                "{kind}: StringLit"
            );
            assert_eq!(
                exit::owns(node, source, ancestors),
                matches!(
                    kind,
                    "return_statement" | "throw_statement" | "yield_statement"
                ),
                "{kind}: Exit"
            );
            seen_kinds.insert(node.kind_id());
        });
        assert_eq!(
            anonymous,
            [(false, false, true, SpaceKind::Class, false), none, none]
        );
        assert_eq!(
            headers,
            [
                ("(int a)".to_owned(), none),
                (
                    "(int q)".to_owned(),
                    (true, false, true, SpaceKind::Function, false)
                ),
                ("()".to_owned(), none),
                ("()".to_owned(), none),
                ("()".to_owned(), none),
                ("()".to_owned(), none),
                ("(int s)".to_owned(), none),
            ]
        );
        // Fixture guard: every kind the membership rows name was walked.
        for kinds in [
            exit::OWNERS,
            call::OWNERS,
            string_lit::OWNERS,
            closure::OWNERS,
            func_opener::OWNERS,
            container::OWNERS,
            member::OWNERS,
        ] {
            for &k in kinds {
                // The hidden `_multiline_string_literal` is pinned unseen
                // in `string_lit_row_declares_the_interpolation_child`.
                if k == Java::MultilineStringLiteral as u16 {
                    continue;
                }
                assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
            }
        }
    }

    // No gate: this reads the declared slices, which exist in every
    // build, and parses nothing. All three tables are listed here, so
    // the check does not depend on which grammars a build enables.
    #[test]
    fn declared_owner_and_payer_lists_are_disjoint_per_role() {
        use crate::roles::{elixir, python};
        for (role, owners, payers) in [
            ("java comparison", comparison::OWNERS, comparison::PAYERS),
            ("java exit", exit::OWNERS, exit::PAYERS),
            ("java string_lit", string_lit::OWNERS, string_lit::PAYERS),
            ("java func_opener", func_opener::OWNERS, func_opener::PAYERS),
            ("java container", container::OWNERS, container::PAYERS),
            ("java closure", closure::OWNERS, closure::PAYERS),
            ("java call", call::OWNERS, call::PAYERS),
            ("java guard", guard::OWNERS, guard::PAYERS),
            (
                "python comparison",
                python::comparison::OWNERS,
                python::comparison::PAYERS,
            ),
            ("python exit", python::exit::OWNERS, python::exit::PAYERS),
            (
                "python string_lit",
                python::string_lit::OWNERS,
                python::string_lit::PAYERS,
            ),
            (
                "python closure",
                python::closure::OWNERS,
                python::closure::PAYERS,
            ),
            (
                "python func_opener",
                python::func_opener::OWNERS,
                python::func_opener::PAYERS,
            ),
            (
                "python container",
                python::container::OWNERS,
                python::container::PAYERS,
            ),
            ("python call", python::call::OWNERS, python::call::PAYERS),
            (
                "elixir func_opener",
                elixir::func_opener::OWNERS,
                elixir::func_opener::PAYERS,
            ),
            (
                "elixir container",
                elixir::container::OWNERS,
                elixir::container::PAYERS,
            ),
            (
                "elixir closure",
                elixir::closure::OWNERS,
                elixir::closure::PAYERS,
            ),
            ("elixir call", elixir::call::OWNERS, elixir::call::PAYERS),
            (
                "elixir string_lit",
                elixir::string_lit::OWNERS,
                elixir::string_lit::PAYERS,
            ),
        ] {
            assert!(
                !crate::roles::kinds_overlap(owners, payers),
                "{role}: declared owners and payers overlap"
            );
        }
        assert_eq!(guard::PAYERS, &[] as &[u16]);
        assert_eq!((guard::PAYS_NEVER, comparison::PAYS_NEVER), (false, false));
        assert_eq!(
            comparison::EXCLUDED
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>(),
            [Java::QMARK as u16]
        );
    }

    // The owner is the `binary_expression`, the payer its `<` token;
    // `List<T>` emits the same token where nothing owns or pays. The
    // `instanceof` test is owned by its expression node and paid by its
    // keyword, one per test. The `&&` chain is a `binary_expression`
    // too, and the gate keeps it out: it is `ShortCircuit`'s, not a
    // comparison. A `?` reaches neither.
    #[cfg(feature = "java")]
    #[test]
    fn comparison_row_separates_the_owner_from_its_token() {
        let source = b"class A {\n  java.util.List<String> l;\n  boolean f(Object x, int a, int b) {\n    return x instanceof String s && a < b ? a == b : false;\n  }\n}\n";
        let seen = comparison_occurrences(source);
        let expect = [
            (Java::LT, "<", false, false, false),
            (Java::GT, ">", false, false, false),
            (
                Java::BinaryExpression,
                "x instanceof String s && a < b",
                false,
                false,
                false,
            ),
            (
                Java::InstanceofExpression,
                "x instanceof String s",
                true,
                false,
                true,
            ),
            (Java::Instanceof, "instanceof", false, true, false),
            (Java::BinaryExpression, "a < b", true, false, true),
            (Java::LT, "<", false, true, false),
            (Java::QMARK, "?", false, false, false),
            (Java::BinaryExpression, "a == b", true, false, true),
            (Java::EQEQ, "==", false, true, false),
        ];
        let want: Vec<_> = expect
            .into_iter()
            .map(|(k, t, o, l, w)| (k, t.to_owned(), o, l, w))
            .collect();
        assert_eq!(seen, want);
    }

    // A fixture only the `<` / `>` gate can decide: no equality token, no
    // `instanceof`, so the comparison count is exactly the gate's answer.
    #[cfg(feature = "java")]
    #[test]
    fn angle_tokens_pay_only_under_a_binary_expression() {
        let source = b"class A { java.util.Map<String, java.util.List<Integer>> m; boolean f(int a) { return a > 1; } }\n";
        let paying: Vec<_> = comparison_occurrences(source)
            .into_iter()
            .filter(|(k, ..)| matches!(k, Java::LT | Java::GT))
            .map(|(_, text, _, leaf, _)| (text, leaf))
            .collect();
        assert_eq!(
            paying,
            [
                ("<".to_owned(), false),
                ("<".to_owned(), false),
                (">".to_owned(), false),
                (">".to_owned(), false),
                (">".to_owned(), true),
            ]
        );
    }

    // A `binary_expression` projects through its `operator` field; an
    // `instanceof_expression` has no such field, so the row's fallback
    // finds the `instanceof` keyword among its children — in the plain
    // test and the Java 16 pattern form alike.
    #[cfg(feature = "java")]
    #[test]
    fn projection_lands_on_each_owners_paying_child() {
        let source = b"class A { boolean f(int a, int b, Object o) { return a <= b && o instanceof String && o instanceof String s; } }\n";
        let mut projected = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if comparison::owns(node, source, Ancestors::known(chain)) {
                let op = comparison::project(node).expect("every owner projects to a payer");
                projected.push((op.kind_id(), op.utf8_text(source).map(str::to_owned)));
            }
        });
        assert_eq!(
            projected,
            [
                (Java::LTEQ as u16, Some("<=".to_owned())),
                (Java::Instanceof as u16, Some("instanceof".to_owned())),
                (Java::Instanceof as u16, Some("instanceof".to_owned())),
            ]
        );
    }

    // The template's `string_literal` wraps a `string_interpolation`;
    // the plain literal beside it does not. Only the row's `WRAPS` set
    // tells them apart — the declaration `Getter::string_operand_type`
    // reads in `getter/java.rs` (#1703).
    #[cfg(feature = "java")]
    #[test]
    fn string_lit_row_declares_the_interpolation_child() {
        let source = b"class A { String s = STR.\"Hi \\{name}!\"; String t = \"plain\"; }\n";
        let mut literals = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            if string_lit::owns(node, source, Ancestors::known(chain)) {
                literals.push((
                    node.utf8_text(source).map(str::to_owned),
                    node.wraps_any(string_lit::WRAPS),
                ));
            }
        });
        assert_eq!(
            literals,
            [
                (Some("\"Hi \\{name}!\"".to_owned()), true),
                (Some("\"plain\"".to_owned()), false),
            ]
        );
        assert_eq!(string_lit::WRAPS, &[Java::StringInterpolation as u16]);
        // Drift marker: the hidden `_multiline_string_literal` rule is
        // never emitted at the pinned grammar (grammar-dispatch §2).
        let parser = JavaParser::new(
            b"class A { String b = \"\"\"\n  text\n  \"\"\"; }\n".to_vec(),
            Path::new("a.java"),
            None,
        );
        assert!(!ast_has_kind_id(
            &parser,
            Java::MultilineStringLiteral as u16
        ));
    }

    // One `opens_space` for the whole table, read from two rows: the
    // anonymous class opens a `Class` only with a body; `new Foo()`
    // opens nothing; the lambda is a closure that opens nothing.
    #[cfg(feature = "java")]
    #[test]
    fn opens_space_folds_func_opener_and_container_without_restating_kinds() {
        let source = b"class A {\n  static { }\n  Runnable r = () -> { };\n  Object o = new Object() { };\n  Object p = new Object();\n}\n";
        let mut spaces = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = opens_space(node, source, ancestors);
            let closure = closure::owns(node, source, ancestors);
            if kind != SpaceKind::Unknown || closure {
                spaces.push((node.kind(), kind, closure));
            }
        });
        assert_eq!(
            spaces,
            [
                ("program", SpaceKind::Unit, false),
                ("class_declaration", SpaceKind::Class, false),
                ("static_initializer", SpaceKind::Function, false),
                ("lambda_expression", SpaceKind::Unknown, true),
                ("object_creation_expression", SpaceKind::Class, false),
            ]
        );
        assert!(!func_opener::OWNERS.contains(&(Java::LambdaExpression as u16)));
        assert!(container::OWNERS.contains(&(Java::ObjectCreationExpression as u16)));
    }

    // The guard node is the one paying node (#1695): cyclomatic counts
    // it and ABC walks its expression; its `when` keyword pays nothing.
    #[cfg(feature = "java")]
    #[test]
    fn guard_row_owns_the_guard_and_pays_at_it() {
        let source = b"class A { int f(Object o) { return switch (o) { case Integer i when i > 0 -> 1; default -> 0; }; } }\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if guard::owns(node, source, ancestors) {
                owned.push(text());
            }
            if guard::pays_at_leaf(node, source, ancestors) {
                paid.push(text());
            }
        });
        assert_eq!(owned, ["when i > 0"]);
        assert_eq!(paid, owned);
    }

    // A row with no `pays` list pays at its owners under both
    // projections, whichever the consumer reads.
    #[cfg(feature = "java")]
    #[test]
    fn plain_rows_answer_both_projections_the_same_way() {
        let source = b"class A { int f(Object o) { return switch (o) { case Integer i when i > 0 -> 1; default -> 0; }; } }\n";
        let mut exits = Vec::new();
        let mut guards = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if node.kind_id() == Java::ReturnStatement as u16 {
                exits.push((
                    exit::owns(node, source, ancestors),
                    exit::pays_at_wrapper(node, source, ancestors),
                    exit::pays_at_leaf(node, source, ancestors),
                ));
            }
            if node.kind_id() == Java::Guard as u16 {
                guards.push((
                    guard::owns(node, source, ancestors),
                    guard::pays_at_wrapper(node, source, ancestors),
                    guard::pays_at_leaf(node, source, ancestors),
                ));
            }
        });
        assert_eq!(exits, [(true, true, true)]);
        assert_eq!(guards, [(true, true, true)]);
    }
}
