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
            | ArrowFunction
            // ES2022 `static { … }` (#1184).
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
        if node.kind_id() == Tsx::ClassStaticBlock as u16 {
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
                Tsx::Pair => "key",
                Tsx::VariableDeclarator => "name",
                _ => return None,
            };
            parent.child_by_field_name(field)
        });
        bound_name.map_or(Some("<anonymous>"), |name| node_text(code, &name))
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
