// Sibling-file unit tests for `markdown_report::sections`, wired in
// via `#[path = "sections_tests.rs"] mod tests;`. The
// `./**/*_tests.rs` rule in `.bcaignore` keeps this file out of the
// self-scan walker so production-file metric caps stay tight.

#![allow(clippy::float_cmp)]

use super::*;
use big_code_analysis::LANG;

/// Build a minimal `FunctionSummary` with caller-supplied `(name, file,
/// start_line, metric)`. Every numeric field is set to the same `metric`
/// so a single argument exercises every per-section sort.
fn summary(name: &str, file: &str, start_line: usize, metric: f64) -> FunctionSummary {
    FunctionSummary {
        file: file.to_string(),
        name: name.to_string(),
        kind: SpaceKind::Function,
        language: LANG::Rust,
        start_line,
        end_line: start_line + 10,
        sloc: metric as usize,
        ploc: metric as usize,
        lloc: metric as usize,
        cloc: 0,
        tokens: 30,
        cyclomatic: metric,
        cognitive: metric,
        halstead_volume: metric,
        halstead_difficulty: metric,
        halstead_effort: metric,
        halstead_bugs: 0.1,
        halstead_time: 28.0,
        mi_original: 80.0,
        mi_sei: 85.0,
        mi_visual_studio: 50.0,
        nargs: metric as usize,
        nexits: metric as usize,
        nom: 1,
        abc: metric,
        wmc: metric,
        npa: 0.0,
        npm: 0.0,
    }
}

#[test]
fn top_n_desc_returns_none_when_filter_drops_every_entry() {
    let entries = [summary("a", "f.rs", 1, 5.0)];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    assert!(top_n_desc(&refs, 10, |_| false, |s| s.cyclomatic).is_none());
}

#[test]
fn top_n_desc_returns_some_empty_when_top_n_is_zero() {
    // Pre-refactor behaviour: heading + empty table. Helper must
    // return `Some(empty)` (not `None`) so the caller still emits the
    // heading for compatibility — even though CLI clamps `--top` at
    // `range(1..)`, the contract is exercised by programmatic callers.
    let entries = [summary("a", "f.rs", 1, 5.0)];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    let got = top_n_desc(&refs, 0, |_| true, |s| s.cyclomatic);
    assert_eq!(got.as_deref().map(<[_]>::len), Some(0));
}

#[test]
fn top_n_desc_top_n_larger_than_len_returns_all_sorted() {
    let entries = [
        summary("low", "f.rs", 1, 1.0),
        summary("high", "f.rs", 2, 10.0),
        summary("mid", "f.rs", 3, 5.0),
    ];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    let got = top_n_desc(&refs, 100, |_| true, |s| s.cyclomatic).expect("Some");
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].name, "high");
    assert_eq!(got[1].name, "mid");
    assert_eq!(got[2].name, "low");
}

#[test]
fn top_n_desc_top_n_less_than_len_keeps_only_highest_n() {
    let entries = [
        summary("low", "f.rs", 1, 1.0),
        summary("high", "f.rs", 2, 10.0),
        summary("mid", "f.rs", 3, 5.0),
    ];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    let got = top_n_desc(&refs, 2, |_| true, |s| s.cyclomatic).expect("Some");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "high");
    assert_eq!(got[1].name, "mid");
}

#[test]
fn top_n_desc_breaks_ties_by_file_then_line_then_name() {
    // All four entries have the same metric value, so the comparator
    // falls through to file (asc) → start_line (asc) → name (asc).
    // The select_nth_unstable_by partition could otherwise pick any
    // arbitrary subset of ties.
    let entries = [
        summary("z", "b.rs", 1, 5.0),
        summary("y", "a.rs", 2, 5.0),
        summary("x", "a.rs", 1, 5.0),
        summary("w", "a.rs", 1, 5.0),
    ];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    let got = top_n_desc(&refs, 4, |_| true, |s| s.cyclomatic).expect("Some");
    // a.rs:1 entries come first (tied), name-ordered: w then x.
    assert_eq!(got[0].file, "a.rs");
    assert_eq!(got[0].start_line, 1);
    assert_eq!(got[0].name, "w");
    assert_eq!(got[1].file, "a.rs");
    assert_eq!(got[1].start_line, 1);
    assert_eq!(got[1].name, "x");
    // Then a.rs:2.
    assert_eq!(got[2].file, "a.rs");
    assert_eq!(got[2].start_line, 2);
    // Then b.rs.
    assert_eq!(got[3].file, "b.rs");
}

#[test]
fn top_n_desc_applies_filter_before_sort() {
    // Filter drops the lowest entry; the helper must NOT include it in
    // the returned set even if it would have fit under `top_n`.
    let entries = [
        summary("low", "f.rs", 1, 1.0),
        summary("high", "f.rs", 2, 10.0),
        summary("mid", "f.rs", 3, 5.0),
    ];
    let refs: Vec<&FunctionSummary> = entries.iter().collect();
    let got = top_n_desc(&refs, 10, |s| s.cyclomatic > 3.0, |s| s.cyclomatic).expect("Some");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "high");
    assert_eq!(got[1].name, "mid");
}
