//! A boolean slot pays one ABC condition unless an arm inside its
//! predicate already paid it (#1526, porting #1520's Ruby rule through
//! `count_boolean_slot`).
//!
//! Every slot once paid only for a predicate that peeled down to a fixed
//! list of terminal kinds, so `if -x`, `if this`, `if x + 1` each scored
//! 0 conditions against a cyclomatic decision of 1. Each row therefore
//! carries the cyclomatic decision count as its oracle, which this fix
//! did not write, beside the ABC count it did; the bare-identifier row of
//! each template is the twin every other spelling is measured against.
//! A row whose two counts differ names the documented reason (a Rule 5
//! `else` / `?` arm, or a negation outside any slot).

use crate::test_support::{function_space, space_verbatim};
use crate::{LANG, Metric, MetricsOptions};

/// The placeholder a template marks its slot with. Not `@`, which
/// Objective-C, PHP and Perl predicates spell.
const SLOT: &str = "PRED";

/// For each `(predicate, conditions, decisions)` row, replaces the
/// template's `SLOT` with the predicate and asserts the function named `f`
/// scores `conditions` ABC conditions and `decisions` cyclomatic
/// decisions (`cyclomatic - 1`).
#[track_caller]
fn assert_rows(lang: LANG, template: &str, rows: &[(&str, u64, u64)]) {
    assert!(template.contains(SLOT), "template has no predicate slot");
    let options = MetricsOptions::default().with_only(&[Metric::Abc, Metric::Cyclomatic]);
    for &(predicate, conditions, decisions) in rows {
        let source = template.replace(SLOT, predicate);
        let root = space_verbatim(lang, source.as_bytes(), options);
        let f = function_space(&root, "f");
        assert_eq!(
            (
                f.metrics.abc.conditions(),
                f.metrics.cyclomatic.cyclomatic() - 1
            ),
            (conditions, decisions),
            "(abc.conditions, decisions) of `{predicate}` in {lang:?}\n  source: {source}"
        );
    }
}

/// Asserts each of #1475's constructs pays its `if` slot and its `&&`
/// chain-operand slot exactly as the identifier `control` does: one
/// condition and one decision in the `if`, two of each in the chain.
/// The control row runs first, so a template that stopped measuring
/// fails there rather than in every construct row.
#[cfg(any(
    feature = "csharp",
    feature = "groovy",
    feature = "perl",
    feature = "ruby",
    feature = "elixir"
))]
#[track_caller]
fn assert_level_with_control(
    lang: LANG,
    [if_slot, chain_slot]: [&str; 2],
    control: &str,
    constructs: &[&str],
) {
    let predicates = || std::iter::once(control).chain(constructs.iter().copied());
    let in_if: Vec<_> = predicates().map(|p| (p, 1, 1)).collect();
    assert_rows(lang, if_slot, &in_if);
    let in_chain: Vec<_> = predicates().map(|p| (p, 2, 2)).collect();
    assert_rows(lang, chain_slot, &in_chain);
}

#[test]
#[cfg(feature = "python")]
fn python_slots_pay_for_any_predicate() {
    let if_rows = &[
        ("b", 1, 1),
        ("-b", 1, 1),
        ("~b", 1, 1),
        ("b + 1", 1, 1),
        ("lambda: b", 1, 1),
        ("(y := b)", 1, 1),
        // The walrus is peeled to the comparison its own arm counts;
        // as a terminal kind it paid the slot a second time.
        ("(y := b > 1)", 1, 1),
        ("b > 1", 1, 1),
        ("not b", 1, 1),
        // `not` pays only for an operand nothing else counts, as Java's
        // `!` does: a negated comparison or chain is paid by its own arm,
        // and a double negation once. Each scored one more before.
        ("not (b > 1)", 1, 1),
        ("not a > b", 1, 1),
        ("not not b", 1, 1),
        ("not (not (b > 1))", 1, 1),
        ("not -b", 1, 1),
        ("not (a and b)", 2, 2),
        ("a and b", 2, 2),
        ("a and -b", 2, 2),
        ("-b or a", 2, 2),
        ("a or b + 1", 2, 2),
        ("a and (y := b > 1)", 2, 2),
    ];
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    if PRED:\n        pass\n",
        if_rows,
    );
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    while PRED:\n        pass\n",
        &[("b", 1, 1), ("-b", 1, 1), ("b > 1", 1, 1)],
    );
    // Outside a slot a `not` is the only thing that makes a value a
    // condition, and a negated comparison still pays once.
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    y = PRED\n    return y\n",
        &[("b", 0, 0), ("not b", 1, 0), ("not (b > 1)", 1, 0)],
    );
    // The conditional expression is one condition of its own, as the
    // `?` token is in the C family.
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    return 1 if PRED else 2\n",
        &[("b", 2, 1), ("-b", 2, 1), ("b > 1", 2, 1)],
    );
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    match a:\n        case 1 if PRED:\n            pass\n",
        &[("b", 2, 2), ("-b", 2, 2), ("b > 1", 2, 2)],
    );
}

/// An `elif` is Java's `else if`: the `else` (Rule 5, no cyclomatic
/// decision of its own) plus a predicate slot, as Ruby's `elsif` is. It
/// paid only the first, so `elif b` scored one below `elif b > 1`.
#[test]
#[cfg(feature = "python")]
fn python_elif_is_an_else_and_a_slot() {
    assert_rows(
        LANG::Python,
        "def f(a, b):\n    if a:\n        pass\n    elif PRED:\n        pass\n",
        &[("b", 3, 2), ("-b", 3, 2), ("b > 1", 3, 2), ("not b", 3, 2)],
    );
}

