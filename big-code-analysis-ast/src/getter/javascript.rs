//! `Getter` implementation for JavaScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for JavascriptCode {
    // The mapping is the `opens` annotations of `roles/javascript.rs`
    // (#1696): every function form and the ES2022 `static { … }` block
    // (#1184) open a `Function` space, a class either spelling a
    // `Class`. The rows read no bytes; the binding gates read the
    // chain, but none of them decides an `opens`, so the byte-less
    // spelling forwards with no chain and answers the same.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::javascript::opens_space(node, code, ancestors)
    }

    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // A class static block has no name token and no naming parent to
        // fall back on, so it would otherwise land on `<anonymous>`
        // alongside every arrow and function expression (#1184).
        // role-bypass: allow(FuncOpener) — a naming rule, not whether
        // the block opens a space
        if node.kind_id() == Javascript::ClassStaticBlock as u16 {
            return Some("<static-init>");
        }
        if let Some(name) = node.child_by_field_name("name") {
            return node_text(code, &name);
        }
        // Otherwise the name comes from the binding site: a pair
        // (`foo: function () {}`) or a variable declaration
        // (`var aFun = function () {}`). The two differ only in which
        // field carries the name, so they collapse to one lookup.
        let bound_name = ancestors.parent(node).and_then(|parent| {
            // role-bypass: allow(DocTarget) — which field of the parent carries the
            // function's name; naming, not whether the pair takes a doc comment
            let field = match parent.kind_id().into() {
                Javascript::Pair => "key",
                Javascript::VariableDeclarator => "name",
                _ => return None,
            };
            parent.child_by_field_name(field)
        });
        bound_name.map_or(Some("<anonymous>"), |name| node_text(code, &name))
    }

    // JSX closing / self-closing tag delimiters. See the macro's
    // `$op_extras` note: they are per-language extras because
    // TypeScript, which shares this macro, has no JSX (#1395).
    // `jsx:` bills element text and its entities as operands, for the
    // same reason (#1483).
    //
    // `Staticget` is one token spelling `static get` followed by a
    // newline, the static getter whose name starts the next line. It is
    // billed beside the shared `static` (#1563) so that form bills a
    // modifier rather than nothing; being one node it is one operator.
    // role-bypass: allow(StringLit) — the operand classification of the literal
    // alias the `StringLit` row owns, a projection the row does not answer
    impl_js_family_get_op_type!(
        Javascript,
        roles: javascript,
        op_extras: [LTSLASH, SLASHGT],
        operand_extras: [Identifier2, String2],
        jsx: [JsxText, HtmlCharacterReference],
    );

    get_operator!(Javascript);
}
