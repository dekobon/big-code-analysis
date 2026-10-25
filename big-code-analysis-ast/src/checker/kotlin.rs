//! `Checker` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{kotlin_doc, never_joins, node_bytes};

impl Checker for KotlinCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LineComment | Kotlin::BlockComment
        )
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        // KDoc has no line form, so only a `BlockComment` can qualify.
        node.kind_id() == Kotlin::BlockComment && kotlin_doc(node_bytes(node, code))
    }

    // Kotlin's parser closes each declaration with
    // `PrecedingDocCommentsBinder`, which pulls in the last KDoc of the
    // comments before it and leaves the earlier ones outside, whatever
    // plain comments or blank rows lie between (#1613).
    //
    // tree-sitter-kotlin parses an annotation with arguments
    // (`@Suppress("x")`) after a top-level declaration as an expression
    // beside the declaration it modifies, which Kotlin's parser reads as
    // that declaration's modifier, so the stretch reads past it.
    //
    // A KDoc among a declaration's own children or its modifier list's —
    // after an annotation, before a body or an accessor — is the one
    // `findDocComment` reads there, decided as before (#1612). Every other
    // KDoc binds forward, so one nested deeper — in a call's or an
    // annotation's arguments, in a type parameter list — documents a
    // declaration directly after it or nothing, as javac's does.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        skips: |node, _| node.kind_id() == Kotlin::AnnotatedExpression,
        forward: |comment, _, ancestors| {
            !ancestors.parent(comment).is_some_and(|parent| {
                // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
                // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
                matches!(
                    parent.kind_id().into(),
                    Kotlin::ClassDeclaration
                        | Kotlin::ObjectDeclaration
                        | Kotlin::CompanionObject
                        | Kotlin::FunctionDeclaration
                        | Kotlin::PropertyDeclaration
                        | Kotlin::TypeAlias
                        | Kotlin::SecondaryConstructor
                        | Kotlin::PrimaryConstructor
                        | Kotlin::EnumEntry
                        | Kotlin::Parameter
                        | Kotlin::ClassParameter
                        | Kotlin::Getter
                        | Kotlin::Setter
                        | Kotlin::Modifiers
                )
            })
        },
        ..DocBinding::new(KeptDocs::Last { joins: never_joins })
    });

    // The declarations Dokka documents. The package directive binds no
    // comment (`BindFirstShebangWithWhitespaceOnly`), an import none, and
    // an `init` block has no page to carry one.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], _: Ancestors<'a, '_>) -> bool {
        // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not
        // membership: what a doc comment binds to is `DocTarget`'s own row (R09)
        matches!(
            node.kind_id().into(),
            Kotlin::ClassDeclaration
                | Kotlin::ObjectDeclaration
                | Kotlin::CompanionObject
                | Kotlin::FunctionDeclaration
                | Kotlin::PropertyDeclaration
                | Kotlin::TypeAlias
                | Kotlin::SecondaryConstructor
                | Kotlin::EnumEntry
                | Kotlin::Parameter
                | Kotlin::ClassParameter
        )
    }

    // Kotlin's KDoc lookup reads past a declaration's annotations and
    // modifiers, so a KDoc after `@Deprecated(…)` documents the function:
    // only the body is in-body, and a bodiless declaration (an interface
    // or `abstract` fun, a delegating secondary constructor) holds none.
    // No Kotlin kind has a `body` field — the body is the `function_body`
    // or `block` child, so the body holds the comment when `below` is one;
    // a search of the children would rescan every KDoc in the signature
    // for each of them (#1612). A lambda literal has no such child: it is all
    // body, decided by its kind so that a lambda holding thousands of
    // statements is not rescanned for every comment in it (#1597).
    // Accessors and `init` blocks open function spaces without being
    // `is_func` (#1184), and Dokka reads a KDoc in them no more than one
    // in a method.
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        below: &Node<'a>,
    ) -> bool {
        // role-bypass: allow(Closure) — where a comment sits in a body, not
        // whether the node opens a space
        if ancestor.kind_id() == Kotlin::LambdaLiteral {
            return true;
        }
        (Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            || matches!(
                ancestor.kind_id().into(),
                // role-bypass: allow(FuncOpener) — where a comment sits in a body, not
                // whether the node opens a space
                Kotlin::Getter | Kotlin::Setter | Kotlin::AnonymousInitializer
            ))
            && matches!(below.kind_id().into(), Kotlin::FunctionBody | Kotlin::Block)
    }

    // The space tree is declared in `roles/kotlin.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain (the interface gate reads the node's children), so the byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::unknown())
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::kotlin::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::kotlin::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::kotlin::closure::owns(node, &[], ancestors)
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Kotlin::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LPAREN | Kotlin::COMMA | Kotlin::RPAREN
        )
    }

    impl_simple_is_string!(Kotlin, StringLiteral, MultilineStringLiteral);

    // tree-sitter-kotlin models `else if` as an `else` keyword sibling
    // followed by an `if_expression` (not an `if_statement`, and no
    // wrapping clause node).
    impl_is_else_if_prev_sibling!(Kotlin, IfExpression, Else);

    // `reified` is the one modifier the grammar spells as an aliased
    // named kind, `reification_modifier`, rather than a keyword token.
    // Keying it by `kind_id` would make `bca ops` list an operator
    // named `reification_modifier`, which appears nowhere in the
    // source; keying it by text lists `reified` (#1558). It has no
    // other kind, so nothing splits across the two `n1` maps.
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // role-bypass: allow(Modifier) — key selection after the row has
        // classified the node: the one modifier keyed by text
        node.kind_id() == Kotlin::ReificationModifier
    }
}
