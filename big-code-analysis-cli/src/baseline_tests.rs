// Sibling-file unit tests for `Baseline` parsing/loading/classify and
// related helpers, wired in via `#[path = "baseline_tests.rs"] mod
// tests;` so the production `baseline.rs` stays under the `bca check`
// per-file metric caps. Matched by the `./**/*_tests.rs` rule in
// `.bcaignore`, so the self-scan walker skips this file the same way
// it skips `./tests/`.

use super::*;
use std::path::PathBuf;

fn v(path: &str, function: &str, start_line: usize, metric: &'static str, value: f64) -> Violation {
    Violation {
        path: PathBuf::from(path),
        start_line,
        end_line: start_line + 1,
        function: function.to_string(),
        metric,
        value,
        limit: 1.0,
        hard_limit: Some(1.0),
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
    }
}

/// Like [`v`] but flagged lower-is-worse (the `mi.*` family), so the
/// direction-aware `classify` ratchet (#827) treats a value *below* the
/// recorded baseline as the regression rather than as coverage.
fn v_low(
    path: &str,
    function: &str,
    start_line: usize,
    metric: &'static str,
    value: f64,
) -> Violation {
    Violation {
        lower_is_worse: true,
        ..v(path, function, start_line, metric, value)
    }
}

/// Like [`v`] but with an explicit body hash, for the fuzzy-match tests.
fn v_hashed(
    path: &str,
    function: &str,
    start_line: usize,
    metric: &'static str,
    value: f64,
    body_hash: u64,
) -> Violation {
    Violation {
        body_hash: Some(body_hash),
        suppressed: false,
        ..v(path, function, start_line, metric, value)
    }
}

/// Canonical empty anchor for unit tests: the violation path is keyed
/// as-passed without prepending a synthetic CWD. Real callers always
/// derive their anchor via [`anchor_for`] from the baseline file path,
/// but for the in-memory tests in this file an empty anchor preserves
/// the pre-#376 semantics of "key on the literal path string the test
/// supplied" while still exercising the new lexical normalisation.
fn test_anchor() -> &'static Path {
    Path::new("")
}

fn parse(text: &str) -> Result<Baseline, String> {
    Baseline::from_str(text, test_anchor(), DEFAULT_LINE_TOLERANCE, false)
}

/// Parse with the fuzzy body-hash fallback enabled.
fn parse_fuzzy(text: &str) -> Result<Baseline, String> {
    Baseline::from_str(text, test_anchor(), DEFAULT_LINE_TOLERANCE, true)
}

// -- parsing / loading -------------------------------------------------

#[test]
fn parse_minimal_version_only() {
    let b = parse("version = 2\n").expect("minimal parse");
    assert_eq!(b.by_symbol.len(), 0);
}

