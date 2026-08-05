//! End-to-end metric coverage for the dedicated C grammar
//! (`LANG::C`, #721).
//!
//! Before #721, C routed through the C++ grammar; the `CCode` metric
//! impls — `Checker` / `Getter` / `Alterator` plus the per-metric arms,
//! several hand-pruned from the `CppCode` clones (no class/namespace/
//! lambda/template, `return`-only exits, classless `npm`/`npa`/`wmc`) —
//! had almost no *direct* `CParser` coverage (the existing `c_*` tests
//! drive `CppParser`, tracked in #730). These exercise the real
//! `analyze(Source::new(LANG::C, …))` path so a regression in the C
//! impls is caught against the actual `tree-sitter-c` parse trees.
//!
//! The whole suite needs the `c` Cargo feature, so it lives in a
//! `#[cfg(feature = "c")]` module rather than a crate-level `#![cfg]`
//! (which would leave an empty, undocumented crate under the
//! minimal-langs / no-default-features CI legs).

#![allow(clippy::float_cmp)]

#[cfg(feature = "c")]
mod c_metrics {
    use big_code_analysis::{FuncSpace, LANG, MetricsOptions, Source, SpaceKind, analyze};

    fn c_space(source: &str) -> FuncSpace {
        analyze(
            Source::new(LANG::C, source.as_bytes()).with_name(Some("m.c".to_owned())),
            MetricsOptions::default(),
        )
        .expect("C parser produced a FuncSpace")
    }

    fn has_class(s: &FuncSpace) -> bool {
        s.kind == SpaceKind::Class || s.spaces.iter().any(has_class)
    }

    /// A C function with the decision kinds the `CCode` cyclomatic/
    /// cognitive/abc/nexits/nargs/halstead arms must each handle: `if` +
    /// `&&`, a `for` loop, a `switch` with a non-default `case`,
    /// assignments, a call, and two `return`s.
    #[test]
    fn c_function_metrics_are_computed() {
        let space = c_space(
            "int classify(int a, int b) {
                 int r = 0;
                 if (a > 0 && b > 0) {
                     r = a + b;
                 }
                 for (int i = 0; i < a; ++i) {
                     r += compute(i);
                 }
                 switch (b) {
                     case 0:
                         return r;
                     default:
                         return -r;
                 }
             }",
        );
        let m = &space.metrics;

        // cyclomatic: unit(1) + fn(1) + if(1) + &&(1) + for(1) + case(1) = 6.
        // `default` and the call do not add a decision.
        assert_eq!(m.cyclomatic.cyclomatic_sum(), 6, "cyclomatic");
        // C has no `throw`, so only the two `return`s are exits.
        assert_eq!(m.nexits.nexits_sum(), 2, "nexits");
        // One function; C has no closures.
        assert_eq!(m.nom.functions_sum(), 1, "functions");
        assert_eq!(m.nom.closures_sum(), 0, "closures (C has none)");
        // `classify` takes two parameters.
        assert_eq!(m.nargs.function_args_sum(), 2, "function args");
        // Halstead populated both operator and operand tables (the pruned
        // C `get_op_type` must classify C's operators, not fall through to
        // Unknown).
        assert!(
            m.halstead.total_operators() > 0 && m.halstead.total_operands() > 0,
            "halstead operators/operands populated: {} / {}",
            m.halstead.total_operators(),
            m.halstead.total_operands()
        );
        // cognitive: if +1, `&&` boolean sequence +1, for +1, switch +1 = 4
        // (each construct sits at function-body top level, so no nesting
        // surcharge applies). Pinning the exact value guards the C
        // cognitive arm's boolean-sequence/nesting accounting, which the
        // cyclomatic assertion above does not redundantly cover.
        assert_eq!(m.cognitive.cognitive_sum(), 4, "cognitive");
    }

    /// Upstream-grammar limitation, deliberately pinned (#1209): in
    /// `tree-sitter-c` 0.24.2 the old-style (K&R) function definition
    /// nests its declarator *inside* the old-style declarator, so an
    /// outer `pointer_declarator` is unreachable. A K&R definition whose
    /// return type wraps the declarator therefore parses as a plain
    /// `declaration` that swallows the first parameter declaration, with
    /// the body orphaned as a bare `compound_statement` sibling — no
    /// `ERROR` node, no `function_definition`, so `is_func` never sees a
    /// node to match. `LANG::Objc` shares the defect and `Cpp` / `Mozcpp`
    /// open no space for either form (no K&R rule at all); both are
    /// recorded in the book's Supported Languages page rather than here,
    /// since this suite drives `LANG::C` only.
    ///
    /// The `krptr` values below are a bug-lock, not an endorsement: an
    /// issue is open on this and the pinned numbers record only where
    /// the decisions land *today* (on the file's unit space). A grammar
    /// bump that parses the form correctly must fail this test rather
    /// than shift metrics silently. `krplain` is the control — the
    /// unwrapped K&R form works and must keep working, which is what
    /// makes the contrast attributable to the wrapping declarator.
    #[test]
    fn c_knr_wrapped_return_type_opens_no_function_space() {
        let plain = c_space("int krplain(a, b) int a; int b; { if (a) { return 0; } return 1; }");
        assert_eq!(plain.spaces.len(), 1, "one space for the plain K&R form");
        assert_eq!(plain.spaces[0].kind, SpaceKind::Function, "space kind");
        assert_eq!(plain.spaces[0].name.as_deref(), Some("krplain"), "name");
        assert_eq!(plain.metrics.nargs.function_args_sum(), 2, "K&R args");
        // unit(1) + fn(1) + if(1) = 3.
        assert_eq!(plain.metrics.cyclomatic.cyclomatic_sum(), 3, "cyclomatic");

        let wrapped = c_space("int *krptr(a, b) int a; int b; { if (a) { return 0; } return 1; }");
        assert!(
            wrapped.spaces.is_empty(),
            "#1209: the wrapped return type currently opens no space; a \
             grammar fix makes this fail, which is the point"
        );
        assert_eq!(wrapped.metrics.nom.functions_sum(), 0, "no function");
        // The orphaned body's `if` is charged to the unit: unit(1) + if(1).
        assert_eq!(
            wrapped.metrics.cyclomatic.cyclomatic_sum(),
            2,
            "the stray decision lands on the file's unit space"
        );
    }

    /// C has no classes/methods/attributes, so `npm` / `npa` / `wmc` are
    /// no-op impls (decision-log #9) and no `Class` space is ever produced
    /// — a `struct` is a data aggregate, not a function space.
    #[test]
    fn c_has_no_class_metrics() {
        let space = c_space(
            "struct point { int x; int y; };
             int dist2(struct point p) { return p.x * p.x + p.y * p.y; }",
        );
        let m = &space.metrics;
        assert_eq!(m.npm.total_npm(), 0, "npm is a no-op for C");
        assert_eq!(m.npa.total_npa(), 0, "npa is a no-op for C");
        assert_eq!(m.wmc.total_wmc(), 0, "wmc is a no-op for C");
        // The function is still counted.
        assert_eq!(m.nom.functions_sum(), 1, "function counted");
        assert!(!has_class(&space), "C must not produce a Class space");
    }
}
