//! End-to-end format-validity smoke tests.
//!
//! Each test runs the binary against a small Rust fixture in a
//! `tempfile::tempdir()`, captures stdout, and pipes the captured
//! bytes through the matching format validator (the duplicates of
//! the lib-crate helpers under
//! `big-code-analysis-cli/tests/common/validators.rs`). This catches
//! CLI-dispatch bugs that bypass the writer-level tests in the lib
//! crate (e.g., a routing regression that emits Checkstyle XML when
//! `--output-format sarif` was requested).
//!
//! The offender formats (Checkstyle, SARIF) are emitted by
//! `bca check --output-format <fmt>`. Each test sets a tight
//! threshold against a deliberately branchy Rust fixture so the
//! `check` walk produces at least one offender record; the resulting
//! document is run through the format validator. (Clean runs are
//! valid input for the validators too, but exercising the document
//! with real offender records catches more dispatch regressions —
//! e.g. a route that emits an empty SARIF run even when offenders
//! were found.)

use assert_cmd::Command;
use big_code_analysis::CSV_HEADER;
use std::io::Write;
use tempfile::TempDir;

mod common;
use common::validators::{assert_checkstyle_well_formed_and_structural, validate_sarif};

fn cli() -> Command {
    common::bca_command()
}

fn write_rust_fixture(dir: &TempDir) -> String {
    let path = dir.path().join("fixture.rs");
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(b"fn f(x: u32) -> u32 { if x > 0 { x } else { 0 } }\n")
        .expect("write fixture");
    path.to_str().expect("fixture path is utf-8").to_string()
}

fn run_metrics(format: &str, fixture_path: &str) -> String {
    let output = cli()
        .args(["--paths", fixture_path, "metrics", "-O", format])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("CLI output is UTF-8")
}

/// Run `bca check --threshold cyclomatic=1 --output-format <fmt>
/// --no-fail` so the walk produces offender records (branchy fixture
/// vs cyclomatic=1) without bumping the exit code. Returns the
/// document stdout.
fn run_check_offender_doc(format: &str, fixture_path: &str) -> String {
    let output = cli()
        .args([
            "--paths",
            fixture_path,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            format,
            "--no-fail",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("CLI output is UTF-8")
}

#[test]
fn cli_check_sarif_output_validates_against_schema() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc("sarif", &fixture);
    if let Err(violations) = validate_sarif(&out) {
        panic!(
            "SARIF schema violations from CLI output:\n  {}\n\nfull document:\n{}",
            violations.join("\n  "),
            out,
        );
    }
    // The fixture has an `if` branch, so cyclomatic=1 produces an
    // offender. Guard against a routing regression that would emit
    // an empty results array: parse the JSON and require at least one
    // entry in `runs[0].results[]`. (A substring check on `"results"`
    // would still match the empty-array case `"results": []`.)
    let doc: serde_json::Value = serde_json::from_str(&out).expect("SARIF stdout parses as JSON");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is an array");
    assert!(
        !results.is_empty(),
        "expected at least one SARIF result for branchy fixture; doc was:\n{out}",
    );
}

#[test]
fn cli_check_checkstyle_output_is_well_formed() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc("checkstyle", &fixture);
    assert_checkstyle_well_formed_and_structural(&out);
    // Same routing-regression guard as the SARIF test: an empty
    // `<checkstyle version="4.3"/>` document is well-formed but
    // wouldn't catch a route that drops offenders silently. With
    // `cyclomatic=1` against a fixture that has an `if` branch, the
    // document must carry at least one `<file>` element with an
    // `<error>` child carrying the cyclomatic metric name.
    assert!(
        out.contains("<file"),
        "expected at least one <file> element in checkstyle output; out was:\n{out}",
    );
    assert!(
        out.contains("cyclomatic"),
        "expected cyclomatic metric in checkstyle output; out was:\n{out}",
    );
}

