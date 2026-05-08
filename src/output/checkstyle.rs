//! Checkstyle 4.3 XML writer for [`OffenderRecord`] batches.
//!
//! Checkstyle is the de-facto interchange format for Jenkins, SonarQube,
//! GitLab, and most "warnings plugin" CI integrations. We emit a single
//! XML document covering every offender, grouped by source path:
//!
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <checkstyle version="4.3">
//!   <file name="src/foo.rs">
//!     <error line="42" column="5" severity="warning"
//!            message="cyclomatic 17 exceeds limit 15"
//!            source="big-code-analysis.cyclomatic"/>
//!   </file>
//! </checkstyle>
//! ```
//!
//! XML escaping is hand-rolled because the surface is tiny (five
//! entities in attribute values) and adding a new dependency is not
//! worth it for that.

use std::collections::BTreeMap;
use std::io::{self, Write};

use crate::output::offenders::{OffenderRecord, TOOL_ID, warn_non_utf8_path};

/// Write Checkstyle 4.3 XML for `offenders` to `writer`.
///
/// Offenders are grouped by `path` (sorted lexicographically by the
/// UTF-8 representation; non-UTF-8 paths are skipped with a warning to
/// stderr) so the output is deterministic and snapshot-friendly. Within
/// a file, errors retain their input order.
///
/// The empty case still emits a well-formed `<checkstyle version="4.3"/>`
/// document so consumers can rely on a non-empty file always being
/// parseable.
pub fn write_checkstyle<W: Write>(offenders: &[OffenderRecord], mut writer: W) -> io::Result<()> {
    writer.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;

    // Group while preserving per-file insertion order. BTreeMap key is
    // the UTF-8 path; this also gives us deterministic file ordering.
    let mut by_file: BTreeMap<&str, Vec<&OffenderRecord>> = BTreeMap::new();
    for record in offenders {
        let Some(path_str) = warn_non_utf8_path("Checkstyle", &record.path) else {
            continue;
        };
        by_file.entry(path_str).or_default().push(record);
    }

    // Empty input *and* all-non-UTF-8 input both end up here with an
    // empty `by_file`, so one branch covers both cases.
    if by_file.is_empty() {
        writer.write_all(b"<checkstyle version=\"4.3\"/>\n")?;
        return Ok(());
    }

    writer.write_all(b"<checkstyle version=\"4.3\">\n")?;
    for (path_str, records) in by_file {
        writeln!(writer, "  <file name=\"{}\">", XmlAttr(path_str))?;
        for record in records {
            write_error(&mut writer, record)?;
        }
        writer.write_all(b"  </file>\n")?;
    }
    writer.write_all(b"</checkstyle>\n")
}

fn write_error<W: Write>(writer: &mut W, record: &OffenderRecord) -> io::Result<()> {
    let message = record.default_message();
    write!(writer, "    <error line=\"{}\"", record.start_line.max(1),)?;
    if let Some(col) = record.start_col {
        write!(writer, " column=\"{col}\"")?;
    }
    writeln!(
        writer,
        " severity=\"{}\" message=\"{}\" source=\"{}.{}\"/>",
        record.severity.as_str(),
        XmlAttr(&message),
        TOOL_ID,
        XmlAttr(&record.metric),
    )
}

/// Format adapter that XML-escapes attribute values. We escape the five
/// XML predefined entities plus control characters that are not allowed
/// in XML 1.0 attribute values (we replace them with `?` so the output
/// remains a valid document; lossy but predictable).
struct XmlAttr<'a>(&'a str);

