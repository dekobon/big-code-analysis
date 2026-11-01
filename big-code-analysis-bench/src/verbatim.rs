//! Library-level zero-drift oracle behind the `verbatim-oracle` binary
//! (#1744, from the #1697 driver).
//!
//! Reads each input file verbatim — no newline normalisation, unlike
//! the CLI's reader, so this is the one path that reaches a node ending
//! at EOF (`.claude/rules/testing.md`, "Know what the harness normalises
//! away") — and emits, per file, the serialised `metrics`, `ops`, the
//! `Alterator`-built AST export under both span/comment settings, the
//! `find` results for `call` / `string` / `function`, and the `count`
//! totals. Each file's record is one JSON object on one line, keyed by
//! path and language, so two runs diff line by line
//! (`utils/drift-oracle.py` captures them, `utils/drift-compare.py`
//! compares).
//!
//! A view that fails is an error for the whole file, never a record:
//! the cached driver serialised a failed `find` as `[]`, which compares
//! equal to a real empty result, so a regression that broke the view
//! would have read as zero drift. [`record`] returns `Err` instead, and
//! [`run`] stops at the first failing file with its name, writing
//! nothing for it.

use std::io::Write;
use std::path::Path;

use big_code_analysis::{Ast, AstCfg, LANG, MetricsOptions, Source};

/// Usage text for the `verbatim-oracle` binary.
pub const USAGE: &str = "usage: verbatim-oracle <language-slug> <file>...\n";

/// The node-kind filters each record carries `find` and `count` results
/// for, in output order.
const FILTERS: [&str; 3] = ["call", "string", "function"];

/// One file's verbatim record as a single JSON line (no trailing
/// newline), or the first view that failed, naming the file.
///
/// # Errors
///
/// The parse, `metrics`, `ops` or any `find` view failing. A failure
/// here is never turned into a partial record, so an `Err` can never
/// compare equal to any `Ok` line.
pub fn record(lang: LANG, path: &str, code: &[u8]) -> Result<String, String> {
    let fail = |view: &str, e: &dyn std::fmt::Display| format!("{path}: {view}: {e}");
    let ast = Ast::parse(Source::new(lang, code).with_name(Some(path.to_owned())))
        .map_err(|e| fail("parse", &e))?;
    let metrics = ast
        .metrics(MetricsOptions::default())
        .map_err(|e| fail("metrics", &e))?;
    let ops = ast.ops().map_err(|e| fail("ops", &e))?;
    let export = |span: bool, comment: bool| {
        ast.dump(AstCfg {
            id: String::new(),
            language: lang.to_string(),
            comment,
            span,
        })
    };
    let mut record = serde_json::json!({
        "path": path,
        "lang": lang.to_string(),
        "metrics": metrics.to_wire(),
        "ops": ops.to_wire(),
        "export_span_comment": export(true, true),
        "export_nospan_nocomment": export(false, false),
    });
    for filter in FILTERS {
        let filters = [filter.to_owned()];
        let found: Vec<serde_json::Value> = ast
            .find(&filters)
            .map_err(|e| fail(&format!("find {filter}"), &e))?
            .iter()
            .map(|n| serde_json::json!([n.kind(), n.kind_id(), n.start_byte(), n.end_byte()]))
            .collect();
        record[format!("find_{filter}")] = found.into();
        record[format!("count_{filter}")] = serde_json::json!(ast.count(&filters));
    }
    Ok(record.to_string())
}

/// Writes one record line per file to `out`, stopping at the first file
/// that cannot be read or recorded.
///
/// # Errors
///
/// An unknown language slug, an unreadable file (named), a failed view
/// (named with its file by [`record`]), or a write failure.
pub fn run<I>(lang: &str, paths: I, out: &mut impl Write) -> Result<(), String>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    let lang: LANG = lang.parse().map_err(|e| format!("{lang}: {e}"))?;
    for path in paths {
        let path = path.as_ref();
        let name = path
            .to_str()
            .ok_or_else(|| format!("{}: path is not valid UTF-8", path.display()))?;
        let code = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        let line = record(lang, name, &code)?;
        writeln!(out, "{line}").map_err(|e| format!("{name}: writing stdout: {e}"))?;
    }
    Ok(())
}

