//! Java's role rows: one row of each shape the macro accepts, so the
//! shapes are proven against a real grammar before any consumer reads
//! them (#1691). The Halstead keyword rows are read by `getter/java.rs`
//! (#1693); the rest restate what `checker/java.rs`, `getter/java.rs`
//! and the root crate's per-language Java dispatch arms currently hold,
//! and those stay the live classifiers until their package lands.

use crate::checker::java_anonymous_class_body;
use crate::role_table;
use crate::{Ancestors, Java, Node};

/// Whether an `object_creation_expression` carries a `class_body`, which
/// makes it an anonymous class rather than a plain `new Foo()` (#463).
fn has_class_body<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    java_anonymous_class_body(node).is_some()
}

/// Whether a `static` qualifies a declaration rather than a directive:
/// the `static` of `import static` and `requires static` bills nothing,
/// because its own keyword — `import`, `requires` — bills nothing here.
/// Java and Groovy bill no directive keyword at all (`package`,
/// `import`, `module`, `requires`, `exports`, `transitive`), measured
/// on a fixture of each; C# bills `using` and so `using static` in
/// full. The two readings are each consistent and #1600 keeps both,
/// recording C# as the outlier in the book's Halstead rules: dropping
/// this guard would bill one directive word and no other.
fn not_in_a_directive<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent(node).is_some_and(|p| {
        matches!(
            p.kind_id().into(),
            Java::ImportDeclaration | Java::RequiresModifier
        )
    })
}

/// Whether a `class` keyword declares a class rather than ending a
/// class literal (`C.class`), where it is the operand the `.` acts on:
/// JLS §15.8.2 makes the literal a primary expression, not a literal
/// (§3.10), so its parts bill as they would anywhere else, as Kotlin's
/// and PHP's `Foo::class` and Groovy's `Foo.class` bill theirs (#1566).
fn not_a_class_literal<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent_has_kind(node, Java::ClassLiteral as u16)
}

role_table! {
    lang: Java;

    // Plain: three statement kinds, no token, no gate.
    Exit { owns [ReturnStatement, ThrowStatement, YieldStatement] }

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

    // Mapping. Every function opener is a `Function` space. The three
    // `callable` owners are what `is_func` answers — a method, a
    // constructor, and a record's compact constructor (#1160), each a
    // function someone names at a call site. `static { … }` is not:
    // it opens a space (#1184) and is nobody's callee, so it carries no
    // `callable` and `nom` / `nargs` / `npm` never see it as a method.
    // The lambda is *not* here: the walk promotes on
    // `promotes_to_func_space_with_code`, and Java's `is_func` and
    // `is_func_space` both leave `lambda_expression` out, so no lambda
    // opens a space today — even though the byte-less `get_space_kind`
    // labelled one `Function` before #1696. The row records the walk's
    // answer, and the parity test below pins it against the walk's
    // predicate.
    FuncOpener {
        owns [
            MethodDeclaration(callable opens Function),
            ConstructorDeclaration(callable opens Function),
            CompactConstructorDeclaration(callable opens Function),
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
            ObjectCreationExpression(opens Class gated_by has_class_body),
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
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    // Every modifier, billed as C# and Kotlin bill theirs (#1565). Each
    // is its keyword leaf, one kind apiece; the `modifiers` wrapper
    // stays unlisted, so a modifier bills once and an annotation inside
    // it adds only its own tokens. `default` is not here: it is one
    // kind for a `switch` label and an interface method's modifier, and
    // bills with the control keywords. `sealed` and `permits`
    // (`Permits2`; `Permits` is the clause) are contextual, and the
    // grammar parses each used as a name (`var sealed = 1`,
    // `int permits`) as an `identifier`, so they need no gate.
    Modifier {
        owns [
            Public, Private, Protected, Static(gated_by not_in_a_directive), Abstract, Final,
            Native, Strictfp, Synchronized, Transient, Volatile, Sealed, NonDASHsealed, Permits2,
        ]
    }
    // The type-declaration keywords, billed as C# bills its own
    // (#1552). Only the leaves: no `*_declaration` wrapper is listed, so
    // a declaration bills its keyword once.
    DeclKeyword {
        owns [Class(gated_by not_a_class_literal), Interface, Enum, Record, ATinterface]
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
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Java documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
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
    /// `(is_func, is_closure, promoted, SpaceKind, is_member)`: the five
    /// answers the space-tree and member consumers read of one node.
    #[cfg(feature = "java")]
    type Answers = (bool, bool, bool, SpaceKind, bool);

    #[cfg(feature = "java")]
    #[test]
    #[allow(clippy::too_many_lines)]
    fn classifiers_read_the_rows_and_the_rows_say_what_the_walk_did() {
        let source = b"enum E { A; int f; }\nrecord R(int a) { R { } int g; }\ninterface I { int K = 1; void im(); }\n@interface An { int v() default 1; int C = 2; }\nclass C {\n  static { }\n  C() { }\n  String s = \"x\";\n  void m() { Runnable r = () -> m(); Object o = new Object() { }; Object p = new Object(); throw new RuntimeException(); }\n  int y(int s) { return switch (s) { default -> { yield 1; } }; }\n}\n";
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
        let mut seen_kinds = std::collections::BTreeSet::new();
        let mut anonymous = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            let promoted = JavaCode::promotes_to_func_space_with_code(node, source, ancestors);
            let got = (
                JavaCode::is_func(node, ancestors),
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
            } else {
                let want = expected.iter().find(|(k, _)| *k == kind).map_or(
                    (false, false, false, SpaceKind::Unknown, false),
                    |(_, v)| *v,
                );
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
            // The byte-less pair is the kind table: the rows read no
            // chain and no bytes, so it agrees with the `_with_code` pair
            // on every node.
            assert_eq!(
                (
                    JavaCode::is_func_space(node),
                    JavaCode::get_space_kind(node)
                ),
                (promoted, got.3),
                "{kind}: the byte-less spellings are the kind table"
            );
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
            [
                (false, false, true, SpaceKind::Class, false),
                (false, false, false, SpaceKind::Unknown, false),
                (false, false, false, SpaceKind::Unknown, false),
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