/// The rows every JS-family grammar shares: the issue's predicates, each
/// beside the bare identifier and the comparison it must neither fall
/// below nor double.
#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
const JS_FAMILY_IF_ROWS: &[(&str, u64, u64)] = &[
    ("b", 1, 1),
    ("-b", 1, 1),
    ("!-b", 1, 1),
    ("this", 1, 1),
    ("!this", 1, 1),
    ("b + 1", 1, 1),
    ("typeof b", 1, 1),
    ("y = b", 1, 1),
    ("'k' in o", 1, 1),
    ("() => b", 1, 1),
    // Wrappers that evaluate to a comparison must not pay twice.
    ("y = b > 1", 1, 1),
    ("a, b > 1", 1, 1),
    ("!(b > 1)", 1, 1),
    ("b > 1", 1, 1),
    ("!b", 1, 1),
    ("a && b", 2, 2),
    ("a && -b", 2, 2),
    ("-b || a", 2, 2),
    ("a && this", 2, 2),
    ("a && (y = b > 1)", 2, 2),
];

#[cfg(any(feature = "javascript", feature = "mozjs", feature = "typescript"))]
#[track_caller]
fn assert_js_family_slots(lang: LANG) {
    let function = "function f(a, b, o, y) { PRED }";
    assert_rows(
        lang,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        JS_FAMILY_IF_ROWS,
    );
    let other_slots = &[("b", 1, 1), ("-b", 1, 1), ("this", 1, 1), ("b > 1", 1, 1)];
    for slot in [
        "while (PRED) { g(); }",
        "do { g(); } while (PRED);",
        "for (; PRED; ) { g(); }",
    ] {
        assert_rows(lang, &function.replace(SLOT, slot), other_slots);
    }
    // The `?` is a condition of its own (Rule 5) beside its slot.
    assert_rows(
        lang,
        &function.replace(SLOT, "return PRED ? 1 : 2;"),
        &[("b", 2, 1), ("-b", 2, 1), ("this", 2, 1), ("b > 1", 2, 1)],
    );
    // Outside a slot only a negation is a condition, whatever it
    // negates: a Rule 9 unary conditional with no decision behind it.
    assert_rows(
        lang,
        &function.replace(SLOT, "return PRED;"),
        &[
            ("b", 0, 0),
            ("-b", 0, 0),
            ("!b", 1, 0),
            ("!this", 1, 0),
            ("!-b", 1, 0),
            ("!(b > 1)", 1, 0),
        ],
    );
    // A call argument is no slot either; the negation still counts,
    // and `this` scores like an identifier there too (#1475).
    assert_rows(
        lang,
        &function.replace(SLOT, "g(PRED);"),
        &[("b", 0, 0), ("!b", 1, 0), ("!this", 1, 0)],
    );
}

#[test]
#[cfg(feature = "javascript")]
fn javascript_slots_pay_for_any_predicate() {
    assert_js_family_slots(LANG::Javascript);
}

/// Mozjs owns no file extension, so no corpus snapshot reaches it.
#[test]
#[cfg(feature = "mozjs")]
fn mozjs_slots_pay_for_any_predicate() {
    assert_js_family_slots(LANG::Mozjs);
}

/// TypeScript's type-only wrappers evaluate to their operand: each pays
/// as the bare operand does, and none pays again for a comparison.
#[cfg(feature = "typescript")]
const TS_TYPE_WRAPPER_ROWS: &[(&str, u64, u64)] = &[
    ("b as any", 1, 1),
    ("b!", 1, 1),
    ("b satisfies boolean", 1, 1),
    ("(b > 1) as boolean", 1, 1),
    ("(b > 1)!", 1, 1),
    ("a && (b as any)", 2, 2),
    ("!(b as any)", 1, 1),
];

#[test]
#[cfg(feature = "typescript")]
fn typescript_slots_pay_for_any_predicate() {
    assert_js_family_slots(LANG::Typescript);
    let template = "function f(a: any, b: any) { if (PRED) { g(); } }";
    assert_rows(LANG::Typescript, template, TS_TYPE_WRAPPER_ROWS);
    // The angle-bracket cast is TypeScript-only; TSX reads it as JSX.
    assert_rows(
        LANG::Typescript,
        template,
        &[("<boolean>b", 1, 1), ("<boolean>(b > 1)", 1, 1)],
    );
}

#[test]
#[cfg(feature = "typescript")]
fn tsx_slots_pay_for_any_predicate() {
    assert_js_family_slots(LANG::Tsx);
    assert_rows(
        LANG::Tsx,
        "function f(a: any, b: any) { if (PRED) { g(); } }",
        TS_TYPE_WRAPPER_ROWS,
    );
}

