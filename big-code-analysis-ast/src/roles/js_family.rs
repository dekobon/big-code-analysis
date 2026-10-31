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

/// The function openers every JS grammar has (#1696). Every one opens a
/// `Function` space — the walk promotes each, a class static block
/// included (#1184) — and which are *functions* rather than closures is
/// the binding question: a declaration, a generator declaration and a
/// method are named by construction; a function expression, a
/// generator expression and an arrow are named only when bound, which
/// `bound_function` / `bound_arrow` (`js_family_binding_gates!`) read
/// off the chain. A generator is a function, not a closure (#1186), and
/// the two generator kinds are not symmetrical: a
/// `generator_function_declaration` has a required name and is
/// unconditionally a function, a `generator_function` an optional one
/// and routes through the binding walk as a function expression does —
/// otherwise `run(function*(){})` would be a function while
/// `run(function(){})` stays a closure. The gates resolve at the
/// dialect's row, since each reads the dialect's kind ids and
/// field-definition spelling.
macro_rules! js_family_func_openers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            FunctionDeclaration(callable opens Function),
            GeneratorFunctionDeclaration(callable opens Function),
            MethodDefinition(callable opens Function),
            FunctionExpression(opens Function callable_when bound_function),
            GeneratorFunction(opens Function callable_when bound_function),
            ArrowFunction(opens Function callable_when bound_arrow),
            ClassStaticBlock(opens Function),
        ] $($rest)*);
    };
}

/// The closures: the same three expression kinds where they are *not*
/// bound — the complement of the `callable_when` gates above, through
/// the same two readers negated, so one rule answers both rows.
macro_rules! js_family_closures {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            FunctionExpression(gated_by unbound_function),
            GeneratorFunction(gated_by unbound_function),
            ArrowFunction(gated_by unbound_arrow),
        ] $($rest)*);
    };
}

/// The containers every JS grammar has: the file, and a class in its
/// declaration and expression spellings (`class C {}` / `const C =
/// class {}`), both `Class`.
macro_rules! js_family_containers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            Program(opens Unit),
            ClassDeclaration(opens Class),
            Class(opens Class),
        ] $($rest)*);
    };
}

/// The binding-site walk, emitted per dialect (#1188) — `$code` is the
/// dialect's `Checker`, whose `is_else_if` the climb consults: is this
/// expression *bound* to a name, or used positionally? The walk climbs
/// the chain until it meets a binding frame — a `variable_declarator`,
/// an assignment, a label, an object `pair`, a class field (`$field`,
/// the dialect's spelling: `field_definition` in the JS grammars,
/// `public_field_definition` in the TS ones) — or a positional one — a
/// block, `return`, `new`, a call or its arguments — and the first met
/// decides; an `else if` on the way is not a frame. The chain is what
/// makes a two-step climb `O(1)` rather than `Node::parent`'s
/// `O(depth)` (#1088); a chain of expression-bodied arrows is the one
/// shape no chain flattens, and stays `O(depth)` per node.
///
/// The two forms differ in one clause, and must: a function
/// expression's `identifier` child is its own name (`run(function g()
/// {})` is a function), an arrow's is its un-parenthesised parameter
/// (`run(x => x)` is a closure), so the arrow asks for a
/// `property_identifier` *sibling* instead — a bare-key object method
/// (`{ m: () => 1 }`) or a class field by name. An IIFE in either
/// spelling is a closure, since the call frame is met first.
macro_rules! js_family_binding_gates {
    ($lang:ident, $code:ident, $field:ident) => {
        // `extra` is a closure, not a value: the arrow form's sibling scan
        // is `O(width)` over the parent's children, and a bound arrow —
        // the common case — never needs it. Evaluated eagerly it made a
        // 4,000-element array of arrows 200× slower (#1696 review).
        fn bound<'a>(
            node: &$crate::Node<'a>,
            ancestors: $crate::Ancestors<'a, '_>,
            extra: impl FnOnce() -> bool,
        ) -> bool {
            use $lang::*;
            node.count_specific_ancestors::<$crate::$code>(
                ancestors,
                |node| {
                    matches!(
                        node.kind_id().into(),
                        VariableDeclarator
                            | AssignmentExpression
                            | LabeledStatement
                            | Pair
                            | $field
                    )
                },
                |node| {
                    matches!(
                        node.kind_id().into(),
                        StatementBlock
                            | ReturnStatement
                            | NewExpression
                            | CallExpression
                            | Arguments
                    )
                },
            ) > 0
                || extra()
        }

        fn bound_function<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            bound(node, ancestors, || node.is_child($lang::Identifier as u16))
        }

        // The sibling question is the *parent's*: whether it holds a
        // `property_identifier` child at all, an `O(width)` scan. Asked
        // through the walk's memo it runs once per parent, so a wide
        // argument list or array literal of positional arrows is linear;
        // `nom` and `nargs` each ask twice per arrow (`is_func` then
        // `is_closure`), and off the walk it is computed each time.
        //
        // At the pinned grammars no parent reaches this clause with a
        // `property_identifier` child: `node-types.json` places one only
        // under `pair`, the field kind and `method_definition`, and the
        // first two bind through the ancestor test before `extra` is
        // asked. So the clause is a `false` the scan confirms, kept for
        // a grammar bump that gives an arrow another named parent, and
        // no fixture can turn it true (answering a constant `false`
        // changes no output on the corpus or the fixtures).
        fn bound_arrow<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            bound(node, ancestors, || {
                ancestors.parent(node).is_some_and(|parent| {
                    ancestors.memoised($crate::node::MemoQuestion::NamesAnArrow, &parent, || {
                        parent.is_child($lang::PropertyIdentifier as u16)
                    })
                })
            })
        }

        fn unbound_function<'a>(
            node: &$crate::Node<'a>,
            code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            !bound_function(node, code, ancestors)
        }

        fn unbound_arrow<'a>(
            node: &$crate::Node<'a>,
            code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            !bound_arrow(node, code, ancestors)
        }
    };
}

