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
        "0 metric(s) changed, 0 added file(s), 0 removed file(s), \
         0 added field(s), 0 removed field(s)"
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

/// Build the one-sided-field row the tests below expect.
fn presence(file: &str, metric: &str, field: &str) -> FieldPresence {
    FieldPresence {
        file: file.to_owned(),
        metric: metric.to_owned(),
        field: field.to_owned(),
    }
}

/// #1623: a leaf numeric on one side only has no counterpart to diff
/// against. It used to be compared with an invented `0.0`, so a field
/// that merely appeared printed as a `0 → 9` movement; now it is listed
/// as added (or removed), the buckets stay empty, and the summary and
/// `--exit-code` still count it as a difference.
#[test]
fn field_present_on_one_side_is_listed_not_diffed() {
    let old = set("a", json!({ "halstead": {} }));
    let new = set("a", json!({ "halstead": { "n1": 9.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert!(
        diff.buckets.is_empty(),
        "no invented 0 → 9 delta: {:?}",
        diff.buckets
    );
    assert_eq!(diff.added_fields, vec![presence("a", "halstead", "n1")]);
    assert_eq!(diff.removed_fields, [] as [FieldPresence; 0]);
    assert!(!diff.is_empty(), "a one-sided leaf is still a difference");
    assert_eq!(
        diff.summary_line(),
        "0 metric(s) changed, 0 added file(s), 0 removed file(s), \
         1 added field(s), 0 removed field(s)"
    );

    // The same pair the other way round is a removal.
    let diff = MetricDiff::from_sets(&new, &old, 0.0, &[]);
    assert!(diff.buckets.is_empty());
    assert_eq!(diff.added_fields, [] as [FieldPresence; 0]);
    assert_eq!(diff.removed_fields, vec![presence("a", "halstead", "n1")]);
    assert!(!diff.is_empty());
}

/// The common trigger (#1623): two captures of one file taken with
/// different `--metrics` sets. Every leaf of a family the old capture
/// never computed is listed under its own bucket name — `loc` leaves
/// under the expanded sub-metric, as the buckets would file them — and
/// the rows come out sorted by (file, metric, field).
#[test]
fn differing_metrics_sets_list_whole_families_by_bucket() {
    let old = set(
        "a",
        json!({ "cyclomatic": { "sum": 3.0 }, "loc": { "sloc": 10.0 } }),
    );
    let new = set(
        "a",
        json!({
            "cyclomatic": { "sum": 3.0 },
            "loc": { "sloc": 10.0, "sloc_average": 5.0 },
            "mi": { "sei": 90.0, "original": 100.0 }
        }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert!(diff.buckets.is_empty(), "{:?}", diff.buckets);
    assert_eq!(
        diff.added_fields,
        vec![
            presence("a", "mi", "original"),
            presence("a", "mi", "sei"),
            presence("a", "sloc", "sloc_average"),
        ]
    );
    assert_eq!(diff.removed_fields, [] as [FieldPresence; 0]);
}

/// A real `0` is a value, not an absence: a field at `0.0` on both sides
/// is unchanged, and `0.0 → 2.0` is a genuine bucketed delta rather than
/// an added field. `classify` is the one place that rule lives.
#[test]
fn zero_is_a_present_value_not_a_missing_one() {
    let zero = set("a", json!({ "abc": { "assignments": 0.0 } }));
    assert!(MetricDiff::from_sets(&zero, &zero, 0.0, &[]).is_empty());

    let two = set("a", json!({ "abc": { "assignments": 2.0 } }));
    let diff = MetricDiff::from_sets(&zero, &two, 0.0, &[]);
    assert_eq!(diff.added_fields, [] as [FieldPresence; 0]);
    let bucket = &diff.buckets["abc"];
    assert_eq!(bucket.changed.len(), 1);
    assert_eq!(bucket.changed[0].old, 0.0);
    assert_eq!(bucket.changed[0].new, 2.0);

    assert_eq!(
        LeafValues::classify(Some(0.0), None),
        Some(LeafValues::OldOnly)
    );
    assert_eq!(
        LeafValues::classify(None, Some(0.0)),
        Some(LeafValues::NewOnly)
    );
    assert_eq!(LeafValues::classify(None, None), None);
}

/// A one-sided leaf belongs to a metric, so `--metric` selects or drops
/// it like a bucket — through the same alias resolution, so the dotted
/// `check` spelling `mi.original` selects the `mi` rows; it has no
/// delta, so `--min-change` never hides it.
#[test]
fn one_sided_fields_honour_metric_filter_but_not_min_change() {
    let old = set("a", json!({ "cyclomatic": { "sum": 3.0 } }));
    let new = set(
        "a",
        json!({
            "cyclomatic": { "sum": 3.0 },
            "mi": { "original": 100.0 },
            "tokens": { "tokens": 5.0 }
        }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 1_000.0, &["mi.original".to_string()]);
    assert!(diff.buckets.is_empty());
    assert_eq!(diff.added_fields, vec![presence("a", "mi", "original")]);

    let diff = MetricDiff::from_sets(&old, &new, 0.0, &["cyclomatic".to_string()]);
    assert!(
        diff.is_empty(),
        "filtered out entirely: {:?}",
        diff.added_fields
    );
}

/// `serde_json` writes a non-finite float as `null`, so a key can exist
/// with no number behind it. That side reads as absent — there is
/// nothing to compare — and two `null`s are not a metric scalar at all.
#[test]
fn null_leaf_reads_as_absent_on_that_side() {
    let null = set("a", json!({ "halstead": { "difficulty": null } }));
    let two = set("a", json!({ "halstead": { "difficulty": 2.0 } }));
    let diff = MetricDiff::from_sets(&null, &two, 0.0, &[]);
    assert!(
        diff.buckets.is_empty(),
        "no 0 → 2 delta: {:?}",
        diff.buckets
    );
    assert_eq!(
        diff.added_fields,
        vec![presence("a", "halstead", "difficulty")]
    );
    assert!(MetricDiff::from_sets(&null, &null, 0.0, &[]).is_empty());
}

/// The exact bytes of both human renderers for a diff carrying every
/// section kind: the field sections sit between the file sections and
/// the buckets, each row is `<file>  <metric>  <field>` with the first
/// two columns padded to the section's widest entry (`longer.rs` and
/// `sloc` set them), and `--strip-prefix` trims the file column.
#[test]
fn tty_and_markdown_list_one_sided_fields() {
    let mut old = set(
        "src/a.rs",
        json!({ "cyclomatic": { "sum": 3.0 }, "nom": { "total": 1.0 } }),
    );
    old.insert(
        "src/longer.rs".to_string(),
        json!({ "nom": { "total": 1.0 } }),
    );
    let mut new = set(
        "src/a.rs",
        json!({
            "cyclomatic": { "sum": 4.0 },
            "mi": { "original": 100.0 },
            "loc": { "sloc_average": 2.0 }
        }),
    );
    new.insert(
        "src/longer.rs".to_string(),
        json!({ "nom": { "total": 1.0 }, "mi": { "sei": 5.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert_eq!(
        diff.render_tty("src/"),
        "1 metric(s) changed, 0 added file(s), 0 removed file(s), \
         3 added field(s), 1 removed field(s)\n\
         \n\
         ## Added fields\n\
         \x20 a.rs       mi    original\n\
         \x20 a.rs       sloc  sloc_average\n\
         \x20 longer.rs  mi    sei\n\
         \n\
         ## Removed fields\n\
         \x20 a.rs  nom  total\n\
         \n\
         ## cyclomatic (1 change(s))\n\
         \x20 a.rs.sum  3 \u{2192} 4\n"
    );
    assert_eq!(
        diff.render_markdown("src/"),
        "1 metric(s) changed, 0 added file(s), 0 removed file(s), \
         3 added field(s), 1 removed field(s)\n\
         \n\
         ## Added fields\n\
         \n\
         ```text\n\
         a.rs       mi    original\n\
         a.rs       sloc  sloc_average\n\
         longer.rs  mi    sei\n\
         ```\n\
         \n\
         ## Removed fields\n\
         \n\
         ```text\n\
         a.rs  nom  total\n\
         ```\n\
         \n\
         ## cyclomatic (1 change(s))\n\
         \n\
         ```text\n\
         a.rs.sum  3 \u{2192} 4\n\
         ```\n"
    );
}

/// A side that is not a JSON object holds nothing numeric, so every leaf
/// the other side has is one-sided — including a whole `metrics` tree
/// that is `null`, and a family that is a bare number on one side and an
/// object on the other, where the number is a leaf of its own (an empty
/// `field`) and must not vanish under the walk into the object. The
/// family case is reachable from disk; the `null` tree is not, since the
/// loader refuses that document (`json_without_a_metrics_object_is_not_a_document`),
/// and is exercised here through `from_sets` directly.
#[test]
fn non_object_side_makes_every_leaf_one_sided() {
    let old = set("a", json!({ "cyclomatic": { "sum": 3.0 }, "nexits": 3.0 }));
    let diff = MetricDiff::from_sets(&old, &set("a", json!(null)), 0.0, &[]);
    assert!(diff.buckets.is_empty());
    assert_eq!(
        diff.removed_fields,
        vec![
            presence("a", "cyclomatic", "sum"),
            presence("a", "nexits", "")
        ]
    );
    assert!(!diff.is_empty(), "the whole tree vanished");

    let new = set(
        "a",
        json!({ "cyclomatic": { "sum": 3.0 }, "nexits": { "sum": 3.0 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert!(diff.buckets.is_empty(), "no 3 → 3 pairing across shapes");
    assert_eq!(diff.removed_fields, vec![presence("a", "nexits", "")]);
    assert_eq!(diff.added_fields, vec![presence("a", "nexits", "sum")]);
    // The empty field path renders as a named placeholder, so the row does
    // not end in the column padding's trailing spaces.
    let tty = diff.render_tty("");
    let scalar_row = tty
        .lines()
        .find(|line| line.contains("(scalar)"))
        .expect("the bare-number leaf is rendered");
    assert_eq!(scalar_row.trim_end(), scalar_row, "{scalar_row:?}");
    assert!(
        tty.lines().all(|line| line.trim_end() == line),
        "no rendered row carries trailing spaces:\n{tty}"
    );
}

/// A family that is a bare number on *both* sides is a comparable pair
/// with an empty field path. Its delta row names the leaf `(scalar)`, as
/// the one-sided sections do, rather than ending the file in a dangling
/// `.` (`a.rs.  3 → 4`).
#[test]
fn bare_number_family_on_both_sides_renders_as_scalar() {
    let old = set("src/a.rs", json!({ "nexits": 3.0 }));
    let new = set("src/a.rs", json!({ "nexits": 4.0 }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    assert_eq!(diff.buckets["nexits"].changed[0].field, "");
    assert_eq!(
        diff.render_tty("src/"),
        "1 metric(s) changed, 0 added file(s), 0 removed file(s), \
         0 added field(s), 0 removed field(s)\n\
         \n\
         ## nexits (1 change(s))\n\
         \x20 a.rs (scalar)  3 \u{2192} 4\n"
    );
    assert!(
        diff.render_markdown("src/")
            .contains("\n```text\na.rs (scalar)  3 \u{2192} 4\n```\n"),
        "{}",
        diff.render_markdown("src/")
    );
}

/// The JSON form keeps `{file, field, old, new}` rows numeric on both
/// sides and lists one-sided leaves beside the file lists, each row
/// `{file, metric, field}`, with the summary counting them.
#[test]
fn json_lists_one_sided_fields_beside_the_file_lists() {
    let old = set("src/a.rs", json!({ "cyclomatic": { "sum": 3.0 } }));
    let new = set(
        "src/a.rs",
        json!({ "cyclomatic": { "sum": 3.0 }, "mi": { "original": 100.5 } }),
    );
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    let rendered = diff.render_json().expect("serialize diff JSON");
    let parsed: Value = serde_json::from_str(&rendered).expect("reparse diff JSON");
    assert_eq!(parsed["summary"]["total_changes"], json!(0));
    assert_eq!(parsed["summary"]["added_fields"], json!(1));
    assert_eq!(parsed["summary"]["removed_fields"], json!(0));
    assert_eq!(parsed["buckets"], json!({}));
    assert_eq!(
        parsed["added_fields"],
        json!([{ "file": "src/a.rs", "metric": "mi", "field": "original" }])
    );
    assert_eq!(parsed["removed_fields"], json!([]));
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
    // expected: summary counts and one cyclomatic delta 3 → 4; the
    // one-sided-field lists are present but empty.
    assert_eq!(parsed["summary"]["metrics_changed"], json!(1));
    assert_eq!(parsed["summary"]["total_changes"], json!(1));
    assert_eq!(parsed["summary"]["added_fields"], json!(0));
    assert_eq!(parsed["summary"]["removed_fields"], json!(0));
    assert_eq!(parsed["added_fields"], json!([]));
    assert_eq!(parsed["removed_fields"], json!([]));
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

/// The added and removed file lists are fenced like every other Markdown
/// section: as `- pkg/__init__.py` bullets, a renderer reads the
/// underscores as emphasis and shows `pkg/init.py` with `init` in bold.
#[test]
fn markdown_fences_added_and_removed_files() {
    let mut old = set("pkg/__init__.py", json!({}));
    old.insert("pkg/__old__.py".to_string(), json!({}));
    let mut new = set("pkg/__init__.py", json!({}));
    new.insert("pkg/__new__.py".to_string(), json!({}));
    let md = MetricDiff::from_sets(&old, &new, 0.0, &[]).render_markdown("");
    assert_eq!(
        md,
        "0 metric(s) changed, 1 added file(s), 1 removed file(s), \
         0 added field(s), 0 removed field(s)\n\
         \n\
         ## Added files\n\
         \n\
         ```text\n\
         pkg/__new__.py\n\
         ```\n\
         \n\
         ## Removed files\n\
         \n\
         ```text\n\
         pkg/__old__.py\n\
         ```\n"
    );
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
        md.contains("## Added files\n\n```text\nb.rs\n```\n"),
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

/// #1131's sibling variant. The two must stay distinguishable in the
/// rendered text: one counts files a worker could not open, the other
/// entries the traversal could not list, and a user told "1 input file"
/// about an unlistable directory would go looking for the wrong thing.
/// The `Before` side is unreachable end to end for the reason above.
#[test]
fn unwalkable_inputs_error_names_the_side_and_counts_entries() {
    let before = DiffError::UnwalkableInputs {
        side: DiffSide::Before,
        count: 1,
    };
    assert_eq!(
        before.to_string(),
        "diff --since before tree: 1 directory entry could not be read (see \
         the warnings above); refusing to trust a partially walked input set"
    );

    let after = DiffError::UnwalkableInputs {
        side: DiffSide::After,
        count: 2,
    };
    assert!(
        after
            .to_string()
            .starts_with("diff --since after tree: 2 directory entries could not be read"),
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
    let round_tripped = load::load_dir_set(dir.path()).expect("directory set loads");

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
    // Keyed in its lexical form, as an on-disk document's `name` is, so
    // a `--since` side pairs with a capture taken with `-p src`.
    assert_eq!(
        divergent.keys().collect::<Vec<_>>(),
        ["src/sample.rs"],
        "the in-memory set must key on the document's `name`, not the streamed path"
    );
}

// --- loader error paths: each names the input it could not use -------

/// A side that does not exist is a read error naming that path, not an
/// empty set that would report every file of the other side as added.
#[test]
fn missing_file_is_a_read_error_naming_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("absent.json");
    let err = load_set(&path).expect_err("missing file");
    assert!(
        matches!(&err, DiffError::Read { path: p, .. } if *p == path),
        "{err:?}"
    );
}

/// The directory walk's own failure — here a root that is not there —
/// surfaces as a read error naming the root rather than an empty walk.
#[test]
fn missing_directory_root_is_a_read_error_naming_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("absent");
    let err = load::load_dir_set(&root).expect_err("missing root");
    assert!(
        matches!(&err, DiffError::Read { path: p, .. } if *p == root),
        "{err:?}"
    );
}

/// One malformed document fails the whole directory set, naming that
/// document, instead of being skipped: a skipped file would read as one
/// the other side added or removed.
#[test]
fn malformed_document_inside_a_directory_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("good.json"),
        r#"{"name":"a.rs","metrics":{}}"#,
    )
    .expect("write");
    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, b"{ not json").expect("write");
    let err = load_set(dir.path()).expect_err("malformed document");
    assert!(
        matches!(&err, DiffError::Parse { path: p, .. } if *p == bad),
        "{err:?}"
    );
}

/// A document with no `name` keys on its path, and a path that is not
/// UTF-8 cannot be a key without a lossy rename, so it is an error. Not on
/// macOS, whose filesystems refuse such a name with `EILSEQ`, nor Windows,
/// which has no byte-path constructor.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
#[test]
fn non_utf8_fallback_key_is_an_error_not_a_lossy_rename() {
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let name = std::ffi::OsStr::from_bytes(b"\xff.json");
    // A filesystem mounted `utf8only` (ZFS) refuses the name as macOS
    // does, so the fixture cannot exist there either.
    if let Err(e) = std::fs::write(dir.path().join(name), r#"{"metrics":{}}"#) {
        eprintln!("skipping: this filesystem refuses a non-UTF-8 name: {e}");
        return;
    }
    let err = load_set(dir.path()).expect_err("non-UTF-8 key");
    assert!(
        matches!(&err, DiffError::NonUtf8Path { path } if path.as_os_str() == name),
        "{err:?}"
    );
}

/// An `ops` result reaching the `--since` collector is refused rather
/// than skipped: a skipped file would read as added or removed (#1098).
#[test]
fn ops_item_in_a_since_walk_is_refused() {
    use big_code_analysis::{Ast, LANG, Source};

    let ops = Ast::parse(Source::new(LANG::Rust, b"fn a() {}\n"))
        .expect("fixture parses")
        .ops()
        .expect("fixture yields ops");
    let err = set_from_spaces([crate::AggregateItem::Ops(
        Box::new(ops),
        PathBuf::from("a.rs"),
    )])
    .expect_err("ops item");
    assert!(matches!(err, DiffError::UnexpectedAggregate), "{err:?}");
}

// --- input validation: nothing the loader cannot pair is skipped -------

/// Write `doc` as JSON to `path`, creating parent directories.
fn write_doc(path: &Path, doc: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, serde_json::to_vec(doc).expect("encode")).expect("write");
}

/// A per-file document named `name` whose cyclomatic sum is `sum`.
fn doc(name: &str, sum: f64) -> Value {
    json!({ "name": name, "metrics": { "cyclomatic": { "sum": sum } } })
}

/// `bca metrics -O json --output <file>` writes one aggregate array.
/// Each element is a per-file document and loads as one, so an aggregate
/// pairs with the per-file tree of the same run. It used to key on the
/// input path with an empty `metrics`, and the diff reported one file
/// removed and another added, exit 0.
#[test]
fn aggregate_array_loads_each_element_and_pairs_with_a_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let aggregate = dir.path().join("before.json");
    write_doc(
        &aggregate,
        &json!([doc("src/a.rs", 3.0), doc("src/b.rs", 1.0)]),
    );
    let tree = dir.path().join("after");
    write_doc(&tree.join("src/a.rs.json"), &doc("src/a.rs", 4.0));
    write_doc(&tree.join("src/b.rs.json"), &doc("src/b.rs", 1.0));

    let loaded = load_set(&aggregate).expect("aggregate loads");
    assert_eq!(loaded.keys().collect::<Vec<_>>(), ["src/a.rs", "src/b.rs"]);
    assert_eq!(loaded["src/a.rs"], json!({ "cyclomatic": { "sum": 3.0 } }));

    let diff = MetricDiff::compute(&aggregate, &tree, 0.0, &[]).expect("diff");
    assert_eq!(diff.added_files, [] as [String; 0]);
    assert_eq!(diff.removed_files, [] as [String; 0]);
    let changed = &diff.buckets["cyclomatic"].changed;
    assert_eq!(
        (
            changed.len(),
            changed[0].file.as_str(),
            changed[0].old,
            changed[0].new
        ),
        (1, "src/a.rs", 3.0, 4.0)
    );
}

/// An element of an aggregate has no path of its own to key on, and a
/// single-file side keyed on its own file name pairs with nothing on the
/// other side; both are errors naming the document.
#[test]
fn nameless_document_outside_a_directory_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let aggregate = dir.path().join("agg.json");
    write_doc(
        &aggregate,
        &json!([doc("src/a.rs", 1.0), { "metrics": { "nom": { "total": 1.0 } } }]),
    );
    let err = load_set(&aggregate).expect_err("nameless element");
    assert!(
        matches!(&err, DiffError::Nameless { origin } if *origin == DocumentOrigin::element(&aggregate, 1)),
        "{err:?}"
    );
    // The hint names the one way bca itself writes a nameless document,
    // and suggests no move into a directory side: that fallback never
    // reaches an aggregate element, and for bca's own null `name` the
    // output-dir path is not UTF-8 either.
    assert_eq!(
        err.to_string(),
        format!(
            "{}[1] has no `name` to pair it on: set `name` to its source path; \
             `bca metrics` itself writes a null `name` only for a source path \
             that is not valid UTF-8, which `bca diff` cannot pair until that \
             file is renamed",
            aggregate.display()
        )
    );

    let single = dir.path().join("one.json");
    write_doc(&single, &json!({ "metrics": {} }));
    let err = load_set(&single).expect_err("nameless file");
    assert!(
        matches!(&err, DiffError::Nameless { origin } if *origin == DocumentOrigin::file(&single)),
        "{err:?}"
    );

    // An aggregate element inside a directory side has no fallback
    // either, and `name: null` is the shape `bca metrics` writes.
    let tree = dir.path().join("tree");
    let nested = tree.join("agg.json");
    write_doc(
        &nested,
        &json!([{ "name": null, "metrics": { "nom": { "total": 1.0 } } }]),
    );
    let err = load_set(&tree).expect_err("nameless element in a tree");
    assert!(
        matches!(&err, DiffError::Nameless { origin } if *origin == DocumentOrigin::element(&nested, 0)),
        "{err:?}"
    );
}

/// A document's `name` and a directory's relpath fallback both key in
/// lexical form: `-p ./src` and `-p src` captures of one tree pair, as do
/// a doubled or trailing separator. A `..` is kept, since collapsing it
/// is only right when its parent is not a symlink.
#[test]
fn pairing_keys_drop_dot_components_and_keep_parent_components() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("old.json");
    write_doc(
        &old,
        &json!([
            doc("./src/a.rs", 1.0),
            doc("src/./b.rs", 1.0),
            doc("src//c.rs", 1.0),
            doc("src/../d.rs", 1.0),
        ]),
    );
    let loaded = load_set(&old).expect("aggregate loads");
    assert_eq!(
        loaded.keys().collect::<Vec<_>>(),
        ["src/../d.rs", "src/a.rs", "src/b.rs", "src/c.rs"]
    );

    let new = dir.path().join("new.json");
    write_doc(&new, &doc("src/a.rs", 2.0));
    let diff = MetricDiff::compute(&old, &new, 0.0, &[]).expect("diff");
    assert_eq!(
        diff.buckets["cyclomatic"].changed[0].file, "src/a.rs",
        "`./src/a.rs` must pair with `src/a.rs`"
    );
}

/// Two spellings of one source path on one side are one document twice.
#[test]
fn spellings_of_one_path_on_one_side_are_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let aggregate = dir.path().join("agg.json");
    write_doc(
        &aggregate,
        &json!([doc("./src/a.rs", 1.0), doc("src/a.rs", 2.0)]),
    );
    let err = load_set(&aggregate).expect_err("one file twice");
    assert!(
        matches!(&err, DiffError::DuplicateName { name, .. } if name == "src/a.rs"),
        "{err:?}"
    );
}

/// A directory side reads only `*.json`. Any other file beside a
/// document is not an input and is passed over, but a directory with no
/// `*.json` file at all (a YAML capture, an empty or mistyped path) is an
/// error naming it: loaded as an empty set it reports every file of the
/// other side, or no change against another such directory. A `*.json`
/// file holding the empty aggregate `[]` is a capture of a walk that
/// matched nothing, and loads as the empty set it is, as it does when
/// passed as a file.
#[test]
fn a_directory_side_needs_at_least_one_json_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let yaml = dir.path().join("yaml");
    std::fs::create_dir_all(yaml.join("src")).expect("mkdir");
    std::fs::write(yaml.join("src/a.rs.yml"), "name: src/a.rs\n").expect("write yaml");
    let err = load_set(&yaml).expect_err("no json");
    assert!(
        matches!(&err, DiffError::NoDocuments { path } if *path == yaml),
        "{err:?}"
    );

    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).expect("mkdir");
    let err = load_set(&empty).expect_err("empty");
    assert!(
        matches!(&err, DiffError::NoDocuments { path } if *path == empty),
        "{err:?}"
    );

    let json_only_empty = dir.path().join("json-empty");
    write_doc(&json_only_empty.join("all.json"), &json!([]));
    let from_dir = load_set(&json_only_empty).expect("an empty aggregate is a capture");
    assert!(from_dir.is_empty(), "{from_dir:?}");
    let from_file = load_set(&json_only_empty.join("all.json")).expect("the same file alone");
    assert!(from_file.is_empty(), "{from_file:?}");

    let mixed = dir.path().join("mixed");
    write_doc(&mixed.join("src/a.rs.json"), &doc("src/a.rs", 1.0));
    std::fs::write(mixed.join("README.md"), "captured by CI\n").expect("write readme");
    let loaded = load_set(&mixed).expect("a README beside the capture is ignored");
    assert_eq!(loaded.keys().collect::<Vec<_>>(), ["src/a.rs"]);
}

/// `--strip-prefix` is matched against the lexical key, so the `./src/`
/// a user copies from a `-p ./src` walk still strips; `./` alone strips
/// nothing, and the trailing `/` is kept but not doubled, so `/` strips
/// the root of an absolute name.
#[test]
fn strip_prefix_is_matched_in_key_form() {
    // Keyed as the loader keys `./src/a.rs`.
    let old = set("src/a.rs", json!({ "cyclomatic": { "sum": 1.0 } }));
    let new = set("src/a.rs", json!({ "cyclomatic": { "sum": 2.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    for (prefix, row) in [
        ("./src/", "a.rs.sum"),
        ("src/", "a.rs.sum"),
        ("./", "src/a.rs.sum"),
        ("./src", "/a.rs.sum"),
    ] {
        let rendered = diff.render_tty(prefix);
        assert!(
            rendered.contains(&format!("  {row}  1 ")),
            "{prefix}: {rendered}"
        );
    }

    // The key form itself: `/` keeps its one slash, and `./` names no
    // component at all.
    assert_eq!(key_prefix("/"), "/");
    assert_eq!(key_prefix("./"), "");

    let old = set("/abs/a.rs", json!({ "cyclomatic": { "sum": 1.0 } }));
    let new = set("/abs/a.rs", json!({ "cyclomatic": { "sum": 2.0 } }));
    let diff = MetricDiff::from_sets(&old, &new, 0.0, &[]);
    for (prefix, row) in [
        ("/", "abs/a.rs.sum"),
        ("/abs/", "a.rs.sum"),
        ("./", "/abs/a.rs.sum"),
    ] {
        let rendered = diff.render_tty(prefix);
        assert!(
            rendered.contains(&format!("  {row}  1 ")),
            "{prefix}: {rendered}"
        );
    }
}

/// JSON that is not a per-file metrics document (an `ops` result, an
/// array of numbers, a `metrics` that is not an object) is refused rather
/// than read as a file with no metrics, which would list every field of
/// the other side's file as added or removed.
#[test]
fn json_without_a_metrics_object_is_not_a_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cases = [
        (
            "ops.json",
            json!({ "name": "src/a.rs", "spaces": [] }),
            None,
        ),
        (
            "null.json",
            json!({ "name": "src/a.rs", "metrics": null }),
            None,
        ),
        ("number.json", json!(3), None),
        ("numbers.json", json!([3]), Some(0)),
    ];
    for (file, value, element) in cases {
        let path = dir.path().join(file);
        write_doc(&path, &value);
        let err = load_set(&path).expect_err(file);
        let expected = DocumentOrigin {
            path: path.clone(),
            element,
        };
        assert!(
            matches!(&err, DiffError::NotADocument { origin } if *origin == expected),
            "{file}: {err:?}"
        );
    }
}

/// Two documents named alike on one side used to overwrite each other in
/// load order, so the diff silently compared whichever came last. Both
/// are named, in a directory and within one aggregate.
#[test]
fn duplicate_names_on_one_side_are_an_error_naming_both() {
    let dir = tempfile::tempdir().expect("tempdir");
    let tree = dir.path().join("tree");
    write_doc(&tree.join("x.json"), &doc("src/a.rs", 1.0));
    write_doc(&tree.join("y.json"), &doc("src/a.rs", 2.0));
    let err = load_set(&tree).expect_err("duplicate in a tree");
    assert!(
        matches!(
            &err,
            DiffError::DuplicateName { name, first, second }
                if name == "src/a.rs"
                    && *first == DocumentOrigin::file(&tree.join("x.json"))
                    && *second == DocumentOrigin::file(&tree.join("y.json"))
        ),
        "{err:?}"
    );

    let aggregate = dir.path().join("agg.json");
    write_doc(
        &aggregate,
        &json!([doc("src/a.rs", 1.0), doc("src/a.rs", 2.0)]),
    );
    let err = load_set(&aggregate).expect_err("duplicate in an aggregate");
    assert!(
        matches!(
            &err,
            DiffError::DuplicateName { first, second, .. }
                if *first == DocumentOrigin::element(&aggregate, 0)
                    && *second == DocumentOrigin::element(&aggregate, 1)
        ),
        "{err:?}"
    );
}

/// A `*.json` symlink that resolves to nothing is a read error naming the
/// link. `Path::is_file` used to answer `false` for it, so the document
/// vanished from its side and the diff reported it removed, exit 0.
#[cfg(unix)]
#[test]
fn dangling_json_symlink_is_a_read_error_naming_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_doc(&dir.path().join("a.json"), &doc("src/a.rs", 1.0));
    let link = dir.path().join("b.json");
    std::os::unix::fs::symlink(dir.path().join("missing.json"), &link).expect("symlink");
    let err = load_set(dir.path()).expect_err("dangling link");
    assert!(
        matches!(&err, DiffError::Read { path, .. } if *path == link),
        "{err:?}"
    );
}

/// The symlink policy, pinned in both directions: a symlinked document is
/// read like the file it names, a symlinked directory inside the side is
/// not entered (as the source walk enters none), and a side whose root is
/// itself a symlink is followed, as a named `--paths` seed is.
#[cfg(unix)]
#[test]
fn symlinked_documents_are_read_and_symlinked_directories_are_not_entered() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let outside = dir.path().join("outside");
    write_doc(&outside.join("a.json"), &doc("src/a.rs", 1.0));
    write_doc(&outside.join("nested/b.json"), &doc("src/b.rs", 1.0));
    let side = dir.path().join("side");
    std::fs::create_dir(&side).expect("mkdir side");
    symlink(outside.join("a.json"), side.join("a.json")).expect("file link");
    symlink(outside.join("nested"), side.join("nested")).expect("dir link");

    let loaded = load_set(&side).expect("side loads");
    assert_eq!(loaded.keys().collect::<Vec<_>>(), ["src/a.rs"]);

    let root_link = dir.path().join("side-link");
    symlink(&side, &root_link).expect("root link");
    let loaded = load_set(&root_link).expect("linked root loads");
    assert_eq!(loaded.keys().collect::<Vec<_>>(), ["src/a.rs"]);
}
