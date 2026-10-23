//! The kind lists the four JS-family grammars share (#1693):
//! JavaScript, MozJS, TypeScript and TSX. Each is a fragment a
//! grammar's row splices with `..name!()`; TypeScript and TSX add their
//! type-system keywords beside it. The lists are what the shared
//! `impl_js_family_get_op_type!` arm billed for all four, so nothing is
//! promoted into a grammar that did not bill it. The fragment shape and
//! the reason it carries its list twice are on `roles/c_family.rs`.

/// `static`, the one class-member modifier every JS grammar has (#1563;
/// C# and Kotlin bill theirs the same way). It is the bare leaf in a
/// field, a method and a `static { … }` block alike, with no modifier
/// wrapper, and one kind in all four grammars. Used as a name
/// (`obj.static`, `static()`, a field named `static`) the grammar emits
/// an identifier instead. `async` is not here: it bills as an
/// expression keyword beside `await`, and every grammar spells the
/// `async` of a method or a function the same way.
macro_rules! js_family_modifiers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* Static,] $($rest)*);
    };
    (@opens_splice $ctx:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@opens_owners $ctx [$($acc)* Static,] $($rest)*);
    };
}

/// The declaration keywords every JS grammar has: `function` and
/// `class` (#1552), and `let` / `var` / `const`. `Function` and
/// `Class2` are the keyword leaves; their expression wrappers —
/// `FunctionExpression` and the unsuffixed `Class` — stay unlisted, so
/// `const f = function () {}` bills one `function`, not two (#1554).
/// The leaf is the keeper because it exists in every spelling: the
/// `async` and generator forms, and an ERROR-recovery parse that drops
/// the wrapper. `let` / `var` / `const` are the leaves of a
/// `lexical_declaration` / `variable_declaration`, neither classified.
macro_rules! js_family_decl_keywords {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* Function, Class2, Let, Var, Const,] $($rest)*);
    };
    (@opens_splice $ctx:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(
            @opens_owners $ctx [$($acc)* Function, Class2, Let, Var, Const,] $($rest)*
        );
    };
}

pub(crate) use {js_family_decl_keywords, js_family_modifiers};

/// The two gates TypeScript's and TSX's keyword rows share, emitted per
/// grammar because each reads that grammar's kind ids.
///
/// `declares_a_type`: a binding *named* `type` in an export clause
/// (`export { type }`, `export { type as kind }`). The grammar's
/// `export_specifier` has no slot for that name, unlike
/// `import_specifier`, so it recovers the keyword leaf under an `ERROR`
/// in the clause, or beside one in the specifier, and the `type`
/// keyword would bill a binding name. A type-only specifier
/// (`export { type Foo }`) parses cleanly and still bills, as does the
/// `type` of `export type * from "m"`, whose `ERROR` hangs from the
/// statement rather than the clause.
///
/// `names_a_type_not_void`: a `void` return or parameter type is a
/// `predefined_type` wrapper around an inner `void` token, and that
/// token is an expression operator of its own (`void 0`), so billing
/// the wrapper too counted one source `void` twice (#453). The wrapper
/// is the primitive for every other spelling — only `void` has an
/// operator-kind child; the `string` keyword's child is an operand-kind
/// alias, which collided in the other direction until #1261 dropped it
/// from the operand extras — and the inner token carries `void`.
macro_rules! ts_family_gates {
    ($lang:ident) => {
        fn declares_a_type<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            let mut up = ancestors.iter(node).map(|(ancestor, _)| ancestor);
            !up.next().is_some_and(|parent| {
                if parent.is_error() {
                    up.next()
                        .is_some_and(|clause| clause.kind_id() == $lang::ExportClause as u16)
                } else {
                    parent.kind_id() == $lang::ExportSpecifier as u16 && parent.has_error()
                }
            })
        }

        fn names_a_type_not_void<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            !node
                .child(0)
                .is_some_and(|child| child.kind_id() == $lang::Void as u16)
        }
    };
}

pub(crate) use ts_family_gates;
