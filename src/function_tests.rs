// Unit tests for `dump_spans`/`dump_span`. Lives in a sibling file and
// is wired in via `#[path = "function_tests.rs"] mod tests;` so the
// production `function.rs` stays under the `bca check` per-file
// metric caps (issue #357 P3). Added to `.bcaignore` alongside the
// rest of the workspace's test files so the self-scan walker skips
// it the same way it skips the `./tests/` directory.

use super::*;
use std::path::PathBuf;

fn make_span(name: &str, start: usize, end: usize) -> FunctionSpan {
    FunctionSpan {
        name: name.to_owned(),
        start_line: start,
        end_line: end,
        error: false,
    }
}

// `dump_spans` writes to the real stdout via `StandardStream`, so we
// cannot capture output here. These tests exercise the index-based
// dispatch added when the `drain + pop().unwrap()` pair was replaced
// with `into_iter().enumerate()`: an off-by-one in the new code
// would still terminate but is most likely to surface as a panic
// (empty `Vec` -> `len() - 1` underflow) for the n=0 case the outer
// guard protects.

#[test]
fn dump_spans_empty_is_ok() {
    // Reverting the `is_empty` guard would underflow `spans.len() - 1`
    // and panic on subtract-with-overflow in debug builds.
    let result = dump_spans(Vec::new(), PathBuf::from("/tmp/empty.rs"));
    assert!(result.is_ok());
}

#[test]
fn dump_spans_single_span_is_ok() {
    // n=1 is the subtle path: `last_idx = 0`, the loop runs once
    // with `i == last_idx` so the single span is marked as last.
    let result = dump_spans(
        vec![make_span("only", 1, 5)],
        PathBuf::from("/tmp/single.rs"),
    );
    assert!(result.is_ok());
}

#[test]
fn dump_spans_many_spans_is_ok() {
    // n>1 exercises the `i != last_idx` branch (non-final entries)
    // followed by the final `i == last_idx` entry.
    let spans = vec![
        make_span("a", 1, 5),
        make_span("b", 7, 12),
        make_span("c", 14, 20),
    ];
    let result = dump_spans(spans, PathBuf::from("/tmp/many.rs"));
    assert!(result.is_ok());
}
