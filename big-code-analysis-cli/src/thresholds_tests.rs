// Sibling-file unit tests for the threshold engine, wired in via
// `#[path = "thresholds_tests.rs"] mod tests;` so the production
// `thresholds.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use super::*;

/// Locks the threshold-engine extractor vocabulary against
/// `threshold_metric_for_name` so the two stay in sync.
/// If a new threshold extractor is added without a matching
/// suppression mapping (or vice versa), this test fails loudly
/// rather than silently dropping suppression for the new metric.
/// `tokens` is the documented exception: it is never suppressible
/// (see `src/suppression.rs::threshold_metric_for_name`).
#[test]
fn every_extractor_resolves_to_metric_kind_or_is_tokens() {
    for extractor in EXTRACTORS {
        let is_suppressible = threshold_metric_for_name(extractor.name).is_some();
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

/// Issue #514: the bare `bca diff --metric` spelling of a `loc`
/// sub-metric is accepted as an alias and resolves to the dotted
/// extractor, so a name copy-pasted from a `diff` run gates correctly.
#[test]
fn build_accepts_bare_loc_submetric_alias() {
    let mut raw = BTreeMap::new();
    raw.insert("sloc".to_string(), 100.0);
    let set = ThresholdSet::build(&raw).expect("bare loc alias resolves");
    let resolved: Vec<(&str, f64)> = set.iter().collect();
    assert_eq!(resolved, [("loc.sloc", 100.0)]);
}

/// Issue #514: a bare family head with no single threshold scalar is
/// ambiguous and rejected with the concrete candidates, not silently
/// mapped to one sub-metric.
#[test]
fn build_rejects_ambiguous_family_head() {
    let mut raw = BTreeMap::new();
    raw.insert("halstead".to_string(), 1.0);
    let err = ThresholdSet::build(&raw).expect_err("ambiguous head");
    assert!(err.contains("ambiguous"), "{err}");
    assert!(err.contains("halstead.volume"), "{err}");
}

/// The existing dotted spelling keeps working unchanged after aliasing.
#[test]
fn build_still_accepts_dotted_spelling() {
    let mut raw = BTreeMap::new();
    raw.insert("loc.sloc".to_string(), 50.0);
    let set = ThresholdSet::build(&raw).expect("dotted still valid");
    let resolved: Vec<(&str, f64)> = set.iter().collect();
    assert_eq!(resolved, [("loc.sloc", 50.0)]);
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
fn is_valid_scale_ratio_rejects_boundary_and_nan() {
    // Pins the `(0, 1]` contract (issue #709): the open lower bound and
    // the closed upper bound, plus the non-finite cases that all fail
    // both `0.0 < ratio` and `ratio <= 1.0` comparisons.
    //
    // Valid interior and the inclusive upper boundary.
    assert!(is_valid_scale_ratio(0.5));
    assert!(is_valid_scale_ratio(1.0), "1.0 is in range (inclusive)");
    assert!(is_valid_scale_ratio(f64::MIN_POSITIVE), "smallest positive");
    // Zero is excluded (open lower bound), and negatives never qualify.
    assert!(!is_valid_scale_ratio(0.0), "0.0 is excluded (open bound)");
    assert!(!is_valid_scale_ratio(-0.0), "negative zero is excluded");
    assert!(!is_valid_scale_ratio(-0.5));
    // Above the inclusive upper bound is rejected.
    assert!(!is_valid_scale_ratio(1.000_000_1));
    // Non-finite inputs: every comparison against NaN is false, and the
    // infinities fall outside the range, so none are valid.
    assert!(!is_valid_scale_ratio(f64::NAN), "NaN must be rejected");
    assert!(!is_valid_scale_ratio(f64::INFINITY));
    assert!(!is_valid_scale_ratio(f64::NEG_INFINITY));
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
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
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
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
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
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
    };

    // Raw bytes round-trip identically — no lossy substitution.
    assert_eq!(v.path.as_os_str().as_encoded_bytes(), raw_bytes);
    // Display does not panic on non-UTF-8 bytes (uses
    // `Path::display`, which substitutes U+FFFD).
    let rendered = v.to_string();
    assert!(rendered.contains("cyclomatic"), "{rendered}");
}

use big_code_analysis::{Metric, SpaceKind, SuppressionScope};
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

fn only_func_scope(metric: Metric) -> SuppressionScope {
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
        only_func_scope(Metric::Cyclomatic),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "matching function-scoped marker should silence, got {out:?}",
    );
}

#[test]
fn report_suppressed_keeps_marked_violation_tagged() {
    // With `report_suppressed = true`, a marker-covered violation is NOT
    // dropped: it is emitted with `suppressed = true` so the code-scan
    // document can surface it as a suppressed alert. Mirrors
    // `honor_policy_suppresses_matching_function_scope` but flips the
    // report flag — the only difference that should matter.
    let mut out = Vec::new();
    let s = space(
        "noisy",
        SpaceKind::Function,
        only_func_scope(Metric::Cyclomatic),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        true,
        &mut out,
    );
    assert_eq!(out.len(), 1, "report_suppressed should keep the offender");
    assert!(
        out[0].suppressed,
        "kept offender must be tagged suppressed, got {:?}",
        out[0],
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
        only_func_scope(Metric::Cognitive),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        false,
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
        only_func_scope(Metric::Cyclomatic),
    );
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Ignore,
        false,
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
        only_func_scope(Metric::Cyclomatic),
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
        false,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "file-scoped marker should also silence nested fn; got {out:?}",
    );
}

