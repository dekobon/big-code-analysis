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
    feature = "c",
    feature = "cpp",
    feature = "java",
    feature = "mozcpp",
    feature = "objc",
    feature = "python",
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
