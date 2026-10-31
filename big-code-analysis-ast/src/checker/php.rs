//! `Checker` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{never_joins, node_bytes, phpdoc};

/// A top-level node phpDocumentor's `createFileDocBlock` passes over
/// looking for the file's first element: `Declare_` and `InlineHTML`, which
/// tree-sitter-php spells as a `declare` statement and the `<?php` / `?>`
/// text around inline HTML.
fn php_file_preamble(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Php::PhpTag | Php::Text | Php::TextInterpolation | Php::DeclareStatement
    )
}

/// Whether `comment` is phpDocumentor's file DocBlock (#1613), which
/// documents the file wherever it sits. `createFileDocBlock` gathers the
/// comments before the first top-level node that is not a preamble; if that
/// node is not a class, function, constant, interface or trait, the first
/// DocBlock among them is the file's, and otherwise it is only when exactly
/// two DocBlocks precede it. The backward test stops at the previous
/// DocBlock, so only the file's first one scans forward.
fn php_file_docblock<'a>(comment: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    let preamble_or_plain = |n: Node| {
        php_file_preamble(&n) || (PhpCode::is_comment(&n) && !PhpCode::is_doc_comment(&n, code))
    };
    if !ancestors.parent_has_kind(comment, Php::Program as u16)
        || !ancestors.siblings_before(comment).all(preamble_or_plain)
    {
        return false;
    }
    let mut docs = 1_usize;
    for node in ancestors.siblings_after(comment) {
        if PhpCode::is_doc_comment(&node, code) {
            docs += 1;
        } else if !PhpCode::is_comment(&node) && !php_file_preamble(&node) {
            return docs == 2
                // role-bypass: allow(FuncOpener, Container, Member) — doc
                // attachment, not membership: what a file docblock precedes
                // is `DocTarget`'s own row (R09)
                || !matches!(
                    node.kind_id().into(),
                    Php::ClassDeclaration
                        | Php::FunctionDefinition
                        | Php::ConstDeclaration
                        | Php::ConstDeclaration2
                        | Php::InterfaceDeclaration
                        | Php::TraitDeclaration
                );
        }
    }
    true
}

impl Checker for PhpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Php::Comment
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        Self::is_comment(node) && phpdoc(node_bytes(node, code))
    }

    // phpDocumentor gives an element PHP-Parser's `getDocComment`: the
    // last DocBlock among the comments before the node's first token, so
    // plain comments in between are read past and only the last of several
    // DocBlocks documents it (#1613). A DocBlock among a declaration's own
    // children — after its `#[…]` attributes, before its body, on a
    // promoted parameter — is the one `ReflectionFunction::getDocComment`
    // reads, decided as before (#1612), as is the file DocBlock, which
    // documents the file rather than what follows. A property, a constant
    // and an enum case hold their `#[…]` attributes as children too, so
    // each is a declaration here like a method.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        forward: |comment, code, ancestors| {
            let in_signature = ancestors.parent(comment).is_some_and(|parent| {
                // role-bypass: allow(FuncOpener, Closure, Container, Member) — doc attachment,
                // not membership: what a doc comment binds to is `DocTarget`'s own row (R09)
                matches!(
                    parent.kind_id().into(),
                    Php::ClassDeclaration
                        | Php::InterfaceDeclaration
                        | Php::TraitDeclaration
                        | Php::EnumDeclaration
                        | Php::FunctionDefinition
                        | Php::MethodDeclaration
                        | Php::PropertyDeclaration
                        | Php::ConstDeclaration
                        | Php::ConstDeclaration2
                        | Php::EnumCase
                        | Php::AnonymousFunction
                        | Php::ArrowFunction
                        | Php::FormalParameters
                        | Php::PropertyPromotionParameter
                )
            });
            !in_signature && !php_file_docblock(comment, code, ancestors)
        },
        ..DocBinding::new(KeptDocs::Leading {
            hidden: |node, code| !Self::is_doc_comment(node, code),
            joins: never_joins,
            // PHP-Parser gives a node every comment since the token before
            // it, whatever row that token ends on.
            after_code: |_| true,
        })
    });

    // The structural elements phpDocumentor's reflection builds: a class,
    // interface, trait or enum, a function or method, a property, a class
    // or global constant and an enum case. `define()` is a call the
    // grammar parses as a statement, so a DocBlock above it stays a plain
    // comment, where phpDocumentor reads it as a constant's; a `use`, a
    // `namespace` or any other statement takes none.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], _: Ancestors<'a, '_>) -> bool {
        // role-bypass: allow(FuncOpener, Container, Member) — doc
        // attachment, not membership: what a doc comment binds to is
        // `DocTarget`'s own row (R09)
        matches!(
            node.kind_id().into(),
            Php::ClassDeclaration
                | Php::InterfaceDeclaration
                | Php::TraitDeclaration
                | Php::EnumDeclaration
                | Php::FunctionDefinition
                | Php::MethodDeclaration
                | Php::PropertyDeclaration
                | Php::ConstDeclaration
                | Php::ConstDeclaration2
                | Php::EnumCase
        )
    }

    // PHP's parser binds the last docblock before a function's name to it,
    // so one written after `#[...]` attributes still documents it, and a
    // promoted constructor property keeps its parameter's docblock
    // (`ReflectionProperty::getDocComment`). Only the body is in-body: the
    // `compound_statement` child, or an arrow function's expression, which
    // is its last child since tree-sitter moves trailing comments out of a
    // node — except before a zero-width MISSING body, which leaves the
    // comment itself ending where the arrow function does. Decided by
    // `below`'s kind and span, because the `body` field lookup walks every
    // comment in the signature (#1612).
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        _ancestors: Ancestors<'a, '_>,
        below: &Node<'a>,
    ) -> bool {
        // role-bypass: allow(FuncOpener, Closure) — where a comment sits in a
        // body, not whether the node opens a space
        match ancestor.kind_id().into() {
            Php::FunctionDefinition | Php::MethodDeclaration | Php::AnonymousFunction => {
                below.kind_id() == Php::CompoundStatement
            }
            Php::ArrowFunction => {
                below.end_byte() == ancestor.end_byte() && !Self::is_comment(below)
            }
            _ => false,
        }
    }

    // The space tree is declared in `roles/php.rs` (#1696): the
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
        crate::roles::php::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::php::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::php::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/php.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(php, call, is_call, is_call_with_code);
    impl_row_predicates!(php, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::LPAREN | Php::LPAREN2 | Php::COMMA | Php::RPAREN | Php::RPAREN2 | Php::DOTDOTDOT
        )
    }

    // The `ElseIf` row of `roles/php.rs` (#529, #1697): the one-word
    // `elseif` clause (both aliases) is the else-if itself, and the
    // two-word `else if` is an `if_statement` directly inside an
    // `else_clause` (either alias). The nested-`if` reading is what lets
    // cognitive suppress the nesting penalty while the wrapping clause
    // still scores its +1; the row reads no bytes, so the empty code is
    // exact.
    impl_else_if_from_row!(php);
}
