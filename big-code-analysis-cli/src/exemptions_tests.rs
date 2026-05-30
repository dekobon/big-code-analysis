// Sibling-file unit tests for the `bca exemptions` renderers, wired in
// via `#[path = "exemptions_tests.rs"] mod tests;` so the production
// `exemptions.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

use std::collections::BTreeSet;

use big_code_analysis::{
    MetricKind, SuppressionDialect, SuppressionMarker, SuppressionScope, SuppressionTarget,
};
use serde_json::Value;

use super::*;

fn marker(
    line: usize,
    target: SuppressionTarget,
    dialect: SuppressionDialect,
    scope: SuppressionScope,
    function: Option<&str>,
) -> SuppressionMarker {
    SuppressionMarker {
        line,
        target,
        scope,
        dialect,
        function: function.map(str::to_owned),
    }
}

fn metric_scope(metrics: &[MetricKind]) -> SuppressionScope {
    SuppressionScope::Some(metrics.iter().copied().collect::<BTreeSet<_>>())
}

fn sample_report() -> ExemptionsReport {
    ExemptionsReport {
        markers: Some(vec![
            MarkerRow {
                path: "src/parser.rs".to_owned(),
                marker: marker(
                    120,
                    SuppressionTarget::Function,
                    SuppressionDialect::Native,
                    SuppressionScope::All,
                    Some("parse_long"),
                ),
            },
            MarkerRow {
                path: "src/bar.rs".to_owned(),
                marker: marker(
                    1,
                    SuppressionTarget::File,
                    SuppressionDialect::Native,
                    SuppressionScope::All,
                    None,
                ),
            },
            MarkerRow {
                path: "src/baz.rs".to_owned(),
                marker: marker(
                    45,
                    SuppressionTarget::Function,
                    SuppressionDialect::Lizard,
                    metric_scope(&[MetricKind::Cyclomatic, MetricKind::Cognitive]),
                    Some("helper_fn"),
                ),
            },
        ]),
        excludes: Some(vec!["tests/**".to_owned(), "src/languages/*.rs".to_owned()]),
        baseline: Some(BaselineSection {
            path: ".bca-baseline.toml".to_owned(),
            entries: vec![BaselineRow {
                path: "src/markdown_report.rs".to_owned(),
                qualified: "write_language_section".to_owned(),
                metric: "cognitive".to_owned(),
                value: 29.0,
                start_line: 88,
            }],
        }),
    }
}

#[test]
fn tty_lists_all_three_sections_with_counts() {
    let out = sample_report()
        .render(OutputFormat::Tty, "")
        .expect("tty render");
    assert!(out.contains("# In-source markers (3)"), "got: {out}");
    assert!(out.contains("# [check.exclude] globs (2)"), "got: {out}");
    assert!(
        out.contains("# Baseline (.bca-baseline.toml, 1 entry)"),
        "got: {out}"
    );
    // Marker syntax label, metric scope, and enclosing function.
    assert!(out.contains("bca: suppress"), "got: {out}");
    assert!(out.contains("parse_long"), "got: {out}");
    // File-scoped marker reads "(whole file)" and elides the function.
    assert!(out.contains("(whole file)"), "got: {out}");
    // Lizard marker with an explicit metric list.
    assert!(out.contains("#lizard forgives"), "got: {out}");
    assert!(out.contains("metrics=cognitive, cyclomatic"), "got: {out}");
    // Exclude globs and baseline entry surface verbatim.
    assert!(out.contains("tests/**"), "got: {out}");
    assert!(
        out.contains("write_language_section cognitive 29"),
        "got: {out}"
    );
}

#[test]
fn markdown_uses_tables_and_marker_syntax() {
    let out = sample_report()
        .render(OutputFormat::Markdown, "")
        .expect("markdown render");
    assert!(out.contains("## In-source markers (3)"), "got: {out}");
    assert!(
        out.contains("| File | Line | Marker | Metrics | Function |"),
        "got: {out}"
    );
    assert!(out.contains("`bca: suppress-file`"), "got: {out}");
    assert!(out.contains("## [check.exclude] globs (2)"), "got: {out}");
    assert!(out.contains("- `tests/**`"), "got: {out}");
    assert!(
        out.contains("## Baseline (`.bca-baseline.toml`, 1 entry)"),
        "got: {out}"
    );
    assert!(
        out.contains("| File | Line | Symbol | Metric | Value |"),
        "got: {out}"
    );
}

