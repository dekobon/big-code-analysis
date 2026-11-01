//! The doc-comment binding the JS-family `Checker` impls share (#1613):
//! which `/** */` comments JSDoc and tsc attach to the declaration after
//! them. Macros, because each language has its own kind enum.

// Generate the attachment rule for a JS-family `Checker` block (#1613):
// a `/** */` outside a function body documents the declaration after it,
// read past `$hidden` comments and past the decorators TypeScript's
// grammar leaves beside a class member, unless `$standalone` says it
// documents the file or a symbol it names. In a body it keeps the
// unbound reading, since JSDoc publishes inner symbols. `$target` is
// the language's `is_doc_target` — the `DocTarget` row of
// `roles/<lang>.rs` (#1699), composed with the parent context tsc reads
// for the TypeScript grammars (`tsdoc_target`) — and `$after_code` says
// before which targets a doc comment after code on its row still
// documents.
macro_rules! impl_js_family_doc_binding {
    ($lang:ident, $hidden:expr, $standalone:path, $target:path, $after_code:expr) => {
        const DOC_BINDING: Option<$crate::lang_helpers::DocBinding> =
            Some($crate::lang_helpers::DocBinding {
                skips: |node, _| node.kind_id() == $lang::Decorator,
                forward: |comment, code, ancestors| {
                    $crate::lang_helpers::doc_comment::outside_function_bodies::<Self>(
                        comment, ancestors,
                    ) && !$standalone($crate::lang_helpers::doc_comment::node_bytes(comment, code))
                },
                ..$crate::lang_helpers::DocBinding::new($crate::lang_helpers::KeptDocs::Leading {
                    hidden: $hidden,
                    joins: $crate::lang_helpers::doc_comment::never_joins,
                    after_code: $after_code,
                })
            });

        fn is_doc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
            $target(node, code, ancestors)
        }

        // Only the body proper: a doc comment after a member's decorator
        // or on a parameter is in the signature, where it binds to
        // nothing. An arrow function's expression body is the child that
        // ends it without starting it.
        fn holds_doc_comment_in_body<'a>(
            ancestor: &Node<'a>,
            ancestors: Ancestors<'a, '_>,
            below: &Node<'a>,
        ) -> bool {
            if !Self::is_func(ancestor, ancestors) && !Self::is_closure(ancestor, ancestors) {
                return false;
            }
            // role-bypass: allow(FuncOpener, Closure) — where a comment
            // sits in an arrow's body, not whether the arrow is one
            if ancestor.kind_id() == $lang::ArrowFunction {
                return below.end_byte() == ancestor.end_byte()
                    && below.start_byte() != ancestor.start_byte()
                    && !Self::is_comment(below);
            }
            below.kind_id() == $lang::StatementBlock
        }
    };
}

// The TypeScript grammars' `is_doc_target` (#1699): the `DocTarget` row
// composed with the one piece of context the row cannot see — tsc reads
// a declaration's comments before its first token only, so a
// declaration *under* an `export_statement` or `ambient_declaration` (a
// sibling of the `export` / `declare` token, inside the wrapper)
// documents nothing on its own; the wrapper is the target, by the row's
// `export_declares_to_tsc` gate. And an enum member spelled as a bare
// `string` under the `enum_body` is a target the row owns by id, TS and
// TSX numbering the kind differently. The row over-approximates the
// Checker on a declaration under `export`, which `roles/doc_tests/mod.rs`
// pins. One fn for both grammars, handed the dialect's row.
macro_rules! tsdoc_target {
    ($lang:ident, $roles:ident) => {
        fn tsdoc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
            // role-bypass: allow(DocTarget) — the wrapper a declaration sits
            // under, where tsc reads no comment: context over the row
            if ancestors.parent(node).is_some_and(|parent| {
                matches!(
                    parent.kind_id().into(),
                    $lang::ExportStatement | $lang::AmbientDeclaration
                )
            }) {
                return false;
            }
            $crate::roles::$roles::doc_target::owns(node, code, ancestors)
        }
    };
}
