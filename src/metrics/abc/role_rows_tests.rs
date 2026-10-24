//! The `Comparison` and `ShortCircuit` rows feed three independent ABC
//! paths — the token arm, the slot's `scores_itself`, and the chain
//! walker — and a fixture any one of them can score reads the right
//! total whichever is dead (grammar-dispatch §11). Each row below gives
//! one input to one path, so a dropped payer, a projection answering
//! `None` or an owner the slot no longer recognises lands on its own
//! number (#1694):
//!
//! - *slot only*: a bare identifier predicate, which only the slot can
//!   pay — zero here means the slot driver stopped being called;
//! - *comparison in a slot*: the token pays and the slot must not —
//!   two here means `scores_itself` lost the owner;
//! - *comparison outside any slot*: `boolean b = x < y;`, which only
//!   the token arm can pay — zero here means the payer was dropped,
//!   and no slot exists to compensate for it;
//! - *the chain*: each operand is a slot and the token pays nothing,
//!   so `a && b` is two and `a && b < 1` is two as well.
//!
//! Cyclomatic is the oracle beside every row (lesson 6): it reads the
//! same `ShortCircuit` row and none of the `Comparison` one, so a
//! comparison row moves `conditions` alone and a chain row moves both.

use crate::test_support::{function_space, space_verbatim};
use crate::{LANG, Metric, MetricsOptions};

/// For each `(body, conditions, decisions)` row, substitutes the body
/// into the template and asserts the function named `f` scores
/// `conditions` ABC conditions and `decisions` cyclomatic decisions.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
#[track_caller]
fn assert_rows(lang: LANG, template: &str, rows: &[(&str, u64, u64)]) {
    assert!(template.contains("BODY"), "template has no body slot");
    let options = MetricsOptions::default().with_only(&[Metric::Abc, Metric::Cyclomatic]);
    for &(body, conditions, decisions) in rows {
        let source = template.replace("BODY", body);
        let root = space_verbatim(lang, source.as_bytes(), options);
        let f = function_space(&root, "f");
        assert_eq!(
            (
                f.metrics.abc.conditions(),
                f.metrics.cyclomatic.cyclomatic() - 1
            ),
            (conditions, decisions),
            "(abc.conditions, decisions) of `{body}` in {lang:?}\n  source: {source}"
        );
    }
}

#[test]
#[cfg(feature = "java")]
fn java_rows_feed_each_abc_path_independently() {
    let template = "class A { boolean f(boolean a, boolean b, int x, int y, Object o) { BODY } }\n";
    assert_rows(
        LANG::Java,
        template,
        &[
            // slot only
            ("if (a) { } return a;", 1, 1),
            // comparison in a slot: the token pays, the slot does not
            ("if (x < y) { } return a;", 1, 1),
            ("if (x == y) { } return a;", 1, 1),
            ("if (o instanceof String) { } return a;", 1, 1),
            ("if (o instanceof String s) { } return a;", 1, 1),
            // outside any slot: only the token arm can pay
            ("boolean c = x < y; return c;", 1, 0),
            ("boolean c = x >= y; return c;", 1, 0),
            ("boolean c = o instanceof String; return c;", 1, 0),
            ("boolean c = o instanceof String s; return c;", 1, 0),
            // a generic bracket pays nothing (the `<` / `>` gate)
            ("java.util.List<String> l = null; return a;", 0, 0),
            // the chain: two operand slots, nothing for the token
            ("return a && b;", 2, 1),
            ("return a || b;", 2, 1),
            ("boolean c = a && x < 1; return c;", 2, 1),
            ("if (a && b) { } return a;", 2, 2),
            ("if (a && b || x > 1) { } return a;", 3, 3),
        ],
    );
    // An `instanceof` keyword error recovery strands under an `ERROR`
    // (the expression lost at end of input) pays for no type test, as
    // the expression node it replaced scored nothing there: the payer
    // is gated to its owner.
    let options = MetricsOptions::default().with_only(&[Metric::Abc]);
    let root = space_verbatim(
        LANG::Java,
        b"class T { boolean f(){ return x instanceof\n",
        options,
    );
    assert_eq!(root.metrics.abc.conditions(), 0);
}

// Python pays the `Comparison` row at its wrapper, so a chained
// `x < y < 3` is one condition, and never pays the `ShortCircuit` token
// — only the chain's operand slots — where cyclomatic counts the token
// and no slot (#395): the same two rows, two policies.
#[test]
#[cfg(feature = "python")]
fn python_rows_feed_each_abc_path_independently() {
    let template = "def f(a, b, x, y, xs):\n    BODY\n";
    assert_rows(
        LANG::Python,
        template,
        &[
            // slot only
            ("if a:\n        pass\n    return a", 1, 1),
            // comparison in a slot: the wrapper pays, the slot does not
            ("if x < y:\n        pass\n    return a", 1, 1),
            ("if x < y < 3:\n        pass\n    return a", 1, 1),
            ("if x in xs:\n        pass\n    return a", 1, 1),
            ("if x not in xs:\n        pass\n    return a", 1, 1),
            ("if x is not None:\n        pass\n    return a", 1, 1),
            // outside any slot: only the wrapper arm can pay
            ("c = x < y\n    return c", 1, 0),
            ("c = x >= y\n    return c", 1, 0),
            ("c = x is None\n    return c", 1, 0),
            // a loop's `in` is no comparison
            ("for i in xs:\n        pass\n    return a", 0, 1),
            // the chain: two operand slots, nothing for the token; the
            // token is cyclomatic's decision
            ("return a and b", 2, 1),
            ("return a or b", 2, 1),
            ("c = a and x < 1\n    return c", 2, 1),
            ("if a and b:\n        pass\n    return a", 2, 2),
            ("if a and b or x > 1:\n        pass\n    return a", 3, 3),
        ],
    );
}