impl std::fmt::Display for XmlAttr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write as _;
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&apos;")?,
                // Tab, newline, CR are legal in attribute values but
                // CI consumers are friendlier when newlines stay
                // literal — keep them as-is. Other C0 controls are
                // illegal in XML 1.0; replace with '?' rather than
                // emit a malformed document.
                '\t' | '\n' | '\r' => f.write_char(ch)?,
                c if (c as u32) < 0x20 => f.write_char('?')?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::offenders::Severity;
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
        write_checkstyle(offenders, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    #[test]
    fn empty_emits_self_closing_root() {
        insta::assert_snapshot!(render(&[]), @r###"
        <?xml version="1.0" encoding="UTF-8"?>
        <checkstyle version="4.3"/>
        "###);
    }

    #[test]
    fn single_offender_round_trips() {
        let offenders = vec![rec("src/foo.rs", "cyclomatic", 17.0, 15.0)];
        insta::assert_snapshot!(render(&offenders), @r###"
        <?xml version="1.0" encoding="UTF-8"?>
        <checkstyle version="4.3">
          <file name="src/foo.rs">
            <error line="42" column="5" severity="warning" message="cyclomatic 17 exceeds limit 15" source="big-code-analysis.cyclomatic"/>
          </file>
        </checkstyle>
        "###);
    }

    #[test]
    fn multiple_files_grouped_alphabetically() {
        let offenders = vec![
            rec("src/zeta.rs", "cyclomatic", 20.0, 15.0),
            rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
            rec("src/alpha.rs", "halstead.volume", 1234.5, 1000.0),
        ];
        insta::assert_snapshot!(render(&offenders), @r###"
        <?xml version="1.0" encoding="UTF-8"?>
        <checkstyle version="4.3">
          <file name="src/alpha.rs">
            <error line="42" column="5" severity="warning" message="loc.lloc 250 exceeds limit 100" source="big-code-analysis.loc.lloc"/>
            <error line="42" column="5" severity="warning" message="halstead.volume 1234.5 exceeds limit 1000" source="big-code-analysis.halstead.volume"/>
          </file>
          <file name="src/zeta.rs">
            <error line="42" column="5" severity="warning" message="cyclomatic 20 exceeds limit 15" source="big-code-analysis.cyclomatic"/>
          </file>
        </checkstyle>
        "###);
    }

    #[test]
    fn error_severity_renders_as_error() {
        let mut r = rec("a.rs", "cyclomatic", 99.0, 15.0);
        r.severity = Severity::Error;
        let out = render(&[r]);
        assert!(out.contains(r#"severity="error""#), "{out}");
    }

    #[test]
    fn missing_column_omits_attribute() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_col = None;
        let out = render(&[r]);
        assert!(!out.contains("column="), "{out}");
        assert!(out.contains(r#"line="42""#), "{out}");
    }

    #[test]
    fn xml_special_chars_in_path_and_metric_are_escaped() {
        let r = OffenderRecord {
            path: PathBuf::from(r#"src/<a&b>"c'd.rs"#),
            function: None,
            start_line: 1,
            end_line: 1,
            start_col: None,
            metric: r#"weird"&<metric>"#.into(),
            value: 1.0,
            limit: 0.0,
            severity: Severity::Warning,
        };
        let out = render(&[r]);
        assert!(
            out.contains(r#"name="src/&lt;a&amp;b&gt;&quot;c&apos;d.rs""#),
            "{out}"
        );
        assert!(
            out.contains(r#"source="big-code-analysis.weird&quot;&amp;&lt;metric&gt;""#),
            "{out}"
        );
    }

    #[test]
    fn start_line_zero_is_clamped_to_one() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_line = 0;
        let out = render(&[r]);
        assert!(out.contains(r#"line="1""#), "{out}");
    }

    #[test]
    fn control_characters_in_message_replaced() {
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: None,
            start_line: 1,
            end_line: 1,
            start_col: None,
            // metric name carries a NUL — bizarre, but escape must keep
            // the document well-formed.
            metric: "weird\u{0001}name".into(),
            value: 1.0,
            limit: 0.0,
            severity: Severity::Warning,
        };
        let out = render(&[r]);
        assert!(out.contains("weird?name"), "{out}");
    }
}
