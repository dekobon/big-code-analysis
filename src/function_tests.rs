// Sibling-file unit tests for `dump_spans` / `dump_span`, wired in via
// `#[path = "function_tests.rs"] mod tests;` so the production
// `function.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use super::*;
use std::path::PathBuf;
use termcolor::NoColor;

fn make_span(name: &str, start: usize, end: usize) -> FunctionSpan {
    FunctionSpan {
        name: name.to_owned(),
        start_line: start,
        end_line: end,
        error: false,
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