/// The C++ rows on both dialects: Mozcpp owns no file extension, so
/// nothing but a parity test like this reaches its rows.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn assert_cpp_rows(lang: LANG) {
    let template = "template <typename T> concept A = requires(T t) { t.a(); };\n\
                    template <typename T> concept B = requires(T t) { t.b(); };\n\
                    template <typename... Ts>\n\
                    bool f(bool a, bool b, int x, int y, Ts... ts) { BODY }\n";
    assert_rows(
        lang,
        template,
        &[
            // slot only
            ("if (a) { } return a;", 1, 1),
            ("if (-x) { } return a;", 1, 1),
            // comparison in a slot: the token pays, the slot does not
            ("if (x < y) { } return a;", 1, 1),
            ("if (x not_eq y) { } return a;", 1, 1),
            ("if ((x <=> y) < 0) { } return a;", 2, 1),
            // outside any slot: only the token arm can pay
            ("bool c = x < y; return c;", 1, 0),
            ("bool c = x >= y; return c;", 1, 0),
            ("bool c = x not_eq y; return c;", 1, 0),
            ("auto c = x <=> y; return a;", 1, 0),
            // the non-applying spellings pay nothing
            ("std::vector<int> v; return a;", 0, 0),
            ("int&& r = static_cast<int&&>(x); return a;", 0, 0),
            // the chain: two operand slots, nothing for the token, in
            // either spelling and mixed
            ("return a && b;", 2, 1),
            ("return a and b;", 2, 1),
            ("return a and b && x > 1;", 3, 2),
            ("if (a or b || x > 1) { } return a;", 3, 3),
            // a fold applies its operator once: each written operand is
            // a slot, and a unary fold's unwritten side one more
            ("return (ts && ...);", 2, 1),
            ("return (... || ts);", 2, 1),
            ("return (a && ... && ts);", 2, 1),
            ("return (ts == ...);", 1, 0),
            // a requires clause decides nothing, parenthesised or not
            ("static_assert(requires { a && b; }); return a;", 0, 0),
        ],
    );
    // A `requires` clause on a function head, unparenthesised and
    // parenthesised: the `&&` scores zero in both forms, and the
    // function body's own slot still pays.
    for head in ["requires A<T> && B<T>", "requires (A<T> && B<T>)"] {
        let source = format!(
            "template <typename T> concept A = requires(T t) {{ t.a(); }};\n\
             template <typename T> concept B = requires(T t) {{ t.b(); }};\n\
             template <typename T> {head} bool f(T t, bool a) {{ if (a) {{ }} return a; }}\n"
        );
        let options = MetricsOptions::default().with_only(&[Metric::Abc, Metric::Cyclomatic]);
        let root = space_verbatim(lang, source.as_bytes(), options);
        let f = function_space(&root, "f");
        assert_eq!(
            (
                f.metrics.abc.conditions(),
                f.metrics.cyclomatic.cyclomatic() - 1
            ),
            (1, 1),
            "`{head}` in {lang:?}"
        );
    }
}

#[test]
#[cfg(feature = "cpp")]
fn cpp_rows_feed_each_abc_path_independently() {
    assert_cpp_rows(LANG::Cpp);
}

#[test]
#[cfg(feature = "mozcpp")]
fn mozcpp_rows_feed_each_abc_path_independently() {
    assert_cpp_rows(LANG::Mozcpp);
}

/// C and Objective-C: the six comparison tokens and the two
/// short-circuit tokens, through the shared `c_family_*` fragments.
#[cfg(any(feature = "c", feature = "objc"))]
fn assert_c_rows(lang: LANG, template: &str) {
    assert_rows(
        lang,
        template,
        &[
            // slot only
            ("if (a) { } return a;", 1, 1),
            ("if (-x) { } return a;", 1, 1),
            // comparison in a slot: the token pays, the slot does not
            ("if (x < y) { } return a;", 1, 1),
            ("if (x != y) { } return a;", 1, 1),
            // outside any slot: only the token arm can pay
            ("int c = x < y; return c;", 1, 0),
            ("int c = x >= y; return c;", 1, 0),
            ("int c = x == y; return c;", 1, 0),
            // the chain: two operand slots, nothing for the token
            ("return a && b;", 2, 1),
            ("return a || b;", 2, 1),
            ("return a && x > 1;", 2, 1),
            ("if (a && b || x > 1) { } return a;", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "c")]
fn c_rows_feed_each_abc_path_independently() {
    assert_c_rows(LANG::C, "int f(int a, int b, int x, int y) { BODY }\n");
    // A `<` error recovery strands under an `ERROR` pays nothing.
    let options = MetricsOptions::default().with_only(&[Metric::Abc]);
    let root = space_verbatim(LANG::C, b"int f(int a) { return a < ;\n", options);
    assert_eq!(root.metrics.abc.conditions(), 0);
}

#[test]
#[cfg(feature = "objc")]
fn objc_rows_feed_each_abc_path_independently() {
    assert_c_rows(
        LANG::Objc,
        "@implementation K\n- (int)f:(int)a b:(int)b x:(int)x y:(int)y { BODY }\n@end\n",
    );
    // A protocol list's `<` / `>` pay nothing: not under a
    // `binary_expression`.
    assert_rows(
        LANG::Objc,
        "@implementation K\n- (int)f:(int)a b:(int)b x:(int)x y:(int)y { BODY }\n@end\n",
        &[("id<NSCopying> o = nil; return a;", 0, 0)],
    );
}

#[test]
#[cfg(feature = "go")]
fn go_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Go,
        "package p\nfunc f(a, b bool, x, y int) bool { BODY }\n",
        &[
            ("if a { }; return a", 1, 1),
            ("if x < y { }; return a", 1, 1),
            ("if x != y { }; return a", 1, 1),
            ("c := x < y; return c", 1, 0),
            ("c := x >= y; return c", 1, 0),
            ("return a && b", 2, 1),
            ("return a || b", 2, 1),
            ("return a && x > 1", 2, 1),
            ("if a && b || x > 1 { }; return a", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "rust")]
fn rust_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Rust,
        "fn f(a: bool, b: bool, x: i32, y: i32, o: Option<i32>) -> bool { BODY }\n",
        &[
            ("if a { } a", 1, 1),
            ("if x < y { } a", 1, 1),
            ("if x != y { } a", 1, 1),
            ("let c = x < y; c", 1, 0),
            ("let c = x >= y; c", 1, 0),
            // a generic opener and a turbofish pay nothing
            ("let v: Vec<Vec<i32>> = Vec::<Vec<i32>>::new(); a", 0, 0),
            ("a && b", 2, 1),
            ("a || b", 2, 1),
            ("a && x > 1", 2, 1),
            ("if a && b || x > 1 { } a", 3, 3),
            // a let-chain owns its `&&` tokens directly
            ("if let Some(n) = o && n > 1 && a { } a", 3, 3),
            // an empty closure's bars pay nothing; a `&&` inside a
            // macro's token tree is a decision to cyclomatic, as it
            // always was, but owns no chain whose operands could pay
            ("let c = || a; c()", 0, 0),
            ("m!(a && b); a", 0, 1),
        ],
    );
}

#[test]
#[cfg(feature = "lua")]
fn lua_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Lua,
        "local function f(a, b, x, y)\n  BODY\nend\n",
        &[
            ("if a then end return a", 1, 1),
            ("if x < y then end return a", 1, 1),
            ("if x ~= y then end return a", 1, 1),
            ("local c = x < y return c", 1, 0),
            ("local c = x >= y return c", 1, 0),
            // an attribute's brackets pay nothing (#1297)
            ("local k <const> = 1 return a", 0, 0),
            ("return a and b", 2, 1),
            ("return a or b", 2, 1),
            ("return a and x > 1", 2, 1),
            ("if a and b or x > 1 then end return a", 3, 3),
        ],
    );
}

