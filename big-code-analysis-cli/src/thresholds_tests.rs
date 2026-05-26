// Sibling-file unit tests for the threshold engine, wired in via
// `#[path = "thresholds_tests.rs"] mod tests;` so the production
// `thresholds.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use super::*;

/// Locks the threshold-engine extractor vocabulary against
/// `MetricKind::for_threshold_name` so the two stay in sync.
/// If a new threshold extractor is added without a matching
/// suppression mapping (or vice versa), this test fails loudly
/// rather than silently dropping suppression for the new metric.
/// `tokens` is the documented exception: it is never suppressible
/// (see `src/suppression.rs::for_threshold_name`).
#[test]
fn every_extractor_resolves_to_metric_kind_or_is_tokens() {
    for extractor in EXTRACTORS {
        let is_suppressible = MetricKind::for_threshold_name(extractor.name).is_some();
        let expected = extractor.name != "tokens";
        assert_eq!(
            is_suppressible, expected,
            "extractor `{}` suppressibility mismatch — expected {expected}, got {is_suppressible}",
            extractor.name,
        );
    }
}

#[test]
fn parse_cli_threshold_accepts_integer() {
    let (name, limit) = parse_cli_threshold("cyclomatic=15").expect("parses");
    assert_eq!(name, "cyclomatic");
    assert_eq!(limit, 15.0);
}

#[test]
fn parse_cli_threshold_accepts_dotted_name_and_float() {
    let (name, limit) = parse_cli_threshold("halstead.volume=12.5").expect("parses");
    assert_eq!(name, "halstead.volume");
    assert_eq!(limit, 12.5);
}

#[test]
fn parse_cli_threshold_accepts_zero() {
    // `0` is meaningful: "no value allowed" is distinct from "no
    // threshold set". Must parse, not be rejected as falsy.
    let (_, limit) = parse_cli_threshold("nargs=0").expect("parses");
    assert_eq!(limit, 0.0);
}

#[test]
fn parse_cli_threshold_rejects_missing_equals() {
    let err = parse_cli_threshold("cyclomatic15").expect_err("missing `=` must error");
    assert!(err.contains("metric=limit"), "{err}");
}

#[test]
fn parse_cli_threshold_rejects_empty_name() {
    let err = parse_cli_threshold("=15").expect_err("empty name must error");
    assert!(err.contains("empty metric name"), "{err}");
}

#[test]
fn parse_cli_threshold_rejects_negative_limit() {
    let err = parse_cli_threshold("cyclomatic=-1").expect_err("negative limit must error");
    assert!(err.contains("non-negative"), "{err}");
}

#[test]
fn parse_cli_threshold_rejects_nan_limit() {
    let err = parse_cli_threshold("cyclomatic=nan").expect_err("NaN limit must error");
    assert!(err.contains("non-negative"), "{err}");
}

#[test]
fn build_rejects_unknown_metric() {
    let mut raw = BTreeMap::new();
    raw.insert("not_a_metric".to_string(), 1.0);
    let err = ThresholdSet::build(&raw).expect_err("unknown name");
    assert!(err.contains("unknown threshold metric"), "{err}");
    assert!(err.contains("not_a_metric"), "{err}");
}

#[test]
fn build_accepts_zero_limit() {
    let mut raw = BTreeMap::new();
    raw.insert("nargs".to_string(), 0.0);
    ThresholdSet::build(&raw).expect("zero limit is valid");
}

#[test]
fn known_metric_names_contains_core_set() {
    let names = known_metric_names();
    for required in [
        "cognitive",
        "cyclomatic",
        "halstead.volume",
        "loc.lloc",
        "nargs",
    ] {
        assert!(
            names.contains(&required),
            "missing {required:?} in {names:?}"
        );
    }
}

#[test]
fn config_parses_thresholds_table() {
    let toml_src = "[thresholds]\ncyclomatic = 15\n\"loc.lloc\" = 200\n";
    let cfg: ThresholdConfig = toml::from_str(toml_src).expect("parses");
    assert_eq!(cfg.thresholds.get("cyclomatic"), Some(&15.0));
    assert_eq!(cfg.thresholds.get("loc.lloc"), Some(&200.0));
}

#[test]
fn violation_display_is_stable() {
    let v = Violation {
        path: "src/foo.rs".into(),
        start_line: 10,
        end_line: 25,
        function: "do_stuff".into(),
        metric: "cyclomatic",
        value: 17.0,
        limit: 15.0,
    };
    assert_eq!(
        v.to_string(),
        "src/foo.rs:10-25: do_stuff: cyclomatic = 17 (limit 15)"
    );
}

#[test]
fn violation_display_keeps_fractional_precision() {
    let v = Violation {
        path: "x".into(),
        start_line: 1,
        end_line: 1,
        function: String::new(),
        metric: "halstead.volume",
        value: 12.5,
        limit: 10.0,
    };
    assert!(v.to_string().contains("= 12.5"), "{v}");
    assert!(v.to_string().contains("limit 10)"), "{v}");
}

