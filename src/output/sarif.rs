//! SARIF 2.1.0 writer for [`OffenderRecord`] batches.
//!
//! SARIF (Static Analysis Results Interchange Format) is the OASIS
//! standard ingested natively by GitHub Code Scanning and most modern
//! IDE/security tooling. Lizard does not have a SARIF output, so this
//! is the obvious modern target for `big-code-analysis` integrations.
//!
//! We model only the subset of SARIF we actually emit as a small set
//! of `Serialize` structs (no `sarif` crate dependency). The shape:
//!
//! ```json
//! {
//!   "version": "2.1.0",
//!   "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
//!   "runs": [{
//!     "tool": { "driver": { "name": "big-code-analysis", "version": "...",
//!                            "rules": [ { "id": "cyclomatic", ... } ] } },
//!     "results": [ { "ruleId": "...", "level": "warning", ... } ]
//!   }]
//! }
//! ```

use std::collections::BTreeSet;
use std::io::{self, Write};

use serde::Serialize;

#[cfg(test)]
use crate::output::offenders::Severity;
use crate::output::offenders::{OffenderRecord, TOOL_ID, warn_non_utf8_path};
use crate::output::rule_descriptions::rule_description;

/// SARIF schema URL — pinned to 2.1.0 (the version GitHub Code
/// Scanning ingests).
const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const SARIF_VERSION: &str = "2.1.0";

/// Convert an OS path string into a SARIF `artifactLocation.uri`
/// value (an RFC 3986 URI reference).
///
/// SARIF 2.1.0 §3.4.4 requires `artifactLocation.uri` be a valid URI
/// reference. Backslash separators (Windows paths) and characters
/// outside the URI unreserved/reserved sets break that — the
/// json-schema validator GitHub Code Scanning uses rejects them
/// under the `uri-reference` format. We:
///
/// - Normalize separators to `/`.
/// - Percent-encode any byte outside the URI unreserved set + `/`
///   so spaces and other path characters survive validation.
/// - For absolute Windows paths beginning with a drive letter
///   (`C:\…` → `C:/…`), prefix with `file:///` so the leading `C:`
///   is not interpreted as a URI scheme.
fn path_to_uri_reference(path: &str) -> String {
    let bytes = path.as_bytes();
    let is_windows_drive_abs = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\');

    let mut out = String::with_capacity(path.len() + if is_windows_drive_abs { 8 } else { 0 });
    if is_windows_drive_abs {
        out.push_str("file:///");
    }
    for &b in bytes {
        match b {
            b'\\' => out.push('/'),
            // RFC 3986 unreserved + path separator + segment-safe sub-delims +
            // ':' '@' (allowed in path) + '%' would need its own escaping but
            // raw paths from the OS will not contain it pre-encoded.
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/'
            | b':'
            | b'@' => out.push(b as char),
            _ => {
                let hi = b >> 4;
                let lo = b & 0xF;
                out.push('%');
                out.push(hex_digit(hi));
                out.push(hex_digit(lo));
            }
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => '0',
    }
}

/// Write a SARIF 2.1.0 document for `offenders` to `writer`.
///
/// Offenders whose path is not valid UTF-8 are skipped with a warning
/// to stderr (SARIF `artifactLocation.uri` requires a UTF-8 string).
/// The empty case emits a well-formed run with empty `results: []` and
/// `rules: []` so snapshots are stable and CI consumers can already
/// integrate before the threshold engine (#96) lands.
///
/// # Errors
///
/// Returns any [`io::Error`] produced by `writer` while emitting the
/// SARIF JSON document, or a `serde_json::Error` (mapped to `io::Error`
/// via `io::Error::other`) if a record cannot be serialised.
pub fn write_sarif<W: Write>(offenders: &[OffenderRecord], mut writer: W) -> io::Result<()> {
    let mut results: Vec<SarifResult<'_>> = Vec::with_capacity(offenders.len());
    // BTreeSet so the rules array is deterministic (alphabetical by id).
    let mut rule_ids: BTreeSet<&str> = BTreeSet::new();

    for record in offenders {
        let Some(path_str) = warn_non_utf8_path("SARIF", &record.path) else {
            continue;
        };
        rule_ids.insert(record.metric.as_str());

        let logical_locations = record.function.as_deref().map(|name| {
            vec![LogicalLocation {
                fully_qualified_name: name,
            }]
        });

        results.push(SarifResult {
            rule_id: &record.metric,
            level: record.severity.as_str(),
            message: Message {
                text: record.default_message(),
            },
            locations: vec![Location {
                physical_location: PhysicalLocation {
                    artifact_location: ArtifactLocation {
                        uri: path_to_uri_reference(path_str),
                    },
                    region: Region {
                        start_line: record.start_line.max(1),
                        end_line: Some(record.end_line.max(record.start_line.max(1))),
                        start_column: record.start_col,
                    },
                },
                logical_locations,
            }],
        });
    }

    let rules: Vec<Rule<'_>> = rule_ids
        .iter()
        .map(|id| Rule {
            id,
            short_description: Description {
                text: rule_description(id).unwrap_or(*id),
            },
        })
        .collect();

    let log = SarifLog {
        schema: SARIF_SCHEMA,
        version: SARIF_VERSION,
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: TOOL_ID,
                    version: env!("CARGO_PKG_VERSION"),
                    rules,
                },
            },
            results,
        }],
    };

    serde_json::to_writer_pretty(&mut writer, &log)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // `serde_json::to_writer_pretty` does not append a trailing
    // newline; add one so the output is POSIX-friendly and snapshot
    // diffs stay clean.
    writer.write_all(b"\n")
}

