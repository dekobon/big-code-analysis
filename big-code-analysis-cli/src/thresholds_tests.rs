// Sibling-file unit tests for the threshold engine, wired in via
// `#[path = "thresholds_tests.rs"] mod tests;` so the production
// `thresholds.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use super::*;
use crate::threshold_soft::is_valid_scale_ratio;
use big_code_analysis::metric_catalog::Direction;

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
fn parse_cli_threshold_accepts_a_negative_mi_floor() {
    // `mi.original` and `mi.sei` are interval-scale and routinely
    // negative, so a floor below zero is a real gate (#1621).
    for metric in ["mi.original", "mi.sei"] {
        let (name, limit) =
            parse_cli_threshold(&format!("{metric}=-50")).expect("negative MI floor parses");
        assert_eq!((name.as_str(), limit), (metric, -50.0));
    }
}

#[test]
fn parse_cli_threshold_canonicalises_before_checking_the_limit() {
    // `sloc` is an alias for `loc.sloc`, a count: the limit check must
    // see the canonical id, or the alias would slip past the catalog.
    let err = parse_cli_threshold("sloc=-1").expect_err("negative count limit");
    assert!(err.contains("non-negative"), "{err}");
}

#[test]
fn parse_cli_threshold_leaves_an_unknown_name_to_the_resolver() {
    // An unknown name is reported one layer down, with its did-you-mean
    // list; the parser checks only that the number is finite.
    assert_eq!(
        parse_cli_threshold("not_a_metric=-1"),
        Ok(("not_a_metric".to_owned(), -1.0))
    );
    let err = parse_cli_threshold("not_a_metric=inf").expect_err("infinite limit");
    assert!(err.contains("finite"), "{err}");
}

#[test]
fn parse_cli_threshold_accepts_out_of_domain_mi_visual_studio() {
    // `mi.visual_studio` is clamped to [0, 100]. A limit of 0 can never
    // fire and 101 fires on everything; both are advisories, not errors,
    // because they parse in 2.x (STABILITY.md "CLI argument grammar").
    for (raw, limit) in [
        ("mi.visual_studio=0", 0.0),
        ("mi.visual_studio=101", 101.0),
        ("mi.visual_studio=-1", -1.0),
    ] {
        assert_eq!(
            parse_cli_threshold(raw),
            Ok(("mi.visual_studio".to_owned(), limit)),
            "{raw} must still parse"
        );
    }
}

#[test]
fn parse_min_change_rejects_non_finite_and_keeps_negative() {
    assert_eq!(parse_min_change("0"), Ok(0.0));
    assert_eq!(parse_min_change(" 2.5 "), Ok(2.5));
    // A negative floor parses — releases before 2.4 took it — and
    // `run_command_diff` warns that it reads as `0`.
    assert_eq!(parse_min_change("-1"), Ok(-1.0));
    for raw in ["nan", "inf", "-inf"] {
        let err = parse_min_change(raw).expect_err(raw);
        assert!(err.contains("finite number"), "{raw}: {err}");
    }
    assert!(
        parse_min_change("x")
            .unwrap_err()
            .contains("invalid min-change")
    );
}