/// Tcl and iRules: a `binop_expr` names no operator field, so each row's
/// owner is gated on holding a payer child and the row's projection is
/// the lookup. The `f` function is a `proc`.
#[cfg(any(feature = "tcl", feature = "irules"))]
fn assert_tcl_rows(lang: LANG) {
    assert_rows(
        lang,
        "proc f {a b x y} {\n    BODY\n}\n",
        &[
            ("if { $a } { }", 1, 1),
            ("if { $x < $y } { }", 1, 1),
            ("if { $x != $y } { }", 1, 1),
            ("if { $a eq \"x\" } { }", 1, 1),
            ("if { $a in {1 2} } { }", 1, 1),
            // outside any slot: only the token arm can pay
            ("set c [expr { $x < $y }]", 1, 0),
            ("set c [expr { $a ne \"x\" }]", 1, 0),
            ("set c [expr { $a in {1 2} }]", 1, 0),
            // the chain: two operand slots, nothing for the token
            ("set c [expr { $a && $b }]", 2, 1),
            ("set c [expr { $a || $b }]", 2, 1),
            ("if { $a && $x > 1 } { }", 2, 2),
            ("if { $a && $b || $x > 1 } { }", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "tcl")]
fn tcl_rows_feed_each_abc_path_independently() {
    assert_tcl_rows(LANG::Tcl);
}

#[test]
#[cfg(feature = "irules")]
fn irules_rows_feed_each_abc_path_independently() {
    assert_tcl_rows(LANG::Irules);
    // iRules' own spellings: the string comparators and the word-form
    // chain operators.
    assert_rows(
        LANG::Irules,
        "proc f {a b x y} {\n    BODY\n}\n",
        &[
            ("if { $a starts_with \"x\" } { }", 1, 1),
            ("if { $a contains \"x\" } { }", 1, 1),
            ("set c [expr { $a matches_regex \"x\" }]", 1, 0),
            ("set c [expr { $a and $b }]", 2, 1),
            ("if { $a or $b || $x > 1 } { }", 3, 3),
        ],
    );
}

/// The JS family: one fixture through the four grammars' rows. A JSX
/// tag delimiter spells `<` / `>` and pays nothing (#1297) in the three
/// grammars that parse JSX as JSX — plain TypeScript reads `<div>` as a
/// type assertion and the rest as a comparison, so that row is the
/// JSX grammars' alone; `??` keeps its own arm pending `NullSafe`.
#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_js_rows(lang: LANG) {
    assert_rows(
        lang,
        "function f(a, b, x, y, o) { BODY }\n",
        &[
            ("if (a) { } return a;", 1, 1),
            ("if (x < y) { } return a;", 1, 1),
            ("if (x !== y) { } return a;", 1, 1),
            ("if (o instanceof Object) { } return a;", 1, 1),
            ("let c = x < y; return c;", 1, 0),
            ("let c = x >= y; return c;", 1, 0),
            ("let c = x === y; return c;", 1, 0),
            ("let c = o instanceof Object; return c;", 1, 0),
            ("return a && b;", 2, 1),
            ("return a || b;", 2, 1),
            ("return a && x > 1;", 2, 1),
            ("if (a && b || x > 1) { } return a;", 3, 3),
        ],
    );
}

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_jsx_pays_nothing(lang: LANG) {
    assert_rows(
        lang,
        "function f(a, b, x, y, o) { BODY }\n",
        &[("let c = <div><span>{x}</span></div>; return a;", 0, 0)],
    );
}

#[test]
#[cfg(feature = "javascript")]
fn javascript_rows_feed_each_abc_path_independently() {
    assert_js_rows(LANG::Javascript);
    assert_jsx_pays_nothing(LANG::Javascript);
}

#[test]
#[cfg(feature = "mozjs")]
fn mozjs_rows_feed_each_abc_path_independently() {
    assert_js_rows(LANG::Mozjs);
    assert_jsx_pays_nothing(LANG::Mozjs);
}

#[test]
#[cfg(feature = "typescript")]
fn typescript_and_tsx_rows_feed_each_abc_path_independently() {
    assert_js_rows(LANG::Typescript);
    assert_js_rows(LANG::Tsx);
    assert_jsx_pays_nothing(LANG::Tsx);
    assert_rows(
        LANG::Typescript,
        "function f<T>(a: boolean, x: number, l: Array<Map<string, T>>): boolean { BODY }\n",
        &[("let v: Array<Array<T>> = []; return a;", 0, 0)],
    );
}

#[test]
#[cfg(feature = "groovy")]
fn groovy_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Groovy,
        "class C { boolean f(boolean a, boolean b, int x, int y, Object o, List<String> l) { BODY } }\n",
        &[
            ("if (a) { }; return a", 1, 1),
            ("if (x < y) { }; return a", 1, 1),
            ("if (x <=> y) { }; return a", 1, 1),
            ("if (o === o) { }; return a", 1, 1),
            ("if (o =~ /x/) { }; return a", 1, 1),
            ("if (x in l) { }; return a", 1, 1),
            ("if (x !in l) { }; return a", 1, 1),
            ("if (o instanceof String) { }; return a", 1, 1),
            ("if (o !instanceof String) { }; return a", 1, 1),
            // outside any slot: only the token (or the tokenless node) pays
            ("def c = x < y; return c", 1, 0),
            ("def c = x <=> y; return c", 1, 0),
            ("def c = o ==~ /x/; return c", 1, 0),
            ("def c = x in l; return c", 1, 0),
            ("def c = x !in l; return c", 1, 0),
            ("def c = o !instanceof String; return c", 1, 0),
            // a generic bracket and a `for` header's `in` pay nothing
            (
                "List<Map<String, Integer>> m = null; for (i in l) { }; return a",
                0,
                1,
            ),
            ("return a && b", 2, 1),
            ("return a || b", 2, 1),
            ("if (a && b || x > 1) { }; return a", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "kotlin")]
fn kotlin_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Kotlin,
        "fun f(a: Boolean, b: Boolean, x: Int, y: Int, o: Any?, l: List<Int>): Boolean { BODY }\n",
        &[
            ("if (a) { }; return a", 1, 1),
            ("if (x < y) { }; return a", 1, 1),
            ("if (o === o) { }; return a", 1, 1),
            ("if (o is String) { }; return a", 1, 1),
            ("if (o !is String) { }; return a", 1, 1),
            ("if (x in l) { }; return a", 1, 1),
            ("if (x !in l) { }; return a", 1, 1),
            ("val c = x < y; return c", 1, 0),
            ("val c = x >= y; return c", 1, 0),
            ("val c = o !is String; return c", 1, 0),
            ("val c = x !in l; return c", 1, 0),
            // a generic bracket and a `for` header's `in` pay nothing
            (
                "val m: Map<String, List<Int>> = mapOf(); for (i in l) { }; return a",
                0,
                1,
            ),
            // a `when` entry's `is` / `in` belong to the entry, which pays
            (
                "return when (o) { is String -> a; in l -> b; !in l -> a; else -> b }",
                3,
                3,
            ),
            // the eager infix `and` / `or` / `xor` (`EagerLogical`) are
            // calls, not chains: a slot holding one pays once, as for any
            // call, and outside a slot nothing pays
            ("if (a and b) { }; return a", 1, 1),
            ("if (a xor b) { }; return a", 1, 1),
            ("val c = a or b; return c", 0, 0),
            ("return a && b", 2, 1),
            ("return a || b", 2, 1),
            ("if (a && b || x > 1) { }; return a", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "csharp")]
fn csharp_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Csharp,
        "class C { bool f(bool a, bool b, int x, int y, object o) { BODY } }\n",
        &[
            ("if (a) { } return a;", 1, 1),
            ("if (x < y) { } return a;", 1, 1),
            ("if (x != y) { } return a;", 1, 1),
            ("if (o is string) { } return a;", 1, 1),
            ("if (o is string s) { } return a;", 1, 1),
            ("if (o is not null) { } return a;", 1, 1),
            ("bool c = x < y; return c;", 1, 0),
            ("bool c = x >= y; return c;", 1, 0),
            ("bool c = o is string; return c;", 1, 0),
            ("bool c = o is not null; return c;", 1, 0),
            // a generic bracket pays nothing
            (
                "var l = new System.Collections.Generic.List<int>(); return a;",
                0,
                0,
            ),
            ("return a && b;", 2, 1),
            ("return a || b;", 2, 1),
            ("if (a && b || x > 1) { } return a;", 3, 3),
        ],
    );
    // A relational pattern's operator belongs to the arm that owns it
    // (#1383); an operator declaration names rather than applies
    // (#1420). Each scores what its control does.
    assert_rows(
        LANG::Csharp,
        "class C { int f(int x) { BODY } }\n",
        &[
            ("return x switch { > 5 => 1, _ => 0 };", 1, 1),
            ("return x switch { 5 => 1, _ => 0 };", 1, 1),
        ],
    );
    // An `is` keyword error recovery strands under an `ERROR` (the
    // expression lost at end of input) pays for no type test, as the
    // expression node it replaced scored nothing there.
    let options = MetricsOptions::default().with_only(&[Metric::Abc]);
    let root = space_verbatim(
        LANG::Csharp,
        b"class C { void F() { var x = o is\n",
        options,
    );
    assert_eq!(root.metrics.abc.conditions(), 0);
}

