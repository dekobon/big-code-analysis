//! Legacy preservation on raw inputs; source owner/text requirements live in AST fixtures.

use big_code_analysis_ast::test_support::documentation;
use serde_json::{Value, json};

use crate::{LANG, Metric, MetricsOptions, Source, analyze};

#[cfg(feature = "rust")]
#[test]
fn documentation_rust_literal_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::{AnyParser, documentation::*};
    let source = b"/// first\n#[doc = r\"second\"]\nfn f() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(
        index
            .complete_text(index.documents().next().unwrap())
            .unwrap()
            .unwrap()
            .text(),
        "first\nsecond"
    );
    for exclude in [false, true] {
        let before = observations();
        let space = analyze(
            Source::new(LANG::Rust, source),
            MetricsOptions::default()
                .with_only(&[Metric::Loc, Metric::Tokens])
                .with_exclude_tests(exclude),
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
            [3, 2, 0, 1, 0, 1]
        );
        // Raw attribute: # [ doc = string_content ]; function: fn f ( ) { }.
        assert_eq!(space.metrics.tokens.tokens_sum(), 12);
        assert_eq!(observations().renders, before.renders);
        assert_eq!(observations().visits, before.visits);
    }
}

#[cfg(feature = "rust")]
#[test]
fn documentation_rust_literal_test_pruning_changes_only_legacy_population() {
    use big_code_analysis_ast::{AnyParser, documentation::*};
    let source = b"#[doc = \"removed\"] #[test]\nfn t() {}\n/// kept\nfn k() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(
        index
            .documents()
            .map(|doc| index.complete_text(doc).unwrap().unwrap().text())
            .collect::<Vec<_>>(),
        ["removed", "kept"]
    );
    for (exclude, tokens, functions) in [(false, 24, 2), (true, 6, 1)] {
        let space = analyze(
            Source::new(LANG::Rust, source),
            MetricsOptions::default()
                .with_only(&[Metric::Loc, Metric::Tokens])
                .with_exclude_tests(exclude),
        )
        .unwrap();
        assert_eq!(space.spaces.len(), functions);
        assert_eq!(space.metrics.tokens.tokens_sum(), tokens);
        assert_eq!(space.metrics.loc.cloc(), 1);
        assert_eq!(space.metrics.loc.dloc(), 1);
    }
}

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
        // A run is a maximal sequence of comments with no other node between them, in
        // which each one starts on the row where the one before it ends, or on the next
        // row. Code before the first comment on its row does not split the run, so the
        // rows `/* t1` and `{ // t` have one each; the comment after code is in the run
        // and in no document.
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

