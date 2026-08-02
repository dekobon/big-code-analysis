// Sibling-file unit tests for `bare_name` and the body-hash helpers,
// wired in via `#[path = "body_hash_tests.rs"] mod tests;` so the
// production `body_hash.rs` stays under the `bca check` per-file
// metric caps. Matched by the `./**/*_tests.rs` rule in `.bcaignore`,
// so the self-scan walker skips this file the same way it skips
// `./tests/`.

use super::*;

#[test]
fn bare_name_strips_qualifier() {
    assert_eq!(bare_name("MyStruct::do_thing"), "do_thing");
    assert_eq!(bare_name("a::b::c"), "c");
    assert_eq!(bare_name("plain"), "plain");
    assert_eq!(bare_name("<file>"), "<file>");
}

#[test]
fn body_hash_ignores_indentation_blank_lines_and_run_width() {
    // The normalisation trims leading/trailing whitespace, collapses
    // internal whitespace *runs* to one space, drops `\r`, and skips
    // blank lines — so re-indenting, reflowing blank lines, or changing
    // CRLF/LF must not change the digest. (It is not insensitive to the
    // presence/absence of whitespace *between* tokens, only its width.)
    let original = b"    let x = 1;\n    return x + 1;\n";
    let reformatted = b"\nlet   x = 1;\r\n\n        return x  +  1;\n\n";
    assert_eq!(
        hash_body(original, 1, 2, ""),
        hash_body(reformatted, 1, 6, "")
    );
}

#[test]
fn body_hash_distinguishes_different_bodies() {
    assert_ne!(
        hash_body(b"let x = 1;", 1, 1, ""),
        hash_body(b"let x = 2;", 1, 1, "")
    );
}

#[test]
fn body_hash_respects_line_range() {
    // Lines 2..=2 of a three-line body hash only the middle line.
    let src = b"line one\nline two\nline three\n";
    assert_eq!(hash_body(src, 2, 2, ""), hash_body(b"line two", 1, 1, ""));
}

#[test]
fn body_hash_out_of_range_is_empty_digest() {
    // A start past EOF yields the empty-body digest (the FNV offset
    // basis) rather than panicking on an out-of-bounds slice.
    let src = b"only one line";
    assert_eq!(hash_body(src, 100, 200, ""), hash_body(b"", 1, 1, ""));
}

#[test]
fn body_hash_elides_own_name_so_rename_matches() {
    // The headline rule-3 property: renaming the function (declaration
    // and recursive self-calls) leaves the digest unchanged, because the
    // bare name is elided.
    let before = b"fn classify(n: i32) -> i32 { classify(n - 1) }";
    let after = b"fn categorize(n: i32) -> i32 { categorize(n - 1) }";
    assert_eq!(
        hash_body(before, 1, 1, "classify"),
        hash_body(after, 1, 1, "categorize")
    );
}

#[test]
fn body_hash_elision_is_whole_word_only() {
    // Eliding `is` must not corrupt the substring inside `is_valid` —
    // two bodies that differ only in an unrelated identifier sharing the
    // elided prefix must still hash differently.
    let a = b"fn is() { is_valid() }";
    let b = b"fn is() { is_ready() }";
    assert_ne!(hash_body(a, 1, 1, "is"), hash_body(b, 1, 1, "is"));
}

#[test]
fn body_hash_round_trips_through_hex_codec() {
    let h = hash_body(b"some body text", 1, 1, "");
    assert_eq!(decode_body_hash(&encode_body_hash(h)), Some(h));
}

#[test]
fn decode_body_hash_rejects_malformed() {
    assert_eq!(decode_body_hash("not-hex"), None);
    assert_eq!(decode_body_hash("dead"), None); // too short
    assert_eq!(decode_body_hash(""), None);
    assert_eq!(
        decode_body_hash("0123456789abcdef"),
        Some(0x0123_4567_89ab_cdef)
    );
}