#[test]
#[cfg(feature = "php")]
fn php_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Php,
        "<?php\nfunction f($a, $b, $x, $y, $o) { BODY }\n",
        &[
            ("if ($a) { } return $a;", 1, 1),
            ("if ($x < $y) { } return $a;", 1, 1),
            ("if ($x <> $y) { } return $a;", 1, 1),
            ("if ($x <=> $y) { } return $a;", 1, 1),
            ("if ($o instanceof C) { } return $a;", 1, 1),
            ("$c = $x < $y; return $c;", 1, 0),
            // a `<` error recovery strands pays, as it always did: the
            // grammar spells it nowhere but as a comparison
            ("if ($x <", 1, 0),
            ("$c = $x !== $y; return $c;", 1, 0),
            ("$c = $o instanceof C; return $c;", 1, 0),
            ("return $a && $b;", 2, 1),
            ("return $a || $b;", 2, 1),
            ("return $a and $b;", 2, 1),
            ("return $a or $b;", 2, 1),
            // `xor` (`EagerLogical`) evaluates both operands: no chain, the
            // slot pays once, and outside a slot nothing pays
            ("if ($a xor $b) { } return $a;", 1, 1),
            ("$c = $a xor $b; return $c;", 0, 0),
            ("if ($a and $b || $x > 1) { } return $a;", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "ruby")]
fn ruby_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Ruby,
        "def f(a, b, x, y, o)\n  BODY\nend\n",
        &[
            ("if a then end; a", 1, 1),
            ("if x < y then end; a", 1, 1),
            ("if x <=> y then end; a", 1, 1),
            ("if o =~ /x/ then end; a", 1, 1),
            ("if o !~ /x/ then end; a", 1, 1),
            ("if a === b then end; a", 1, 1),
            ("c = x < y; c", 1, 0),
            ("c = x >= y; c", 1, 0),
            ("c = o =~ /x/; c", 1, 0),
            // an operator method's name is no comparison
            ("def ==(o); end; a", 0, 0),
            ("a && b", 2, 1),
            ("a || b", 2, 1),
            ("a and b", 2, 1),
            ("a or b", 2, 1),
            ("if a and b || x > 1 then end; a", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "elixir")]
fn elixir_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Elixir,
        "defmodule M do\n  def f(a, b, x, y, l) do\n    BODY\n  end\nend\n",
        &[
            ("if a, do: 1", 1, 1),
            ("if x < y, do: 1", 1, 1),
            ("if x === y, do: 1", 1, 1),
            ("if x in l, do: 1", 1, 1),
            ("if x not in l, do: 1", 1, 1),
            ("c = x < y", 1, 0),
            ("c = x >= y", 1, 0),
            ("c = x not in l", 1, 0),
            // a capture names the operator and applies nothing
            ("c = &==/2", 0, 0),
            ("a && b", 2, 1),
            ("a || b", 2, 1),
            ("a and b", 2, 1),
            ("a or b", 2, 1),
            // the row's named extra: a `Kernel` remote call applies it
            ("Kernel.||(a, b)", 2, 1),
            ("if a and b || x > 1, do: 1", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "perl")]
fn perl_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Perl,
        "sub f { my ($a, $b, $x, $y, $s) = @_; BODY }\n",
        &[
            ("if ($a) { }", 1, 1),
            ("if ($x < $y) { }", 1, 1),
            ("if ($x <=> $y) { }", 1, 1),
            ("if ($s eq \"x\") { }", 1, 1),
            ("if ($s cmp \"x\") { }", 1, 1),
            ("if ($s =~ /x/) { }", 1, 1),
            ("my $c = $x < $y;", 1, 0),
            ("my $c = $s ne \"x\";", 1, 0),
            ("my $c = $s !~ /x/;", 1, 0),
            // a binding that rewrites is an edit, no test (#1540); a
            // comparison against a substitution's result is a comparison,
            // inside a slot (where the slot would otherwise pay for it)
            // and outside one (where only the token can)
            ("my $c = $s =~ s/a/b/;", 0, 0),
            ("if ($x == s/a/b/) { }", 1, 1),
            ("my $c = $x == s/a/b/;", 1, 0),
            // a readline's brackets are no comparison
            ("my $l = <$s>;", 0, 0),
            ("my $c = $a && $b;", 2, 1),
            ("my $c = $a || $b;", 2, 1),
            ("my $c = $a and $b;", 2, 1),
            ("my $c = $a or $b;", 2, 1),
            // an auto-quoted `and` key is no operator (#1539)
            ("my %h = (and => 1);", 0, 0),
            // `xor` (`EagerLogical`) evaluates both operands: the slot
            // pays once, and outside a slot nothing pays
            ("if ($a xor $b) { }", 1, 1),
            ("my $c = $a xor $b;", 0, 0),
            ("if ($a and $b || $x > 1) { }", 3, 3),
        ],
    );
}

