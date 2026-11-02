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

use crate::metric_catalog::lookup;
#[cfg(test)]
use crate::output::offenders::Severity;
use crate::output::offenders::{OffenderRecord, TOOL_ID, warn_non_utf8_path};

/// SARIF schema URL — pinned to 2.1.0 (the version GitHub Code
/// Scanning ingests).
const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const SARIF_VERSION: &str = "2.1.0";

/// A `C:`-style Windows drive prefix that must be emitted as `file:///C:`
/// so the leading drive letter is not parsed as a URI scheme.
fn is_windows_drive_abs(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\')
}

/// A relative reference (no leading separator, not a Windows drive) whose
/// first path segment carries a colon is scheme-ambiguous under RFC 3986
/// §4.2; the `./` prefix in [`path_to_uri_reference`] neutralizes it
/// (#798). The first segment ends at the first `/` or `\` separator.
fn relative_first_segment_has_colon(bytes: &[u8]) -> bool {
    !is_windows_drive_abs(bytes)
        && bytes.first() != Some(&b'/')
        && bytes.first() != Some(&b'\\')
        && bytes
            .iter()
            .take_while(|&&b| b != b'/' && b != b'\\')
            .any(|&b| b == b':')
}

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
/// - For a relative path whose first segment contains a colon
///   (`a:b/c.rs`), prefix with `./` so the colon no longer sits in
///   the first segment: RFC 3986 §4.2 reads a bare `a:b/c.rs` as
///   scheme `a:`, but `./a:b/c.rs` is an unambiguous relative-ref
///   (`./` is a complete colon-free first segment). A colon *after*
///   the first `/` is already RFC-legal and passes through untouched.
fn path_to_uri_reference(path: &str) -> String {
    let bytes = path.as_bytes();
    let is_windows_drive_abs = is_windows_drive_abs(bytes);
    let first_segment_has_colon = relative_first_segment_has_colon(bytes);

    let mut out = String::with_capacity(
        path.len()
            + if is_windows_drive_abs { 8 } else { 0 }
            + usize::from(first_segment_has_colon) * 2,
    );
    if is_windows_drive_abs {
        out.push_str("file:///");
    } else if first_segment_has_colon {
        out.push_str("./");
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
pub fn write_sarif<W: Write>(offenders: &[OffenderRecord], writer: W) -> io::Result<()> {
    write_sarif_with_suppressed(offenders, &[], &[], writer)
}

/// Write a SARIF 2.1.0 document that also surfaces *suppressed* offenders.
///
/// `active` offenders are emitted as ordinary results (open findings).
/// `in_source` and `baseline` offenders are emitted with a SARIF
/// `suppressions` entry (`kind: "inSource"` and `kind: "external"`
/// respectively) so consumers such as GitHub Code Scanning render them as
/// suppressed/closed alerts rather than active findings — the debt stays
/// visible without counting against the open-alert total. This backs
/// `bca check --report-suppressed`, which keeps suppression-marker and
/// baseline-covered offenders out of the gate yet records them in the
/// code-scan report.
///
/// `write_sarif` is the `active`-only special case.
///
/// # Errors
///
/// Same contract as [`write_sarif`]: any [`io::Error`] from `writer`, or a
/// serialisation failure mapped to `io::Error`.
pub fn write_sarif_with_suppressed<W: Write>(
    active: &[OffenderRecord],
    in_source: &[OffenderRecord],
    baseline: &[OffenderRecord],
    writer: W,
) -> io::Result<()> {
    write_sarif_with_provenance(
        active,
        in_source,
        baseline,
        &SarifProvenance::default(),
        writer,
    )
}

/// Build provenance a caller can stamp into the SARIF `tool.driver`
/// block, under `properties` (the SARIF 2.1.0 extension point —
/// `toolComponent` admits no other unknown key).
///
/// The library itself knows only its own package version. A binary that
/// embeds it, such as `bca`, can record what *it* was built from so a
/// detached report traces to a build: `bca` sets `build_inputs` to the
/// content digest of everything compiled into it and `build_git` to the
/// checkout revision (#1773). `Default` carries nothing, and a default
/// provenance leaves the document byte-identical to [`write_sarif`]'s.
///
/// The shape is `MetricsOptions`'s: `#[non_exhaustive]`, private fields,
/// `Default` plus `with_*` setters, so a later field is additive. These
/// are facts about the *tool's* build, not about the scanned repository
/// — which is why they sit here and not in SARIF's
/// `run.versionControlProvenance`, even though in a self-scan the two
/// revisions coincide. Run-level metadata a host wants to add later
/// (invocation, timestamps) belongs on this struct, never as a further
/// writer parameter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SarifProvenance {
    build_inputs: Option<String>,
    build_git: Option<String>,
}

impl SarifProvenance {
    /// Record a content digest of the inputs compiled into the reporting
    /// binary; rendered as the `bca.buildInputs` property.
    #[must_use]
    pub fn with_build_inputs(mut self, digest: impl Into<String>) -> Self {
        self.build_inputs = Some(digest.into());
        self
    }

    /// Record the revision the reporting binary was built from; rendered
    /// as the `bca.buildGit` property.
    #[must_use]
    pub fn with_build_git(mut self, revision: impl Into<String>) -> Self {
        self.build_git = Some(revision.into());
        self
    }

    fn is_empty(&self) -> bool {
        self.build_inputs.is_none() && self.build_git.is_none()
    }
}

/// [`write_sarif_with_suppressed`] with a [`SarifProvenance`] stamped
/// into `tool.driver.properties`. An empty provenance emits no
/// `properties` key at all, so the two older writers forward here with
/// their output unchanged.
///
/// # Errors
///
/// Same contract as [`write_sarif`]: any [`io::Error`] from `writer`, or a
/// serialisation failure mapped to `io::Error`.
pub fn write_sarif_with_provenance<W: Write>(
    active: &[OffenderRecord],
    in_source: &[OffenderRecord],
    baseline: &[OffenderRecord],
    provenance: &SarifProvenance,
    mut writer: W,
) -> io::Result<()> {
    let mut results: Vec<SarifResult<'_>> =
        Vec::with_capacity(active.len() + in_source.len() + baseline.len());
    // BTreeSet so the rules array is deterministic (alphabetical by id).
    let mut rule_ids: BTreeSet<&str> = BTreeSet::new();

    for (offenders, origin) in [
        (active, None),
        (in_source, Some(SuppressionOrigin::InSource)),
        (baseline, Some(SuppressionOrigin::Baseline)),
    ] {
        collect_results(offenders, origin, &mut results, &mut rule_ids);
    }

    let rules: Vec<Rule<'_>> = rule_ids
        .iter()
        .map(|id| Rule {
            id,
            short_description: Description {
                text: lookup(id).map_or(*id, |info| info.long_description),
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
                    properties: (!provenance.is_empty()).then_some(DriverProperties {
                        build_inputs: provenance.build_inputs.as_deref(),
                        build_git: provenance.build_git.as_deref(),
                    }),
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

/// Append one [`SarifResult`] per offender in `offenders`, tagging each
/// with `origin` (when `Some`) via the SARIF `suppressions` property.
/// Non-UTF-8 paths are skipped with a warning (SARIF `uri` must be UTF-8).
fn collect_results<'a>(
    offenders: &'a [OffenderRecord],
    origin: Option<SuppressionOrigin>,
    results: &mut Vec<SarifResult<'a>>,
    rule_ids: &mut BTreeSet<&'a str>,
) {
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
                        // SARIF §3.30.6 requires `startColumn >= 1`;
                        // clamp symmetrically with `startLine` so a
                        // future 0-based-column producer can never emit
                        // `startColumn: 0` (#698). `start_col` is `None`
                        // in production today, so this is latent.
                        start_column: record.start_col.map(|c| c.max(1)),
                    },
                },
                logical_locations,
            }],
            suppressions: origin.map(|o| {
                vec![Suppression {
                    kind: o.sarif_kind(),
                    justification: o.justification(&record.metric),
                }]
            }),
        });
    }
}

