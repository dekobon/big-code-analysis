//! Java's role rows: one row of each shape the macro accepts, so the
//! shapes are proven against a real grammar before any consumer reads
//! them (#1691). The Halstead keyword rows are read by `getter/java.rs`
//! (#1693); the rest restate what `checker/java.rs`, `getter/java.rs`
//! and the root crate's per-language Java dispatch arms currently hold,
//! and those stay the live classifiers until their package lands.

use crate::checker::java_anonymous_class_body;
use crate::role_table;
use crate::{Ancestors, Java, Node};

/// Whether a `binary_expression`'s `operator` field is an equality or
/// ordering token — one of the `Comparison` payers. The same node kind
/// carries `&&` / `||`, which belong to `ShortCircuit`, and the
/// arithmetic operators, which belong to no role here; the field is the
/// one place the grammar says which.
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

/// Whether a bare `<` / `>` token is the operator of a
/// `binary_expression`. tree-sitter-java emits the same tokens from
/// `type_arguments` and `type_parameters`, where they are type syntax
/// and pay nothing (#1274).
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Java::BinaryExpression as u16)
}

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
    // keyword pays: the grammar spells that keyword in no other
    // production, so one pays per test (#1461). `?` is excluded here
    // rather than left unmentioned: a ternary's `?` is a `Ternary`
    // matter, a wildcard bound's is type syntax, and neither is a
    // comparison.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            InstanceofExpression,
        ]
        pays [
            EQEQ,
            BANGEQ,
            LTEQ,
            GTEQ,
            LT gated_by under_binary_expression,
            GT gated_by under_binary_expression,
            Instanceof,
        ]
        project "operator"
        excludes [QMARK "a `?` is a ternary's decision or a wildcard bound; `Ternary` owns it"]
    }

    // The same `binary_expression` owns an `&&` / `||` chain when its
    // `operator` field holds one; the two tokens occur in no other
    // production, so they pay ungated. `a && b || c` is a left-nested
    // chain of these nodes, each owning one operator.
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
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

    // Mapping. Every function opener is a `Function` space. `static { … }`
    // is `is_func_space` rather than `is_func` today (#1184), so it opens
    // a space without being a callable. The lambda is *not* here: the
    // walk promotes on `promotes_to_func_space_with_code`, and Java's
    // `is_func` and `is_func_space` both leave `lambda_expression` out,
    // so no lambda opens a space today — even though `get_space_kind`
    // would label one `Function` if asked. That is a live disagreement
    // between the two halves of the space tree (grammar-dispatch §7,
    // "the pair that fails quietly"), reachable only by a caller that
    // asks the getter about a node the walk never promoted; the row
    // records the walk's answer, and the parity test below pins it
    // against the walk's predicate rather than the getter's.
    FuncOpener {
        owns [
            MethodDeclaration(opens Function),
            ConstructorDeclaration(opens Function),
            CompactConstructorDeclaration(opens Function),
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

    // `pays never`, on a stand-in. A `case … when guard ->` arm's guard
    // is an owner with no token of its own: the `when` keyword is not
    // counted anywhere, and the guard's expression pays through its own
    // comparison. No consumer reads this row yet; it exists to prove the
    // shape expands.
    Guard { owns [Guard] pays never }

    NullSafe { absent "Java has no optional-navigation or null-coalescing operator" }
    // JLS §15.22.2 makes `&` / `|` / `^` on boolean operands eager
    // logical operators, and the grammar emits them as the same
    // `binary_expression` as the integral forms, so the syntax alone
    // cannot tell them apart. That is a recognition boundary for the
    // package that declares the paying constructs, not an absence.
    EagerLogical { pending R05 }

    // Filled in by the packages that migrate their consumers.
    Ternary { pending R05 }
    CaseArm { pending R05 }
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
    Member { pending R06 }
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

    // The rows restate `checker/java.rs` and `getter/java.rs`; this pins
    // the restatement node by node, on a fixture holding every kind the
    // membership rows name, so a kind dropped from a row (or added to
    // the classifier alone) fails here rather than drifting silently
    // (lesson 35). The per-shape tests above do not cover membership:
    // dropping `YieldStatement` from `Exit` or `EnumDeclaration` from
    // `Container` failed none of them.
    #[cfg(feature = "java")]
    #[test]
    fn membership_rows_agree_with_the_live_classifiers() {
        let source = b"enum E { A; }\nrecord R(int a) { R { } }\ninterface I { }\n@interface An { }\nclass C {\n  static { }\n  C() { }\n  String s = \"x\";\n  void m() { Runnable r = () -> m(); Object o = new Object() { }; Object p = new Object(); throw new RuntimeException(); }\n  int y(int s) { return switch (s) { default -> { yield 1; } }; }\n}\n";
        let mut seen_kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            assert_eq!(
                call::owns(node, source, ancestors),
                JavaCode::is_call_with_code(node, source, ancestors),
                "{kind}: Call"
            );
            assert_eq!(
                string_lit::owns(node, source, ancestors),
                JavaCode::is_string_with_code(node, source, ancestors),
                "{kind}: StringLit"
            );
            assert_eq!(
                closure::owns(node, source, ancestors),
                JavaCode::is_closure(node, ancestors),
                "{kind}: Closure"
            );
            // The space tree: `FuncOpener` ∪ `Container` owners are what
            // the walk promotes — its predicate, unaugmented — and
            // `opens_space` is the kind it labels a promoted node with.
            // The getter is asked only about promoted nodes, because for
            // a lambda it answers `Function` while the walk opens
            // nothing; the `FuncOpener` row says why.
            let promoted = func_opener::owns(node, source, ancestors)
                || container::owns(node, source, ancestors);
            assert_eq!(
                promoted,
                JavaCode::promotes_to_func_space_with_code(node, source, ancestors),
                "{kind}: space promotion"
            );
            let labelled = opens_space(node, source, ancestors);
            assert_eq!(
                labelled != SpaceKind::Unknown,
                promoted,
                "{kind}: opens_space vs promotion"
            );
            if promoted {
                assert_eq!(
                    labelled,
                    JavaCode::get_space_kind_with_code(node, source, ancestors),
                    "{kind}: SpaceKind"
                );
            }
            if kind == "lambda_expression" {
                assert_eq!(
                    JavaCode::get_space_kind_with_code(node, source, ancestors),
                    SpaceKind::Function,
                    "the getter's lambda answer the walk never reads"
                );
            }
            // `Exit` has no classifier in this crate — its oracle is the
            // root's exit arm — so the fixture's three exit statements
            // are pinned by kind rather than by comparison.
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
        // Fixture guard: every kind the membership rows name was walked.
        for kinds in [
            exit::OWNERS,
            call::OWNERS,
            string_lit::OWNERS,
            closure::OWNERS,
            func_opener::OWNERS,
            container::OWNERS,
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
        assert_eq!((guard::PAYS_NEVER, comparison::PAYS_NEVER), (true, false));
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

    #[cfg(feature = "java")]
    #[test]
    fn projection_lands_on_the_operator_field() {
        let source = b"class A { boolean f(int a, int b) { return a <= b; } }\n";
        let mut projected = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, _| {
            if node.kind_id() == Java::BinaryExpression as u16 {
                let op = comparison::project(node).expect("binary_expression has an operator");
                projected.push((op.kind_id(), op.utf8_text(source).map(str::to_owned)));
            }
        });
        assert_eq!(projected, [(Java::LTEQ as u16, Some("<=".to_owned()))]);
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

    #[cfg(feature = "java")]
    #[test]
    fn guard_row_owns_the_guard_and_pays_nowhere() {
        let source = b"class A { int f(Object o) { return switch (o) { case Integer i when i > 0 -> 1; default -> 0; }; } }\n";
        let mut guards = 0;
        let mut paid = 0;
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            guards += usize::from(guard::owns(node, source, ancestors));
            paid += usize::from(guard::pays_at_leaf(node, source, ancestors));
        });
        assert_eq!((guards, paid), (1, 0));
    }

    // A row with no `pays` list pays at its owners under both
    // projections; a `pays never` row pays under neither, even though
    // it owns the node.
    #[cfg(feature = "java")]
    #[test]
    fn plain_and_never_rows_answer_both_projections_the_same_way() {
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
        assert_eq!(guards, [(true, false, false)]);
    }
}
