//! `Checker` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{java_doc, node_bytes};

impl Checker for GroovyCode {
    fn is_comment(node: &Node) -> bool {
        // `groovydoc_comment` is Groovy's `/** … */` Groovydoc kind, a
        // distinct grammar node (Groovy 150) from the `//` LineComment
        // and `/* */` BlockComment. The `Loc` cloc arm already counts it
        // (loc.rs); this restores parity so tokens/find agree too (#697).
        matches!(
            node.kind_id().into(),
            Groovy::LineComment | Groovy::BlockComment | Groovy::GroovydocComment
        )
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        // The grammar marks Groovydoc, but its `groovydoc_comment` also
        // takes the empty `/**/`, which documents nothing. Groovy's own
        // `GroovydocManager` reads any other comment opening `/**`, a
        // `/***` one included, exactly as javac does for Javadoc.
        node.kind_id() == Groovy::GroovydocComment && java_doc(node_bytes(node, code))
    }

    // `GroovydocManager::findDocCommentByNode` reads the last comment
    // token before a declaration and keeps it only if it opens `/**`, so
    // a plain comment in between detaches the Groovydoc; blank rows do
    // not. A package declaration resets nothing, so the file's first
    // declaration still reads a comment above `package` (#1613).
    // The lexer sends a block comment that code follows on its row to the
    // hidden channel (`ignoreMultiLineCommentConditionally`), where the
    // manager never sees it.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        skips: |node, _| node.kind_id() == Groovy::PackageDeclaration,
        ..DocBinding::new(KeptDocs::Nearest {
            hidden: |comment, next| {
                comment.kind_id() != Groovy::LineComment && next.start_row() == comment.end_row()
            },
        })
    });

    // The nodes `AstBuilder` hands to `GroovydocManager::handle`: a type,
    // a method or constructor, a field or property, an enum constant.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], _: Ancestors<'a, '_>) -> bool {
        // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
        // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
        matches!(
            node.kind_id().into(),
            Groovy::ClassDeclaration
                | Groovy::TraitDeclaration
                | Groovy::InterfaceDeclaration
                | Groovy::EnumDeclaration
                | Groovy::RecordDeclaration
                | Groovy::AnnotationTypeDeclaration
                | Groovy::MethodDeclaration
                | Groovy::ConstructorDeclaration
                | Groovy::FieldDeclaration
                | Groovy::EnumConstant
        )
    }

    // A `static { … }` initializer opens a function space without being
    // `is_func`, and Groovydoc reads a doc comment in it no more than one
    // in a method body.
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        _below: &Node<'a>,
    ) -> bool {
        Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            // role-bypass: allow(FuncOpener) — where a comment sits in a body, not
            // whether the node opens a space
            || ancestor.kind_id() == Groovy::StaticInitializer
    }

    // Mirrors `impl Checker for JavaCode` exactly: `EnumDeclaration`,
    // `RecordDeclaration`, and `AnnotationTypeDeclaration` open class
    // spaces so `Npa`/`Npm`/`Wmc` walk their bodies. Cross-language parity
    // with Java is the point — identical-shape sources must agree on
    // class-shaped metrics (issue #280).
    // The space tree is declared in `roles/groovy.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::groovy::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::groovy::func_opener::is_callable(node, &[], ancestors)
    }

    // A primitive type is a name kind spelled `int`, `void`, … (#1568),
    // which only the bytes can tell: the `Primitive` row's text gate.
    // The byte-less spelling answers the kind table alone, which is
    // every name — the limitation #1599 recorded — so the Halstead key
    // selection reads the row through `keys_operator_by_text`, and a
    // second consumer asking this of a plain name gets `true` only here.
    fn is_primitive(node: &Node) -> bool {
        crate::roles::groovy::primitive::OWNERS.contains(&node.kind_id())
    }

    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::groovy::primitive::owns(node, code, ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::groovy::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/groovy.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_call_row!(groovy);
    impl_string_lit_row!(groovy);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LPAREN | Groovy::COMMA | Groovy::RPAREN
        )
    }

    // The dekobon Groovy grammar models `if_statement` with the `else`
    // keyword token emitted inline followed by the inner `if_statement`
    // sibling — same shape as the prior amaanq grammar and Java.
    impl_is_else_if_prev_sibling!(Groovy, IfStatement, Else);
}