#[test]
fn cli_csv_output_round_trips_through_csv_crate() {
    // CSV is a per-file format. Re-parse the captured stdout via
    // the same `csv` crate the project already uses; reject any row
    // whose field count differs from CSV_HEADER, and require at least
    // one data row so we know real metric content is being emitted.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics("csv", &fixture);

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(out.as_bytes());

    let mut rows = 0;
    for record in rdr.records() {
        let record = record.expect("CLI CSV output round-trips through csv::Reader");
        // Pin against the public CSV_HEADER constant rather than the
        // first row's width — catches a regression that emits the
        // wrong number of columns consistently across all rows.
        assert_eq!(
            record.len(),
            CSV_HEADER.len(),
            "row {rows} field count {} differs from CSV_HEADER ({})",
            record.len(),
            CSV_HEADER.len(),
        );
        rows += 1;
    }
    // Header row plus at least one data row.
    assert!(
        rows >= 2,
        "expected header + at least one data row in CLI csv output, got {rows} rows"
    );
}

#[test]
fn cli_check_code_climate_output_matches_gitlab_shape() {
    // Routing + spec-compliance smoke for `--output-format code-climate`.
    // Mirrors the SARIF/Checkstyle tests above: branchy fixture +
    // tight threshold so the document carries real offenders, then
    // assert it conforms to GitLab's strict subset of the Code
    // Climate JSON spec (single array of objects, with each entry
    // carrying the five required fields and `severity` from the
    // GitLab enum).
    const REQUIRED_KEYS: &[&str] = &[
        "description",
        "check_name",
        "fingerprint",
        "severity",
        "location",
    ];
    const SEVERITY_ENUM: &[&str] = &["info", "minor", "major", "critical", "blocker"];

    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc("code-climate", &fixture);

    let doc: serde_json::Value =
        serde_json::from_str(&out).expect("code-climate stdout parses as JSON");
    let issues = doc
        .as_array()
        .expect("code-climate output must be a JSON array");
    assert!(
        !issues.is_empty(),
        "expected at least one code-climate issue for branchy fixture; doc was:\n{out}",
    );

    for (i, issue) in issues.iter().enumerate() {
        for key in REQUIRED_KEYS {
            assert!(
                issue.get(*key).is_some(),
                "issue[{i}] missing required key {key:?}; entry was:\n{issue}",
            );
        }
        let description = issue["description"]
            .as_str()
            .expect("description is a string");
        assert!(
            !description.is_empty(),
            "issue[{i}] description must not be empty (GitLab MR widget would render a blank row)",
        );
        let severity = issue["severity"].as_str().expect("severity is a string");
        assert!(
            SEVERITY_ENUM.contains(&severity),
            "issue[{i}] severity {severity:?} not in GitLab enum {SEVERITY_ENUM:?}",
        );
        let path = issue["location"]["path"]
            .as_str()
            .expect("path is a string");
        assert!(
            !path.starts_with("./"),
            "issue[{i}] location.path {path:?} starts with forbidden './'",
        );
        assert!(
            !path.contains('\\'),
            "issue[{i}] location.path {path:?} contains backslash",
        );
        assert!(
            issue["check_name"]
                .as_str()
                .expect("check_name is a string")
                .starts_with("big-code-analysis/"),
            "issue[{i}] check_name not namespaced",
        );
        let fp = issue["fingerprint"]
            .as_str()
            .expect("fingerprint is a string");
        assert_eq!(
            fp.len(),
            32,
            "issue[{i}] fingerprint must be 32 hex chars, got {fp:?}",
        );
        let begin = issue["location"]["lines"]["begin"]
            .as_u64()
            .expect("lines.begin is an integer");
        assert!(
            begin >= 1,
            "issue[{i}] lines.begin must be >= 1, got {begin}",
        );
    }

    // GitLab forbids a BOM at the start of the artifact.
    assert!(
        !out.starts_with('\u{FEFF}'),
        "code-climate output must not start with a BOM",
    );
}
