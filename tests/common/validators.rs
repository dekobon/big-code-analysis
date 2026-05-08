//! Format-validity helpers for the integration suite.
//!
//! Each helper validates one of the output formats produced by
//! `big_code_analysis::output::*` against either its published schema
//! (SARIF) or a structural mirror of its upstream XSD / well-formedness
//! contract (Checkstyle, HTML).
//!
//! Reused across:
//!
//! - `tests/sarif_test.rs`
//! - `tests/checkstyle_test.rs`
//! - `tests/html_test.rs`
//!
//! The CLI crate has its own duplicate at
//! `big-code-analysis-cli/tests/common/validators.rs` because Cargo
//! `[dev-dependencies]` and shared modules do not propagate across
//! workspace members.

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

// --------------------------------------------------------------------
// HTML — well-formedness via quick-xml, scoped to <body>.
// --------------------------------------------------------------------

/// Validate the well-formedness of an HTML document by extracting the
/// `<body>...</body>` block, removing inline `<style>` and `<script>`
/// blocks (whose content contains characters that are legal HTML5 but
/// not legal XML character data), wrapping the result in a synthetic
/// `<root>` element, and parsing with `quick-xml`.
///
/// The static head (doctype, meta, title) and the inline style/script
/// bodies are intentionally skipped because:
///
/// 1. Inline `<script>` content contains JS comparison operators
///    (`<`, `>`) that are not legal XML character data.
/// 2. Inline `<style>` content can contain CSS escape sequences and
///    selectors that confuse XML parsers.
/// 3. `<meta charset="utf-8">` is HTML5 syntax (void element without
///    `/>`) so an XML parser waits for `</meta>` and hits EOF.
///
/// The body's dynamic content (table, headers, rows, cells with
/// `data-metric` / `data-value` attributes) is what regression-checks
/// need to cover. The static head and inline assets are fixed and
/// already snapshot-pinned by the unit tests in `src/output/html.rs`.
///
/// Panics with a descriptive message including byte offset on parse
/// failure.
pub fn assert_html_well_formed(html_text: &str) {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let body = extract_body(html_text);
    let stripped = strip_block(body, "style");
    let stripped = strip_block(&stripped, "script");
    let wrapped = format!("<root>{stripped}</root>");

    let mut reader = Reader::from_str(&wrapped);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    loop {
        let pos = reader.buffer_position();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                let snippet_start = (pos as usize).saturating_sub(40);
                let snippet_end = ((pos as usize) + 40).min(wrapped.len());
                let snippet = &wrapped[snippet_start..snippet_end];
                panic!("html: not well-formed at byte {pos}: {e}\n  near: {snippet:?}");
            }
        }
        buf.clear();
    }
}

/// Find `<body...>...</body>` and return its inner content. Matches
/// only a true `<body>` element start tag (`<body` followed by `>`,
/// whitespace, or `/`) so a `<body` substring inside JS, a comment,
/// or a title can't masquerade as the document's body. Panics if the
/// document does not have a matched `<body>` open/close pair.
fn extract_body(html: &str) -> &str {
    let Some(body_open_pos) = find_element_start(html, "body") else {
        panic!("html: no <body> element in document");
    };
    let Some(rel_close) = html[body_open_pos..].find('>') else {
        panic!("html: <body has no closing > at byte {body_open_pos}");
    };
    let body_open_end = body_open_pos + rel_close + 1;
    let Some(body_close_pos) = html.rfind("</body>") else {
        panic!("html: no </body> in document (open was at byte {body_open_pos})");
    };
    if body_close_pos < body_open_end {
        panic!("html: </body> at byte {body_close_pos} precedes <body> at byte {body_open_pos}");
    }
    &html[body_open_end..body_close_pos]
}

/// Find the byte offset of an element start tag for `tag` (e.g.
/// `body`, `style`, `script`). The match requires `<tag` followed by
/// `>`, whitespace, or `/` so substrings like `<bodyfoo` or `<body` in
/// quoted attribute values cannot match.
fn find_element_start(text: &str, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find(&needle) {
        let abs = cursor + rel;
        let after = abs + needle.len();
        // The HTML5 tag-name terminators: `>`, `/`, or ASCII whitespace.
        match text.as_bytes().get(after) {
            Some(b) if matches!(b, b'>' | b'/') || b.is_ascii_whitespace() => return Some(abs),
            None => return None,
            _ => cursor = after,
        }
    }
    None
}

/// Replace the *content* of every `<tag ...>...</tag>` element with
/// nothing, leaving the tags themselves intact so the surrounding
/// markup still parses. Tolerates attributes on the start tag (e.g.
/// `<style nonce="...">`); the start tag is matched via
/// [`find_element_start`] so a `<tag` substring elsewhere can't false-
/// match.
///
/// Used to drop `<script>` / `<style>` bodies from the HTML well-
/// formedness check, since their content (JS comparison operators,
/// CSS escape sequences) is HTML5-legal but XML-illegal.
fn strip_block(text: &str, tag: &str) -> String {
    let end_tag = format!("</{tag}>");
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(start_abs) = find_element_start(&text[cursor..], tag).map(|p| cursor + p) else {
            out.push_str(&text[cursor..]);
            break;
        };
        // Emit everything up to the start tag, then the start tag itself.
        let Some(start_close_rel) = text[start_abs..].find('>') else {
            // Malformed open tag; emit verbatim and bail.
            out.push_str(&text[cursor..]);
            break;
        };
        let start_close_abs = start_abs + start_close_rel + 1;
        out.push_str(&text[cursor..start_close_abs]);
        // Find the matching close tag and skip the inner content.
        let Some(end_rel) = text[start_close_abs..].find(&end_tag) else {
            // Unbalanced; emit rest verbatim and let quick-xml report it.
            out.push_str(&text[start_close_abs..]);
            break;
        };
        out.push_str(&end_tag);
        cursor = start_close_abs + end_rel + end_tag.len();
    }
    out
}
