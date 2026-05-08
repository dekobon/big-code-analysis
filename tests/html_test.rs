//! Integration tests for the HTML output format.
//!
//! Verifies well-formedness of the dynamic content (the `<body>`
//! block: doctype-free heading, table, sortable header, nested rows,
//! per-cell `data-metric` / `data-value` attributes) via the
//! `quick-xml`-driven walker in
//! [`common::validators::assert_html_well_formed`]. See that helper's
//! doc-comment for why the static head (inline style/script,
//! `<meta charset="utf-8">`) is intentionally skipped.

use std::path::{Path, PathBuf};

use big_code_analysis::{FuncSpace, ParserTrait, RustParser, SpaceKind, metrics, write_html};

mod common;
use common::fixtures::empty_space;
use common::validators::assert_html_well_formed;

fn render(space: &FuncSpace, path: &Path) -> String {
    let mut buf = Vec::new();
    write_html(space, path, &mut buf).expect("writing to Vec is infallible");
    String::from_utf8(buf).expect("output is UTF-8")
}

#[test]
fn html_empty_unit_is_well_formed() {
    let space = empty_space("root", SpaceKind::Unit, 1, 1);
    let out = render(&space, Path::new("a.rs"));
    assert_html_well_formed(&out);
}

#[test]
fn html_nested_spaces_is_well_formed() {
    let mut root = empty_space("root", SpaceKind::Unit, 1, 100);
    let mut outer = empty_space("outer", SpaceKind::Function, 10, 50);
    let inner = empty_space("inner", SpaceKind::Function, 20, 30);
    outer.spaces.push(inner);
    let sibling = empty_space("sibling", SpaceKind::Function, 60, 80);
    root.spaces.push(outer);
    root.spaces.push(sibling);

    let out = render(&root, Path::new("src/example.rs"));
    assert_html_well_formed(&out);
}

#[test]
fn html_with_xss_payload_remains_well_formed() {
    // Crafted source path and space name with HTML metacharacters —
    // the writer must escape them so the document still parses
    // cleanly. Mirrors the existing XSS unit test in
    // `src/output/html.rs::tests`.
    let space = empty_space("<svg/onload=alert(1)>", SpaceKind::Function, 1, 1);
    let out = render(&space, Path::new("a&b\"c'd<e>f.rs"));
    assert_html_well_formed(&out);
}

#[test]
fn html_data_attributes_are_properly_quoted() {
    // Every numeric cell carries `data-metric="..."` and
    // `data-value="..."`. Improper quoting (missing `=`, unbalanced
    // quotes, raw `&` inside attribute) breaks XML parsing. Drive the
    // writer with a real metric tree from a small Rust source so the
    // attributes carry realistic values, then validate well-formedness.
    let source = b"fn f(x: u32) -> u32 { if x > 0 { x } else { 0 } }\n";
    let path = PathBuf::from("a.rs");
    let parser = RustParser::new(source.to_vec(), &path, None);
    let space = metrics(&parser, &path).expect("metrics returns Some for valid input");

    let out = render(&space, &path);
    assert_html_well_formed(&out);

    // Sanity-check that the data attributes really do appear (the
    // walker validates structural correctness only; this asserts the
    // semantic shape exists).
    assert!(
        out.contains("data-metric=\"loc.sloc\""),
        "expected a data-metric=\"loc.sloc\" cell in:\n{out}"
    );
    assert!(
        out.contains("data-value=\""),
        "expected at least one data-value attribute in:\n{out}"
    );
}