#[test]
fn build_accepts_a_negative_mi_floor_but_not_a_negative_count() {
    // A floor of −50 builds and passes an MI above it. A function deep
    // enough to fall below it is too large for a unit fixture; the
    // breaching side is `a_negative_mi_floor_parses_and_gates` in
    // `tests/check/check_baseline.rs`.
    let (space, mi, _) = analyzed_leaf();
    let mut out = Vec::new();
    threshold_set("mi.original", -50.0).evaluate_with_policy(
        Path::new("fixture.rs"),
        &space,
        SuppressionPolicy::Honor,
        false,
        &mut out,
    );
    assert!(out.is_empty(), "MI {mi} is above a floor of -50: {out:?}");
    let err = ThresholdSet::build(&BTreeMap::from([("cyclomatic".to_string(), -1.0)]))
        .expect_err("a negative count limit is still rejected");
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

/// Parse a `[thresholds]` table the way both the manifest and
/// `--config` do, so alias/duplicate assertions exercise the real parse
/// boundary rather than a hand-built map.
fn split(toml_src: &str) -> Result<ParsedThresholds, String> {
    let cfg: ThresholdConfig = toml::from_str(toml_src).expect("parses as TOML");
    split_thresholds_table(&cfg.thresholds)
}

/// Issue #514: the bare `bca diff --metric` spelling of a `loc`
/// sub-metric is accepted as an alias and resolves to the dotted
/// extractor, so a name copy-pasted from a `diff` run gates correctly.
///
/// Issue #1165 moved the resolution from `ThresholdSet::build_tiered` to
/// the parse boundary, which is why this asserts on the parsed key and
/// not only on the built set: the key is what every later layer merges
/// against.
#[test]
fn parse_canonicalises_bare_loc_submetric_alias() {
    let parsed = split("[thresholds]\nsloc = 100\n").expect("bare loc alias resolves");
    assert_eq!(parsed.hard.keys().collect::<Vec<_>>(), ["loc.sloc"]);
    let set = ThresholdSet::build(&parsed.hard).expect("canonical key builds");
    let resolved: Vec<(&str, f64)> = set.iter().collect();
    assert_eq!(resolved, [("loc.sloc", 100.0)]);
}

/// The soft tier canonicalises too (#1165). Keyed by the raw spelling, a
/// `[thresholds.soft] sloc = "0.9x"` would find no hard limit named
/// `sloc` to scale and die with "no hard [thresholds] limit exists".
#[test]
fn parse_canonicalises_soft_subtable_alias() {
    let parsed = split("[thresholds]\n\"loc.sloc\" = 100\n[thresholds.soft]\nsloc = \"0.9x\"\n")
        .expect("bare loc alias resolves in the soft table");
    assert_eq!(parsed.soft.get("loc.sloc"), Some(&SoftLimit::Scale(0.9)));
    assert_eq!(
        parsed
            .soft
            .get("loc.sloc")
            .expect("soft entry")
            .resolve("loc.sloc", parsed.hard.get("loc.sloc").copied()),
        Ok(90.0),
        "the canonical soft key must find the canonical hard limit to scale",
    );
}

/// Issue #1165: a table that names one metric under two spellings is
/// rejected. Before the fix the merge kept both and gated the same
/// extractor twice; silently keeping whichever key sorts last would be
/// the same surprise in a quieter form.
///
/// Both key orders are covered because the table is a `BTreeMap`: which
/// spelling is *seen* second is decided by sort order, not by the order
/// the user typed. `loc.ploc` sorts before `ploc`, but `blank` sorts
/// before `loc.blank` — so the dotted id is the reported one in the
/// second case, and the message has to read correctly either way.
#[test]
fn parse_rejects_one_metric_under_two_spellings() {
    for (src, canonical) in [
        ("[thresholds]\n\"loc.ploc\" = 6\nploc = 100\n", "loc.ploc"),
        // `blank` sorts before `loc.blank`, so here the *dotted* id is
        // the key reported — the direction whose message would otherwise
        // read `"loc.blank": "loc.blank" is already set`, naming one key
        // twice and the other not at all.
        (
            "[thresholds]\nblank = 6\n\"loc.blank\" = 100\n",
            "loc.blank",
        ),
    ] {
        let err = split(src).expect_err("two spellings of one metric");
        assert!(err.contains("[thresholds]"), "{err}");
        assert!(err.contains(canonical), "{err}");
        assert!(
            err.contains("are one metric, so set it once"),
            "the message explains the alias relation in both directions: {err}"
        );
    }
}

/// Issue #514: a bare family head with no single threshold scalar is
/// ambiguous and rejected with the concrete candidates, not silently
/// mapped to one sub-metric. Since #1165 the rejection happens at the
/// parse boundary, attributed to the table it came from.
#[test]
fn parse_rejects_ambiguous_family_head() {
    let err = split("[thresholds]\nhalstead = 1\n").expect_err("ambiguous head");
    assert!(err.contains("[thresholds]"), "{err}");
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
        threshold: test_threshold("cyclomatic", 15.0),
        hard_limit: Some(test_threshold("cyclomatic", 15.0)),
        body_hash: None,
        suppressed: false,
        nargs_split: None,
    };
    assert_eq!(
        v.to_string(),
        "src/foo.rs:10-25: do_stuff: cyclomatic = 17 (limit 15)"
    );
}

