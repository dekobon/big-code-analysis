//! `Getter` implementation for TypeScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Typescript::TypeArguments as u16,
        Typescript::TypeParameters as u16,
    ],
    openers: &[Typescript::LT as u16],
    closers: &[Typescript::GT as u16],
    is_misparse: never_misparsed,
};

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

    // TypeScript's only operand extra is `TypeIdentifier` (#1557,
    // below). `NestedIdentifier` and `MemberExpression4` — the TS-only
    // member-expression productions — were listed until #1263 and are
    // now deliberately absent, matching the `MemberExpression*` drop in
    // the macro body: `namespace N.M` contributes the operands `N` and
    // `M` plus the `.` operator, and `a.b` contributes `a` and `b`,
    // never the composite text as well. The composite was billed on top
    // of leaves the walker already reached, which is grammar-dispatch
    // section 5's container/leaf double-count.
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
    //
    // No `jsx:` argument: the `.ts` grammar's enum carries `JsxText` in
    // its shared externals, but no production emits it (#1483).
    //
    // `Interface` / `Enum` are the TS-only type-declaration keyword
    // leaves, billed as the shared `class` is (#1552) and as C# and
    // Java bill theirs; their `*_declaration` wrappers stay unlisted.
    //
    // #1557 adds the rest: `type`, `namespace`, `module`, `declare` and
    // `global`. `Module2` is the `module` keyword leaf; the unsuffixed
    // `Module` is the `module` declaration node that wraps it, so it
    // stays unlisted. Each word is contextual, but used as a name
    // (`let type = 1`, `module.exports`) the grammar emits an
    // `identifier`, never the keyword leaf. The one exception is a
    // binding named `type` in an export clause (`export { type }`),
    // which the grammar recovers as the keyword leaf; `export_type_name`
    // leaves that leaf unbilled. `type` also bills in `import type` /
    // `export type`, where it is the same leaf.
    //
    // `TypeIdentifier` is the operand extra: every type *name* — a
    // class, interface or alias name, a type parameter, an annotation
    // (`x: Foo`), a generic (`Map<K, V>`), the tail of `ns.T` — is this
    // one leaf kind, and it was in neither arm before #1557, so TS lost
    // every one while C#, Rust and the C family bill theirs. No
    // classified node wraps it (`generic_type` and
    // `nested_type_identifier` are unlisted), so it bills once.
    //
    // #1561 adds the type-level operator keywords, the siblings of the
    // `as` / `typeof` / `extends` the macro already bills: `satisfies`,
    // `keyof`, `infer`, the type-predicate `is` and `asserts`. Each is
    // its keyword leaf; the `satisfies_expression`, `index_type_query`,
    // `infer_type`, `type_predicate` and `asserts` wrappers stay
    // unlisted, so one keyword bills once. `Asserts2` is the keyword
    // token and the unsuffixed `Asserts` the node wrapping it, so
    // `asserts a is T` bills `asserts` and `is` once each. Which of the
    // two bills is unobservable — every node holds exactly one token and
    // both render `asserts` — so the leaf is kept, as everywhere else
    // here; listing both is what the tests catch. Used as names
    // (`const keyof = 1`, `o.is`, a `satisfies` parameter) the words
    // parse as identifiers. One valid spelling the pinned grammar gets
    // wrong is `let satisfies = 1`: it reads `let` as an identifier and
    // recovers a `satisfies_expression` around an ERROR, so that
    // statement is mis-scored whatever this list holds.
    //
    // `generic_angles`: a type-argument or type-parameter list's `<`
    // bills as the `<>` pair and its `>` as nothing (#1559). A `<T>x`
    // type assertion wraps a `type_arguments` node, so it is a pair too;
    // a comparison's `<` / `>` sit under `binary_expression` and stay
    // their own operators.
    impl_js_family_get_op_type!(
        Typescript,
        op_extras: [
            QMARKDOT, PredefinedType, Interface, Enum, Type, Namespace, Module2, Declare, Global,
            Satisfies, Keyof, Infer, Is, Asserts2,
        ],
        operand_extras: [TypeIdentifier],
        predefined_void: PredefinedType,
        generic_angles: GENERIC_ANGLES,
        export_type_name: [Type, ExportClause, ExportSpecifier],
    );

    get_operator!(Typescript);
}