#[test]
fn tokens_threshold_never_suppressed() {
    // `threshold_metric_for_name("tokens")` returns None, so
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
    assert_eq!(threshold_metric_for_name("tokens"), None);

    let extractor = EXTRACTORS
        .iter()
        .find(|e| e.name == "tokens")
        .expect("tokens extractor exists");
    let set = ThresholdSet {
        entries: vec![ResolvedThreshold {
            extractor,
            limit: -0.5,
            lower_is_worse: false,
            scope: metric_scope(extractor.name),
        }],
    };

    let mut out = Vec::new();
    let s = space("noisy", SpaceKind::Function, SuppressionScope::All);
    set.evaluate_with_policy(
        Path::new("fixture.rs"),
        &s,
        SuppressionPolicy::Honor,
        false,
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
    // Unit(file) -> Impl(MyStruct) -> Function(do_thing). cyclomatic is
    // Function-scoped (#969), so with a cyclomatic limit of 0 only the
    // `do_thing` leaf trips — and its violation's function slot reveals the
    // full container-qualified symbol `MyStruct::do_thing`. The `Unit` root
    // and the `MyStruct` impl are skipped by scope, so this asserts the
    // qualified-symbol chain; the `<file>` collapse path is covered by
    // `file_scoped_loc_fires_only_on_unit`.
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
        false,
        &mut out,
    );
    let names: Vec<&str> = out.iter().map(|v| v.function.as_str()).collect();
    assert_eq!(
        names,
        ["MyStruct::do_thing"],
        "only the leaf fires, stamped with its container chain: {names:?}"
    );
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
        false,
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
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
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

/// Analyze a real snippet and return its deepest leaf `FuncSpace` (all
/// child spaces stripped) together with that leaf's computed
/// `mi.original` and `cyclomatic`. Evaluating a leaf means
/// [`ThresholdSet::evaluate_with_policy`] touches exactly one space, so a
/// gate test can assert an exact offender count without the root/child
/// magnitudes interfering. The MI value is derived (no public setter), so
/// the tests bracket the threshold around the value read here (#698).
fn analyzed_leaf() -> (FuncSpace, f64, f64) {
    let mut space = big_code_analysis::analyze(
        big_code_analysis::Source::new(
            big_code_analysis::LANG::Rust,
            b"fn f(x: i32) -> i32 { if x > 0 { x + 1 } else { x - 1 } }",
        ),
        big_code_analysis::MetricsOptions::default(),
    )
    .expect("snippet has a top-level FuncSpace");
    // Descend to the deepest single child and detach it as a standalone
    // leaf so evaluation visits one space only.
    while let Some(child) = space.spaces.pop() {
        space = child;
    }
    space.spaces.clear();
    let mi = space.metrics.mi.original();
    let cyclo = space.metrics.cyclomatic.cyclomatic() as f64;
    (space, mi, cyclo)
}

#[test]
fn mi_low_value_is_flagged_high_value_is_not() {
    // `mi.*` is lower-is-worse: a value BELOW the limit is the violation.
    // A limit set ABOVE the observed MI must flag it; a limit set BELOW
    // must not. The pre-#698 `value <= limit` gate did the exact
    // opposite (flagged healthy/high MI, ignored unhealthy/low MI).
    let (space, mi, _) = analyzed_leaf();
    assert!(mi.is_finite(), "fixture MI must be finite, got {mi}");

    // Limit comfortably above the value -> value falls below -> flagged.
    let high_limit = mi + 50.0;
    let mut flagged = Vec::new();
    threshold_set("mi.original", high_limit).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut flagged,
    );
    assert_eq!(
        flagged.len(),
        1,
        "MI {mi} below limit {high_limit} must be flagged, got {flagged:?}"
    );
    assert_eq!(flagged[0].metric, "mi.original");
    assert!(flagged[0].lower_is_worse, "mi offender must carry the flag");

    // Limit at/below the value (clamped to a valid non-negative limit)
    // -> value is healthy -> not flagged.
    let low_limit = (mi - 50.0).max(0.0);
    let mut clean = Vec::new();
    threshold_set("mi.original", low_limit).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut clean,
    );
    assert!(
        clean.is_empty(),
        "MI {mi} at/above limit {low_limit} must NOT be flagged, got {clean:?}"
    );

    // Boundary: a value EXACTLY at the limit is acceptable (not a
    // violation) for lower-is-worse too — the gate is `value < limit`,
    // not `<=`, mirroring the `>`/healthy semantics of higher-is-worse.
    let mut at_limit = Vec::new();
    threshold_set("mi.original", mi).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut at_limit,
    );
    assert!(
        at_limit.is_empty(),
        "MI {mi} exactly at limit {mi} must NOT be flagged, got {at_limit:?}"
    );
}