#[test]
#[cfg(feature = "bash")]
fn bash_rows_feed_each_abc_path_independently() {
    assert_rows(
        LANG::Bash,
        "f() {\n    BODY\n}\n",
        &[
            // an `if` is the condition; its command is a branch
            ("if cmd; then :; fi", 1, 1),
            ("if [[ $a == x ]]; then :; fi", 2, 1),
            ("if [[ $a =~ x ]]; then :; fi", 2, 1),
            ("if (( a < b )); then :; fi", 2, 1),
            ("if [ \"$a\" -lt \"$b\" ]; then :; fi", 2, 1),
            ("if [ -z \"$a\" ]; then :; fi", 2, 1),
            // outside any slot: a comparison in arithmetic
            ("x=$(( a <= b ))", 1, 0),
            // a redirection's `>` pays nothing
            ("echo a > out", 0, 0),
            // `&&` / `||` are decisions to cyclomatic and nothing to ABC
            // (#1536); `-a` / `-o` likewise
            ("cmd1 && cmd2 || cmd3", 0, 2),
            ("if [[ $a == x && $b != y ]]; then :; fi", 3, 2),
            ("if [ -n \"$a\" -a -n \"$b\" ]; then :; fi", 3, 2),
        ],
    );
}

// ---- `Ternary` (#1695) ---------------------------------------------------
//
// One row per language that has a ternary: the construct outside any
// slot (the row pays its decision — at the `?` where the token pays, at
// the node where the node does — and its condition slot pays one more,
// Fitzpatrick Rule 6), inside a slot (that slot does not pay again), with
// both branches negated (the slot walker's operand read), and — where
// the grammar spells `?` elsewhere — that spelling paying nothing.
// Cyclomatic is the oracle: a ternary is one decision everywhere.

#[test]
#[cfg(any(feature = "c", feature = "objc"))]
fn c_family_ternary_row_pays_once() {
    #[cfg(feature = "c")]
    assert_rows(
        LANG::C,
        "int f(int a, int b, int x, int y) { BODY }\n",
        &[
            ("int c = a ? b : x; return c;", 2, 1),
            ("if (a ? b : x) { } return a;", 2, 2),
            ("int c = a ? !b : !x; return c;", 4, 1),
            // the GNU elision is the same node with no consequence
            ("int c = a ?: x; return c;", 2, 1),
        ],
    );
    #[cfg(feature = "objc")]
    assert_rows(
        LANG::Objc,
        "@implementation K\n- (int)f:(int)a b:(int)b x:(int)x y:(int)y { BODY }\n@end\n",
        &[
            ("int c = a ? b : x; return c;", 2, 1),
            ("if (a ? b : x) { } return a;", 2, 2),
            ("int c = a ? !b : !x; return c;", 4, 1),
        ],
    );
}

#[test]
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn cpp_ternary_row_pays_once() {
    for lang in [LANG::Cpp, LANG::Mozcpp] {
        if !lang.is_enabled() {
            continue;
        }
        assert_rows(
            lang,
            "int f(int a, int b, int x, int y) { BODY }\n",
            &[
                ("int c = a ? b : x; return c;", 2, 1),
                ("if (a ? b : x) { } return a;", 2, 2),
                ("int c = a ? !b : !x; return c;", 4, 1),
                ("int c = a ?: x; return c;", 2, 1),
            ],
        );
    }
}

#[test]
#[cfg(feature = "java")]
fn java_ternary_row_pays_once_and_a_wildcard_nothing() {
    assert_rows(
        LANG::Java,
        "class A { boolean f(boolean a, boolean b, int x, java.util.List<?> l) { BODY } }\n",
        &[
            ("boolean c = a ? b : a; return c;", 2, 1),
            ("if (a ? b : a) { } return a;", 2, 2),
            ("boolean c = a ? !b : !a; return c;", 4, 1),
            // a wildcard bound's `?` is type syntax (#1274)
            ("java.util.List<? extends Number> m = null; return a;", 0, 0),
        ],
    );
}

#[test]
#[cfg(feature = "groovy")]
fn groovy_ternary_row_pays_once_and_a_wildcard_nothing() {
    assert_rows(
        LANG::Groovy,
        "class C { boolean f(boolean a, boolean b, int x, List<?> l) { BODY } }\n",
        &[
            ("def c = a ? b : a; return c", 2, 1),
            ("if (a ? b : a) { }; return a", 2, 2),
            ("def c = a ? !b : !a; return c", 4, 1),
            ("List<? extends Number> m = null; return a", 0, 0),
            // the elvis is `NullSafe`'s: one per token, as before
            ("def c = a ?: b; return c", 1, 1),
        ],
    );
}

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_js_ternary(lang: LANG) {
    assert_rows(
        lang,
        "function f(a, b, x, y, o) { BODY }\n",
        &[
            ("let c = a ? b : x; return c;", 2, 1),
            ("if (a ? b : x) { } return a;", 2, 2),
            ("let c = a ? !b : !x; return c;", 4, 1),
        ],
    );
}

// A `?` error recovery strands under an `ERROR` (its ternary lost at end
// of input) has always paid in the JavaScript grammars, whose only `?`
// is the ternary's; the TypeScript grammars gate theirs and it pays
// nothing there, as before.
#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_js_recovered_question_mark(lang: LANG, conditions: u64) {
    let options = MetricsOptions::default().with_only(&[Metric::Abc]);
    let root = space_verbatim(lang, b"let c = a ?\n", options);
    assert_eq!(root.metrics.abc.conditions(), conditions, "{lang:?}");
}

#[test]
#[cfg(feature = "javascript")]
fn javascript_ternary_row_pays_once() {
    assert_js_ternary(LANG::Javascript);
    assert_js_recovered_question_mark(LANG::Javascript, 1);
}

#[test]
#[cfg(feature = "mozjs")]
fn mozjs_ternary_row_pays_once() {
    assert_js_ternary(LANG::Mozjs);
    assert_js_recovered_question_mark(LANG::Mozjs, 1);
}

#[test]
#[cfg(feature = "typescript")]
fn typescript_ternary_row_pays_once_and_type_syntax_nothing() {
    assert_js_ternary(LANG::Typescript);
    assert_js_ternary(LANG::Tsx);
    assert_js_recovered_question_mark(LANG::Typescript, 0);
    assert_js_recovered_question_mark(LANG::Tsx, 0);
    // Three of the ten type productions that spell `?` (#1275): an
    // optional parameter and member, an optional tuple member, a
    // conditional type.
    assert_rows(
        LANG::Typescript,
        "function f<T>(a: boolean, x?: number, o?: { p?: string }): boolean { BODY }\n",
        &[(
            "type U = T extends string ? 1 : 2; let t: [number, string?] = [1]; return a;",
            0,
            0,
        )],
    );
}

