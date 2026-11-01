//! `Getter` implementation for MozJS.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for MozjsCode {
    // The mapping is the `opens` annotations of `roles/mozjs.rs` (#1696);
    // see `getter/javascript.rs`.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::mozjs::opens_space(node, code, ancestors)
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
        if node.kind_id() == Mozjs::ClassStaticBlock as u16 {
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
                Mozjs::Pair => "key",
                Mozjs::VariableDeclarator => "name",
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
    // same reason (#1483). `Staticget`: see the JavaScript invocation
    // (#1563).
    // role-bypass: allow(StringLit) — the operand classification of the literal
    // alias the `StringLit` row owns, a projection the row does not answer
    impl_js_family_get_op_type!(
        Mozjs,
        roles: mozjs,
        op_extras: [LTSLASH, SLASHGT],
        operand_extras: [Identifier2, String2],
        jsx: [JsxText, HtmlCharacterReference],
    );

    get_operator!(Mozjs);
}
