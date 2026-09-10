//! `Checker` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for JavaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Java::LineComment || node.kind_id() == Java::BlockComment
    }

    // `EnumDeclaration`, `RecordDeclaration`, and `AnnotationTypeDeclaration`
    // are class-like declarations that can contain fields and methods,
    // so they open a class space alongside `ClassDeclaration` /
    // `InterfaceDeclaration` (issue #280). Without them, `Npa`/`Npm`/`Wmc`
    // never see their bodies as class scopes and silently produce zero
    // counts. Annotation types map to `Interface` in `get_space_kind`
    // (their elements are abstract methods at the bytecode level);
    // enums and records map to `Class`.
    //
    // An `object_creation_expression` carrying a `class_body` child is
    // an anonymous class (`new Runnable() { ... }`); it opens its own
    // Class space so its members are attributed to it, not the
    // enclosing method (#463). A plain `new Foo()` has no `class_body`
    // child and must not open a space, so the arm is gated on the
    // body's presence. This mirrors PHP's `AnonymousClass` handling and
    // brings Java to parity with PHP/C# anonymous forms.
    fn is_func_space(node: &Node) -> bool {
        if node.kind_id() == Java::ObjectCreationExpression as u16 {
            return java_anonymous_class_body(node).is_some();
        }
        matches!(
            node.kind_id().into(),
            Java::Program
                | Java::ClassDeclaration
                | Java::InterfaceDeclaration
                | Java::EnumDeclaration
                | Java::RecordDeclaration
                | Java::AnnotationTypeDeclaration
                // `static { … }` carries executable code — conventionally
                // the class's initialisation logic — but was referenced
                // nowhere outside the generated enum, so it opened no
                // space and its control flow was charged to the class
                // (#1184). `is_func_space` rather than `is_func`: it is
                // not a callable anyone names at a call site.
                | Java::StaticInitializer
        )
    }

    // A record's compact constructor (`record R(int a) { R { … } }`, JLS
    // 8.10.4) parses as its own `compact_constructor_declaration` kind
    // rather than as a `constructor_declaration`, so it has to be listed
    // separately. It is a constructor in every sense that matters here —
    // it has a body, it can be arbitrarily complex, and it is where
    // record validation conventionally lives — so it opens a function
    // space alongside the canonical spelling (#1160). Everything keyed on
    // `is_func` follows for free: `Nom`, `NArgs`, `Npm`'s
    // `direct_child_funcs`, and the space tree that `Wmc` and `NExits`
    // are attributed through.
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Java::MethodDeclaration
                | Java::ConstructorDeclaration
                | Java::CompactConstructorDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Java::LambdaExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Java::MethodInvocation
    }

    /// `x -> x + 1` puts a bare `identifier` in the `parameters` field;
    /// `(x) -> …` and `(x, y) -> …` put an `inferred_parameters` list
    /// and `() -> …` a `formal_parameters` one (#1185).
    fn is_bare_param(node: &Node) -> bool {
        node.kind_id() == Java::Identifier as u16
    }

    fn is_non_arg(node: &Node) -> bool {
        // Java's explicit receiver parameter (`void m(S this, int a)`, JLS
        // 8.4.1) parses as a `receiver_parameter` child of
        // `formal_parameters`, distinct from a real `formal_parameter`. It
        // binds `this`, not a value, so it is not a formal parameter and
        // must be excluded — matching Rust's `SelfParameter` (#457), Go's
        // `receiver` field, and C++'s implicit `this` (#470).
        matches!(
            node.kind_id().into(),
            Java::LPAREN | Java::COMMA | Java::RPAREN | Java::ReceiverParameter
        )
    }

    impl_simple_is_string!(Java, StringLiteral, MultilineStringLiteral);

    // tree-sitter-java models `else if` as an `Else` keyword token followed
    // by a nested `if_statement` (no wrapping `else_clause` node).
    impl_is_else_if_prev_sibling!(Java, IfStatement, Else);
}