#[test]
#[cfg(feature = "csharp")]
fn csharp_ternary_row_pays_once_and_type_syntax_nothing() {
    assert_rows(
        LANG::Csharp,
        "class C { bool f(bool a, bool b, int x, object o) { BODY } }\n",
        &[
            ("bool c = a ? b : a; return c;", 2, 1),
            ("if (a ? b : a) { } return a;", 2, 2),
            ("bool c = a ? !b : !a; return c;", 4, 1),
            // a nullable type's `?` is type syntax (#1275); a conditional
            // access's is the `NullSafe` row's and still pays, as before
            ("int? n = null; return a;", 0, 0),
            ("var s = o?.ToString(); return a;", 1, 1),
        ],
    );
}

#[test]
#[cfg(feature = "ruby")]
fn ruby_ternary_row_pays_once() {
    assert_rows(
        LANG::Ruby,
        "def f(a, b, x, y, o)\n  BODY\nend\n",
        &[
            ("c = a ? b : x; c", 2, 1),
            ("if a ? b : x then end; a", 2, 2),
            ("c = a ? !b : !x; c", 4, 1),
            // a character literal and a predicate name are their own tokens
            ("c = ?a; d = x.nil?; a", 0, 0),
        ],
    );
}

#[test]
#[cfg(feature = "perl")]
fn perl_ternary_row_pays_once() {
    assert_rows(
        LANG::Perl,
        "sub f { my ($a, $b, $x, $y, $s) = @_; BODY }\n",
        &[
            ("my $c = $a ? $b : $x;", 2, 1),
            ("if ($a ? $b : $x) { }", 2, 2),
            ("my $c = $a ? !$b : !$x;", 4, 1),
        ],
    );
}

#[test]
#[cfg(feature = "php")]
fn php_ternary_row_pays_once_and_an_optional_type_nothing() {
    assert_rows(
        LANG::Php,
        "<?php\nfunction f($a, $b, $x, $y, $o) { BODY }\n",
        &[
            ("$c = $a ? $b : $x; return $c;", 2, 1),
            ("if ($a ? $b : $x) { } return $a;", 2, 2),
            ("$c = $a ? !$b : !$x; return $c;", 4, 1),
            // the short form is the same node with no body
            ("$c = $a ?: $x; return $c;", 2, 1),
        ],
    );
    assert_rows(
        LANG::Php,
        "<?php\nfunction f(?int $a) { BODY }\n",
        &[("return $a;", 0, 0)],
    );
}

#[test]
#[cfg(feature = "python")]
fn python_ternary_row_pays_once() {
    assert_rows(
        LANG::Python,
        "def f(a, b, x, y):\n    BODY\n",
        &[
            ("c = b if a else x; return c", 2, 1),
            ("if (b if a else x): pass\n    return a", 2, 2),
        ],
    );
}

#[cfg(any(feature = "tcl", feature = "irules"))]
fn assert_tcl_ternary(lang: LANG) {
    assert_rows(
        lang,
        "proc f {a b x y} {\n    BODY\n}\n",
        &[
            ("set c [expr { $a ? $b : $x }]", 2, 1),
            ("if { $a ? $b : $x } { }", 2, 2),
            ("set c [expr { $a ? !$b : !$x }]", 4, 1),
            // a parenthesised condition shifts every operand (#1180)
            ("set c [expr { ($a) ? !$b : $x }]", 3, 1),
        ],
    );
}

#[test]
#[cfg(feature = "tcl")]
fn tcl_ternary_row_pays_once() {
    assert_tcl_ternary(LANG::Tcl);
}

#[test]
#[cfg(feature = "irules")]
fn irules_ternary_row_pays_once() {
    assert_tcl_ternary(LANG::Irules);
}

#[test]
#[cfg(feature = "bash")]
fn bash_ternary_row_pays_once_and_an_expansion_nothing() {
    assert_rows(
        LANG::Bash,
        "f() {\n    BODY\n}\n",
        &[
            // Bash has no slot walker: the node alone pays
            ("c=$(( a ? b : x ))", 1, 1),
            ("c=$(( a ? (b ? 1 : 2) : x ))", 2, 2),
            // an expansion's `?` and `$?` are no ternary
            ("echo ${y?msg} $?", 0, 0),
        ],
    );
}

// ---- `CaseArm` and `Guard` (#1695) ----------------------------------------
//
// One fixture per language with a multi-way branch: every arm the row
// owns is one condition and one decision, the fallback arm (`default`,
// `_`, `else`, `*)`, `true`) is neither, and a guard is one more of
// each — its expression a boolean slot, so a negated guard pays as
// `if (!x)` does. Expected values are the pre-row binary's; cyclomatic
// is read from the same rows, so the two columns are the oracle for
// each other except where a policy comment says why they part.

#[test]
#[cfg(any(feature = "c", feature = "objc"))]
fn c_family_case_arms_pay_at_the_case_label() {
    #[cfg(feature = "c")]
    assert_rows(
        LANG::C,
        "int f(int x, int y) { BODY }\n",
        // a fall-through run `case 2: case 3:` is two labels
        &[(
            "switch (x) { case 1: return 1; case 2: case 3: return 2; default: return 0; } return 0;",
            3,
            3,
        )],
    );
    #[cfg(feature = "objc")]
    assert_rows(
        LANG::Objc,
        "@implementation K\n- (int)f:(int)x y:(int)y { BODY }\n@end\n",
        &[(
            "switch (x) { case 1: return 1; default: return 0; } return 0;",
            1,
            1,
        )],
    );
}

#[test]
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn cpp_case_arms_pay_at_the_case_label() {
    for lang in [LANG::Cpp, LANG::Mozcpp] {
        if !lang.is_enabled() {
            continue;
        }
        assert_rows(
            lang,
            "int f(int x, int y) { BODY }\n",
            &[(
                "switch (x) { case 1: return 1; case 2: case 3: return 2; default: return 0; } return 0;",
                3,
                3,
            )],
        );
    }
}

#[test]
#[cfg(feature = "java")]
fn java_case_arms_and_guards_pay_once_each() {
    assert_rows(
        LANG::Java,
        "class A { int f(int x, Object o) { BODY } }\n",
        &[
            (
                "switch (x) { case 1: return 1; case 2: case 3: return 2; default: return 0; }",
                3,
                3,
            ),
            // a multi-pattern arrow arm is one label; the guard is one
            // more, and its expression a slot
            (
                "return switch (o) { case String s when s.isEmpty() -> 1; case Integer i, Long l -> 2; default -> 0; };",
                3,
                3,
            ),
            (
                "return switch (o) { case String s when !s.isEmpty() -> 1; default -> 0; };",
                2,
                2,
            ),
        ],
    );
}

#[test]
#[cfg(feature = "groovy")]
fn groovy_case_arms_pay_at_the_case_label() {
    assert_rows(
        LANG::Groovy,
        "class C { int f(int x, Object o) { BODY } }\n",
        &[(
            "switch (x) { case 1: return 1; case 2: return 2; default: return 0 }",
            2,
            2,
        )],
    );
}

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_js_case_arms(lang: LANG) {
    assert_rows(
        lang,
        "function f(x, o) { BODY }\n",
        &[(
            "switch (x) { case 1: return 1; case 2: case 3: return 2; default: return 0; }",
            3,
            3,
        )],
    );
}

