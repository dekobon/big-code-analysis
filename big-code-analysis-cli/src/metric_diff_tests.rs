// Sibling-file unit tests for the `metric_diff` module (issue #487).
// Wired via `#[path = "metric_diff_tests.rs"] mod tests;`. Matched by
// the `./**/*_tests.rs` rule in `.bcaignore`.

use super::*;
use serde_json::json;

/// Build a one-file [`MetricSet`] from a JSON `metrics` object literal.
fn set(file: &str, metrics: Value) -> MetricSet {
    let mut m = MetricSet::new();
    m.insert(file.to_string(), metrics);
    m
}

#[test]
fn unchanged_field_is_not_reported() {
    // Default min_change == 0.0 means "any change", NOT "every field":
    // a field that did not move must not appear.
    let old = set("a", json!({ "cyclomatic": { "sum": 3.0 } }));
    let new = set("a", json!({ "cyclomatic": { "sum": 3.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert!(diff.buckets.is_empty());
    assert_eq!(
        diff.summary_line(),
        "0 metric(s) changed, 0 added file(s), 0 removed file(s)"
    );
}

#[test]
fn changed_scalar_buckets_under_its_family() {
    let old = set("a", json!({ "cyclomatic": { "sum": 3.0, "max": 2.0 } }));
    let new = set("a", json!({ "cyclomatic": { "sum": 4.0, "max": 2.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    // expected: one bucket `cyclomatic`, one changed field (`sum`),
    // `max` unchanged so excluded.
    assert_eq!(diff.buckets.len(), 1);
    let bucket = &diff.buckets["cyclomatic"];
    assert_eq!(bucket.changed.len(), 1);
    assert_eq!(bucket.changed[0].field, "sum");
    assert_eq!(bucket.changed[0].old, 3.0);
    assert_eq!(bucket.changed[0].new, 4.0);
}

#[test]
fn nested_field_keeps_dotted_path_under_family() {
    // `cyclomatic.modified.sum` must bucket under `cyclomatic` with a
    // dotted field path, NOT under a phantom `modified` bucket.
    let old = set("a", json!({ "cyclomatic": { "modified": { "sum": 3.0 } } }));
    let new = set("a", json!({ "cyclomatic": { "modified": { "sum": 5.0 } } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert_eq!(diff.buckets.len(), 1);
    let bucket = &diff.buckets["cyclomatic"];
    assert_eq!(bucket.changed.len(), 1);
    assert_eq!(bucket.changed[0].field, "modified.sum");
}

#[test]
fn loc_expands_to_sub_metric_buckets() {
    // `loc` is the one family that expands: each sub-metric is its own
    // bucket name (matching `bca list-metrics`), never a `loc` bucket.
    let old = set(
        "a",
        json!({ "loc": { "sloc": 3.0, "ploc": 3.0, "cloc": 0.0 } }),
    );
    let new = set(
        "a",
        json!({ "loc": { "sloc": 5.0, "ploc": 3.0, "cloc": 2.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    // expected: `sloc` and `cloc` changed; `ploc` unchanged; no `loc`.
    let names: Vec<&str> = diff.buckets.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["cloc", "sloc"]);
    assert!(!diff.buckets.contains_key("loc"));
    assert!(!diff.buckets.contains_key("ploc"));
}

#[test]
fn loc_average_suffix_buckets_to_base_sub_metric() {
    // The emitter appends `_average` / `_min` / `_max` to loc rows; the
    // first underscore-segment is the sub-metric, so `sloc_average`
    // buckets under `sloc`.
    let old = set("a", json!({ "loc": { "sloc_average": 1.5 } }));
    let new = set("a", json!({ "loc": { "sloc_average": 2.5 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert_eq!(diff.buckets.keys().collect::<Vec<_>>(), vec!["sloc"]);
}

#[test]
fn added_and_removed_files_are_set_level() {
    let old = set("gone", json!({ "tokens": { "tokens": 1.0 } }));
    let new = set("added", json!({ "tokens": { "tokens": 1.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    // expected: no shared file, so no buckets; one added, one removed.
    assert!(diff.buckets.is_empty());
    assert_eq!(diff.added_files, vec!["added"]);
    assert_eq!(diff.removed_files, vec!["gone"]);
}

#[test]
fn field_present_on_one_side_diffs_against_zero() {
    // A metric field that appears (or vanishes) between grammar
    // versions is a genuine delta, diffed against 0.0.
    let old = set("a", json!({ "halstead": {} }));
    let new = set("a", json!({ "halstead": { "n1": 9.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    let bucket = &diff.buckets["halstead"];
    assert_eq!(bucket.changed.len(), 1);
    assert_eq!(bucket.changed[0].field, "n1");
    assert_eq!(bucket.changed[0].old, 0.0);
    assert_eq!(bucket.changed[0].new, 9.0);
}

#[test]
fn unknown_family_buckets_under_its_own_key_not_loc() {
    // A metric key with no catalog family must bucket under its own
    // name, never misfile under `loc` (the expand-family fallback).
    let old = set("a", json!({ "future_metric": { "sum": 1.0 } }));
    let new = set("a", json!({ "future_metric": { "sum": 2.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert_eq!(
        diff.buckets.keys().collect::<Vec<_>>(),
        vec!["future_metric"]
    );
    assert!(!diff.buckets.contains_key("loc"));
}

#[test]
fn min_change_suppresses_sub_threshold_movement() {
    let old = set(
        "a",
        json!({ "cyclomatic": { "sum": 3.0 }, "halstead": { "effort": 100.0 } }),
    );
    let new = set(
        "a",
        json!({ "cyclomatic": { "sum": 4.0 }, "halstead": { "effort": 600.0 } }),
    );
    // min_change 5 drops the +1 cyclomatic move, keeps the +500 effort.
    let diff = MetricDiff::from_sets(&old, &new, 5.0, &[]);
    assert_eq!(diff.buckets.keys().collect::<Vec<_>>(), vec!["halstead"]);
}

#[test]
fn metric_filter_restricts_buckets() {
    let old = set(
        "a",
        json!({ "cyclomatic": { "sum": 3.0 }, "cognitive": { "sum": 2.0 } }),
    );
    let new = set(
        "a",
        json!({ "cyclomatic": { "sum": 4.0 }, "cognitive": { "sum": 3.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &["cyclomatic".to_string()]);
    assert_eq!(diff.buckets.keys().collect::<Vec<_>>(), vec!["cyclomatic"]);
}

#[test]
fn json_render_has_stable_schema() {
    let old = set("src/a.rs", json!({ "cyclomatic": { "sum": 3.0 } }));
    let new = set("src/a.rs", json!({ "cyclomatic": { "sum": 4.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    let rendered = diff.render_json().expect("serialize diff JSON");
    let parsed: Value = serde_json::from_str(&rendered).expect("reparse diff JSON");
    // expected: summary counts and one cyclomatic delta 3 → 4.
    assert_eq!(parsed["summary"]["metrics_changed"], json!(1));
    assert_eq!(parsed["summary"]["total_changes"], json!(1));
    assert_eq!(
        parsed["buckets"]["cyclomatic"]["changed"][0]["field"],
        json!("sum")
    );
    assert_eq!(
        parsed["buckets"]["cyclomatic"]["changed"][0]["old"],
        json!(3.0)
    );
    assert_eq!(
        parsed["buckets"]["cyclomatic"]["changed"][0]["new"],
        json!(4.0)
    );
}

#[test]
fn tty_render_lists_changes_and_files() {
    let old = set("a", json!({ "cyclomatic": { "sum": 3.0 } }));
    let mut new = set("a", json!({ "cyclomatic": { "sum": 4.0 } }));
    new.insert("b".to_string(), json!({ "tokens": { "tokens": 1.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    let tty = diff.render_tty();
    assert!(tty.contains("1 metric(s) changed, 1 added file(s), 0 removed file(s)"));
    assert!(tty.contains("## cyclomatic"));
    assert!(tty.contains("a.sum"));
    assert!(tty.contains("3 \u{2192} 4"));
    assert!(tty.contains("## Added files"));
    assert!(tty.contains("  b"));
}

#[test]
fn load_set_directory_keys_on_name_field_with_relpath_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sub = dir.path().join("nested");
    std::fs::create_dir_all(&sub).expect("mkdir");
    // With a `name`, the entry keys on it — the source-of-truth identity
    // bca emits — so a directory set pairs with a single-file set and
    // the `.json` output suffix / dir layout never leak into the key.
    std::fs::write(
        sub.join("x.json"),
        serde_json::to_vec(
            &json!({ "name": "src/x.rs", "metrics": { "tokens": { "tokens": 7.0 } } }),
        )
        .expect("encode"),
    )
    .expect("write");
    // Without a `name`, it falls back to the path relative to the root.
    std::fs::write(
        sub.join("y.json"),
        serde_json::to_vec(&json!({ "metrics": { "tokens": { "tokens": 1.0 } } })).expect("encode"),
    )
    .expect("write");
    let loaded = load_set(dir.path()).expect("load dir");
    assert!(loaded.contains_key("src/x.rs"));
    assert_eq!(loaded["src/x.rs"]["tokens"]["tokens"], json!(7.0));
    assert!(loaded.contains_key("nested/y.json"));
}

#[test]
fn diff_pairs_single_file_against_directory_on_name() {
    // Regression: a single-file input keys on `name`, so a directory
    // input must too — else `bca diff <file> <dir>` reports the same
    // source file as added+removed (disjoint key spaces) instead of a
    // delta. old = directory (`name` src/a.rs), new = single file (same
    // `name`); they must pair as a 3 -> 5 change, not 1 added + 1 removed.
    let scratch = tempfile::tempdir().expect("tempdir");
    let old_dir = scratch.path().join("old");
    std::fs::create_dir_all(&old_dir).expect("mkdir");
    std::fs::write(
        old_dir.join("a.rs.json"),
        serde_json::to_vec(
            &json!({ "name": "src/a.rs", "metrics": { "cyclomatic": { "sum": 3.0 } } }),
        )
        .expect("encode"),
    )
    .expect("write");
    let new_file = scratch.path().join("after.json");
    std::fs::write(
        &new_file,
        serde_json::to_vec(
            &json!({ "name": "src/a.rs", "metrics": { "cyclomatic": { "sum": 5.0 } } }),
        )
        .expect("encode"),
    )
    .expect("write");

    let diff = MetricDiff::compute(old_dir.as_path(), &new_file, 0.0, &[]).expect("diff");
    assert_eq!(diff.added_files.len(), 0, "must pair, not report added");
    assert_eq!(diff.removed_files.len(), 0, "must pair, not report removed");
    assert_eq!(diff.buckets["cyclomatic"].changed[0].old, 3.0);
    assert_eq!(diff.buckets["cyclomatic"].changed[0].new, 5.0);
}

#[test]
fn load_set_single_file_keys_on_name_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("metrics.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &json!({ "name": "src/a.rs", "metrics": { "tokens": { "tokens": 7.0 } } }),
        )
        .expect("encode"),
    )
    .expect("write");
    let loaded = load_set(&path).expect("load file");
    assert!(loaded.contains_key("src/a.rs"));
}

#[test]
fn parse_error_is_surfaced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.json");
    std::fs::write(&path, b"{ not json").expect("write");
    let err = MetricDiff::compute(&path, &path, 0.0, &[]).expect_err("must error");
    assert!(matches!(err, DiffError::Parse { .. }));
}
