// Sibling-file unit tests for the `baseline_diff` module. Wired via
// `#[path = "baseline_diff_tests.rs"] mod tests;`. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`.

use super::*;

fn entry(path: &str, qualified: &str, metric: &str, start_line: usize, value: f64) -> DiffEntry {
    DiffEntry {
        path: path.to_string(),
        qualified: qualified.to_string(),
        metric: metric.to_string(),
        start_line,
        value,
    }
}

/// Filter that shows everything (the default with no `--*-only` flag).
fn all() -> SectionFilter {
    SectionFilter::from_flags([false, false, false, false])
}

#[test]
fn added_entry_is_bucketed_as_added() {
    let old = vec![];
    let new = vec![entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0)];
    let diff = BaselineDiff::compute(&old, &new);
    assert_eq!(diff.added.len(), 1);
    assert!(diff.removed.is_empty());
    assert!(diff.worsened.is_empty());
    assert!(diff.improved.is_empty());
    assert_eq!(diff.added[0].qualified, "do_thing");
    assert_eq!(diff.added[0].value, 27.0);
}

#[test]
fn removed_entry_is_bucketed_as_removed() {
    let old = vec![entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0)];
    let new = vec![];
    let diff = BaselineDiff::compute(&old, &new);
    assert_eq!(diff.removed.len(), 1);
    assert!(diff.added.is_empty());
    assert_eq!(diff.removed[0].value, 27.0);
}

#[test]
fn higher_value_is_worsened_lower_is_improved() {
    let old = vec![
        entry("src/bar.rs", "act", "cognitive", 5, 60.0),
        entry("src/baz.rs", "resolve", "cognitive", 8, 41.0),
    ];
    let new = vec![
        entry("src/bar.rs", "act", "cognitive", 5, 63.0),
        entry("src/baz.rs", "resolve", "cognitive", 8, 33.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    assert_eq!(diff.worsened.len(), 1);
    assert_eq!(diff.improved.len(), 1);
    assert_eq!(diff.worsened[0].qualified, "act");
    assert_eq!(diff.worsened[0].old, 60.0);
    assert_eq!(diff.worsened[0].new, 63.0);
    assert_eq!(diff.improved[0].qualified, "resolve");
    assert_eq!(diff.improved[0].old, 41.0);
    assert_eq!(diff.improved[0].new, 33.0);
}

#[test]
fn unchanged_value_is_omitted_from_all_buckets() {
    // Byte-identical re-baseline of unchanged code: the entry exists on
    // both sides with the same value, so it appears in no bucket.
    let old = vec![entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0)];
    let new = vec![entry("src/foo.rs", "do_thing", "cognitive", 12, 27.0)];
    let diff = BaselineDiff::compute(&old, &new);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.worsened.is_empty());
    assert!(diff.improved.is_empty());
}

#[test]
fn line_drift_does_not_re_key_an_entry() {
    // The identity omits start_line, so a function that moved down the
    // file (10 -> 200) with the same value is unchanged, not add+remove.
    let old = vec![entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0)];
    let new = vec![entry("src/foo.rs", "do_thing", "cognitive", 200, 30.0)];
    let diff = BaselineDiff::compute(&old, &new);
    assert!(diff.added.is_empty(), "line drift must not add");
    assert!(diff.removed.is_empty(), "line drift must not remove");
    assert_eq!(diff.worsened.len(), 1);
    assert_eq!(diff.worsened[0].new, 30.0);
}