/// The string literal every JS grammar has (#1698): `String` and the
/// template literal, which `wraps` a `template_substitution` — the
/// `${…}` whose inner expression is walked and billed on its own, so
/// the wrapper yields no operand of its own (#192, the #420 skip). Each
/// dialect adds or excludes its own `"string"` alias beside this: the
/// four grammars give that kind string to different things.
macro_rules! js_family_strings {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* String, TemplateString,] $($rest)*);
    };
}

pub(crate) use {
    js_family_binding_gates, js_family_closures, js_family_containers, js_family_func_openers,
    js_family_strings,
};

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
            LT under [BinaryExpression], GT under [BinaryExpression],
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

/// The loop statements: `for (;;)`, `for … in` / `for … of` (one
/// `for_in_statement` kind for both), `while` and `do … while`. The
/// `for` keyword pays once in each `for` form and `while` once in a
/// `while` and once in a `do … while` (#284).
macro_rules! js_family_loops {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            ForStatement, ForInStatement, WhileStatement, DoStatement,
        ] $($rest)*);
    };
}

/// The exits: `return`, `throw`, and a generator's `yield` / `yield*`
/// (control leaves the body, resumably — counted as Python's `yield`
/// is).
macro_rules! js_family_exits {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            ReturnStatement, ThrowStatement, YieldExpression,
        ] $($rest)*);
    };
}

pub(crate) use {
    js_family_comparison_payers, js_family_exits, js_family_loops, js_family_short_circuit_payers,
    ts_family_decl_keywords, ts_family_gates, ts_family_modifiers,
};

#[cfg(test)]
mod tests {
    use crate::test_support::for_each_node_with_chain;
    use crate::{Ancestors, Checker, Getter, LanguageInfo, Node, SpaceKind};

    /// The binding shapes #1188 and #1186 settled, with the answer each
    /// classifier gave before the rows and must give after. Every
    /// function form opens a `Function` space; which are functions is
    /// the binding question, and the arrow and function-expression
    /// forms answer it differently in exactly one clause: a function
    /// expression's own name makes it a function (`run(function g() {})`),
    /// an arrow's single bare parameter does not (`run(x => x)`).
    #[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
    const SOURCE: &str = "function f(a) { return a; }\n\
function* gen() { yield 1; }\n\
const g = function (b) { return b; };\n\
const h = function named(c) { return c; };\n\
const gg = function* (c) { yield c; };\n\
const i = (d) => d;\n\
const j = e => e;\n\
let l; l = function () {};\n\
label: function m() {}\n\
const o = { p: function () {}, q: () => 1, \"r\": () => 2, [s]: () => 3 };\n\
class K extends Object {\n\
  static { let z = 1; }\n\
  fld = () => 1;\n\
  \"str\" = function () {};\n\
  [\"comp\"] = () => 3;\n\
  constructor() { super(); }\n\
  method() { return f(1); }\n\
}\n\
const K2 = class {};\n\
run(function () {});\n\
run(function named2() {});\n\
run(x => x);\n\
run((x) => x);\n\
(function () {})();\n\
(() => {})();\n\
new Foo(() => 1);\n\
run(function* () {});\n";

