//! GitLab Code Climate JSON writer for [`OffenderRecord`] batches.
//!
//! GitLab's merge-request *Code Quality* widget consumes a strict
//! subset of the upstream [Code Climate engine
//! spec](https://github.com/codeclimate/platform/blob/master/spec/analyzers/SPEC.md);
//! this writer emits exactly that subset so a `bca check` artifact
//! can drop straight into `.gitlab-ci.yml`'s
//! `artifacts.reports.codequality:` slot. See the authoritative
//! GitLab docs at
//! <https://docs.gitlab.com/ci/testing/code_quality/> for the
//! consumer side.
//!
//! # Fields emitted
//!
//! | JSON field | Source |
//! |------------|--------|
//! | `description` | [`metric_catalog`](crate::metric_catalog) long-form + [`OffenderRecord::default_message`]; bare `default_message` for unknown metrics |
//! | `check_name` | `"big-code-analysis/<metric>"` (namespaced so multi-tool pipelines do not collide) |
//! | `fingerprint` | SHA-256 of `path \0 function.unwrap_or("") \0 metric`, truncated to 32 hex chars. Deliberately excludes line / value so re-runs after upstream-line edits still dedup in the MR widget. |
//! | `severity` | Ratio-band mapping over `value / limit` (inverted for the `mi.*` family — lower is worse there). Falls back to a per-record `Severity` lookup when the ratio is ill-defined. |
//! | `location.path` | UTF-8 relative path, forward slashes, leading `./` stripped. Non-UTF-8 paths emit a stderr warning and the offender is skipped. |
//! | `location.lines.begin`, `lines.end` | `start_line` (clamped ≥ 1) and `end_line` (only when `> start_line`). |
//! | `location.positions.begin` | `{line, column}` emitted only when `start_col` is `Some(c)` with `c > 0`. |
//!
//! # Not emitted
//!
//! The upstream Code Climate spec defines `type`, `categories`,
//! `remediation_points`, and `content`; GitLab ignores all of
//! them, so we omit them to keep the artifact small. Adding them
//! later is a purely additive change.
//!
//! # Framing
//!
//! Single JSON array of objects, no byte-order-mark, one trailing
//! newline. The empty case emits the literal `[]\n` so consumers
//! that pipe through `jq` see a well-formed document even when no
//! offenders triggered.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::{self, Write};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::metric_catalog::{Direction, lookup};
use crate::output::numfmt::MessageMetric;
use crate::output::offenders::{OffenderRecord, Severity, TOOL_ID, warn_non_utf8_path};

/// Number of leading SHA-256 bytes retained in each fingerprint
/// (matches the issue spec — 128 bits is enough to keep collision
/// probability negligible for any realistic offender corpus while
/// keeping the JSON artifact compact). The hex-encoded width is
/// `FINGERPRINT_BYTE_LEN * 2` chars (32) by construction.
const FINGERPRINT_BYTE_LEN: usize = 16;

/// Write a GitLab Code Climate JSON report for `offenders` to
/// `writer`.
///
/// Offenders whose path is not valid UTF-8 — or whose
/// repo-relative path collapses to the empty string after
/// normalization — are skipped with a warning to stderr. The
/// empty case emits the literal `[]\n` so the artifact is always
/// well-formed, even before the threshold engine produces any
/// violations.
///
/// # Errors
///
/// Returns any [`io::Error`] produced by `writer` while emitting
/// the JSON document, or a `serde_json::Error` (mapped to
/// `io::Error` via [`io::Error::new`]) if a record cannot be
/// serialised.
pub fn write_code_climate<W: Write>(offenders: &[OffenderRecord], mut writer: W) -> io::Result<()> {
    if offenders.is_empty() {
        return writer.write_all(b"[]\n");
    }
    let mut issues: Vec<CodeClimateIssue> = Vec::with_capacity(offenders.len());
    for record in offenders {
        let Some(path_raw) = warn_non_utf8_path("code-climate", &record.path) else {
            continue;
        };
        let Some(path) = normalize_path(path_raw) else {
            eprintln!(
                "Warning: skipping empty repo-relative path in code-climate output: {}",
                record.path.display()
            );
            continue;
        };
        let start_line = record.start_line.max(1);
        let lines_end = (record.end_line > start_line).then_some(record.end_line);
        let positions = record.start_col.filter(|c| *c > 0).map(|column| Positions {
            begin: Position {
                line: start_line,
                column,
            },
        });
        issues.push(CodeClimateIssue {
            description: build_description(record),
            check_name: format!("{TOOL_ID}/{}", record.metric),
            fingerprint: fingerprint(&path, record.function.as_deref(), &record.metric),
            severity: severity_band(&record.metric, record.value, record.limit, record.severity),
            location: Location {
                path,
                lines: Lines {
                    begin: start_line,
                    end: lines_end,
                },
                positions,
            },
        });
    }
    serde_json::to_writer(&mut writer, &issues)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writer.write_all(b"\n")
}

