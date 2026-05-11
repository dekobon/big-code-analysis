#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Format-validity helpers for the integration suite.
//!
//! Each helper validates one of the output formats produced by
//! `big_code_analysis::output::*` against either its published schema
//! (SARIF) or a structural mirror of its upstream XSD / well-formedness
//! contract (Checkstyle).
//!
//! Reused across:
//!
//! - `tests/sarif_test.rs`
//! - `tests/checkstyle_test.rs`
//!
//! The CLI crate has its own copy at
//! `big-code-analysis-cli/tests/common/validators.rs` because Cargo
//! `[dev-dependencies]` and shared modules do not propagate across
//! workspace members. That copy additionally carries an HTML
//! well-formedness helper used by the `bca report html` integration
//! tests.

// Inner `#![allow(dead_code)]` is unneeded — the `pub mod validators` in
// `tests/common/mod.rs` already carries it. Each integration test only
// uses a subset of the helpers, but since they're behind `pub mod`, the
// outer allow covers them.

use std::sync::OnceLock;

// --------------------------------------------------------------------
// SARIF — full schema validation against the vendored Draft-07 schema.
// --------------------------------------------------------------------

const SARIF_SCHEMA_JSON: &str = include_str!("../fixtures/sarif-2.1.0.json");

/// Validate a SARIF document against the vendored 2.1.0 JSON Schema.
///
/// On failure, returns one human-readable string per violation, each
/// including the JSON-pointer path provided by `jsonschema`. The schema
/// is parsed once per test binary via `OnceLock`.
pub fn validate_sarif(json_text: &str) -> Result<(), Vec<String>> {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

    let validator = VALIDATOR.get_or_init(|| {
        let schema: serde_json::Value = serde_json::from_str(SARIF_SCHEMA_JSON)
            .expect("vendored SARIF schema is valid JSON; refresh tests/fixtures/sarif-2.1.0.json");
        jsonschema::draft7::new(&schema).expect("vendored SARIF schema is a valid Draft-07 schema")
    });

    let instance: serde_json::Value = match serde_json::from_str(json_text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("SARIF output is not valid JSON: {e}")]),
    };

    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|err| format!("{err} (at {})", err.instance_path()))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Parse the vendored SARIF schema and return its top-level `$id` and
/// `$schema` fields. Used by the schema-canary self-check test in
/// `tests/sarif_test.rs` to detect a refresh that vendored the wrong
/// file.
pub fn sarif_schema_metadata() -> (String, String) {
    let schema: serde_json::Value =
        serde_json::from_str(SARIF_SCHEMA_JSON).expect("vendored SARIF schema is valid JSON");
    let id = schema["$id"].as_str().unwrap_or("").to_owned();
    let dialect = schema["$schema"].as_str().unwrap_or("").to_owned();
    (id, dialect)
}

// --------------------------------------------------------------------
// Checkstyle — structural walker mirroring the official XSD.
// --------------------------------------------------------------------

