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
//! would have read as zero drift. [`record`] returns `Err` instead —
//! including for an export whose `root` is `None`, which the library
//! documents as "an error has occurred" and which would otherwise
//! serialise as `"root": null` — and [`run`] stops at the first failing
//! file with its name, writing nothing for it.

use std::io::Write;
use std::path::Path;

use big_code_analysis::{Ast, AstCfg, AstResponse, LANG, MetricsOptions, Source};

/// Usage text for the `verbatim-oracle` binary.
pub const USAGE: &str = "usage: verbatim-oracle <language-slug> <file>...\n";

/// The node-kind filters each record carries `find` and `count` results
/// for, in output order.
const FILTERS: [&str; 3] = ["call", "string", "function"];

/// One file's verbatim record as a JSON value, or the first view that
/// failed, naming the file. [`run`] streams the value to its writer
/// rather than rendering it to a `String` first: the export of a large
/// corpus file is the bulk of the record, and holding its text beside
/// its tree doubled peak memory.
///
/// # Errors
///
/// The parse, `metrics`, `ops`, either export or any `find` view
/// failing. An export with no root is a failure, not a record with
/// `"root": null`. A failure here is never turned into a partial
/// record, so an `Err` can never compare equal to any `Ok` value.
pub fn record(lang: LANG, path: &str, code: &[u8]) -> Result<serde_json::Value, String> {
    let fail = |view: &str, e: &dyn std::fmt::Display| format!("{path}: {view}: {e}");
    let ast = Ast::parse(Source::new(lang, code).with_name(Some(path.to_owned())))
        .map_err(|e| fail("parse", &e))?;
    let metrics = ast
        .metrics(MetricsOptions::default())
        .map_err(|e| fail("metrics", &e))?;
    let ops = ast.ops().map_err(|e| fail("ops", &e))?;
    let export = |view: &str, span: bool, comment: bool| {
        rooted(ast.dump(AstCfg {
            id: String::new(),
            language: lang.to_string(),
            comment,
            span,
        }))
        .map_err(|e| fail(view, &e))
    };
    let mut record = serde_json::json!({
        "path": path,
        "lang": lang.to_string(),
        "metrics": metrics.to_wire(),
        "ops": ops.to_wire(),
        "export_span_comment": export("dump", true, true)?,
        "export_nospan_nocomment": export("dump without span or comments", false, false)?,
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
    Ok(record)
}

/// An export with a root, or the reason it is not a record: the
/// library documents `root: None` as "an error has occurred", and
/// serialised as-is it would be `"root": null` — a line that compares
/// equal to itself across two revisions that both failed. Not reachable
/// on the pinned grammars (an empty or comment-only input still has a
/// root), so this is the module contract made explicit rather than a
/// repair.
fn rooted(response: AstResponse) -> Result<AstResponse, &'static str> {
    if response.root.is_none() {
        return Err("export has no root");
    }
    Ok(response)
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
        let value = record(lang, name, &code)?;
        let write_err = |e: &dyn std::fmt::Display| format!("{name}: writing stdout: {e}");
        serde_json::to_writer(&mut *out, &value).map_err(|e| write_err(&e))?;
        writeln!(out).map_err(|e| write_err(&e))?;
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
    use super::{FILTERS, main_with, record, rooted, run};
    use big_code_analysis::{AstResponse, LANG};

    fn parse_line(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("a record is one JSON object")
    }

    fn line_of(v: &serde_json::Value) -> String {
        v.to_string()
    }

    /// A record carries every view, on the raw bytes: the input has no
    /// trailing newline, and the `function` kind the Rust grammar emits
    /// for `fn` is found and counted.
    #[test]
    fn record_carries_every_view_verbatim() {
        let v = record(LANG::Rust, "f.rs", b"fn f() { g(\"s\") }").expect("a clean parse");
        let line = line_of(&v);
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

    /// A genuinely empty input is a record with empty views — the value
    /// the cached driver's `unwrap_or_default()` also produced for a
    /// *failed* view, which is why failures are `Err` here instead.
    #[test]
    fn empty_input_is_a_record_with_empty_views() {
        let v = record(LANG::Rust, "e.rs", b"").expect("empty input parses");
        assert_eq!(v["find_call"], serde_json::json!([]));
        assert_eq!(v["count_call"], serde_json::json!([0, 1]));
        assert_eq!(v["export_span_comment"]["root"]["type"], "source_file");
    }

    /// An export whose root is `None` is refused rather than recorded as
    /// `"root": null`. Constructed directly: no pinned grammar produces
    /// one.
    #[test]
    fn an_export_without_a_root_is_a_failure_not_a_record() {
        let rootless = AstResponse {
            id: String::new(),
            language: "rust".to_owned(),
            root: None,
        };
        assert_eq!(rooted(rootless).map(|_| ()), Err("export has no root"));
        let v = record(LANG::Rust, "c.rs", b"// only a comment").expect("comment-only parses");
        assert!(v["export_nospan_nocomment"]["root"].is_object());
    }

    /// The failure the library can report today, an unknown slug /
    /// disabled language, stops `run` before any line is written.
    #[test]
    fn unknown_slug_writes_nothing_before_the_first_record() {
        let mut out = Vec::new();
        let err = run("not-a-language", ["e.rs"], &mut out).expect_err("unknown slug fails");
        assert!(err.starts_with("not-a-language: "), "{err}");
        assert_eq!(out, b"", "nothing written on failure");
    }

    /// The record is streamed: one JSON object and a newline per file,
    /// byte-identical to the rendered value.
    #[test]
    fn run_streams_one_line_per_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.rs");
        std::fs::write(&a, "fn a() {}").expect("write");
        std::fs::write(&b, "fn b() { a() }").expect("write");
        let mut out = Vec::new();
        run("rust", [&a, &b], &mut out).expect("two clean files");
        let text = String::from_utf8(out).expect("utf-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for (line, path) in lines.iter().zip([&a, &b]) {
            let expected = record(
                LANG::Rust,
                path.to_str().expect("utf-8"),
                &std::fs::read(path).expect("read"),
            )
            .expect("clean");
            assert_eq!(*line, line_of(&expected));
        }
        assert!(text.ends_with('\n'));
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
