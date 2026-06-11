//! Integration tests for `bca metrics --metrics <name,...>` (issue
//! #691). The selector restricts the computed metric set via the
//! library `MetricsOptions::with_only`, so the structured output carries
//! only the requested metrics (plus their auto-resolved dependencies),
//! and an unknown name errors at parse time with a did-you-mean hint
//! (reusing the #381/#662 validator).

use std::collections::BTreeSet;

use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Branchy Rust fixture so every metric family has a non-trivial value.
const FIXTURE: &str = "fn f(x: u32) -> u32 { if x > 0 { x } else { 0 } }\n";

fn fixture() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fixture.rs");
    std::fs::write(&path, FIXTURE).unwrap();
    let path = path.to_str().unwrap().to_string();
    (dir, path)
}

/// Run `bca metrics --metrics <sel> -O json` and return the sorted set
/// of metric keys present on the file-level space.
fn metric_keys(path: &str, selector: &[&str]) -> BTreeSet<String> {
    let mut args = vec!["--paths", path, "metrics", "-O", "json"];
    for s in selector {
        args.push("--metrics");
        args.push(s);
    }
    let out = common::bca_command()
        .args(&args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doc: serde_json::Value = serde_json::from_slice(&out).expect("json");
    doc["metrics"]
        .as_object()
        .expect("metrics object")
        .keys()
        .cloned()
        .collect()
}

/// #691: a single `--metrics cyclomatic` computes only that metric.
#[test]
fn single_metric_restricts_output() {
    let (_dir, path) = fixture();
    let keys = metric_keys(&path, &["cyclomatic"]);
    assert_eq!(
        keys,
        BTreeSet::from(["cyclomatic".to_string()]),
        "only cyclomatic should be present"
    );
}

/// #691: the comma-separated form and the repeated form union the same
/// way; `cognitive` pulls in its `nom` dependency, proving deps resolve.
#[test]
fn comma_and_repeated_forms_union() {
    let (_dir, path) = fixture();
    let comma = metric_keys(&path, &["cyclomatic,cognitive"]);
    let repeated = metric_keys(&path, &["cyclomatic", "cognitive"]);
    assert_eq!(comma, repeated, "comma and repeated forms must agree");
    assert!(comma.contains("cyclomatic"));
    assert!(comma.contains("cognitive"));
}

/// #691: absent `--metrics` computes every metric — parity with the
/// pre-#691 default. The full set is strictly larger than any single
/// selection.
#[test]
fn absent_selector_computes_all_metrics() {
    let (_dir, path) = fixture();
    let all = metric_keys(&path, &[]);
    let one = metric_keys(&path, &["cyclomatic"]);
    assert!(
        all.len() > one.len(),
        "default must compute more metrics than a single selection: {all:?}"
    );
    assert!(
        all.contains("halstead"),
        "default includes halstead: {all:?}"
    );
}

/// #691: an unknown metric name errors (exit 1) with the known-names
/// list and a did-you-mean suggestion, reusing the #662 validator.
#[test]
fn unknown_metric_errors_with_suggestion() {
    let (_dir, path) = fixture();
    common::bca_command()
        .args(["--paths", &path, "metrics", "--metrics", "cylomatic"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown metric"))
        .stderr(predicate::str::contains("did you mean"))
        .stderr(predicate::str::contains("cyclomatic"));
}
