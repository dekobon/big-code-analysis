//! `Getter` implementation for TSX.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for TsxCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Tsx::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration | AbstractClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
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
                    Tsx::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            return node_text(code, &name);
                        }
                    }
                    Tsx::VariableDeclarator => {
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

    // TSX exposes two anonymous `"string"` aliases: `String2` (the
    // string-literal alias, kind_id 261) and `String3` (the
    // type-annotation keyword, kind_id 141 — the role TS's `String2`
    // plays). `Checker::is_string` matches both (#283); including
    // `String3` here closes the audit gap surfaced by #313 (the same
    // class of inconsistency that #313 fixed for TS::String2).
    impl_js_family_get_op_type!(
        Tsx,
        op_extras: [QMARKDOT, PredefinedType],
        operand_extras: [Identifier2, String2, String3, NestedIdentifier, MemberExpression4],
        predefined_void: PredefinedType,
    );

    get_operator!(Tsx);
}