#[derive(Serialize)]
struct CodeClimateIssue {
    description: String,
    check_name: String,
    fingerprint: String,
    severity: &'static str,
    location: Location,
}

#[derive(Serialize)]
struct Location {
    path: String,
    lines: Lines,
    #[serde(skip_serializing_if = "Option::is_none")]
    positions: Option<Positions>,
}

#[derive(Serialize)]
struct Lines {
    begin: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
}

#[derive(Serialize)]
struct Positions {
    begin: Position,
}

#[derive(Serialize)]
struct Position {
    line: u32,
    column: u32,
}

/// Map a metric value/limit ratio onto GitLab's five-level severity
/// enum.
///
/// GitLab accepts `info`, `minor`, `major`, `critical`, `blocker`.
/// We never emit `info`; the lowest band starts at `minor` so a
/// threshold violation always shows in the MR widget (per the
/// issue spec). The `mi.*` family inverts the ratio direction —
/// for Maintainability Index, lower values mean *worse*, so the
/// "how many times the threshold did we breach by" is `limit /
/// value`, not `value / limit`.
fn severity_band(metric: &str, value: f64, limit: f64, severity: Severity) -> &'static str {
    let fallback = || match severity {
        Severity::Warning => "minor",
        Severity::Error => "major",
    };
    // Filter ill-defined inputs BEFORE choosing the ratio direction:
    // a future refactor that moves the metric-family check above this
    // guard would let `mi.*` reach the inverted ratio with `value <=
    // 0.0` and divide-by-zero. Keep the guards here.
    if !value.is_finite() || !limit.is_finite() || limit <= 0.0 || value <= 0.0 {
        return fallback();
    }
    let lower_is_worse =
        lookup(metric).is_some_and(|i| matches!(i.direction, Direction::LowerIsWorse));
    let ratio = if lower_is_worse {
        limit / value
    } else {
        value / limit
    };
    if ratio <= 1.5 {
        "minor"
    } else if ratio <= 2.0 {
        "major"
    } else if ratio <= 4.0 {
        "critical"
    } else {
        "blocker"
    }
}

/// Compute a stable per-violation fingerprint. GitLab's MR widget
/// uses this for deduplication across pipeline runs and for the
/// base-vs-head diff, so we deliberately omit the line number and
/// metric value — both shift on cosmetic edits that should not
/// re-surface a known violation.
///
/// Truncated to [`FINGERPRINT_BYTE_LEN`] bytes per the issue spec
/// (32 hex chars by construction). Leading-zero bytes are preserved
/// by [`hex_lower_bytes`] (which a `format!("{:x}", u128)` rendering
/// would silently drop).
fn fingerprint(path: &str, function: Option<&str>, metric: &str) -> String {
    let mut h = Sha256::new();
    h.update(path.as_bytes());
    h.update(b"\0");
    h.update(function.unwrap_or("").as_bytes());
    h.update(b"\0");
    h.update(metric.as_bytes());
    let digest = h.finalize();
    hex_lower_bytes(&digest[..FINGERPRINT_BYTE_LEN])
}

/// Lowercase, zero-padded hex encoding of `bytes`. Extracted from
/// [`fingerprint`] so the zero-padding invariant can be tested with
/// synthetic byte sequences (including bytes < `0x10`) that the
/// SHA-256 driver of `fingerprint` does not surface deterministically.
fn hex_lower_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // write! to a String is infallible; the Result is discarded
        // intentionally rather than unwrapped to avoid an `expect`
        // in non-test code.
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn normalize_path(raw: &str) -> Option<String> {
    // Avoid a transient String allocation on the no-backslash path
    // (the typical Linux-CI case). The `replace` allocation only
    // pays for itself when there's actually a backslash to swap.
    let normalized: Cow<'_, str> = if raw.contains('\\') {
        Cow::Owned(raw.replace('\\', "/"))
    } else {
        Cow::Borrowed(raw)
    };
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_owned())
    }
}