#[cfg(feature = "groovy")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_groovy_source_and_legacy_views_are_independent() {
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
    let primary = documentation::source("primary.groovy");
    let spaces = documentation::source("groovy-spaces.groovy");
    let rows = [
        Row {
            name: "l01_field_own_row",
            source: b"class C {\n    /** d */\n    int x\n}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l02_field_two_declarators",
            source: b"class C {\n    /** d */\n    int a, b\n}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l03_property",
            source: b"class C {\n    /** d */\n    def x\n}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l04_same_row_class",
            source: b"/**audit*/ class C {}\n",
            loc: [1, 1, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l05_same_row_method",
            source: b"class C { /**audit*/ void f() {} }\n",
            loc: [1, 1, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l06_same_row_constructor",
            source: b"class C { /**audit*/ C() {} }\n",
            loc: [1, 1, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l07_same_row_field",
            source: b"class C { /**audit*/ int x }\n",
            loc: [1, 1, 0, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l08_block_above_package",
            source: b"/** d */\npackage p\nclass C {}\n",
            loc: [3, 2, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l09_constructor_own_row",
            source: b"class C {\n    /** d */\n    C() {}\n}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l10_enum_constant",
            source: b"enum E {\n    /** d */\n    A\n}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l11_script_level_def",
            source: b"/** d */\ndef f() {}\n",
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l12_fixture_primary",
            source: &primary,
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l13_fixture_groovy_spaces",
            source: &spaces,
            loc: [11, 6, 1, 5, 0, 5],
            dloc_rows: &[0, 2, 4, 6, 8],
        },
    ];
    let mut renders_total = 0;
    for row in rows {
        let name = row.name;
        let parser =
            crate::langs::AnyParser::parse(LANG::Groovy, row.source.to_vec(), None, None).unwrap();
        let root = parser.ts_tree().root_node();
        assert!(!root.has_error(), "{name}");
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
        // expected: the collector's walk enters each node exactly once, named or unnamed.
        assert_eq!(visits, nodes, "{name}");
        assert_ne!(visits, 0, "{name}");
        let renders = collected.renders - idle.renders;
        // expected: the collector renders text once for each published document.
        assert_eq!(renders, index.documents().len(), "{name}");
        renders_total += renders;
        let members = built - unbuilt;
        // expected: the marker stream builds one member per document fragment and one for
        // each retained candidate, which also moves the counter on same-row sources.
        assert_eq!(
            members,
            index
                .documents()
                .map(|doc| doc.get().fragment_count())
                .sum::<usize>()
                + index.evidence().len(),
            "{name}"
        );
        assert_ne!(members, 0, "{name}");

        let space = analyze(
            Source::new(LANG::Groovy, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = observations();
        // expected: legacy LOC does not build or render a source documentation index.
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
    }
    // The other nine rows control the zero render delta of the four same-row forms.
    assert_ne!(renders_total, 0);
}

#[cfg(feature = "kotlin")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_kotlin_source_and_legacy_views_are_independent() {
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
    let fixture = documentation::source("primary.kt");
    let rows = [
        Row {
            name: "l01_property",
            source: b"class C {\n    /** d */\n    val p = 1\n}\n",
            loc: [4, 3, 1, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l02_class_parameter",
            source: b"class C(/**audit*/ val x: Int)\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l03_function_parameter",
            source: b"fun f(/**p*/ x: Int) {}\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l04_modifier_list",
            source: b"public /**audit*/ inline fun f() {}\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l05_before_constructor",
            source: b"class C private /**audit*/ constructor()\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l06_before_body",
            source: b"fun f() /**audit*/ {}\n",
            loc: [1, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
        Row {
            name: "l07_split_annotation",
            source: b"fun g() {}\n/** d */\n@Suppress(\"x\")\nfun f() {}\n",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l08_context_top_level",
            source: b"/** d */\ncontext(Foo)\nfun f() {}\n",
            loc: [3, 2, 1, 1, 0, 0],
            dloc_rows: &[],
        },
        Row {
            name: "l09_older_and_newer",
            source: b"/**old*/\n// plain\n/**new*/\nfun f() {}\n",
            loc: [4, 1, 0, 3, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l10_before_accessor",
            source: b"class C {\n    val q: Int\n        /** g */\n        get() = 1\n}\n",
            loc: [5, 4, 1, 1, 0, 1],
            dloc_rows: &[2],
        },
        Row {
            name: "l11_after_annotation",
            source: b"@Deprecated(\"x\")\n/** d */\nfun f() {}\n",
            loc: [3, 2, 0, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            name: "l12_fixture_primary_kt",
            source: &fixture,
            loc: [2, 1, 0, 1, 0, 1],
            dloc_rows: &[0],
        },
    ];
    let (mut renders_total, mut evidence_total) = (0, 0);
    for row in rows {
        let name = row.name;
        let parser =
            crate::langs::AnyParser::parse(LANG::Kotlin, row.source.to_vec(), None, None).unwrap();
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
        // expected: the collector enters every tree node once, named or not, root included.
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{name}"
        );
        // expected: the collection renders each of its own published documents once.
        let renders = collected.renders - idle.renders;
        assert_eq!(renders, index.documents().len(), "{name}");
        // expected: default options build one member per selected document fragment.
        let members = built - unbuilt;
        assert_eq!(
            members,
            index
                .documents()
                .map(|document| document.get().fragment_count())
                .sum::<usize>(),
            "{name}"
        );
        renders_total += renders;
        evidence_total += members;

        let space = analyze(
            Source::new(LANG::Kotlin, row.source),
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
        let analyzed = observations();
        // expected: analyze uses legacy LOC rows and builds no documentation index or evidence.
        assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
        assert_eq!(analyzed.renders - collected.renders, 0, "{name}");
        assert_eq!(evidence_built::observed() - built, 0, "{name}");
    }
    // The context row need not publish a document; the others control both positive deltas.
    assert_ne!(renders_total, 0);
    assert_ne!(evidence_total, 0);
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_go_member_sources_keep_their_legacy_rows() {
    use big_code_analysis_ast::documentation::streaming::positional::{
        evidence_built, members_materialized, positional_runs_collected,
    };
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, included_range_visits, observations,
    };
    // Struct fields and interface elements, as the source-view tests of
    // `go_tests.rs` use them. The legacy rows were measured before the collector gave
    // these members owners, and they do not depend on it.
    struct Row<'a> {
        source: &'a [u8],
        syntax_error: bool,
        loc: [u64; 6],
        dloc_rows: &'a [usize],
        documents: usize,
        // The rule of `comment_runs` in the test above.
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
    let sources = [
        Row {
            source: b"package p\ntype S struct {\n// audit\nX int\n}\n",
            syntax_error: false,
            loc: [5, 4, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 1,
        },
        // `_ int` separates the two comments.
        Row {
            source: b"package p\ntype S struct {\n// pad\n_ int\n// two\n_, X int\n}\n",
            syntax_error: false,
            loc: [7, 5, 0, 2, 0, 2],
            dloc_rows: &[2, 4],
            documents: 2,
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype S struct {\n// audit\nio.Reader\n}\n",
            syntax_error: false,
            loc: [5, 4, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 1,
        },
        // An element separates each comment from the next.
        Row {
            source: b"package p\ntype I interface {\n// a\nint | string\n// c\nG[int]\n// d\nE\n// f\nM(x int) (int, error)\n// g\nMap[string, int]\n}\n",
            syntax_error: false,
            loc: [13, 8, 0, 5, 0, 5],
            dloc_rows: &[2, 4, 6, 8, 10],
            documents: 5,
            comment_runs: 5,
        },
        // The second comment is inside the method that follows the first.
        Row {
            source: b"package p\ntype I interface {\n// m\nM(s struct {\n// d\nX int\n})\n}\n",
            syntax_error: false,
            loc: [8, 6, 0, 2, 0, 2],
            dloc_rows: &[2, 4],
            documents: 2,
            comment_runs: 2,
        },
        // `// t` follows code on row 2 and `// d` is on the next row: one run, and
        // only row 3 documents.
        Row {
            source: b"package p\ntype S struct {\nX int // t\n// d\nY int\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 2, 0, 1],
            dloc_rows: &[3],
            documents: 1,
            comment_runs: 1,
        },
        // A blank row separates the two comments, and another separates the second
        // from the field.
        Row {
            source: b"package p\ntype S struct {\nX int // t\n\n// lost\n\nY int\n}\n",
            syntax_error: false,
            loc: [8, 5, 0, 2, 2, 0],
            dloc_rows: &[],
            documents: 0,
            comment_runs: 2,
        },
        // The second comment is inside the field that follows the first.
        Row {
            source: b"package p\ntype S struct {\n// d\nX, /* c */ Y int\n}\n",
            syntax_error: false,
            loc: [5, 4, 0, 2, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype S struct {\n// d\nX /* a\n b */ int\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 3, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype S struct {\n// d\nX func() /* a\n b */ int\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 3, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 2,
        },
        Row {
            source: b"package p\nfunc F() {\ntype T struct {\n// a\nX int\n}\n}\n",
            syntax_error: false,
            loc: [7, 6, 0, 1, 0, 0],
            dloc_rows: &[],
            documents: 0,
            comment_runs: 1,
        },
        Row {
            source: b"package p\ntype I interface {\n// u\n~int\n}\n",
            syntax_error: false,
            loc: [5, 4, 0, 1, 0, 0],
            dloc_rows: &[],
            documents: 0,
            comment_runs: 1,
        },
        Row {
            source: b"package p\ntype S struct {\n// a\nX %% int\n}\n",
            syntax_error: true,
            loc: [5, 4, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 1,
        },
        // A field separates each comment from the next.
        Row {
            source: b"package p\ntype S struct {\n// a\nX int\n// b\nY %% int\n// c\nZ int\n}\n",
            syntax_error: true,
            loc: [9, 6, 0, 3, 0, 3],
            dloc_rows: &[2, 4, 6],
            documents: 3,
            comment_runs: 3,
        },
        // Members that Go ends before the end of their node, and members over several
        // rows that Go reads whole. The collector keeps the first three pending.
        Row {
            source: b"package p\ntype S struct {\n// d\nX [2]\nint\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 1,
        },
        // The second comment is inside the field that follows the first.
        Row {
            source: b"package p\ntype S struct {\n// d\nX [2] // c\nint\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 2, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype I interface {\n// d\nE | func() /* stop\n here */ B | C\n}\n",
            syntax_error: false,
            loc: [6, 5, 0, 3, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 2,
        },
        Row {
            source: b"package p\ntype I interface {\n// d\nM(\n\ta int,\n\tb int,\n) error\n}\n",
            syntax_error: false,
            loc: [8, 7, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
            comment_runs: 1,
        },
        // A field separates each comment from the next.
        Row {
            source: b"package p\ntype S struct {\n// d\nA struct {\n// x\nX int\n// y\nY int\n}\n}\n",
            syntax_error: false,
            loc: [10, 7, 0, 3, 0, 3],
            dloc_rows: &[2, 4, 6],
            documents: 3,
            comment_runs: 3,
        },
    ];
    for row in sources {
        let name = row.source.escape_ascii();
        let parser =
            crate::langs::AnyParser::parse(LANG::Go, row.source.to_vec(), None, None).unwrap();
        assert_eq!(
            parser.ts_tree().root_node().has_error(),
            row.syntax_error,
            "{name}"
        );
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
        assert_eq!(index.documents().len(), row.documents, "{name}");
        assert_eq!(renders, row.documents, "{name}");
        // expected: under default options the positional stream builds one member for
        // each fragment of a selected run, and none for a trailing member.
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
        // expected: in these sources a document is published exactly where the legacy
        // view counts a row.
        assert_eq!(row.documents == 0, row.dloc_rows.is_empty(), "{name}");

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

/// One Perl source of the POD unit with its legacy values, measured before the collector
/// read any of them: name, bytes, `[sloc, ploc, lloc, cloc, blank, dloc]` and the
/// zero-based documentation rows.
#[cfg(feature = "perl")]
type PerlPodSource = (&'static str, &'static [u8], [u64; 6], &'static [usize]);

#[cfg(feature = "perl")]
static PERL_POD_SOURCES: [PerlPodSource; 261] = [
    (
        "a01_primary",
        b"=head2 f\n\nAudit\n\n=cut\n\nsub f {}",
        [7, 1, 0, 5, 1, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a02_two_blocks",
        b"my $x = 1;\n=head1 A\n\na\n\n=cut\nmy $y = 2;\n=head1 B\n\nb\n\n=cut\nmy $z \
          = 3;\n",
        [13, 3, 3, 10, 0, 10],
        &[1, 2, 3, 4, 5, 7, 8, 9, 10, 11],
    ),
    (
        "a03_in_sub",
        b"sub f {\n    my $x = 1;\n\n=pod\n\nDoc.\n\n=cut\n\n    return $x;\n}\n",
        [11, 4, 2, 5, 2, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "a04_adjacent",
        b"=head1 A\n\na\n\n=cut\n=head1 B\n\nb\n\n=cut\n",
        [10, 0, 0, 10, 0, 10],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    ),
    (
        "a05_begin_html",
        b"=begin html\n\n<p>x</p>\n\n=end html\n\n=cut\n",
        [7, 0, 0, 7, 0, 7],
        &[0, 1, 2, 3, 4, 5, 6],
    ),
    (
        "a06_cut_with_text",
        b"=head1 A\n\na\n\n=cut here is text\nmy $y = 2;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a07_cut_digit",
        b"=head1 A\n\ntext\n\n=cut2\nmy $y = 2;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a08_command_and_cut",
        b"=pod\n=cut\n",
        [2, 0, 0, 2, 0, 2],
        &[0, 1],
    ),
    (
        "a09_whitespace_line",
        b"=pod\n\nx\n \n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a10_blank_lines",
        b"=pod\n\n\n\ny\n\n\n=cut\n",
        [8, 0, 0, 8, 0, 8],
        &[0, 1, 2, 3, 4, 5, 6, 7],
    ),
    (
        "a11_collision",
        b"=head2 f\n\nAudit\n\n=cut\n\nsub f {}\nsub f {}",
        [8, 2, 0, 5, 1, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a12_package",
        b"package Foo;\n\n=head1 NAME\n\nFoo\n\n=cut\n\nsub new {}\n",
        [9, 2, 1, 5, 2, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "a13_nested_sub",
        b"sub o {\n  sub i {\n=head2 i\n\nInner.\n\n=cut\n  }\n}\n",
        [9, 4, 0, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "a14_begin_colon_comment",
        b"=begin :comment\n\nx\n\n=end :comment\n\n=cut\n",
        [7, 0, 0, 7, 0, 7],
        &[0, 1, 2, 3, 4, 5, 6],
    ),
    (
        "a15_item_tab",
        b"=item\t*\n\ny\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a16_vertical_tab",
        b"=pod\x0bx\n\ny\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a17_cut_form_feed",
        b"=pod\n\nx\n\n=cut\x0cfoo\nmy $y;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a18_end_marker_inside",
        b"=pod\n\n__END__\n\n=cut\nmy $x;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "a19_uppercase_cut",
        b"=pod\n\ny\n\n=CUT\nmy $x;\n",
        [6, 0, 0, 6, 0, 6],
        &[0, 1, 2, 3, 4, 5],
    ),
    (
        "b01_crlf",
        b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n",
        [6, 1, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "b02_unterminated",
        b"my $x = 1;\n\n=head1 NAME\n\nNever closed.\n",
        [5, 1, 1, 3, 1, 3],
        &[2, 3, 4],
    ),
    (
        "b03_unterminated_no_final_lf",
        b"my $x = 1;\n\n=head1 NAME\n\nNever closed.",
        [5, 1, 1, 3, 1, 3],
        &[2, 3, 4],
    ),
    (
        "b04_unterminated_blank_rows",
        b"=head1 NAME\n\nNever closed.\n\n\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "b05_cut_at_end_of_input",
        b"=pod\n\nx\n\n=cut",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "b06_crlf_unterminated",
        b"=pod\r\n\r\nx\r\n",
        [3, 0, 0, 3, 0, 3],
        &[0, 1, 2],
    ),
    (
        "b07_crlf_cut_at_end",
        b"=pod\r\n\r\nx\r\n\r\n=cut\r\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    ("b08_command_line_only", b"=pod\n", [1, 0, 0, 1, 0, 1], &[0]),
    (
        "c01_comment_only",
        b"=begin comment\n\nnot prose\n\n=end comment\n\n=cut\n",
        [7, 0, 0, 7, 0, 0],
        &[],
    ),
    (
        "c02_two_comment_only",
        b"=begin comment\n\na\n\n=end comment\n\n=cut\n\nmy $x;\n\n=for comment b\n\n=\
          cut\n",
        [13, 1, 1, 10, 2, 0],
        &[],
    ),
    (
        "c03_comment_then_document",
        b"=begin comment\n\nx\n\n=end comment\n\n=cut\n\n=head1 A\n\na\n\n=cut\n",
        [13, 0, 0, 12, 1, 5],
        &[8, 9, 10, 11, 12],
    ),
    (
        "c04_mixed",
        b"=head2 f\n\nAudit\n\n=begin comment\n\nnot prose\n\n=end comment\n\n=cut\n\n\
          sub f { return 1; }\n__END__\n=head1 NAME\n\nData docs\n\n=cut\n",
        [19, 2, 1, 16, 1, 16],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 14, 15, 16, 17, 18],
    ),
    (
        "c05_region_first",
        b"=for comment\nInternal note.\n\n=head1 NAME\n\nFoo - bar\n\n=cut\n",
        [8, 0, 0, 8, 0, 8],
        &[0, 1, 2, 3, 4, 5, 6, 7],
    ),
    (
        "c06_document_then_mixed",
        b"=head1 A\n\na\n\n=cut\n\n=head1 B\n\n=for comment x\n\nb\n\n=cut\n",
        [13, 0, 0, 12, 1, 12],
        &[0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12],
    ),
    (
        "c07_command_in_paragraph",
        b"=head1 A\n\ntext\n=for comment x\n\nmore\n\n=cut\n",
        [8, 0, 0, 8, 0, 8],
        &[0, 1, 2, 3, 4, 5, 6, 7],
    ),
    (
        "w01_stray_cut",
        b"my $x = 1;\n=cut\nmy $y = 2;\n",
        [3, 1, 1, 2, 0, 2],
        &[1, 2],
    ),
    (
        "w02_digit_command",
        b"my $x = 1;\n=1\ntext\n=cut\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "w03_head_dash",
        b"=head-x\n\ntext\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w04_indented",
        b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\n",
        [6, 1, 1, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "w05_same_row",
        b"my $x = 1; =head1 NAME\n\ntext\n\n=cut\n",
        [5, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w06_cutting_opens",
        b"my $x;\n=cutting\ntext\n=cut\nmy $y;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "w07_cutting_closes",
        b"=head1 A\n\ntext\n\n=cutting\nmy $y = 2;\n=cut\n",
        [7, 1, 1, 6, 0, 6],
        &[0, 1, 2, 3, 4, 6],
    ),
    (
        "w08_nul_in_text",
        b"=pod\n\na\x00b\n\n=cut\n",
        [5, 2, 0, 3, 1, 3],
        &[0, 1, 2],
    ),
    (
        "w09_nul_after_cut",
        b"=pod\n\nx\n\n=cut\x00foo\nmy $y;\n",
        [6, 2, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w10_carriage_returns_only",
        b"=pod\r\rx\r\r=cut\rmy $x;\r",
        [1, 0, 0, 1, 0, 1],
        &[0],
    ),
    (
        "w11_carriage_return_in_text",
        b"=pod\n\na\rb\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w12_carriage_return_at_end",
        b"=pod\n\nx\n\n=cut\r",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w13_byte_order_mark",
        b"\xef\xbb\xbf=pod\n\nx\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w14_carriage_return_before",
        b"my $x;\r=pod\n\nx\n\n=cut\n",
        [5, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w15_withheld_then_document",
        b"my $x = 1;\n  =head1 X\n\nx\n\n=cut\n=head1 A\n\na\n\n=cut\n",
        [11, 1, 1, 10, 0, 10],
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    ),
    (
        "w16_underscore_command",
        b"my $x = 1;\n=_x\ntext\n=cut\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "w17_command_with_dot",
        b"=pod.\n\nx\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "w18_stray_cut_first",
        b"=cut\n\nx\n\n=cut\nmy $y;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "r01_error_before",
        b"%%%% (\n=pod\n\nx\n\n=cut\n",
        [6, 1, 0, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "r02_missing_semicolon",
        b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
        [7, 2, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "r03_unclosed_sub",
        b"sub f {\n=pod\n\nx\n\n=cut\n",
        [6, 2, 0, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "r04_error_after",
        b"=pod\n\nx\n\n=cut\n}}}}\n",
        [6, 1, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "r05_return_before",
        b"sub f {\nreturn\n=pod\n\nx\n\n=cut\n1;\n}\n",
        [9, 4, 1, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "s01_hash_comment",
        b"# leads f\nsub f {}\n",
        [2, 1, 0, 1, 0, 0],
        &[],
    ),
    (
        "s02_heredoc",
        b"my $t = <<'EOT';\n=head1 NAME\n\nnot pod\n\n=cut\nEOT\n",
        [7, 7, 1, 0, 0, 0],
        &[],
    ),
    (
        "s03_string",
        b"my $s = \"\n=pod\n\nx\n\n=cut\n\";\nmy $after = 2;\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "s04_word_list",
        b"my @a = qw(\n=pod\n\nx\n\n=cut\n);\nmy $after = 2;\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "s05_pattern",
        b"my $r = qr/\n=pod\n\nx\n\n=cut\n/x;\nmy $after = 2;\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "s06_unclosed_string",
        b"my $s = \"abc\n=pod\n\nx\n\n=cut\n",
        [6, 1, 0, 0, 5, 0],
        &[],
    ),
    (
        "s07_in_list",
        b"my @a = (\n1,\n=pod\n\nx\n\n=cut\n2);\n",
        [8, 4, 1, 0, 4, 0],
        &[],
    ),
    (
        "s08_command_at_end_of_input",
        b"=pod",
        [1, 1, 0, 0, 0, 0],
        &[],
    ),
    (
        "s09_no_semicolon_before",
        b"my $x = 1\n=pod\n\nx\n\n=cut\n;\n",
        [7, 4, 1, 0, 3, 0],
        &[],
    ),
    (
        "d01_end",
        b"__END__\n=head1 NAME\n\nAudit\n\n=cut\n",
        [6, 1, 0, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    ("d02_data", b"__DATA__\nrow\n", [2, 2, 0, 0, 0, 0], &[]),
    (
        "d03_end_at_end_of_input",
        b"__END__",
        [1, 1, 0, 0, 0, 0],
        &[],
    ),
    (
        "d04_end_in_sub",
        b"sub f {\n__END__\n=pod\n\nx\n\n=cut\n",
        [7, 2, 0, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "i01_invalid_utf8",
        b"=head1 A\n\n\xff\n\n=cut\n",
        [5, 0, 0, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    ("e01_empty", b"", [0, 0, 0, 0, 0, 0], &[]),
    (
        "e02_code_only",
        b"sub f { return 1; }\n",
        [1, 1, 1, 0, 0, 0],
        &[],
    ),
    (
        "g01_unterminated_whitespace_rows",
        b"my $x = 1;\n=pod\nabc\n \n \n",
        [5, 1, 1, 4, 0, 4],
        &[1, 2, 3, 4],
    ),
    (
        "g02_stray_cut_then_cut",
        b"my $x = 1;\n=cut\nnot perl at all (\n=cut\nmy $y = 2;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "g03_indented_then_code",
        b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\nmy $y = 2;\n",
        [7, 2, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "g04_digit_then_code",
        b"my $x = 1;\n=1\ntext\n=cut\nmy $y = 2;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "g05_underscore_then_code",
        b"my $x = 1;\n=_x\ntext\n=cut\nmy $y = 2;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "g06_head_dash_then_code",
        b"my $x = 1;\n=head-x\ntext\n=cut\nmy $y = 2;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "g07_cutting_closes",
        b"=head1 A\n\na\n\n=cutting\nmy $y = 2;\n",
        [6, 1, 1, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "g08_cutting_opens_then_code",
        b"my $x = 1;\n=cutting\ntext\n=cut\nmy $y = 2;\n",
        [5, 2, 2, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "g09_cutting_then_garbage",
        b"=head1 A\n\na\n\n=cutting\nnot perl at all (\n=cut\nmy $y = 2;\n",
        [8, 1, 0, 7, 0, 7],
        &[0, 1, 2, 3, 4, 6, 7],
    ),
    (
        "g10_unterminated_whitespace_rows_data",
        b"1;\n__END__\n=pod\nabc\n \n \n",
        [6, 2, 1, 2, 2, 2],
        &[2, 3],
    ),
    (
        "g11_whitespace_line_ends_region",
        b"=for comment x\n \nprose\n=cut\n",
        [4, 0, 0, 4, 0, 4],
        &[0, 1, 2, 3],
    ),
    (
        "g12_digit_in_data_section",
        b"1;\n__END__\n=1\ntext\n=cut\n",
        [5, 5, 1, 0, 0, 0],
        &[],
    ),
    (
        "x01_two_blocks",
        b"my $x = 1;\n=head1 A\n\na\n\n=cut\nmy $y = 2;\n=head1 B\n\nb\n\n=cut\n",
        [12, 2, 2, 10, 0, 10],
        &[1, 2, 3, 4, 5, 7, 8, 9, 10, 11],
    ),
    (
        "r06_empty_tag",
        b"my $s = <<\"\";\n=pod\n\nprint \"MARK\";\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "r07_empty_tag_single",
        b"my $s = <<'';\n=pod\n\nprint \"MARK\";\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "r08_empty_tag_then_real",
        b"my $s = <<\"\";\nlit\n\nprint \"MARK\";\n\n=pod\n\nreal\n\n=cut\n",
        [10, 3, 2, 5, 2, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "r09_control_d",
        b"1;\n\x04\n=pod\n\nd\n\n=cut\n",
        [7, 2, 1, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "r10_control_z",
        b"1;\n\x1a\n=pod\n\nd\n\n=cut\n",
        [7, 2, 1, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "r11_format_at_opener",
        b"format STDOUT =\n=pod\n.\n",
        [3, 1, 0, 2, 0, 2],
        &[1, 2],
    ),
    (
        "r12_error_then_real",
        b"sub isn't { 1 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 1, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "r13_error_between",
        b"=pod\n\nx\n\n=cut\n}}}}\n=pod\n\ny\n\n=cut\n",
        [11, 1, 0, 10, 0, 10],
        &[0, 1, 2, 3, 4, 6, 7, 8, 9, 10],
    ),
    (
        "f01_named",
        b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f02_default_name",
        b"format =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f03_qualified",
        b"format Pkg::F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f04_no_spaces",
        b"format STDOUT=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f05_trailing_blanks",
        b"format STDOUT =  \t\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f06_trailing_comment",
        b"format STDOUT = # c\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 6, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f07_name_next_row",
        b"format\nSTDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [11, 6, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f08_equals_next_row",
        b"format STDOUT\n=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [11, 6, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f09_comments_between",
        b"format # a\nSTDOUT # b\n=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [12, 7, 2, 7, 0, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "f10_indented",
        b"  format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f11_after_statement",
        b"my $q = 1; format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f12_in_sub",
        b"sub g {\nformat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n}\n",
        [12, 7, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f13_carriage_return",
        b"format STDOUT =\r\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f14_after_label",
        b"L: format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f15_no_last_line",
        b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n2;\n",
        [8, 3, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f16_then_real",
        b"format STDOUT =\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
        [10, 4, 1, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f17_real_then_format",
        b"=pod\n\nd\n\n=cut\nformat STDOUT =\n1\n.\n2;\nprint \"MARK\";\n",
        [10, 5, 2, 5, 0, 5],
        &[0, 1, 2, 3, 4],
    ),
    (
        "f18_inside_and_after",
        b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
        [16, 5, 2, 10, 1, 10],
        &[2, 3, 4, 5, 6, 11, 12, 13, 14, 15],
    ),
    (
        "f19_last_line_blanks",
        b"format STDOUT =\n1\n. \t\r\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [11, 5, 2, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f20_crlf",
        b"format STDOUT =\r\n1\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \"\
          MARK\";\r\n",
        [11, 5, 2, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f21_two_formats",
        b"format A =\n1\n.\n2;\nformat B =\n3\n.\n4;\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
        [15, 9, 3, 5, 1, 5],
        &[9, 10, 11, 12, 13],
    ),
    (
        "f22_opener_in_body",
        b"format A =\nformat B =\n1\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [12, 6, 2, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f23_method",
        b"my $o = bless {};\nsub format { 7 }\nmy $v = $o->format;\n\n=pod\n\nd\n\n=cu\
          t\nprint \"MARK\";\n",
        [10, 4, 3, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "f24_hash_key",
        b"my %h = (format => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f25_hash_key_next_row",
        b"my %h = (format\n=> 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [9, 3, 2, 5, 1, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f26_in_string",
        b"my $s = 'format STDOUT =';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f27_in_comment",
        b"# format STDOUT =\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 1, 1, 6, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f28_upper_case",
        b"FORMAT STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f29_subscript",
        b"my %h; $h{format} =\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [9, 3, 3, 5, 1, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f30_quote_in_picture",
        b"format F =\na 'b\n.\nmy $s = ';\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
        [11, 5, 1, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "f31_opener_in_picture",
        b"format F =\n1; print <<EOT;\n.\nmy $s = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
        [12, 6, 2, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f32_excluded_after",
        b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
        [12, 6, 3, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f34_dot_then_text",
        b"format STDOUT =\n1\n. x;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [11, 6, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "f35_indented_dot",
        b"format STDOUT =\n1\n .\n2;\n=pod\n\nd\n\n=cut\n3\n.\n4;\n",
        [12, 7, 2, 5, 0, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "f36_stray_cut_in_format",
        b"format STDOUT =\n1;\n=cut\n1\n.\n2;\n",
        [6, 2, 1, 4, 0, 4],
        &[2, 3, 4, 5],
    ),
    (
        "f37_comment_only_in_format",
        b"format STDOUT =\n1;\n=begin comment\n\nx\n\n=end comment\n\n=cut\n1\n.\n2;\n",
        [12, 5, 2, 7, 0, 0],
        &[],
    ),
    (
        "f33_keyword_like",
        b"forXmat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 5, 2, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h01_in_string",
        b"my $s = \"@{[ <<EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h02_in_string_reference",
        b"my $s = \"${\\ <<EOT }\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h03_in_qq",
        b"my $s = qq{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h04_in_pattern_code",
        b"my $r = 'a' =~ /(?{ <<EOT })/;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h05_in_substitution",
        b"my $t = 'a';\n$t =~ s/a/<<EOT/e;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h06_in_m",
        b"my $r = 'a' =~ m{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h07_in_qr",
        b"my $r = qr{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h08_tilde_tag",
        b"my $s = \"@{[ <<~EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h09_quoted_tag",
        b"my $s = qq{@{[ <<\"EOT\" ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h10_digit_tag",
        b"my $s = qq{@{[ <<1 ]}};\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h11_file_handle",
        b"my $fh = \\*STDOUT;\nprint $fh <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"M\
          ARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h12_file_handle_digit",
        b"my $fh = \\*STDOUT;\nprint $fh <<1;\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h13_file_handle_tilde",
        b"my $fh = \\*STDOUT;\nprint $fh <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h30_block_file_handle",
        b"print {*STDOUT} <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h14_single_quoted",
        b"my $s = 'a <<EOT b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h15_q",
        b"my $s = q{<<EOT};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h16_shift",
        b"my $s = 1<<2;\nmy $t = $s << 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [9, 3, 3, 5, 1, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "h17_arrows_in_string",
        b"my $s = \"<<< x >>> 1 << 2\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h18_word_in_string",
        b"my $s = \"a<<b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h19_then_real",
        b"my $s = qq{@{[ <<\"1;\" ]}};\nlit;\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
        [10, 4, 4, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h20_opener_row_blank",
        b"my $s = <<EOT; \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 4, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h21_crlf",
        b"my $s = <<EOT;\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nEOT\r\nprint \"MARK\";\r\n",
        [8, 4, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h22_late_body",
        b"my $s = <<EOT; \nlit\nEOT\n;\nmy $t = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
        [13, 7, 2, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "h23_end_row_blank",
        b"my $s = <<EOT;\nlit\nEOT \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [10, 5, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "h24_end_row_tab_indented",
        b"my $s = <<~EOT;\nlit\nEOT\t\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [10, 5, 2, 5, 0, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "h25_then_real",
        b"my $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 2, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h26_two_on_a_row",
        b"my @s = (<<EOT, <<EOU);\na\nEOT\nb\nEOU\n\n=pod\n\nd\n\n=cut\nprint \"MARK\"\
          ;\n",
        [12, 6, 2, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "h27_comment_after_opener",
        b"my $s = <<EOT; # c\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 2, 6, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h28_end_row_crlf",
        b"my $s = <<EOT;\nlit\nEOT\r\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 2, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h29_late_then_real",
        b"my $s = <<EOT; \nlit;\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 3, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h31_shift_before_digit",
        b"my $t = 2; my $s = $t <<2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h32_shift_after_blank",
        b"my $t = 2; my $s = $t <<$t;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h33_string_with_blank",
        b"my $s = \"a b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h34_then_code",
        b"my $s = <<EOT;\nlit\nEOT\n\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
        [12, 5, 3, 5, 2, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "h37_in_backticks",
        b"my $s = `@{[ <<EOT ]}`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h38_in_qx",
        b"my $s = qx{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h39_file_handle_bare",
        b"my $fh = \\*STDOUT;\nprint $fh <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h40_file_handle_single_quoted",
        b"my $fh = \\*STDOUT;\nprint $fh <<'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h41_file_handle_backslash",
        b"my $fh = \\*STDOUT;\nprint $fh <<\\EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h42_file_handle_underscore",
        b"my $fh = \\*STDOUT;\nprint $fh <<_E;\n=pod\n\nd\n\n=cut\n_E\nprint \"MARK\";\
          \n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h43_file_handle_command",
        b"my $fh = \\*STDOUT;\nprint $fh <<`EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h44_opener_on_second_row",
        b"my $k = 1;\nmy $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [11, 5, 3, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "h35_operator_in_string",
        b"my $s = \"<<\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h36_empty_tag_in_qq",
        b"my $s = qq{@{[ <<\"\" ]}};\n=pod\n\nprint \"MARK\";\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "q01_escaped_bang",
        b"my $s = q!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q02_escaped_brace",
        b"my $s = q{a\\};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q03_escaped_words",
        b"my @s = qw!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q04_nested_words",
        b"my @s = qw{a{b};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q05_escaped_backslash",
        b"my $s = q{a\\\\};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q06_nested_q",
        b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q07_words",
        b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q08_nested_literals",
        b"my %h; my $s = qq{ $h{qq{ $h{qq{ x }} }} };\n\n=pod\n\nd\n\n=cut\nprint \"MA\
          RK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "t01_heredoc_dq",
        b"my $s = <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t02_heredoc_sq",
        b"my $s = <<'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t03_heredoc_indented",
        b"my $s = <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t04_heredoc_backslash",
        b"my $s = <<\\EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t05_heredoc_in_call",
        b"sub g { 7 }\nmy $s = g(<<EOT);\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [9, 9, 2, 0, 0, 0],
        &[],
    ),
    (
        "t06_heredoc_in_subscript",
        b"my %h; my $s = $h{<<EOT};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 3, 0, 0, 0],
        &[],
    ),
    (
        "t07_heredoc_two",
        b"my @s = (<<EOT, <<EOU);\n=pod\n\nd\n\n=cut\nEOT\n=head1 X\n\n=cut\nEOU\nprin\
          t \"MARK\";\n",
        [12, 12, 2, 0, 0, 0],
        &[],
    ),
    (
        "t08_heredoc_in_heredoc",
        b"my $s = <<EOA;\n@{[ <<EOT ]}\n=pod\n\nd\n\n=cut\nEOT\nEOA\nprint \"MARK\";\n",
        [10, 10, 2, 0, 0, 0],
        &[],
    ),
    (
        "t09_heredoc_print",
        b"print STDOUT <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t10_heredoc_continues",
        b"my %h = (a => <<EOT,\n=pod\n\nd\n\n=cut\nEOT\n  b => 2);\nprint \"MARK\";\n",
        [9, 7, 0, 0, 2, 0],
        &[],
    ),
    (
        "t11_q_brace",
        b"my $s = q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t12_q_paren",
        b"my $s = q(\n=pod\n\nd\n\n=cut\n);\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t13_q_word_delimiter",
        b"my $s = q X\n=pod\n\nd\n\n=cut\nX;\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t14_q_hash",
        b"my $s = q#\n=pod\n\nd\n\n=cut\n#;\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t15_qq_brace",
        b"my $s = qq{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t16_m_brace",
        b"my $s = 'a' =~ m{\n=pod\n\nd\n\n=cut\n}x;\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t17_qr_slash",
        b"my $s = qr/\n=pod\n\nd\n\n=cut\n/x;\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t18_s_pattern",
        b"my $s = 'a'; $s =~ s/\n=pod\n\nd\n\n=cut\n/b/x;\nprint \"MARK\";\n",
        [8, 8, 3, 0, 0, 0],
        &[],
    ),
    (
        "t19_s_replacement",
        b"my $s = 'a'; $s =~ s{a}{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 8, 3, 0, 0, 0],
        &[],
    ),
    (
        "t20_tr",
        b"my $s = 'a'; $s =~ tr/a/\n=pod\n\nd\n\n=cut\n/;\nprint \"MARK\";\n",
        [8, 8, 3, 0, 0, 0],
        &[],
    ),
    (
        "t21_y",
        b"my $s = 'a'; $s =~ y/a/\n=pod\n\nd\n\n=cut\n/;\nprint \"MARK\";\n",
        [8, 8, 3, 0, 0, 0],
        &[],
    ),
    (
        "t22_single_quoted",
        b"my $s = '\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t23_backticks",
        b"my $s = `\n=pod\n\nd\n\n=cut\n`;\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t24_qx",
        b"my $s = qx{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t25_heredoc_command",
        b"my $s = <<`EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "t26_end_same_row",
        b"1; __END__\n=pod\n\nd\n\n=cut\n",
        [6, 1, 1, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "t27_end_as_key",
        b"my %h = (__END__ => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 0, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "t28_format_then_pod",
        b"format STDOUT =\nline\n.\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [9, 7, 1, 0, 2, 0],
        &[],
    ),
    (
        "t29_format_tick_name",
        b"format Pkg'F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
        [10, 1, 0, 0, 9, 0],
        &[],
    ),
    (
        "t30_bareword_assignment",
        b"my $y\n=pod\n;\nprint \"MARK\";\n",
        [4, 4, 2, 0, 0, 0],
        &[],
    ),
    (
        "n01_source_filter",
        b"use Filter::Util::Call;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "r14_hidden_block",
        b"my $s = q'a\\'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 1, 0, 0, 7, 0],
        &[],
    ),
    (
        "r15_command_before_data",
        b"my $x = (1;\nmy $s = \"\n=pod\n\";\n__END__\n=pod\n\nd\n\n=cut\n",
        [10, 5, 2, 5, 0, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "r16_command_in_data_only",
        b"my $x = (1;\n__END__\n=pod\n\nd\n\n=cut\n",
        [7, 2, 1, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "r17_command_before_error",
        b"my $s = '\n=pod\n';\nmy $x = (1;\n",
        [4, 4, 2, 0, 0, 0],
        &[],
    ),
    (
        "r18_no_command_line",
        b"my $x = (1;\nmy $s = \"\n=1\n==\n= a\n =pod\nx=pod\n\";\n",
        [8, 8, 2, 0, 0, 0],
        &[],
    ),
    (
        "r19_error_without_pod",
        b"my $x = (1;\nmy $y = 2;\n",
        [2, 2, 2, 0, 0, 0],
        &[],
    ),
    (
        "r21_block_with_a_node",
        b"my $x = (1;\n\n=head1 A\n\na\n\n=cut\n",
        [7, 1, 1, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "f38_value_heredoc",
        b"format F =\n@x;\nqq{@{[ <<TAG ]}};\n1\n.\n2;\n=pod\n\nd\n\n=cut\nTAG\n.\n2;\
          \n",
        [14, 9, 4, 5, 0, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f39_value_escaped_q",
        b"format F =\n@x;\nq!abc\\!;\n1\n.\n2;\n=pod\n\nd\n\n=cut\n!\n.\n2;\n",
        [14, 9, 4, 5, 0, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f40_value_list",
        b"format F =\n@x;\n$a, $b\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [12, 6, 3, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f41_dot_where_values",
        b"format F =\n@x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [11, 5, 2, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f42_value_string",
        b"format F =\n@x;\n\"a\"\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [12, 6, 3, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f43_value_brackets",
        b"format F =\n@x;\n[ $a,\n$b ]\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [13, 7, 3, 5, 1, 5],
        &[7, 8, 9, 10, 11],
    ),
    (
        "f44_tilde_fields",
        b"format F =\n~~@x;\n$a\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [12, 6, 3, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f45_field_in_comment",
        b"format F =\n1\n# @x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [12, 5, 2, 6, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f46_values_then_picture",
        b"format F =\n@x;\n$a,\n@y;\n$b;\ntext\n. \n2;\n\n=pod\n\nd\n\n=cut\nprint \"M\
          ARK\";\n",
        [15, 9, 5, 5, 1, 5],
        &[9, 10, 11, 12, 13],
    ),
    (
        "f47_value_dot_blank",
        b"format F =\n@x\n. \n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [11, 5, 2, 5, 1, 5],
        &[5, 6, 7, 8, 9],
    ),
    (
        "f48_value_crlf",
        b"format F =\r\n@x;\r\n$a\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \
          \"MARK\";\r\n",
        [12, 6, 3, 5, 1, 5],
        &[6, 7, 8, 9, 10],
    ),
    (
        "f49_method_assignment",
        b"my $o = bless {};\n$o->format =\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 3, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "f50_glob_assignment",
        b"sub g { 7 }\n*format =\n\\&g;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [10, 4, 2, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "h45_block_handle_blanks",
        b"my $fh = \\*STDOUT;\nprint {$fh} <<  \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h46_bare_handle_empty_tag",
        b"print STDOUT << \"\";\n=pod\n\nprint \"MARK\";\n",
        [4, 1, 1, 3, 0, 3],
        &[1, 2, 3],
    ),
    (
        "h47_glob_handle_tabs",
        b"print {*STDOUT} <<\t\t'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "h52_block_handle_command",
        b"my $fh = \\*STDOUT;\nprint {$fh} << `EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"\
          MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h48_scalar_handle_blank",
        b"my $fh = \\*STDOUT;\nprint $fh << \"EOT\";\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
        [9, 3, 3, 5, 1, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "h49_tilde_blank",
        b"my $fh = \\*STDOUT;\nprint {$fh} <<~ \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h50_shift_by_bareword",
        b"my $t = 2; my $x = $t <<EOT;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "h51_shift_after_blanks",
        b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q09_escaped_open_then_string",
        b"my $x = q(a\\(b); my $s = \"x);\n=pod\n\nd\n\n=cut\n\";\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q10_tr_escaped_open",
        b"my $t = 'a';\n$t =~ tr{a\\{}{b}; my $s = \"x}{y};\n=pod\n\nd\n\n=cut\n\";\np\
          rint \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q11_y_escaped_open",
        b"my $t = 'a';\n$t =~ y(a\\()(b); my $s = \"x)(y);\n=pod\n\nd\n\n=cut\n\";\npr\
          int \"MARK\";\n",
        [9, 4, 3, 5, 0, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q12_escaped_open_closed",
        b"my $x = q(a\\(b); my $s = \"x)\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q13_two_parts_with_space",
        b"my $t = 'a';\n$t =~ s{a}\n  {b}g; $t =~ tr{a} {b};\n\n=pod\n\nd\n\n=cut\npri\
          nt \"MARK\";\n",
        [10, 4, 4, 5, 1, 5],
        &[4, 5, 6, 7, 8],
    ),
    (
        "q14_hash_delimiter",
        b"my $s = q#a#;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q15_quote_delimiter",
        b"my $s = q'ab'; my $m = 'a' =~ m'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q16_flags",
        b"my $t = 'a';\n$t =~ s/a/b/gi; $t =~ tr/a/b/r; my $m = $t =~ m{a}x;\n\n=pod\n\
          \nd\n\n=cut\nprint \"MARK\";\n",
        [9, 3, 5, 5, 1, 5],
        &[3, 4, 5, 6, 7],
    ),
    (
        "q17_escaped_in_evaluated",
        b"my $s = qq{a\\}b}; my $r = qr(a\\)b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q18_core_operator",
        b"my $x = CORE::q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
        [8, 3, 2, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q19_division",
        b"my $z = time / 2; my $s = \"/; 1; \"; my $u = \";\n=pod\n\nd\n\n=cut\n\";\n# \
          \"\nprint \"MARK\";\n",
        [9, 4, 4, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q20_old_separator",
        b"my $x = Foo'bar(1); my $s = '; 1; '; my $u = ';\n=pod\n\nd\n\n=cut\n';\n# '\
          \nprint \"MARK\";\n",
        [9, 4, 4, 5, 0, 5],
        &[1, 2, 3, 4, 5],
    ),
    (
        "q21_word_before_arrow",
        b"my %k = (y => 1, s => 2, q => 3);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q22_pattern_after_keyword",
        b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 4, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q23_pattern_after_word",
        b"my $m = lc /A/;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q24_plain_division",
        b"my $z = 4 / 2; my $w = $z / 2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q25_word_in_subscript",
        b"my %k; my $v = $k{q}; my $w = { 1 };\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 3, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "q26_short_identifier",
        b"my $v = 1; sub go { 7 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 2, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
    (
        "t31_data_as_key",
        b"my %h = (__DATA__ => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        [8, 2, 0, 5, 1, 5],
        &[2, 3, 4, 5, 6],
    ),
];

#[cfg(feature = "perl")]
fn perl_loc(source: &[u8]) -> ([u64; 6], Vec<usize>) {
    let space = analyze(
        Source::new(LANG::Perl, source),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .unwrap();
    let loc = &space.metrics.loc;
    (
        [
            loc.sloc(),
            loc.ploc(),
            loc.lloc(),
            loc.cloc(),
            loc.blank(),
            loc.dloc(),
        ],
        (0..=source.len())
            .filter(|row| loc.dloc.lines.contains(*row))
            .collect(),
    )
}

#[cfg(feature = "perl")]
#[test]
fn documentation_perl_pod_legacy_rows() {
    for &(name, source, loc, rows) in &PERL_POD_SOURCES {
        assert_eq!(perl_loc(source), (loc, rows.to_vec()), "{name}");
    }
}

#[cfg(feature = "perl")]
#[test]
fn documentation_perl_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::{
        Content, DocumentationContext, DocumentationOptions, observations,
    };
    let counters = || {
        let seen = observations();
        [
            seen.visits,
            seen.renders,
            seen.owner_projections,
            seen.polls,
        ]
    };
    let (mut documents, mut texts, mut pending) = (0, 0, 0);
    for &(name, source, loc, rows) in &PERL_POD_SOURCES {
        let parser =
            crate::langs::AnyParser::parse(LANG::Perl, source.to_vec(), None, None).unwrap();
        let idle = counters();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = counters();
        let complete = index
            .documents()
            .filter(|document| matches!(document.get().content(), Content::Complete { .. }))
            .count();
        let [visits, renders, owners, polls] = std::array::from_fn(|at| collected[at] - idle[at]);
        // expected: the walk enters every node once.
        assert_eq!(visits, parser.root_node().preorder().count(), "{name}");
        // expected: one render for each document that has a complete text.
        assert_eq!(renders, complete, "{name}");
        // expected: the file is the only owner.
        assert_eq!(owners, 1, "{name}");
        // The checkpoint total has no closed form over these sources. Every collection
        // makes the seven explicit checkpoints of one without a document.
        assert!(polls >= 7, "{name}: {polls}");
        documents += index.documents().len();
        texts += complete;
        pending += index.assessments().len();

        assert_eq!(perl_loc(source), (loc, rows.to_vec()), "{name}");
        // expected: the metric walk enters no collector node, renders no text, projects
        // no owner and polls no budget.
        assert_eq!(counters(), collected, "{name}");
    }
    // expected: over the 261 sources, 93 blocks are documents, 87 of them with a
    // complete text (five have pending text, one is no UTF-8). The reader's own
    // tests withhold 29 blocks and the rules for literal text 92; five documents have
    // pending text, 12 data sections are reported, and 10 files are marked for POD
    // that the grammar may hide: 148 assessments.
    assert_eq!((documents, texts, pending), (93, 87, 148));
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[test]
fn documentation_js_variable_projection_does_not_change_legacy_rows() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    const BOTH: &[LANG] = &[LANG::Javascript, LANG::Mozjs];
    let rows: &[(&[u8], [u64; 6], &[usize])] = &[
        (
            b"/**audit*/\nconst f = () => 1;\n",
            [2, 1, 1, 1, 0, 1],
            &[0],
        ),
        (
            b"/**audit*/\nconst a = 1, b = 2;\n",
            [2, 1, 1, 1, 0, 1],
            &[0],
        ),
        (
            b"/**audit*/\nexport const e = 3;\n",
            [2, 1, 1, 1, 0, 1],
            &[0],
        ),
        (b"/**audit*/\nusing u = f();\n", [2, 1, 1, 1, 0, 1], &[0]),
    ];
    for &language in BOTH {
        if !language.is_enabled() {
            continue;
        }
        for &(source, expected, dloc_rows) in rows {
            let parser =
                crate::langs::AnyParser::parse(language, source.to_vec(), None, None).unwrap();
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
            assert_eq!(index.documents().len(), 1);
            // expected: one collector visit per grammar node, one rendered selected block,
            // one source marker, the file, and exactly one first-declarator projection.
            assert_eq!(
                collected.visits - idle.visits,
                parser.root_node().preorder().count()
            );
            assert_eq!(collected.renders - idle.renders, 1);
            assert_eq!(built - unbuilt, 1);
            assert_eq!(collected.owner_projections - idle.owner_projections, 2);
            let space = analyze(
                Source::new(language, source),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            let analyzed = observations();
            // expected: legacy analysis builds no collector index or rendered source evidence.
            assert_eq!(analyzed.visits - collected.visits, 0);
            assert_eq!(analyzed.renders - collected.renders, 0);
            assert_eq!(analyzed.owner_projections - collected.owner_projections, 0);
            assert_eq!(evidence_built::observed() - built, 0);
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
                expected
            );
            assert_eq!(
                (0..source.len())
                    .filter(|at| loc.dloc.lines.contains(*at))
                    .collect::<Vec<_>>(),
                dloc_rows
            );
        }
    }
}
