//! Format-validity helpers for the CLI integration suite.
//!
//! Duplicate of `tests/common/validators.rs` in the lib crate (Cargo
//! does not share modules across workspace members). Keep the two
//! files in sync; they are small enough that drift is easily spotted
//! during code review.
//!
//! The vendored schema files live in the lib crate's
//! `tests/fixtures/`; we reach them via a workspace-relative
//! `include_str!` path with three `..` segments:
//!
//!   `big-code-analysis-cli/tests/common/validators.rs`
//!     up 1 -> `big-code-analysis-cli/tests/`
//!     up 2 -> `big-code-analysis-cli/`
//!     up 3 -> workspace root
//!     down -> `tests/fixtures/sarif-2.1.0.json`

// Inner `#![allow(dead_code)]` is unneeded — the `pub mod validators`
// in `big-code-analysis-cli/tests/common/mod.rs` already carries it.

use std::sync::OnceLock;

// --------------------------------------------------------------------
// SARIF — full schema validation against the vendored Draft-07 schema.
// --------------------------------------------------------------------

const SARIF_SCHEMA_JSON: &str = include_str!("../../../tests/fixtures/sarif-2.1.0.json");

/// Validate a SARIF document against the vendored 2.1.0 JSON Schema.
///
/// On failure, returns one human-readable string per violation. The
/// schema is parsed once per test binary via `OnceLock`.
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

// --------------------------------------------------------------------
// Checkstyle — structural walker mirroring the official XSD.
// --------------------------------------------------------------------

/// Walk a Checkstyle 4.3 XML document via `quick-xml` and assert
/// structural conformance to the upstream XSD. See the lib-crate
/// helper for the full contract; this duplicate matches it byte-for-
/// byte modulo whitespace.
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

/// Mirror of `xs:positiveInteger`: parse as `u32`, require ≥ 1.
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
/// `<body>...</body>` block, removing inline `<style>` / `<script>`
/// blocks (whose content is HTML5-legal but XML-illegal), wrapping in
/// a synthetic `<root>`, and parsing with `quick-xml`. See the
/// lib-crate helper for the full contract.
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

/// Find the byte offset of an element start tag for `tag`. The match
/// requires `<tag` followed by `>`, whitespace, or `/`, so substrings
/// like `<bodyfoo` or `<body` inside quoted attribute values cannot
/// match.
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

/// Replace the content of every `<tag ...>...</tag>` element with
/// nothing. Tolerates attributes on the start tag.
fn strip_block(text: &str, tag: &str) -> String {
    let end_tag = format!("</{tag}>");
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(start_abs) = find_element_start(&text[cursor..], tag).map(|p| cursor + p) else {
            out.push_str(&text[cursor..]);
            break;
        };
        let Some(start_close_rel) = text[start_abs..].find('>') else {
            out.push_str(&text[cursor..]);
            break;
        };
        let start_close_abs = start_abs + start_close_rel + 1;
        out.push_str(&text[cursor..start_close_abs]);
        let Some(end_rel) = text[start_close_abs..].find(&end_tag) else {
            out.push_str(&text[start_close_abs..]);
            break;
        };
        out.push_str(&end_tag);
        cursor = start_close_abs + end_rel + end_tag.len();
    }
    out
}
