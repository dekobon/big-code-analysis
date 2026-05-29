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

/// The threshold engine's extractor ids and the library's canonical
/// metric catalog must describe exactly the same set of offender ids,
/// in both directions. This is the cross-crate guard the consolidation
/// in #397 introduced: a metric added to one table but not the other
/// (the failure mode that left ten `RULE_DESCRIPTIONS` keys orphaned
/// for two model versions) fails here rather than silently shipping a
/// half-wired metric.
#[test]
fn extractor_ids_match_library_catalog() {
    use std::collections::BTreeSet;

    let extractor_ids: BTreeSet<&str> = EXTRACTORS.iter().map(|e| e.name).collect();
    let catalog_ids: BTreeSet<&str> = big_code_analysis::metric_catalog::METRICS
        .iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        extractor_ids, catalog_ids,
        "threshold EXTRACTORS and library metric_catalog::METRICS disagree on offender ids",
    );
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
    let parsed = split_thresholds_table(&cfg.thresholds).expect("split");
    assert_eq!(parsed.hard.get("cyclomatic"), Some(&15.0));
    assert_eq!(parsed.hard.get("loc.lloc"), Some(&200.0));
    assert!(parsed.soft.is_empty(), "no soft table configured");
}

#[test]
fn split_separates_soft_subtable_from_hard_limits() {
    let toml_src = "[thresholds]\n\
                    cognitive = 25\n\
                    cyclomatic = 15\n\
                    nargs = 7\n\
                    [thresholds.soft]\n\
                    cognitive = 18\n\
                    cyclomatic = \"0.9x\"\n";
    let cfg: ThresholdConfig = toml::from_str(toml_src).expect("parses");
    let parsed = split_thresholds_table(&cfg.thresholds).expect("split");

    // Hard layer keeps every scalar; `soft` is not mistaken for one.
    assert_eq!(parsed.hard.get("cognitive"), Some(&25.0));
    assert_eq!(parsed.hard.get("nargs"), Some(&7.0));
    assert!(!parsed.hard.contains_key("soft"));

    // Absolute and scale-relative soft forms parse into the right variants.
    assert_eq!(
        parsed.soft.get("cognitive"),
        Some(&SoftLimit::Absolute(18.0))
    );
    assert_eq!(parsed.soft.get("cyclomatic"), Some(&SoftLimit::Scale(0.9)));
    // `nargs` has no soft override — it inherits the hard limit at the soft tier.
    assert!(!parsed.soft.contains_key("nargs"));
}

#[test]
fn soft_scale_resolves_against_hard_limit() {
    // `7 * 0.95` rounds to the same readable 6.65 the headroom path emits.
    assert_eq!(SoftLimit::Scale(0.95).resolve("nargs", Some(7.0)), Ok(6.65));
    // Absolute ignores the hard base.
    assert_eq!(
        SoftLimit::Absolute(6.0).resolve("nargs", Some(7.0)),
        Ok(6.0)
    );
    assert_eq!(SoftLimit::Absolute(6.0).resolve("nargs", None), Ok(6.0));
}

#[test]
fn soft_scale_without_hard_base_is_an_error() {
    let err = SoftLimit::Scale(0.9)
        .resolve("cognitive", None)
        .expect_err("scale-relative with no hard base must error");
    assert!(
        err.contains("no hard") && err.contains("cognitive"),
        "error should name the metric and the missing hard limit: {err}"
    );
}

#[test]
fn soft_scale_string_must_end_in_x_and_be_in_range() {
    // Missing `x` suffix.
    let cfg: ThresholdConfig =
        toml::from_str("[thresholds.soft]\ncyclomatic = \"0.9\"\n").expect("parses");
    assert!(split_thresholds_table(&cfg.thresholds).is_err());

    // Out-of-range factor (> 1) — a soft tier looser than hard is rejected.
    let cfg: ThresholdConfig =
        toml::from_str("[thresholds.soft]\ncyclomatic = \"1.5x\"\n").expect("parses");
    assert!(split_thresholds_table(&cfg.thresholds).is_err());

    // Non-numeric, non-string value.
    let cfg: ThresholdConfig =
        toml::from_str("[thresholds.soft]\ncyclomatic = true\n").expect("parses");
    assert!(split_thresholds_table(&cfg.thresholds).is_err());
}