/// Why a surfaced offender is exempt from the `bca check` gate, mapped to a
/// SARIF `suppressions[].kind`. Emitted by [`write_sarif_with_suppressed`]
/// so suppressed debt appears as a *closed* code-scan alert.
#[derive(Debug, Clone, Copy)]
enum SuppressionOrigin {
    /// Silenced by an in-source `bca: suppress` / `suppress-file` marker.
    InSource,
    /// Within a recorded `.bca-baseline.toml` entry.
    Baseline,
}

impl SuppressionOrigin {
    /// SARIF 2.1.0 `suppressions[].kind`: `inSource` for source markers,
    /// `external` for the out-of-band baseline file.
    fn sarif_kind(self) -> &'static str {
        match self {
            Self::InSource => "inSource",
            Self::Baseline => "external",
        }
    }

    fn justification(self, metric: &str) -> String {
        match self {
            Self::InSource => {
                format!("metric '{metric}' silenced by an in-source bca suppression marker")
            }
            Self::Baseline => format!("metric '{metric}' within the recorded bca baseline"),
        }
    }
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
    /// Present only when a [`SarifProvenance`] carried something, so the
    /// provenance-free writers' output never gains a key.
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<DriverProperties<'a>>,
}

/// The `tool.driver.properties` bag. Keys are namespaced with `bca.` as
/// SARIF recommends for producer-specific properties.
#[derive(Serialize)]
struct DriverProperties<'a> {
    #[serde(rename = "bca.buildInputs", skip_serializing_if = "Option::is_none")]
    build_inputs: Option<&'a str>,
    #[serde(rename = "bca.buildGit", skip_serializing_if = "Option::is_none")]
    build_git: Option<&'a str>,
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
    /// SARIF `suppressions`: present (non-empty) marks the result as a
    /// suppressed/closed alert. Elided for active findings so existing
    /// `write_sarif` output is byte-for-byte unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    suppressions: Option<Vec<Suppression>>,
}

