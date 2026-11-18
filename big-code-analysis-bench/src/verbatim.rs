//! Library-level zero-drift oracle behind the `verbatim-oracle` binary
//! (#1744, from the #1697 driver).
//!
//! Reads each input file verbatim — no newline normalisation, unlike
//! the CLI's reader, so this is the one path that reaches a node ending
//! at EOF (`docs/development/testing.md`, "Know what the harness normalises
//! away") — and emits, per file, the serialised `metrics`, `ops`, the
//! `Alterator`-built AST export under both span/comment settings, the
//! `find` results for `call` / `string` / `function`, and the `count`
//! totals. Each file's record is one JSON object on one line, keyed by
//! path and language, so two runs diff line by line
//! (`utils/drift-oracle.py` captures them, `utils/drift-compare.py`
//! compares).
//!
//! `--metrics-view` emits only the metric space tree with explicit
//! effective options and parsed byte length. It shares the raw reader
//! and parser with the default record; no filesystem configuration is
//! consulted. The default invocation retains its original record shape.
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

use big_code_analysis::{Ast, AstCfg, AstResponse, LANG, Metric, MetricsOptions, Source};

/// Usage text for the `verbatim-oracle` binary.
pub const USAGE: &str = "\
usage: verbatim-oracle <language-slug> <file>...
       verbatim-oracle --metrics-view [options] -- <language-slug> <file>...

Metrics-only records carry the selected options and the full metric space tree.
Options (each at most once; no configuration or environment overrides):
  --mi-charges-docs=false|true       default false
  --exclude-tests                  default false
  --cyclomatic-count-try=false|true default true
  --metrics NAME                   one canonical metric; default all
";

/// The node-kind filters each record carries `find` and `count` results
/// for, in output order.
const FILTERS: [&str; 3] = ["call", "string", "function"];

struct MetricView {
    exclude_tests: bool,
    count_cyclomatic_try: bool,
    mi_charges_docs: bool,
    metric: Option<Metric>,
}

impl MetricView {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut view = Self {
            exclude_tests: false,
            count_cyclomatic_try: true,
            mi_charges_docs: false,
            metric: None,
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let (name, value) = arg.split_once('=').unwrap_or((arg, ""));
            if !seen.insert(name) {
                return Err(format!("duplicate option: {name}"));
            }
            match (name, value) {
                ("--exclude-tests", "") if arg == name => view.exclude_tests = true,
                ("--metrics", "") if arg == name => {
                    let metric = args
                        .next()
                        .ok_or("--metrics needs a canonical metric name")?;
                    view.metric = Some(metric.parse::<Metric>().map_err(|e| e.to_string())?);
                }
                ("--mi-charges-docs", value) => {
                    view.mi_charges_docs = value
                        .parse()
                        .map_err(|_| "--mi-charges-docs requires =false or =true")?;
                }
                ("--cyclomatic-count-try", value) => {
                    view.count_cyclomatic_try = value
                        .parse()
                        .map_err(|_| "--cyclomatic-count-try requires =false or =true")?;
                }
                _ => return Err(format!("unknown metrics-view option: {arg}")),
            }
        }
        Ok(view)
    }

    fn record(&self, lang: LANG, path: &str, code: &[u8]) -> Result<serde_json::Value, String> {
        let ast = parse(lang, path, code)?;
        let mut options = MetricsOptions::default()
            .with_exclude_tests(self.exclude_tests)
            .with_count_cyclomatic_try(self.count_cyclomatic_try)
            .with_mi_charges_docs(self.mi_charges_docs);
        if let Some(metric) = self.metric {
            options = options.with_only(&[metric]);
        }
        let metrics = ast
            .metrics(options)
            .map_err(|e| format!("{path}: metrics: {e}"))?;
        Ok(serde_json::json!({
            "path": path,
            "lang": lang.to_string(),
            "source_bytes": ast.source().len(),
            "options": {
                "exclude_tests": self.exclude_tests,
                "count_cyclomatic_try": self.count_cyclomatic_try,
                "mi_charges_docs": self.mi_charges_docs,
                "metrics": self.metric.map_or_else(|| "all".to_owned(), |m| m.to_string()),
            },
            "metrics": metrics.to_wire(),
        }))
    }
}

fn parse(lang: LANG, path: &str, code: &[u8]) -> Result<Ast, String> {
    Ast::parse(Source::new(lang, code).with_name(Some(path.to_owned())))
        .map_err(|e| format!("{path}: parse: {e}"))
}

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
    let ast = parse(lang, path, code)?;
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
/// No input files, an unknown language slug, an unreadable file (named), a failed view
/// (named with its file by [`record`]), or a write failure.
pub fn run<I>(lang: &str, paths: I, out: &mut impl Write) -> Result<(), String>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    run_with(lang, paths, out, record)
}

