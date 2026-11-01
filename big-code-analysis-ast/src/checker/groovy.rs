//! `Checker` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::node_bytes;

impl Checker for GroovyCode {
    // The `Comment` and `DocComment` rows of `roles/groovy.rs` (#1699):
    // `groovydoc_comment` sits on `DocComment` alone, so `is_comment`'s
    // union is what keeps the empty `/**/` a comment (#697).
    impl_comment_from_rows!(groovy);
    impl_doc_comment_from_row!(groovy);

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
                // role-bypass: allow(Comment) — which comment form the lexer hides
                // when code follows on its row (a block form), a binding policy
                // over the row, not membership
                comment.kind_id() != Groovy::LineComment && next.start_row() == comment.end_row()
            },
        })
    });

    // The `DocTarget` row of `roles/groovy.rs` (#1699).
    impl_doc_target_from_row!(groovy);

    // The callables and the closure, then the `HoldsDocInBody` row of
    // `roles/groovy.rs` — `static { … }` (#1699).
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        _below: &Node<'a>,
    ) -> bool {
        Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            || holds_doc_in_body_kind!(groovy, ancestor, ancestors)
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
        crate::roles::groovy::primitive::lists_kind(node.kind_id())
    }

    // `var` is keyed by text as well (#1706): the `DeclKeyword` row owns
    // it as the keyword leaf of a local and as the `type_identifier` of
    // a field or a modified local, two kinds that spell one keyword, and
    // Halstead's `u_operators` sums the kind-keyed and the text-keyed
    // maps — so left kind-keyed the two spellings would count as two
    // distinct operators. The byte test is what keeps the row's other
    // keywords (`class`, `def`, …) kind-keyed as they were — a move no
    // metric can see, since each is one spelling under one key either
    // way; `groovy_text_keyed_operators_are_the_primitives_1599` scans
    // the walk for the keyed set and is the pin.
    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        use crate::roles::groovy::{decl_keyword, primitive};

        (node_bytes(node, code) == b"var" && decl_keyword::owns(node, code, ancestors))
            || primitive::owns(node, code, ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::groovy::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/groovy.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(groovy, call, is_call, is_call_with_code);
    impl_row_predicates!(groovy, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LPAREN | Groovy::COMMA | Groovy::RPAREN
        )
    }

    // The `ElseIf` row of `roles/groovy.rs`: an `if_statement` whose
    // previous sibling is the bare `else` token (no clause wrapper, as in
    // Java), read off the caller's chain. The row reads no bytes, so the
    // empty code is exact (#1697).
    impl_else_if_from_row!(groovy);
}