/// The rows the four C-family grammars share. C and C++ test any scalar
/// for non-zero, so every one of the issue's predicates is a decision.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
const C_FAMILY_IF_ROWS: &[(&str, u64, u64)] = &[
    ("b", 1, 1),
    ("-b", 1, 1),
    ("~b", 1, 1),
    ("*p", 1, 1),
    ("&b", 1, 1),
    ("b + 1", 1, 1),
    ("b & 4", 1, 1),
    ("sizeof b", 1, 1),
    ("b++", 1, 1),
    ("y = b", 1, 1),
    ("y += 1", 1, 1),
    ("!-b", 1, 1),
    ("!*p", 1, 1),
    // Wrappers that evaluate to a comparison must not pay twice. The
    // cast scored 2 while `cast_expression` was a terminal kind.
    ("y = b > 1", 1, 1),
    ("(int)(b > 1)", 1, 1),
    ("(int)b", 1, 1),
    ("a, b > 1", 1, 1),
    ("!(b > 1)", 1, 1),
    ("b > 1", 1, 1),
    ("!b", 1, 1),
    // A negated literal is a negation like any other (#1469).
    ("!\"s\"", 1, 1),
    // A ternary predicate is counted by its own `?` and pays its
    // condition slot; the `if` slot must not pay a third time.
    ("a ? b : y", 2, 2),
    ("a && b", 2, 2),
    ("a && -b", 2, 2),
    ("*p || a", 2, 2),
    ("a && (y = b > 1)", 2, 2),
];

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[track_caller]
fn assert_c_family_slots(lang: LANG) {
    let function = "int f(int a, int b, int y, int *p) { PRED return 0; }";
    assert_rows(
        lang,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        C_FAMILY_IF_ROWS,
    );
    let other_slots = &[("b", 1, 1), ("-b", 1, 1), ("*p", 1, 1), ("b > 1", 1, 1)];
    for slot in [
        "while (PRED) { g(); }",
        "do { g(); } while (PRED);",
        "for (; PRED; ) { g(); }",
    ] {
        assert_rows(lang, &function.replace(SLOT, slot), other_slots);
    }
    // The `?` is a condition of its own (Rule 5) beside its slot.
    assert_rows(
        lang,
        &function.replace(SLOT, "y = PRED ? 1 : 2;"),
        &[("b", 2, 1), ("-b", 2, 1), ("*p", 2, 1), ("b > 1", 2, 1)],
    );
    // Outside a slot only a negation is a condition, whatever it
    // negates.
    assert_rows(
        lang,
        &function.replace(SLOT, "g(PRED);"),
        &[
            ("b", 0, 0),
            ("-b", 0, 0),
            ("!b", 1, 0),
            ("!*p", 1, 0),
            ("!\"s\"", 1, 0),
            ("!(b > 1)", 1, 0),
        ],
    );
}

#[test]
#[cfg(feature = "c")]
fn c_slots_pay_for_any_predicate() {
    assert_c_family_slots(LANG::C);
}

/// The C++ rows beyond C: `this`, `nullptr`, a lambda, the `not`
/// alternative token, and the `condition_clause` forms — an
/// init-statement and a condition declaration, which pays as its value
/// does.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
#[track_caller]
fn assert_cpp_slots(lang: LANG) {
    assert_c_family_slots(lang);
    assert_rows(
        lang,
        "struct S { bool f(int a, int b) { if (PRED) { g(); } return 0; } };",
        &[
            ("this", 1, 1),
            ("nullptr", 1, 1),
            ("[] { return 1; }", 1, 1),
            ("not b", 1, 1),
            ("not -b", 1, 1),
            ("a and -b", 2, 2),
            ("auto q = g()", 1, 1),
            ("bool y = b > 1", 1, 1),
            // A braced condition declaration holds its value in a
            // one-element list, which the peel opens like the `=` form.
            ("bool y{b > 1}", 1, 1),
            ("bool y{b}", 1, 1),
            ("int x = g(); x", 1, 1),
            ("int x = g(); -x", 1, 1),
            ("int x = g(); x > 1", 1, 1),
        ],
    );
    // A fold over a comparison is counted by its operator, as
    // `cpp_operator_is_applied` decides; the slot must not pay again.
    // A fold over `&&` / `||` applies its operator once, so it scores
    // one application, each row level with the spelled twin below it —
    // a unary fold's unwritten side is one more operand (#1533).
    // `(... && a)` scored 1 in a slot and 0 in a `return`.
    //
    // `(predicate, conditions, decisions)` in a `return`; the `if` adds
    // one decision and no condition.
    let folds = [
        ("(... == a)", 1, 0),
        ("(a < ... < 0)", 1, 0),
        ("(... && a)", 2, 1),
        ("a && a", 2, 1),
        ("(a || ...)", 2, 1),
        ("a || a", 2, 1),
        ("(true && ... && a)", 2, 1),
        ("true && a", 2, 1),
        ("(... && !a)", 2, 1),
        ("!a && !a", 2, 1),
        ("(0 || ... || !a)", 2, 1),
        ("0 || !a", 2, 1),
        ("(... && (a > 0))", 2, 1),
        ("(a > 0) && (a > 0)", 2, 1),
    ];
    assert_rows(
        lang,
        "template <class... T> bool f(T... a) { return PRED; }",
        &folds,
    );
    let in_slot: Vec<_> = folds.iter().map(|&(p, c, d)| (p, c, d + 1)).collect();
    assert_rows(
        lang,
        "template <class... T> bool f(T... a) { if (PRED) { g(); } return 0; }",
        &in_slot,
    );
    // A requires clause pays no condition however it is written:
    // unparenthesised it is a `constraint_conjunction`, parenthesised an
    // ordinary expression, and a requires-expression's requirements are
    // never evaluated (#1533). Cyclomatic agrees in the second column.
    let constraints = &[
        ("A<T> && B<T>", 0, 0),
        ("(A<T> && B<T>)", 0, 0),
        ("(A<T> || (B<T> && C<T>))", 0, 0),
        ("(!(A<T> && B<T>))", 0, 0),
        ("(!A<T>)", 0, 0),
        ("(... && A<T>)", 0, 0),
        ("(sizeof(T) > 4)", 0, 0),
        ("(X ? A<T> : B<T>)", 0, 0),
        ("requires(T b) { b && b; }", 0, 0),
    ];
    assert_rows(
        lang,
        "template <class T> void f(T a) requires PRED {}",
        constraints,
    );
    // The clause's reach ends with it: the body that follows still
    // scores its `a && a`, two conditions and one decision.
    let with_body: Vec<_> = constraints.iter().map(|&(p, _, _)| (p, 2, 1)).collect();
    assert_rows(
        lang,
        "template <class T> bool f(T a) requires PRED { return a && a; }",
        &with_body,
    );
}

#[test]
#[cfg(feature = "cpp")]
fn cpp_slots_pay_for_any_predicate() {
    assert_cpp_slots(LANG::Cpp);
}

/// Mozcpp owns no file extension, so no corpus snapshot reaches it.
#[test]
#[cfg(feature = "mozcpp")]
fn mozcpp_slots_pay_for_any_predicate() {
    assert_cpp_slots(LANG::Mozcpp);
}