#[test]
fn higher_is_worse_metric_keeps_above_limit_gate() {
    // The direction-aware gate must not disturb higher-is-worse metrics:
    // cyclomatic still fires only when the value EXCEEDS the limit.
    let (space, _, cyclo) = analyzed_leaf();
    assert!(cyclo >= 1.0, "fixture cyclomatic must be >= 1, got {cyclo}");

    // Limit below the value -> exceeds -> flagged.
    let mut flagged = Vec::new();
    threshold_set("cyclomatic", cyclo - 0.5).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut flagged,
    );
    assert_eq!(flagged.len(), 1, "cyclomatic above limit must flag");
    assert!(!flagged[0].lower_is_worse);

    // Limit above the value -> within budget -> not flagged.
    let mut clean = Vec::new();
    threshold_set("cyclomatic", cyclo + 5.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut clean,
    );
    assert!(clean.is_empty(), "cyclomatic below limit must not flag");
}

#[test]
fn mi_ratio_inverts_so_lower_value_ranks_worse() {
    // `Violation::ratio` normalizes so a bigger ratio is always worse.
    // For MI (lower-is-worse) that means `limit / value`: MI 5 against
    // limit 50 (10x) must outrank MI 45 against limit 50 (~1.1x).
    let severe = Violation {
        path: PathBuf::from("a.rs"),
        start_line: 1,
        end_line: 2,
        function: "f".into(),
        metric: "mi.original",
        value: 5.0,
        limit: 50.0,
        lower_is_worse: true,
        body_hash: None,
        suppressed: false,
    };
    let mild = Violation {
        value: 45.0,
        ..severe.clone()
    };
    assert!(
        severe.ratio() > mild.ratio(),
        "a lower MI must produce a larger breach ratio: {} vs {}",
        severe.ratio(),
        mild.ratio()
    );
    assert!(
        Violation::pick_worst(&[&mild, &severe])
            .is_some_and(|w| (w.value - 5.0).abs() < f64::EPSILON),
        "pick_worst must rank the lower MI as worse"
    );
}

