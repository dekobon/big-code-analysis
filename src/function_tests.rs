// Sibling-file unit tests for `dump_spans` / `dump_span`, wired in via
// `#[path = "function_tests.rs"] mod tests;` so the production
// `function.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use super::*;
use std::path::PathBuf;
use termcolor::{Ansi, NoColor};

fn make_span(name: &str, start: usize, end: usize) -> FunctionSpan {
    FunctionSpan {
        name: name.to_owned(),
        start_line: start,
        end_line: end,
        error: false,
    }
}

// Render a single `dump_span` invocation into a `termcolor::Ansi` buffer.
// `Ansi` writes ANSI escape sequences inline regardless of the surrounding
// terminal, so the byte sequence is reproducible across hosts and pins the
// exact color-then-text layout the segment-table refactor must preserve.
fn dump_span_ansi(span: FunctionSpan, last: bool) -> String {
    let mut buf = Ansi::new(Vec::<u8>::new());
    dump_span(span, &mut buf, last).expect("write to in-memory buffer");
    String::from_utf8(buf.into_inner()).expect("UTF-8")
}

fn assert_segments_in_order(out: &str, segments: &[&str]) {
    let mut cursor = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        let found = out[cursor..]
            .find(seg)
            .unwrap_or_else(|| panic!("segment {i} {seg:?} missing from output: {out:?}"))
            + cursor;
        if i > 0 {
            // Each segment after the first must be preceded by at least one
            // ANSI color escape (`\x1b[`). A refactor that drops a color()
            // call leaves two text fragments touching without a code in
            // between — this assertion catches it.
            assert!(
                out[cursor..found].contains("\x1b["),
                "missing color escape before segment {i} {seg:?}: gap was {gap:?}",
                gap = &out[cursor..found],
            );
        }
        cursor = found + seg.len();
    }
}

// Render `dump_spans` into an in-memory `NoColor` buffer so tests can
// assert on the rendered text. `NoColor<Vec<u8>>` implements
// `WriteColor` but discards `set_color` calls, so the bytes are pure
// text — exactly the surface we want to pin against an off-by-one in
// the `i == last_idx` comparison that picks the prefix glyph.
fn render(spans: Vec<FunctionSpan>, path: &str) -> String {
    let mut buf = NoColor::new(Vec::<u8>::new());
    dump_spans(spans, &PathBuf::from(path), &mut buf).expect("write to in-memory buffer");
    String::from_utf8(buf.into_inner()).expect("UTF-8")
}

const LAST_PREFIX: &str = "   `- ";
const MID_PREFIX: &str = "   |- ";

#[test]
fn dump_spans_empty_writes_nothing() {
    // Reverting the `is_empty` guard would underflow `spans.len() - 1`
    // and panic on subtract-with-overflow in debug builds; assert
    // additionally that no header / prefix is emitted for n=0.
    let out = render(Vec::new(), "/tmp/empty.rs");
    assert!(
        out.is_empty(),
        "no output expected for empty spans, got {out:?}"
    );
}

#[test]
fn dump_spans_single_span_uses_last_prefix() {
    // n=1 is the subtle path: `last_idx = 0`, the loop runs once
    // with `i == last_idx` so the single span is marked as last.
    // An inverted comparison (`i != last_idx`) would emit the
    // mid-prefix instead — caught by the prefix-count assertions.
    let out = render(vec![make_span("only", 1, 5)], "/tmp/single.rs");

    assert!(
        out.contains("In file /tmp/single.rs"),
        "header missing: {out:?}",
    );
    assert_eq!(
        out.matches(LAST_PREFIX).count(),
        1,
        "single span must use last-prefix exactly once: {out:?}",
    );
    assert_eq!(
        out.matches(MID_PREFIX).count(),
        0,
        "single span must not use mid-prefix: {out:?}",
    );
    assert!(
        out.contains("only: from line 1 to line 5."),
        "span body missing: {out:?}",
    );
}

#[test]
fn dump_spans_many_spans_marks_only_last() {
    // n>1 exercises the `i != last_idx` branch (non-final entries)
    // followed by the final `i == last_idx` entry. An inverted
    // comparison would flip the prefix counts to (mid=1, last=2)
    // and reorder which span sits under each prefix.
    let spans = vec![
        make_span("a", 1, 5),
        make_span("b", 7, 12),
        make_span("c", 14, 20),
    ];
    let out = render(spans, "/tmp/many.rs");

    assert_eq!(
        out.matches(LAST_PREFIX).count(),
        1,
        "exactly one last-prefix expected for n=3: {out:?}",
    );
    assert_eq!(
        out.matches(MID_PREFIX).count(),
        2,
        "exactly two mid-prefixes expected for n=3: {out:?}",
    );

    // Order matters: the last-prefix line must be the final span (`c`),
    // not the first. This pins the `i == last_idx` direction.
    let last_line = out
        .lines()
        .find(|line| line.starts_with(LAST_PREFIX))
        .expect("a line with last-prefix");
    assert!(
        last_line.contains("c: from line 14 to line 20."),
        "last-prefix line should belong to the final span `c`, got: {last_line:?}",
    );
}

#[test]
fn dump_span_ansi_layout_non_error_branch() {
    // Pins both the text segments *and* the color-then-text layout
    // that the segment-table refactor must preserve byte-for-byte.
    // A reordering of segments (e.g. start_line ↔ end_line) makes
    // `find` skip past the misplaced fragment and fail. A dropped
    // `color()` call leaves two text fragments adjacent and trips
    // the gap-must-contain-escape check.
    let out = dump_span_ansi(make_span("hello", 1, 5), false);

    assert!(
        out.starts_with("\x1b["),
        "must begin with a color escape: {out:?}"
    );
    assert!(out.ends_with("5.\n"), "trailing newline missing: {out:?}");
    assert_segments_in_order(
        &out,
        &["   |- ", "hello: ", "from line ", "1", " to line ", "5.\n"],
    );
}

#[test]
fn dump_span_ansi_layout_error_branch() {
    // The error branch substitutes the span name with the literal
    // "error: " and must not leak the name. `last = true` selects
    // the last-prefix glyph. Same color-then-text invariant as the
    // non-error branch.
    let span = FunctionSpan {
        name: "should-not-appear".into(),
        start_line: 7,
        end_line: 8,
        error: true,
    };
    let out = dump_span_ansi(span, true);

    assert!(
        out.starts_with("\x1b["),
        "must begin with a color escape: {out:?}"
    );
    assert!(out.ends_with("8.\n"), "trailing newline missing: {out:?}");
    assert!(
        !out.contains("should-not-appear"),
        "error branch must not include span name: {out:?}",
    );
    assert_segments_in_order(
        &out,
        &["   `- ", "error: ", "from line ", "7", " to line ", "8.\n"],
    );
}