#[test]
#[cfg(feature = "objc")]
fn objc_slots_pay_for_any_predicate() {
    assert_c_family_slots(LANG::Objc);
    assert_rows(
        LANG::Objc,
        "@implementation A\n- (int)f:(int)b { if (PRED) { g(); } return 0; }\n@end\n",
        &[
            ("self", 1, 1),
            ("[self ok]", 1, 1),
            ("@available(iOS 13.0, *)", 1, 1),
            ("-b", 1, 1),
            ("b > 1", 1, 1),
        ],
    );
}

/// A Rust slot only admits a `bool`, so every row here is a valid
/// predicate the terminal-kind list left at zero, beside the twins that
/// already scored.
#[test]
#[cfg(feature = "rust")]
fn rust_slots_pay_for_any_predicate() {
    let rows = &[
        ("b", 1, 1),
        ("*p", 1, 1),
        ("!*p", 1, 1),
        ("a & b", 1, 1),
        ("a ^ b", 1, 1),
        ("{ a }", 1, 1),
        ("unsafe { a }", 1, 1),
        // A block evaluates to its last expression: the comparison
        // inside pays, the slot does not pay again.
        ("{ x > 1 }", 1, 1),
        // The match arm is a decision of its own; the slot tests the
        // value the match yields.
        ("match x { 1 => true, _ => false }", 2, 2),
        ("x > 1", 1, 1),
        ("!(x > 1)", 1, 1),
        ("let Some(z) = o", 1, 1),
        ("a && b", 2, 2),
        ("a && *p", 2, 2),
        ("a && b || *p", 3, 3),
        ("a && let Some(z) = o", 2, 2),
        ("let Some(z) = o && *p", 2, 2),
        // A let-chain is flat: its operands are walked once, not once per
        // `&&` (it scored 5 and 4 here).
        ("let Some(z) = o && b && *p", 3, 3),
        ("a && let Some(z) = o && *p", 3, 3),
    ];
    let function = "fn f(a: bool, b: bool, p: &bool, x: i32, o: Option<i32>) { PRED }";
    assert_rows(
        LANG::Rust,
        &function.replace(SLOT, "if PRED { g(); }"),
        rows,
    );
    assert_rows(
        LANG::Rust,
        &function.replace(SLOT, "while PRED { g(); }"),
        &[("b", 1, 1), ("*p", 1, 1), ("a & b", 1, 1)],
    );
    // A guard is a slot beside its arm's own condition.
    assert_rows(
        LANG::Rust,
        &function.replace(SLOT, "match x { 1 if PRED => g(), _ => g() }"),
        &[("b", 2, 2), ("*p", 2, 2), ("a & b", 2, 2), ("x > 1", 2, 2)],
    );
    // Outside a slot only a negation is a condition.
    assert_rows(
        LANG::Rust,
        &function.replace(SLOT, "g(PRED);"),
        &[("*p", 0, 0), ("!*p", 1, 0), ("!(a & b)", 1, 0)],
    );
}

/// A Java slot only admits a `boolean`; every row is a valid predicate.
/// The non-short-circuit `&` / `|` / `^` and a boolean `switch` were
/// never terminal kinds, and an assignment was not peeled.
#[test]
#[cfg(feature = "java")]
fn java_slots_pay_for_any_predicate() {
    let function = "class C { boolean y; boolean f(boolean a, boolean b, Object o, int x) \
                    { PRED return a; } }";
    assert_rows(
        LANG::Java,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        &[
            ("b", 1, 1),
            ("y = b", 1, 1),
            ("y &= b", 1, 1),
            ("a & b", 1, 1),
            ("a ^ b", 1, 1),
            ("a | b", 1, 1),
            ("!(a & b)", 1, 1),
            ("switch (x) { case 1 -> true; default -> false; }", 2, 2),
            // The assignment arm already scores a negated value, and the
            // comparison arm a compared one: the slot must not pay again.
            ("y = !b", 1, 1),
            ("y = x > 1", 1, 1),
            // A cast scored 2 while `cast_expression` was a terminal kind.
            ("(boolean) (x > 1)", 1, 1),
            ("(Boolean) o", 1, 1),
            ("o instanceof String s", 1, 1),
            ("x > 1", 1, 1),
            ("!b", 1, 1),
            ("a && b", 2, 2),
            ("a && (y = b)", 2, 2),
            ("a && (a & b)", 2, 2),
            ("a && (y = !b)", 2, 2),
        ],
    );
    for slot in [
        "while (PRED) { g(); }",
        "do { g(); } while (PRED);",
        "for (; PRED; ) { g(); }",
    ] {
        assert_rows(
            LANG::Java,
            &function.replace(SLOT, slot),
            &[("b", 1, 1), ("a & b", 1, 1), ("x > 1", 1, 1)],
        );
    }
    // The `?` is a condition of its own (Rule 5) beside its slot.
    assert_rows(
        LANG::Java,
        &function.replace(SLOT, "y = PRED ? true : false;"),
        &[("b", 2, 1), ("a & b", 2, 1), ("x > 1", 2, 1)],
    );
    // A guard is a slot beside its `case` arm's own condition.
    assert_rows(
        LANG::Java,
        "class C { void f(Object o, boolean a, boolean b) \
         { switch (o) { case Integer i when PRED -> g(); default -> g(); } } }",
        &[
            ("b", 2, 2),
            ("a & b", 2, 2),
            ("/*c*/ a & b", 2, 2),
            ("a == b", 2, 2),
        ],
    );
    // Outside a slot only a negation is a condition.
    assert_rows(
        LANG::Java,
        &function.replace(SLOT, "g(PRED);"),
        &[("a & b", 0, 0), ("!(a & b)", 1, 0)],
    );
}

