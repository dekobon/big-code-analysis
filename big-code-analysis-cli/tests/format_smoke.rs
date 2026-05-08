//! End-to-end format-validity smoke tests.
//!
//! Each test runs `bca metrics -O <fmt> --paths <fixture>` against a
//! small Rust fixture in a `tempfile::tempdir()`, captures stdout, and
//! pipes the captured bytes through the matching format validator
//! (the duplicates of the lib-crate helpers under
//! `big-code-analysis-cli/tests/common/validators.rs`). This catches
//! CLI-dispatch bugs that bypass the writer-level tests in the lib
//! crate (e.g., a routing regression that emits Checkstyle XML when
//! `--output-format sarif` was requested).
//!
//! The aggregated formats (Checkstyle, SARIF) currently emit empty
//! documents because the threshold engine (issue #96) hasn't landed.
//! The validators must accept these well-formed-but-empty shapes —
//! which they do, by design — so the smoke tests still run today.
//! Once #96 lands, the same tests automatically exercise real
//! offender content with no test changes needed.

use assert_cmd::Command;
use big_code_analysis::CSV_HEADER;
use std::io::Write;
use tempfile::TempDir;

mod common;
use common::validators::{
    assert_checkstyle_well_formed_and_structural, assert_html_well_formed, validate_sarif,
};

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
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

#[test]
fn cli_sarif_output_validates_against_schema() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics("sarif", &fixture);
    if let Err(violations) = validate_sarif(&out) {
        panic!(
            "SARIF schema violations from CLI output:\n  {}\n\nfull document:\n{}",
            violations.join("\n  "),
            out,
        );
    }
}

#[test]
fn cli_checkstyle_output_is_well_formed() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics("checkstyle", &fixture);
    assert_checkstyle_well_formed_and_structural(&out);
}

#[test]
fn cli_html_output_is_well_formed() {
    // HTML is a per-file format. With no --output, the binary streams
    // the rendered HTML for the fixture to stdout.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics("html", &fixture);
    assert_html_well_formed(&out);
}

#[test]
fn cli_csv_output_round_trips_through_csv_crate() {
    // CSV is per-file like HTML. Re-parse the captured stdout via
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
