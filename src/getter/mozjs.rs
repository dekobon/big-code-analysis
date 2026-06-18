//! `Getter` implementation for MozJS.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for MozjsCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Mozjs::*;

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

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            node_text(code, &name)
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            return node_text(code, &name);
                        }
                    }
                    Mozjs::VariableDeclarator => {
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
        Mozjs,
        op_extras: [OptionalChain],
        operand_extras: [Identifier2, String2],
    );

    get_operator!(Mozjs);
}
