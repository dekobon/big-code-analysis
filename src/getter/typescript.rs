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

    // TypeScript's operand extras are empty. `NestedIdentifier` and
    // `MemberExpression4` — the TS-only member-expression productions —
    // were listed until #1263 and are now deliberately absent, matching
    // the `MemberExpression*` drop in the macro body: `namespace N.M`
    // contributes the operands `N` and `M` plus the `.` operator, and
    // `a.b` contributes `a` and `b`, never the composite text as well.
    // The composite was billed on top of leaves the walker already
    // reached, which is grammar-dispatch section 5's container/leaf
    // double-count.
    //
    // TS's anonymous `"string"` alias `String2` (kind_id 135, the
    // `: string` type keyword, emitted only as the child of a
    // `predefined_type` wrapper) is deliberately NOT an operand either:
    // the wrapper already counts as the text-keyed `"string"` operator
    // via `is_primitive`, so #313's listing of the child too counted one
    // source token as operator AND operand — the mirror image of the
    // #453 `void` collision — while `: number` / `: boolean` counted
    // once (#1261). `Checker::is_string` no longer matches the keyword
    // either, so the #313 parity rationale is retired rather than
    // contradicted.
    impl_js_family_get_op_type!(
        Typescript,
        op_extras: [QMARKDOT, PredefinedType],
        operand_extras: [],
        predefined_void: PredefinedType,
    );

    get_operator!(Typescript);
}
