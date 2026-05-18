//! Integration tests for in-source suppression markers (#98).
//!
//! Exercises the comment-extraction path through `analyze`
//! across a representative C-family language (C++) and a non-C-family
//! language (Python and Rust), per the issue's acceptance criteria.
//! Tests cover both native `bca:` markers and Lizard compatibility
//! markers, plus the unknown-metric error path.

#![allow(clippy::needless_raw_string_hashes)]

use big_code_analysis::{
    FuncSpace, LANG, MetricKind, MetricsOptions, Source, SuppressionScope, analyze,
};

fn analyze_lang(source: &str, path: &str) -> FuncSpace {
    let ext = path.rsplit('.').next().unwrap_or("");
    let lang = match ext {
        "py" => LANG::Python,
        "cpp" | "cc" | "hpp" | "h" => LANG::Cpp,
        "rs" => LANG::Rust,
        other => panic!("unsupported test extension {other:?}"),
    };
    analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(path.to_owned())),
        MetricsOptions::default(),
    )
    .expect("parser produced no top-level FuncSpace")
}

/// Recursively locate the first non-Unit space whose name matches
/// `name`. Tests use this to assert markers attached to the correct
/// function rather than leaking up to the file-level space.
fn find_function<'a>(space: &'a FuncSpace, name: &str) -> Option<&'a FuncSpace> {
    if space.name.as_deref() == Some(name) {
        return Some(space);
    }
    space.spaces.iter().find_map(|s| find_function(s, name))
}

#[test]
fn python_native_function_scoped_marker_attaches_to_enclosing_function() {
    // Python is a non-C-family language: comments are `#`, distinct
    // from C/C++ `//`/`/*…*/`. The marker should land on `noisy`, not
    // on `quiet` (different range) and not on the module-level Unit.
    let src = r#"
def quiet():
    return 1

def noisy(x):
    # bca: allow(cyclomatic)
    if x > 0:
        return 1
    return 0
"#;
    let space = analyze_lang(src, "fixture.py");
    let noisy = find_function(&space, "noisy").expect("noisy function should be present");
    assert!(
        noisy.suppressed.covers(MetricKind::Cyclomatic),
        "marker did not attach to noisy",
    );
    let quiet = find_function(&space, "quiet").expect("quiet function should be present");
    assert!(
        quiet.suppressed.is_empty(),
        "marker leaked to quiet (scope: {:?})",
        quiet.suppressed,
    );
    assert!(
        space.suppressed.is_empty(),
        "function-scoped marker should not bubble to file scope",
    );
}

#[test]
fn cpp_native_function_scoped_marker_attaches_to_enclosing_function() {
    // C++ exercises the `//`-comment path; the marker is identical to
    // the Python case but lives in a `// …` instead of `# …`.
    let src = r#"
int noisy(int x) {
    // bca: allow(cognitive, cyclomatic)
    if (x > 0) {
        return 1;
    }
    return 0;
}
"#;
    let space = analyze_lang(src, "fixture.cpp");
    let noisy = find_function(&space, "noisy").expect("noisy function should be present");
    assert!(noisy.suppressed.covers(MetricKind::Cognitive));
    assert!(noisy.suppressed.covers(MetricKind::Cyclomatic));
    assert!(!noisy.suppressed.covers(MetricKind::Halstead));
    // Symmetry with the Python sibling test: a function-scoped marker
    // must not bubble up to the file's Unit space. Without this
    // assertion a regression that attached every marker to the
    // top-level space would still pass the noisy-side checks.
    assert!(
        !space.suppressed.covers(MetricKind::Cognitive),
        "function-scoped marker should not bubble to file scope",
    );
    assert!(
        !space.suppressed.covers(MetricKind::Cyclomatic),
        "function-scoped marker should not bubble to file scope",
    );
}

#[test]
fn rust_block_comment_marker_attaches() {
    // Rust block-comment form `/* bca: allow */` exercises a different
    // comment kind_id than `//` line comments. The marker is inside a
    // function body so it must attach there, not at file scope.
    let src = r#"
fn noisy(x: i32) -> i32 {
    /* bca: allow */
    if x > 0 { 1 } else { 0 }
}
"#;
    let space = analyze_lang(src, "fixture.rs");
    let noisy = find_function(&space, "noisy").expect("noisy function should be present");
    assert!(noisy.suppressed.is_all(), "expected All scope on noisy");
}

#[test]
fn native_file_scoped_marker_lands_on_unit_space() {
    let src = r#"
# bca: allow-file(loc, halstead)

def fine():
    return 1
"#;
    let space = analyze_lang(src, "fixture.py");
    assert!(space.suppressed.covers(MetricKind::Loc));
    assert!(space.suppressed.covers(MetricKind::Halstead));
    assert!(!space.suppressed.covers(MetricKind::Cyclomatic));
    // The function should not have inherited the file-scope marker:
    // file scope is resolved at threshold-check time, not by physical
    // propagation through the space tree.
    let fine = find_function(&space, "fine").expect("function fine should exist");
    assert!(fine.suppressed.is_empty());
}

#[test]
fn lizard_function_marker_recognized_on_python() {
    // Lizard's `#lizard forgives` is verbatim Python-comment-shaped, so
    // Python is the natural compat-layer test.
    let src = r#"
def noisy(x):
    #lizard forgives
    if x > 0:
        return 1
    return 0
"#;
    let space = analyze_lang(src, "fixture.py");
    let noisy = find_function(&space, "noisy").expect("noisy function should be present");
    assert!(
        noisy.suppressed.is_all(),
        "Lizard marker should produce All scope",
    );
}