/// One SARIF 2.1.0 `suppressions` entry. A non-empty `suppressions` array
/// tells consumers the result is suppressed; `kind` distinguishes in-source
/// markers (`inSource`) from the external baseline file (`external`).
#[derive(Serialize)]
struct Suppression {
    kind: &'static str,
    justification: String,
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
        assert_eq!(v["runs"][0]["results"], serde_json::json!([]));
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"],
            serde_json::json!([])
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
    fn provenance_lands_under_driver_properties_and_an_empty_one_adds_no_key() {
        let offenders = vec![rec("src/foo.rs", "cyclomatic", 17.0, 15.0)];
        let stamped = SarifProvenance::default()
            .with_build_inputs("ab".repeat(32))
            .with_build_git("deadbeef-dirty");
        let mut buf = Vec::new();
        write_sarif_with_provenance(&offenders, &[], &[], &stamped, &mut buf)
            .expect("writing to Vec is infallible");
        let v: serde_json::Value = serde_json::from_slice(&buf).expect("valid JSON");
        let props = &v["runs"][0]["tool"]["driver"]["properties"];
        assert_eq!(props["bca.buildInputs"], "ab".repeat(32));
        assert_eq!(props["bca.buildGit"], "deadbeef-dirty");
        // `toolComponent` admits no unknown direct keys, so the digest
        // must live in the bag and nowhere else.
        assert!(
            v["runs"][0]["tool"]["driver"]
                .get("bca.buildInputs")
                .is_none()
        );

        // Every older writer forwards with an empty provenance, so its
        // document gains no `properties` key — byte-identical output.
        let mut plain = Vec::new();
        write_sarif_with_provenance(
            &offenders,
            &[],
            &[],
            &SarifProvenance::default(),
            &mut plain,
        )
        .expect("writing to Vec is infallible");
        assert_eq!(plain, render(&offenders).into_bytes());
        assert!(
            !String::from_utf8(plain)
                .expect("utf-8")
                .contains("properties")
        );

        // Half-populated: only the present field is written.
        let mut half = Vec::new();
        let git_only = SarifProvenance::default().with_build_git("deadbeef");
        write_sarif_with_provenance(&offenders, &[], &[], &git_only, &mut half)
            .expect("writing to Vec is infallible");
        let v: serde_json::Value = serde_json::from_slice(&half).expect("valid JSON");
        let props = &v["runs"][0]["tool"]["driver"]["properties"];
        assert_eq!(props["bca.buildGit"], "deadbeef");
        assert!(props.get("bca.buildInputs").is_none());
    }

