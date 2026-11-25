//! Legacy preservation on raw inputs; source owner/text requirements live in AST fixtures.

use big_code_analysis_ast::test_support::documentation;
use serde_json::{Value, json};

use crate::{LANG, Metric, MetricsOptions, Source, analyze};

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
#[derive(Clone, Copy)]
struct PositionalRows<'a> {
    loc: [u64; 6],
    docs: &'a [usize],
    code: &'a [usize],
    comments: &'a [usize],
    scans: usize,
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
#[allow(clippy::arithmetic_side_effects)]
fn positional_rows(lang: LANG, source: &[u8], expected: PositionalRows<'_>) {
    use big_code_analysis_ast::documentation::streaming::positional::{
        evidence_built, members_materialized, positional_runs_collected,
    };
    let before = evidence_built::observed();
    let materialized = members_materialized::observed();
    let scans = positional_runs_collected::observed();
    let intersections = big_code_analysis_ast::documentation::included_range_visits::observed();
    let space = analyze(
        Source::new(lang, source),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .unwrap();
    let loc = &space.metrics.loc;
    assert_eq!(
        [
            loc.sloc(),
            loc.ploc(),
            loc.lloc(),
            loc.cloc(),
            loc.blank(),
            loc.dloc()
        ],
        expected.loc
    );
    assert_eq!(
        (0..source.len())
            .filter(|row| loc.dloc.lines.contains(*row))
            .collect::<Vec<_>>(),
        expected.docs
    );
    assert_optional_rows(
        loc,
        source.len(),
        &json!({"ploc_rows": expected.code, "cloc_rows": expected.comments}),
    );
    assert_eq!(evidence_built::observed() - before, 0);
    assert_eq!(members_materialized::observed() - materialized, 0);
    assert_eq!(
        big_code_analysis_ast::documentation::included_range_visits::observed() - intersections,
        0
    );
    assert_eq!(
        positional_runs_collected::observed() - scans,
        expected.scans
    );
    assert_eq!(space.spaces.len(), 1);
    assert_eq!(space.spaces[0].metrics.loc.dloc(), 0);
}

#[cfg(feature = "go")]
#[test]
fn documentation_go_positional_exact_rows() {
    // Eight physical rows: package/function code, five comment rows, one source blank.
    positional_rows(
        LANG::Go,
        b"package p\n// a\n//go:generate tool\n//\n/* b */ /* c */\n\n// d\nfunc F() {}",
        PositionalRows {
            loc: [8, 2, 0, 5, 1, 1],
            docs: &[6],
            code: &[0, 7],
            comments: &[1, 2, 3, 4, 6],
            scans: 2,
        },
    );
    positional_rows(
        LANG::Go,
        b"package p\n// a\n//go:generate tool\n//\n/* b */ /* c */\nfunc F() {}",
        PositionalRows {
            loc: [6, 2, 0, 4, 0, 3],
            docs: &[1, 3, 4],
            code: &[0, 5],
            comments: &[1, 2, 3, 4],
            scans: 1,
        },
    );
}

#[cfg(feature = "ruby")]
#[test]
fn documentation_ruby_positional_exact_rows() {
    positional_rows(
        LANG::Ruby,
        b"#!/ruby\n# frozen_string_literal: true\n# a\n#\n# b\ndef f; end",
        PositionalRows {
            loc: [6, 1, 1, 5, 0, 3],
            docs: &[2, 3, 4],
            code: &[5],
            comments: &[0, 1, 2, 3, 4],
            scans: 1,
        },
    );
    positional_rows(
        LANG::Ruby,
        b"# a\n\n# b\ndef f; end",
        PositionalRows {
            loc: [4, 1, 1, 2, 1, 1],
            docs: &[2],
            code: &[3],
            comments: &[0, 2],
            scans: 2,
        },
    );
}

#[cfg(feature = "ruby")]
#[test]
fn documentation_ruby_block_legacy_rows_remain_in_physical_space() {
    let source = b"foo do\n# audit\ndef f; end\nend";
    let space = analyze(
        Source::new(LANG::Ruby, source),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .unwrap();
    assert_eq!(
        [
            space.metrics.loc.sloc(),
            space.metrics.loc.ploc(),
            space.metrics.loc.lloc(),
            space.metrics.loc.cloc(),
            space.metrics.loc.blank(),
            space.metrics.loc.dloc()
        ],
        [4, 3, 1, 1, 0, 1]
    );
    assert_eq!(space.spaces.len(), 1);
    let block = &space.spaces[0];
    assert_eq!(block.metrics.loc.dloc(), 1);
    assert!(block.metrics.loc.dloc.lines.contains(1));
    assert_eq!(block.spaces.len(), 1);
    assert_eq!(block.spaces[0].metrics.loc.dloc(), 0);
}

#[cfg(feature = "lua")]
#[test]
fn documentation_lua_positional_exact_rows() {
    positional_rows(
        LANG::Lua,
        b"module(\"m\")\n---a\n--\n-- b\nfunction f() end",
        PositionalRows {
            loc: [5, 2, 1, 3, 0, 3],
            docs: &[1, 2, 3],
            code: &[0, 4],
            comments: &[1, 2, 3],
            scans: 1,
        },
    );
    positional_rows(
        LANG::Lua,
        b"module(\"m\")\n-- a\nfunction f() end",
        PositionalRows {
            loc: [3, 2, 1, 1, 0, 0],
            docs: &[],
            code: &[0, 2],
            comments: &[1],
            scans: 1,
        },
    );
}

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

#[cfg(feature = "csharp")]
#[test]
fn documentation_csharp_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    let fixture = documentation::source("csharp-primary.cs");
    let sources: [(&[u8], [u64; 6], &[usize]); 9] = [
        (
            b"/// d\n#if FLAG\nclass C { }\n#endif\n",
            [4, 3, 0, 1, 0, 0],
            &[],
        ),
        (
            b"#if FLAG\n/// d\nclass G { }\n#endif\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"/// x\n#if FLAG\n#endif\nclass H { }\n",
            [4, 3, 0, 1, 0, 0],
            &[],
        ),
        (
            b"/// d\n#if NET5_0\n[System.Obsolete]\n#endif\npublic class C { }\n",
            [5, 4, 0, 1, 0, 1],
            &[0],
        ),
        (
            b"class C {\n    /// ab\n    int a, b;\n}\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"class C {\n    /// d\n#if A\n    void F() { }\n#else\n    void G() { }\n#endif\n}\n",
            [8, 7, 0, 1, 0, 0],
            &[],
        ),
        (b"///a\n///b\nclass C {}\n", [3, 1, 0, 2, 0, 2], &[0, 1]),
        (
            b"///<summary>audit</summary>\n[System.Obsolete] class C {}\n",
            [2, 1, 0, 1, 0, 1],
            &[0],
        ),
        (&fixture, [2, 1, 1, 1, 0, 1], &[0]),
    ];
    for (source, tuple, rows) in sources {
        let name = source.escape_ascii();
        let parser =
            crate::langs::AnyParser::parse(LANG::Csharp, source.to_vec(), None, None).unwrap();
        let idle = observations();
        let unbuilt = evidence_built::observed();
        parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let before = observations();
        let built = evidence_built::observed();
        // The collection moves each counter on this thread, so a zero across `analyze` is
        // a reading.
        assert_ne!(before.visits - idle.visits, 0, "{name}");
        assert_ne!(before.renders - idle.renders, 0, "{name}");
        assert_ne!(built - unbuilt, 0, "{name}");
        let space = analyze(
            Source::new(LANG::Csharp, source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let after = observations();
        assert_eq!(evidence_built::observed() - built, 0, "{name}");
        assert_eq!(after.renders - before.renders, 0, "{name}");
        assert_eq!(after.visits - before.visits, 0, "{name}");
        let loc = &space.metrics.loc;
        assert_eq!(
            [
                loc.sloc(),
                loc.ploc(),
                loc.lloc(),
                loc.cloc(),
                loc.blank(),
                loc.dloc()
            ],
            tuple,
            "{name}"
        );
        assert_eq!(
            (0..source.len())
                .filter(|row| loc.dloc.lines.contains(*row))
                .collect::<Vec<_>>(),
            rows,
            "{name}"
        );
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_go_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::positional::{
        evidence_built, members_materialized, positional_runs_collected,
    };
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, included_range_visits, observations,
    };
    struct Row<'a> {
        source: &'a [u8],
        loc: [u64; 6],
        dloc_rows: &'a [usize],
        // A run is a maximal sequence of comments in which each one starts on the row
        // where the one before it ends, or on the next row.
        comment_runs: usize,
    }
    let counters = || {
        [
            observations().visits,
            observations().renders,
            evidence_built::observed(),
            members_materialized::observed(),
            positional_runs_collected::observed(),
            included_range_visits::observed(),
        ]
    };
    let fixture = documentation::source("primary.go");
    let sources = [
        Row {
            source: b"package p\ntype S struct {\n// both\nX, Y int\n}\n",
            loc: [5, 4, 0, 1, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
        // Code separates the three comments.
        Row {
            source: b"package p\ntype I interface {\n// m\nM()\n// e\nio.Reader\n// u\n~int\n}\n",
            loc: [9, 6, 0, 3, 0, 2],
            dloc_rows: &[2, 4],
            comment_runs: 3,
        },
        // `var (` separates the two comments.
        Row {
            source: b"package p\n// group\nvar (\n// member\nX int\n)\n",
            loc: [6, 4, 1, 2, 0, 2],
            dloc_rows: &[1, 3],
            comment_runs: 2,
        },
        Row {
            source: b"package p\n// audit\n//go:generate tool\nfunc F() {}\n",
            loc: [4, 2, 0, 2, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        Row {
            source: b"package p\ntype S struct {\n//go:notinheap\n// audit\nX int\n}\n",
            loc: [6, 4, 0, 2, 0, 1],
            dloc_rows: &[3],
            comment_runs: 1,
        },
        // Three comments on rows 1 to 3: the second starts where the first ends.
        Row {
            source: b"package p\nvar y = 0 /* t1\nt2 */ /* t3 */\n// doc\nvar z = 1\n",
            loc: [5, 3, 2, 3, 0, 1],
            dloc_rows: &[3],
            comment_runs: 1,
        },
        // A blank row separates the first comment from the other three.
        Row {
            source: b"package p\n// a\n\n// b\n//\n// c\nfunc F() {}\n",
            loc: [7, 2, 0, 4, 1, 3],
            dloc_rows: &[3, 4, 5],
            comment_runs: 2,
        },
        // `N struct {` separates the two comments.
        Row {
            source: b"package p\ntype S struct {\n// same\nN struct {\n// same\nX, Y int\n}\n}\n",
            loc: [8, 6, 0, 2, 0, 2],
            dloc_rows: &[2, 4],
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype S struct { // t\n// d\nX int\n}\n",
            loc: [5, 4, 0, 2, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
        Row {
            source: b"package p\nfunc F() {\nx := 1\n// local\nvar y = x\n_ = y\n}\n",
            loc: [7, 6, 3, 1, 0, 0],
            dloc_rows: &[],
            comment_runs: 1,
        },
        Row {
            source: b"package p\n// not doc\nimport \"fmt\"\n",
            loc: [3, 2, 0, 1, 0, 0],
            dloc_rows: &[],
            comment_runs: 1,
        },
        Row {
            source: &fixture,
            loc: [3, 2, 0, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
    ];
    for row in sources {
        let name = row.source.escape_ascii();
        let parser =
            crate::langs::AnyParser::parse(LANG::Go, row.source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error(), "{name}");
        let idle = counters();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = counters();
        let space = analyze(
            Source::new(LANG::Go, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = counters();

        let loc = &space.metrics.loc;
        assert_eq!(
            [
                loc.sloc(),
                loc.ploc(),
                loc.lloc(),
                loc.cloc(),
                loc.blank(),
                loc.dloc()
            ],
            row.loc,
            "{name}"
        );
        assert_eq!(
            (0..row.source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            row.dloc_rows,
            "{name}"
        );

        // The collection moves each counter on this thread, so a zero across `analyze` is
        // a reading.
        let [visits, renders, built, materialized, scanned, covered] =
            std::array::from_fn(|at| collected[at] - idle[at]);
        // expected: the walk enters every node of the tree once.
        assert_eq!(visits, parser.root_node().preorder().count(), "{name}");
        // expected: one render per published document.
        assert_eq!(renders, index.documents().len(), "{name}");
        // expected: under default options the positional stream builds one member for
        // each fragment of a selected run, and none for a directive or a trailing member.
        let fragments: usize = index
            .documents()
            .map(|document| document.get().fragment_count())
            .sum();
        assert_eq!(built, fragments, "{name}");
        // expected: every comment is a member of exactly one run, and the source view
        // materializes each run once.
        let comments = parser
            .root_node()
            .preorder()
            .filter(|node| node.kind() == "comment")
            .count();
        assert_eq!(materialized, comments, "{name}");
        // expected: the source view scans every run once, at its last comment.
        assert_eq!(scanned, row.comment_runs, "{name}");
        // Not exact: the number of included-range questions is the collector's own
        // accounting and follows from no fact of the tree.
        assert_ne!(covered, 0, "{name}");
        // A source without a document moves neither `renders` nor `evidence_built`.
        // expected: in these sources a document is published exactly where the legacy
        // view counts a row.
        assert_eq!(
            index.documents().len() == 0,
            row.dloc_rows.is_empty(),
            "{name}"
        );

        // The same six counters across `analyze`.
        let [visits, renders, built, materialized, scanned, covered] =
            std::array::from_fn(|at| analyzed[at] - collected[at]);
        // expected: 0 and 0, because `analyze` builds no documentation index.
        assert_eq!(visits, 0, "{name}");
        assert_eq!(renders, 0, "{name}");
        // expected: 0, because the legacy view builds no source evidence.
        assert_eq!(built, 0, "{name}");
        // expected: 0, because the legacy view walks members without a buffer.
        assert_eq!(materialized, 0, "{name}");
        // expected: the legacy view scans every run once as well, at its last comment.
        assert_eq!(scanned, row.comment_runs, "{name}");
        // expected: 0, because the legacy view has no source context to ask.
        assert_eq!(covered, 0, "{name}");
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_c_family_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    /// One source: the variants it runs in, its bytes, its LOC tuple and its DLOC rows.
    type Case<'a> = (&'a [LANG], &'a [u8], [u64; 6], &'a [usize]);
    const ALL: &[LANG] = &[LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc];
    const CXX: &[LANG] = &[LANG::Cpp, LANG::Mozcpp];
    let primary_c = documentation::source("primary.c");
    let primary_cpp = documentation::source("primary.cpp");
    let primary_m = documentation::source("primary.m");
    let sources: [Case<'_>; 15] = [
        (ALL, b"/**audit*/\nint a, b;\n", [2, 1, 1, 1, 0, 1], &[0]),
        (
            ALL,
            b"#ifndef G_H\n#define G_H\n/** d */\nint f(void);\n#endif\n",
            [5, 4, 1, 1, 0, 1],
            &[2],
        ),
        (
            ALL,
            b"/** d */\n#if A\nvoid f(void);\n#else\nvoid g(void);\n#endif\n",
            [6, 5, 2, 1, 0, 1],
            &[0],
        ),
        (
            ALL,
            b"enum E {\n  A, /**< ta */\n  B  /**< tb */\n};\n",
            [4, 4, 0, 2, 0, 2],
            &[1, 2],
        ),
        (
            ALL,
            b"/** d */\nextern \"C\" {\nint f(void);\n}\n",
            [4, 3, 1, 1, 0, 1],
            &[0],
        ),
        (
            CXX,
            b"/** d */\ntemplate <class T> class X {};\n",
            [2, 1, 0, 1, 0, 1],
            &[0],
        ),
        (
            ALL,
            b"int f(void) {\n  /** note */\n  return 0;\n}\n",
            [4, 3, 1, 1, 0, 1],
            &[1],
        ),
        (ALL, b"int a, /**< ta */ b;\n", [1, 1, 1, 1, 0, 1], &[0]),
        (ALL, b"/** @file */\nint x;\n", [2, 1, 1, 1, 0, 1], &[0]),
        (
            ALL,
            b"struct S { int x; ///<audit\n};\n",
            [2, 2, 0, 1, 0, 1],
            &[0],
        ),
        (
            ALL,
            b"/**@fn void f(void)\n * audit*/\nvoid f(void);\n",
            [3, 1, 1, 2, 0, 2],
            &[0, 1],
        ),
        (
            &[LANG::Objc],
            b"@interface C\n/**audit*/\n- (void)f;\n@end\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (&[LANG::C], &primary_c, [2, 1, 0, 1, 0, 1], &[0]),
        (CXX, &primary_cpp, [2, 1, 0, 1, 0, 1], &[0]),
        (&[LANG::Objc], &primary_m, [2, 1, 0, 1, 0, 1], &[0]),
    ];
    let mut ran = 0;
    for (langs, source, tuple, rows) in sources {
        for &lang in langs {
            if !lang.is_enabled() {
                continue;
            }
            ran += 1;
            let name = format!("{lang:?}: {}", source.escape_ascii());
            let parser = crate::langs::AnyParser::parse(lang, source.to_vec(), None, None).unwrap();
            let root = parser.ts_tree().root_node();
            assert!(!root.has_error(), "{name}");
            let mut nodes = 0;
            let mut unvisited = vec![root];
            while let Some(node) = unvisited.pop() {
                nodes += 1;
                unvisited.extend(node.children(&mut node.walk()));
            }
            let idle = observations();
            let unbuilt = evidence_built::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let before = observations();
            let built = evidence_built::observed();
            let (visited, rendered, members) = (
                before.visits - idle.visits,
                before.renders - idle.renders,
                built - unbuilt,
            );
            // expected: the collector's walk enters every node of the tree once, named or
            // not, the root included.
            assert_eq!(visited, nodes, "{name}");
            // expected: one text render per document.
            assert_eq!(rendered, index.documents().len(), "{name}");
            // expected: one built member per marker comment that a run selects. Under the
            // default options nothing else is built, so these are the fragments of the
            // documents.
            assert_eq!(
                members,
                index
                    .documents()
                    .map(|document| document.get().fragment_count())
                    .sum::<usize>(),
                "{name}"
            );
            // The collection moves each counter on this thread, so a zero across `analyze`
            // is a reading.
            assert_ne!(visited, 0, "{name}");
            assert_ne!(rendered, 0, "{name}");
            assert_ne!(members, 0, "{name}");
            let space = analyze(
                Source::new(lang, source),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            let after = observations();
            // expected: 0 for each. `analyze` takes its documentation rows from the legacy
            // scan and does not run the collector.
            assert_eq!(after.visits - before.visits, 0, "{name}");
            assert_eq!(after.renders - before.renders, 0, "{name}");
            assert_eq!(evidence_built::observed() - built, 0, "{name}");
            let loc = &space.metrics.loc;
            assert_eq!(
                [
                    loc.sloc(),
                    loc.ploc(),
                    loc.lloc(),
                    loc.cloc(),
                    loc.blank(),
                    loc.dloc()
                ],
                tuple,
                "{name}"
            );
            assert_eq!(
                (0..source.len())
                    .filter(|row| loc.dloc.lines.contains(*row))
                    .collect::<Vec<_>>(),
                rows,
                "{name}"
            );
        }
    }
    assert!(ran > 0);
}

#[cfg(feature = "lua")]
struct LuaLegacyRow<'a> {
    name: &'static str,
    source: &'a [u8],
    loc: [u64; 6],
    dloc_rows: &'a [usize],
    // A run is a maximal sequence of comments whose next comment starts on the same row
    // where the previous one ends, or on the next row.
    comment_runs: usize,
}

#[cfg(feature = "lua")]
fn lua_documentation_counters() -> [usize; 6] {
    use big_code_analysis_ast::documentation::streaming::positional::{
        evidence_built, members_materialized, positional_runs_collected,
    };
    use big_code_analysis_ast::documentation::{included_range_visits, observations};
    [
        observations().visits,
        observations().renders,
        evidence_built::observed(),
        members_materialized::observed(),
        positional_runs_collected::observed(),
        included_range_visits::observed(),
    ]
}

#[cfg(feature = "lua")]
/// Checks one row and returns the number of documents its collection published.
#[allow(clippy::arithmetic_side_effects)]
fn check_lua_legacy_row(row: &LuaLegacyRow<'_>) -> usize {
    use big_code_analysis_ast::documentation::{DocumentationContext, DocumentationOptions};
    let name = row.name;
    let parser =
        crate::langs::AnyParser::parse(LANG::Lua, row.source.to_vec(), None, None).unwrap();
    assert!(!parser.ts_tree().root_node().has_error(), "{name}");
    let idle = lua_documentation_counters();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let collected = lua_documentation_counters();
    let space = analyze(
        Source::new(LANG::Lua, row.source),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .unwrap();
    let analyzed = lua_documentation_counters();
    let loc = &space.metrics.loc;
    assert_eq!(
        [
            loc.sloc(),
            loc.ploc(),
            loc.lloc(),
            loc.cloc(),
            loc.blank(),
            loc.dloc()
        ],
        row.loc,
        "{name}"
    );
    assert_eq!(
        (0..row.source.len())
            .filter(|at| loc.dloc.lines.contains(*at))
            .collect::<Vec<_>>(),
        row.dloc_rows,
        "{name}"
    );

    let [visits, renders, built, materialized, scanned, covered] =
        std::array::from_fn(|at| collected[at] - idle[at]);
    // expected: the collector's walk enters every node exactly once.
    assert_eq!(visits, parser.root_node().preorder().count(), "{name}");
    // expected: one render per document published by the collection.
    assert_eq!(renders, index.documents().len(), "{name}");
    // expected: default options build one member for every selected fragment and none
    // for comments outside documents.
    assert_eq!(
        built,
        index
            .documents()
            .map(|document| document.get().fragment_count())
            .sum::<usize>(),
        "{name}"
    );
    // expected: each comment is a member of one run, materialized once by the source view.
    assert_eq!(
        materialized,
        parser
            .root_node()
            .preorder()
            .filter(|node| node.kind() == "comment")
            .count(),
        "{name}"
    );
    // expected: the source view scans every run once, at its final comment.
    assert_eq!(scanned, row.comment_runs, "{name}");
    // The coverage counter counts the collector's questions, with no tree-derived
    // closed form; the collection must move it on this thread.
    assert_ne!(covered, 0, "{name}");

    let [visits, renders, built, materialized, scanned, covered] =
        std::array::from_fn(|at| analyzed[at] - collected[at]);
    // expected: analyze never builds or renders a documentation index.
    assert_eq!(visits, 0, "{name}");
    assert_eq!(renders, 0, "{name}");
    // expected: the legacy view builds no source evidence or buffered members.
    assert_eq!(built, 0, "{name}");
    assert_eq!(materialized, 0, "{name}");
    // expected: the legacy view also scans every run once, at its final comment.
    assert_eq!(scanned, row.comment_runs, "{name}");
    // expected: the legacy view has no source context in which to ask a coverage question.
    assert_eq!(covered, 0, "{name}");
    index.documents().len()
}

#[cfg(feature = "lua")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_lua_source_and_legacy_views_are_independent() {
    let first_file = documentation::source("lua-first-file.lua");
    let primary = documentation::source("primary.lua");
    let rows = [
        LuaLegacyRow {
            name: "l01_tagged_block_above_function",
            source: b"module(\"m\")\n--- audit\n-- @param x thing\nfunction f(x) end\n",
            loc: [4, 2, 1, 2, 0, 2],
            dloc_rows: &[1, 2],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l02_luals_tag_above_function",
            source: b"module(\"m\")\n---@param x integer\nfunction f(x) end\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        // The trailing `-- correction` on a code row starts a second run.
        LuaLegacyRow {
            name: "l03_table_field_row",
            source: b"module(\"m\")\n--- settings\n-- @table options\noptions = {\n\
                      alpha = 1, -- correction\n}\n",
            loc: [6, 4, 1, 3, 0, 2],
            dloc_rows: &[1, 2],
            comment_runs: 2,
        },
        // The trailing `-- input` on a parameter row starts a second run.
        LuaLegacyRow {
            name: "l04_parameter_row",
            source: b"module(\"m\")\n---audit\nfunction f(\nx -- input\n) end\n",
            loc: [5, 4, 1, 2, 0, 1],
            dloc_rows: &[1],
            comment_runs: 2,
        },
        LuaLegacyRow {
            name: "l05_explicit_virtual_row",
            source: b"module(\"m\")\n---audit\n-- @function ghost\n",
            loc: [3, 1, 0, 2, 0, 2],
            dloc_rows: &[1, 2],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l06_function_assigned_to_field",
            source: b"module(\"m\")\n---audit\nM.f = function() end\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l07_local_table",
            source: b"module(\"m\")\n---audit\nlocal t = {}\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l08_return_table",
            source: b"module(\"m\")\n---audit\nreturn {}\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l10_long_block_several_rows",
            source: b"module(\"m\")\n--[[--\naudit\nsecond\n]]\nfunction f() end\n",
            loc: [6, 2, 1, 4, 0, 4],
            dloc_rows: &[1, 2, 3, 4],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l11_first_block_with_module_tag",
            source: b"--- Module.\n-- @module m\nlocal M = {}\n",
            loc: [3, 1, 1, 2, 0, 2],
            dloc_rows: &[0, 1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l12_fixture_first_file",
            source: &first_file,
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "l13_fixture_primary",
            source: &primary,
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        // The code between `-- plain` and the marked block separates their runs.
        LuaLegacyRow {
            name: "l14_plain_comment_module_block",
            source: b"-- plain\nmodule(\"m\")\n---audit\nfunction f() end\n",
            loc: [4, 2, 1, 2, 0, 1],
            dloc_rows: &[2],
            comment_runs: 2,
        },
        LuaLegacyRow {
            name: "l15_shebang_module_block",
            source: b"#!/usr/bin/env lua\nmodule(\"m\")\n---audit\nfunction f() end\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
    ];
    for row in rows {
        let name = row.name;
        assert_ne!(check_lua_legacy_row(&row), 0, "{name}");
    }
    // Whether a block publishes a document after code other than `module()` is open;
    // the other fourteen rows provide the positive controls for render and evidence.
    check_lua_legacy_row(&LuaLegacyRow {
        name: "l09_code_first_no_module",
        source: b"require(\"m\")\n---audit\nfunction f() end\n",
        loc: [3, 2, 1, 1, 0, 1],
        dloc_rows: &[1],
        comment_runs: 1,
    });
}

#[cfg(feature = "php")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_php_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    struct Row<'a> {
        name: &'static str,
        source: &'a [u8],
        loc: [u64; 6],
        dloc_rows: &'a [usize],
    }
    let fixture = documentation::source("php-define.php");
    let rows = [
        Row {
            name: "l01_property",
            source: b"<?php\nclass C {\n    /** d */\n    public $p;\n}\n",
            loc: [5, 4, 1, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l02_class_constant",
            source: b"<?php\nclass C {\n    /** d */\n    const K = 1;\n}\n",
            loc: [5, 4, 1, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l03_several_names",
            source: b"<?php\nclass C {\n    /** ab */\n    public $a, $b = 2;\n    \
                      /** kl */\n    const A = 1, B = 2;\n}\n",
            loc: [7, 5, 2, 2, 0, 2],
            dloc_rows: &[2, 4],
        },
        Row {
            name: "l04_promoted_parameter",
            source: b"<?php class C { function __construct(/** audit */ public int $x) {} }\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l05_after_attribute_property",
            source: b"<?php\nclass C { #[Attr]\n/** d */\npublic int $p = 1; }\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l06_before_and_after_attribute",
            source: b"<?php\n$x = 0;\n/** a */\n#[A]\n/** b */\nfunction g() {}\n",
            loc: [6, 4, 1, 2, 0, 2],
            dloc_rows: &[2, 4],
        },
        Row {
            name: "l07_two_blocks_before_first_enum",
            source: b"<?php\n/** file */\n/** item */\nenum E { case A; }\n",
            loc: [4, 2, 0, 2, 0, 2],
            dloc_rows: &[1, 2],
        },
        Row {
            name: "l08_two_blocks_before_first_define",
            source: b"<?php\n/** file */\n/** item */\ndefine('X', 1);\n",
            loc: [4, 2, 1, 2, 0, 2],
            dloc_rows: &[1, 2],
        },
        Row {
            name: "l09_define_in_function_body",
            source: b"<?php\nfunction f() {\n/** d */\ndefine(\"X\", 1);\n}\n",
            loc: [5, 4, 1, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l10_define_dynamic_name",
            source: b"<?php\nfunction z() {}\n/** d */\ndefine($n, 1);\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l11_define_aliased_as_make",
            source: b"<?php\nuse function define as make;\nfunction z() {}\n\
                      /** d */\nmake(\"X\", 1);\n",
            loc: [5, 4, 2, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l12_nested_function_in_body",
            source: b"<?php\nfunction f() {\n    /** d */\n    function g() {}\n}\n",
            loc: [5, 4, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l13_fixture_php_define",
            source: &fixture,
            loc: [4, 3, 3, 2, 0, 1],
            dloc_rows: &[1],
        },
    ];
    let mut rendered_total = 0;
    let mut built_total = 0;
    for row in rows {
        let name = row.name;
        let parser =
            crate::langs::AnyParser::parse(LANG::Php, row.source.to_vec(), None, None).unwrap();
        let root = parser.ts_tree().root_node();
        assert!(!root.has_error(), "{name}");
        let mut nodes = 0;
        let mut unvisited = vec![root];
        while let Some(node) = unvisited.pop() {
            nodes += 1;
            unvisited.extend(node.children(&mut node.walk()));
        }
        let idle = observations();
        let unbuilt = evidence_built::observed();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let built = evidence_built::observed();
        // expected: the collector enters every node once, named or unnamed, root included.
        assert_eq!(collected.visits - idle.visits, nodes, "{name}");
        // expected: complete text is built exactly once for each published document.
        let renders = collected.renders - idle.renders;
        assert_eq!(renders, index.documents().len(), "{name}");
        // expected: default options build one selected member per document fragment.
        let members = built - unbuilt;
        assert_eq!(
            members,
            index
                .documents()
                .map(|doc| doc.get().fragment_count())
                .sum::<usize>(),
            "{name}"
        );
        rendered_total += renders;
        built_total += members;

        let space = analyze(
            Source::new(LANG::Php, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let loc = &space.metrics.loc;
        assert_eq!(
            [
                loc.sloc(),
                loc.ploc(),
                loc.lloc(),
                loc.cloc(),
                loc.blank(),
                loc.dloc()
            ],
            row.loc,
            "{name}"
        );
        assert_eq!(
            (0..row.source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            row.dloc_rows,
            "{name}"
        );
        let analyzed = observations();
        // expected: analyze uses legacy LOC rows and builds no source evidence.
        assert_eq!(evidence_built::observed() - built, 0, "{name}");
        assert_eq!(analyzed.renders - collected.renders, 0, "{name}");
        assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
    }
    assert_ne!(rendered_total, 0);
    assert_ne!(built_total, 0);
}

#[cfg(feature = "ruby")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_ruby_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::positional::{
        evidence_built, members_materialized, positional_runs_collected,
    };
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, included_range_visits, observations,
    };
    struct Row<'a> {
        name: &'static str,
        source: &'a [u8],
        loc: [u64; 6],
        dloc_rows: &'a [usize],
        // A run is a maximal sequence of comments on consecutive rows that hold nothing but
        // comments. Only l10 has two: `# :nodoc:` follows code on its row, so it does not join
        // the run of `# audit` and counts on its own.
        comment_runs: usize,
    }
    let counters = || {
        [
            observations().visits,
            observations().renders,
            evidence_built::observed(),
            members_materialized::observed(),
            positional_runs_collected::observed(),
            included_range_visits::observed(),
        ]
    };
    let fixture = documentation::source("primary.rb");
    let sources = [
        Row {
            name: "l01_visibility_wrapped_def",
            source: b"class C\n# audit\nprivate def f; end\nend\n",
            loc: [4, 3, 2, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        Row {
            name: "l02_attribute_call",
            source: b"# audit\nattr_reader :x\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            comment_runs: 1,
        },
        Row {
            name: "l03_attribute_two_names",
            source: b"class C\n# audit\nattr_accessor :a, :b\nend\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        Row {
            name: "l04_constant",
            source: b"# audit\nX = 1\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            comment_runs: 1,
        },
        Row {
            name: "l05_alias",
            source: b"class C\ndef a; end\n# audit\nalias b a\nend\n",
            loc: [5, 4, 3, 1, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
        Row {
            name: "l06_blank_row_before_def",
            source: b"# audit\n\ndef f; end\n",
            loc: [3, 1, 1, 1, 1, 0],
            dloc_rows: &[],
            comment_runs: 1,
        },
        Row {
            name: "l07_double_hash_above_def",
            source: b"##\n# audit\ndef f; end\n",
            loc: [3, 1, 1, 2, 0, 2],
            dloc_rows: &[0, 1],
            comment_runs: 1,
        },
        Row {
            name: "l08_yard_virtual_method",
            source: b"# @!method ghost\n#   audit\n",
            loc: [2, 0, 0, 2, 0, 2],
            dloc_rows: &[0, 1],
            comment_runs: 1,
        },
        Row {
            name: "l09_block_then_row",
            source: b"=begin\na\n=end\n# b\ndef f; end\n",
            loc: [5, 1, 1, 4, 0, 4],
            dloc_rows: &[0, 1, 2, 3],
            comment_runs: 1,
        },
        Row {
            name: "l10_nodoc_after_def",
            source: b"# audit\ndef f; end # :nodoc:\n",
            loc: [2, 1, 1, 2, 0, 1],
            dloc_rows: &[0],
            comment_runs: 2,
        },
        Row {
            name: "l11_shebang_and_magic_comment",
            source: b"#!/usr/bin/env ruby\n# frozen_string_literal: true\n# audit\ndef f; end\n",
            loc: [4, 1, 1, 3, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
        Row {
            name: "l12_later_visibility_symbol",
            source: b"class C\n# audit\ndef f; end\nprivate :f\nend\n",
            loc: [5, 4, 2, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        Row {
            name: "l13_fixture_primary_rb",
            source: &fixture,
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[0],
            comment_runs: 1,
        },
    ];
    let (mut renders_total, mut evidence_total) = (0, 0);
    for row in sources {
        let name = row.name;
        let parser =
            crate::langs::AnyParser::parse(LANG::Ruby, row.source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error(), "{name}");
        let idle = counters();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = counters();
        let space = analyze(
            Source::new(LANG::Ruby, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = counters();

        let loc = &space.metrics.loc;
        assert_eq!(
            [
                loc.sloc(),
                loc.ploc(),
                loc.lloc(),
                loc.cloc(),
                loc.blank(),
                loc.dloc()
            ],
            row.loc,
            "{name}"
        );
        assert_eq!(
            (0..row.source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            row.dloc_rows,
            "{name}"
        );

        let [visits, renders, built, materialized, scanned, covered] =
            std::array::from_fn(|at| collected[at] - idle[at]);
        // expected: the walk visits each node exactly once.
        assert_eq!(visits, parser.root_node().preorder().count(), "{name}");
        // expected: one render per document published by the collection.
        assert_eq!(renders, index.documents().len(), "{name}");
        // expected: the default positional stream builds one member per selected fragment.
        assert_eq!(
            built,
            index
                .documents()
                .map(|document| document.get().fragment_count())
                .sum::<usize>(),
            "{name}"
        );
        // expected: every comment is materialized once as a member of a run.
        assert_eq!(
            materialized,
            parser
                .root_node()
                .preorder()
                .filter(|node| node.kind() == "comment")
                .count(),
            "{name}"
        );
        // expected: every run is scanned once, at its last comment.
        assert_eq!(scanned, row.comment_runs, "{name}");
        // Coverage questions have no tree-derived exact count; collection must ask some.
        assert_ne!(covered, 0, "{name}");
        renders_total += renders;
        evidence_total += built;

        let [visits, renders, built, materialized, scanned, covered] =
            std::array::from_fn(|at| analyzed[at] - collected[at]);
        // expected: analyze does not collect or render a source documentation index.
        assert_eq!(visits, 0, "{name}");
        assert_eq!(renders, 0, "{name}");
        // expected: the legacy view builds no source evidence or buffered members.
        assert_eq!(built, 0, "{name}");
        assert_eq!(materialized, 0, "{name}");
        // expected: the legacy view scans each comment run once too.
        assert_eq!(scanned, row.comment_runs, "{name}");
        // expected: the legacy view has no source context to ask coverage questions.
        assert_eq!(covered, 0, "{name}");
    }
    // One row may publish nothing; the remaining rows control both positive deltas.
    assert_ne!(renders_total, 0);
    assert_ne!(evidence_total, 0);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_js_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    struct Row<'a> {
        variants: &'a [LANG],
        name: &'static str,
        source: &'a [u8],
        loc: [u64; 6],
        dloc_rows: &'a [usize],
    }
    const BOTH: &[LANG] = &[LANG::Javascript, LANG::Mozjs];
    let rows = [
        Row {
            variants: BOTH,
            name: "l01_const_arrow",
            source: b"/**audit*/\nconst f = () => 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l02_const_two_declarators",
            source: b"/**audit*/\nconst a = 1, b = 2;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l03_export_const",
            source: b"/**audit*/\nexport const e = 3;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l04_assignment",
            source: b"/**audit*/\nx = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l05_default_anonymous_function",
            source: b"/**audit*/\nexport default function () {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l06_virtual_name",
            source: b"/**@name ghost\n * @function\n * audit*/\n",
            loc: [3, 0, 0, 3, 0, 3],
            dloc_rows: &[0, 1, 2],
        },
        Row {
            variants: BOTH,
            name: "l07_body_before_return",
            source: b"function f() {\n    /** note */\n    return 1;\n}\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            variants: BOTH,
            name: "l08_inner_declaration",
            source: b"function outer() { /**audit*/ function inner() {} }\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: BOTH,
            name: "l09_two_blocks_at_the_first_statement",
            source: b"/**old*/\n// plain\n/**new*/\nfunction f() {}\n",
            loc: [4, 1, 0, 3, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            variants: BOTH,
            name: "l10_static_on_its_own_row",
            source: b"class C {\nstatic\n/** d */\nm() {}\n}\n",
            loc: [5, 4, 0, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            variants: BOTH,
            name: "l11_using",
            source: b"/**audit*/\nusing u = f();\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            variants: &[LANG::Mozjs],
            name: "l12_preproc_line_between",
            source: b"/** d */\n#ifdef XP_WIN\nfunction f() {}\n#endif\n",
            loc: [4, 3, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            variants: BOTH,
            name: "l13_through_a_decorator",
            source: b"/** d */\n@dec\nclass C {}\n",
            loc: [3, 2, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
    ];
    let mut ran = 0;
    let mut rendered = 0;
    for row in rows {
        for &lang in row.variants {
            if !lang.is_enabled() {
                continue;
            }
            ran += 1;
            let name = format!("{lang:?}: {}", row.name);
            let parser =
                crate::langs::AnyParser::parse(lang, row.source.to_vec(), None, None).unwrap();
            assert!(!parser.ts_tree().root_node().has_error(), "{name}");
            let nodes = parser.root_node().preorder().count();
            let idle = observations();
            let unbuilt = evidence_built::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions {
                        retain_unbound: true,
                        ..DocumentationOptions::default()
                    },
                )
                .unwrap();
            let collected = observations();
            let built = evidence_built::observed();
            let visits = collected.visits - idle.visits;
            let renders = collected.renders - idle.renders;
            let members = built - unbuilt;
            // expected: the collector visits each node exactly once, including unnamed nodes.
            assert_eq!(visits, nodes, "{name}");
            // expected: each published document is rendered once; retained candidates have no text.
            assert_eq!(renders, index.documents().len(), "{name}");
            // expected: each document fragment and retained candidate builds one marker member.
            assert_eq!(
                members,
                index
                    .documents()
                    .map(|doc| doc.get().fragment_count())
                    .sum::<usize>()
                    + index.evidence().len(),
                "{name}"
            );
            assert_ne!(visits, 0, "{name}");
            assert_ne!(members, 0, "{name}");
            rendered += renders;
            let space = analyze(
                Source::new(lang, row.source),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            let analyzed = observations();
            // expected: legacy LOC uses no source-view collector or rendered evidence.
            assert_eq!(evidence_built::observed() - built, 0, "{name}");
            assert_eq!(analyzed.renders - collected.renders, 0, "{name}");
            assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
            let loc = &space.metrics.loc;
            assert_eq!(
                [
                    loc.sloc(),
                    loc.ploc(),
                    loc.lloc(),
                    loc.cloc(),
                    loc.blank(),
                    loc.dloc(),
                ],
                row.loc,
                "{name}"
            );
            assert_eq!(
                (0..row.source.len())
                    .filter(|at| loc.dloc.lines.contains(*at))
                    .collect::<Vec<_>>(),
                row.dloc_rows,
                "{name}",
            );
        }
    }
    assert!(ran > 0);
    // A row without a document renders nothing, so the control for `renders` is the total.
    assert_ne!(rendered, 0);
}

#[cfg(feature = "typescript")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_typescript_and_tsx_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    struct Row<'a> {
        language: LANG,
        source: &'a [u8],
        loc: [u64; 6],
        dloc_rows: &'a [usize],
        documents: usize,
    }
    let ts = documentation::source("primary.ts");
    let tsx = documentation::source("primary.tsx");
    let rows = [
        Row {
            language: LANG::Typescript,
            source: b"/** above */\ndeclare /** inside */ function f(): void;\n",
            loc: [2, 1, 0, 2, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** above */\ndeclare /** inside */ function f(): void;\n",
            loc: [2, 1, 0, 2, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/**audit*/\nexport default function() {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**audit*/\nexport default function() {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/**audit*/\nexport default class {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**audit*/\nexport default class {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** d */\nnamespace N {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** d */\nnamespace N {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** d */\nexport namespace N {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** d */\nexport namespace N {}\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** d */\ndeclare namespace N {}\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** d */\ndeclare namespace N {}\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/**audit*/\nconst x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**audit*/\nconst x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** @packageDocumentation */\nexport const x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** @packageDocumentation */\nexport const x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** d */\ndeclare const x: number;\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** d */\ndeclare const x: number;\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"function f(/**audit*/ x: number) {}\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"function f(/**audit*/ x: number) {}\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/**one*/\nfunction f(x: string): string;\n/**two*/\n\
function f(x: number): number;\n",
            loc: [4, 2, 0, 2, 0, 2],
            dloc_rows: &[0, 2],
            documents: 2,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**one*/\nfunction f(x: string): string;\n/**two*/\n\
function f(x: number): number;\n",
            loc: [4, 2, 0, 2, 0, 2],
            dloc_rows: &[0, 2],
            documents: 2,
        },
        Row {
            language: LANG::Typescript,
            source: b"/**audit*/\nexport const {x} = value;\n",
            loc: [2, 1, 1, 1, 0, 0],
            dloc_rows: &[],
            documents: 0,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**audit*/\nexport const {x} = value;\n",
            loc: [2, 1, 1, 1, 0, 0],
            dloc_rows: &[],
            documents: 0,
        },
        Row {
            language: LANG::Typescript,
            source: b"class C {\n/**a*/\n@dec\n/**b*/\nm() {}\n}\n",
            loc: [6, 4, 0, 2, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"class C {\n/**a*/\n@dec\n/**b*/\nm() {}\n}\n",
            loc: [6, 4, 0, 2, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** above */\nexport /** inside */ function f(): void {}\n",
            loc: [2, 1, 1, 2, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** above */\nexport /** inside */ function f(): void {}\n",
            loc: [2, 1, 1, 2, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** @packageDocumentation */\n/** d */\nfunction f(): void {}\n",
            loc: [3, 1, 0, 2, 0, 2],
            dloc_rows: &[0, 1],
            documents: 2,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** @packageDocumentation */\n/** d */\nfunction f(): void {}\n",
            loc: [3, 1, 0, 2, 0, 2],
            dloc_rows: &[0, 1],
            documents: 2,
        },
        Row {
            language: LANG::Tsx,
            source: b"/**audit*/\nfunction f() { return <div/>; }\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: &ts,
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: &tsx,
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Typescript,
            source: b"/** d */\nexport const x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
        Row {
            language: LANG::Tsx,
            source: b"/** d */\nexport const x = 1;\n",
            loc: [2, 1, 1, 1, 0, 1],
            dloc_rows: &[0],
            documents: 1,
        },
    ];
    for row in rows {
        let name = format!("{:?}: {}", row.language, row.source.escape_ascii());
        let parser =
            crate::langs::AnyParser::parse(row.language, row.source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error(), "{name}");
        let idle = observations();
        let unbuilt = evidence_built::observed();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let built = evidence_built::observed();
        // expected: the collector enters each named or unnamed parse-tree node once.
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{name}"
        );
        // expected: one completed render per published document.
        assert_eq!(
            collected.renders - idle.renders,
            index.documents().len(),
            "{name}"
        );
        assert_eq!(index.documents().len(), row.documents, "{name}");
        // expected: one marker member per selected block, none for unselected candidates.
        assert_eq!(built - unbuilt, row.documents, "{name}");
        let space = analyze(
            Source::new(row.language, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = observations();
        // expected: legacy LOC does not collect documentation a second time.
        assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
        assert_eq!(analyzed.renders - collected.renders, 0, "{name}");
        assert_eq!(evidence_built::observed() - built, 0, "{name}");
        let loc = &space.metrics.loc;
        assert_eq!(
            [
                loc.sloc(),
                loc.ploc(),
                loc.lloc(),
                loc.cloc(),
                loc.blank(),
                loc.dloc(),
            ],
            row.loc,
            "{name}"
        );
        assert_eq!(
            (0..row.source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            row.dloc_rows,
            "{name}"
        );
    }
}

#[cfg(feature = "java")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_java_field_owners_leave_legacy_rows_unchanged() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::{AnyParser, documentation::*};
    for (source, documents, owners, projections, loc, rows) in [
        (
            "class C { /**audit*/ int x; }\n",
            1,
            vec![1],
            // expected: file, class, field declaration.
            3,
            [1, 1, 0, 1, 0, 1],
            vec![0],
        ),
        (
            "class C { /**audit*/ int a, b; }\n",
            1,
            vec![2],
            // expected: file, class, field declaration.
            3,
            [1, 1, 0, 1, 0, 1],
            vec![0],
        ),
        (
            "interface I { /**audit*/ int K = 1; }\n",
            1,
            vec![1],
            // expected: file, interface, constant declaration.
            3,
            [1, 1, 0, 1, 0, 1],
            vec![0],
        ),
        (
            "enum E { /**a*/ A, /**b*/ B; /**f*/ int x; }\n",
            3,
            vec![1, 1, 1],
            // expected: file, enum, two constants, field declaration.
            5,
            [1, 1, 0, 1, 0, 1],
            vec![0],
        ),
        (
            "class C {\n  /** a */\n  int x,\n  /** b */\n  y;\n}\n",
            1,
            vec![2],
            // expected: file, class, field declaration.
            3,
            [6, 4, 0, 2, 0, 1],
            vec![1],
        ),
        (
            "class C {\n/** d */\nint x\n/** e */\nint y;\n}\n",
            1,
            vec![],
            // expected: file and the second field declaration; the class and the first
            // field hold the syntax error and stay pending.
            2,
            [6, 5, 0, 2, 0, 1],
            vec![1],
        ),
        (
            "class C {\n  int x;\n  /**d*/\n  ;\n  void f() {}\n}\n",
            0,
            vec![],
            // expected: file, class, field declaration, method.
            4,
            [6, 5, 0, 1, 0, 0],
            vec![],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
        let idle = observations();
        let unbuilt = evidence_built::observed();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), documents, "{source}");
        assert_eq!(
            index
                .associations()
                .map(|association| association.get().owner_count())
                .collect::<Vec<_>>(),
            owners,
            "{source}"
        );
        let before = observations();
        let built = evidence_built::observed();
        // The collection moves each counter on this thread, so a zero across `analyze`
        // is a reading. The last row has no document; the other six move the render
        // and evidence counters.
        // expected: the walk enters every node once.
        assert_eq!(
            before.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{source}"
        );
        // expected: one render and one built member per document; each has one fragment.
        assert_eq!(before.renders - idle.renders, documents, "{source}");
        assert_eq!(built - unbuilt, documents, "{source}");
        assert_eq!(
            before.owner_projections - idle.owner_projections,
            projections,
            "{source}"
        );
        // The checkpoint total has no closed form.
        assert_ne!(before.polls - idle.polls, 0, "{source}");
        let space = analyze(
            Source::new(LANG::Java, source.as_bytes()),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let stats = &space.metrics.loc;
        assert_eq!(
            [
                stats.sloc(),
                stats.ploc(),
                stats.lloc(),
                stats.cloc(),
                stats.blank(),
                stats.dloc()
            ],
            loc,
            "{source}"
        );
        assert_eq!(
            (0..source.len())
                .filter(|row| stats.dloc.lines.contains(*row))
                .collect::<Vec<_>>(),
            rows,
            "{source}"
        );
        let after = observations();
        assert_eq!(evidence_built::observed() - built, 0, "{source}");
        assert_eq!(after.renders - before.renders, 0, "{source}");
        assert_eq!(after.visits - before.visits, 0, "{source}");
        assert_eq!(
            after.owner_projections - before.owner_projections,
            0,
            "{source}"
        );
        assert_eq!(after.polls - before.polls, 0, "{source}");
    }
}
