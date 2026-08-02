#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

/// A small fixture file known to the repo, resolved relative to the workspace
/// root so the path is valid regardless of the test runner's CWD. The shared
/// helper makes a missing integration corpus name itself (#1171).
fn fixture_path() -> String {
    common::corpus_fixture_path()
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
fn report_defaults_to_markdown_format() {
    // Issue #513: `bca report` with no format selector now defaults to
    // the markdown report rather than erroring on a missing positional.
    cli()
        .args(["report", "--paths", &fixture_path()])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Code Quality Metrics Summary"));
}

#[test]
fn report_canonical_format_flag_selects_markdown() {
    // The canonical `--format` spelling (issue #513) selects the same
    // markdown report the legacy positional did.
    cli()
        .args(["report", "--paths", &fixture_path(), "--format", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Code Quality Metrics Summary"));
}

#[test]
fn report_top_zero_shows_all() {
    // Issue #602 unified `0 = all`: `--top 0` is no longer a usage error but
    // a valid "show every row" request, and the MI title says "all".
    cli()
        .args([
            "report",
            "--paths",
            &fixture_path(),
            "markdown",
            "--top",
            "0",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Maintainability Index hotspots (all, by MI)",
        ));
}

#[test]
fn report_output_must_not_be_directory() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "report",
            "--paths",
            &fixture_path(),
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
            "report",
            "--paths",
            &fixture_path(),
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
        .args(["report", "--paths", &fixture_path(), "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Code Quality Metrics Summary"))
        // Global header is a two-column table (issue #671); the label cell is
        // left-aligned so it starts right after the `| ` regardless of padding.
        .stdout(predicate::str::contains("| Metric "))
        .stdout(predicate::str::contains("| Files analyzed"));
}

#[test]
fn report_markdown_to_file() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("report.md");
    cli()
        .args([
            "report",
            "--paths",
            &fixture_path(),
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
        content.contains("| Files analyzed"),
        "expected file-count row in global header table",
    );
    assert!(
        content.contains("## Python"),
        "expected per-language section for Python",
    );
}

#[test]
fn report_collects_nonzero_summaries() {
    let output = cli()
        .args(["report", "--paths", &fixture_path(), "markdown"])
        .output()
        .expect("run bca report");
    assert!(output.status.success(), "report should succeed");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    assert!(
        stdout.contains("## Per-language overview"),
        "expected per-language overview section",
    );
    // Global header is a two-column table (issue #671). The Functions/methods
    // row must exist and carry a non-zero value (right-aligned `| N |`).
    let func_row = stdout
        .lines()
        .find(|l| l.contains("Functions/methods"))
        .expect("Functions/methods row present in global header table");
    // Cells are `| label | value |`; the value is the second pipe-delimited
    // field, padded by alignment, so trim it before comparing.
    let value = func_row
        .split('|')
        .nth(2)
        .map(str::trim)
        .expect("Functions/methods row has a value cell");
    assert_ne!(value, "0", "Functions/methods value should be non-zero");
}

#[test]
fn report_with_no_matching_files_produces_empty_summary() {
    // Point at an empty directory so the walk resolves zero files. Since
    // #596 a pathless `report` defaults to `.` and would walk the
    // runner's cwd, so the empty-input case is now an explicit empty
    // `--paths` target. `--no-config` keeps it hermetic from the repo's
    // root `bca.toml`.
    let empty = TempDir::new().expect("tempdir");
    cli()
        .args([
            "report",
            "--no-config",
            "--paths",
            empty.path().to_str().expect("utf-8"),
            "markdown",
        ])
        .assert()
        .success()
        // Global header is a two-column table (issue #671) and, for an empty
        // walk, its Files row reads 0 (padding-tolerant: the value column is
        // right-aligned, so its width depends on the data).
        .stdout(predicate::str::is_match(r"\| Files analyzed\s+\|\s+0 \|").expect("valid regex"));
}

#[test]
fn report_is_deterministic_across_runs() {
    let fp = fixture_path();
    let outputs: Vec<Vec<u8>> = (0..5)
        .map(|_| {
            cli()
                .args(["report", "--paths", &fp, "markdown"])
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
        .args(["metrics", "--paths", &fixture_path(), "-O", "cbor"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "CBOR is binary and cannot be printed to stdout; use --output",
        ));
}

#[test]
fn report_renders_nonzero_tokens_for_real_file() {
    let output = cli()
        .args(["report", "--paths", &fixture_path(), "markdown"])
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
    let prefix = common::corpus_fixture_strip_prefix();
    let suffix = common::corpus_fixture_suffix();
    let output = cli()
        .args([
            "report",
            "--paths",
            &fp,
            "markdown",
            "--strip-prefix",
            &prefix,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&suffix),
        "stripped path {suffix:?} should appear in report"
    );
    // `--strip-prefix` rewrites the per-file table paths only. The provenance
    // footer (issue #680) deliberately records the literal seed path the user
    // passed (what was scanned), which here is the full `fp`; exclude that
    // footer line from the "stripped everywhere" check.
    let body = stdout
        .split_once("_Generated by bca ")
        .map_or(stdout.as_ref(), |(head, _)| head);
    assert!(
        !body.contains(&fp),
        "full path should be stripped from the report body, but found: {fp}"
    );
}