/// Non-UTF-8 path bytes must survive the threshold pipeline
/// byte-for-byte. Pre-#240 the `Violation::path: String` field
/// (built from `&str` via `to_string()`) discarded them at the
/// `evaluate` boundary. Gated on `cfg(unix)` because
/// `OsString::from_vec` is Unix-only — Windows paths are
/// constrained differently (WTF-8) and out of scope for this
/// regression.
#[cfg(unix)]
#[test]
fn violation_path_preserves_non_utf8_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    // 0xFF / 0xFE form a lone surrogate pair under UTF-8 and
    // would have been replaced with U+FFFD by `to_string_lossy`.
    let raw_bytes: &[u8] = b"non-utf8-\xff\xfe.rs";
    let path = PathBuf::from(OsString::from_vec(raw_bytes.to_vec()));

    let v = Violation {
        path: path.clone(),
        start_line: 1,
        end_line: 1,
        function: "f".to_string(),
        metric: "cyclomatic",
        value: 5.0,
        limit: 1.0,
    };

    // Raw bytes round-trip identically — no lossy substitution.
    assert_eq!(v.path.as_os_str().as_encoded_bytes(), raw_bytes);
    // Display does not panic on non-UTF-8 bytes (uses
    // `Path::display`, which substitutes U+FFFD).
    let rendered = v.to_string();
    assert!(rendered.contains("cyclomatic"), "{rendered}");
}

use big_code_analysis::{SpaceKind, SuppressionScope};
use std::collections::BTreeSet;

/// Build a leaf `FuncSpace` with no children. Cyclomatic defaults to
/// `1.0`, so a `limit = 0` makes the threshold fire deterministically
/// without forcing the suppression tests to construct a real parse.
fn space(name: &str, kind: SpaceKind, suppressed: SuppressionScope) -> FuncSpace {
    FuncSpace {
        name: Some(name.into()),
        start_line: 1,
        end_line: 10,
        kind,
        spaces: Vec::new(),
        metrics: CodeMetrics::default(),
        suppressed,
    }
}

fn threshold_set(name: &str, limit: f64) -> ThresholdSet {
    let mut raw = BTreeMap::new();
    raw.insert(name.into(), limit);
    ThresholdSet::build(&raw).expect("threshold builds")
}

fn only_func_scope(metric: MetricKind) -> SuppressionScope {
    SuppressionScope::Some(BTreeSet::from([metric]))
}

#[test]
fn honor_policy_suppresses_matching_function_scope() {
    // `bca: suppress(cyclomatic)` on the function silences a cyclomatic
    // violation when the policy honors markers — the headline
    // behaviour the CLI relies on.
    let mut out = Vec::new();
    let s = space(
        "noisy",
        SpaceKind::Function,
        only_func_scope(MetricKind::Cyclomatic),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "matching function-scoped marker should silence, got {out:?}",
    );
}

#[test]
fn honor_policy_emits_for_non_matching_metric() {
    // A marker covering only `cognitive` must not silence a
    // `cyclomatic` violation — symmetry with the previous test.
    let mut out = Vec::new();
    let s = space(
        "noisy",
        SpaceKind::Function,
        only_func_scope(MetricKind::Cognitive),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        &mut out,
    );
    assert_eq!(out.len(), 1, "expected one violation; got {out:?}");
    assert_eq!(out[0].metric, "cyclomatic");
}

#[test]
fn ignore_policy_emits_despite_matching_marker() {
    // `--no-suppress` (Ignore) must surface violations even when the
    // function carries a covering marker — that's the audit path.
    let mut out = Vec::new();
    let s = space(
        "noisy",
        SpaceKind::Function,
        only_func_scope(MetricKind::Cyclomatic),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Ignore,
        &mut out,
    );
    assert_eq!(out.len(), 1, "expected one violation; got {out:?}");
}

#[test]
fn file_scope_silences_nested_function() {
    // `allow-file(cyclomatic)` lives on the top-level Unit space
    // and must apply to every nested function too. The nested
    // function carries the default (empty) scope; suppression
    // comes entirely from the file scope.
    let mut out = Vec::new();
    let mut unit = space(
        "fixture.rs",
        SpaceKind::Unit,
        only_func_scope(MetricKind::Cyclomatic),
    );
    unit.spaces.push(space(
        "inner",
        SpaceKind::Function,
        SuppressionScope::default(),
    ));
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &unit,
        SuppressionPolicy::Honor,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "file-scoped marker should also silence nested fn; got {out:?}",
    );
}

