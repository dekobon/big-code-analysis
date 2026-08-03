//! `Getter` implementation for TypeScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for TypescriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Typescript::*;

        match node.kind_id().into() {
            // `ClassStaticBlock` is ES2022 `static { … }` (#1184).
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction
            | ClassStaticBlock => SpaceKind::Function,
            Class | ClassDeclaration | AbstractClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
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
        if node.kind_id() == Typescript::ClassStaticBlock as u16 {
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
            let field = match parent.kind_id().into() {
                Typescript::Pair => "key",
                Typescript::VariableDeclarator => "name",
                _ => return None,
            };
            parent.child_by_field_name(field)
        });
        bound_name.map_or(Some("<anonymous>"), |name| node_text(code, &name))
    }

    // TS exposes `String2` as the anonymous `"string"` alias for the
    // type-annotation keyword (kind_id 135, in the type-keyword block
    // of the enum). `Checker::is_string` already matches it (#283);
    // including it here closes the Checker/Getter agreement gap
    // (#313). `NestedIdentifier` and `MemberExpression4` are TS-only
    // member-expression productions.
    impl_js_family_get_op_type!(
        Typescript,
        op_extras: [QMARKDOT, PredefinedType],
        operand_extras: [String2, NestedIdentifier, MemberExpression4],
        predefined_void: PredefinedType,
    );

    get_operator!(Typescript);
}
