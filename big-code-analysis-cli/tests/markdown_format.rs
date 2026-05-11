#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli() -> Command {
    Command::cargo_bin("bca").unwrap()
}

/// A small fixture file known to the repo, resolved relative to the workspace
/// root so the path is valid regardless of the test runner's CWD.
fn fixture_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../big-code-analysis-cli
    let workspace = std::path::Path::new(manifest)
        .parent()
        .expect("manifest dir has parent");
    workspace
        .join("tests/repositories/DeepSpeech/stats.py")
        .to_str()
        .expect("path is utf-8")
        .to_string()
}

#[test]
fn report_help_lists_format_top_and_strip_prefix() {
    cli()
        .args(["report", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("markdown"))
        .stdout(predicate::str::contains("--top"))
        .stdout(predicate::str::contains("--strip-prefix"));
}

#[test]
fn report_requires_a_format() {
    cli()
        .args(["report"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<FORMAT>"));
}

#[test]
fn report_top_zero_rejected() {
    cli()
        .args([
            "report",
            "markdown",
            "--top",
            "0",
            "--paths",
            &fixture_path(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--top"));
}

#[test]
fn report_output_must_not_be_directory() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "report",
            "markdown",
            "--output",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--output must be a file path for `report`",
        ));
}

#[test]
fn report_output_parent_must_exist() {
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "report",
            "markdown",
            "--output",
            "/tmp/nonexistent_dir_12345/report.md",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "parent directory of --output does not exist",
        ));
}

#[test]
fn report_markdown_to_stdout() {
    cli()
        .args(["--paths", &fixture_path(), "report", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Code Quality Metrics Summary"))
        .stdout(predicate::str::contains("**Files analyzed:** 1"));
}

#[test]
fn report_markdown_to_file() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("report.md");
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "report",
            "markdown",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("# Code Quality Metrics Summary"),
        "expected markdown report header in file output",
    );
    assert!(
        content.contains("**Files analyzed:** 1"),
        "expected file count in report",
    );
    assert!(
        content.contains("## Python"),
        "expected per-language section for Python",
    );
}

#[test]
fn report_collects_nonzero_summaries() {
    cli()
        .args(["--paths", &fixture_path(), "report", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("**Functions/methods:** 0").not())
        .stdout(predicate::str::contains("**Functions/methods:**"))
        .stdout(predicate::str::contains("## Per-language overview"));
}

#[test]
fn report_with_no_paths_produces_empty_summary() {
    cli()
        .args(["report", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("**Files analyzed:** 0"));
}

#[test]
fn report_is_deterministic_across_runs() {
    let fp = fixture_path();
    let outputs: Vec<Vec<u8>> = (0..5)
        .map(|_| {
            cli()
                .args(["--paths", &fp, "report", "markdown"])
                .output()
                .unwrap()
                .stdout
        })
        .collect();
    assert!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "non-deterministic output across 5 runs"
    );
}

#[test]
fn metrics_cbor_without_output_rejects_cleanly() {
    cli()
        .args(["--paths", &fixture_path(), "metrics", "-O", "cbor"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "CBOR is binary and cannot be printed to stdout; use --output",
        ));
}

#[test]
fn report_renders_nonzero_tokens_for_real_file() {
    let output = cli()
        .args(["--paths", &fixture_path(), "report", "markdown"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mi_section = stdout
        .split_once("### Maintainability Index")
        .expect("MI section present")
        .1;

    let split_cells = |row: &str| -> Vec<String> {
        row.trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect()
    };

    let mut table_rows = mi_section.lines().filter(|l| l.starts_with('|'));
    let header = split_cells(table_rows.next().expect("MI header row"));
    let tokens_idx = header
        .iter()
        .position(|c| c == "Tokens")
        .unwrap_or_else(|| panic!("MI table missing Tokens column. Header: {header:?}"));
    table_rows.next(); // skip GFM separator row
    let data_row = split_cells(
        table_rows
            .find(|l| l.contains("stats.py"))
            .expect("stats.py row in MI table"),
    );
    let tokens: u64 = data_row[tokens_idx]
        .replace(',', "")
        .parse()
        .expect("Tokens column should be a numeric cell");
    assert!(
        tokens > 0,
        "Tokens cell for stats.py should be non-zero, got {tokens}"
    );
}

#[test]
fn report_strip_prefix_removes_path_prefix() {
    let fp = fixture_path();
    let prefix = {
        let idx = fp
            .find("DeepSpeech/")
            .expect("fixture contains DeepSpeech/");
        &fp[..idx]
    };
    let output = cli()
        .args([
            "--paths",
            &fp,
            "report",
            "markdown",
            "--strip-prefix",
            prefix,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DeepSpeech/stats.py"),
        "stripped path should appear in report"
    );
    assert!(
        !stdout.contains(&fp),
        "full path should be stripped from report, but found: {fp}"
    );
}