#[test]
fn tokens_threshold_never_suppressed() {
    // `MetricKind::for_threshold_name("tokens")` returns None, so
    // the evaluator cannot map the threshold name onto any
    // suppression metric family. Result: even a function carrying
    // `SuppressionScope::All` fails to silence a `tokens`
    // violation. This is intentional — `tokens` is a hard
    // resource cap (not a maintainability heuristic), and we
    // don't want markers turning it off.
    //
    // We construct ThresholdSet manually with limit `-0.5` so
    // tokens_sum default of 0.0 still exceeds it, since
    // `ThresholdSet::build` rejects negative limits.
    assert_eq!(MetricKind::for_threshold_name("tokens"), None);

    let extractor = EXTRACTORS
        .iter()
        .find(|e| e.name == "tokens")
        .expect("tokens extractor exists");
    let set = ThresholdSet {
        entries: vec![(extractor, -0.5)],
    };

    let mut out = Vec::new();
    let s = space("noisy", SpaceKind::Function, SuppressionScope::All);
    set.evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        &mut out,
    );
    assert_eq!(
        out.len(),
        1,
        "tokens violation must survive SuppressionScope::All",
    );
    assert_eq!(out[0].metric, "tokens");
}

// -- render_violation_line --------------------------------------------

fn sample_violation() -> Violation {
    Violation {
        path: PathBuf::from("src/foo.rs"),
        start_line: 10,
        end_line: 20,
        function: "do_thing".to_string(),
        metric: "cyclomatic",
        value: 30.0,
        limit: 10.0,
    }
}

#[test]
fn render_no_tag_byte_identical_to_display() {
    // Load-bearing backward-compat invariant: invocations without
    // --baseline must continue emitting the exact same stderr line
    // shape as today. CI tooling grep-anchors on the leading path.
    let v = sample_violation();
    assert_eq!(render_violation_line(&v, None), format!("{v}"));
}

#[test]
fn render_new_tag_prefixes_line() {
    let v = sample_violation();
    let out = render_violation_line(&v, Some(&Coverage::New));
    assert!(out.starts_with("[new] "), "got: {out}");
    // The rest of the line is unchanged.
    assert_eq!(out.strip_prefix("[new] ").unwrap(), format!("{v}"));
}

#[test]
fn render_regressed_integer_percent() {
    let v = sample_violation();
    // recorded 20, value 30 → +50%
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 20.0 }));
    assert!(out.starts_with("[regr +50%] "), "got: {out}");
}

#[test]
fn render_regressed_rounds_half_to_nearest_even_or_away() {
    // Halstead can produce values that round to a nearby integer;
    // we use f64::round (half-away-from-zero) — pin the boundary.
    let mut v = sample_violation();
    v.value = 100.5;
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 100.0 }));
    // (100.5 - 100) / 100 * 100 = 0.5 → rounds to 1.
    assert!(out.starts_with("[regr +1%] "), "got: {out}");
}

#[test]
fn render_regressed_caps_above_9999_percent() {
    // recorded 1, value 1e6 → ratio 999900 → cap at "+>9999%".
    let mut v = sample_violation();
    v.value = 1_000_000.0;
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 1.0 }));
    assert!(out.starts_with("[regr +>9999%] "), "got: {out}");
}

#[test]
fn render_regressed_at_9999_percent_boundary() {
    // 9999% exactly must NOT be capped — the cap applies *above* 9999.
    // Pick values that compute to exactly pct = 9999.0 in f64:
    //   recorded = 100, value = 10099
    //   (10099 - 100) / 100 * 100 = 9999.0   (all values exact in f64)
    // This pins both the cap threshold *and* the inclusivity of the
    // `>` operator: a mutation flipping `>` to `>=` would cap at this
    // input and emit `[regr +>9999%]`, failing the assertion.
    let mut v = sample_violation();
    v.value = 10099.0;
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 100.0 }));
    assert!(out.starts_with("[regr +9999%] "), "got: {out}");
}

#[test]
fn render_regressed_with_zero_recorded() {
    // Avoid divide-by-zero; render `[regr from 0]` instead.
    let v = sample_violation();
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 0.0 }));
    assert!(out.starts_with("[regr from 0] "), "got: {out}");
}

#[test]
fn render_regressed_with_nan_value() {
    // Degenerate Halstead inputs can produce NaN; render
    // `[regr NaN]` rather than crashing on `NaN.round()` (which is
    // NaN; cast to i64 saturates to 0 — would emit `+0%`, misleading).
    let mut v = sample_violation();
    v.value = f64::NAN;
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 5.0 }));
    assert!(out.starts_with("[regr NaN] "), "got: {out}");
}

#[test]
fn render_covered_falls_back_to_unprefixed() {
    // Covered violations are filtered out before reaching the
    // renderer in production. This test pins the defensive
    // fallback so a future refactor that accidentally pipes
    // Covered to the renderer doesn't crash or emit a misleading
    // tag — it just renders the unprefixed line.
    let v = sample_violation();
    let out = render_violation_line(&v, Some(&Coverage::Covered { recorded: 30.0 }));
    assert_eq!(out, format!("{v}"));
}