/// Kotlin joined the shared slot in #1533. A `when` whose only entry is
/// `else ->` charges nothing of its own, so the slot holding it pays: it
/// scored 0 against the slot's decision. Its twin is `true`, the value
/// such a `when` evaluates to. A `when` with other entries pays per entry
/// and the slot pays as well, as the slot holding Java's `switch`, C#'s
/// `switch` expression or Rust's `match` does (each scores 2 for the
/// one-case form). An `if` expression (its `else`, like a ternary's `?`),
/// a `try` (its `try` / `catch`) and an elvis (its `?:`) pay through
/// their own arms, and the slot does not pay again; the infix `a and b`
/// is an eager call, a value like `f()`.
#[test]
#[cfg(feature = "kotlin")]
fn kotlin_slots_pay_for_any_predicate() {
    let function = "fun f(a: Boolean, b: Boolean, x: Int, nb: Boolean?, o: Any) { PRED }";
    let rows = &[
        ("b", 1, 1),
        ("true", 1, 1),
        ("when (x) { else -> true }", 1, 1),
        ("(when (x) { else -> true })", 1, 1),
        ("!when (x) { else -> true }", 1, 1),
        ("when (x) { 1 -> true else -> false }", 2, 2),
        ("when { a -> true else -> false }", 2, 2),
        ("if (a) b else false", 2, 2),
        ("try { b } catch (e: Exception) { false }", 2, 2),
        ("nb ?: false", 1, 2),
        ("x > 1", 1, 1),
        ("(x > 1)!!", 1, 1),
        ("o is String", 1, 1),
        ("o as? Boolean ?: false", 2, 2),
        ("a && b", 2, 2),
        ("a and b", 1, 1),
        ("-x > 0", 1, 1),
    ];
    for slot in [
        "if (PRED) { g() }",
        "while (PRED) { g() }",
        "do { g() } while (PRED)",
        "when { PRED -> g() }",
    ] {
        assert_rows(LANG::Kotlin, &function.replace(SLOT, slot), rows);
    }
    // A chain operand is a slot too, beside the `&&`'s own decision and
    // the `a` operand's condition.
    let in_chain: Vec<_> = rows.iter().map(|&(p, c, d)| (p, c + 1, d + 1)).collect();
    assert_rows(
        LANG::Kotlin,
        &function.replace(SLOT, "if (a && (PRED)) { g() }"),
        &in_chain,
    );
}

/// A C# slot only admits a `bool`; every row is a valid predicate. As in
/// Java, the non-short-circuit operators and a boolean `switch` were
/// never terminal kinds, and an assignment was not peeled.
#[test]
#[cfg(feature = "csharp")]
fn csharp_slots_pay_for_any_predicate() {
    let function = "class C { bool y; bool f(bool a, bool b, object o, int x, bool? n) \
                    { PRED return a; } }";
    assert_rows(
        LANG::Csharp,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        &[
            ("b", 1, 1),
            ("y = b", 1, 1),
            ("y &= b", 1, 1),
            ("a & b", 1, 1),
            ("a ^ b", 1, 1),
            ("!(a & b)", 1, 1),
            ("x switch { 1 => true, _ => false }", 2, 2),
            // The assignment arm already scores a negated value, and the
            // comparison arm a compared one: the slot must not pay again.
            ("y = !b", 1, 1),
            ("y = x > 1", 1, 1),
            // A cast scored 2 while `cast_expression` was a terminal kind.
            ("(bool)(x > 1)", 1, 1),
            ("(bool)o", 1, 1),
            ("b!", 1, 1),
            ("o is string s", 1, 1),
            ("x > 1", 1, 1),
            ("!b", 1, 1),
            ("a && b", 2, 2),
            ("a && (y = b)", 2, 2),
            ("a && (a & b)", 2, 2),
        ],
    );
    for slot in [
        "while (PRED) { g(); }",
        "do { g(); } while (PRED);",
        "for (; PRED; ) { g(); }",
    ] {
        assert_rows(
            LANG::Csharp,
            &function.replace(SLOT, slot),
            &[("b", 1, 1), ("a & b", 1, 1), ("x > 1", 1, 1)],
        );
    }
    // The `?` is a condition of its own (Rule 5) beside its slot.
    assert_rows(
        LANG::Csharp,
        &function.replace(SLOT, "y = PRED ? true : false;"),
        &[("b", 2, 1), ("a & b", 2, 1), ("x > 1", 2, 1)],
    );
    // A guard is a slot beside its arm's own condition; a comment before
    // it pays nothing.
    assert_rows(
        LANG::Csharp,
        "class C { int f(object o, bool a, bool b) \
         { return o switch { int i when PRED => 1, _ => 0 }; } }",
        &[
            ("b", 2, 2),
            ("a & b", 2, 2),
            ("/*c*/ a & b", 2, 2),
            ("a == b", 2, 2),
        ],
    );
    // Outside a slot only a negation is a condition.
    assert_rows(
        LANG::Csharp,
        &function.replace(SLOT, "y = PRED;"),
        &[("a & b", 0, 0), ("!(a & b)", 1, 0)],
    );
}

/// The C# spellings #1475 measured at zero before #1526 made every slot
/// pay: `default(bool)`, the `checked` wrapper (and its `unchecked`
/// twin), an assignment whose value is a call, and an object creation
/// converted to `bool` by a user-defined operator.
#[test]
#[cfg(feature = "csharp")]
fn csharp_1475_constructs_pay_their_slot() {
    let function = "class C { bool y; bool f(bool a, bool b) { PRED return a; } }";
    assert_level_with_control(
        LANG::Csharp,
        [
            &function.replace(SLOT, "if (PRED) { g(); }"),
            &function.replace(SLOT, "if (a && (PRED)) { g(); }"),
        ],
        "b",
        &[
            "default(bool)",
            "checked(b)",
            "unchecked(b)",
            "y = C()",
            "new Wrapper()",
        ],
    );
}