    #[test]
    fn write_sarif_omits_suppressions_for_active_results() {
        // The active-only `write_sarif` path must never emit a
        // `suppressions` key, so existing Code Scanning output (and
        // snapshot consumers) stay byte-for-byte unchanged.
        let out = render(&[rec("src/foo.rs", "cyclomatic", 17.0, 15.0)]);
        assert!(
            !out.contains("suppressions"),
            "active result must not carry a suppressions array:\n{out}"
        );
    }

    #[test]
    fn suppressed_offenders_carry_suppressions_with_kind() {
        let active = vec![rec("src/active.rs", "cyclomatic", 17.0, 15.0)];
        let in_source = vec![rec("src/marked.rs", "halstead.effort", 9e4, 5e4)];
        let baseline = vec![rec("src/legacy.rs", "cognitive", 26.0, 25.0)];

        let mut buf = Vec::new();
        write_sarif_with_suppressed(&active, &in_source, &baseline, &mut buf)
            .expect("writing to Vec is infallible");
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8(buf).expect("utf8")).expect("valid JSON");
        let results = v["runs"][0]["results"].as_array().expect("array");
        assert_eq!(results.len(), 3);

        // Active result: no suppressions key (open finding).
        let active_r = results
            .iter()
            .find(|r| {
                r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == "src/active.rs"
            })
            .expect("active result present");
        assert!(active_r.get("suppressions").is_none());

        // In-source marker → kind "inSource".
        let marked = results
            .iter()
            .find(|r| {
                r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == "src/marked.rs"
            })
            .expect("in-source result present");
        assert_eq!(marked["suppressions"][0]["kind"], "inSource");
        assert!(
            marked["suppressions"][0]["justification"]
                .as_str()
                .expect("justification string")
                .contains("in-source")
        );

        // Baseline-covered → kind "external".
        let legacy = results
            .iter()
            .find(|r| {
                r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == "src/legacy.rs"
            })
            .expect("baseline result present");
        assert_eq!(legacy["suppressions"][0]["kind"], "external");
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
    fn start_column_zero_is_clamped_to_one() {
        // SARIF §3.30.6 requires `startColumn >= 1`. A 0-based-column
        // producer feeding `start_col = Some(0)` must be clamped to 1,
        // symmetric with the already-clamped `startLine` (#698).
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_col = Some(0);
        let out = render(&[r]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(
            region["startColumn"], 1,
            "startColumn must clamp 0 -> 1, got: {region}"
        );
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
    fn relative_path_with_colon_in_first_segment_is_not_scheme_ambiguous() {
        // RFC 3986 §4.2: a bare `a:b/c.rs` parses as scheme `a:`. The
        // `./` prefix makes it an unambiguous relative-ref (#798).
        assert_eq!(path_to_uri_reference("a:b/c.rs"), "./a:b/c.rs");
        assert_eq!(path_to_uri_reference("foo:bar/baz.rs"), "./foo:bar/baz.rs");
    }

    #[test]
    fn relative_path_with_colon_after_first_slash_is_unchanged() {
        // A colon *after* the first segment is RFC-3986-legal in a
        // relative-ref (only the first segment may not contain one), so
        // it must pass through without a `./` prefix (#798).
        assert_eq!(path_to_uri_reference("a/b:c.rs"), "a/b:c.rs");
    }

    #[test]
    fn normal_relative_path_keeps_no_dot_slash_prefix() {
        // No colon in the first segment → untouched (#798 guard).
        assert_eq!(path_to_uri_reference("a/b/c.rs"), "a/b/c.rs");
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