fn build_description(record: &OffenderRecord) -> String {
    let Some(long_form) = lookup(&record.metric).map(|i| i.long_description) else {
        return record.default_message();
    };
    // Single-allocation render: write the long-form prefix and the
    // default-message components directly into one String instead of
    // round-tripping through `record.default_message()` + `format!`.
    let mut out = String::with_capacity(long_form.len() + record.metric.len() + 32);
    out.push_str(long_form);
    out.push(' ');
    // SAFETY: writing to a String is infallible.
    let _ = write!(
        &mut out,
        "{} {} exceeds limit {}",
        record.metric,
        MessageMetric(record.value),
        MessageMetric(record.limit),
    );
    out
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
        write_code_climate(offenders, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    fn render_value(offenders: &[OffenderRecord]) -> serde_json::Value {
        serde_json::from_str(&render(offenders)).expect("valid JSON")
    }

    #[test]
    fn empty_input_emits_bracket_newline() {
        assert_eq!(render(&[]), "[]\n");
    }

    #[test]
    fn single_offender_anchored_snapshot() {
        let mut r = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        r.start_col = None;
        let v = render_value(&[r]);
        insta::assert_json_snapshot!(v, @r#"
        [
          {
            "check_name": "big-code-analysis/cyclomatic",
            "description": "Cyclomatic Complexity exceeds the configured threshold. cyclomatic 17 exceeds limit 15",
            "fingerprint": "209c41c7caa70e296f0bb82946cce7cc",
            "location": {
              "lines": {
                "begin": 42,
                "end": 50
              },
              "path": "src/foo.rs"
            },
            "severity": "minor"
          }
        ]
        "#);
    }

    #[test]
    fn multi_offender_with_column_and_file_level() {
        let with_col = rec("src/a.rs", "cyclomatic", 30.0, 15.0);
        let mut file_level = rec("src/b.rs", "loc.lloc", 250.0, 100.0);
        file_level.function = None;
        file_level.start_col = None;
        let v = render_value(&[with_col, file_level]);
        insta::assert_json_snapshot!(v, @r#"
        [
          {
            "check_name": "big-code-analysis/cyclomatic",
            "description": "Cyclomatic Complexity exceeds the configured threshold. cyclomatic 30 exceeds limit 15",
            "fingerprint": "03dd26a883d163bd752853e1dd15557d",
            "location": {
              "lines": {
                "begin": 42,
                "end": 50
              },
              "path": "src/a.rs",
              "positions": {
                "begin": {
                  "column": 5,
                  "line": 42
                }
              }
            },
            "severity": "major"
          },
          {
            "check_name": "big-code-analysis/loc.lloc",
            "description": "Logical lines of code exceed the configured threshold. loc.lloc 250 exceeds limit 100",
            "fingerprint": "cc3f570c9b909e186681cf36a6cffe5c",
            "location": {
              "lines": {
                "begin": 42,
                "end": 50
              },
              "path": "src/b.rs"
            },
            "severity": "critical"
          }
        ]
        "#);
    }

    #[test]
    fn severity_band_table_upward_metric() {
        // limit=10, value at 1.0x/1.25x/1.75x/3.0x/10.0x of limit.
        assert_eq!(
            severity_band("cyclomatic", 10.0, 10.0, Severity::Warning),
            "minor"
        );
        assert_eq!(
            severity_band("cyclomatic", 12.5, 10.0, Severity::Warning),
            "minor"
        );
        assert_eq!(
            severity_band("cyclomatic", 17.5, 10.0, Severity::Warning),
            "major"
        );
        assert_eq!(
            severity_band("cyclomatic", 30.0, 10.0, Severity::Warning),
            "critical"
        );
        assert_eq!(
            severity_band("cyclomatic", 100.0, 10.0, Severity::Warning),
            "blocker"
        );
    }

    #[test]
    fn severity_band_table_mi_family_inverts() {
        // `mi.original` is the real offender id (the threshold-engine
        // EXTRACTOR key). `severity_band` only ever sees offender ids,
        // and inversion is now driven by `metric_catalog`'s per-row
        // `Direction` rather than a `starts_with("mi.")` prefix — so the
        // id must match the catalog exactly, not just the `mi.` prefix.
        // limit=100 (MI threshold), lower value = worse violation.
        assert_eq!(
            severity_band("mi.original", 100.0, 100.0, Severity::Warning),
            "minor"
        );
        // 100/50 = 2.0 → major.
        assert_eq!(
            severity_band("mi.original", 50.0, 100.0, Severity::Warning),
            "major"
        );
        // 100/40 = 2.5 → critical.
        assert_eq!(
            severity_band("mi.original", 40.0, 100.0, Severity::Warning),
            "critical"
        );
        // 100/10 = 10.0 → blocker.
        assert_eq!(
            severity_band("mi.original", 10.0, 100.0, Severity::Warning),
            "blocker"
        );
    }

    #[test]
    fn severity_band_falls_back_when_limit_zero() {
        assert_eq!(
            severity_band("cyclomatic", 5.0, 0.0, Severity::Warning),
            "minor"
        );
        assert_eq!(
            severity_band("cyclomatic", 5.0, 0.0, Severity::Error),
            "major"
        );
    }

    #[test]
    fn severity_band_falls_back_when_value_nan() {
        assert_eq!(
            severity_band("cyclomatic", f64::NAN, 10.0, Severity::Warning),
            "minor"
        );
        assert_eq!(
            severity_band("cyclomatic", f64::NAN, 10.0, Severity::Error),
            "major"
        );
    }

    #[test]
    fn severity_band_falls_back_when_value_inf() {
        assert_eq!(
            severity_band("cyclomatic", f64::INFINITY, 10.0, Severity::Warning),
            "minor"
        );
        assert_eq!(
            severity_band("cyclomatic", f64::INFINITY, 10.0, Severity::Error),
            "major"
        );
    }

    #[test]
    fn fingerprint_is_line_value_insensitive() {
        let mut a = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        let mut b = rec("src/foo.rs", "cyclomatic", 99.0, 15.0);
        a.start_line = 10;
        b.start_line = 20;
        let va = render_value(&[a]);
        let vb = render_value(&[b]);
        assert_eq!(va[0]["fingerprint"], vb[0]["fingerprint"]);
    }

    #[test]
    fn fingerprint_changes_with_metric() {
        let a = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        let b = rec("src/foo.rs", "cognitive", 17.0, 15.0);
        let va = render_value(&[a]);
        let vb = render_value(&[b]);
        assert_ne!(va[0]["fingerprint"], vb[0]["fingerprint"]);
    }

    #[test]
    fn fingerprint_changes_with_function() {
        let mut a = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        let mut b = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        a.function = Some("foo".into());
        b.function = Some("bar".into());
        let va = render_value(&[a]);
        let vb = render_value(&[b]);
        assert_ne!(va[0]["fingerprint"], vb[0]["fingerprint"]);
    }

    #[test]
    fn fingerprint_changes_with_path() {
        let a = rec("src/foo.rs", "cyclomatic", 17.0, 15.0);
        let b = rec("src/bar.rs", "cyclomatic", 17.0, 15.0);
        let va = render_value(&[a]);
        let vb = render_value(&[b]);
        assert_ne!(va[0]["fingerprint"], vb[0]["fingerprint"]);
    }

    #[test]
    fn fingerprint_handles_none_function() {
        let none_fp = fingerprint("a.rs", None, "cyclomatic");
        let empty_fp = fingerprint("a.rs", Some(""), "cyclomatic");
        assert_eq!(none_fp, empty_fp);
    }

    #[test]
    fn hex_lower_bytes_pads_low_bytes_to_two_chars() {
        // Deterministic, input-controlled check of the zero-padding
        // invariant. A `{:02x}` → `{:x}` regression would render
        // `0x00` as `"0"` (not `"00"`), shortening the output and
        // failing the explicit string comparison below. The
        // fingerprint pipeline cannot directly surface a digest with
        // leading zero bytes, so this is the load-bearing test for
        // the format spec used by `fingerprint`.
        assert_eq!(
            hex_lower_bytes(&[0x00, 0x01, 0x0f, 0x10, 0xab, 0xff]),
            "00010f10abff",
        );
        // Empty input → empty output (preserves the `len * 2` invariant).
        assert_eq!(hex_lower_bytes(&[]), "");
        // Single zero byte.
        assert_eq!(hex_lower_bytes(&[0x00]), "00");
    }

    #[test]
    fn fingerprint_uses_full_truncation_width() {
        // Lock the constant against drift. If `FINGERPRINT_BYTE_LEN`
        // changes, the truncation width in fingerprints changes too;
        // we want a loud failure rather than a silent shift.
        let fp = fingerprint("a.rs", Some("fn"), "cyclomatic");
        assert_eq!(fp.len(), FINGERPRINT_BYTE_LEN * 2);
        assert!(
            fp.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let a = fingerprint("src/x.rs", Some("f"), "cyclomatic");
        let b = fingerprint("src/x.rs", Some("f"), "cyclomatic");
        assert_eq!(a, b);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_is_skipped() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let bad = OffenderRecord {
            path: PathBuf::from(OsString::from_vec(b"weird-\xff\xfe.rs".to_vec())),
            function: Some("f".into()),
            start_line: 1,
            end_line: 1,
            start_col: None,
            metric: "cyclomatic".into(),
            value: 17.0,
            limit: 15.0,
            severity: Severity::Warning,
        };
        let good = rec("src/ok.rs", "cyclomatic", 17.0, 15.0);
        let v = render_value(&[bad, good]);
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1, "bad-path record skipped");
        assert_eq!(arr[0]["location"]["path"], "src/ok.rs");
    }

    #[test]
    fn windows_backslash_path_is_normalized() {
        assert_eq!(
            normalize_path(r"src\foo\bar.rs"),
            Some("src/foo/bar.rs".to_owned())
        );
    }

    #[test]
    fn dot_slash_prefix_is_stripped() {
        assert_eq!(
            normalize_path("./src/foo.rs"),
            Some("src/foo.rs".to_owned())
        );
        // Only one strip — leftover `./` is preserved.
        assert_eq!(
            normalize_path("././src/foo.rs"),
            Some("./src/foo.rs".to_owned())
        );
    }

    #[test]
    fn path_normalising_to_empty_is_skipped() {
        assert_eq!(normalize_path("./"), None);
        assert_eq!(normalize_path(""), None);
    }

    #[test]
    fn start_line_zero_is_clamped_to_one() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_line = 0;
        r.end_line = 0;
        let v = render_value(&[r]);
        assert_eq!(v[0]["location"]["lines"]["begin"], 1);
        assert!(v[0]["location"]["lines"].get("end").is_none());
    }

    #[test]
    fn end_line_less_than_or_equal_start_omits_end() {
        let mut equal = rec("a.rs", "cyclomatic", 17.0, 15.0);
        equal.start_line = 10;
        equal.end_line = 10;
        let v_equal = render_value(&[equal]);
        assert!(v_equal[0]["location"]["lines"].get("end").is_none());

        let mut less = rec("a.rs", "cyclomatic", 17.0, 15.0);
        less.start_line = 10;
        less.end_line = 5;
        let v_less = render_value(&[less]);
        assert!(v_less[0]["location"]["lines"].get("end").is_none());
    }

    #[test]
    fn start_col_zero_omits_positions() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_col = Some(0);
        let v = render_value(&[r]);
        assert!(v[0]["location"].get("positions").is_none());
    }

    #[test]
    fn description_includes_long_form_when_metric_known() {
        let known = rec("a.rs", "cyclomatic", 17.0, 15.0);
        let v = render_value(&[known]);
        let desc = v[0]["description"].as_str().expect("string");
        assert!(
            desc.starts_with("Cyclomatic Complexity exceeds the configured threshold."),
            "expected long-form prefix, got: {desc}"
        );
        assert!(
            desc.ends_with("cyclomatic 17 exceeds limit 15"),
            "expected default_message tail, got: {desc}"
        );

        let unknown = rec("a.rs", "made.up.metric", 1.0, 0.0);
        let v = render_value(&[unknown]);
        assert_eq!(v[0]["description"], "made.up.metric 1 exceeds limit 0");
    }

    #[test]
    fn check_name_is_tool_namespaced() {
        let r = rec("a.rs", "halstead.effort", 5000.0, 1000.0);
        let v = render_value(&[r]);
        assert_eq!(v[0]["check_name"], "big-code-analysis/halstead.effort");
    }

    #[test]
    fn output_has_no_bom() {
        let r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        let mut buf = Vec::new();
        write_code_climate(&[r], &mut buf).expect("writing to Vec is infallible");
        // GitLab's parser rejects a UTF-8 BOM (EF BB BF) at the start
        // of the artifact, so check the full three-byte prefix rather
        // than just the first byte — the latter would still admit a
        // future regression that emits only the leading `EF`.
        assert!(
            !buf.starts_with(&[0xEF, 0xBB, 0xBF]),
            "code-climate output must not start with a UTF-8 BOM"
        );
        assert_eq!(buf[0], b'[', "first byte must be the opening bracket");
    }
}