#[derive(Serialize)]
struct SarifLog<'a> {
    #[serde(rename = "$schema")]
    schema: &'a str,
    version: &'a str,
    runs: Vec<Run<'a>>,
}

#[derive(Serialize)]
struct Run<'a> {
    tool: Tool<'a>,
    results: Vec<SarifResult<'a>>,
}

#[derive(Serialize)]
struct Tool<'a> {
    driver: Driver<'a>,
}

#[derive(Serialize)]
struct Driver<'a> {
    name: &'a str,
    version: &'a str,
    rules: Vec<Rule<'a>>,
}

#[derive(Serialize)]
struct Rule<'a> {
    id: &'a str,
    #[serde(rename = "shortDescription")]
    short_description: Description<'a>,
}

#[derive(Serialize)]
struct Description<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult<'a> {
    rule_id: &'a str,
    level: &'static str,
    message: Message,
    locations: Vec<Location<'a>>,
}

#[derive(Serialize)]
struct Message {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Location<'a> {
    physical_location: PhysicalLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical_locations: Option<Vec<LogicalLocation<'a>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: ArtifactLocation,
    region: Region,
}

#[derive(Serialize)]
struct ArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_column: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogicalLocation<'a> {
    fully_qualified_name: &'a str,
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn rec(path: &str, metric: &str, value: f64, limit: f64) -> OffenderRecord {
        OffenderRecord {
            path: PathBuf::from(path),
            function: Some("f".into()),
            start_line: 42,
            end_line: 50,
            start_col: Some(5),
            metric: metric.into(),
            value,
            limit,
            severity: Severity::Warning,
        }
    }

    fn render(offenders: &[OffenderRecord]) -> String {
        let mut buf = Vec::new();
        write_sarif(offenders, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    #[test]
    fn empty_emits_minimal_valid_run() {
        let out = render(&[]);
        // Round-trips cleanly through serde_json so we know it parses.
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["version"], "2.1.0");
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "big-code-analysis");
        assert!(
            v["runs"][0]["results"]
                .as_array()
                .expect("array")
                .is_empty()
        );
        assert!(
            v["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .expect("array")
                .is_empty()
        );
    }

    #[test]
    fn single_offender_includes_rule_and_result() {
        let offenders = vec![rec("src/foo.rs", "cyclomatic", 17.0, 15.0)];
        let out = render(&offenders);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let result = &v["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "cyclomatic");
        assert_eq!(result["level"], "warning");
        assert_eq!(result["message"]["text"], "cyclomatic 17 exceeds limit 15");
        let loc = &result["locations"][0];
        assert_eq!(
            loc["physicalLocation"]["artifactLocation"]["uri"],
            "src/foo.rs"
        );
        assert_eq!(loc["physicalLocation"]["region"]["startLine"], 42);
        assert_eq!(loc["physicalLocation"]["region"]["endLine"], 50);
        assert_eq!(loc["physicalLocation"]["region"]["startColumn"], 5);
        assert_eq!(loc["logicalLocations"][0]["fullyQualifiedName"], "f");

        let rule = &v["runs"][0]["tool"]["driver"]["rules"][0];
        assert_eq!(rule["id"], "cyclomatic");
        assert!(rule["shortDescription"]["text"].is_string());
    }

    #[test]
    fn error_severity_maps_to_error_level() {
        let mut r = rec("a.rs", "cyclomatic", 99.0, 15.0);
        r.severity = Severity::Error;
        let out = render(&[r]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["runs"][0]["results"][0]["level"], "error");
    }

    #[test]
    fn missing_column_omits_field() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_col = None;
        let out = render(&[r]);
        assert!(!out.contains("startColumn"), "{out}");
    }

    #[test]
    fn missing_function_omits_logical_locations() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.function = None;
        let out = render(&[r]);
        assert!(!out.contains("logicalLocations"), "{out}");
    }

    #[test]
    fn rules_deduplicate_per_metric() {
        let offenders = vec![
            rec("a.rs", "cyclomatic", 17.0, 15.0),
            rec("b.rs", "cyclomatic", 20.0, 15.0),
            rec("a.rs", "loc.lloc", 250.0, 100.0),
        ];
        let out = render(&offenders);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let rules = v["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("array");
        assert_eq!(rules.len(), 2);
        // BTreeSet iteration order: alphabetical.
        assert_eq!(rules[0]["id"], "cyclomatic");
        assert_eq!(rules[1]["id"], "loc.lloc");
    }

    #[test]
    fn unknown_metric_falls_back_to_metric_name_as_description() {
        let r = rec("a.rs", "made.up.metric", 1.0, 0.0);
        let out = render(&[r]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"][0]["shortDescription"]["text"],
            "made.up.metric"
        );
    }

    #[test]
    fn start_line_zero_is_clamped_to_one() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_line = 0;
        r.end_line = 0;
        let out = render(&[r]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(
            v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            1
        );
    }

    #[test]
    fn driver_version_matches_pkg_version() {
        let out = render(&[]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["version"],
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn windows_drive_path_becomes_file_uri() {
        // Windows absolute path: backslashes flip to /, drive letter
        // gets wrapped in `file:///` so it isn't parsed as a scheme.
        assert_eq!(
            path_to_uri_reference(r"C:\Users\RUNNER~1\AppData\Local\Temp\fixture.rs"),
            "file:///C:/Users/RUNNER~1/AppData/Local/Temp/fixture.rs"
        );
    }

    #[test]
    fn posix_relative_path_is_unchanged() {
        assert_eq!(path_to_uri_reference("src/foo.rs"), "src/foo.rs");
    }

    #[test]
    fn posix_absolute_path_keeps_leading_slash() {
        assert_eq!(path_to_uri_reference("/tmp/foo.rs"), "/tmp/foo.rs");
    }

    #[test]
    fn space_is_percent_encoded() {
        assert_eq!(path_to_uri_reference("src/my file.rs"), "src/my%20file.rs");
    }

    #[test]
    fn empty_snapshot_is_stable() {
        insta::assert_snapshot!("sarif_empty", render(&[]));
    }

    #[test]
    fn multi_offender_snapshot_is_stable() {
        let mut err = rec("src/zeta.rs", "cognitive", 30.0, 15.0);
        err.severity = Severity::Error;
        err.start_col = None;
        err.function = None;
        let offenders = vec![
            rec("src/alpha.rs", "cyclomatic", 17.0, 15.0),
            rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
            err,
        ];
        insta::assert_snapshot!("sarif_multi", render(&offenders));
    }
}