/// Walk a Checkstyle 4.3 XML document via `quick-xml` and assert
/// structural conformance to `tests/fixtures/checkstyle-report-1.0.0.xsd`:
///
/// - root element `<checkstyle>` with `version` attribute (always
///   present in our writer; the XSD declares it `xs:string` without
///   `use="required"`, but absence indicates a writer regression)
/// - each `<file>` has a required `name` attribute (`use="required"`)
/// - each `<error>` has `line`, `severity`, `message`, `source`;
///   `column` is optional and must satisfy `xs:positiveInteger` (>0)
///   when present
/// - `severity` is one of the XSD enum values: `{error, warning, info}`
///
/// Panics with a descriptive message on failure including the byte
/// position from `quick_xml::Reader::buffer_position()`.
pub fn assert_checkstyle_well_formed_and_structural(xml_text: &str) {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml_text);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut saw_root = false;
    let mut depth = 0usize;

    loop {
        let pos = reader.buffer_position();
        let evt = match reader.read_event_into(&mut buf) {
            Ok(e) => e,
            Err(e) => panic!("checkstyle parse error at byte {pos}: {e}"),
        };
        match evt {
            // Skip XML declaration, comments, doctype, CDATA, and text
            // nodes. (`trim_text(true)` discards whitespace-only text.)
            Event::Decl(_)
            | Event::Comment(_)
            | Event::DocType(_)
            | Event::CData(_)
            | Event::Text(_) => {}

            Event::Start(start) => {
                check_element(&start, &mut saw_root, pos);
                depth += 1;
            }
            Event::Empty(start) => {
                check_element(&start, &mut saw_root, pos);
                // Empty elements have no End event, so depth is not adjusted.
            }
            Event::End(end) => {
                let name_bytes = end.name();
                let name = std::str::from_utf8(name_bytes.as_ref()).unwrap_or_else(|_| {
                    panic!("checkstyle end-element name is not UTF-8 at byte {pos}")
                });
                if !matches!(name, "checkstyle" | "file" | "error" | "exception") {
                    panic!("checkstyle: unexpected end-element </{name}> at byte {pos}");
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            other => panic!("checkstyle: unexpected event {other:?} at byte {pos}"),
        }
        buf.clear();
    }

    if !saw_root {
        panic!("checkstyle: document did not contain a <checkstyle> root element");
    }
    if depth != 0 {
        panic!("checkstyle: unbalanced element depth {depth} at EOF");
    }
}

fn check_element(start: &quick_xml::events::BytesStart<'_>, saw_root: &mut bool, pos: u64) {
    let name_bytes = start.name();
    let name = std::str::from_utf8(name_bytes.as_ref())
        .unwrap_or_else(|_| panic!("checkstyle element name is not UTF-8 at byte {pos}"));

    match name {
        "checkstyle" => {
            if *saw_root {
                panic!("checkstyle: unexpected second <checkstyle> at byte {pos}");
            }
            *saw_root = true;
            require_attr(start, "version", "checkstyle", pos);
        }
        "file" => require_attr(start, "name", "file", pos),
        "error" => {
            require_attr(start, "line", "error", pos);
            require_attr(start, "severity", "error", pos);
            require_attr(start, "message", "error", pos);
            require_attr(start, "source", "error", pos);

            let sev = attr_value(start, "severity").expect("checked by require_attr above");
            if !matches!(sev.as_str(), "error" | "warning" | "info") {
                panic!(
                    "checkstyle: <error> severity={sev:?} not in XSD enum {{error, warning, info}} at byte {pos}"
                );
            }

            let line = attr_value(start, "line").expect("checked by require_attr above");
            assert_positive_integer(&line, "line", pos);
            if let Some(col) = attr_value(start, "column") {
                assert_positive_integer(&col, "column", pos);
            }
        }
        "exception" => { /* allowed by XSD; we don't emit them but accept them */ }
        other => panic!("checkstyle: unexpected element <{other}> at byte {pos}"),
    }
}

/// Assert that `value` parses as an unsigned integer ≥ 1, mirroring
/// the XSD `xs:positiveInteger` constraint applied to the `line` and
/// `column` attributes of `<error>`. Panics with a descriptive
/// message including the offending attribute name and byte position.
fn assert_positive_integer(value: &str, attr: &str, pos: u64) {
    let n: u32 = value.parse().unwrap_or_else(|_| {
        panic!("checkstyle: <error> {attr}={value:?} is not an unsigned integer at byte {pos}")
    });
    if n == 0 {
        panic!("checkstyle: <error> {attr}=0 violates xs:positiveInteger at byte {pos}");
    }
}

fn require_attr(start: &quick_xml::events::BytesStart<'_>, attr: &str, elem: &str, pos: u64) {
    if attr_value(start, attr).is_none() {
        panic!("checkstyle: <{elem}> missing required attribute `{attr}` at byte {pos}");
    }
}

fn attr_value(start: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    use std::borrow::Cow;
    for attr in start.attributes().with_checks(false).flatten() {
        if attr.key.as_ref() == name.as_bytes() {
            // unescape decodes character references like &lt; back to <;
            // fall back to a lossy decode if the attribute is malformed.
            return Some(attr.unescape_value().map_or_else(
                |_| String::from_utf8_lossy(&attr.value).into_owned(),
                Cow::into_owned,
            ));
        }
    }
    None
}
