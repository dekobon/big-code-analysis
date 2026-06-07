//! Integration tests for the CSV output format.
//!
//! Per-language coverage is exercised because metric availability
//! varies (WMC / NPM / NPA only become non-empty for OOP languages).
//! Each fixture is small and self-contained so the snapshots stay
//! reviewable in code review.

// These tests drive the public `analyze` + `Source` seam — the single
// public analysis entry point after the path-positional surface was
// retired in #570. For valid-UTF-8 names the resulting `FuncSpace` is
// byte-for-byte what the old `metrics(&parser, &path)` shim produced, so
// the CSV snapshot coverage is unchanged.

use std::path::{Path, PathBuf};

use big_code_analysis::{CSV_HEADER, LANG, MetricsOptions, Source, analyze, write_csv};

fn render_csv(lang: LANG, source: &[u8], path: &Path) -> String {
    let name = path.to_str().map(str::to_owned);
    let space = analyze(
        Source::new(lang, source).with_name(name),
        MetricsOptions::default(),
    )
    .expect("analyze returns a top-level space for valid input");
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
    let out = render_csv(LANG::Rust, source.as_bytes(), &path);
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
    let out = render_csv(LANG::Python, source.as_bytes(), &path);
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
    let out = render_csv(LANG::Cpp, source.as_bytes(), &path);
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
    let space = analyze(
        Source::new(LANG::Rust, b"").with_name(path.to_str().map(str::to_owned)),
        MetricsOptions::default(),
    )
    .expect("analyze returns a top-level space");
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
