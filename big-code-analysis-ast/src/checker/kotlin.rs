//! `Checker` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::never_joins;

impl Checker for KotlinCode {
    // The `Comment` and `DocComment` rows of `roles/kotlin.rs` (#1699):
    // KDoc has no line form, so only the block comment can qualify.
    impl_comment_from_rows!(kotlin);
    impl_doc_comment_from_row!(kotlin);

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
                let kind = parent.kind_id();
                // The declarations are the `DocTarget` row's kind table;
                // beside them the primary constructor, the accessors and
                // the modifier list, which hold a KDoc in a signature
                // without being targets themselves.
                crate::roles::kotlin::doc_target::lists_kind(kind)
                    // role-bypass: allow(FuncOpener, HoldsDocInBody) — a KDoc *inside* one
                    // of these is in its signature; where a declaration's children end
                    // is a binding question, not membership
                    || matches!(
                        kind.into(),
                        Kotlin::PrimaryConstructor
                            | Kotlin::Getter
                            | Kotlin::Setter
                            | Kotlin::Modifiers
                    )
            })
        },
        ..DocBinding::new(KeptDocs::Last { joins: never_joins })
    });

    // The `DocTarget` row of `roles/kotlin.rs` (#1699).
    impl_doc_target_from_row!(kotlin);

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
        // role-bypass: allow(Closure) — the lambda is all body, decided
        // without the body-child test below; a Checker refinement over
        // the `Closure` row, not membership
        if ancestor.kind_id() == Kotlin::LambdaLiteral {
            return true;
        }
        (Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            || holds_doc_in_body_kind!(kotlin, ancestor, ancestors))
            && matches!(below.kind_id().into(), Kotlin::FunctionBody | Kotlin::Block)
    }

    // The space tree is declared in `roles/kotlin.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain (the
    // interface gate reads the node's children), so the byte-less
    // spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
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

    // `Call` and `StringLit` are the rows of `roles/kotlin.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(kotlin, call, is_call, is_call_with_code);
    impl_row_predicates!(kotlin, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LPAREN | Kotlin::COMMA | Kotlin::RPAREN
        )
    }

    // The `ElseIf` row of `roles/kotlin.rs`: an `if_expression` whose
    // previous sibling is the bare `else` token (no clause wrapper), read
    // off the caller's chain. The row reads no bytes (#1697).
    impl_else_if_from_row!(kotlin);

    // `reified` is the one modifier the grammar spells as an aliased
    // named kind, `reification_modifier`, rather than a keyword token.
    // Keying it by `kind_id` would make `bca ops` list an operator
    // named `reification_modifier`, which appears nowhere in the
    // source; keying it by text lists `reified` (#1558). It has no
    // other kind, so nothing splits across the two `n1` maps. Kotlin
    // has no primitive kinds (the row is `absent`), so `is_primitive`
    // keeps the trait's `false` and this hook is the whole key selection.
    #[inline]
    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        // role-bypass: allow(Modifier) — key selection after the row has
        // classified the node: the one modifier keyed by text
        node.kind_id() == Kotlin::ReificationModifier
    }
}
