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
            | ArrowFunction => SpaceKind::Function,
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