#[test]
#[cfg(feature = "javascript")]
fn javascript_case_arms_pay_at_the_case_label() {
    assert_js_case_arms(LANG::Javascript);
}

#[test]
#[cfg(feature = "mozjs")]
fn mozjs_case_arms_pay_at_the_case_label() {
    assert_js_case_arms(LANG::Mozjs);
}

#[test]
#[cfg(feature = "typescript")]
fn typescript_and_tsx_case_arms_pay_at_the_case_label() {
    assert_js_case_arms(LANG::Typescript);
    assert_js_case_arms(LANG::Tsx);
}

#[test]
#[cfg(feature = "csharp")]
fn csharp_case_arms_and_guards_pay_once_each() {
    assert_rows(
        LANG::Csharp,
        "class C { int f(int x, object o) { BODY } }\n",
        &[
            // each `case` label of a fall-through section
            (
                "switch (x) { case 1: case 2: return 1; default: return 0; }",
                2,
                2,
            ),
            // an expression arm pays at the node; the bare discard does not
            (
                "return o switch { int i when i > 0 => 1, string => 2, _ => 0 };",
                3,
                3,
            ),
            // a guarded discard pays the arm and the guard; `var _` is bare
            ("return o switch { _ when x > 0 => 1, var _ => 0 };", 2, 2),
            // an exception filter is a guard; `try` and `catch` pay beside it
            (
                "try { } catch (System.Exception e) when (x > 0) { } return 0;",
                3,
                2,
            ),
            // `goto case` is a jump, not an arm (#1450)
            (
                "switch (x) { case 1: goto case 2; case 2: return 1; default: goto default; }",
                2,
                2,
            ),
        ],
    );
}

#[test]
#[cfg(feature = "kotlin")]
fn kotlin_when_entries_pay_at_the_node() {
    assert_rows(
        LANG::Kotlin,
        "fun f(x: Int, o: Any): Int { BODY }\n",
        &[
            (
                "return when (x) { 1 -> 1; 2, 3 -> 2; in 4..5 -> 3; else -> 0 }",
                3,
                3,
            ),
            // a subject-less entry's condition is a slot its own test pays
            (
                "return when { x > 0 -> 1; o is String -> 2; else -> 0 }",
                2,
                2,
            ),
        ],
    );
}

#[test]
#[cfg(feature = "rust")]
fn rust_match_arms_and_guards_pay_once_each() {
    assert_rows(
        LANG::Rust,
        "fn f(x: Option<i32>, b: bool) -> i32 { BODY }\n",
        &[
            // three arms (`_ =>` is none), two guards, and one guard's
            // comparison; `_ if b` is an arm by its guard
            (
                "match x { Some(n) if n > 0 => 1, Some(_) => 2, _ if b => 3, _ => 0 }",
                5,
                5,
            ),
            // a leading `|` on a bare `_` makes the arm count, as it
            // always has: the pattern's named children are what the gate
            // reads, and tree-sitter-rust nests `| _` in an `or_pattern`
            ("match x { | _ => 0 }", 1, 1),
        ],
    );
}

#[test]
#[cfg(feature = "python")]
fn python_case_clauses_and_guards_pay_once_each() {
    assert_rows(
        LANG::Python,
        "def f(x, b):\n    BODY\n",
        &[(
            "match x:\n        case 1 if b: return 1\n        case [a, *_]: return 2\n        case _ if b: return 3\n        case _: return 0",
            5,
            5,
        )],
    );
}

#[test]
#[cfg(feature = "go")]
fn go_case_arms_pay_at_the_node() {
    assert_rows(
        LANG::Go,
        "package p\nfunc f(x int, b bool, c chan int) int { BODY }\n",
        &[
            ("switch x { case 1, 2: return 1; default: return 0 }", 1, 1),
            // a tagless switch's case expressions are boolean slots
            (
                "switch { case b: return 1; case x > 0: return 2; default: return 0 }",
                2,
                2,
            ),
            ("select { case <-c: return 1; default: return 0 }", 1, 1),
        ],
    );
}

#[test]
#[cfg(feature = "ruby")]
fn ruby_case_arms_and_guards_pay_once_each() {
    assert_rows(
        LANG::Ruby,
        "def f(x, b)\n  BODY\nend\n",
        &[
            // `when 2, 3` is an implicit `||`: two operand slots (#1453)
            (
                "case x\n  when 1 then 1\n  when 2, 3 then 2\n  else 0\n  end",
                3,
                2,
            ),
            // three `in` arms (`in _` is none), two guards, one of them
            // what makes its `_` an arm
            (
                "case x\n  in Integer if b then 1\n  in [a, *] then 2\n  in _ unless b then 3\n  in _ then 0\n  end",
                5,
                5,
            ),
        ],
    );
}

#[test]
#[cfg(feature = "php")]
fn php_case_arms_pay_at_the_node() {
    assert_rows(
        LANG::Php,
        "<?php\nfunction f($x, $o) { BODY }\n",
        &[
            (
                "switch ($x) { case 1: return 1; case 2: return 2; default: return 0; }",
                2,
                2,
            ),
            (
                "return match ($x) { 1, 2 => 1, 3 => 2, default => 0 };",
                2,
                2,
            ),
        ],
    );
}

#[test]
#[cfg(feature = "elixir")]
fn elixir_clauses_and_guards_pay_once_each() {
    assert_rows(
        LANG::Elixir,
        "defmodule M do\n  def f(x, b) do\n    BODY\n  end\nend\n",
        &[
            // two arms (`_ ->` is none) and one guard
            (
                "case x do\n      1 -> 1\n      n when n > 0 -> 2\n      _ -> 0\n    end",
                3,
                3,
            ),
            (
                "cond do\n      b -> 1\n      x > 0 -> 2\n      true -> 0\n    end",
                2,
                2,
            ),
        ],
    );
    // A guard on a function head is one condition of the file's, with
    // nothing else in it.
    let options = MetricsOptions::default().with_only(&[Metric::Abc, Metric::Cyclomatic]);
    let root = space_verbatim(
        LANG::Elixir,
        b"defmodule M do\n  def f(x) when is_integer(x) do\n    x\n  end\nend\n",
        options,
    );
    assert_eq!(root.metrics.abc.conditions_sum(), 1);
}

#[test]
#[cfg(feature = "bash")]
fn bash_case_items_pay_at_the_node() {
    assert_rows(
        LANG::Bash,
        "f() {\n    BODY\n}\n",
        // `b|c)` is not bare; `*)` is the catch-all
        &[("case $x in a) :;; b|c) :;; *) :;; esac", 2, 2)],
    );
}

#[cfg(any(feature = "tcl", feature = "irules"))]
fn assert_tcl_switch_arms(lang: LANG, body: &str, decisions: u64) {
    // Tcl ABC has never scored a switch arm as a condition — each arm is
    // a command, a branch — so only cyclomatic reads the row here.
    assert_rows(lang, "proc f {x} {\n    BODY\n}\n", &[(body, 0, decisions)]);
}