#[test]
fn distinct_metrics_on_same_function_are_independent() {
    let old = vec![entry("src/q.rs", "pipeline", "cyclomatic", 1, 18.0)];
    let new = vec![
        entry("src/q.rs", "pipeline", "cyclomatic", 1, 19.0),
        entry("src/q.rs", "pipeline", "cognitive", 1, 12.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    assert_eq!(diff.worsened.len(), 1, "cyclomatic 18 -> 19");
    assert_eq!(diff.added.len(), 1, "cognitive is a new metric entry");
    assert_eq!(diff.added[0].metric, "cognitive");
}

#[test]
fn ambiguous_key_pairs_by_sorted_value() {
    // Two records share (path, qualified, metric): paired by sorted
    // value, the surplus on the larger side falls out as added/removed.
    let old = vec![
        entry("src/o.rs", "f", "cognitive", 10, 30.0),
        entry("src/o.rs", "f", "cognitive", 90, 40.0),
    ];
    let new = vec![
        entry("src/o.rs", "f", "cognitive", 10, 31.0),
        entry("src/o.rs", "f", "cognitive", 90, 45.0),
        entry("src/o.rs", "f", "cognitive", 150, 50.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    // 30->31 and 40->45 paired (both worsened); the third new (50) added.
    assert_eq!(diff.worsened.len(), 2);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.added[0].value, 50.0);
    assert!(diff.removed.is_empty());
}

#[test]
fn bucket_entries_are_sorted_by_identity() {
    // The diff is meant to be reviewable, so within a bucket entries
    // must come out in a deterministic (path, qualified, metric) order
    // regardless of input order. Feed three worsened entries shuffled.
    let old = vec![
        entry("src/z.rs", "z_fn", "cognitive", 1, 10.0),
        entry("src/a.rs", "a_fn", "cognitive", 1, 10.0),
        entry("src/m.rs", "m_fn", "cognitive", 1, 10.0),
    ];
    let new = vec![
        entry("src/z.rs", "z_fn", "cognitive", 1, 11.0),
        entry("src/a.rs", "a_fn", "cognitive", 1, 11.0),
        entry("src/m.rs", "m_fn", "cognitive", 1, 11.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    let paths: Vec<&str> = diff.worsened.iter().map(|w| w.path.as_str()).collect();
    assert_eq!(paths, ["src/a.rs", "src/m.rs", "src/z.rs"]);
}

#[test]
fn empty_diff_renders_only_summary_line() {
    let diff = BaselineDiff::compute(&[], &[]);
    let tty = diff.render_tty(all());
    assert_eq!(tty, "0 added, 0 removed, 0 worsened, 0 improved\n");
}

#[test]
fn summary_line_reports_full_counts() {
    let old = vec![entry("a.rs", "x", "cognitive", 1, 10.0)];
    let new = vec![
        entry("a.rs", "x", "cognitive", 1, 12.0),
        entry("b.rs", "y", "cognitive", 1, 5.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    let tty = diff.render_tty(all());
    assert!(
        tty.starts_with("1 added, 0 removed, 1 worsened, 0 improved\n"),
        "summary line wrong: {tty}"
    );
}

#[test]
fn tty_renders_arrow_for_value_changes_and_equals_for_added() {
    let old = vec![entry("src/bar.rs", "act", "cognitive", 5, 60.0)];
    let new = vec![
        entry("src/bar.rs", "act", "cognitive", 5, 63.0),
        entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0),
    ];
    let diff = BaselineDiff::compute(&old, &new);
    let tty = diff.render_tty(all());
    assert!(
        tty.contains("## Worsened"),
        "missing worsened header: {tty}"
    );
    assert!(tty.contains("## Added"), "missing added header: {tty}");
    assert!(
        tty.contains("src/bar.rs::act"),
        "missing worsened identity: {tty}"
    );
    assert!(tty.contains("60 \u{2192} 63"), "missing arrow row: {tty}");
    assert!(
        tty.contains("src/foo.rs::do_thing"),
        "missing added identity: {tty}"
    );
    assert!(tty.contains("= 27"), "missing added value: {tty}");
}

#[test]
fn integer_values_print_without_decimal() {
    let old = vec![];
    let new = vec![entry("a.rs", "x", "cognitive", 1, 27.0)];
    let tty = BaselineDiff::compute(&old, &new).render_tty(all());
    assert!(tty.contains("= 27"), "integer should print bare: {tty}");
    assert!(!tty.contains("= 27.0"), "no trailing .0: {tty}");
}

#[test]
fn filter_added_only_hides_other_sections() {
    let old = vec![entry("a.rs", "x", "cognitive", 1, 10.0)];
    let new = vec![
        entry("a.rs", "x", "cognitive", 1, 12.0), // worsened
        entry("b.rs", "y", "cognitive", 1, 5.0),  // added
    ];
    let diff = BaselineDiff::compute(&old, &new);
    let filter = SectionFilter::from_flags([true, false, false, false]);
    let tty = diff.render_tty(filter);
    assert!(tty.contains("## Added"), "added shown: {tty}");
    assert!(!tty.contains("## Worsened"), "worsened hidden: {tty}");
    // Summary line still reports the full counts.
    assert!(tty.starts_with("1 added, 0 removed, 1 worsened, 0 improved\n"));
}

#[test]
fn filter_flags_are_combinable() {
    let old = vec![entry("a.rs", "x", "cognitive", 1, 10.0)];
    let new = vec![
        entry("a.rs", "x", "cognitive", 1, 12.0), // worsened
        entry("b.rs", "y", "cognitive", 1, 5.0),  // added
    ];
    let diff = BaselineDiff::compute(&old, &new);
    let filter = SectionFilter::from_flags([true, false, true, false]);
    let tty = diff.render_tty(filter);
    assert!(tty.contains("## Added"));
    assert!(tty.contains("## Worsened"));
}

#[test]
fn markdown_wraps_sections_in_fenced_blocks() {
    let old = vec![];
    let new = vec![entry("src/foo.rs", "do_thing", "cognitive", 10, 27.0)];
    let md = BaselineDiff::compute(&old, &new).render_markdown(all());
    assert!(md.contains("## Added"), "header: {md}");
    assert!(md.contains("```text"), "fence open: {md}");
    assert!(md.contains("```\n"), "fence close: {md}");
    assert!(md.contains("src/foo.rs::do_thing"), "identity: {md}");
}

#[test]
fn json_emits_all_buckets_with_summary_ignoring_filter() {
    let old = vec![entry("a.rs", "x", "cognitive", 1, 10.0)];
    let new = vec![
        entry("a.rs", "x", "cognitive", 1, 12.0),
        entry("b.rs", "y", "cognitive", 1, 5.0),
    ];
    let json = BaselineDiff::compute(&old, &new).render_json().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["summary"]["added"], 1);
    assert_eq!(parsed["summary"]["worsened"], 1);
    assert_eq!(parsed["worsened"][0]["old"], 10.0);
    assert_eq!(parsed["worsened"][0]["new"], 12.0);
    assert_eq!(parsed["added"][0]["qualified"], "y");
    assert_eq!(parsed["added"][0]["value"], 5.0);
}

#[test]
fn file_level_metric_identity_uses_file_sentinel() {
    let old = vec![];
    let new = vec![entry("src/big.rs", "<file>", "loc.sloc", 1, 852.0)];
    let tty = BaselineDiff::compute(&old, &new).render_tty(all());
    assert!(
        tty.contains("src/big.rs::<file>"),
        "file-level identity: {tty}"
    );
}
