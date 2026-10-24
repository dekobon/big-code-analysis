#![allow(clippy::float_cmp)]

//! Metric parity between `LANG::Cpp` (upstream `tree-sitter-cpp`) and
//! `LANG::Mozcpp` (the vendored Mozilla fork) on **non-Gecko C++**.
//!
//! `Mozcpp` is upstream `tree-sitter-cpp` plus a Gecko macro overlay
//! (#720). On ordinary C++ — without `MOZ_*` / `QM_TRY_*` / alone-macro
//! constructs — the two grammars produce equivalent parse trees, so the
//! `CppCode` and `MozcppCode` metric impls (which are deliberate clones
//! of one another) must agree exactly.
//!
//! This guard matters because **`Mozcpp` owns no file extensions**, so no
//! integration-snapshot corpus exercises it — a divergence in the cloned
//! `MozcppCode` impls would otherwise ship silently. (A real instance:
//! while adding `LANG::C` in #721, an over-wide bulk edit stripped
//! `AssignmentExpression2` / `NewExpression` / `<=>` / `try` / `catch`
//! from the `MozcppCode` ABC arms; `make pre-commit` stayed green because
//! nothing covered Mozcpp. This test would have caught it.)
//!
//! The fixture deliberately exercises the constructs that regression
//! touched: `new` allocation, a compound assignment, the `<=>` spaceship,
//! and a `try` / `catch` pair, plus ordinary branches and returns.
//!
//! Both grammars must be compiled in for any of this to mean
//! anything, so every item below is gated on the pair. Without the
//! gate a reduced-feature build did not skip these tests, it panicked
//! inside `analyze`'s `LanguageDisabled` error (#1281).

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
use big_code_analysis::LANG;