#[test]
#[cfg(feature = "tcl")]
fn tcl_switch_arms_pay_at_the_arm_command() {
    // `b -` falls through and is an arm; `default` is none
    assert_tcl_switch_arms(
        LANG::Tcl,
        "switch -- $x {\n        a { return 1 }\n        b -\n        c { return 2 }\n        default { return 0 }\n    }",
        3,
    );
    // An arm whose pattern spells `for` or `switch` is a command the
    // name dispatch reads as that command too, as it always has: the
    // grammar cannot tell the arm from the command, and the row's read
    // adds to that reading rather than replacing it.
    assert_tcl_switch_arms(
        LANG::Tcl,
        "switch $x {\n        for { puts yes }\n        default { puts no }\n    }",
        2,
    );
    // The split form has no arm list and owns nothing (#467).
    assert_tcl_switch_arms(LANG::Tcl, "switch $x a { return 1 } b { return 2 }", 0);
}

#[test]
#[cfg(feature = "irules")]
fn irules_switch_arms_pay_at_the_arm_node() {
    assert_tcl_switch_arms(
        LANG::Irules,
        "switch -- $x {\n        a { return 1 }\n        b { return 2 }\n        default { return 0 }\n    }",
        2,
    );
    // An arm error recovery strands under an `ERROR` (its `switch` lost
    // at end of input) is no arm, as it never was.
    let options = MetricsOptions::default().with_only(&[Metric::Cyclomatic]);
    let root = space_verbatim(
        LANG::Irules,
        b"when HTTP_REQUEST { switch -- [HTTP::uri] { \"/a\" { return }\n",
        options,
    );
    assert_eq!(root.metrics.cyclomatic.cyclomatic_sum(), 1);
}

// ---- `NullSafe` (#1695) -------------------------------------------------
//
// One fixture per language with a null-safe operator: the coalescing
// form (`??`, elvis `?:`, defined-or `//`) outside a slot and inside one,
// and the optional navigation (`?.`, `?->`, `&.`, `?[`) in a chain. The
// two columns part by policy, recorded on each row: ABC scores a
// coalescing operator as a condition where the language's ABC always
// did (not PHP, where it is a branch's operand; Perl walks the chain's
// operands instead) and a navigation nowhere but C#, while cyclomatic
// pays every token. The compound `??=` / `//=` is excluded on every row
// and keeps its consumers' own reading: a decision, no condition.

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
fn assert_js_null_safe(lang: LANG, template: &str) {
    assert_rows(
        lang,
        template,
        &[
            ("let c = a ?? b; return c;", 1, 1),
            ("if (a ?? b) { } return a;", 1, 2),
            // three `?.` in a chain, one decision each and no condition
            ("let c = o?.p?.[0]?.(); return c;", 0, 3),
            ("a ??= b; return a;", 0, 1),
        ],
    );
}

#[test]
#[cfg(feature = "javascript")]
fn javascript_null_safe_row_pays_per_token() {
    assert_js_null_safe(LANG::Javascript, "function f(a, b, o) { BODY }\n");
}

#[test]
#[cfg(feature = "mozjs")]
fn mozjs_null_safe_row_pays_per_token() {
    assert_js_null_safe(LANG::Mozjs, "function f(a, b, o) { BODY }\n");
}

#[test]
#[cfg(feature = "typescript")]
fn typescript_and_tsx_null_safe_rows_pay_per_token() {
    // The `?.` token under its `optional_chain` wrapper pays once.
    assert_js_null_safe(
        LANG::Typescript,
        "function f(a: any, b: any, o: any) { BODY }\n",
    );
    assert_js_null_safe(LANG::Tsx, "function f(a: any, b: any, o: any) { BODY }\n");
}

#[test]
#[cfg(feature = "csharp")]
fn csharp_null_safe_row_pays_coalescing_and_access() {
    assert_rows(
        LANG::Csharp,
        "class C { object f(object a, object b, object o, int[] x) { BODY } }\n",
        &[
            ("var c = a ?? b; return c;", 1, 1),
            ("if (a ?? b) { } return a;", 1, 2),
            // a conditional access pays at the node, to both consumers
            // — the C#-internal agreement no sibling shares
            ("var c = o?.ToString()?.Length; return c;", 2, 2),
            ("var c = x?[0]; return c;", 1, 1),
            ("a ??= b; return a;", 0, 1),
        ],
    );
}

#[test]
#[cfg(feature = "kotlin")]
fn kotlin_null_safe_row_pays_elvis_and_navigation() {
    assert_rows(
        LANG::Kotlin,
        "fun f(a: String?, b: String, o: String?): Any { BODY }\n",
        &[
            ("val c = a ?: b; return c", 1, 1),
            ("if (a ?: b == \"\") { }; return b", 2, 2),
            ("val c = o?.length?.toString(); return b", 0, 2),
            // `!!` throws rather than branches
            ("val c = o!!; return c", 0, 0),
        ],
    );
}

#[test]
#[cfg(feature = "groovy")]
fn groovy_null_safe_row_pays_elvis_and_navigations() {
    assert_rows(
        LANG::Groovy,
        "class C { def f(a, b, o, x) { BODY } }\n",
        &[
            ("def c = a ?: b; return c", 1, 1),
            ("if (a ?: b) { }; return a", 1, 2),
            // `?.`, `??.` and `?[`, one decision each
            ("def c = o?.p; def d = o??.q; def e = x?[0]; return c", 0, 3),
        ],
    );
}

#[test]
#[cfg(feature = "php")]
fn php_null_safe_row_pays_per_token() {
    assert_rows(
        LANG::Php,
        "<?php\nfunction f($a, $b, $o) { BODY }\n",
        &[
            // PHP ABC scores `??` nowhere but inside a slot
            ("$c = $a ?? $b; return $c;", 0, 1),
            ("if ($a ?? $b) { } return $a;", 1, 2),
            ("$c = $o?->p?->m(); return $c;", 0, 2),
            ("$a ??= $b; return $a;", 0, 1),
        ],
    );
}

#[test]
#[cfg(feature = "perl")]
fn perl_null_safe_row_walks_the_defined_or_chain() {
    assert_rows(
        LANG::Perl,
        "sub f { my ($a, $b, $x) = @_; BODY }\n",
        &[
            // each operand of a `//` chain is one condition (Rule 9)
            ("my $c = $a // $b;", 2, 1),
            ("if ($a // $b) { }", 2, 2),
            ("my $c = $a // $b // $x;", 3, 2),
            ("$a //= $b;", 0, 1),
        ],
    );
}

#[test]
#[cfg(feature = "ruby")]
fn ruby_null_safe_row_pays_per_token() {
    assert_rows(
        LANG::Ruby,
        "def f(a, b, o)\n  BODY\nend\n",
        &[("c = o&.p&.q; c", 0, 2), ("c = o&.r(1); c", 0, 1)],
    );
}
