//! `Checker` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{java_doc, node_bytes};

/// A declaration javac attaches a doc comment to, the package aside.
fn java_declaration(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Java::ModuleDeclaration
            | Java::ClassDeclaration
            | Java::InterfaceDeclaration
            | Java::EnumDeclaration
            | Java::RecordDeclaration
            | Java::AnnotationTypeDeclaration
            | Java::MethodDeclaration
            | Java::ConstructorDeclaration
            | Java::CompactConstructorDeclaration
            | Java::FieldDeclaration
            | Java::ConstantDeclaration
            | Java::AnnotationTypeElementDeclaration
            | Java::EnumConstant
    )
}

/// Whether two `///` doc comments on consecutive rows are one Markdown
/// doc comment, as javac's scanner lexes them.
fn java_markdown_lines_join(prev: &Node, next: &Node, code: &[u8]) -> bool {
    let line = |node: &Node| node_bytes(node, code).starts_with(b"///");
    line(prev) && line(next) && prev.end_row().saturating_add(1) == next.start_row()
}

impl Checker for JavaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Java::LineComment || node.kind_id() == Java::BlockComment
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        Self::is_comment(node) && java_doc(node_bytes(node, code))
    }

    // javac's `Token::docComment` gives a declaration the last doc comment
    // before its first token, its annotations and modifiers included, and
    // its dangling-doc-comments lint names every earlier one; blank rows
    // and plain comments in between do not matter (#1613).
    // The scanner lexes `///` lines on consecutive rows as one doc comment
    // (`JavaTokenizer`, JEP 467).
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding::new(KeptDocs::Last {
        joins: java_markdown_lines_join,
    }));

    // What javac attaches a doc comment to (`JavacParser`'s `attach`
    // calls): a type, a member, an enum constant and a module. An import
    // or a statement takes none, and an initializer's is dropped
    // (`ignoreDanglingComments`). The package declaration takes the
    // file's first doc comment, but javadoc reads a package's only from
    // `package-info.java`, and javac's lint calls it dangling once a
    // declaration follows; a file with none after its package is that
    // file's shape. The scan stops at the next package too, so a file of
    // repeated packages — invalid, but parsed without error — is read in
    // disjoint stretches rather than to its end from each one (#1446).
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        if node.kind_id() == Java::PackageDeclaration {
            return !ancestors
                .siblings_after(node)
                .find(|n| java_declaration(n) || n.kind_id() == Java::PackageDeclaration)
                .is_some_and(|n| java_declaration(&n));
        }
        java_declaration(node)
    }

    // A `static { … }` initializer opens a function space (#1184) without
    // being `is_func`, and an instance initializer is a bare `block` in a
    // class or enum body; javadoc reads a doc comment in either no more
    // than one in a method body. Every `block` and `switch_block` is
    // executable code — a field initializer's `switch` expression holds
    // one outside any method — so the kind alone decides.
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        _below: &Node<'a>,
    ) -> bool {
        Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            || matches!(
                ancestor.kind_id().into(),
                Java::StaticInitializer | Java::Block | Java::SwitchBlock
            )
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
