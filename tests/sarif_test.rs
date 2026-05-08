//! Integration tests for the SARIF 2.1.0 output format.
//!
//! Exercises the writer with constructed [`OffenderRecord`] batches
//! covering every severity/optional-field combination, then snapshots
//! the JSON output and round-trips it through `serde_json::Value` to
//! confirm the document parses cleanly.

use std::path::PathBuf;

use big_code_analysis::{OffenderRecord, Severity, write_sarif};

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
