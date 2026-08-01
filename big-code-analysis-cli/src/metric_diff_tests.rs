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

/// Issue #514: the dotted `bca check --threshold` spelling of a `loc`
/// sub-metric is accepted as an alias for the bare bucket name, so a
/// filter copy-pasted from a `check` config selects the right bucket.
#[test]
fn metric_filter_accepts_dotted_loc_alias() {
    let old = set(
        "a",
        json!({ "loc": { "sloc": 3.0 }, "cognitive": { "sum": 2.0 } }),
    );
    let new = set(
        "a",
        json!({ "loc": { "sloc": 5.0 }, "cognitive": { "sum": 3.0 } }),
    );
    // `loc.sloc` (check spelling) must select the `sloc` bucket.
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &["loc.sloc".to_string()]);
    assert_eq!(diff.buckets.keys().collect::<Vec<_>>(), vec!["sloc"]);
}

/// A dotted family head (`halstead.volume`) aliases to its single bucket
/// (`halstead`).
#[test]
fn metric_filter_accepts_dotted_family_alias() {
    let old = set(
        "a",
        json!({ "halstead": { "volume": 10.0 }, "cognitive": { "sum": 2.0 } }),
    );
    let new = set(
        "a",
        json!({ "halstead": { "volume": 20.0 }, "cognitive": { "sum": 3.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &["halstead.volume".to_string()]);
    assert_eq!(diff.buckets.keys().collect::<Vec<_>>(), vec!["halstead"]);
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
    let tty = diff.render_tty("");
    assert!(tty.contains("1 metric(s) changed, 1 added file(s), 0 removed file(s)"));
    assert!(tty.contains("## cyclomatic"));
    assert!(tty.contains("a.sum"));
    assert!(tty.contains("3 \u{2192} 4"));
    assert!(tty.contains("## Added files"));
    assert!(tty.contains("  b"));
}

#[test]
fn strip_prefix_trims_displayed_paths_in_tty_and_markdown() {
    let old = set("src/a.rs", json!({ "cyclomatic": { "sum": 3.0 } }));
    let mut new = set("src/a.rs", json!({ "cyclomatic": { "sum": 4.0 } }));
    new.insert(
        "src/b.rs".to_string(),
        json!({ "tokens": { "tokens": 1.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);

    let tty = diff.render_tty("src/");
    assert!(
        tty.contains("a.rs.sum"),
        "TTY change row should be trimmed: {tty}"
    );
    assert!(
        !tty.contains("src/a.rs"),
        "TTY must not show the prefix: {tty}"
    );
    assert!(
        tty.contains("  b.rs"),
        "TTY added-files row should be trimmed: {tty}"
    );

    let md = diff.render_markdown("src/");
    assert!(
        md.contains("a.rs.sum"),
        "Markdown change row should be trimmed: {md}"
    );
    assert!(
        !md.contains("src/a.rs"),
        "Markdown must not show the prefix: {md}"
    );
    assert!(
        md.contains("- b.rs"),
        "Markdown added-files row should be trimmed: {md}"
    );

    // A non-matching prefix is a no-op passthrough.
    let untouched = diff.render_tty("nope/");
    assert!(
        untouched.contains("src/a.rs.sum"),
        "non-matching prefix passes through: {untouched}"
    );
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

/// #1098: the read-failure error names the side and pluralises the
/// noun. The `Before` side is unreachable from an integration test — the
/// `<ref>` extraction is written by this process and git records no
/// permission bits beyond the executable one — so its rendering is
/// pinned here, alongside the plural form the shared summary produces.
#[test]
fn unreadable_inputs_error_names_the_side() {
    let before = DiffError::UnreadableInputs {
        side: DiffSide::Before,
        count: 1,
    };
    assert_eq!(
        before.to_string(),
        "diff --since before tree: 1 input file could not be read (see the \
         errors above); refusing to trust a partially analysed input set"
    );

    let after = DiffError::UnreadableInputs {
        side: DiffSide::After,
        count: 2,
    };
    assert!(
        after
            .to_string()
            .starts_with("diff --since after tree: 2 input files could not be read"),
        "rendered: {after}"
    );
}

// --- #1116: the in-memory set must equal the file round-trip --------

/// `set_from_spaces` must produce exactly what `load_dir_set` produced
/// from the same walk (#1116).
///
/// `bca diff --since` used to serialize every `FuncSpace` to a temp JSON
/// tree and immediately parse it back. Dropping that round-trip is only
/// safe if the two routes agree on *both* halves of every entry: the
/// pairing key (the document's `name`, not the path the writer chose)
/// and the `metrics` value (which must survive `f64` -> text -> `f64`
/// unchanged — the workspace enables `serde_json`'s `float_roundtrip`
/// precisely so it does).
///
/// The fixture is real parsed source, not a hand-built `Value`, so the
/// comparison covers the actual `Serialize` impl and the actual float
/// values a walk produces.
#[test]
fn set_from_spaces_matches_the_file_round_trip() {
    use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

    // Branchy enough to give Halstead and MI irrational values, which
    // are what a lossy float round-trip would corrupt.
    const RUST: &str = r"
pub fn classify(n: i32, m: i32) -> &'static str {
    if n < 0 {
        'neg'
    } else if n == 0 && m > 3 {
        'zero'
    } else if n < m {
        'small'
    } else {
        'large'
    }
}
";
    let dir = tempfile::TempDir::new().expect("tempdir");
    let name = "./src/sample.rs";
    let space = Ast::parse(
        Source::new(LANG::Rust, RUST.replace('\'', "\"").as_bytes())
            .with_name(Some(name.to_owned())),
    )
    .expect("fixture parses")
    .metrics(MetricsOptions::default())
    .expect("metrics");

    // Route A: what the walk now does — straight from the in-memory tree.
    let in_memory = set_from_spaces([crate::AggregateItem::Metrics(
        Box::new(space.clone()),
        PathBuf::from(name),
    )])
    .expect("in-memory set builds");

    // Route B: what it used to do — write the per-file document, then
    // re-walk and re-parse the directory.
    let doc = dir.path().join("src");
    std::fs::create_dir_all(&doc).expect("create output dir");
    let file = doc.join("sample.rs.json");
    serde_json::to_writer(
        std::fs::File::create(&file).expect("create document"),
        &space,
    )
    .expect("write document");
    let round_tripped = load_dir_set(dir.path()).expect("directory set loads");

    assert_eq!(
        in_memory, round_tripped,
        "in-memory set diverged from the JSON round-trip it replaced"
    );
    // Guard against the comparison passing because both sides are empty.
    let metrics = in_memory.values().next().expect("one entry");
    assert!(
        metrics
            .get("halstead")
            .is_some_and(|h| h.get("volume").is_some()),
        "fixture produced no halstead.volume; the equality above proves little: {metrics}"
    );

    // The key comes from the document's `name`, never from the streamed
    // path. In a real `--since` walk the two coincide — the CWD is
    // anchored at the side's root and the seeds are relative, so the
    // emitted path *is* the name — which means swapping them is
    // unobservable in the equality above. It is pinned separately here
    // because `load_dir_set` reads `name` too, and that agreement is the
    // entire reason the before side (a /tmp extraction) pairs with the
    // after side (the working tree) despite different absolute roots.
    let divergent = set_from_spaces([crate::AggregateItem::Metrics(
        Box::new(space),
        PathBuf::from("some/other/spelling.rs"),
    )])
    .expect("in-memory set builds");
    assert_eq!(
        divergent.keys().collect::<Vec<_>>(),
        vec![name],
        "the in-memory set must key on the document's `name`, not the streamed path"
    );
}
