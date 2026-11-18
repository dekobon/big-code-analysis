//! Legacy preservation on raw inputs; source owner/text requirements live in AST fixtures.

use big_code_analysis_ast::test_support::documentation;
use serde_json::{Value, json};

use crate::{LANG, Metric, MetricsOptions, Source, analyze};

#[cfg(all(feature = "java", feature = "rust"))]
#[test]
fn documentation_marker_stream_preserves_rows_without_source_materialization() {
    use big_code_analysis_ast::documentation::streaming::markers::{
        evidence_built, stretches_collected,
    };
    for (lang, source, excluded, rows, scans) in [
        (
            LANG::Java,
            "/** a\n * b\n * c */\n/**/\nclass C {}\n",
            false,
            vec![0, 1, 2],
            1,
        ),
        (
            LANG::Java,
            "class C { void f() { /**a*/\n/**b*/\nclass N {} } }",
            false,
            vec![],
            0,
        ),
        (LANG::Java, "/**a*/\n/**b*/", false, vec![], 0),
        (LANG::Rust, "/*!*/\nfn f() {}\n", false, vec![0], 1),
        (
            LANG::Rust,
            "/// outer\n#[test]\nfn test_f() {}\n\n/// kept\nfn f() {}\n",
            true,
            vec![4],
            1,
        ),
    ] {
        let built = evidence_built::observed();
        let collected = stretches_collected::observed();
        let space = analyze(
            Source::new(lang, source.as_bytes()),
            MetricsOptions::default().with_exclude_tests(excluded),
        )
        .unwrap();
        assert_eq!(
            (0..source.len())
                .filter(|row| space.metrics.loc.dloc.lines.contains(*row))
                .collect::<Vec<_>>(),
            rows,
            "{source}"
        );
        assert_eq!(evidence_built::observed() - built, 0);
        assert_eq!(stretches_collected::observed() - collected, scans);
    }
}

#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc",
    feature = "csharp",
    feature = "rust",
    feature = "java",
    feature = "javascript",
    feature = "mozjs",
    feature = "python",
    feature = "php",
    feature = "perl",
    feature = "groovy",
    feature = "tcl",
    feature = "irules"
))]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_legacy_whole_metrics() {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/reference/manifest.json"
    ))
    .expect("capture manifest");
    let selection: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/selection.json"
    ))
    .expect("selected keys");
    let keys = selection.as_array().expect("keys");
    assert_eq!(keys.len(), 39);
    assert_eq!(
        keys.iter()
            .map(|key| key.as_str().expect("key"))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        keys.len(),
        "each selected language/input key is unique"
    );
    let expected_runs = keys
        .iter()
        .filter(|key| {
            key.as_str()
                .expect("key")
                .split('/')
                .next()
                .expect("language")
                .parse::<LANG>()
                .expect("known LANG")
                .is_enabled()
        })
        .count()
        * 10;
    assert_eq!(manifest["keys"], selection);
    assert_eq!(manifest["complete"], true);
    assert_eq!(
        manifest["views"],
        json!([
            "verbatim_metrics_mi_charges_docs_false",
            "verbatim_metrics_mi_charges_docs_true",
            "verbatim_metrics_exclude_tests_mi_charges_docs_false",
            "verbatim_metrics_exclude_tests_mi_charges_docs_true",
            "verbatim_metrics_only_mi_mi_charges_docs_false",
            "verbatim_metrics_only_mi_mi_charges_docs_true",
            "verbatim_metrics_only_mi_exclude_tests_mi_charges_docs_false",
            "verbatim_metrics_only_mi_exclude_tests_mi_charges_docs_true",
            "verbatim_metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_false",
            "verbatim_metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_true"
        ])
    );
    let anchors: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/legacy.json"
    ))
    .expect("independent anchors");
    let selected_files: std::collections::BTreeSet<_> = selection
        .as_array()
        .expect("keys")
        .iter()
        .map(|k| k.as_str().expect("key").rsplit('/').next().expect("file"))
        .collect();
    assert_eq!(
        selected_files,
        anchors
            .as_object()
            .expect("anchors")
            .keys()
            .map(String::as_str)
            .collect()
    );
    let mut ran = 0;
    for key in selection.as_array().expect("keys") {
        let key = key.as_str().expect("key");
        let mut parts = key.split('/');
        let lang: LANG = parts.next().expect("language").parse().expect("known LANG");
        assert_eq!(parts.next(), Some("documentation"));
        let file = parts.next().expect("file");
        assert!(parts.next().is_none());
        if !lang.is_enabled() {
            continue;
        }
        let source = documentation::source(file);
        for view in manifest["views"].as_array().expect("views") {
            let view = view.as_str().expect("view");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/documentation/reference")
                .join(format!("{key}.{view}.txt"));
            let expected: Value =
                serde_json::from_slice(&std::fs::read(path).expect("checked capture"))
                    .expect("wire JSON");
            let opt = &expected["options"];
            assert_eq!(
                opt,
                &json!({
                    "mi_charges_docs": view.ends_with("_true"),
                    "exclude_tests": view.contains("exclude_tests"),
                    "count_cyclomatic_try": !view.contains("no_count_try"),
                    "metrics": if view.contains("only_mi") { "mi" } else { "all" },
                }),
                "{key}/{view}: options must match the named view"
            );
            let excluded = opt["exclude_tests"].as_bool().expect("exclude_tests");
            let mut options = MetricsOptions::default()
                .with_exclude_tests(excluded)
                .with_count_cyclomatic_try(opt["count_cyclomatic_try"].as_bool().expect("try"))
                .with_mi_charges_docs(opt["mi_charges_docs"].as_bool().expect("MI mode"));
            match opt["metrics"].as_str().expect("selection") {
                "all" => {}
                "mi" => options = options.with_only(&[Metric::Mi]),
                other => panic!("unexpected selection {other}"),
            }
            let name = format!("tests/fixtures/documentation/sources/{file}");
            let space = analyze(
                Source::new(lang, &source).with_name(Some(name.clone())),
                options,
            )
            .expect("legacy analysis");
            let observed = json!({
                "path": name, "lang": lang.to_string(), "source_bytes": source.len(),
                "options": opt, "metrics": space.to_wire(),
            });
            assert_eq!(observed, expected, "{key}/{view}");
            let anchor = anchors[file]
                .get(if excluded { "excluded" } else { "included" })
                .unwrap_or(&anchors[file]["included"]);
            assert_eq!(
                json!([
                    space.metrics.loc.sloc(),
                    space.metrics.loc.ploc(),
                    space.metrics.loc.lloc(),
                    space.metrics.loc.cloc(),
                    space.metrics.loc.blank(),
                    space.metrics.loc.dloc()
                ]),
                anchor["loc"],
                "{key}/{view}"
            );
            let loc = &space.metrics.loc;
            let rows: Vec<_> = (0..source.len())
                .filter(|row| loc.dloc.lines.contains(*row))
                .collect();
            assert_eq!(json!(rows), anchor["dloc_rows"], "{key}/{view}");
            assert_eq!(rows.len() as u64, loc.dloc());
            assert_optional_rows(loc, source.len(), anchor);
            let mut nested = Vec::new();
            let mut stack: Vec<_> = space.spaces.iter().rev().collect();
            while let Some(child) = stack.pop() {
                let rows: Vec<_> = (0..source.len())
                    .filter(|row| child.metrics.loc.dloc.lines.contains(*row))
                    .collect();
                assert_eq!(rows.len() as u64, child.metrics.loc.dloc());
                nested.push(json!([
                    child.name,
                    child.kind,
                    child.start_line,
                    child.end_line,
                    rows
                ]));
                stack.extend(child.spaces.iter().rev());
            }
            assert_eq!(
                json!(nested),
                anchor["spaces"],
                "{key}/{view}: exact space/row population"
            );
            if opt["metrics"] == "mi" {
                assert_eq!(
                    observed["metrics"]["metrics"]
                        .as_object()
                        .expect("metrics")
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    ["cyclomatic", "halstead", "loc", "mi"]
                );
            }
            if file == "python-runs.py" || file == "rust-prune.rs" {
                let mi = &observed["metrics"]["metrics"]["mi"];
                assert!(mi["original"].as_f64().expect("MI") > 0.0);
                assert_ne!(observed["metrics"]["metrics"]["halstead"]["length"], 0);
                if opt["mi_charges_docs"] == true || file == "rust-prune.rs" {
                    assert!(
                        mi["sei"].as_f64().expect("SEI")
                            > mi["original"].as_f64().expect("original")
                    );
                } else {
                    assert_eq!(mi["sei"], mi["original"]);
                }
            }
            ran += 1;
        }
    }
    assert_eq!(ran, expected_runs, "every enabled key and view was checked");
    assert!(ran > 0);
}