    /// The forms in `SOURCE`, in document order, each with its
    /// expectation: `F` a function, `C` a closure. Every one opens a
    /// `Function` space.
    #[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
    const FORMS: &[(&str, char)] = &[
        ("function f(a) { return a; }", 'F'),
        ("function* gen() { yield 1; }", 'F'),
        ("function (b) { return b; }", 'F'),
        ("function named(c) { return c; }", 'F'),
        ("function* (c) { yield c; }", 'F'),
        ("(d) => d", 'F'),
        ("e => e", 'F'),
        ("function () {}", 'F'),
        ("function m() {}", 'F'),
        ("function () {}", 'F'),
        ("() => 1", 'F'),
        ("() => 2", 'F'),
        ("() => 3", 'F'),
        ("() => 1", 'F'),
        ("function () {}", 'F'),
        ("() => 3", 'F'),
        ("constructor() { super(); }", 'F'),
        ("method() { return f(1); }", 'F'),
        ("function () {}", 'C'),
        ("function named2() {}", 'F'),
        ("x => x", 'C'),
        ("(x) => x", 'C'),
        ("function () {}", 'C'),
        ("() => {}", 'C'),
        ("() => 1", 'C'),
        ("function* () {}", 'C'),
    ];

    #[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
    fn check<L: LanguageInfo + Checker + Getter>(
        opens_space: fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> SpaceKind,
        func_owners: &[u16],
        closure_owners: &[u16],
    ) {
        let source = SOURCE.as_bytes();
        let mut seen = Vec::new();
        for_each_node_with_chain::<L>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            let got = (
                L::is_func(node, ancestors),
                L::is_closure(node, ancestors),
                L::promotes_to_func_space_with_code(node, source, ancestors),
                L::get_space_kind_with_code(node, source, ancestors),
            );
            let text = node.utf8_text(source).expect("fixture is UTF-8");
            if matches!(
                kind,
                "function_declaration"
                    | "generator_function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "arrow_function"
                    | "method_definition"
            ) {
                let (form, verdict) = FORMS[seen.len()];
                assert_eq!(text, form, "form #{} of the fixture", seen.len());
                let want = match verdict {
                    'F' => (true, false, true, SpaceKind::Function),
                    _ => (false, true, true, SpaceKind::Function),
                };
                assert_eq!(got, want, "{kind} `{text}`");
                seen.push(text.to_owned());
            } else if kind == "class_static_block" {
                assert_eq!(got, (false, false, true, SpaceKind::Function), "{kind}");
            } else if kind == "class_declaration" || (kind == "class" && node.is_named()) {
                // The `class` *keyword* leaf shares the kind string
                // (`Class2`); only the named node is the container.
                assert_eq!(got, (false, false, true, SpaceKind::Class), "{kind}");
            } else if kind == "program" {
                assert_eq!(got, (false, false, true, SpaceKind::Unit));
            } else {
                assert_eq!(
                    got,
                    (false, false, false, SpaceKind::Unknown),
                    "{kind} `{text}`"
                );
            }
            assert_eq!(
                got.3 != SpaceKind::Unknown,
                got.2,
                "{kind}: opens_space vs promotion"
            );
            // The byte-less pair is the kind table: the binding gates
            // sit on `callable_when`, which `opens_space` never reads, so
            // it agrees with the `_with_code` pair on every node.
            assert_eq!(
                (L::is_func_space(node), L::get_space_kind(node)),
                (got.2, got.3),
                "{kind}: the byte-less spellings are the kind table"
            );
            assert_eq!(
                got.3,
                opens_space(node, source, ancestors),
                "{kind}: the getter reads the fold"
            );
        });
        // Every form in the fixture reached an expectation, and every
        // declared owner was met.
        assert_eq!(seen.len(), FORMS.len(), "{seen:?}");
        let kinds: std::collections::BTreeSet<u16> = {
            let mut k = std::collections::BTreeSet::new();
            for_each_node_with_chain::<L>(source, |node, _| {
                k.insert(node.kind_id());
            });
            k
        };
        for &owner in func_owners.iter().chain(closure_owners) {
            assert!(kinds.contains(&owner), "fixture never emits kind {owner}");
        }
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_binding_shapes() {
        use crate::roles::javascript::{closure, func_opener, opens_space};
        check::<crate::JavascriptCode>(opens_space, func_opener::OWNERS, closure::OWNERS);
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_binding_shapes() {
        use crate::roles::mozjs::{closure, func_opener, opens_space};
        check::<crate::MozjsCode>(opens_space, func_opener::OWNERS, closure::OWNERS);
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_binding_shapes() {
        use crate::roles::typescript::{closure, func_opener, opens_space};
        check::<crate::TypescriptCode>(opens_space, func_opener::OWNERS, closure::OWNERS);
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_binding_shapes() {
        use crate::roles::tsx::{closure, func_opener, opens_space};
        check::<crate::TsxCode>(opens_space, func_opener::OWNERS, closure::OWNERS);
    }
}