#[test]
fn parse_round_trip_preserves_entries() {
    let original = from_violations(
        vec![
            v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
            v("src/b.rs", "bar", 20, "cognitive", 7.0),
        ],
        test_anchor(),
        Provenance::hard(),
    );
    let rendered = render(&original).expect("render");
    let reloaded = parse(&rendered).expect("reload");
    assert_eq!(reloaded.by_symbol.len(), 2);
    let v_now = v("src/a.rs", "foo", 10, "cyclomatic", 5.0);
    assert!(matches!(
        reloaded.classify(&v_now),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn parse_drops_negative_zero_values() {
    // `-0.0 < 0.0` is false under IEEE 754, so a `< 0.0` filter
    // would miss `-0.0` and store `recorded == -0.0` — then the
    // regression renderer divides by zero producing an `inf`-shaped
    // tag. `is_sign_negative()` correctly catches `-0.0`.
    let toml = "version = 3\n[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=-0.0\n";
    let b = parse(toml).expect("parse");
    assert_eq!(b.by_symbol.len(), 0);
}

#[test]
fn parse_drops_negative_values() {
    // Hand-edited baselines with negative `value` entries are
    // silently dropped, matching the non-finite defence above.
    // The `from_str` filter prevents `format_regressed_tag` from
    // emitting a double-signed `[regr +-N%]` tag for a corrupted
    // baseline.
    let toml = "version = 2\n[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=-10.0\n";
    let b = parse(toml).expect("parse");
    assert_eq!(b.by_symbol.len(), 0);
    // The corresponding violation classifies as `New`, not Covered
    // or Regressed, because the entry was dropped at parse time.
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", 5.0)),
        Coverage::New
    ));
}

#[test]
fn parse_rejects_higher_version() {
    let err = parse("version = 99\n").unwrap_err();
    assert!(
        err.contains("upgrade bca") || err.contains("regenerate"),
        "msg: {err}"
    );
}

#[test]
fn parse_rejects_missing_version() {
    let err = parse(
        "[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=1.0\n",
    )
    .unwrap_err();
    assert!(err.contains("missing version field"), "msg: {err}");
}

#[test]
fn parse_rejects_empty_file() {
    let err = parse("").unwrap_err();
    assert!(err.contains("missing version field"), "msg: {err}");
}

#[test]
fn parse_rejects_malformed_value() {
    let err = parse(
        "version = 2\n[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=\"oops\"\n",
    )
    .unwrap_err();
    assert!(err.contains("malformed baseline TOML"), "msg: {err}");
}

#[test]
fn parse_accepts_legacy_v2_and_re_canonicalizes() {
    // v2 baselines pre-date the anchor-relative key form from
    // issue #376. The loader runs each legacy entry's path through
    // the v3 pipeline so a v2 entry keyed `./src/a.rs` still matches
    // a violation reported as `src/a.rs` under the new canonical
    // form. The migration is best-effort — ASCII-clean paths migrate
    // transparently; pre-encoded non-ASCII paths may double-encode
    // and need a `--write-baseline` refresh.
    let b = parse(
        "version = 2\n[[entry]]\npath=\"./src/a.rs\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\n",
    )
    .expect("parse");
    assert_eq!(b.by_symbol.len(), 1);
    assert!(matches!(
        b.classify(&v("src/a.rs", "f", 1, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn parse_rejects_below_legacy_minimum() {
    // v1 is below LEGACY_MIN_VERSION (2) — its percent-encoding
    // semantics differ enough that silent migration would
    // mis-key non-ASCII paths.
    let err = parse("version = 1\n").unwrap_err();
    assert!(
        err.contains("regenerate") || err.contains("upgrade bca"),
        "msg: {err}"
    );
}

#[test]
fn parse_silently_ignores_unknown_metric() {
    // An entry naming a metric that no extractor exists for parses
    // cleanly; it just never matches anything (no extractor produces
    // that metric name in a Violation).
    let b = parse(
        "version = 2\n[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"imaginary\"\nvalue=1.0\n",
    )
    .expect("parse");
    assert_eq!(b.by_symbol.len(), 1);
    // No violation will ever have metric = "imaginary" (it's not in
    // the registry), so classify() always returns New for real input.
    let v_real = v("a", "f", 1, "cyclomatic", 1.0);
    assert!(matches!(b.classify(&v_real), Coverage::New));
}

#[test]
fn parse_silently_ignores_unknown_fields() {
    let b = parse(
        "version = 2\n[[entry]]\npath=\"a\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=1.0\nextra_field=42\n",
    )
    .expect("parse");
    assert_eq!(b.by_symbol.len(), 1);
}

// -- from_violations ---------------------------------------------------

#[test]
fn from_violations_skips_negative_values() {
    // Round-trip symmetry with `from_str`: writer drops the same
    // entries reader would silently filter, so a synthetic
    // `Violation { value: -1.0 }` round-trip is consistent rather
    // than producing a TOML entry the reader then deletes.
    // `is_sign_negative()` also catches `-0.0`.
    let file = from_violations(
        vec![
            v("a", "neg", 1, "cyclomatic", -1.0),
            v("a", "nzero", 2, "cyclomatic", -0.0),
            v("a", "ok", 3, "cyclomatic", 5.0),
        ],
        test_anchor(),
        Provenance::hard(),
    );
    assert_eq!(file.entries.len(), 1);
    assert_eq!(file.entries[0].qualified, "ok");
}

#[test]
fn from_violations_skips_non_finite() {
    let file = from_violations(
        vec![
            v("a", "f", 1, "cyclomatic", f64::NAN),
            v("a", "g", 2, "cyclomatic", f64::INFINITY),
            v("a", "h", 3, "cyclomatic", f64::NEG_INFINITY),
            v("a", "i", 4, "cyclomatic", 5.0),
        ],
        test_anchor(),
        Provenance::hard(),
    );
    assert_eq!(file.entries.len(), 1);
    assert_eq!(file.entries[0].qualified, "i");
}

#[test]
fn from_violations_deterministic_order() {
    // Inputs are crafted so every tiebreaker in the
    // (path, qualified, metric, start_line) sort is the deciding
    // comparator for at least one adjacent pair in the output:
    //
    //   [0] vs [1]: same path + qualified -> metric breaks tie
    //   [1] vs [2]: same path + qualified + metric (an ambiguous
    //               identity) -> start_line breaks tie
    //   [2] vs [3]: same path, different qualified -> qualified breaks tie
    //   [3] vs [4]: different path -> path breaks tie
    //
    // The same fixture pins which entries keep a `start_line` (#1170):
    // only [1] and [2], the two sharing one identity.
    //
    // The ambiguous pair is fed 99-before-10, against the order it must
    // come back in. That is what makes the `start_line` claim above
    // falsifiable: `sort_by` is stable, so an already-ascending pair
    // comes back ascending whether or not the comparator ever looks at
    // the line. Dropping `start_line` from `BaselineEntry::identity`
    // failed none of the suite's 5054 tests while this vector was
    // pre-sorted.
    let unsorted = vec![
        v("src/z.rs", "z", 100, "cyclomatic", 5.0),
        v("src/a.rs", "b", 10, "cognitive", 4.0),
        v("src/a.rs", "a", 10, "cognitive", 3.0),
        v("src/a.rs", "a", 99, "cyclomatic", 6.0),
        v("src/a.rs", "a", 10, "cyclomatic", 5.0),
    ];
    let file = from_violations(unsorted, test_anchor(), Provenance::hard());
    assert_eq!(file.entries[0].path, "src/a.rs");
    assert_eq!(file.entries[0].qualified, "a");
    // Unique identity -> no line recorded.
    assert_eq!(file.entries[0].start_line, None);
    assert_eq!(file.entries[0].metric, "cognitive");
    assert_eq!(file.entries[1].path, "src/a.rs");
    assert_eq!(file.entries[1].qualified, "a");
    assert_eq!(file.entries[1].start_line, Some(10));
    assert_eq!(file.entries[1].metric, "cyclomatic");
    assert_eq!(file.entries[2].path, "src/a.rs");
    assert_eq!(file.entries[2].qualified, "a");
    assert_eq!(file.entries[2].start_line, Some(99));
    assert_eq!(file.entries[3].path, "src/a.rs");
    assert_eq!(file.entries[3].qualified, "b");
    assert_eq!(file.entries[3].start_line, None);
    assert_eq!(file.entries[4].path, "src/z.rs");
    assert_eq!(file.entries[4].start_line, None);
}

#[test]
fn from_violations_byte_equal_across_two_calls() {
    let input = vec![
        v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
        v("src/b.rs", "bar", 20, "cognitive", 7.0),
    ];
    let a = render(&from_violations(
        input.clone(),
        test_anchor(),
        Provenance::hard(),
    ))
    .expect("render a");
    let b = render(&from_violations(input, test_anchor(), Provenance::hard())).expect("render b");
    assert_eq!(a, b);
}

/// The churn half of #1170, and the more valuable one: a baseline
/// written before and after an edit that shifted every function down the
/// file must be byte-identical, so a *real* baseline change (a `value`
/// moving) is the only thing a reviewer ever sees in the diff.
#[test]
fn line_drift_leaves_the_rendered_baseline_byte_identical() {
    let before = vec![
        v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
        v("src/a.rs", "Bar::baz", 40, "cognitive", 7.0),
        v("src/b.rs", "qux", 20, "cognitive", 7.0),
    ];
    // Same functions, same values, each pushed 500 lines down by an
    // import block or a sibling function added above.
    let after: Vec<Violation> = before
        .iter()
        .map(|v| Violation {
            start_line: v.start_line + 500,
            end_line: v.end_line + 500,
            ..v.clone()
        })
        .collect();
    let rendered_before =
        render(&from_violations(before, test_anchor(), Provenance::hard())).expect("render before");
    let rendered_after =
        render(&from_violations(after, test_anchor(), Provenance::hard())).expect("render after");
    assert_eq!(rendered_before, rendered_after);
    // Guard against passing for the wrong reason: the file must really
    // hold the entries, and really hold no line numbers.
    assert!(rendered_before.contains("qualified = \"Bar::baz\""));
    assert!(
        !rendered_before.contains("start_line"),
        "unique identities record no line:\n{rendered_before}"
    );
}

/// The exception that keeps the tolerance disambiguator working: when
/// two entries share one `(path, qualified, metric)` identity, a line is
/// the only thing that tells them apart, so both record one — and those
/// two entries do still move under line drift.
///
/// `src/b.rs`'s `unique` is what pins the `path` third of that identity,
/// and it has to sit *immediately after* `src/a.rs`'s `unique` to do it:
/// the grouping is `chunk_by_mut`, so only adjacent entries are ever
/// compared, and entries are sorted by path first. A same-named function
/// in a non-adjacent file would chunk alone under a broken predicate too,
/// and prove nothing. Dropping `a.path == b.path` from `same_identity`
/// failed none of the suite's 5053 tests before this entry existed —
/// which is #1170's churn bug returning for `new` / `fmt` / `default`,
/// the commonest symbol shape in a Rust tree.
#[test]
fn ambiguous_identity_records_a_line_for_every_member() {
    let file = from_violations(
        vec![
            v("src/a.rs", "Trait::is_valid", 10, "cyclomatic", 5.0),
            v("src/a.rs", "Trait::is_valid", 900, "cyclomatic", 6.0),
            v("src/a.rs", "unique", 40, "cyclomatic", 8.0),
            v("src/b.rs", "unique", 40, "cyclomatic", 8.0),
        ],
        test_anchor(),
        Provenance::hard(),
    );
    let lines: Vec<(&str, &str, Option<usize>)> = file
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e.qualified.as_str(), e.start_line))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("src/a.rs", "Trait::is_valid", Some(10)),
            ("src/a.rs", "Trait::is_valid", Some(900)),
            ("src/a.rs", "unique", None),
            ("src/b.rs", "unique", None),
        ]
    );
}

#[test]
fn path_normalized_forward_slash_on_serialize() {
    // Construct a Violation with a backslash path directly (so the
    // test passes on any host).
    let file = from_violations(
        vec![v("a\\b\\c.rs", "f", 1, "cyclomatic", 5.0)],
        test_anchor(),
        Provenance::hard(),
    );
    assert_eq!(file.entries[0].path, "a/b/c.rs");
}

// -- covers ------------------------------------------------------------

fn baseline_with(entries: Vec<BaselineEntry>) -> Baseline {
    let file = BaselineFile {
        version: Some(BASELINE_VERSION),
        provenance: None,
        entries,
    };
    let text = render(&file).expect("render");
    Baseline::from_str(&text, test_anchor(), DEFAULT_LINE_TOLERANCE, false).expect("parse")
}

fn entry(
    path: &str,
    qualified: &str,
    start_line: usize,
    metric: &str,
    value: f64,
) -> BaselineEntry {
    BaselineEntry {
        path: path.to_string(),
        qualified: qualified.to_string(),
        start_line: Some(start_line),
        metric: metric.to_string(),
        value,
        body_hash: None,
    }
}

/// A hand-built entry that pins no `start_line` — the shape a v6 file
/// writes for a unique identity (#1170).
fn entry_without_line(path: &str, qualified: &str, metric: &str, value: f64) -> BaselineEntry {
    BaselineEntry {
        start_line: None,
        ..entry(path, qualified, 0, metric, value)
    }
}

#[test]
fn classify_at_exact_baseline_is_covered() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    // Equality is covered, not regressed. This pins the `<=` boundary;
    // a mutation flipping `<=` to `<` would classify this as Regressed.
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_below_baseline_is_covered() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", 3.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_worsened_is_regressed() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", 6.0)),
        Coverage::Regressed { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_lower_is_worse_drop_is_regressed() {
    // For an mi.* (lower-is-worse) metric a value *below* the recorded
    // baseline is a genuine regression. Before #827 `classify` ratcheted
    // unconditionally on `value <= recorded`, so this returned `Covered`
    // and the gate silently dropped the regression.
    let b = baseline_with(vec![entry("a", "f", 1, "mi.original", 60.0)]);
    assert!(matches!(
        b.classify(&v_low("a", "f", 1, "mi.original", 45.0)),
        Coverage::Regressed { recorded } if recorded == 60.0
    ));
}

#[test]
fn classify_lower_is_worse_rise_is_covered() {
    // For an mi.* metric a value at or *above* the recorded baseline has
    // not worsened, so it is covered (the improvement direction).
    let b = baseline_with(vec![entry("a", "f", 1, "mi.original", 60.0)]);
    assert!(matches!(
        b.classify(&v_low("a", "f", 1, "mi.original", 75.0)),
        Coverage::Covered { recorded } if recorded == 60.0
    ));
    // Equality is still covered for the lower-is-worse direction too.
    assert!(matches!(
        b.classify(&v_low("a", "f", 1, "mi.original", 60.0)),
        Coverage::Covered { recorded } if recorded == 60.0
    ));
}

#[test]
fn classify_different_path_is_new() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("b", "f", 1, "cyclomatic", 5.0)),
        Coverage::New
    ));
}