#[test]
fn lizard_file_marker_recognized_on_cpp() {
    // Lizard's `#lizard forgive global` placed in a C++ comment.
    let src = r#"
// #lizard forgive global

int fine() { return 1; }
"#;
    let space = analyze_lang(src, "fixture.cpp");
    assert!(space.suppressed.is_all());
}

#[test]
fn nested_function_marker_lands_on_inner_function() {
    // The innermost containing function wins. Without that rule, a
    // marker inside an inner function would silence the outer
    // function's metrics too, which surprises authors who annotate at
    // the closest scope.
    let src = r#"
fn outer() -> i32 {
    fn inner() -> i32 {
        // bca: allow(cyclomatic)
        1
    }
    inner()
}
"#;
    let space = analyze_lang(src, "fixture.rs");
    let outer = find_function(&space, "outer").expect("outer should be present");
    let inner = find_function(&space, "inner").expect("inner should be present");
    assert!(inner.suppressed.covers(MetricKind::Cyclomatic));
    assert!(
        outer.suppressed.is_empty(),
        "outer should not inherit inner's marker",
    );
    // Belt-and-braces: explicitly check the metric named in the
    // inner marker. `is_empty()` alone would still pass if a future
    // refactor switched `SuppressionScope` to a default that returned
    // `false` from `is_empty()` despite covering Cyclomatic.
    assert!(
        !outer.suppressed.covers(MetricKind::Cyclomatic),
        "outer should not cover Cyclomatic",
    );
}

#[test]
fn empty_scope_serializes_elided() {
    // Function spaces without markers must round-trip through JSON
    // without a `suppressed` key — otherwise existing snapshot consumers
    // would see every existing snapshot change, which the issue
    // explicitly forbids ("metric computation is unaffected").
    let src = r#"
def fine():
    return 1
"#;
    let space = analyze_lang(src, "fixture.py");
    let json = serde_json::to_string(&space).expect("serialize");
    assert!(
        !json.contains("\"suppressed\""),
        "expected `suppressed` to be elided when empty; got: {json}",
    );
}

#[test]
fn populated_scope_serializes_with_metrics_list() {
    // When a marker fires, the JSON output should expose the scope so
    // tooling (audit logs, dashboards) can see what was silenced
    // without re-parsing the source. Parse the JSON instead of
    // substring-matching: `"loc"` substring-matches `CodeMetrics.loc`
    // (the metric block, present on every space), which would let a
    // missing or malformed `suppressed` field slip through.
    let src = r#"
# bca: allow-file(loc)
"#;
    let space = analyze_lang(src, "fixture.py");
    let json = serde_json::to_string(&space).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let suppressed = value
        .get("suppressed")
        .expect("suppressed field present on file space");
    assert_eq!(
        suppressed.get("kind").and_then(|v| v.as_str()),
        Some("some"),
        "expected kind=some scope; got {suppressed}",
    );
    let metrics = suppressed
        .get("metrics")
        .and_then(|m| m.as_array())
        .expect("metrics array present");
    let metric_names: Vec<&str> = metrics.iter().filter_map(|m| m.as_str()).collect();
    assert_eq!(
        metric_names,
        ["loc"],
        "unexpected metrics list: {metrics:?}"
    );
}

#[test]
fn unknown_metric_in_marker_has_no_effect() {
    // Per the issue's "unknown identifiers must error so typos do not
    // silently widen scope" requirement. At the library boundary this
    // surfaces as a stderr warning (no propagated error type), so the
    // observable behaviour from an integration test is "the marker is
    // discarded": the enclosing function's scope stays empty. The
    // actual `SuppressionError::UnknownMetric(_)` variant is exercised
    // by the unit test `native_unknown_metric_errors` in
    // `src/suppression.rs`.
    let src = r#"
def fine():
    # bca: allow(no_such_metric)
    return 1
"#;
    let space = analyze_lang(src, "fixture.py");
    let fine = find_function(&space, "fine").expect("function fine should exist");
    assert!(
        fine.suppressed.is_empty(),
        "malformed marker should not produce suppressions; got {:?}",
        fine.suppressed,
    );
}

#[test]
fn marker_outside_any_function_is_silently_ignored() {
    // A function-scoped marker that lies outside every function body
    // has no enclosing function. Treat as a no-op (the issue says
    // file-scope is the explicit `allow-file` verb).
    let src = r#"
# bca: allow

def fine():
    return 1
"#;
    let space = analyze_lang(src, "fixture.py");
    assert!(space.suppressed.is_empty());
    let fine = find_function(&space, "fine").expect("function fine should exist");
    assert!(fine.suppressed.is_empty());
}

#[test]
fn multiple_markers_union_on_same_function() {
    // Stacking markers should union the metric lists, so an author
    // can split suppressions across multiple comment lines without
    // the second overwriting the first.
    let src = r#"
fn busy() -> i32 {
    // bca: allow(cyclomatic)
    // bca: allow(cognitive)
    if true { 1 } else { 0 }
}
"#;
    let space = analyze_lang(src, "fixture.rs");
    let busy = find_function(&space, "busy").expect("busy should be present");
    assert!(busy.suppressed.covers(MetricKind::Cyclomatic));
    assert!(busy.suppressed.covers(MetricKind::Cognitive));
}

#[test]
fn default_scope_does_not_cover_any_metric() {
    // Sanity check on the default `FuncSpace::suppressed` value: every
    // metric must report as not-covered when no markers fire, so the
    // threshold engine's invariant "evaluate with no markers ≡
    // evaluate ignoring markers" holds.
    let s = SuppressionScope::default();
    for &m in MetricKind::ALL {
        assert!(!s.covers(m), "default scope unexpectedly covers {m}");
    }
}
