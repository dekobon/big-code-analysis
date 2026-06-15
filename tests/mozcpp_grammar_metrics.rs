//! End-to-end metric coverage for the opt-in Mozcpp dialect
//! (`LANG::Mozcpp`, #720).
//!
//! Mozcpp owns no file extensions, so no integration-snapshot corpus
//! exercises it (lesson #74). Its metric impls are clones of `CppCode`
//! and previously had only a parse-only check (`src/c_langs_macros`) and
//! the `cpp`-vs-`mozcpp` parity guard. These tests prove Mozcpp computes
//! structural and complexity metrics on the **Gecko constructs that
//! justify keeping the fork** — `MOZ_*` macro annotations on a class, and
//! a `QM_TRY`-style decl-first-argument call — which upstream
//! `tree-sitter-cpp` (now backing `LANG::Cpp`) does not parse.
//!
//! The suite needs the `mozcpp` Cargo feature, so it lives in a
//! `#[cfg(feature = "mozcpp")]` module rather than a crate-level `#![cfg]`
//! (which would leave an empty, undocumented crate under the
//! minimal-langs / no-default-features CI legs).

#![allow(clippy::float_cmp)]

#[cfg(feature = "mozcpp")]
mod mozcpp_metrics {
    use big_code_analysis::{FuncSpace, LANG, MetricsOptions, Source, SpaceKind, analyze};

    fn mozcpp_space(source: &str) -> FuncSpace {
        analyze(
            Source::new(LANG::Mozcpp, source.as_bytes()).with_name(Some("g.cpp".to_owned())),
            MetricsOptions::default(),
        )
        .expect("Mozcpp parser produced a FuncSpace")
    }

    fn has_class(s: &FuncSpace) -> bool {
        s.kind == SpaceKind::Class || s.spaces.iter().any(has_class)
    }