fn assert_optional_rows(loc: &super::Stats, source_len: usize, anchor: &Value) {
    if let Some(expected) = anchor.get("ploc_rows") {
        let rows: Vec<_> = (0..source_len)
            .filter(|row| loc.ploc.lines.contains(*row))
            .collect();
        assert_eq!(json!(rows), *expected, "exact code rows");
    }
    if let Some(expected) = anchor.get("cloc_rows") {
        let rows: Vec<_> = (0..source_len)
            .filter(|row| {
                loc.cloc.only_comment_line_starts.contains(*row)
                    || loc.cloc.code_comment_line_starts.contains(*row)
            })
            .collect();
        assert_eq!(json!(rows), *expected, "exact comment rows");
    }
}

#[cfg(feature = "tcl")]
#[test]
fn documentation_optional_rows_follow_anchors_not_filenames() {
    let source = documentation::source("tcl-crlf.tcl");
    let space = analyze(
        Source::new(LANG::Tcl, &source).with_name(Some("renamed.tcl".into())),
        MetricsOptions::default(),
    )
    .expect("renamed fixture");
    assert_optional_rows(
        &space.metrics.loc,
        source.len(),
        &json!({"ploc_rows":[1,3],"cloc_rows":[0]}),
    );
    let source = b"proc unrelated {} {}\n";
    let space = analyze(
        Source::new(LANG::Tcl, source).with_name(Some("unrelated-crlf.tcl".into())),
        MetricsOptions::default(),
    )
    .expect("unrelated fixture");
    assert_optional_rows(&space.metrics.loc, source.len(), &json!({}));
    assert_optional_rows(
        &space.metrics.loc,
        source.len(),
        &json!({"ploc_rows":[0],"cloc_rows":[]}),
    );
    assert!(
        std::panic::catch_unwind(|| assert_optional_rows(
            &space.metrics.loc,
            source.len(),
            &json!({"ploc_rows":[1,3],"cloc_rows":[0]})
        ))
        .is_err()
    );
}
