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
    Baseline::from_str(text, test_anchor())
}

// -- parsing / loading -------------------------------------------------

#[test]
fn parse_minimal_version_only() {
    let b = parse("version = 2\n").expect("minimal parse");
    assert_eq!(b.by_key.len(), 0);
}

#[test]
fn parse_round_trip_preserves_entries() {
    let original = from_violations(
        vec![
            v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
            v("src/b.rs", "bar", 20, "cognitive", 7.0),
        ],
        test_anchor(),
    );
    let rendered = render(&original).expect("render");
    let reloaded = parse(&rendered).expect("reload");
    assert_eq!(reloaded.by_key.len(), 2);
    let v_now = v("src/a.rs", "foo", 10, "cyclomatic", 5.0);
    assert!(matches!(
        reloaded.classify(&v_now),
        Coverage::Covered { recorded } if recorded == 5.0
    ));
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
    assert_eq!(b.by_key.len(), 0);
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
    assert_eq!(b.by_key.len(), 1);
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
    assert_eq!(b.by_key.len(), 1);
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
    assert_eq!(b.by_key.len(), 1);
}

// -- from_violations ---------------------------------------------------

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
    );
    assert_eq!(file.entries.len(), 1);
    assert_eq!(file.entries[0].function, "i");
}

#[test]
fn from_violations_deterministic_order() {
    // Inputs are crafted so every tiebreaker in the
    // (path, start_line, function, metric) sort is the deciding
    // comparator for at least one adjacent pair in the output:
    //
    //   [0] vs [1]: same path + start_line + function -> metric breaks tie
    //   [1] vs [2]: same path + start_line, different function
    //               -> function breaks tie
    //   [2] vs [3]: same path, different start_line
    //               -> start_line breaks tie
    //   [3] vs [4]: different path -> path breaks tie
    let unsorted = vec![
        v("src/z.rs", "z", 100, "cyclomatic", 5.0),
        v("src/a.rs", "b", 10, "cognitive", 4.0),
        v("src/a.rs", "a", 10, "cognitive", 3.0),
        v("src/a.rs", "a", 10, "cyclomatic", 5.0),
        v("src/a.rs", "a", 99, "cyclomatic", 6.0),
    ];
    let file = from_violations(unsorted, test_anchor());
    assert_eq!(file.entries[0].path, "src/a.rs");
    assert_eq!(file.entries[0].start_line, 10);
    assert_eq!(file.entries[0].function, "a");
    assert_eq!(file.entries[0].metric, "cognitive");
    assert_eq!(file.entries[1].path, "src/a.rs");
    assert_eq!(file.entries[1].start_line, 10);
    assert_eq!(file.entries[1].function, "a");
    assert_eq!(file.entries[1].metric, "cyclomatic");
    assert_eq!(file.entries[2].path, "src/a.rs");
    assert_eq!(file.entries[2].start_line, 10);
    assert_eq!(file.entries[2].function, "b");
    assert_eq!(file.entries[3].path, "src/a.rs");
    assert_eq!(file.entries[3].start_line, 99);
    assert_eq!(file.entries[4].path, "src/z.rs");
}

#[test]
fn from_violations_byte_equal_across_two_calls() {
    let input = vec![
        v("src/a.rs", "foo", 10, "cyclomatic", 5.0),
        v("src/b.rs", "bar", 20, "cognitive", 7.0),
    ];
    let a = render(&from_violations(input.clone(), test_anchor())).expect("render a");
    let b = render(&from_violations(input, test_anchor())).expect("render b");
    assert_eq!(a, b);
}

#[test]
fn path_normalized_forward_slash_on_serialize() {
    // Construct a Violation with a backslash path directly (so the
    // test passes on any host).
    let file = from_violations(
        vec![v("a\\b\\c.rs", "f", 1, "cyclomatic", 5.0)],
        test_anchor(),
    );
    assert_eq!(file.entries[0].path, "a/b/c.rs");
}

// -- covers ------------------------------------------------------------

fn baseline_with(entries: Vec<BaselineEntry>) -> Baseline {
    let file = BaselineFile {
        version: Some(BASELINE_VERSION),
        entries,
    };
    let text = render(&file).expect("render");
    Baseline::from_str(&text, test_anchor()).expect("parse")
}

