//! `Checker` implementation for TypeScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for TypescriptCode {
    fn is_comment(node: &Node) -> bool {
        // See MozjsCode::is_comment — `html_comment` (Annex-B `<!-- -->`)
        // is a comment kind for loc/tokens/find parity (#697).
        matches!(
            node.kind_id().into(),
            Typescript::Comment | Typescript::HtmlComment
        )
    }

    // tsc reads `/***` as JSDoc, so TypeScript follows tsc, not JSDoc.
    impl_js_family_doc_comment!(Typescript, tsdoc_block);

    // tsc's `getJSDocCommentsAndTags` takes the last `/**` before a
    // declaration's first token, past every plain comment; a comment after
    // a decorator is inside the declaration and documents nothing. TypeDoc
    // withholds a `@module` or `@packageDocumentation` comment from the
    // declaration and reads it as the file's (#1613). tsc reads a
    // declaration's leading comments from the line break after the token
    // before it, so a doc comment after code on its row documents nothing,
    // except before a parameter, whose trailing comments it reads too.
    impl_js_family_doc_binding!(
        Typescript,
        |node, code| Self::is_comment(node) && !Self::is_doc_comment(node, code),
        crate::lang_helpers::doc_comment::tsdoc_module,
        tsdoc_target,
        |target| matches!(
            target.kind_id().into(),
            Typescript::RequiredParameter
                | Typescript::RequiredParameter2
                | Typescript::OptionalParameter
                | Typescript::OptionalParameter2
        )
    );

    // The space tree is declared in `roles/typescript.rs` (#1696): the
    // `FuncOpener` row (every function form, the ES2022 `static { … }`
    // block — #1184) and the `Container` row are what the walk
    // promotes, and `opens_space` labels them; which function form is
    // a *function* rather than a closure is the binding-site question
    // the rows' gates answer off the chain (#1188). The byte-less
    // `is_func_space` is the legacy wrapper: no `opens` here is gated,
    // so it answers the same with no chain.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::typescript::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::typescript::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::typescript::closure::owns(node, &[], ancestors)
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Typescript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::LPAREN | Typescript::COMMA | Typescript::RPAREN
        )
    }

    // TS's only anonymous `"string"` alias, `String2` (kind_id 135),
    // is the `: string` type-annotation keyword — not a literal — so
    // it is deliberately excluded: `find string` / `count string`
    // report literals, and `Getter::get_op_type` classifies the
    // keyword's `predefined_type` wrapper as an operator (#1261,
    // retiring the #313 parity that made the keyword an operand).
    impl_js_family_is_string!(Typescript);

    impl_is_else_if_parent_clause!(Typescript, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // role-bypass: allow(Primitive) — key selection after the row
        // has classified the node: the wrapper is one kind for many
        // spellings and so is keyed by text, not whether the node is a
        // primitive
        node.kind_id() == Typescript::PredefinedType
    }
}
