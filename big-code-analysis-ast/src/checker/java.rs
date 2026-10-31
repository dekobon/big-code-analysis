//! `Checker` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{java_doc, node_bytes};

/// A declaration javac attaches a doc comment to, the package aside.
fn java_declaration(node: &Node) -> bool {
    // role-bypass: allow(FuncOpener, Container, Member) — doc attachment,
    // not membership: what a doc comment binds to is `DocTarget`'s own
    // row (R09), which overlaps the space openers and the members
    // without being them.
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
                // role-bypass: allow(FuncOpener) — doc-comment context, not whether the node opens
                // a space (`HoldsDocInBody`'s own row, R09)
                Java::StaticInitializer | Java::Block | Java::SwitchBlock
            )
    }

    // The space tree is declared in `roles/java.rs` (#1696): the
    // `Container` row (class-like declarations, an anonymous class gated
    // on its body — #280, #463) and the `FuncOpener` row (the callables
    // and `static { … }`, #1184) are what the walk promotes;
    // `opens_space` labels them. The byte-less spellings here are the
    // legacy wrappers: Java's rows read no bytes and no chain, so they
    // answer identically with none.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::java::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // A record's compact constructor (`record R(int a) { R { … } }`, JLS
    // 8.10.4) is a `callable` owner of the `FuncOpener` row beside the
    // canonical constructor (#1160). Everything keyed on `is_func`
    // follows from the row: `Nom`, `NArgs`, `Npm`'s `direct_child_funcs`,
    // and the space tree that `Wmc` and `NExits` are attributed through.
    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::java::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::java::closure::owns(node, &[], ancestors)
    }

    /// `x -> x + 1` puts a bare `identifier` in the `parameters` field;
    /// `(x) -> …` and `(x, y) -> …` put an `inferred_parameters` list
    /// and `() -> …` a `formal_parameters` one (#1185).
    fn is_bare_param(node: &Node) -> bool {
        node.kind_id() == Java::Identifier as u16
    }

    // `Call` and `StringLit` are the rows of `roles/java.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(java, call, is_call, is_call_with_code);
    impl_row_predicates!(java, string_lit, is_string, is_string_with_code);

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

    // The `ElseIf` row of `roles/java.rs`: an `if_statement` whose
    // previous sibling is the bare `else` token (no clause wrapper), read
    // off the caller's chain. The row reads no bytes (`TEXT_GATED` is
    // pinned false), so the empty code is exact (#1697).
    impl_else_if_from_row!(java);
}
