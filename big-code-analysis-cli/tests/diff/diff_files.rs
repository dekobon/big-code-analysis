//! Integration tests for file-mode `bca diff <old> <new>` over two
//! captured per-file metric documents.

use std::path::Path;

use assert_cmd::Command;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

/// A tempdir path as the `&str` the CLI positional needs.
fn arg(path: &Path) -> &str {
    path.to_str().expect("tempdir path is UTF-8")
}

/// #1623: two captures of one file taken with different `--metrics`
/// sets. The family the old capture never computed is listed as added
/// fields, not diffed against an invented `0 → n`, in every renderer —
/// and it still counts as a difference for `--exit-code`.
#[test]
fn one_sided_field_is_listed_and_counts_for_exit_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    std::fs::write(
        &old,
        r#"{"name":"src/a.rs","metrics":{"cyclomatic":{"sum":3}}}"#,
    )
    .expect("write old");
    std::fs::write(
        &new,
        r#"{"name":"src/a.rs","metrics":{"cyclomatic":{"sum":3},"mi":{"original":100.5}}}"#,
    )
    .expect("write new");

    cli()
        .args(["diff", arg(&old), arg(&new)])
        .assert()
        .success()
        .stdout(
            "0 metric(s) changed, 0 added file(s), 0 removed file(s), \
             1 added field(s), 0 removed field(s)\n\
             \n\
             ## Added fields\n\
             \x20 src/a.rs  mi  original\n",
        );

    let assert = cli()
        .args(["diff", arg(&old), arg(&new), "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(doc["summary"]["total_changes"], 0);
    assert_eq!(doc["summary"]["added_fields"], 1);
    assert_eq!(doc["buckets"], serde_json::json!({}));
    assert_eq!(
        doc["added_fields"],
        serde_json::json!([{ "file": "src/a.rs", "metric": "mi", "field": "original" }])
    );

    cli()
        .args(["diff", arg(&old), arg(&new), "--exit-code"])
        .assert()
        .code(2);
}
