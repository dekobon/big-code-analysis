//! Integration tests for the SARIF 2.1.0 output format.
//!
//! Exercises the writer with constructed [`OffenderRecord`] batches
//! covering every severity/optional-field combination, then snapshots
//! the JSON output, round-trips it through `serde_json::Value`, and
//! validates it against the vendored Draft-07 JSON Schema.

use std::path::PathBuf;

use big_code_analysis::{OffenderRecord, Severity, write_sarif};

mod common;
use common::validators::{sarif_schema_metadata, validate_sarif};

fn assert_valid_sarif(out: &str) {
    if let Err(violations) = validate_sarif(out) {
        panic!(
            "SARIF schema violations:\n  {}\n\nfull document:\n{}",
            violations.join("\n  "),
            out,
        );
    }
}

fn render(offenders: &[OffenderRecord]) -> String {
    let mut buf = Vec::new();
    write_sarif(offenders, &mut buf).expect("writing to Vec is infallible");
    String::from_utf8(buf).expect("output is UTF-8")
}

fn parse(json: &str) -> serde_json::Value {
    serde_json::from_str(json).expect("output is valid JSON")
}

#[test]
fn sarif_zero_offenders_minimal() {
    let out = render(&[]);
    let v = parse(&out);
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(
        v["$schema"],
        "https://json.schemastore.org/sarif-2.1.0.json"
    );
    insta::assert_snapshot!("sarif_zero_offenders", out);
}

#[test]
fn sarif_multi_offender_two_files_mixed_severity() {
    let offenders = vec![
        OffenderRecord {
            path: PathBuf::from("src/alpha.rs"),
            function: Some("compute".into()),
            start_line: 12,
            end_line: 48,
            start_col: Some(4),
            metric: "cyclomatic".into(),
            value: 17.0,
            limit: 15.0,
            severity: Severity::Warning,
        },
        OffenderRecord {
            path: PathBuf::from("src/alpha.rs"),
            function: Some("compute".into()),
            start_line: 12,
            end_line: 48,
            start_col: Some(4),
            metric: "loc.lloc".into(),
            value: 250.0,
            limit: 100.0,
            severity: Severity::Warning,
        },
        // No function name, no column, error severity — exercises both
        // optional-field skips at once.
        OffenderRecord {
            path: PathBuf::from("src/zeta.rs"),
            function: None,
            start_line: 1,
            end_line: 200,
            start_col: None,
            metric: "halstead.volume".into(),
            value: 1234.5,
            limit: 1000.0,
            severity: Severity::Error,
        },
    ];

    let out = render(&offenders);
    let v = parse(&out);

    let results = v["runs"][0]["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);

    // Severity mapping
    assert_eq!(results[0]["level"], "warning");
    assert_eq!(results[2]["level"], "error");

    // Optional-field skip on the third result: no startColumn, no logicalLocations.
    let third = &results[2];
    assert!(third.get("logicalLocations").is_none());
    assert!(
        third["locations"][0]["physicalLocation"]["region"]
            .get("startColumn")
            .is_none()
    );

    // Rules deduplicated and sorted alphabetically.
    let rules = v["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules array");
    let ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["cyclomatic", "halstead.volume", "loc.lloc"]);

    insta::assert_snapshot!("sarif_multi_offender", out);
}

// --------------------------------------------------------------------
// Schema-validation tests: every emitted document must validate
// against the vendored SARIF 2.1.0 JSON Schema (Draft-07). These
// catch structural drift the per-field assertions above would miss
// (e.g., a renamed field, a wrong-typed value, an extra property the
// schema rejects).
// --------------------------------------------------------------------

#[test]
fn sarif_zero_offenders_validates_against_schema() {
    assert_valid_sarif(&render(&[]));
}

#[test]
fn sarif_single_offender_validates_against_schema() {
    let offenders = vec![OffenderRecord {
        path: PathBuf::from("src/foo.rs"),
        function: Some("bar".into()),
        start_line: 10,
        end_line: 20,
        start_col: Some(4),
        metric: "cyclomatic".into(),
        value: 17.0,
        limit: 15.0,
        severity: Severity::Warning,
    }];
    assert_valid_sarif(&render(&offenders));
}

#[test]
fn sarif_multi_offender_validates_against_schema() {
    let offenders = vec![
        OffenderRecord {
            path: PathBuf::from("src/alpha.rs"),
            function: Some("f".into()),
            start_line: 1,
            end_line: 5,
            start_col: Some(1),
            metric: "cyclomatic".into(),
            value: 17.0,
            limit: 15.0,
            severity: Severity::Warning,
        },
        OffenderRecord {
            path: PathBuf::from("src/beta.rs"),
            function: None,
            start_line: 1,
            end_line: 200,
            start_col: None,
            metric: "loc.lloc".into(),
            value: 250.0,
            limit: 100.0,
            severity: Severity::Warning,
        },
    ];
    assert_valid_sarif(&render(&offenders));
}

#[test]
fn sarif_error_severity_validates_against_schema() {
    let offenders = vec![OffenderRecord {
        path: PathBuf::from("src/foo.rs"),
        function: Some("f".into()),
        start_line: 1,
        end_line: 1,
        start_col: Some(1),
        metric: "cognitive".into(),
        value: 99.0,
        limit: 15.0,
        severity: Severity::Error,
    }];
    assert_valid_sarif(&render(&offenders));
}

#[test]
fn sarif_omitted_optional_fields_validates_against_schema() {
    // function = None -> no logicalLocations
    // start_col = None -> no startColumn
    // The schema must accept the document with both omitted.
    let offenders = vec![OffenderRecord {
        path: PathBuf::from("a.rs"),
        function: None,
        start_line: 42,
        end_line: 42,
        start_col: None,
        metric: "halstead.volume".into(),
        value: 1234.5,
        limit: 1000.0,
        severity: Severity::Warning,
    }];
    assert_valid_sarif(&render(&offenders));
}

/// Schema-canary self-check: ensures the vendored
/// `tests/fixtures/sarif-2.1.0.json` parses, declares the Draft-07
/// dialect, and carries the OASIS canonical `$id`. If a contributor
/// refreshes the fixture and accidentally vendors the wrong file
/// (e.g., a 2.0 schema or a non-self-contained re-export), this test
/// fails loud before the validation tests get to run.
#[test]
fn sarif_vendored_schema_is_draft7_and_targets_2_1_0() {
    let (id, dialect) = sarif_schema_metadata();
    assert_eq!(
        dialect, "http://json-schema.org/draft-07/schema#",
        "vendored SARIF schema is not Draft-07; refresh tests/fixtures/sarif-2.1.0.json",
    );
    assert!(
        id.contains("sarif-schema-2.1.0") || id.contains("sarif-2.1.0"),
        "vendored SARIF schema $id does not look like 2.1.0: {id}",
    );
}