/// PHP is truthy-valued, so every predicate is a decision. `??` is no
/// condition token in PHP, so a slot holding one pays for it.
#[test]
#[cfg(feature = "php")]
fn php_slots_pay_for_any_predicate() {
    let function = "<?php\nfunction f($a, $b, $x, $y) { PRED return 0; }\n";
    assert_rows(
        LANG::Php,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        &[
            ("$b", 1, 1),
            ("-$x", 1, 1),
            ("!-$x", 1, 1),
            ("$x + 1", 1, 1),
            ("$a & $b", 1, 1),
            ("$y = $x", 1, 1),
            ("$y += 1", 1, 1),
            ("A::B", 1, 1),
            ("$a ?? $b", 1, 2),
            ("fn() => 1", 1, 1),
            ("@$a", 1, 1),
            ("print $a", 1, 1),
            // Wrappers that evaluate to a comparison must not pay twice.
            // The cast scored 2 while `cast_expression` was a terminal
            // kind.
            ("$y = $x > 1", 1, 1),
            ("(int)($x > 1)", 1, 1),
            ("@($x > 1)", 1, 1),
            ("!($x > 1)", 1, 1),
            ("$x > 1", 1, 1),
            ("!$b", 1, 1),
            ("$a && $b", 2, 2),
            ("$a && -$x", 2, 2),
            ("$a and -$x", 2, 2),
            ("$a xor -$x", 2, 2),
            ("$a || ($y = $x > 1)", 2, 2),
            // A ternary scores itself through its own arm.
            ("$a ? $b : $x", 2, 2),
        ],
    );
    for slot in [
        "while (PRED) { g(); }",
        "do { g(); } while (PRED);",
        "for (; PRED; ) { g(); }",
    ] {
        assert_rows(
            LANG::Php,
            &function.replace(SLOT, slot),
            &[("$b", 1, 1), ("-$x", 1, 1), ("$x > 1", 1, 1)],
        );
    }
    // The ternary node is a condition of its own beside its slot.
    assert_rows(
        LANG::Php,
        &function.replace(SLOT, "$y = PRED ? 1 : 2;"),
        &[("$b", 2, 1), ("-$x", 2, 1), ("$x > 1", 2, 1)],
    );
    // Outside a slot only a negation is a condition, whatever it
    // negates — positional and named arguments alike.
    for call in ["g(PRED);", "g(name: PRED);"] {
        assert_rows(
            LANG::Php,
            &function.replace(SLOT, call),
            &[("-$x", 0, 0), ("!-$x", 1, 0), ("!$b", 1, 0)],
        );
    }
}

/// `elseif` is PHP's `else if`, and scores what the two-word spelling
/// does: the `else` (Rule 5, no cyclomatic decision of its own) plus a
/// predicate slot. It paid only the first, so `elseif ($b)` scored one
/// below `else if ($b)` while `elseif ($x > 1)` matched it.
#[test]
#[cfg(feature = "php")]
fn php_elseif_is_an_else_and_a_slot() {
    let rows = &[("$b", 3, 2), ("-$x", 3, 2), ("$x > 1", 3, 2)];
    for chain in [
        "if ($a) { g(); } elseif (PRED) { g(); }",
        "if ($a): g(); elseif (PRED): g(); endif;",
        "if ($a) { g(); } else if (PRED) { g(); }",
    ] {
        assert_rows(
            LANG::Php,
            &format!("<?php\nfunction f($a, $b, $x) {{ {chain} return 0; }}\n"),
            rows,
        );
    }
}

/// Lua is truthy-valued, so every predicate is a decision.
#[test]
#[cfg(feature = "lua")]
fn lua_slots_pay_for_any_predicate() {
    let function = "function f(a, b, x, t) PRED end\n";
    let rows = &[
        ("b", 1, 1),
        ("-x", 1, 1),
        ("#t", 1, 1),
        ("x + 1", 1, 1),
        ("not -x", 1, 1),
        ("x > 1", 1, 1),
        ("not (x > 1)", 1, 1),
        ("a and b", 2, 2),
        ("a and -x", 2, 2),
        ("#t or a", 2, 2),
    ];
    for slot in [
        "if PRED then g() end",
        "while PRED do g() end",
        "repeat g() until PRED",
    ] {
        assert_rows(LANG::Lua, &function.replace(SLOT, slot), rows);
    }
    // Outside a slot only a negation is a condition.
    for value in ["g(PRED)", "return PRED"] {
        assert_rows(
            LANG::Lua,
            &function.replace(SLOT, value),
            &[("-x", 0, 0), ("not -x", 1, 0), ("not b", 1, 0)],
        );
    }
}

/// An `elseif` is the `else` (Rule 5, no cyclomatic decision of its own)
/// plus a predicate slot. It paid only the first, so `elseif b` scored
/// one below `elseif x > 1`.
#[test]
#[cfg(feature = "lua")]
fn lua_elseif_is_an_else_and_a_slot() {
    assert_rows(
        LANG::Lua,
        "function f(a, b, x) if a then g() elseif PRED then g() end end\n",
        &[("b", 3, 2), ("-x", 3, 2), ("x > 1", 3, 2), ("not b", 3, 2)],
    );
}

