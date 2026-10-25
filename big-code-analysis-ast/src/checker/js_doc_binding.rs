//! The doc-comment binding the JS-family `Checker` impls share (#1613):
//! which `/** */` comments JSDoc and tsc attach to the declaration after
//! them. Macros, because each language has its own kind enum.

// Generate the attachment rule for a JS-family `Checker` block (#1613):
// a `/** */` outside a function body documents the declaration after it,
// read past `$hidden` comments and past the decorators TypeScript's
// grammar leaves beside a class member, unless `$standalone` says it
// documents the file or a symbol it names. In a body it keeps the
// unbound reading, since JSDoc publishes inner symbols. `$target` is the
// language's `is_doc_target` body, and `$after_code` says before which
// targets a doc comment after code on its row still documents.
macro_rules! impl_js_family_doc_binding {
    ($lang:ident, $hidden:expr, $standalone:path, $target:ident, $after_code:expr) => {
        const DOC_BINDING: Option<$crate::lang_helpers::DocBinding> =
            Some($crate::lang_helpers::DocBinding {
                skips: |node, _| node.kind_id() == $lang::Decorator,
                forward: |comment, code, ancestors| {
                    $crate::lang_helpers::doc_comment::outside_function_bodies::<Self>(
                        comment, ancestors,
                    ) && !$standalone($crate::lang_helpers::doc_comment::node_bytes(comment, code))
                },
                ..$crate::lang_helpers::DocBinding::new($crate::lang_helpers::KeptDocs::Leading {
                    hidden: $hidden,
                    joins: $crate::lang_helpers::doc_comment::never_joins,
                    after_code: $after_code,
                })
            });

        fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
            $target!($lang, node, ancestors)
        }

        // Only the body proper: a doc comment after a member's decorator
        // or on a parameter is in the signature, where it binds to
        // nothing. An arrow function's expression body is the child that
        // ends it without starting it.
        fn holds_doc_comment_in_body<'a>(
            ancestor: &Node<'a>,
            ancestors: Ancestors<'a, '_>,
            below: &Node<'a>,
        ) -> bool {
            if !Self::is_func(ancestor, ancestors) && !Self::is_closure(ancestor, ancestors) {
                return false;
            }
            // role-bypass: allow(FuncOpener, Closure) — where a comment
            // sits in an arrow's body, not whether the arrow is one
            if ancestor.kind_id() == $lang::ArrowFunction {
                return below.end_byte() == ancestor.end_byte()
                    && below.start_byte() != ancestor.start_byte()
                    && !Self::is_comment(below);
            }
            below.kind_id() == $lang::StatementBlock
        }
    };
}

// What JSDoc's `astnode.getInfo` names, and so documents: a declaration,
// a class member, an object member, a variable declaration whose first
// declarator binds an identifier, an export other than a bare
// `export { … }` list, and an expression statement that assigns to a
// name or a member (`x = 1`, `a.b += 1`). Any other statement — a call,
// an `if`, an `import`, a destructuring assignment — takes none, checked
// with `jsdoc -X`.
macro_rules! jsdoc_target {
    ($lang:ident, $node:ident, $ancestors:ident) => {{
        use $lang::*;
        let _ = $ancestors;
        match $node.kind_id().into() {
            FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ClassDeclaration
            | MethodDefinition
            | FieldDefinition
            | Pair
            | ShorthandPropertyIdentifier => true,
            VariableDeclaration | LexicalDeclaration | UsingDeclaration => $node
                .children()
                .find(|c| c.kind_id() == VariableDeclarator)
                .and_then(|d| d.child_by_field_name("name"))
                .is_some_and(|name| name.kind_id() == Identifier),
            ExportStatement => !$node.is_child(ExportClause as u16),
            ExpressionStatement => {
                // The parenthesized expression by role, not by index: a
                // comment after the `(` is a child before it.
                let mut expr = $node.child(0);
                while let Some(e) = expr.filter(|e| e.kind_id() == ParenthesizedExpression) {
                    expr = e
                        .children()
                        .find(|c| c.is_named() && !c.as_tree_sitter().is_extra());
                }
                expr.filter(|e| {
                    matches!(
                        e.kind_id().into(),
                        AssignmentExpression | AugmentedAssignmentExpression
                    )
                })
                .and_then(|e| e.child_by_field_name("left"))
                .is_some_and(|left| !matches!(left.kind_id().into(), ObjectPattern | ArrayPattern))
            }
            _ => false,
        }
    }};
}

