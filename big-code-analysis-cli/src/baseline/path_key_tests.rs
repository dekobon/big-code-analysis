// Sibling-file unit tests for baseline path-key canonicalisation and
// percent-encoding, wired in via `#[path = "path_key_tests.rs"] mod
// tests;` so the production `path_key.rs` stays under the `bca check`
// per-file metric caps. Matched by the `./**/*_tests.rs` rule in
// `.bcaignore`, so the self-scan walker skips this file the same way
// it skips `./tests/`.

use super::*;

/// Canonical empty anchor for unit tests: the violation path is keyed
/// as-passed without prepending a synthetic CWD. Real callers always
/// derive their anchor via [`anchor_for`] from the baseline file path,
/// but for the in-memory tests in this file an empty anchor preserves
/// the pre-#376 semantics of "key on the literal path string the test
/// supplied" while still exercising the new lexical normalisation.
fn test_anchor() -> &'static Path {
    Path::new("")
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
fn lexical_normalize_folds_parent_past_root() {
    // POSIX: `..` immediately after a RootDir is a no-op. Before the
    // fix the function preserved `..` literally, yielding `/..` —
    // non-canonical and `strip_prefix` would not match a canonical
    // anchor. A hand-crafted v2 entry like `path = "/../etc/passwd"`
    // could exploit this to produce keys that bypass anchor
    // relativisation; the fold keeps the encoder's output canonical.
    assert_eq!(lexical_normalize(Path::new("/..")), Path::new("/"));
    assert_eq!(lexical_normalize(Path::new("/../..")), Path::new("/"));
    assert_eq!(
        lexical_normalize(Path::new("/../etc/passwd")),
        Path::new("/etc/passwd")
    );
    // Mixing: Normal pop still works after a root-fold no-op.
    assert_eq!(
        lexical_normalize(Path::new("/foo/../../bar")),
        Path::new("/bar")
    );
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
