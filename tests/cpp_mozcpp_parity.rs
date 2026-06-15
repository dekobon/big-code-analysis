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

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// Headline integer metric sums for one parse of `source` as `lang`.
fn metric_sums(lang: LANG, source: &str, ext: &str) -> Vec<(&'static str, u64)> {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .expect("parser produced a FuncSpace");
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
    let get = |key: &str| {
        cpp.iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| panic!("metric_sums omitted {key}: {cpp:?}"), |(_, v)| *v)
    };
    // conditions: `<=>` +1, the `< 0` on its result +1, the `if (a < b)`
    // +1, `try` +1, `catch` +1, the `less ? a : b` ternary +1 = 6.
    assert_eq!(
        get("abc.conditions"),
        6,
        "fixture exercises <=>/try/catch conditions: {cpp:?}"
    );
    assert!(
        get("abc.assignments") >= 1 && get("abc.branches") >= 1,
        "fixture should exercise assignments and branches: {cpp:?}"
    );
}