/// Groovy truth makes every value testable, so every predicate is a
/// decision. Every relational form scores itself through its own arm,
/// and an assignment through its value, as in Java.
#[test]
#[cfg(feature = "groovy")]
fn groovy_slots_pay_for_any_predicate() {
    let function = "class C { def y\n def f(a, b, x, o, l) { PRED; return a } }\n";
    assert_rows(
        LANG::Groovy,
        &function.replace(SLOT, "if (PRED) { g() }"),
        &[
            ("b", 1, 1),
            ("-x", 1, 1),
            ("!-x", 1, 1),
            ("x + 1", 1, 1),
            ("(y = b)", 1, 1),
            ("a ==> b", 1, 1),
            ("{ -> 1 }", 1, 1),
            ("y = !b", 1, 1),
            ("y = x > 1", 1, 1),
            // A cast scored 2 while `cast_expression` was a terminal kind.
            ("(boolean) (x > 1)", 1, 1),
            ("x > 1", 1, 1),
            ("a === b", 1, 1),
            ("x =~ /r/", 1, 1),
            ("x <=> 1", 1, 1),
            ("x in l", 1, 1),
            ("o instanceof String", 1, 1),
            ("a && b", 2, 2),
            ("a && -x", 2, 2),
            ("-x || a", 2, 2),
            ("a && (y = !b)", 2, 2),
        ],
    );
    for slot in [
        "while (PRED) { g() }",
        "do { g() } while (PRED)",
        "for (; PRED; ) { g() }",
    ] {
        assert_rows(
            LANG::Groovy,
            &function.replace(SLOT, slot),
            &[("b", 1, 1), ("-x", 1, 1), ("x > 1", 1, 1)],
        );
    }
    // The `?` is a condition of its own (Rule 5) beside its slot.
    assert_rows(
        LANG::Groovy,
        &function.replace(SLOT, "y = PRED ? 1 : 2"),
        &[("b", 2, 1), ("-x", 2, 1), ("x > 1", 2, 1)],
    );
    // Outside a slot only a negation is a condition.
    assert_rows(
        LANG::Groovy,
        &function.replace(SLOT, "y = PRED"),
        &[("-x", 0, 0), ("!-x", 1, 0)],
    );
}

/// #1475's Groovy rows: an object creation, which JS and PHP already
/// counted, and a spread-dot navigation, whose list result is tested
/// for emptiness.
#[test]
#[cfg(feature = "groovy")]
fn groovy_1475_constructs_pay_their_slot() {
    let function = "class C { def f(a, b) { PRED; return a } }\n";
    assert_level_with_control(
        LANG::Groovy,
        [
            &function.replace(SLOT, "if (PRED) { g() }"),
            &function.replace(SLOT, "if (a && (PRED)) { g() }"),
        ],
        "b",
        &["new Foo()", "a*.b"],
    );
}

/// Perl is truthy-valued, so every predicate is a decision.
#[test]
#[cfg(feature = "perl")]
fn perl_slots_pay_for_any_predicate() {
    let function = "sub f { my ($a, $b, $x, $y) = @_; PRED }\n";
    assert_rows(
        LANG::Perl,
        &function.replace(SLOT, "if (PRED) { g(); }"),
        &[
            ("$b", 1, 1),
            ("-$x", 1, 1),
            ("!-$x", 1, 1),
            ("not -$x", 1, 1),
            ("$x + 1", 1, 1),
            ("my $y = $x", 1, 1),
            ("$y += 1", 1, 1),
            // Wrappers that evaluate to a comparison must not pay twice.
            ("my $y = $x > 1", 1, 1),
            ("$a, $x > 1", 1, 1),
            ("$x > 1", 1, 1),
            ("/re/", 1, 1),
            ("!/re/", 1, 1),
            ("$x =~ /re/", 1, 1),
            ("$a && $b", 2, 2),
            ("$a && -$x", 2, 2),
            ("$a or -$x", 2, 2),
            // tree-sitter-perl parses the low-precedence `and` as a
            // two-operand `unary_expression`; it scored 0.
            ("$a and $b", 2, 2),
            ("$a and -$x", 2, 2),
        ],
    );
    for slot in [
        "unless (PRED) { g(); }",
        "while (PRED) { g(); }",
        "until (PRED) { g(); }",
        "for (my $i = 0; PRED; $i++) { g(); }",
        "g() if PRED;",
        "g() if (PRED);",
        "g() unless PRED;",
        "g() while PRED;",
    ] {
        assert_rows(
            LANG::Perl,
            &function.replace(SLOT, slot),
            &[("$b", 1, 1), ("-$x", 1, 1), ("$x > 1", 1, 1)],
        );
    }
    // `EXPR if ();` is the one spelling that puts a
    // `parenthesized_argument` in the modifier slot: empty, so always
    // false, and still a decision.
    assert_rows(
        LANG::Perl,
        &function.replace(SLOT, "g() if (PRED);"),
        &[("", 1, 1)],
    );
    // The ternary node is a condition of its own beside its slot.
    assert_rows(
        LANG::Perl,
        &function.replace(SLOT, "my $r = PRED ? 1 : 2;"),
        &[("$b", 2, 1), ("-$x", 2, 1)],
    );
    // Outside a slot only a negation is a condition.
    for value in ["g(PRED);", "return PRED;"] {
        assert_rows(
            LANG::Perl,
            &function.replace(SLOT, value),
            &[("-$x", 0, 0), ("!-$x", 1, 0), ("!$b", 1, 0)],
        );
    }
}

/// An `elsif` is the `else` (Rule 5, no cyclomatic decision of its own)
/// plus a predicate slot. It paid only the first, so `elsif ($b)` scored
/// one below `elsif ($x > 1)`.
#[test]
#[cfg(feature = "perl")]
fn perl_elsif_is_an_else_and_a_slot() {
    assert_rows(
        LANG::Perl,
        "sub f { my ($a, $b, $x) = @_; if ($a) { g(); } elsif (PRED) { g(); } }\n",
        &[("$b", 3, 2), ("-$x", 3, 2), ("$x > 1", 3, 2)],
    );
}

