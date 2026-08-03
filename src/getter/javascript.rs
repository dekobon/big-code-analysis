//! `Getter` implementation for JavaScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for JavascriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Javascript::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction
            // ES2022 `static { … }` (#1184).
            | ClassStaticBlock => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // A class static block has no name token and no naming parent to
        // fall back on, so it would otherwise land on `<anonymous>`
        // alongside every arrow and function expression (#1184).
        if node.kind_id() == Javascript::ClassStaticBlock as u16 {
            return Some("<static-init>");
        }
        if let Some(name) = node.child_by_field_name("name") {
            node_text(code, &name)
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = ancestors.parent(node) {
                match parent.kind_id().into() {
                    Javascript::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            return node_text(code, &name);
                        }
                    }
                    Javascript::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            return node_text(code, &name);
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    impl_js_family_get_op_type!(
        Javascript,
        op_extras: [OptionalChain],
        operand_extras: [Identifier2, String2],
    );

    get_operator!(Javascript);
}