// --- Per-metric threshold scope (#969) ---------------------------------
//
// `bca check` walked every `FuncSpace`, so a subtree-sum metric read at
// the file-level `Unit` root or a container (a sum across many functions)
// tripped a per-function limit on any non-trivial file or `impl`. Scope
// pins each metric to the kind it measures: `loc.*` to the `Unit` root,
// the per-function metrics to `SpaceKind::Function` leaves, and the OO
// size metrics (nom/wmc/npm/npa) to container spaces. These tests pin all
// three directions.

/// Parse a small multi-line Rust snippet into its full space tree: a
/// `SpaceKind::Unit` root with a nested function child, both carrying real
/// `loc.*` and `mi.*` values (neither has a public setter). Used by the
/// scope tests that need a genuine file-aggregate-vs-per-function split.
fn analyzed_tree() -> FuncSpace {
    big_code_analysis::analyze(
        big_code_analysis::Source::new(
            big_code_analysis::LANG::Rust,
            b"fn f(x: i32) -> i32 {\n    if x > 0 {\n        x + 1\n    } else {\n        x - 1\n    }\n}\n",
        ),
        big_code_analysis::MetricsOptions::default(),
    )
    .expect("snippet has a top-level FuncSpace")
}

/// Parse a Rust `struct` plus an `impl` of two methods. The `impl` space's
/// `nom.total()` is 2 (its two methods), giving the Container-scope test a
/// real container metric to gate (no public setter exists for `nom`).
fn analyzed_impl_tree() -> FuncSpace {
    big_code_analysis::analyze(
        big_code_analysis::Source::new(
            big_code_analysis::LANG::Rust,
            b"struct S;\nimpl S {\n    fn a(&self) {}\n    fn b(&self) {}\n}\n",
        ),
        big_code_analysis::MetricsOptions::default(),
    )
    .expect("snippet has a top-level FuncSpace")
}

/// Attach `child` under a fresh `SpaceKind::Unit` root and return the root.
/// The root keeps `CodeMetrics::default()` (cyclomatic `1.0`), so a
/// `cyclomatic = 0` threshold fired on it under the pre-#969 file-aggregate
/// behavior — letting the scope tests assert it no longer does.
fn unit_with_child(child: FuncSpace) -> FuncSpace {
    let mut root = space("<file>", SpaceKind::Unit, SuppressionScope::default());
    root.spaces.push(child);
    root
}

#[test]
fn function_scoped_metric_skips_unit_root() {
    // cyclomatic is Function-scoped: the file-level Unit root is never
    // gated, even though its default cyclomatic of 1.0 exceeds a `0` limit.
    // Pre-#969 this fired a spurious `<file>` violation.
    let root = space("<file>", SpaceKind::Unit, SuppressionScope::default());
    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "Unit root must not be gated for a Function-scoped metric, got {out:?}"
    );
}

#[test]
fn function_scoped_metric_fires_on_nested_function() {
    let child = space("inner", SpaceKind::Function, SuppressionScope::default());
    let root = unit_with_child(child);
    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert_eq!(
        out.len(),
        1,
        "exactly the nested function fires, got {out:?}"
    );
    assert_eq!(out[0].function, "inner");
    assert_eq!(out[0].metric, "cyclomatic");
}

#[test]
fn function_scoped_metric_skips_container() {
    // Function scope is `SpaceKind::Function` only: a container (impl /
    // class) is never gated for a per-function metric, so an `impl`'s
    // method-summed value cannot fire as if it were one function's.
    let container = space("MyType", SpaceKind::Impl, SuppressionScope::default());
    let root = unit_with_child(container);
    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "container must not be gated for a Function-scoped metric, got {out:?}"
    );
}

