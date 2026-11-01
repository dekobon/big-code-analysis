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

/// The aggregate `bca metrics -O json --output <file>` writes is a
/// top-level array of per-file documents, and `bca diff` reads each one,
/// so an aggregate capture pairs with an `--output-dir` capture of the
/// same tree. It used to read the array as one nameless document keyed
/// on the capture's own path: one file removed, one added, exit 0.
#[test]
fn metrics_aggregate_output_pairs_with_an_output_dir_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir(&src).expect("mkdir src");
    std::fs::write(src.join("a.rs"), "fn a(x: i32) -> i32 {\n    x\n}\n").expect("write flat");
    cli()
        .current_dir(dir.path())
        .args([
            "metrics",
            "-p",
            "src",
            "-O",
            "json",
            "--output",
            "before.json",
        ])
        .assert()
        .success();
    std::fs::write(
        src.join("a.rs"),
        "fn a(x: i32) -> i32 {\n    if x > 0 { 1 } else { 0 }\n}\n",
    )
    .expect("write branchy");
    cli()
        .current_dir(dir.path())
        .args([
            "metrics",
            "-p",
            "src",
            "-O",
            "json",
            "--output-dir",
            "after",
        ])
        .assert()
        .success();

    let assert = cli()
        .current_dir(dir.path())
        .args(["diff", "before.json", "after", "--format", "json"])
        .assert()
        .success();
    let doc: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).expect("json");
    assert_eq!(doc["summary"]["added_files"], 0, "{doc}");
    assert_eq!(doc["summary"]["removed_files"], 0, "{doc}");
    assert_eq!(
        doc["buckets"]["cyclomatic"]["changed"][0]["file"], "src/a.rs",
        "{doc}"
    );
}

/// One tree captured as `-p ./src` and as `-p src` records `name` as
/// `./src/a.rs` and `src/a.rs`. The two spellings name one file, so they
/// pair as a change; keyed verbatim they read as one file removed and
/// another added.
#[test]
fn captures_differing_only_in_a_dot_component_pair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir(&src).expect("mkdir src");
    std::fs::write(src.join("a.rs"), "fn a(x: i32) -> i32 {\n    x\n}\n").expect("write flat");
    cli()
        .current_dir(dir.path())
        .args([
            "metrics",
            "-p",
            "./src",
            "-O",
            "json",
            "--output-dir",
            "before",
        ])
        .assert()
        .success();
    std::fs::write(
        src.join("a.rs"),
        "fn a(x: i32) -> i32 {\n    if x > 0 { 1 } else { 0 }\n}\n",
    )
    .expect("write branchy");
    cli()
        .current_dir(dir.path())
        .args([
            "metrics",
            "-p",
            "src",
            "-O",
            "json",
            "--output",
            "after.json",
        ])
        .assert()
        .success();

    let assert = cli()
        .current_dir(dir.path())
        .args(["diff", "before", "after.json", "--format", "json"])
        .assert()
        .success();
    let doc: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).expect("json");
    assert_eq!(doc["summary"]["added_files"], 0, "{doc}");
    assert_eq!(doc["summary"]["removed_files"], 0, "{doc}");
    assert_eq!(
        doc["buckets"]["cyclomatic"]["changed"][0]["file"], "src/a.rs",
        "{doc}"
    );
}

/// A directory side reads only `*.json`, so a capture written as YAML
/// holds no input file. Two of them used to diff as "0 changed", exit 0
/// under `--exit-code`, whatever the trees held; it is now an error
/// naming the directory.
#[test]
fn a_directory_side_with_no_json_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir(&src).expect("mkdir src");
    std::fs::write(src.join("a.rs"), "fn a() {}\n").expect("write source");
    cli()
        .current_dir(dir.path())
        .args(["metrics", "-p", "src", "-O", "yaml", "--output-dir", "yaml"])
        .assert()
        .success();

    cli()
        .current_dir(dir.path())
        .args(["diff", "--exit-code", "yaml", "yaml"])
        .assert()
        .code(1)
        .stdout(predicates::str::is_empty())
        .stderr(predicates::str::contains("yaml holds no `*.json` file"));
}

/// `--strip-prefix /` strips the root off an absolute document name, as
/// a capture taken with `-p /abs/path` carries one. The prefix is put in
/// key form, and the key of `/` is already `/`, so restoring the
/// trailing slash used to make it `//`, which matched nothing.
#[test]
fn strip_prefix_slash_strips_an_absolute_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    std::fs::write(
        &old,
        r#"{"name":"/abs/a.rs","metrics":{"cyclomatic":{"sum":1}}}"#,
    )
    .expect("write old");
    std::fs::write(
        &new,
        r#"{"name":"/abs/a.rs","metrics":{"cyclomatic":{"sum":2}}}"#,
    )
    .expect("write new");

    let assert = cli()
        .args(["diff", arg(&old), arg(&new), "--strip-prefix", "/"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains("  abs/a.rs.sum  1 "), "{stdout}");
    assert!(!stdout.contains("/abs/a.rs"), "{stdout}");
}