#[test]
fn violation_display_keeps_fractional_precision() {
    let v = ranked("halstead.volume", 12.5, 10.0);
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
        ..sample_violation()
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
    // A real parsed function carries a non-zero token count, so a `tokens`
    // limit of 0 breaches it; the function is then marked fully
    // suppressed.
    assert_eq!(threshold_metric_for_name("tokens"), None);

    let mut s = analyzed_tree()
        .spaces
        .pop()
        .expect("the snippet has a function space");
    s.suppressed = SuppressionScope::All;
    assert!(
        s.metrics.tokens.tokens_sum() > 0,
        "fixture must have tokens"
    );
    let set = threshold_set("tokens", 0.0);

    let mut out = Vec::new();
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
        threshold: test_threshold("cyclomatic", 10.0),
        hard_limit: Some(test_threshold("cyclomatic", 10.0)),
        body_hash: None,
        suppressed: false,
        nargs_split: None,
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
fn render_regressed_lower_is_worse_tag_is_well_formed() {
    // Regression for #1242. `Baseline::classify` is direction-aware, so
    // for the lower-is-worse `mi.*` family it emits `Regressed` when the
    // value *drops* below the recorded one. The tag formatter assumed
    // the opposite direction and prefixed a literal `+` to what was then
    // a negative percentage, rendering `[regr +-29%]` — a shape no log
    // parser anchored on `[regr +N%]` can read. The magnitude is now
    // measured in the metric's own direction, so it stays "worse by".
    // Values are the issue's reproducer: `mi.original` recorded at
    // 142.75 dropping to 101.03 is worse by 41.72 points (#1621 made the
    // `mi.*` tag points rather than a percentage).
    let mut v = sample_violation();
    v.metric = "mi.original";
    v.threshold = test_threshold("mi.original", 160.0);
    v.value = 101.03;
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 142.75 }));
    assert!(out.starts_with("[regr +41.7 pts] "), "got: {out}");
    assert!(!out.contains("+-"), "double-signed amount: {out}");
}

#[test]
fn render_regressed_direction_is_not_a_blanket_sign_flip() {
    // The companion to the test above: fixing the `mi.*` direction must
    // not invert the higher-is-worse one. A cyclomatic rise from 20 to
    // 30 is still `+50%`, not `-50%` — pinned here because a sign flip
    // in the wrong branch would satisfy the test above on its own
    // (#1242).
    let v = sample_violation();
    assert_eq!(
        v.threshold.info().direction,
        Direction::HigherIsWorse,
        "the fixture must be higher-is-worse"
    );
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded: 20.0 }));
    assert!(out.starts_with("[regr +50%] "), "got: {out}");
}

/// The tag `format_regressed_tag` renders for `metric` moving from
/// `recorded` to `value`.
fn regressed_tag(metric: &'static str, recorded: f64, value: f64) -> String {
    let v = Violation {
        metric,
        value,
        threshold: test_threshold(metric, 0.0),
        ..sample_violation()
    };
    let out = render_violation_line(&v, Some(&Coverage::Regressed { recorded }));
    let (tag, _) = out.split_once("] ").expect("a tagged line");
    format!("{tag}]")
}

#[test]
fn regressed_tag_is_points_for_mi_and_percent_for_counts() {
    // expected: mi.* moves are the plain point difference in the bad
    // direction (−10 → −20 and 0 → −12 are worse by 10 and 12); a zero
    // MI record is an ordinary point on an interval scale, so it never
    // reads `from 0`. Counts keep the percentage of the record: 10 → 20
    // is +100%, a zero record is `from 0`, and 1 → 1e6 caps.
    for (metric, recorded, value, want) in [
        // `mi.original` and `mi.sei` are unclamped, so a baseline can
        // record them below zero (#1611). Dividing by the signed record
        // printed the double-signed `[regr +-100%]` for this row.
        ("mi.original", -10.0, -20.0, "[regr +10.0 pts]"),
        ("mi.original", 0.0, -12.0, "[regr +12.0 pts]"),
        ("mi.sei", 40.0, 27.66, "[regr +12.3 pts]"),
        // The `>9999` cap belongs to the percentage of a count; a points
        // amount is printed as measured.
        ("mi.sei", -1.0, -1000.0, "[regr +999.0 pts]"),
        // #1621's reproducer R1: a 0.0107-point move, which a percentage
        // of the record rendered as `[regr +1069%]`.
        (
            "mi.sei",
            0.001,
            -0.009_693_594_449_785_792,
            "[regr +0.0 pts]",
        ),
        ("cyclomatic", 10.0, 20.0, "[regr +100%]"),
        ("cyclomatic", 0.0, 5.0, "[regr from 0]"),
        ("halstead.effort", 1.0, 1_000_000.0, "[regr +>9999%]"),
    ] {
        assert_eq!(
            regressed_tag(metric, recorded, value),
            want,
            "{metric}: {recorded} -> {value}"
        );
    }
}

/// The `[regr ...]` tag exactly as it rendered before #1621, for a
/// higher-is-worse count.
fn pre_1621_count_tag(recorded: f64, value: f64) -> String {
    if value.is_nan() {
        return "[regr NaN]".to_string();
    }
    if recorded == 0.0 {
        return "[regr from 0]".to_string();
    }
    let pct = ((value - recorded) / recorded * 100.0).round();
    if pct > 9999.0 {
        return "[regr +>9999%]".to_string();
    }
    format!("[regr +{pct:.0}%]")
}