    /// A `MOZ_*`-annotated class with two public methods: the
    /// `macro_annotation` overlay must not derail class / method
    /// recognition, so the `MozcppCode` `Checker::is_func_space`
    /// (`ClassSpecifier`), `Getter::get_space_kind` (Class), and `Npm`
    /// arms all fire.
    #[test]
    fn mozcpp_moz_annotated_class_metrics() {
        let space = mozcpp_space(
            "class MOZ_STACK_CLASS Parser {
             public:
                 int parse(int n) {
                     if (n > 0) {
                         return n;
                     }
                     return 0;
                 }
                 void reset() {}
             };",
        );
        let m = &space.metrics;
        // The MOZ_-annotated class still exposes its two public methods.
        assert_eq!(
            m.npm.class_npm_sum(),
            2,
            "MOZ_-annotated class public methods"
        );
        // A Class space is produced under the annotation overlay.
        assert!(has_class(&space), "Mozcpp recognised the class space");
        // cyclomatic: every space seeds 1, so the sum is
        // unit(1) + class Parser(1) + parse(1 + the `if` 1) + reset(1) = 5.
        // Pinning the exact value catches a dropped `if` decision point
        // (sum 4), which a loose `>= 3` would silently admit.
        assert_eq!(
            m.cyclomatic.cyclomatic_sum(),
            5,
            "cyclomatic: unit + class + (parse + if) + reset"
        );
    }

    /// `QM_TRY_INSPECT` is a Gecko decl-first-argument macro that only the
    /// mozcpp overlay parses; upstream `tree-sitter-cpp` ERROR-cascades on
    /// it. Ensure the enclosing function's metrics are computed rather
    /// than lost to an error cascade.
    #[test]
    fn mozcpp_qm_try_function_metrics() {
        let space = mozcpp_space(
            "nsresult Foo() {
                 QM_TRY_INSPECT(const int32_t& v, MOZ_TO_RESULT_INVOKE(x, Get));
                 if (v > 0) {
                     return NS_OK;
                 }
                 return NS_ERROR_FAILURE;
             }",
        );
        let m = &space.metrics;
        assert_eq!(m.nom.functions_sum(), 1, "function recognised");
        assert_eq!(m.nexits.nexits_sum(), 2, "two returns");
        // cyclomatic: unit(1) + function Foo(1) + the `if` (1) = 3. Exact
        // equality catches both a dropped `if` (sum 2) and an overcount
        // (e.g. a `QM_TRY_INSPECT` macro argument spuriously adding a
        // branch), which a loose `>= 3` would miss.
        assert_eq!(
            m.cyclomatic.cyclomatic_sum(),
            3,
            "cyclomatic: unit + Foo + if"
        );
    }

    /// On non-Gecko C++, Mozcpp and the upstream-backed `Cpp` grammar must
    /// agree — the overlay only adds rules, it does not change how
    /// ordinary C++ is measured. (Complements `tests/cpp_mozcpp_parity.rs`,
    /// here via a class so the `npm`/`npa`/`wmc` class arms are part of the
    /// comparison.)
    #[test]
    fn mozcpp_matches_cpp_on_plain_class() {
        let src = "class Widget {
             public:
                 int width;
                 int area(int w, int h) { return w * h; }
                 int perimeter(int w, int h) { return 2 * (w + h); }
             private:
                 int cached;
             };";
        let moz = mozcpp_space(src);
        let cpp = analyze(
            Source::new(LANG::Cpp, src.as_bytes()).with_name(Some("g.cpp".to_owned())),
            MetricsOptions::default(),
        )
        .expect("Cpp parser produced a FuncSpace");
        assert_eq!(
            moz.metrics.npm.class_npm_sum(),
            cpp.metrics.npm.class_npm_sum(),
            "npm parity"
        );
        assert_eq!(
            moz.metrics.npa.class_npa_sum(),
            cpp.metrics.npa.class_npa_sum(),
            "npa parity"
        );
        assert_eq!(
            moz.metrics.wmc.total_wmc(),
            cpp.metrics.wmc.total_wmc(),
            "wmc parity"
        );
        // Non-degenerate: two public methods and one public attribute
        // (`width`; the private `cached` is not an NPA), so the parity
        // above compares real non-zero values.
        assert_eq!(moz.metrics.npm.class_npm_sum(), 2, "two public methods");
        assert_eq!(moz.metrics.npa.class_npa_sum(), 1, "one public attribute");
    }

    /// ABC condition counting must also match across the two grammars.
    /// This guards the `cpp_inspect_container` / `cpp_count_unary_conditions`
    /// helpers, which match on node-kind *names* (#732, mirroring the
    /// `npa` fix in #731) rather than the `Cpp` enum discriminants — the
    /// Mozilla fork assigns different `kind_id`s to the same kinds, so a
    /// name-agnostic match is the only grammar-correct option.
    ///
    /// The fixture's boolean operands are deliberately not bare
    /// identifiers: `identifier` has `kind_id` 1 in *both* grammars, so a
    /// fixture of `a`/`b`/`c` would pass even under the buggy id-based
    /// code and protect nothing. The terminals exercised here all have
    /// `kind_id`s that **diverge** between `Cpp` and the fork —
    /// `field_expression` (361 vs 450), `subscript_expression`
    /// (349 vs 438), `qualified_identifier` (484 vs 573), `call_expression`
    /// (251 vs 340), `cast_expression` (343 vs 432) — so id-based matching
    /// silently miscounts them for Mozcpp. Reverting either helper to
    /// `kind_id().into()` against `Cpp` makes this test fail.
    #[test]
    fn mozcpp_matches_cpp_on_conditions() {
        // Operands span every divergent-id boolean terminal: `o->ready`
        // (field_expression), `arr[i]` (subscript_expression), `ns::enabled`
        // (qualified_identifier), `run()` (call_expression), `(bool)i`
        // (cast_expression).
        let src = "bool check(Obj* o, int* arr, int i) {
                 if (o->ready && arr[i]) { return ns::enabled || run(); }
                 return (bool)i ? o->ready : false;
             }";
        let moz = mozcpp_space(src);
        let cpp = analyze(
            Source::new(LANG::Cpp, src.as_bytes()).with_name(Some("g.cpp".to_owned())),
            MetricsOptions::default(),
        )
        .expect("Cpp parser produced a FuncSpace");
        assert_eq!(
            moz.metrics.abc.conditions_sum(),
            cpp.metrics.abc.conditions_sum(),
            "abc conditions parity"
        );
        assert_eq!(
            moz.metrics.abc.assignments_sum(),
            cpp.metrics.abc.assignments_sum(),
            "abc assignments parity"
        );
        // Non-degenerate, and pins the divergent-id terminals are all
        // counted: o->ready, arr[i], ns::enabled, run(), (bool)i = 5.
        assert_eq!(
            cpp.metrics.abc.conditions_sum(),
            5,
            "fixture exercises all five divergent-id boolean terminals"
        );
    }
}