/// #1475's Perl rows: the substitution and transliteration operators
/// (all three spellings), an anonymous sub, and the array and hash
/// variables in both their named and dereferencing forms. `@$b` was
/// the case #1475 called the clearest defect, since `@a` scored and it
/// did not. Each pays a slot, the `=~` / `!~`-bound rewrites included:
/// outside one they score nothing (#1540), so the slot is all they pay.
#[test]
#[cfg(feature = "perl")]
fn perl_1475_constructs_pay_their_slot() {
    let function = "sub f { my ($a, $b, $h) = @_; my @a; my %h; PRED }\n";
    assert_level_with_control(
        LANG::Perl,
        [
            &function.replace(SLOT, "if (PRED) { g(); }"),
            &function.replace(SLOT, "if ($a && (PRED)) { g(); }"),
        ],
        "$b",
        &[
            "s/x/y/",
            "tr/a/b/",
            "y/a/b/",
            "$b =~ s/x/y/",
            "$b !~ s/x/y/",
            "$b =~ tr/a/b/",
            "sub {1}",
            "@a",
            "@$b",
            "%h",
            "%$h",
        ],
    );
}

/// #1475's Ruby rows: both lambda spellings and a range.
#[test]
#[cfg(feature = "ruby")]
fn ruby_1475_constructs_pay_their_slot() {
    assert_level_with_control(
        LANG::Ruby,
        [
            "def f(a, b)\n  if PRED\n    g\n  end\nend\n",
            "def f(a, b)\n  if a && (PRED)\n    g\n  end\nend\n",
        ],
        "b",
        &["lambda {}", "-> {}", "(1..2)"],
    );
}

/// #1475's Elixir rows: an anonymous function and a function capture.
#[test]
#[cfg(feature = "elixir")]
fn elixir_1475_constructs_pay_their_slot() {
    let function = "defmodule M do\n  def f(a, b) do\n    PRED\n  end\nend\n";
    assert_level_with_control(
        LANG::Elixir,
        [
            &function.replace(SLOT, "if PRED do\n      g()\n    end"),
            &function.replace(SLOT, "if a && (PRED) do\n      g()\n    end"),
        ],
        "b",
        &["fn -> 1 end", "&f/1"],
    );
}

/// Tcl's `expr` tests any number for non-zero, so every predicate is a
/// decision. iRules is its dialect and shares every row.
#[cfg(any(feature = "tcl", feature = "irules"))]
#[track_caller]
fn assert_tcl_family_slots(lang: LANG) {
    let function = "proc f {a b x} { PRED }\n";
    let rows = &[
        ("$b", 1, 1),
        ("-$x", 1, 1),
        ("!-$x", 1, 1),
        ("$x + 1", 1, 1),
        ("$x > 1", 1, 1),
        ("($x > 1)", 1, 1),
        // A ternary scores itself through its own arm.
        ("$a ? $b : $x", 2, 2),
        ("$a && $b", 2, 2),
        ("$a && -$x", 2, 2),
    ];
    for slot in ["if {PRED} { g }", "while {PRED} { g }"] {
        assert_rows(lang, &function.replace(SLOT, slot), rows);
    }
    // An `elseif` is the `else` plus a predicate slot.
    assert_rows(
        lang,
        &function.replace(SLOT, "if {$a} { g } elseif {PRED} { g }"),
        &[("$b", 3, 2), ("-$x", 3, 2), ("$x > 1", 3, 2)],
    );
    // The ternary is a condition of its own beside its slot; a branch
    // counts only behind a negation.
    assert_rows(
        lang,
        &function.replace(SLOT, "set r [expr {PRED ? 1 : 2}]"),
        &[("$b", 2, 1), ("-$x", 2, 1)],
    );
    assert_rows(
        lang,
        &function.replace(SLOT, "set r [expr {$a ? PRED : 2}]"),
        &[("-$x", 2, 1), ("!-$x", 3, 1)],
    );
}

#[test]
#[cfg(feature = "tcl")]
fn tcl_slots_pay_for_any_predicate() {
    assert_tcl_family_slots(LANG::Tcl);
}

#[test]
#[cfg(feature = "irules")]
fn irules_slots_pay_for_any_predicate() {
    assert_tcl_family_slots(LANG::Irules);
    // The word-form operators iRules adds: a comparison scores itself,
    // and a keyword chain pays a slot per operand.
    assert_rows(
        LANG::Irules,
        "proc f {a b x} { if {PRED} { g } }\n",
        &[
            ("$x contains \"a\"", 1, 1),
            ("$a and -$x", 2, 2),
            // `not` is `!` spelled as a keyword: the comparison it negates
            // pays, the slot does not pay again.
            ("not ($x == 1)", 1, 1),
            ("not ($x contains \"a\")", 1, 1),
            ("not -$x", 1, 1),
        ],
    );
}

/// A bare `return;` holds no value, so the negated-operand path that
/// reads a `return`'s value has nothing to score. Each template pairs one
/// with a valued `return` / declarator, so a bare `return` scoring
/// anything shows as one over the row's twin.
#[test]
#[cfg(any(feature = "csharp", feature = "php", feature = "perl"))]
fn bare_return_scores_nothing() {
    let mut ran = 0;
    #[cfg(feature = "csharp")]
    {
        assert_rows(
            LANG::Csharp,
            "class A { void f(bool b) { if (b) { return; } var r = PRED; } }\n",
            &[("!b", 2, 1), ("b", 1, 1)],
        );
        ran += 1;
    }
    #[cfg(feature = "php")]
    {
        assert_rows(
            LANG::Php,
            "<?php\nfunction f($b) {\n    if ($b) { return; }\n    return PRED;\n}\n",
            &[("!$b", 2, 1), ("$b", 1, 1)],
        );
        ran += 1;
    }
    #[cfg(feature = "perl")]
    {
        assert_rows(
            LANG::Perl,
            "sub f { my ($b) = @_; if ($b) { return; } return PRED; }\n",
            &[("!$b", 2, 1), ("$b", 1, 1)],
        );
        ran += 1;
    }
    assert!(ran > 0, "no language enabled; this test asserted nothing");
}