fn entry(path: &str, function: &str, start_line: usize, metric: &str, value: f64) -> BaselineEntry {
    BaselineEntry {
        path: path.to_string(),
        function: function.to_string(),
        start_line,
        metric: metric.to_string(),
        value,
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
fn classify_different_start_line_is_new() {
    let b = baseline_with(vec![entry("a", "f", 1, "cyclomatic", 5.0)]);
    assert!(matches!(
        b.classify(&v("a", "f", 2, "cyclomatic", 5.0)),
        Coverage::New
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

// -- anchor + lexical normalisation (issue #376) ----------------------

#[test]
fn lexical_normalize_folds_curdir_and_parent() {
    assert_eq!(lexical_normalize(Path::new("./a/b")), Path::new("a/b"));
    assert_eq!(lexical_normalize(Path::new("a/./b")), Path::new("a/b"));
    assert_eq!(lexical_normalize(Path::new("a/b/../c")), Path::new("a/c"));
    assert_eq!(
        lexical_normalize(Path::new("a/b/c/../../d")),
        Path::new("a/d")
    );
}

#[test]
fn lexical_normalize_preserves_escaping_parents() {
    // `..` past every accumulated Normal component is preserved so
    // an entry that genuinely lives one level above the anchor
    // (e.g., a sibling-crate analysis) still has an identity.
    assert_eq!(lexical_normalize(Path::new("../a")), Path::new("../a"));
    assert_eq!(lexical_normalize(Path::new("a/../../b")), Path::new("../b"));
}

#[cfg(unix)]
#[test]
fn anchor_for_strips_baseline_filename() {
    // `anchor_for` is lexical-only — no filesystem access — so the
    // assertion can be a pure path comparison against synthetic
    // input. Pinning to a fixed prefix keeps the test independent
    // of `$TMPDIR` shape across CI hosts.
    assert_eq!(
        anchor_for(Path::new("/tmp/bca-anchor-test/baseline.toml")),
        Path::new("/tmp/bca-anchor-test"),
    );
}

#[cfg(unix)]
#[test]
fn normalize_path_canonicalises_against_anchor() {
    // Three distinct typings of the same file under one anchor must
    // collapse to the same key.
    let anchor = Path::new("/repo");
    let key_dot = normalize_path(anchor, Path::new("/repo/src/foo.rs"));
    let key_rel = normalize_path(anchor, Path::new("src/./foo.rs"));
    let key_parent = normalize_path(anchor, Path::new("src/x/../foo.rs"));
    assert_eq!(key_dot, "src/foo.rs");
    assert_eq!(key_rel, "src/foo.rs");
    assert_eq!(key_parent, "src/foo.rs");
}

#[cfg(unix)]
#[test]
fn normalize_path_outside_anchor_uses_absolute_form() {
    // A path that isn't under the anchor keeps its absolute form
    // rather than degrading to `../` chains. Legitimate use case:
    // a baseline at the repo root recording offenders from a
    // sibling vendored crate kept outside the tree.
    let key = normalize_path(Path::new("/repo"), Path::new("/elsewhere/file.rs"));
    assert_eq!(key, "/elsewhere/file.rs");
}

// -- non-UTF-8 path identity ------------------------------------------

#[test]
fn normalize_path_utf8_unchanged_for_unreserved_ascii() {
    // Regression guard: the common UTF-8 case (all-unreserved-ASCII
    // path components) must round-trip untouched. Non-UTF-8
    // encoding shenanigans must not leak into ordinary inputs (no
    // unexpected percent escapes, no extra markers).
    assert_eq!(
        normalize_path(test_anchor(), Path::new("src/foo.rs")),
        "src/foo.rs"
    );
    assert_eq!(
        normalize_path(test_anchor(), Path::new("crates/a/b.rs")),
        "crates/a/b.rs"
    );
    // Backslashes are still normalized to forward slashes for the
    // UTF-8 path so that cross-OS baselines match.
    assert_eq!(
        normalize_path(test_anchor(), Path::new("a\\b\\c.rs")),
        "a/b/c.rs"
    );
}

#[test]
fn normalize_path_utf8_escapes_percent() {
    // `%` must be escaped in the UTF-8 fast path so it cannot collide
    // with a non-UTF-8 byte's `%XX` escape. See `normalize_path_utf8_
    // non_utf8_byte_no_collision` for the actual collision check.
    assert_eq!(
        normalize_path(test_anchor(), Path::new("foo%FF.rs")),
        "foo%25FF.rs"
    );
    assert_eq!(
        normalize_path(test_anchor(), Path::new("a%b%c.rs")),
        "a%25b%25c.rs"
    );
}

#[cfg(unix)]
#[test]
fn normalize_path_utf8_percent_vs_non_utf8_byte_no_collision() {
    // The bug: a UTF-8 path containing the literal text `%FF` and a
    // non-UTF-8 path containing the byte `0xFF` at the same position
    // used to normalize to the same key (both `foo%FF.rs`), so a
    // baseline written for one silently covered violations from the
    // other. With `%` percent-encoded on the UTF-8 side, the keys
    // diverge.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let utf8 = Path::new("foo%FF.rs");
    let non_utf8 = PathBuf::from(OsStr::from_bytes(b"foo\xff.rs"));
    let key_utf8 = normalize_path(test_anchor(), utf8);
    let key_non_utf8 = normalize_path(test_anchor(), &non_utf8);
    assert_eq!(key_utf8, "foo%25FF.rs");
    assert_eq!(key_non_utf8, "foo%FF.rs");
    assert_ne!(key_utf8, key_non_utf8);
}

#[cfg(unix)]
#[test]
fn baseline_key_preserves_non_utf8_identity() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    // Two distinct non-UTF-8 paths must produce two distinct
    // baseline keys. The previous `display().to_string()` fallback
    // collapsed both onto a sequence of U+FFFD replacement chars,
    // so a baseline written from path A would silently cover
    // violations from path B.
    let a = PathBuf::from("src").join(OsStr::from_bytes(b"bad-\xff\xfe.rs"));
    let b = PathBuf::from("src").join(OsStr::from_bytes(b"bad-\xfe\xff.rs"));
    let key_a = normalize_path(test_anchor(), &a);
    let key_b = normalize_path(test_anchor(), &b);
    assert_ne!(key_a, key_b);
    // The encoded keys are valid UTF-8 (required by TOML) and
    // contain only ASCII bytes after percent-encoding.
    assert!(key_a.is_ascii());
    assert!(key_b.is_ascii());
}

// -- WTF-16 percent-encoding (always-on, synthetic input) ------------

#[test]
fn wtf16_encode_pure_ascii() {
    // ASCII path bytes are unreserved, so they survive unchanged.
    let out = percent_encode_wtf16("src/foo.rs".encode_utf16());
    assert_eq!(out, "src/foo.rs");
}

#[test]
fn wtf16_encode_empty() {
    assert_eq!(percent_encode_wtf16(std::iter::empty::<u16>()), "");
}

#[test]
fn wtf16_encode_bmp_non_ascii() {
    // U+00E9 (é) is BMP; UTF-8 = 0xC3 0xA9; both bytes are
    // non-unreserved and percent-encode to %C3%A9.
    let out = percent_encode_wtf16("é".encode_utf16());
    assert_eq!(out, "%C3%A9");
}

#[test]
fn wtf16_encode_supplementary_plane() {
    // U+1F600 (😀) requires a surrogate pair in WTF-16
    // (0xD83D, 0xDE00) and UTF-8-encodes as 0xF0 0x9F 0x98 0x80.
    // `char::decode_utf16` pairs the surrogates back to the scalar,
    // so the encoder must emit the UTF-8 byte form.
    let units = [0xD83D_u16, 0xDE00_u16];
    let out = percent_encode_wtf16(units);
    assert_eq!(out, "%F0%9F%98%80");
    // Sanity: the same character entered as a string round-trips
    // identically through `encode_utf16`.
    assert_eq!(out, percent_encode_wtf16("😀".encode_utf16()));
}

#[test]
fn wtf16_encode_unpaired_high_surrogate() {
    let out = percent_encode_wtf16([0xD83D_u16]);
    assert_eq!(out, "%uD83D");
}

#[test]
fn wtf16_encode_unpaired_low_surrogate() {
    // A lone low surrogate (no preceding high) is unpaired.
    let out = percent_encode_wtf16([0xDE00_u16]);
    assert_eq!(out, "%uDE00");
}

#[test]
fn wtf16_encode_high_followed_by_non_low_is_unpaired() {
    // High surrogate followed by ASCII: the high is unpaired and
    // the ASCII byte is encoded normally afterwards.
    let units = [0xD83D_u16, u16::from(b'x')];
    let out = percent_encode_wtf16(units);
    assert_eq!(out, "%uD83Dx");
}

#[test]
fn wtf16_encode_leading_low_then_pair() {
    // A lone low surrogate followed by a real pair: the leading low
    // must not consume the next code unit (the high of the pair).
    let units = [0xDC00_u16, 0xD83D_u16, 0xDE00_u16];
    let out = percent_encode_wtf16(units);
    assert_eq!(out, "%uDC00%F0%9F%98%80");
}

#[test]
fn wtf16_encode_distinct_unpaired_surrogates_do_not_collide() {
    // The whole point of the fix: two distinct invalid WTF-16
    // sequences that `to_string_lossy()` would have collapsed onto
    // a single U+FFFD must produce two distinct encoded keys.
    let a = percent_encode_wtf16([0xD83D_u16]);
    let b = percent_encode_wtf16([0xDE00_u16]);
    assert_ne!(a, b);
    // And two different lone high surrogates also separate cleanly.
    let c = percent_encode_wtf16([0xD800_u16]);
    let d = percent_encode_wtf16([0xDBFF_u16]);
    assert_ne!(c, d);
}

#[test]
fn wtf16_encode_marker_never_emitted_by_scalar_bytes() {
    // Regression guard: the byte encoder only emits `%` followed by
    // exactly two uppercase hex digits, never `%u`. Scalars cannot
    // produce a string that begins with `%u` from their UTF-8 bytes
    // — `u` is unreserved, so it stays as `u`, but the preceding
    // `%` only appears when a non-unreserved byte is escaped (and
    // is then immediately followed by two hex digits, not `u`).
    // Therefore parsing `%u…` is unambiguous.
    for codepoint in ['u', '%', '!', '\u{00E9}', '\u{1F600}'] {
        let s = codepoint.to_string();
        let out = percent_encode_wtf16(s.encode_utf16());
        assert!(!out.contains("%u"), "scalar {codepoint:?} produced {out:?}");
    }
}

#[cfg(windows)]
#[test]
fn baseline_key_preserves_non_utf16_identity_on_windows() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    // Two distinct paths that differ only by an unpaired surrogate
    // value would collapse to the same `to_string_lossy()` key
    // (both surrogates become U+FFFD). With the WTF-16 encoder they
    // stay distinct.
    let a_units: [u16; 5] = [
        u16::from(b'a'),
        u16::from(b'/'),
        0xD83D,
        u16::from(b'.'),
        u16::from(b's'),
    ];
    let b_units: [u16; 5] = [
        u16::from(b'a'),
        u16::from(b'/'),
        0xDE00,
        u16::from(b'.'),
        u16::from(b's'),
    ];
    let path_a = PathBuf::from(OsString::from_wide(&a_units));
    let path_b = PathBuf::from(OsString::from_wide(&b_units));
    let key_a = normalize_path(test_anchor(), &path_a);
    let key_b = normalize_path(test_anchor(), &path_b);
    assert_ne!(key_a, key_b);
    assert!(key_a.is_ascii());
    assert!(key_b.is_ascii());
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
    };
    let violation_b = Violation {
        path: path_b,
        start_line: 1,
        end_line: 2,
        function: "f".to_string(),
        metric: "cyclomatic",
        value: 5.0,
        limit: 1.0,
    };

    // Baseline contains only `path_a`. classify(violation_b) would
    // wrongly return Covered if both non-UTF-8 paths normalized
    // to the same lossy key.
    let file = from_violations(vec![violation_a.clone()], test_anchor());
    let rendered = render(&file).expect("render");
    let b = Baseline::from_str(&rendered, test_anchor()).expect("parse");
    assert!(matches!(b.classify(&violation_a), Coverage::Covered { .. }));
    assert!(matches!(b.classify(&violation_b), Coverage::New));
}