#[test]
fn classify_different_function_is_new() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "g", 1, "cyclomatic", 5.0)),
        Coverage::New
    ));
}

#[test]
fn classify_v4_qualified_symbol_matches_without_legacy_bare_reduction() {
    // Regression for the v4 bare-name reduction the BASELINE_VERSION
    // 4->5 bump (#486 provenance) introduced: `legacy_symbol_match`
    // must stay pinned to `version < QUALIFIED_SYMBOL_MIN_VERSION` (4),
    // not `version < BASELINE_VERSION`. A v4 entry already keys by the
    // qualified symbol, so a current violation with the same qualified
    // name (`MyStruct::do_thing`) must classify `Covered` — under the
    // bug it was reduced to the bare `do_thing`, mis-keyed against the
    // stored `MyStruct::do_thing`, and wrongly reclassified `New`,
    // failing the gate for every downstream v4 baseline with `::`.
    let toml = "version = 4\n[[entry]]\npath=\"a\"\nqualified=\"MyStruct::do_thing\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\n";
    let b = parse(toml).expect("parse");
    assert!(matches!(
        b.classify(&v("a", "MyStruct::do_thing", 1, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_survives_arbitrary_line_drift_for_unique_symbol() {
    // The headline #377 behaviour: a baseline entry whose qualified
    // symbol is unique in the file matches the violation regardless of
    // how far the function has drifted down the file. Adding imports or
    // a sibling function above `f` must not re-key it as `[new]`.
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 9_999, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_ambiguous_symbol_picks_closest_within_tolerance() {
    // Two methods share the qualified symbol `is_valid` (e.g. the
    // analyzer could not resolve distinct containers). The start_line
    // disambiguator routes each violation to the nearer record so the
    // recorded values do not cross-contaminate.
    let b = baseline_with(vec![
        entry("a", "is_valid", 10, "cyclomatic", 5.0),
        entry("a", "is_valid", 200, "cyclomatic", 8.0),
    ]);
    // A violation at line 12 (drifted 2 from the first record) matches
    // the value-5 record, not the value-8 one 190 lines away.
    assert!(matches!(
        b.classify(&v("a", "is_valid", 12, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
    // A violation at line 198 matches the value-8 record.
    assert!(matches!(
        b.classify(&v("a", "is_valid", 198, "cyclomatic", 8.0)),
        Coverage::Covered { recorded } if recorded == 8.0
    ));
}

#[test]
fn classify_ambiguous_symbol_equidistant_tie_prefers_higher_value() {
    // Two equidistant ambiguous records (lines 10 and 20, violation at
    // 15). The tie breaks toward the higher recorded value (8.0), so an
    // unplaceable violation of 7.0 is Covered rather than a spurious
    // regression against the lower record (5.0). Deterministic
    // regardless of entry order.
    let b = baseline_with(vec![
        entry("a", "f", 10, "cyclomatic", 5.0),
        entry("a", "f", 20, "cyclomatic", 8.0),
    ]);
    assert!(matches!(
        b.classify(&v("a", "f", 15, "cyclomatic", 7.0)),
        Coverage::Covered { recorded } if recorded == 8.0
    ));
}

#[test]
fn classify_respects_custom_tolerance() {
    // The tolerance value threaded from `--baseline-line-tolerance` /
    // `baseline_line_tolerance` is honoured by `from_str`: with a tight
    // tolerance of 2, an ambiguous-symbol violation 3 lines from the
    // nearer record is `New`, but 2 lines away matches.
    let file = BaselineFile {
        version: Some(BASELINE_VERSION),
        provenance: None,
        entries: vec![
            entry("a", "f", 10, "cyclomatic", 5.0),
            entry("a", "f", 100, "cyclomatic", 8.0),
        ],
    };
    let text = render(&file).expect("render");
    let tight = Baseline::from_str(&text, test_anchor(), 2, false).expect("parse");

    assert!(matches!(
        tight.classify(&v("a", "f", 12, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
    assert!(matches!(
        tight.classify(&v("a", "f", 13, "cyclomatic", 5.0)),
        Coverage::New
    ));
}

#[test]
fn parse_fuzzy_tolerates_malformed_body_hash() {
    // A hand-edited baseline with a corrupt `body_hash` still loads; the
    // entry simply loses fuzzy eligibility (degrades to no-fuzzy) rather
    // than aborting the parse. The qualified symbol still matches.
    let toml = "version = 4\n[[entry]]\npath=\"a\"\nqualified=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\nbody_hash=\"not-a-valid-hex-digest\"\n";
    let b = parse_fuzzy(toml).expect("parse");
    assert_eq!(b.by_symbol.len(), 1);
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_ambiguous_symbol_beyond_tolerance_is_new() {
    // Both records for the ambiguous symbol are far from the observed
    // line (>50 default tolerance), so neither disambiguates — the
    // violation is genuinely unplaceable and surfaces as New.
    let b = baseline_with(vec![
        entry("a", "is_valid", 10, "cyclomatic", 5.0),
        entry("a", "is_valid", 1_000, "cyclomatic", 8.0),
    ]);
    assert!(matches!(
        b.classify(&v("a", "is_valid", 500, "cyclomatic", 5.0)),
        Coverage::New
    ));
}

#[test]
fn classify_fuzzy_matches_renamed_function_by_body_hash() {
    // A function was renamed (`old_name` -> `new_name`) but its body is
    // unchanged. The qualified symbol no longer matches, but with fuzzy
    // matching on, the body hash routes the violation to the recorded
    // entry. Without fuzzy it would be New.
    let file = BaselineFile {
        version: Some(BASELINE_VERSION),
        provenance: None,
        entries: vec![BaselineEntry {
            path: "a".to_string(),
            qualified: "old_name".to_string(),
            start_line: Some(1),
            metric: "cyclomatic".to_string(),
            value: 5.0,
            body_hash: Some(format!("{:016x}", 0xdead_beef_u64)),
        }],
    };
    let text = render(&file).expect("render");
    let renamed = v_hashed("a", "new_name", 1, "cyclomatic", 5.0, 0xdead_beef);

    let strict = parse(&text).expect("parse");
    assert!(matches!(strict.classify(&renamed), Coverage::New));

    let fuzzy = parse_fuzzy(&text).expect("parse");
    assert!(matches!(
        fuzzy.classify(&renamed),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_legacy_v3_matches_on_bare_name() {
    // A v3 baseline stored the bare `function` name (`do_thing`). A v4
    // violation now reports the qualified symbol `MyStruct::do_thing`.
    // Legacy matching compares the violation's bare name against the
    // stored bare name, so the entry still covers it — and survives
    // line drift now, which v3's exact-line key did not.
    let toml = "version = 3\n[[entry]]\npath=\"a\"\nfunction=\"do_thing\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\n";
    let b = parse(toml).expect("parse");
    assert!(matches!(
        b.classify(&v("a", "MyStruct::do_thing", 42, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_different_metric_is_new() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cognitive", 5.0)),
        Coverage::New
    ));
}

#[test]
fn classify_normalizes_filter_path() {
    // Baseline entry uses forward slashes; filter side passes a
    // path with backslashes. They should match after normalization.
    let b = baseline_with(vec![entry("src/a.rs", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("src\\a.rs", "f", 1, "cyclomatic", 5.0)),
        Coverage::Covered { .. }
    ));
}

#[test]
fn classify_nan_value_with_entry_is_regressed() {
    // NaN current values can occur on degenerate Halstead inputs.
    // Without the explicit NaN guard in classify(), `NaN <= recorded`
    // is false → the violation would fall to the trailing
    // Regressed arm anyway, but the guard makes the intent loud
    // and lets the renderer key off is_nan() to emit `[regr NaN]`.
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 1, "cyclomatic", f64::NAN)),
        Coverage::Regressed { recorded } if recorded == 5.0
    ));
}

#[test]
fn classify_zero_recorded_regression_carries_zero() {
    // Edge case for the [regr from 0] renderer branch: a baseline
    // can record 0.0 when a metric was zero at write time. The
    // classifier still produces Regressed; the renderer handles
    // the divide-by-zero in `+N%`.
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 0.0)]);
    let coverage = b.classify(&v("a", "f", 1, "cyclomatic", 5.0));
    match coverage {
        Coverage::Regressed { recorded } => {
            assert_eq!(recorded.to_bits(), 0.0_f64.to_bits());
        }
        other => panic!("expected Regressed, got {other:?}"),
    }
}

#[test]
fn classify_recorded_round_trips_bit_exactly() {
    // The renderer relies on `recorded` being the same f64 bits
    // as the stored entry — anything else would shift the rendered
    // percentage by a ULP on float-fragile metrics.
    let recorded = 1.234_567_890_123_456_7_f64;
    let b = baseline_with(vec![entry("a", "f", 1, "halstead.volume", recorded)]);
    let coverage = b.classify(&v("a", "f", 1, "halstead.volume", recorded * 2.0));
    match coverage {
        Coverage::Regressed { recorded: got } => {
            assert_eq!(got.to_bits(), recorded.to_bits());
        }
        other => panic!("expected Regressed, got {other:?}"),
    }
}

// -- optional start_line (issue #1170) --------------------------------

/// The acceptance criterion for the v6 re-key: a legacy baseline that
/// records a line for every entry and its v6 rewrite that records none
/// must classify the *same set* of violations the same way. Asserting
/// the whole set, not a count — a count can hold steady while the set
/// changes.
#[test]
fn v5_and_v6_baselines_classify_an_identical_offender_set() {
    use std::fmt::Write as _;

    let recorded = [
        ("src/a.rs", "foo", 10, "cyclomatic", 5.0),
        ("src/a.rs", "Bar::baz", 40, "cognitive", 7.0),
        ("src/b.rs", "qux", 300, "halstead.effort", 60_000.0),
    ];
    // Same entries either way; only the schema stamp and the presence
    // of `start_line` differ.
    let mut v5 = String::from("version = 5\n");
    for (path, qualified, line, metric, value) in recorded {
        let _ = write!(
            v5,
            "[[entry]]\npath = \"{path}\"\nqualified = \"{qualified}\"\n\
             start_line = {line}\nmetric = \"{metric}\"\nvalue = {value}\n"
        );
    }
    let v6 = render(&from_violations(
        recorded
            .iter()
            .map(|&(p, q, l, m, val)| v(p, q, l, m, val))
            .collect(),
        test_anchor(),
        Provenance::hard(),
    ))
    .expect("render v6");
    assert!(!v6.contains("start_line"));

    // Probe every interesting outcome: covered at the recorded value,
    // covered below it, regressed above it, drifted far past the
    // tolerance, and an entry the baseline never had.
    let probes = [
        v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
        v("src/a.rs", "foo", 4_000, "cyclomatic", 4.0),
        v("src/a.rs", "Bar::baz", 40, "cognitive", 9.0),
        v("src/b.rs", "qux", 1, "halstead.effort", 60_000.0),
        v("src/b.rs", "never_recorded", 7, "cognitive", 3.0),
    ];
    let classify_all = |text: &str| -> Vec<Coverage> {
        let b = parse(text).expect("parse");
        probes.iter().map(|p| b.classify(p)).collect()
    };
    assert_eq!(classify_all(&v5), classify_all(&v6));
    // And pin what that agreed-on set actually is, so the assertion
    // cannot pass by both sides degrading to `New` together.
    assert_eq!(
        classify_all(&v6),
        vec![
            Coverage::Covered { recorded: 5.0 },
            Coverage::Covered { recorded: 5.0 },
            Coverage::Regressed { recorded: 7.0 },
            Coverage::Covered { recorded: 60_000.0 },
            Coverage::New,
        ]
    );
}

/// Two entries that differ only in `start_line` are one identity, and
/// stay one identity when the lines are gone: the group is what the
/// tolerance rule operates on, not the individual lines.
#[test]
fn entries_differing_only_in_start_line_share_one_identity() {
    let b = baseline_with(vec![
        entry("a", "Trait::f", 10, "cyclomatic", 5.0),
        entry("a", "Trait::f", 900, "cyclomatic", 9.0),
    ]);
    assert_eq!(b.by_symbol.len(), 1, "one key, two records under it");
    // With the lines present the tolerance still places each violation
    // on the nearer record.
    assert!(matches!(
        b.classify(&v("a", "Trait::f", 12, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
    assert!(matches!(
        b.classify(&v("a", "Trait::f", 902, "cyclomatic", 9.0)),
        Coverage::Covered { recorded } if recorded == 9.0
    ));
}

/// A lone record matches regardless of line, so dropping the line
/// changes nothing for it — the property that makes omitting the field
/// safe in the first place.
#[test]
fn lone_record_without_a_line_matches_at_any_distance() {
    let b = baseline_with(vec![entry_without_line("a", "f", "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 100_000, "cyclomatic", 5.0)),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

/// Only reachable by hand-editing: a record inside an *ambiguous* group
/// with no line cannot be placed, so it drops out of the tolerance match
/// rather than matching at distance zero and shadowing its sibling.
#[test]
fn unplaceable_record_in_an_ambiguous_group_is_skipped() {
    let b = baseline_with(vec![
        entry_without_line("a", "Trait::f", "cyclomatic", 99.0),
        entry("a", "Trait::f", 900, "cyclomatic", 9.0),
    ]);
    // The lineless record's generous 99.0 must not be what covers a
    // violation sitting on the other record's line.
    assert!(matches!(
        b.classify(&v("a", "Trait::f", 900, "cyclomatic", 20.0)),
        Coverage::Regressed { recorded } if recorded == 9.0
    ));
    // And a violation nowhere near the placed record is `New`, not
    // silently covered by the unplaceable one.
    assert!(matches!(
        b.classify(&v("a", "Trait::f", 5, "cyclomatic", 20.0)),
        Coverage::New
    ));
}

/// A file from a *newer* schema can fail to deserialize before the
/// version check runs — v6 dropping a field a v5-era build requires is
/// exactly that shape. The error must still name the remedy rather than
/// surfacing a bare serde message.
#[test]
fn parse_reports_the_version_when_a_newer_schema_fails_to_deserialize() {
    // `value` is required in every schema, so its absence stands in for
    // "a field this build needs that version 99 no longer writes".
    let err = parse(
        "version = 99\n[[entry]]\npath = \"a\"\nqualified = \"f\"\nmetric = \"cyclomatic\"\n",
    )
    .unwrap_err();
    assert!(
        err.contains("version 99") && err.contains("--write-baseline"),
        "msg: {err}"
    );
}

/// …but a genuinely malformed file still gets the parser's own message,
/// which is the one that says *where* it broke.
#[test]
fn parse_reports_the_toml_error_when_the_version_is_supported() {
    let err = parse("version = 6\n[[entry]]\npath = \"a\"\nvalue = \"oops\"\n").unwrap_err();
    assert!(err.contains("malformed baseline TOML"), "msg: {err}");
}
// -- path-key integration through `Baseline` ---------------------------

#[cfg(unix)]
#[test]
fn from_str_defensive_anchor_normalization() {
    // Caller bypasses `anchor_for` and supplies an un-normalised anchor
    // (`/repo/.` instead of `/repo`). `Path::strip_prefix` is
    // component-exact, so without defensive normalisation every key
    // would fail to strip and surface as absolute. The defensive
    // `lexical_normalize(anchor)` in `from_str` lets classify match
    // a violation at `/repo/src/foo.rs` against an entry keyed
    // `src/foo.rs`.
    let toml = "version = 3\n[[entry]]\npath=\"src/foo.rs\"\nfunction=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\n";
    let b = Baseline::from_str(toml, Path::new("/repo/."), DEFAULT_LINE_TOLERANCE, false)
        .expect("parse");
    assert!(matches!(
        b.classify(&Violation {
            path: PathBuf::from("/repo/src/foo.rs"),
            start_line: 1,
            end_line: 2,
            function: "f".to_string(),
            metric: "cyclomatic",
            value: 5.0,
            limit: 1.0,
            hard_limit: Some(1.0),
            lower_is_worse: false,
            body_hash: None,
            suppressed: false,
        }),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
}

#[cfg(unix)]
#[test]
fn baseline_covers_distinguishes_non_utf8_paths() {
    // End-to-end: a baseline written for path A must not cover a
    // violation reported against path B when the only difference
    // is the invalid byte sequence in the filename.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let path_a = PathBuf::from("src").join(OsStr::from_bytes(b"\xff\xfe.rs"));
    let path_b = PathBuf::from("src").join(OsStr::from_bytes(b"\xfe\xff.rs"));

    let violation_a = Violation {
        path: path_a.clone(),
        start_line: 1,
        end_line: 2,
        function: "f".to_string(),
        metric: "cyclomatic",
        value: 5.0,
        limit: 1.0,
        hard_limit: Some(1.0),
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
    };
    let violation_b = Violation {
        path: path_b,
        start_line: 1,
        end_line: 2,
        function: "f".to_string(),
        metric: "cyclomatic",
        value: 5.0,
        limit: 1.0,
        hard_limit: Some(1.0),
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
    };

    // Baseline contains only `path_a`. classify(violation_b) would
    // wrongly return Covered if both non-UTF-8 paths normalized
    // to the same lossy key.
    let file = from_violations(vec![violation_a.clone()], test_anchor(), Provenance::hard());
    let rendered = render(&file).expect("render");
    let b =
        Baseline::from_str(&rendered, test_anchor(), DEFAULT_LINE_TOLERANCE, false).expect("parse");
    assert!(matches!(b.classify(&violation_a), Coverage::Covered { .. }));
    assert!(matches!(b.classify(&violation_b), Coverage::New));
}

// -- provenance (issue #486) -------------------------------------------

#[test]
fn provenance_v5_round_trips_soft_headroom() {
    // A soft-headroom baseline written at 0.95 must read back with the
    // same tier and ratio, and the rendered TOML must carry a real
    // `[provenance]` table (not a comment) so `diff-baseline` and other
    // tooling can parse it.
    let file = from_violations(
        vec![v("src/a.rs", "foo", 10, "cyclomatic", 5.0)],
        test_anchor(),
        Provenance::soft_headroom(0.95),
    );
    let rendered = render(&file).expect("render");
    assert!(
        rendered.contains("[provenance]"),
        "expected a [provenance] table; got:\n{rendered}"
    );
    assert!(
        rendered.contains("tier = \"soft\""),
        "tier missing:\n{rendered}"
    );
    assert!(
        rendered.contains("headroom = 0.95"),
        "headroom missing:\n{rendered}"
    );
    let reloaded = Baseline::from_str(&rendered, test_anchor(), DEFAULT_LINE_TOLERANCE, false)
        .expect("reload");
    assert_eq!(reloaded.provenance(), Some(Provenance::soft_headroom(0.95)));
}

#[test]
fn provenance_v5_round_trips_hard() {
    // A hard-tier baseline omits the headroom key but records the tier.
    let file = from_violations(
        vec![v("src/a.rs", "foo", 10, "cyclomatic", 5.0)],
        test_anchor(),
        Provenance::hard(),
    );
    let rendered = render(&file).expect("render");
    assert!(
        rendered.contains("tier = \"hard\""),
        "tier missing:\n{rendered}"
    );
    assert!(
        !rendered.contains("headroom"),
        "hard baseline must not emit a headroom key:\n{rendered}"
    );
    let reloaded = Baseline::from_str(&rendered, test_anchor(), DEFAULT_LINE_TOLERANCE, false)
        .expect("reload");
    assert_eq!(reloaded.provenance(), Some(Provenance::hard()));
    assert_eq!(
        reloaded.provenance().and_then(|p| p.strictness()),
        Some(1.0)
    );
}

#[test]
fn provenance_v5_soft_table_has_no_ratio() {
    // A `[thresholds.soft]`-table baseline records soft tier with no
    // headroom; its strictness scalar is unknown (None).
    let file = from_violations(
        vec![v("src/a.rs", "foo", 10, "cyclomatic", 5.0)],
        test_anchor(),
        Provenance::soft_table(),
    );
    let rendered = render(&file).expect("render");
    assert!(
        rendered.contains("tier = \"soft\""),
        "tier missing:\n{rendered}"
    );
    assert!(
        !rendered.contains("headroom"),
        "soft-table baseline carries no single ratio:\n{rendered}"
    );
    let reloaded = Baseline::from_str(&rendered, test_anchor(), DEFAULT_LINE_TOLERANCE, false)
        .expect("reload");
    assert_eq!(reloaded.provenance(), Some(Provenance::soft_table()));
    assert_eq!(reloaded.provenance().and_then(|p| p.strictness()), None);
}

#[test]
fn provenance_absent_for_legacy_v2_v3_v4() {
    // v2–v4 baselines predate provenance: read without error and report
    // provenance as absent (so the directional check stays silent).
    for v in [2u32, 3, 4] {
        let toml = format!(
            "version = {v}\n[[entry]]\npath=\"a\"\n{name}=\"f\"\nstart_line=1\nmetric=\"cyclomatic\"\nvalue=5.0\n",
            name = if v < 4 { "function" } else { "qualified" },
        );
        let b = Baseline::from_str(&toml, test_anchor(), DEFAULT_LINE_TOLERANCE, false)
            .unwrap_or_else(|e| panic!("v{v} parse: {e}"));
        assert_eq!(b.provenance(), None, "v{v} must have absent provenance");
    }
}

#[test]
fn check_provenance_silent_when_hard_reads_soft() {
    // The repo's intended setup: a hard self-scan (strictness 1.0)
    // reading a soft-0.95 baseline (strictness 0.95) sees a SUPERSET of
    // its offenders, so it must stay silent.
    assert_eq!(
        check_provenance(Provenance::hard(), Some(Provenance::soft_headroom(0.95))),
        ProvenanceCheck::Ok
    );
}

#[test]
fn check_provenance_warns_when_stricter_than_baseline() {
    // A soft check at headroom 0.90 (stricter) reading a baseline
    // written at 0.95 (looser) may under-cover: warn.
    assert_eq!(
        check_provenance(
            Provenance::soft_headroom(0.90),
            Some(Provenance::soft_headroom(0.95)),
        ),
        ProvenanceCheck::StricterThanBaseline {
            current: 0.90,
            baseline: 0.95,
        }
    );
}

#[test]
fn check_provenance_silent_when_equal() {
    assert_eq!(
        check_provenance(
            Provenance::soft_headroom(0.95),
            Some(Provenance::soft_headroom(0.95)),
        ),
        ProvenanceCheck::Ok
    );
    assert_eq!(
        check_provenance(Provenance::hard(), Some(Provenance::hard())),
        ProvenanceCheck::Ok
    );
}

#[test]
fn check_provenance_silent_when_baseline_absent() {
    // Pre-v5 baseline: provenance unknown, never warn (the v<VERSION
    // refresh hint already nudges the upgrade).
    assert_eq!(
        check_provenance(Provenance::hard(), None),
        ProvenanceCheck::Ok
    );
    assert_eq!(
        check_provenance(Provenance::soft_headroom(0.50), None),
        ProvenanceCheck::Ok
    );
}

#[test]
fn check_provenance_silent_when_either_side_is_soft_table() {
    // A soft-table baseline has no single ratio; skip the comparison
    // rather than guess (conservative — never false-fires).
    assert_eq!(
        check_provenance(
            Provenance::soft_headroom(0.50),
            Some(Provenance::soft_table())
        ),
        ProvenanceCheck::Ok
    );
    assert_eq!(
        check_provenance(
            Provenance::soft_table(),
            Some(Provenance::soft_headroom(0.95))
        ),
        ProvenanceCheck::Ok
    );
}