/// The `verbatim-oracle` binary's body: parses `argv` (the program name
/// already stripped), runs, and renders the failure for stderr.
///
/// # Errors
///
/// Missing arguments (with [`USAGE`]) or anything [`run`] reports.
pub fn main_with(argv: &[String], out: &mut impl Write) -> Result<(), String> {
    let [lang, paths @ ..] = argv else {
        return Err(format!("missing language slug\n{USAGE}"));
    };
    if paths.is_empty() {
        return Err(format!("no input files\n{USAGE}"));
    }
    run(lang, paths, out)
}

#[cfg(test)]
mod tests {
    use super::{FILTERS, main_with, record, run};
    use big_code_analysis::LANG;

    fn parse_line(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("a record is one JSON object")
    }

    /// A record carries every view, on the raw bytes: the input has no
    /// trailing newline, and the `function` kind the Rust grammar emits
    /// for `fn` is found and counted.
    #[test]
    fn record_carries_every_view_verbatim() {
        let line = record(LANG::Rust, "f.rs", b"fn f() { g(\"s\") }").expect("a clean parse");
        let v = parse_line(&line);
        assert_eq!(v["path"], "f.rs");
        assert_eq!(v["lang"], "rust");
        for key in [
            "metrics",
            "ops",
            "export_span_comment",
            "export_nospan_nocomment",
        ] {
            assert!(v[key].is_object(), "{key} missing: {line}");
        }
        for filter in FILTERS {
            assert!(
                v[format!("find_{filter}")].is_array(),
                "find_{filter} missing"
            );
            assert!(
                v[format!("count_{filter}")].is_array(),
                "count_{filter} missing"
            );
        }
        assert_eq!(v["find_function"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["count_function"][0], 1);
        assert_eq!(v["find_call"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["find_string"].as_array().map(Vec::len), Some(1));
        // The span is reported in verbatim bytes: `g("s")` starts at 9.
        assert_eq!(v["find_call"][0][2], 9);
        assert_eq!(
            v["export_span_comment"]["root"]["type"], "source_file",
            "the export carries the tree"
        );
    }

    /// An unreadable input is an error naming the file, and nothing is
    /// written for it — not an empty or partial record.
    #[test]
    fn unreadable_input_names_the_file_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = dir.path().join("a.rs");
        std::fs::write(&good, "fn a() {}").expect("write");
        let missing = dir.path().join("missing.rs");
        let mut out = Vec::new();
        let err = run("rust", [&good, &missing], &mut out).expect_err("missing file fails");
        assert!(
            err.starts_with(&format!("{}: ", missing.display())),
            "error names the file: {err}"
        );
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(
            text.lines().count(),
            1,
            "only the readable file was recorded"
        );
        assert_eq!(
            parse_line(text.lines().next().expect("one line"))["path"].as_str(),
            good.to_str()
        );
    }

    /// A failed view is never a record: the result type keeps `Err` apart
    /// from every `Ok`, and in particular from the `Ok` of a genuinely
    /// empty input — the case the cached driver's `unwrap_or_default()`
    /// collapsed. The failure exercised is the one the library can
    /// report today, an unknown slug / disabled language at `run`, which
    /// stops before any line is written.
    #[test]
    fn a_failed_view_cannot_compare_equal_to_a_successful_one() {
        let empty = record(LANG::Rust, "e.rs", b"").expect("empty input parses");
        let v = parse_line(&empty);
        assert_eq!(v["find_call"], serde_json::json!([]));
        assert_eq!(v["count_call"], serde_json::json!([0, 1]));

        let mut out = Vec::new();
        let err = run("not-a-language", ["e.rs"], &mut out).expect_err("unknown slug fails");
        assert!(err.starts_with("not-a-language: "), "{err}");
        assert_eq!(out, b"", "nothing written on failure");
        assert_ne!(Err::<String, _>(err), Ok(empty));
    }

    #[test]
    fn main_rejects_missing_arguments_with_usage() {
        let mut out = Vec::new();
        let err = main_with(&[], &mut out).expect_err("no slug");
        assert!(err.contains("usage: verbatim-oracle"), "{err}");
        let err = main_with(&["rust".to_owned()], &mut out).expect_err("no files");
        assert!(err.starts_with("no input files"), "{err}");
        assert_eq!(out, b"");
    }
}
