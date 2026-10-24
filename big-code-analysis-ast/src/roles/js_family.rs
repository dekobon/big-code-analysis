//! The kind lists the four JS-family grammars share (#1693):
//! JavaScript, MozJS, TypeScript and TSX. Each is a fragment a
//! grammar's row splices with `..name!()`; TypeScript and TSX add their
//! type-system keywords beside it. The lists are what the shared
//! `impl_js_family_get_op_type!` arm billed for all four, so nothing is
//! promoted into a grammar that did not bill it. The fragment shape is
//! on `roles/c_family.rs`.

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

/// The TypeScript-only modifiers both TS grammars bill beside the
/// shared JS-family `static` (#1563, #1557): `public`, `private`,
/// `protected`, `override`, `readonly`, `abstract`, `accessor` and
/// `declare`. Each is its keyword leaf, one kind apiece; the
/// `accessibility_modifier` / `override_modifier` wrappers stay
/// unlisted so a modifier bills once. The spelling notes — where each
/// leaf bills outside a class, and the name spellings the pinned
/// grammar misreads under an ERROR — are on the TypeScript row.
macro_rules! ts_family_modifiers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            Public, Private, Protected, Override, Readonly, Abstract, Accessor, Declare,
        ] $($rest)*);
    };
}

/// The TypeScript-only declaration keywords both TS grammars bill
/// beside the shared JS-family `class` / `function` (#1552, #1557):
/// `interface`, `enum`, `type` — gated by the dialect's
/// `declares_a_type`, which resolves at the splicing row —
/// `namespace`, `module` (`Module2`, the keyword leaf; `Module` is the
/// declaration node) and `global`. The contextual-word notes are on
/// the TypeScript row.
macro_rules! ts_family_decl_keywords {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            Interface, Enum, Type(gated_by declares_a_type), Namespace, Module2, Global,
        ] $($rest)*);
    };
}

/// The comparison tokens every JS grammar applies under a
/// `binary_expression` (#1695): the four equalities, `<=` / `>=`, the
/// `instanceof` keyword, and `<` / `>` — which the TypeScript grammars
/// also spell as generic brackets and the JSX grammars as tag
/// delimiters, so those two are gated to the parent. `in` is a
/// relational operator too, but no counting consumer has ever scored
/// it (the slot holding one pays), and claiming it would move output;
/// it stays with the getters' own arms.
macro_rules! js_family_comparison_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            EQEQ, EQEQEQ, BANGEQ, BANGEQEQ, LTEQ, GTEQ, Instanceof,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
        ] $($rest)*);
    };
}

/// `&&` / `||`, which every JS grammar emits only under a
/// `binary_expression`. `??` is `NullSafe`'s, and the compound
/// `&&=` / `||=` are assignments.
macro_rules! js_family_short_circuit_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* AMPAMP, PIPEPIPE,] $($rest)*);
    };
}

/// The four gates the JS-family `Comparison` / `ShortCircuit` /
/// `Ternary` rows read, per grammar because each reads the grammar's
/// kind ids.
macro_rules! js_family_operator_gates {
    ($lang:ident) => {
        /// Whether a `?` is a ternary's, the gate the TypeScript rows
        /// read: those grammars spell the bare token in ten type
        /// productions — an optional parameter, property, method or
        /// tuple member, `optional_type`, `flow_maybe_type`,
        /// `conditional_type` (`T extends U ? X : Y`, resolved by the
        /// type checker and erased before runtime) — none a decision
        /// (#1275). The JavaScript grammars spell it nowhere else and
        /// pay it ungated. `?.` and `??` are distinct tokens.
        fn under_ternary<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            ancestors.parent_has_kind(node, $lang::TernaryExpression as u16)
        }

        /// Whether a bare `<` / `>` is the operator of a
        /// `binary_expression` rather than a generic bracket, a JSX tag
        /// delimiter or a token error recovery strands.
        fn under_binary_expression<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            ancestors.parent_has_kind(node, $lang::BinaryExpression as u16)
        }

        /// Whether a `binary_expression`'s `operator` field is a
        /// `Comparison` payer.
        fn comparison_operator<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
        }

        /// Whether a `binary_expression`'s `operator` field is `&&` / `||`.
        fn short_circuit_operator<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            short_circuit::project(node)
                .is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
        }
    };
}

/// Whether a `binary_expression`'s `operator` field is `??`.
macro_rules! js_family_null_safe_gate {
    () => {
        /// Whether a `binary_expression` applies `??`.
        fn null_coalescing_operator<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            null_safe::project(node).is_some_and(|op| null_safe::PAYERS.contains(&op.kind_id()))
        }
    };
}

/// The `CaseArm` gate every JS grammar reads (#1695): a `switch_case`
/// owns a decision and its `case` keyword pays; the `switch_default`
/// and its `default` keyword are the fallback and pay nothing (lesson
/// 11, #469). Every `switch_case` holds a `case`, which the gate pins.
macro_rules! js_family_case_arm_gate {
    ($lang:ident) => {
        /// Whether a `switch_case` holds its `case` keyword.
        fn is_case_label<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            node.is_child($lang::Case as u16)
        }
    };
}

pub(crate) use {
    js_family_case_arm_gate, js_family_comparison_payers, js_family_null_safe_gate,
    js_family_operator_gates, js_family_short_circuit_payers, ts_family_decl_keywords,
    ts_family_gates, ts_family_modifiers,
};