#[test]
fn container_scoped_metric_fires_on_container_only() {
    // nom is Container-scoped: it gates the `impl` (methods-per-container),
    // not the file root (a file-wide method sum) and not the leaf methods.
    let root = analyzed_impl_tree();
    let mut out = Vec::new();
    // Limit 1 so the impl's `nom` of 2 trips while the leaf methods
    // (`nom` 0/1) and the Unit root are not flagged.
    threshold_set("nom", 1.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert_eq!(
        out.len(),
        1,
        "only the impl container fires for nom, got {out:?}"
    );
    assert_ne!(out[0].function, "<file>", "the Unit root must not be gated");
    assert_eq!(out[0].metric, "nom");
}

#[test]
fn file_scoped_loc_fires_only_on_unit() {
    // loc.sloc is File-scoped: it gates the whole-file Unit root, never the
    // nested function — even though both spans exceed the limit.
    let root = analyzed_tree();
    let unit_sloc = root.metrics.loc.sloc();
    assert!(
        unit_sloc > 2,
        "fixture Unit sloc must exceed the test limit, got {unit_sloc}"
    );
    let mut out = Vec::new();
    threshold_set("loc.sloc", 2.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert_eq!(
        out.len(),
        1,
        "only the Unit root fires for loc.sloc, got {out:?}"
    );
    assert_eq!(out[0].function, "<file>");
    assert_eq!(out[0].metric, "loc.sloc");
}

#[test]
fn scope_guard_runs_before_suppression() {
    // The scope guard is independent of suppression: even under `Ignore`
    // (which honors no markers) the Function-scoped metric does not fire on
    // the Unit root, while the nested function still does.
    let child = space("inner", SpaceKind::Function, SuppressionScope::All);
    let mut root = space("<file>", SpaceKind::Unit, SuppressionScope::All);
    root.spaces.push(child);
    let mut out = Vec::new();
    threshold_set("cyclomatic", 0.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Ignore,
        false,
        &mut out,
    );
    assert_eq!(
        out.len(),
        1,
        "Unit skipped by scope; function fires under Ignore: {out:?}"
    );
    assert_eq!(out[0].function, "inner");
}

#[test]
fn mi_lower_is_worse_skips_unit_root() {
    // `mi.*` is lower-is-worse AND Function-scoped: a limit above the Unit's
    // MI would breach it if the file root were gated, but the scope guard
    // runs before the direction-aware breach test, so the `<file>` aggregate
    // never appears.
    let root = analyzed_tree();
    let unit_mi = root.metrics.mi.original();
    assert!(
        unit_mi.is_finite(),
        "fixture Unit MI must be finite, got {unit_mi}"
    );
    let mut out = Vec::new();
    threshold_set("mi.original", unit_mi + 50.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &root,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert!(
        !out.iter().any(|v| v.function == "<file>"),
        "the file aggregate must not be flagged for a Function-scoped mi.*: {out:?}"
    );
}

// --- #1113: the metric family each extractor reads ------------------

/// The `metric` field must agree with `threshold_metric_for_name`
/// wherever that helper commits to an answer, so the two mappings cannot
/// drift as new extractors land.
///
/// `tokens` is the one deliberate divergence and the reason the field
/// exists at all: the helper answers a *suppression* question and
/// returns `None` there, but `tokens` is a real threshold whose family
/// must still be computed. Asserting it explicitly means deleting the
/// divergence (by "simplifying" `metric` away into a call to the helper)
/// fails here rather than silently disarming the `tokens` gate.
#[test]
fn extractor_metric_family_agrees_with_the_suppression_mapping() {
    for extractor in EXTRACTORS {
        match threshold_metric_for_name(extractor.name) {
            Some(family) => assert_eq!(
                extractor.metric, family,
                "extractor `{}` declares {:?} but resolves to {family:?}",
                extractor.name, extractor.metric,
            ),
            None => assert_eq!(
                extractor.name, "tokens",
                "extractor `{}` has no suppression mapping; only `tokens` may",
                extractor.name,
            ),
        }
    }
    // The divergence itself: the helper declines, the registry does not.
    let tokens = lookup_extractor("tokens").expect("tokens is a threshold");
    assert!(threshold_metric_for_name("tokens").is_none());
    assert_eq!(tokens.metric, Metric::Tokens);
}

/// `selected_metrics` collapses the several dotted names that share one
/// family down to a single entry, and keeps registry order.
#[test]
fn selected_metrics_dedupes_families_sharing_a_name_prefix() {
    let raw = BTreeMap::from([
        ("halstead.volume".to_owned(), 1.0),
        ("halstead.effort".to_owned(), 1.0),
        ("loc.sloc".to_owned(), 1.0),
        ("loc.ploc".to_owned(), 1.0),
        ("cyclomatic".to_owned(), 1.0),
    ]);
    let set = ThresholdSet::build(&raw).expect("builds");

    assert_eq!(
        set.selected_metrics(),
        vec![Metric::Cyclomatic, Metric::Halstead, Metric::Loc],
        "five thresholds span exactly three families, in registry order",
    );
}

/// Every extractor's declared `metric` must be *sufficient* to compute
/// the accessor it reads (#1113).
///
/// This is the assertion that actually protects the gate. Narrowing the
/// check walk means each threshold is evaluated against a `CodeMetrics`
/// built with `with_only(&[that one family])`; if the declared family is
/// wrong, or if a derived metric's dependencies are not resolved, the
/// accessor reads a default-constructed `Stats` and silently returns
/// zero instead of erroring. So compare the narrow computation against
/// the full suite at every space in the tree — an exact match at each
/// one is the only outcome that means "this family was enough".
///
/// The equality alone could pass vacuously if a value were zero on both
/// sides, so each extractor must also read non-zero at *some* space.
/// That is why the fixture is Java with a populated class: `wmc`, `npm`,
/// `npa`, and `loc.cloc` are zero inside a method body and only become
/// observable at the class or file space, while `cognitive`, `abc`, and
/// the branch-driven families are zero at the file space and only
/// observable inside the method.
#[test]
fn every_extractor_metric_family_suffices_to_compute_its_accessor() {
    use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

    /// Class with public members, branches, arguments, comments, blank
    /// lines and several exits, so no extractor reads zero everywhere.
    const JAVA: &str = r#"
// A class with public members, branches, args and several exits.
public class Sample {
    public int width;
    public int height;

    public String classify(int n, int m, boolean flag, String tag, int limit) {
        if (n < 0) {
            return "neg";
        } else if (n == 0 && flag) {
            return "zero";
        }

        for (int i = 0; i < m; i++) {
            if (i > limit) {
                return tag;
            }
        }
        return "other";
    }

    public int area() {
        return width * height;
    }
}
"#;

    /// Walk two structurally identical trees in lockstep, applying
    /// `visit` to each `(full, narrow)` space pair.
    fn zip_spaces(
        full: &FuncSpace,
        narrow: &FuncSpace,
        visit: &mut impl FnMut(&FuncSpace, &FuncSpace),
    ) {
        visit(full, narrow);
        assert_eq!(
            full.spaces.len(),
            narrow.spaces.len(),
            "metric selection must not change the shape of the space tree",
        );
        for (f, n) in full.spaces.iter().zip(&narrow.spaces) {
            zip_spaces(f, n, visit);
        }
    }

    let ast = Ast::parse(Source::new(LANG::Java, JAVA.as_bytes())).expect("fixture parses");
    let full = ast
        .metrics(MetricsOptions::default())
        .expect("full metrics");

    for extractor in EXTRACTORS {
        let narrow = ast
            .metrics(MetricsOptions::default().with_only(&[extractor.metric]))
            .expect("narrowed metrics");

        let mut any_non_zero = false;
        zip_spaces(&full, &narrow, &mut |f, n| {
            let expected = (extractor.extract)(&f.metrics);
            let actual = (extractor.extract)(&n.metrics);
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "`{}` computed with only {:?} disagrees with the full suite at space {:?}",
                extractor.name,
                extractor.metric,
                f.name,
            );
            any_non_zero |= expected != 0.0;
        });
        assert!(
            any_non_zero,
            "`{}` is zero at every space in the fixture, so the comparison above proves nothing",
            extractor.name,
        );
    }
}