// What tsc and TypeDoc document: JSDoc's declarations without its
// assignment statements, which tsc does not declare, plus TypeScript's
// own — interfaces and their members, type aliases, enums and their
// members, namespaces and modules, ambient declarations and abstract
// members. A destructuring declaration documents no binding (tsc's quick
// info is empty), and an export documents what it declares, or the names
// an `export { … }` list re-exports; `export *` and `export default <expr>`
// take none: tsc keeps the JSDoc on the export assignment node, but
// neither quick info on the imported binding nor TypeDoc surfaces it. The
// anonymous `export default function () {}` and `export default class {}`
// are declarations to tsc, which the grammar can only read as an
// expression in the `value` field, so they document as their named twins
// do; `export default abstract class {}` it reads as an error before an
// object, so that one documents nothing (upstream grammar gap, no row
// pins it). tsc reads a declaration's comments
// before its first token only, so one after `export` or `declare` — a
// sibling of the declaration inside the `export_statement` or
// `ambient_declaration` — documents nothing.
macro_rules! tsdoc_target {
    ($lang:ident, $node:ident, $ancestors:ident) => {{
        use $lang::*;
        let declaration = |node: &Node| match node.kind_id().into() {
            FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ClassDeclaration
            | AbstractClassDeclaration
            | InterfaceDeclaration
            | TypeAliasDeclaration
            | EnumDeclaration
            | Module
            | AmbientDeclaration
            | FunctionSignature
            | MethodDefinition
            | MethodSignature
            | AbstractMethodSignature
            | PublicFieldDefinition
            | PropertySignature
            | IndexSignature
            | CallSignature
            | ConstructSignature
            | Pair
            | ShorthandPropertyIdentifier
            | EnumAssignment
            | RequiredParameter
            | RequiredParameter2
            | OptionalParameter
            | OptionalParameter2 => true,
            VariableDeclaration | LexicalDeclaration => node
                .children()
                .find(|c| c.kind_id() == VariableDeclarator)
                .and_then(|d| d.child_by_field_name("name"))
                .is_some_and(|name| name.kind_id() == Identifier),
            // The grammar parses `using x = f()` as an assignment its
            // `using` token opens, under an `await_expression` for `await
            // using`; tsc declares `x` as it does for `const x = f()`.
            ExpressionStatement => node.child(0).is_some_and(|e| {
                if e.kind_id() == InternalModule {
                    return true;
                }
                let assignment = if e.kind_id() == AwaitExpression {
                    e.children()
                        .find(|c| c.is_named() && !c.as_tree_sitter().is_extra())
                } else {
                    Some(e)
                };
                assignment.is_some_and(|a| {
                    a.kind_id() == AssignmentExpression
                        && a.child(0).is_some_and(|u| u.kind_id() == Using)
                        && a.child_by_field_name("left")
                            .is_some_and(|left| left.kind_id() == Identifier)
                })
            }),
            _ => false,
        };
        let parent = $ancestors.parent($node).map(|p| p.kind_id().into());
        match ($node.kind_id().into(), parent) {
            (_, Some(ExportStatement | AmbientDeclaration)) => false,
            (ExportStatement, _) => match $node.child_by_field_name("declaration") {
                Some(inner) => declaration(&inner) || inner.kind_id() == InternalModule,
                None => {
                    $node.is_child(ExportClause as u16)
                        || $node.child_by_field_name("value").is_some_and(|value| {
                            matches!(
                                value.kind_id().into(),
                                FunctionExpression | GeneratorFunction | Class
                            )
                        })
                }
            },
            // An enum member without an initializer is a bare name in the
            // enum body — an identifier, a string or a computed name — and
            // anywhere else a `PropertyIdentifier` is a member name after a
            // modifier or a `.`, inside what it names. The string is
            // `String` in TS and `String2` in TSX, so its kind name decides.
            (PropertyIdentifier | ComputedPropertyName, Some(EnumBody)) => true,
            (_, Some(EnumBody)) => $node.kind() == "string" || declaration($node),
            _ => declaration($node),
        }
    }};
}