#[test]
fn regressed_tag_for_counts_is_byte_identical_to_pre_1621() {
    // Moving the tag onto `Threshold::change` must not move a single
    // byte for a ratio-scale metric: only `mi.*` changed unit.
    let pairs = [
        (20.0, 30.0),
        (100.0, 100.5),
        (100.0, 10_099.0),
        (100.0, 10_100.0),
        (3.0, 4.0),
        (7.0, 7.000_001),
        (0.0, 1.0),
        (1.0, 1_000_000.0),
        (5.0, f64::NAN),
        (49_999.5, 61_984.213_363_755_734),
    ];
    for metric in [
        "cyclomatic",
        "cognitive",
        "halstead.effort",
        "loc.sloc",
        "nom",
    ] {
        for (recorded, value) in pairs {
            assert_eq!(
                regressed_tag(metric, recorded, value),
                pre_1621_count_tag(recorded, value),
                "{metric}: {recorded} -> {value}"
            );
        }
    }
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
    assert_eq!(
        flagged[0].threshold.info().direction,
        Direction::LowerIsWorse,
        "mi offender must carry the lower-is-worse direction"
    );

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
fn an_mi_floor_skips_a_space_with_no_code_to_measure() {
    // A bodiless Ruby block is a function space with zero Halstead
    // volume, so every `mi.*` variant reads the `0.0` sentinel
    // (`mi::Stats::is_empty`) and must not breach a floor that the
    // file's one measured function, on row 3, does breach (#1621).
    let root = big_code_analysis::analyze(
        big_code_analysis::Source::new(
            big_code_analysis::LANG::Ruby,
            b"x = lambda {}\n[1, 2].each { }\ndef f(a)\n  a + 1\nend\n",
        ),
        big_code_analysis::MetricsOptions::default(),
    )
    .expect("snippet has a top-level FuncSpace");
    let empties = root
        .spaces
        .iter()
        .filter(|s| s.kind == SpaceKind::Function && s.metrics.mi.is_empty())
        .count();
    assert_eq!(empties, 2, "both blocks must be MI-empty: {root:?}");
    for (metric, floor) in [
        ("mi.original", 171.0),
        ("mi.sei", 171.0),
        ("mi.visual_studio", 100.0),
    ] {
        let mut out = Vec::new();
        threshold_set(metric, floor).evaluate_with_policy(
            Path::new("fixture.rb"),
            &root,
            SuppressionPolicy::Honor,
            false,
            &mut out,
        );
        let rows: Vec<usize> = out.iter().map(|v| v.start_line).collect();
        assert_eq!(rows, [3], "{metric}={floor}: {out:?}");
    }
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
    assert_eq!(
        flagged[0].threshold.info().direction,
        Direction::HigherIsWorse
    );

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

// -- Severity ranking (#1617, #1621) -------------------------------------
//
// Before #1621 the key was a ratio, `limit / value` for `mi.*`, which
// saturates to infinity for every MI at or below zero; the tie then broke
// on the raw value and kept the *larger* one — for a lower-is-worse
// metric, the milder breach. The key is now the catalog's
// `Threshold::cmp_breach`.

/// A violation of `metric` at `value` against `limit`, its direction and
/// scale taken from the catalog so a fixture cannot mislabel either.
fn ranked(metric: &'static str, value: f64, limit: f64) -> Violation {
    Violation {
        metric,
        value,
        threshold: test_threshold(metric, limit),
        hard_limit: Some(test_threshold(metric, limit)),
        ..sample_violation()
    }
}

/// Asserts that `a` is a strictly worse breach than `b`.
#[track_caller]
fn assert_worse(a: &Violation, b: &Violation) {
    assert_eq!(
        a.cmp_severity(b),
        std::cmp::Ordering::Greater,
        "{a:?} vs {b:?}"
    );
}

#[test]
fn mi_lower_value_ranks_worse() {
    // MI 5 against a floor of 50 must outrank MI 45 against the same
    // floor, whichever order `pick_worst` meets them in.
    let severe = ranked("mi.original", 5.0, 50.0);
    let mild = ranked("mi.original", 45.0, 50.0);
    for vs in [[&mild, &severe], [&severe, &mild]] {
        assert_eq!(Violation::pick_worst(&vs).map(|w| w.value), Some(5.0));
    }
}

#[test]
fn pick_worst_names_the_more_negative_mi_breach() {
    // #1617's reproducer: one function's `mi.original` and `mi.sei`,
    // both gated at 20. −76.55 sits 96.55 below the limit and −14.77
    // only 34.77, so `mi.sei` is the worse breach — and the one the
    // metric-name tie-break would *not* pick. The second pair shares a
    // metric, so no later key can rescue a tie; both input orders run
    // because `max_by` returns the later of two equal elements.
    let pairs = [
        (
            ranked("mi.original", -14.77, 20.0),
            ranked("mi.sei", -76.55, 20.0),
        ),
        (
            ranked("mi.sei", -14.77, 20.0),
            ranked("mi.sei", -76.55, 20.0),
        ),
    ];
    for (mild, deep) in &pairs {
        for vs in [[mild, deep], [deep, mild]] {
            let worst = Violation::pick_worst(&vs).map(|w| (w.metric, w.value));
            assert_eq!(worst, Some((deep.metric, deep.value)), "from {vs:?}");
        }
    }
}

#[test]
fn mi_severity_grows_monotonically_through_zero() {
    // For one limit, each step down is a strictly worse breach, on both
    // sides of zero and across it; a ratio saturated at zero.
    let descending = [19.0, 10.0, 1.0, 0.001, 0.0, -0.001, -14.77, -76.55];
    for metric in ["mi.original", "mi.sei"] {
        for &[milder, worse] in descending.array_windows() {
            let (milder, worse) = (ranked(metric, milder, 20.0), ranked(metric, worse, 20.0));
            assert_worse(&worse, &milder);
            assert_eq!(milder.cmp_severity(&worse), std::cmp::Ordering::Less);
        }
    }
}

#[test]
fn cross_metric_ranking_reads_mi_in_index_units() {
    // #1621's decided ordering: cognitive 200 against 15 is a 13.3x
    // breach; mi.original −14.77 against 20 is 34.77 points, about two
    // Visual Studio bands (1 + 34.77 / 17.1 ≈ 3.03). The ratio key ranked
    // the MI infinitely bad, and named it worst.
    let cognitive = ranked("cognitive", 200.0, 15.0);
    let mi = ranked("mi.original", -14.77, 20.0);
    for vs in [[&cognitive, &mi], [&mi, &cognitive]] {
        assert_eq!(
            Violation::pick_worst(&vs).map(|w| w.metric),
            Some("cognitive")
        );
    }
    // A deep enough MI does outrank it: −250 is 270 points, 16.8 bands.
    let deep = ranked("mi.original", -250.0, 20.0);
    assert_worse(&deep, &cognitive);
}

#[test]
fn a_zero_limit_breach_saturates_and_breaks_ties_on_excess() {
    // A zero count limit has no ratio; it ranks above every finite
    // severity, and two such breaches order by how far past zero each
    // sits.
    let shallow = ranked("cyclomatic", 3.0, 0.0);
    let deep = ranked("cyclomatic", 50.0, 0.0);
    let finite = ranked("cyclomatic", 1000.0, 10.0);
    let mi = ranked("mi.original", -1000.0, 20.0);
    assert_worse(&shallow, &finite);
    assert_worse(&shallow, &mi);
    assert_worse(&deep, &shallow);
}

#[test]
fn equal_ratio_ties_still_favour_the_larger_value() {
    // Regression guard for every count tie the raw value broke before
    // #1617. At a fixed ratio both the excess and the value grow with
    // the limit, so these orders must not move.
    assert_worse(
        &ranked("cyclomatic", 30.0, 10.0),
        &ranked("cyclomatic", 11.0, 10.0),
    );
    // Both 2x: 30 sits 15 over its limit, 20 only 10.
    assert_worse(
        &ranked("cyclomatic", 30.0, 15.0),
        &ranked("cyclomatic", 20.0, 10.0),
    );
    assert_worse(
        &ranked("cyclomatic", 5.0, 0.0),
        &ranked("cyclomatic", 3.0, 0.0),
    );
}

#[test]
fn equal_severity_breaks_on_metric_name_ascending() {
    // Same value and limit: severity and excess tie exactly, so only the
    // documented metric-name key can decide, in either input order.
    let a = ranked("cognitive", 30.0, 15.0);
    let b = ranked("cyclomatic", 30.0, 15.0);
    for vs in [[&a, &b], [&b, &a]] {
        assert_eq!(
            Violation::pick_worst(&vs).map(|w| w.metric),
            Some("cognitive")
        );
    }
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
/// observable inside the method. The Javadoc on `area` is what makes
/// `loc.dloc` observable (#1597).
#[test]
fn every_extractor_metric_family_suffices_to_compute_its_accessor() {
    use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

    /// Class with public members, branches, arguments, comments, a
    /// Javadoc, blank lines and several exits, so no extractor reads
    /// zero everywhere.
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

    /** The area of the box. */
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