use super::support::metrics_space;
#[cfg(all(feature = "cpp", feature = "mozcpp"))]
use super::support::ops_space;

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
/// Headline integer metric sums for one parse of `source` as `lang`.
fn metric_sums(lang: LANG, source: &str, ext: &str) -> Vec<(&'static str, u64)> {
    let name = format!("parity.{ext}");
    let space = metrics_space(lang, source, &name);
    let m = &space.metrics;
    vec![
        ("cyclomatic", m.cyclomatic.cyclomatic_sum()),
        ("cognitive", m.cognitive.cognitive_sum()),
        ("nexits", m.nexits.nexits_sum()),
        ("abc.assignments", m.abc.assignments_sum()),
        ("abc.branches", m.abc.branches_sum()),
        ("abc.conditions", m.abc.conditions_sum()),
        ("nom.functions", m.nom.functions_sum()),
        ("halstead.operators", m.halstead.total_operators()),
        ("halstead.operands", m.halstead.total_operands()),
    ]
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
/// The value `metric_sums` recorded for `key`, panicking with the whole
/// row set when the key is absent.
///
/// A missing key means `metric_sums` stopped reporting that metric, and
/// the three call sites below each name several — swallowing it into a
/// `0` would turn a dropped metric into a plausible assertion failure
/// about a number the run never produced.
#[track_caller]
fn metric(sums: &[(&'static str, u64)], key: &str) -> u64 {
    sums.iter().find(|(k, _)| *k == key).map_or_else(
        || panic!("metric_sums omitted {key}: {sums:?}"),
        |(_, v)| *v,
    )
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_plain_cpp() {
    // Plain C++: `new` / compound-assign / `<=>` / `try`-`catch` are all
    // base-grammar constructs both `tree-sitter-cpp` and the mozcpp fork
    // parse identically (no Gecko overlay rules fire here).
    let source = r"
        int f(int a, int b) {
            int* p = new int(a);
            a += b;
            bool less = (a <=> b) < 0;
            try {
                if (a < b) {
                    return a;
                }
            } catch (...) {
                return -1;
            }
            delete p;
            return less ? a : b;
        }
    ";

    let cpp = metric_sums(LANG::Cpp, source, "cpp");
    let mozcpp = metric_sums(LANG::Mozcpp, source, "cpp");
    assert_eq!(
        cpp, mozcpp,
        "Cpp and Mozcpp must compute identical metrics on non-Gecko C++"
    );
    // Sanity: the fixture is non-trivial, so the run is meaningful (a
    // degenerate all-zero parse would make the equality vacuous). Guard
    // all three ABC dimensions — in particular `conditions`, which is
    // exactly where the docstring's load-bearing `<=>` / `try` / `catch`
    // constructs accumulate.
    let get = |key: &str| metric(&cpp, key);
    // conditions: `<=>` +1, the `< 0` on its result +1, the `if (a < b)`
    // +1, `try` +1, `catch` +1, the `less ? a : b` ternary +1, and that
    // ternary's bare-identifier condition operand +1 (#1102) = 7.
    assert_eq!(
        get("abc.conditions"),
        7,
        "fixture exercises <=>/try/catch conditions: {cpp:?}"
    );
    assert!(
        get("abc.assignments") >= 1 && get("abc.branches") >= 1,
        "fixture should exercise assignments and branches: {cpp:?}"
    );
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_raw_string_delimiters() {
    // #1314 guards `LPAREN` under a `RawStringLiteral` parent in both
    // `CppCode::get_op_type` and its `MozcppCode` clone. Mozcpp owns no
    // file extension, so nothing else exercises its copy — this is the
    // whole coverage the clone has.
    //
    // The fixture pairs two raw strings with a real call, so the
    // assertion below separates "the delimiter stopped counting" from
    // "every `(` stopped counting": a guard widened past the literal
    // would drop `f(a)`'s parenthesis too and take N1 to 7.
    let source = "auto a = R\"(raw)\";\nauto b = R\"tag(raw)tag\";\nint c = f(a);\n";

    let cpp = metric_sums(LANG::Cpp, source, "cpp");
    let mozcpp = metric_sums(LANG::Mozcpp, source, "cpp");
    assert_eq!(
        cpp, mozcpp,
        "Cpp and Mozcpp must agree on raw-string delimiter classification"
    );

    let get = |key: &str| metric(&cpp, key);
    // Operators: `;` x 3, `=` x 3, `int`, and the one `()` from `f(a)`
    // -> N1 = 8. Before the guard the two raw-string openers added two
    // more `()` -> N1 = 10.
    assert_eq!(
        get("halstead.operators"),
        8,
        "raw-string openers must not count, and the real call must: {cpp:?}"
    );
    // Operands: the two raw-string literals (distinct text), `a` twice,
    // `b`, `c`, `f` -> N2 = 7.
    assert_eq!(
        get("halstead.operands"),
        7,
        "both raw-string literals must still count as operands: {cpp:?}"
    );
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_this() {
    // #1361 added `This` to the operand arm of both `CppCode::get_op_type`
    // and its `MozcppCode` clone. Mozcpp owns no file extension, so
    // nothing else at the metric level exercises its copy.
    //
    // The fixture pairs `this->x` with `p->x`, which are the same
    // `<receiver> -> <field>` shape. That pairing is what separates "the
    // receiver stopped counting" from "every operand stopped counting":
    // an arm that dropped `this` again takes N2 to 9 while N1 stays at
    // 17, and only the operand row moves.
    let source = r"
        struct S {
            int x;
            int m1() { return this->x; }
            int m2(S* p) { return p->x; }
        };
    ";

    let cpp = metric_sums(LANG::Cpp, source, "cpp");
    let mozcpp = metric_sums(LANG::Mozcpp, source, "cpp");
    assert_eq!(
        cpp, mozcpp,
        "Cpp and Mozcpp must agree on `this` classification"
    );

    let get = |key: &str| metric(&cpp, key);
    // Operators: `struct` (#1552), `{` x3, `int` x3, `;` x4, `(` x2,
    // `return` x2, `->` x2, and the `*` of `S* p` -> N1 = 18. `this` must
    // not appear here — classifying it as an operator (the Java / C# /
    // Kotlin reading, #1380) would take this to 19 and the operand row
    // to 9.
    assert_eq!(
        get("halstead.operators"),
        18,
        "`this` must not be billed as an operator: {cpp:?}"
    );
    // Operands: `S` x2, `x` x3, `m1`, `m2`, `p` x2, `this` -> N2 = 10.
    // Before #1361 `this` was in neither arm, so this was 9.
    assert_eq!(
        get("halstead.operands"),
        10,
        "`this` must be billed as an operand: {cpp:?}"
    );
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_modifiers() {
    // #1578 added the declaration modifiers to the operator arm of both
    // `CppCode::get_op_type` and its `MozcppCode` clone. Mozcpp owns no
    // file extension, so nothing else at the metric level exercises its
    // copy.
    //
    // The fixture carries one of each wrapper the grammar puts a
    // modifier in — `access_specifier`, `storage_class_specifier`,
    // `virtual_specifier`, `type_qualifier` — beside a linkage
    // specification whose `"C"` must stay an operand.
    let source = "class C { public: static int a; virtual void f() const override final; \
                  mutable int b; };\n\
                  extern \"C\" inline constexpr int g() { return 1; }\n";

    let cpp = metric_sums(LANG::Cpp, source, "cpp");
    let mozcpp = metric_sums(LANG::Mozcpp, source, "cpp");
    assert_eq!(
        cpp, mozcpp,
        "Cpp and Mozcpp must agree on modifier classification"
    );

    let get = |key: &str| metric(&cpp, key);
    // Operators: `class`, `{` x2, `public`, `:`, `static`, `int` x3,
    // `;` x5, `virtual`, `void`, `(` x2, `const`, `override`, `final`,
    // `mutable`, `extern`, `inline`, `constexpr`, `return` -> N1 = 26.
    // Before #1578 the ten modifiers billed nothing and this was 16.
    assert_eq!(
        get("halstead.operators"),
        26,
        "each modifier must bill once as an operator: {cpp:?}"
    );
    // Operands: `C`, `a`, `f`, `b`, `"C"`, `g`, `1` -> N2 = 7.
    assert_eq!(
        get("halstead.operands"),
        7,
        "no modifier may bill as an operand: {cpp:?}"
    );
}

/// The three keyword rows' spellings, one fixture line's worth each,
/// written out rather than taken from a Cpp run so a kind dropped from
/// *both* dialects' rows fails by name (#1693).
#[cfg(all(feature = "cpp", feature = "mozcpp"))]
const KEYWORD_ROWS: &[(&str, &[&str])] = &[
    // Modifier: the 22 shared spellings plus the 12 C++ ones.
    (
        "Modifier",
        &[
            "static",
            "extern",
            "register",
            "inline",
            "__inline",
            "__inline__",
            "__forceinline",
            "thread_local",
            "__thread",
            "const",
            "constexpr",
            "volatile",
            "__volatile__",
            "restrict",
            "__restrict__",
            "_Atomic",
            "_Noreturn",
            "noreturn",
            "_Nonnull",
            "alignas",
            "_Alignas",
            "__extension__",
            "virtual",
            "mutable",
            "constinit",
            "consteval",
            "explicit",
            "friend",
            "public",
            "private",
            "protected",
            "override",
            "final",
            "noexcept",
        ],
    ),
    // DeclKeyword: the 4 shared spellings plus the 5 C++ ones.
    (
        "DeclKeyword",
        &[
            "struct",
            "union",
            "enum",
            "typedef",
            "class",
            "namespace",
            "template",
            "typename",
            "using",
        ],
    ),
    // Primitive: `int` and `void` by text, the sized-type words by kind.
    (
        "Primitive",
        &["int", "void", "unsigned", "long", "signed", "short"],
    ),
];

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_keyword_vocabularies() {
    // #1693 moved the modifiers, the declaration keywords and the
    // primitive types out of both getters into the `Modifier` /
    // `DeclKeyword` / `Primitive` rows of `roles/cpp.rs` and
    // `roles/mozcpp.rs`. Mozcpp's rows are a copy, and nothing routes to
    // Mozcpp, so this is the whole coverage that copy has.
    //
    // One spelling per row entry that a plain declaration can carry;
    // `__volatile__` only ever appears as a GNU `asm` qualifier, and the
    // vendor spellings each have a kind of their own.
    let source = "namespace N { template <typename T, class U> class C : public B { \
                  public: virtual void f() const noexcept override final; \
                  private: mutable int m; protected: static constexpr int k = 1; \
                  friend class F; explicit C(int); consteval int g(); \
                  constinit static int s; };\n\
                  struct S { union U { enum class E { X } e; }; }; typedef int I; \
                  using J = int; extern \"C\" int e; thread_local int t; \
                  __thread int tt; inline int i; __inline int ii; __inline__ int iii; \
                  __forceinline int iv; register int r; volatile int v; \
                  restrict int *rp; __restrict__ int *rrp; _Atomic int at; \
                  alignas(8) int al; _Alignas(8) int al2; __extension__ int ex; \
                  _Noreturn void nr(); noreturn void nr2(); int * _Nonnull nn; \
                  unsigned long long ull; signed short ss; \
                  void a(void) { __asm__ __volatile__(\"nop\"); } }\n";

    let cpp = ops_space(LANG::Cpp, source, "parity.cpp");
    let mozcpp = ops_space(LANG::Mozcpp, source, "parity.cpp");
    assert_eq!(
        cpp.operators, mozcpp.operators,
        "Cpp and Mozcpp must agree on the operator vocabulary"
    );
    assert_eq!(
        cpp.operands, mozcpp.operands,
        "Cpp and Mozcpp must agree on the operand vocabulary"
    );

    for (row, spellings) in KEYWORD_ROWS {
        for spelling in *spellings {
            assert!(
                cpp.operators.iter().any(|o| o == spelling),
                "{row}: `{spelling}` must be an operator; operators were {:?}",
                cpp.operators
            );
            assert!(
                !cpp.operands.iter().any(|o| o == spelling),
                "{row}: `{spelling}` must not be an operand; operands were {:?}",
                cpp.operands
            );
        }
    }
    // The fixture spells no control keyword, so its alphabetic
    // operators are exactly the three rows: an operator outside them
    // would be a keyword billed by the getter's own arm as well.
    let keyword_operators = cpp
        .operators
        .iter()
        .filter(|o| o.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .count();
    assert_eq!(
        keyword_operators,
        KEYWORD_ROWS.iter().map(|(_, s)| s.len()).sum::<usize>(),
        "an alphabetic operator outside the three rows: {:?}",
        cpp.operators
    );
}

#[cfg(all(feature = "cpp", feature = "mozcpp"))]
#[test]
fn cpp_and_mozcpp_agree_on_comparison_and_short_circuit_rows() {
    // #1694 declared the `Comparison` and `ShortCircuit` rows of both
    // dialects through the shared `cpp_family_*` fragments of
    // `roles/c_family.rs`, and every consumer — ABC, cyclomatic,
    // cognitive, Halstead — reads them. One fixture spelling every payer
    // in its applying form, the fold forms, the ISO alternative
    // spellings mixed with their symbols, and the non-applying spellings
    // (an overload name, a reference declarator, a requires clause, a
    // template delimiter) that must score nothing; the expected values
    // are written out so a kind dropped from *both* dialects' rows fails
    // by name rather than by parity.
    let source = r"
        template <typename T> concept A = requires(T t) { t.a(); };
        template <typename T> concept B = requires(T t) { t.b(); };
        template <typename T> requires A<T> && B<T> void both(T) { }
        template <typename T> requires (A<T> || B<T>) void either(T) { }
        template <typename... Ts> bool fold(Ts... ts) { return (ts && ...) || (... or ts) || (ts == ...); }
        struct S { bool operator<(const S&) const; bool operator and(const S&) const; void q() &&; };
        bool f(int&& x, int y, bool a, bool b, std::vector<int> v) {
            bool c = x < y || x > y || x <= y || x >= y || x == y || x != y || x not_eq y;
            auto d = x <=> y;
            if (a && b and x > 1 || c or d < 0) { return true; }
            return a;
        }
    ";

    let cpp = metric_sums(LANG::Cpp, source, "cpp");
    let mozcpp = metric_sums(LANG::Mozcpp, source, "cpp");
    assert_eq!(
        cpp, mozcpp,
        "Cpp and Mozcpp must agree on the row consumers"
    );
    let get = |key: &str| metric(&cpp, key);
    // cyclomatic: a base of 1 per space — the unit, `both`, `either`,
    // `fold`, `S` and `f` (the bodiless overload declarations open
    // none) — plus the decisions: `fold` the `&&` fold, the `or` fold
    // and its two `||` = 4; `f` the six `||` of `c`, the `if`, its
    // `&&`, `and`, `||`, `or` = 11; the requires clauses, the overload
    // names and the `int&&` 0. 6 + 15.
    assert_eq!(get("cyclomatic"), 6 + 15, "cyclomatic: {cpp:?}");
    // abc.conditions: `fold` — `(ts && ...)` one written operand + the
    // unwritten side = 2, `(... or ts)` likewise 2, `(ts == ...)` the
    // `==` = 1, and the two `||` each pay their chain operands, which
    // are folds already paid = 0: 5. `f` — seven comparison tokens in
    // `c` = 7 (their `||` operands are all comparisons, paid), `<=>` = 1,
    // the `if`: `x > 1` and `d < 0` = 2, operands `a`, `b`, `c` = 3: 13.
    // The requires clauses, the overloads and the `int&&` pay 0.
    assert_eq!(get("abc.conditions"), 5 + 13, "abc.conditions: {cpp:?}");
    // cognitive: `fold` one `||` sequence (+1) — the inner folds are
    // sequences of their own operator but each is one token, +1 for the
    // `&&` fold, +1 for the `or` fold; `f` the `if` (+1), one `&&`/`and`
    // sequence (+1), one `||`/`or` sequence (+1), the `||` sequence of
    // `c` (+1).
    assert_eq!(get("cognitive"), 3 + 4, "cognitive: {cpp:?}");
}
