// Sibling-file unit tests for `write_checkstyle`, wired in via
// `#[path = "checkstyle_tests.rs"] mod tests;` so the production
// `checkstyle.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`, so the
// self-scan walker skips this file the same way it skips `./tests/`.

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

#[test]
fn whitespace_in_attribute_round_trips_via_numeric_refs() {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    // XML 1.0 §3.3.3: a conforming parser collapses raw TAB / LF /
    // CR inside an attribute value to a single space on read. POSIX
    // paths legally contain '\n', so emitting them as literal bytes
    // would silently mangle every offender that lands on such a
    // file. Numeric character references are exempt from this
    // normalization — emit them and the parser-visible value
    // matches what we wrote.
    let r = OffenderRecord {
        path: PathBuf::from("src/weird\npath\twith\rwhitespace.rs"),
        function: None,
        start_line: 1,
        end_line: 1,
        start_col: None,
        metric: "cyclomatic".into(),
        value: 1.0,
        limit: 0.0,
        severity: Severity::Warning,
    };
    let out = render(&[r]);

    // Emitter side: the three whitespace bytes must appear as
    // numeric character references, never as literal bytes inside
    // the attribute.
    assert!(out.contains("&#xA;"), "missing &#xA; (LF) in {out}");
    assert!(out.contains("&#x9;"), "missing &#x9; (TAB) in {out}");
    assert!(out.contains("&#xD;"), "missing &#xD; (CR) in {out}");
    // The `name="..."` attribute itself must not contain a raw LF /
    // TAB / CR — otherwise attribute-value normalization would
    // collapse it to a space on read.
    let name_open = out.find("name=\"").expect("name attribute present");
    let after_open = &out[name_open + b"name=\"".len()..];
    let name_close = after_open.find('"').expect("name attribute closed");
    let attr_lit = &after_open[..name_close];
    assert!(
        !attr_lit.contains('\n') && !attr_lit.contains('\t') && !attr_lit.contains('\r'),
        "raw whitespace leaked into attribute literal: {attr_lit:?}"
    );

    // Parser side: re-parse with quick-xml and confirm the
    // round-tripped value still contains the original raw bytes.
    let mut reader = Reader::from_str(&out);
    let mut buf = Vec::new();
    let mut roundtripped: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf).expect("well-formed XML") {
            Event::Start(start) | Event::Empty(start) if start.name().as_ref() == b"file" => {
                for attr in start.attributes().with_checks(false).flatten() {
                    if attr.key.as_ref() == b"name" {
                        roundtripped = Some(
                            attr.unescape_value()
                                .expect("attribute value decodes")
                                .into_owned(),
                        );
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let roundtripped = roundtripped.expect("found <file name=...>");
    assert_eq!(roundtripped, "src/weird\npath\twith\rwhitespace.rs");
}

#[test]
fn xml_char_illegal_noncharacters_replaced_and_roundtrips() {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    // XML 1.0 §2.2's Char production excludes U+FFFE and U+FFFF.
    // Strict libxml2-based consumers (Jenkins, SonarQube) reject
    // them and every supplementary-plane non-character (U+nFFFE /
    // U+nFFFF for plane n in 1..=16) as fatal errors. All 32
    // plane-end non-characters must be substituted with `?`, and
    // the resulting document must re-parse cleanly.
    //
    // The chosen inputs span both halves of the bitmask
    // `(cp & 0xFFFF) >= 0xFFFE`:
    //   path:   U+FFFE U+FFFF       (BMP)
    //   metric: U+1FFFE U+10FFFF    (supplementary)
    let r = OffenderRecord {
        path: PathBuf::from("src/bad\u{FFFE}\u{FFFF}path.rs"),
        function: None,
        start_line: 1,
        end_line: 1,
        start_col: None,
        metric: "weird\u{1FFFE}\u{10FFFF}metric".into(),
        value: 1.0,
        limit: 0.0,
        severity: Severity::Warning,
    };
    let out = render(&[r]);

    // Emitter side: no non-character may appear literally.
    for nc in ['\u{FFFE}', '\u{FFFF}', '\u{1FFFE}', '\u{10FFFF}'] {
        assert!(
            !out.contains(nc),
            "non-character {:04X} leaked into output: {out:?}",
            nc as u32,
        );
    }
    // The substitute character must appear in both attribute values
    // (two `?` per offending string).
    assert!(out.contains("src/bad??path.rs"), "{out}");
    assert!(out.contains("weird??metric"), "{out}");

    // Parser side: the document must re-parse cleanly.
    let mut reader = Reader::from_str(&out);
    let mut buf = Vec::new();
    while !matches!(
        reader.read_event_into(&mut buf).expect("well-formed XML"),
        Event::Eof
    ) {
        buf.clear();
    }
}

#[test]
fn predefined_entities_still_escape_after_whitespace_fix() {
    // Regression guard: tightening the TAB/LF/CR arms must not
    // disturb the five predefined-entity escapes that this format
    // has always emitted.
    let r = OffenderRecord {
        path: PathBuf::from("a&b<c>d\"e'f.rs"),
        function: None,
        start_line: 1,
        end_line: 1,
        start_col: None,
        metric: "cyclomatic".into(),
        value: 1.0,
        limit: 0.0,
        severity: Severity::Warning,
    };
    let out = render(&[r]);
    assert!(
        out.contains(r#"name="a&amp;b&lt;c&gt;d&quot;e&apos;f.rs""#),
        "predefined-entity escapes regressed: {out}"
    );
}
