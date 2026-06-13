#![cfg(feature = "c")]
#![allow(clippy::float_cmp)]

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

/// A C function with the decision kinds the `CCode` cyclomatic/cognitive/
/// abc/nexits/nargs/halstead arms must each handle: `if` + `&&`, a `for`
/// loop, a `switch` with a non-default `case`, assignments, a call, and
/// two `return`s.
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
    // Halstead populated both operator and operand tables (the pruned C
    // `get_op_type` must classify C's operators, not fall through to
    // Unknown).
    assert!(
        m.halstead.total_operators() > 0 && m.halstead.total_operands() > 0,
        "halstead operators/operands populated: {} / {}",
        m.halstead.total_operators(),
        m.halstead.total_operands()
    );
    // Nested control flow yields non-zero cognitive complexity.
    assert!(m.cognitive.cognitive_sum() > 0, "cognitive > 0");
}

/// C has no classes/methods/attributes, so `npm` / `npa` / `wmc` are
/// no-op impls (decision-log #9) and no `Class` space is ever produced —
/// a `struct` is a data aggregate, not a function space.
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