#[test]
fn hard_limit_must_be_numeric() {
    let cfg: ThresholdConfig =
        toml::from_str("[thresholds]\ncyclomatic = \"15\"\n").expect("parses");
    let err = split_thresholds_table(&cfg.thresholds).expect_err("string hard limit must error");
    assert!(
        err.contains("cyclomatic") && err.contains("number"),
        "{err}"
    );
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
        body_hash: None,
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
        body_hash: None,
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
        body_hash: None,
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

// -- qualified symbols (issue #377) -----------------------------------

#[test]
fn evaluate_stamps_qualified_symbols_through_container_chain() {
    // Unit(file) -> Impl(MyStruct) -> Function(do_thing). With a
    // cyclomatic limit of 0 every space's default complexity of 1 trips,
    // so each emitted violation's function slot reveals its qualified
    // symbol: the file collapses to `<file>`, the impl keeps its bare
    // name, and the method is qualified by its container.
    let mut unit = space("src/foo.rs", SpaceKind::Unit, SuppressionScope::default());
    let mut imp = space("MyStruct", SpaceKind::Impl, SuppressionScope::default());
    imp.spaces.push(space(
        "do_thing",
        SpaceKind::Function,
        SuppressionScope::default(),
    ));
    unit.spaces.push(imp);

    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("src/foo.rs"),
        &unit,
        SuppressionPolicy::Ignore,
        &mut out,
    );
    let names: Vec<&str> = out.iter().map(|v| v.function.as_str()).collect();
    assert!(names.contains(&"<file>"), "{names:?}");
    assert!(names.contains(&"MyStruct"), "{names:?}");
    assert!(names.contains(&"MyStruct::do_thing"), "{names:?}");
}

#[test]
fn evaluate_anonymous_space_uses_line_qualified_symbol() {
    // A closure surfaces as the literal `<anonymous>` name; the walk
    // rewrites it to `<anon@L{line}>` so it keeps a stable identity that
    // bakes in the line (the documented anon line-drift degradation).
    let mut closure = space(
        "<anonymous>",
        SpaceKind::Function,
        SuppressionScope::default(),
    );
    closure.start_line = 42;

    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("src/foo.rs"),
        &closure,
        SuppressionPolicy::Ignore,
        &mut out,
    );
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].function, "<anon@L42>");
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
        body_hash: None,
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

#[test]
fn closest_metric_names_suggests_single_typo() {
    // Levenshtein-1 from `cyclomatic`: the only candidate within the
    // cutoff. The suggester must surface exactly one name, not the
    // whole registry.
    let names = known_metric_names();
    let suggestions = crate::threshold_suggestion::closest_names("cyclomatc", &names);
    assert_eq!(suggestions, vec!["cyclomatic"]);
}

#[test]
fn closest_metric_names_suggests_dotted_typo() {
    // A typo in the post-dot portion of a compound metric name must
    // still find the right candidate — verifies the suggester is
    // string-based, not segmented on `.`.
    let names = known_metric_names();
    let suggestions = crate::threshold_suggestion::closest_names("halstead.efort", &names);
    assert_eq!(suggestions, vec!["halstead.effort"]);
}

#[test]
fn closest_metric_names_suggests_truncation() {
    // Truncation case from the issue title: `cyclic` -> `cyclomatic`
    // (4-edit Levenshtein, but a 4-byte shared prefix). The shared-
    // prefix strategy must rescue this so the error is actionable.
    let names = known_metric_names();
    let suggestions = crate::threshold_suggestion::closest_names("cyclic", &names);
    assert!(
        suggestions.contains(&"cyclomatic"),
        "expected `cyclomatic` in {suggestions:?}"
    );
}

#[test]
fn closest_metric_names_returns_empty_for_unrelated_input() {
    // Pure garbage input must produce no suggestion so the existing
    // "unknown metric" error remains the primary signal.
    let names = known_metric_names();
    let suggestions = crate::threshold_suggestion::closest_names("xyznonexistent", &names);
    assert!(suggestions.is_empty(), "{suggestions:?}");
}

#[test]
fn closest_metric_names_returns_empty_for_very_short_input() {
    // A 1-character input falls below the prefix-strategy minimum
    // and has cutoff 0 under the edit-distance strategy, so it must
    // produce no suggestion. Without this, every short candidate
    // would match by trivial substitution.
    let names = known_metric_names();
    assert!(crate::threshold_suggestion::closest_names("z", &names).is_empty());
}

#[test]
fn build_unknown_metric_error_includes_suggestion() {
    let mut raw = BTreeMap::new();
    raw.insert("cyclomatc".to_string(), 1.0);
    let err = ThresholdSet::build(&raw).expect_err("unknown name");
    assert!(err.contains("did you mean"), "{err}");
    assert!(err.contains("cyclomatic"), "{err}");
}

#[test]
fn build_unknown_metric_error_omits_suggestion_for_unrelated_input() {
    let mut raw = BTreeMap::new();
    raw.insert("xyznonexistent".to_string(), 1.0);
    let err = ThresholdSet::build(&raw).expect_err("unknown name");
    assert!(!err.contains("did you mean"), "{err}");
    assert!(err.contains("unknown threshold metric"), "{err}");
}

#[test]
fn edit_distance_with_cutoff_short_circuits_far_apart() {
    // Inputs whose length difference alone exceeds the cutoff must
    // be rejected without doing the full DP — verifies the early
    // exit path. `cutoff + 1` is the documented sentinel.
    let d = crate::threshold_suggestion::edit_distance_with_cutoff("ab", "abcdefghij", 2);
    assert!(d > 2, "{d}");
}

#[test]
fn edit_distance_with_cutoff_handles_equal_strings() {
    assert_eq!(
        crate::threshold_suggestion::edit_distance_with_cutoff("cyclomatic", "cyclomatic", 2),
        0
    );
}