fn run_with<I>(
    lang: &str,
    paths: I,
    out: &mut impl Write,
    record: impl Fn(LANG, &str, &[u8]) -> Result<serde_json::Value, String>,
) -> Result<(), String>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    let lang: LANG = lang.parse().map_err(|e| format!("{lang}: {e}"))?;
    let mut paths = paths.into_iter().peekable();
    if paths.peek().is_none() {
        return Err(format!("no input files\n{USAGE}"));
    }
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
/// Missing arguments (with [`USAGE`]), invalid metrics-view options, or
/// anything [`run`] reports.
pub fn main_with(argv: &[String], out: &mut impl Write) -> Result<(), String> {
    let (view, argv) = if argv.first().is_some_and(|arg| arg == "--metrics-view") {
        let end = argv
            .iter()
            .position(|arg| arg == "--")
            .ok_or_else(|| format!("--metrics-view requires -- before the language\n{USAGE}"))?;
        (Some(MetricView::parse(&argv[1..end])?), &argv[end + 1..])
    } else {
        (None, argv)
    };
    let [lang, paths @ ..] = argv else {
        return Err(format!("missing language slug\n{USAGE}"));
    };
    if let Some(view) = view {
        run_with(lang, paths, out, |lang, path, code| {
            view.record(lang, path, code)
        })
    } else {
        run(lang, paths, out)
    }
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

    fn metric_view(lang: &str, source: &[u8], options: &[&str]) -> serde_json::Value {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("input");
        std::fs::write(&path, source).expect("write verbatim");
        let mut argv = vec!["--metrics-view".to_owned()];
        argv.extend(options.iter().map(|s| (*s).to_owned()));
        argv.extend([
            "--".to_owned(),
            lang.to_owned(),
            path.to_str().expect("UTF-8").to_owned(),
        ]);
        let mut out = Vec::new();
        main_with(&argv, &mut out).expect("metrics view");
        let text = String::from_utf8(out).expect("UTF-8");
        assert_eq!(text.lines().count(), 1, "exactly one input record");
        assert!(text.ends_with('\n'));
        let value = parse_line(&text);
        assert_eq!(value["path"], path.to_str().expect("UTF-8"));
        assert_eq!(value["lang"], lang);
        assert_eq!(value["source_bytes"], source.len(), "parsed buffer length");
        value
    }

    #[test]
    fn metrics_view_defaults_preserve_metrics_and_raw_endings() {
        for source in [
            b"fn f() {}\r\n/// tail".as_slice(),
            b"fn f() {}\n/// tail",
            b"fn f() {}\n/// tail\n",
            b"fn f() {}\r\n/// tail\r\n",
        ] {
            let view = metric_view("rust", source, &[]);
            let default = record(LANG::Rust, view["path"].as_str().expect("path"), source)
                .expect("default record");
            assert_eq!(view["metrics"], default["metrics"]);
            assert_eq!(
                view["options"],
                serde_json::json!({
                    "exclude_tests": false,
                    "count_cyclomatic_try": true,
                    "mi_charges_docs": false,
                    "metrics": "all",
                })
            );
            assert_eq!(view.as_object().expect("record").len(), 5);
            assert_eq!(default.as_object().expect("record").len(), 12);
            assert_eq!(view["metrics"]["metrics"]["loc"]["sloc"], 2);
            assert_eq!(view["metrics"]["spaces"].as_array().map(Vec::len), Some(1));
        }
    }

    #[test]
    fn metrics_view_mi_modes_and_dependency_closure_reach_each_space() {
        // The existing MI fixture's independent inputs: N=25, n=12,
        // ploc=6, cloc=dloc=5, sloc=11; G=4 at root, 3 in clamp.
        let source = b"def clamp(value, low, high):\n    \"\"\"Return ``value`` held within ``[low, high]``.\n\n    The bounds are inclusive and are not checked against each\n    other, so ``low > high`` returns ``high`` for every input.\n    \"\"\"\n    if value < low:\n        return low\n    if value > high:\n        return high\n    return value\n";
        for selection in [vec![], vec!["--metrics", "mi"]] {
            for (mode, charged) in [
                ("--mi-charges-docs=false", false),
                ("--mi-charges-docs=true", true),
            ] {
                let mut options = selection.clone();
                options.push(mode);
                let view = metric_view("python", source, &options);
                assert_eq!(view["options"]["mi_charges_docs"], charged);
                assert_eq!(
                    view["options"]["metrics"],
                    if selection.is_empty() { "all" } else { "mi" }
                );
                let root = &view["metrics"];
                let children = root["spaces"].as_array().expect("spaces");
                assert_eq!(children.len(), 1);
                assert_eq!(children[0]["name"], "clamp");
                for (space, g, original, sei) in [
                    (root, 4, 117.676_252_636_666_53, 160.909_070_973_313_36),
                    (
                        &children[0],
                        3,
                        117.906_252_636_666_52,
                        161.139_070_973_313_32,
                    ),
                ] {
                    let m = &space["metrics"];
                    for (key, expected) in [("ploc", 6), ("cloc", 5), ("dloc", 5), ("sloc", 11)] {
                        assert_eq!(m["loc"][key], expected, "{key}: {m}");
                    }
                    assert_eq!(m["cyclomatic"]["sum"], g);
                    assert_eq!(m["halstead"]["length"], 25);
                    assert_eq!(m["halstead"]["vocabulary"], 12);
                    assert!((m["mi"]["original"].as_f64().expect("MI") - original).abs() < 1e-9);
                    let expected_sei = if charged { sei } else { original };
                    assert!((m["mi"]["sei"].as_f64().expect("MI") - expected_sei).abs() < 1e-9);
                    if !selection.is_empty() {
                        assert_eq!(
                            m.as_object()
                                .expect("metrics")
                                .keys()
                                .map(String::as_str)
                                .collect::<Vec<_>>(),
                            ["cyclomatic", "halstead", "loc", "mi"]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn metrics_view_combines_pruning_try_count_and_mi_modes() {
        let source = b"/// kept\r\nfn kept() { work()?; }\r\n/// pruned\r\n#[test]\r\nfn removed() { work()?; }";
        for exclude in [false, true] {
            for count_try in [false, true] {
                for charged in [false, true] {
                    let mut options = vec![
                        "--metrics",
                        "mi",
                        if count_try {
                            "--cyclomatic-count-try=true"
                        } else {
                            "--cyclomatic-count-try=false"
                        },
                        if charged {
                            "--mi-charges-docs=true"
                        } else {
                            "--mi-charges-docs=false"
                        },
                    ];
                    if exclude {
                        options.push("--exclude-tests");
                    }
                    let view = metric_view("rust", source, &options);
                    assert_eq!(
                        view["options"],
                        serde_json::json!({
                            "metrics": "mi", "exclude_tests": exclude,
                            "count_cyclomatic_try": count_try, "mi_charges_docs": charged,
                        })
                    );
                    let root = &view["metrics"];
                    let children = root["spaces"].as_array().expect("spaces");
                    let names: Vec<_> = children
                        .iter()
                        .map(|s| s["name"].as_str().expect("name"))
                        .collect();
                    assert_eq!(
                        names,
                        if exclude {
                            vec!["kept"]
                        } else {
                            vec!["kept", "removed"]
                        }
                    );
                    assert_eq!(root["metrics"]["loc"]["dloc"], if exclude { 1 } else { 2 });
                    let child_cyclomatic = if count_try { 2 } else { 1 };
                    assert_eq!(
                        root["metrics"]["cyclomatic"]["sum"],
                        1 + names.len() * child_cyclomatic
                    );
                    for child in children {
                        assert_eq!(child["metrics"]["cyclomatic"]["sum"], child_cyclomatic);
                        assert_eq!(child["metrics"]["loc"]["dloc"], 0);
                    }
                    let mi = &root["metrics"]["mi"];
                    let original = mi["original"].as_f64().expect("MI");
                    assert!(original > 0.0, "real code, not the empty sentinel");
                    if charged {
                        assert!(mi["sei"].as_f64().expect("MI") > original);
                    } else {
                        assert_eq!(mi["sei"], mi["original"]);
                    }
                }
            }
        }
    }

    #[test]
    fn metrics_view_rejects_ambiguous_or_invalid_options_before_reading() {
        for options in [
            vec!["--mi-charges-docs"],
            vec!["--mi-charges-docs=yes"],
            vec!["--mi-charges-docs=false", "--mi-charges-docs=true"],
            vec!["--cyclomatic-count-try=0"],
            vec!["--cyclomatic-count-try=true", "--cyclomatic-count-try=true"],
            vec!["--exclude-tests=false"],
            vec!["--exclude-tests="],
            vec!["--exclude-tests", "--exclude-tests"],
            vec!["--metrics"],
            vec!["--metrics", "unknown"],
            vec!["--metrics", "mi,loc"],
            vec!["--metrics", "mi", "--metrics", "loc"],
            vec!["--unknown"],
        ] {
            let mut args = vec!["--metrics-view".to_owned()];
            args.extend(options.iter().map(|s| (*s).to_owned()));
            args.extend([
                "--".to_owned(),
                "rust".to_owned(),
                "never-read.rs".to_owned(),
            ]);
            let mut out = Vec::new();
            let err = main_with(&args, &mut out).expect_err("invalid options");
            assert!(
                !err.contains("never-read.rs"),
                "validated before I/O: {err}"
            );
            assert!(out.is_empty());
        }
    }

    #[test]
    fn metrics_view_and_run_reject_zero_inputs() {
        for args in [
            vec!["--metrics-view", "rust", "file.rs"],
            vec!["--metrics-view", "--"],
            vec!["--metrics-view", "--", "rust"],
        ] {
            let args: Vec<_> = args.into_iter().map(str::to_owned).collect();
            let mut out = Vec::new();
            assert!(
                main_with(&args, &mut out)
                    .expect_err("incomplete invocation")
                    .contains("usage: verbatim-oracle")
            );
            assert!(out.is_empty());
        }
        let mut out = Vec::new();
        assert!(
            run("rust", std::iter::empty::<&str>(), &mut out)
                .expect_err("zero files")
                .starts_with("no input files")
        );
        assert!(out.is_empty());
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
