//! Integration tests for the CSV output format.
//!
//! Per-language coverage is exercised because metric availability
//! varies (WMC / NPM / NPA only become non-empty for OOP languages).
//! Each fixture is small and self-contained so the snapshots stay
//! reviewable in code review.

// Existing tests use the generic `Parser<T>` flavour together with
// `metrics(&parser, &path)`. Both halves of that surface remain
// available; `metrics` is `#[deprecated]` post-#254 in favour of
// `analyze(Source { ... }, ...)`. The two seams give equivalent
// `FuncSpace` results for valid-UTF-8 paths, so the CSV snapshot
// coverage is unchanged. Scope the allowance here rather than
// migrating: these tests double as regression coverage for the
// deprecated shim.
#![allow(deprecated)]

use std::path::{Path, PathBuf};

use big_code_analysis::{
    CSV_HEADER, CppParser, ParserTrait, PythonParser, RustParser, metrics, write_csv,
};

fn render_csv<T: ParserTrait>(parser: &T, path: &Path) -> String {
    let space = metrics(parser, path).expect("metrics returns Some for valid input");
    let mut buf = Vec::new();
    write_csv(&space, path, &mut buf).expect("writing to Vec is infallible");
    String::from_utf8(buf).expect("output is UTF-8")
}

/// Each row must have exactly `CSV_HEADER.len()` comma-separated
/// fields *outside* of any quoted strings. The csv crate handles
/// quoting; this smoke check just confirms we never emit a malformed
/// row.
fn assert_well_formed(csv_text: &str) {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_text.as_bytes());
    let mut rows = 0;
    for record in rdr.records() {
        let record = record.expect("csv parses round-trip");
        assert_eq!(
            record.len(),
            CSV_HEADER.len(),
            "row {rows} had {} fields, expected {}",
            record.len(),
            CSV_HEADER.len()
        );
        rows += 1;
    }
    assert!(rows >= 2, "expected header + at least one data row");
}

#[test]
fn csv_rust_function_and_impl() {
    let source = r"
struct Counter { n: u32 }

impl Counter {
    fn bump(&mut self) -> u32 {
        if self.n > 10 {
            self.n
        } else {
            self.n += 1;
            self.n
        }
    }
}
";
    let path = PathBuf::from("counter.rs");
    let parser = RustParser::new(source.as_bytes().to_vec(), &path, None);
    let out = render_csv(&parser, &path);
    assert_well_formed(&out);

    insta::assert_snapshot!("csv_rust_counter", out);
}

#[test]
fn csv_python_class() {
    let source = r#"
class Greeter:
    def __init__(self, name):
        self.name = name

    def greet(self):
        if self.name:
            return f"Hello, {self.name}!"
        return "Hello!"
"#;
    let path = PathBuf::from("greeter.py");
    let parser = PythonParser::new(source.as_bytes().to_vec(), &path, None);
    let out = render_csv(&parser, &path);
    assert_well_formed(&out);

    insta::assert_snapshot!("csv_python_greeter", out);
}

#[test]
fn csv_cpp_namespace_and_class() {
    let source = r"
namespace ns {
class Widget {
public:
    int value() const { return v_; }
    void set(int x) { v_ = x; }
private:
    int v_;
};
}
";
    let path = PathBuf::from("widget.cc");
    let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
    let out = render_csv(&parser, &path);
    assert_well_formed(&out);

    insta::assert_snapshot!("csv_cpp_widget", out);
}

#[test]
fn csv_header_row_is_documented_constant() {
    // Cheap regression: if anyone reorders columns in csv.rs the
    // CSV_HEADER constant and the actual header row must move
    // together. write_csv asserts this internally too, but having
    // the test in the integration suite makes the contract obvious
    // to downstream consumers reading these tests.
    let path = PathBuf::from("empty.rs");
    let parser = RustParser::new(b"".to_vec(), &path, None);
    let space = metrics(&parser, &path).expect("metrics returns Some");
    let mut buf = Vec::new();
    write_csv(&space, &path, &mut buf).expect("ok");
    let text = String::from_utf8(buf).expect("utf-8");
    let header: Vec<&str> = text
        .lines()
        .next()
        .expect("at least header row")
        .split(',')
        .collect();
    assert_eq!(header, CSV_HEADER.to_vec());
}