#[test]
fn json_nests_three_sections_under_suppressions_envelope() {
    let out = sample_report()
        .render(OutputFormat::Json, "")
        .expect("json render");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    let s = &v["suppressions"];
    assert!(s["markers"].is_array(), "markers must be an array: {out}");
    assert!(s["excludes"].is_array(), "excludes must be an array: {out}");
    assert!(s["baseline"].is_array(), "baseline must be an array: {out}");
    // Marker shape: target/dialect snake_case, scope tagged enum.
    let m0 = &s["markers"][0];
    assert_eq!(m0["path"], "src/parser.rs");
    assert_eq!(m0["line"], 120);
    assert_eq!(m0["target"], "function");
    assert_eq!(m0["dialect"], "native");
    assert_eq!(m0["scope"]["kind"], "all");
    assert_eq!(m0["function"], "parse_long");
    // Explicit metric list serializes under the `some` tag.
    let m2 = &s["markers"][2];
    assert_eq!(m2["scope"]["kind"], "some");
    assert!(m2["scope"]["metrics"].is_array());
    // Excludes are plain strings; baseline carries the identity tuple.
    assert_eq!(s["excludes"][0], "tests/**");
    assert_eq!(s["baseline"][0]["qualified"], "write_language_section");
    assert_eq!(s["baseline"][0]["metric"], "cognitive");
    assert_eq!(s["baseline"][0]["value"], 29.0);
}

#[test]
fn json_omitted_sections_are_null_not_empty() {
    // `--only-markers` leaves excludes/baseline as `None`, which must
    // serialize to JSON `null` (not requested) — distinct from `[]`
    // (requested, empty), a distinction `jq` filters rely on.
    let report = ExemptionsReport {
        markers: Some(Vec::new()),
        excludes: None,
        baseline: None,
    };
    let out = report.render(OutputFormat::Json, "").expect("json render");
    let v: Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(v["suppressions"]["markers"].is_array());
    assert!(v["suppressions"]["markers"].as_array().unwrap().is_empty());
    assert!(v["suppressions"]["excludes"].is_null());
    assert!(v["suppressions"]["baseline"].is_null());
}

#[test]
fn empty_requested_sections_render_explicit_none() {
    let report = ExemptionsReport {
        markers: Some(Vec::new()),
        excludes: Some(Vec::new()),
        baseline: Some(BaselineSection {
            path: ".bca-baseline.toml".to_owned(),
            entries: Vec::new(),
        }),
    };
    let tty = report.render(OutputFormat::Tty, "").expect("tty render");
    assert!(tty.contains("# In-source markers (0)"), "got: {tty}");
    assert_eq!(tty.matches("(none)").count(), 3, "got: {tty}");

    let md = report
        .render(OutputFormat::Markdown, "")
        .expect("md render");
    assert_eq!(md.matches("_None._").count(), 3, "got: {md}");
}

#[test]
fn strip_prefix_trims_displayed_paths_in_every_format() {
    let prefix = "src/";
    let tty = sample_report()
        .render(OutputFormat::Tty, prefix)
        .expect("tty");
    assert!(tty.contains("parser.rs:120"), "got: {tty}");
    assert!(!tty.contains("src/parser.rs"), "prefix not stripped: {tty}");

    let json = sample_report()
        .render(OutputFormat::Json, prefix)
        .expect("json");
    let v: Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(v["suppressions"]["markers"][0]["path"], "parser.rs");
    assert_eq!(
        v["suppressions"]["baseline"][0]["path"],
        "markdown_report.rs"
    );
}

#[test]
fn dead_function_marker_reads_no_enclosing_fn() {
    // A function-scoped marker with no enclosing function silences
    // nothing; the audit flags it explicitly rather than leaving the
    // cell blank.
    let report = ExemptionsReport {
        markers: Some(vec![MarkerRow {
            path: "src/top.rs".to_owned(),
            marker: marker(
                1,
                SuppressionTarget::Function,
                SuppressionDialect::Native,
                SuppressionScope::All,
                None,
            ),
        }]),
        excludes: None,
        baseline: None,
    };
    let tty = report.render(OutputFormat::Tty, "").expect("tty");
    assert!(tty.contains("(no enclosing fn)"), "got: {tty}");
}

#[test]
fn empty_metric_list_marker_renders_none_metrics() {
    // `bca: suppress()` parses to an empty explicit set — a marker that
    // covers no metrics. The renderer must distinguish this from `all`.
    let report = ExemptionsReport {
        markers: Some(vec![MarkerRow {
            path: "src/x.rs".to_owned(),
            marker: marker(
                5,
                SuppressionTarget::Function,
                SuppressionDialect::Native,
                metric_scope(&[]),
                Some("f"),
            ),
        }]),
        excludes: None,
        baseline: None,
    };
    let tty = report.render(OutputFormat::Tty, "").expect("tty");
    assert!(tty.contains("metrics=none"), "got: {tty}");
}
