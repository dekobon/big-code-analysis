//! Legacy preservation on raw inputs; source owner/text requirements live in AST fixtures.

use big_code_analysis_ast::test_support::documentation;
use serde_json::{Value, json};

use crate::{LANG, Metric, MetricsOptions, Source, analyze};

#[cfg(feature = "kotlin")]
#[test]
fn documentation_kotlin_property_slots_leave_legacy_rows_unchanged() {
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    for (source, loc_expected, rows_expected) in [
        (
            b"class C {\n    /** d */\n    val p = 1\n}\n".as_slice(),
            [4, 3, 1, 1, 0, 1],
            &[1][..],
        ),
        (
            b"/** d */\nval x = 1\n".as_slice(),
            [2, 1, 1, 1, 0, 1],
            &[0][..],
        ),
    ] {
        let parser =
            crate::langs::AnyParser::parse(LANG::Kotlin, source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error());
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after_collection = observations();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(
            index
                .complete_text(index.documents().next().unwrap())
                .unwrap()
                .unwrap()
                .text(),
            "d"
        );
        assert_eq!(
            after_collection.visits - before.visits,
            parser.root_node().preorder().count()
        );
        assert_eq!(after_collection.renders - before.renders, 1);
        let space = analyze(
            Source::new(LANG::Kotlin, source),
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
            loc_expected
        );
        assert_eq!(
            (0..source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            rows_expected
        );
        let after_analysis = observations();
        assert_eq!(after_analysis.visits, after_collection.visits);
        assert_eq!(after_analysis.renders, after_collection.renders);
    }
}

#[cfg(feature = "java")]
#[test]
fn documentation_java_package_and_forms_keep_legacy_rows() {
    use big_code_analysis_ast::{AnyParser, documentation::*};
    for (source, loc, rows, documents) in [
        (
            &b"/**pkg*/\npackage p;\nclass C {}\n"[..],
            [3, 2, 0, 1, 0, 0],
            &[][..],
            1,
        ),
        (
            include_bytes!("../../../tests/fixtures/documentation/sources/java-selection.java"),
            [5, 1, 0, 4, 0, 3],
            &[0, 1, 2],
            1,
        ),
        (
            include_bytes!("../../../tests/fixtures/documentation/sources/java-empty.java"),
            [2, 1, 0, 1, 0, 0],
            &[],
            1,
        ),
        (
            include_bytes!("../../../tests/fixtures/documentation/sources/java-record.java"),
            [2, 1, 0, 1, 0, 1],
            &[0],
            1,
        ),
        (
            include_bytes!("../../../tests/fixtures/documentation/sources/java-compact.java"),
            [2, 1, 0, 1, 0, 1],
            &[0],
            1,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let idle = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        assert_eq!(index.documents().len(), documents);
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count()
        );
        assert_eq!(collected.renders - idle.renders, documents);
        let space = analyze(
            Source::new(LANG::Java, source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = observations();
        let found = &space.metrics.loc;
        assert_eq!(
            [
                found.sloc(),
                found.ploc(),
                found.lloc(),
                found.cloc(),
                found.blank(),
                found.dloc()
            ],
            loc
        );
        assert_eq!(
            (0..source.len())
                .filter(|at| found.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            rows
        );
        assert_eq!(analyzed.visits - collected.visits, 0);
        assert_eq!(analyzed.renders - collected.renders, 0);
    }
}

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
                    &DocumentationOptions::default().with_unbound_evidence(true),
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
                &DocumentationOptions::default().with_unbound_evidence(true),
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

/// Every Tcl source of the Ruff documentation tests of the AST crate, with the tuple
/// `[sloc, ploc, lloc, cloc, blank, dloc]` and the comment rows, counted from 0, that the
/// metric gave it before any of those documents existed.
#[cfg(feature = "tcl")]
static TCL_RUFF_SOURCES: [(&[u8], [u64; 6], &[usize]); 327] = [
    // a01_primary
    (b"proc f {} {\n# audit\nreturn\n}", [4, 3, 2, 1, 0, 0], &[1]),
    // a02_primary_final_lf
    (b"proc f {} {\n# audit\nreturn\n}\n", [4, 3, 2, 1, 0, 0], &[1]),
    // b01_leading_blank_rows
    (b"proc f {} {\n\n\n# audit\n# more\nreturn\n}\n", [7, 3, 2, 2, 2, 0], &[3, 4]),
    // b02_blank_row_ends
    (b"proc f {} {\n# first\n\n# second\nreturn\n}\n", [6, 3, 2, 2, 1, 0], &[1, 3]),
    // b03_space_row_ends
    (b"proc f {} {\n# a\n   \n# b\nreturn\n}\n", [6, 3, 2, 2, 1, 0], &[1, 3]),
    // b04_command_ends
    (b"proc f {} {\n# first\nset x 1\n# plain\n# later\n}", [6, 3, 2, 3, 0, 0], &[1, 3, 4]),
    // b05_indented
    (b"proc f {} {\n    # a\n    # b\n    return\n}\n", [5, 3, 2, 2, 0, 0], &[1, 2]),
    // b06_brace_row
    (b"proc f {} { # audit\nreturn\n}\n", [3, 3, 2, 1, 0, 0], &[0]),
    // b07_brace_row_no_space
    (b"proc f {} {# audit\n# more\nreturn\n}\n", [4, 3, 2, 2, 0, 0], &[0, 1]),
    // b08_empty_comment_row
    (b"proc f {} {\n# a\n#\n# b\nreturn\n}\n", [6, 3, 2, 3, 0, 0], &[1, 2, 3]),
    // b09_bare_backslash_row
    (b"proc f {} {\n# a\n\\\n# b\nreturn\n}\n", [6, 3, 2, 3, 0, 0], &[1, 2, 3]),
    // b10_only_comment
    (b"proc f {} {\n# a\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // b11_command_first
    (b"proc f {} {\nset x 1\n# later\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // m01_contract
    (b"proc f {} {\n# first\nset x 1\n#ruff\n# later\n}", [6, 3, 2, 3, 0, 0], &[1, 3, 4]),
    // m02_marker_with_text
    (b"proc f {} {\n# first\nset x 1\n#ruff later\n}", [5, 3, 2, 2, 0, 0], &[1, 3]),
    // m03_marker_in_plain_run
    (b"proc f {} {\n# first\nset x 1\n# plain\n#ruff\n# later\n}\n", [7, 3, 2, 4, 0, 0], &[1, 3, 4, 5]),
    // m04_marked_block_only
    (b"proc f {} {\nset x 1\n#ruff\n# later\n}\n", [5, 3, 2, 2, 0, 0], &[2, 3]),
    // m05_two_marked_blocks
    (b"proc f {} {\nset x 1\n#ruff a\nset y 2\n#ruff b\n# c\n}\n", [7, 4, 3, 3, 0, 0], &[2, 4, 5]),
    // m06_marker_in_lead
    (b"proc f {} {\n# a\n#ruff b\n# c\nreturn\n}\n", [6, 3, 2, 3, 0, 0], &[1, 2, 3]),
    // m07_marker_first_row
    (b"proc f {} {\n#ruff\n# a\n\n# b\n}\n", [6, 2, 1, 3, 1, 0], &[1, 2, 4]),
    // m08_marker_after_blank_row
    (b"proc f {} {\n# a\n\n#ruff b\n}\n", [5, 2, 1, 2, 1, 0], &[1, 3]),
    // m10_row_text
    (b"proc f {} {\n## doubled\n#x\n#  two\n# trail  \n#ruff  w\n#ruff \n#end\n}\n", [9, 2, 1, 7, 0, 0], &[1, 2, 3, 4, 5, 6, 7]),
    // m13_tab_separators
    (b"proc f {} {\n#\ty\n#ruff\tz\n#ruff\t w\n}\n", [5, 2, 1, 3, 0, 0], &[1, 2, 3]),
    // m11_empty_boundary_rows
    (b"proc f {} {\n#ruff\n# a\n#ruff\n}\n", [5, 2, 1, 3, 0, 0], &[1, 2, 3]),
    // m12_only_empty_row
    (b"proc f {} {\n#\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // m14_empty_row_then_blank_row
    (b"proc f {} {\n#\n\n# after\n}\n", [5, 2, 1, 2, 1, 0], &[1, 3]),
    // o01_options_in_and_after_block
    (b"proc f {} {\n# a\n#ruffopt includedformats html\n# b\nset x 1\n# plain\n#ruffopt excludedformats {html}\n}\n", [8, 3, 2, 5, 0, 0], &[1, 2, 3, 5, 6]),
    // o02_options_only
    (b"proc f {} {\n#ruffopt includedformats html\nreturn\n}\n", [4, 3, 2, 1, 0, 0], &[1]),
    // o03_options_before_lead
    (b"proc f {} {\n#ruffopt a b\n\n# lead\n}\n", [5, 2, 1, 2, 1, 0], &[1, 3]),
    // o04_options_blank_then_row
    (b"proc f {} {\n# a\n#ruffopt x y\n\n# b\n}\n", [6, 2, 1, 3, 1, 0], &[1, 2, 4]),
    // o07_blank_row_then_options
    (b"proc f {} {\n# a\n\n#ruffopt x y\n# b\n}\n", [6, 2, 1, 3, 1, 0], &[1, 3, 4]),
    // o05_longer_word
    (b"proc f {} {\n#ruffoptions x\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // o06_options_after_semicolon
    (b"proc f {} {\nset x 1 ;#ruffopt a b\n}\n", [3, 3, 2, 1, 0, 0], &[1]),
    // m20_other_words_in_lead
    (b"proc f {} {\n#rufftail x\n# ruff spaced\n#ruffian\n}\n", [5, 2, 1, 3, 0, 0], &[1, 2, 3]),
    // m21_other_words_after_command
    (b"proc f {} {\n# a\nset x 1\n#rufftail\n# ruff\n#ruff\x0cff\n}\n", [7, 3, 2, 4, 0, 0], &[1, 3, 4, 5]),
    // b12_after_semicolon
    (b"proc f {} {\n# a\nset x 1 ;# after\n;# lead semi\n#ruff m\n}\n", [6, 4, 2, 4, 0, 0], &[1, 2, 3, 4]),
    // b13_marker_after_semicolon
    (b"proc f {} {\nset x 1\n;#ruff x\n}\n", [4, 4, 2, 1, 0, 0], &[2]),
    // b20_nested_marker_rows
    (b"proc f {} {\n# lead\nif {$x} {\n#ruff nested\n}\nputs {\n#ruff string\n}\n}\n", [9, 6, 3, 3, 0, 0], &[1, 3, 6]),
    // b21_nested_marker_only
    (b"proc f {} {\nif {$x} {\n#ruff nested\n}\n}\n", [5, 4, 2, 1, 0, 0], &[2]),
    // c01_continued_row_text
    (b"proc f {} {\n# audit \\\nmore text\nreturn\n}\n", [5, 4, 3, 1, 0, 0], &[1]),
    // c02_continued_row_comment
    (b"proc f {} {\n# a \\\n# b\nreturn\n}\n", [5, 3, 2, 2, 0, 0], &[1, 2]),
    // c03_even_run
    (b"proc f {} {\n# a \\\\\n# b\n}\n", [4, 2, 1, 2, 0, 0], &[1, 2]),
    // c04_marker_below_continued_plain
    (b"proc f {} {\nset x 1\n# plain \\\n#ruff\n# later\n}\n", [6, 3, 2, 3, 0, 0], &[2, 3, 4]),
    // c05_marker_below_bare_rows
    (b"proc f {} {\nset x 1\n# plain \\\n\\\n#ruff x\n}\n", [6, 3, 2, 3, 0, 0], &[2, 3, 4]),
    // c06_blank_row_ends_the_line
    (b"proc f {} {\nset x 1\n# plain \\\n\n#ruff x\n}\n", [6, 3, 2, 2, 1, 0], &[2, 4]),
    // c07_continued_row_crlf
    (b"proc f {} {\r\n# audit \\\r\nreturn\r\n}\r\n", [4, 3, 2, 1, 0, 0], &[1]),
    // c08_continued_options_row
    (b"proc f {} {\n#ruffopt a \\\n#ruff\n# later\n}\n", [5, 2, 1, 3, 0, 0], &[1, 2, 3]),
    // a03_crlf
    (b"proc f {} {\r\n# audit\r\n#ruff\r\n# more\r\nreturn\r\n}\r\n", [6, 3, 2, 3, 0, 0], &[1, 2, 3]),
    // a04_lone_cr_in_comment
    (b"proc f {} {\n# a\rb\nreturn\n}\n", [4, 3, 2, 1, 0, 0], &[1]),
    // k01_stray_bracket
    (b"proc f {} {\n# audit\nputs ]\nreturn\n}\nproc g {} {\n# doc g\n}\n", [8, 6, 4, 2, 0, 0], &[1, 6]),
    // k02_semicolon_after_word
    (b"proc f {} {\n# a\nreturn;# done\n#ruff b\n}\n", [5, 3, 1, 3, 0, 0], &[1, 2, 3]),
    // k03_recovery_after_brace
    (b"proc f {} {;# c\nreturn\n}\n", [3, 3, 2, 1, 0, 0], &[0]),
    // k04_missing_token_in_command
    (b"proc f {} {\n# a\nset x 1; set y 2\n#ruff b\n}\n", [5, 3, 3, 2, 0, 0], &[1, 3]),
    // k08_recovery_after_command
    (b"proc f {} {\n# a\n# b\nset x 1 # text\n}\n", [5, 3, 2, 3, 0, 0], &[1, 2, 3]),
    // k12_recovery_row
    (b"proc f {} {\n# a\n]\n# b\n}\n", [5, 3, 1, 2, 0, 0], &[1, 3]),
    // k09_missing_bracket_in_command
    (b"proc f {} {\n# a\nputs [x\n}\nproc g {} {\n# b\n}\n", [7, 5, 3, 2, 0, 0], &[1, 5]),
    // k05_recovery_holds_lead
    (b"proc f {} {\n# a\nset x 1 # text\n#ruff\n# later\n}\n", [6, 4, 2, 4, 0, 0], &[1, 2, 3, 4]),
    // k06_nul_in_first_row
    (b"proc f {} {\n# a\x00b\nreturn\n}\n", [4, 4, 3, 1, 0, 0], &[1]),
    // k07_nul_in_second_row
    (b"proc f {} {\n# a\n# b\x00c\nreturn\n}\n", [5, 4, 1, 2, 0, 0], &[1, 2]),
    // k10_recovery_holds_marker_row
    (b"proc f {} {\n# lead\nset x 1\n#ruff a\nset y 2 # text\n#ruff c\n}\n", [7, 4, 3, 4, 0, 0], &[1, 3, 4, 5]),
    // k11_nul_in_marker_row
    (b"proc f {} {\n# lead\nset x 1\n#ruff b\x00c\nreturn\n}\n", [6, 5, 2, 2, 0, 0], &[1, 3]),
    // k13_recovery_on_continued_row
    (b"proc f {} {\nif {1} {} \\\n#ruff b\n}\n", [4, 4, 2, 1, 0, 0], &[2]),
    // k14_directive_then_cut_comment
    (b"proc f {} {\n#ruffopt a b\n# a\x00b\nreturn\n}\n", [5, 4, 1, 2, 0, 0], &[1, 2]),
    // n01_qualified_name
    (b"proc ::ns::f {} {\n# n\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n02_arguments_word
    (b"proc f args {\n# a\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n03_variable_name
    (b"proc $name {} {\n# dyn\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n04_command_name
    (b"proc [mk] {} {\n# dyn\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n05_quoted_name
    (b"proc \"g\" {} {\n# q\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n06_braced_name
    (b"proc {h} {} {\n# b\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // n10_name_of_two_parts
    (b"proc a$b {} {\n# c\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n11_expansion
    (b"proc {*}$spec\nproc g {} {\n# doc g\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // n12_quoted_body
    (b"proc f {} \"\n# q\n\"\n", [3, 3, 1, 0, 0, 0], &[]),
    // n13_word_body
    (b"proc f {} return\n", [1, 1, 1, 0, 0, 0], &[]),
    // n14_missing_body
    (b"proc f {}\nproc g {} {\n# doc g\nreturn\n}\n", [5, 4, 3, 1, 0, 0], &[2]),
    // n15_malformed_arguments
    (b"proc a {} {\n# doc a\nreturn\n}\nproc bad {x {\n# doc bad\n}\nproc c {} {\n# doc c\nreturn\n}\n", [11, 9, 4, 2, 0, 0], &[1, 8]),
    // n16_recovery_in_name
    (b"proc [a;] {} {\n# c\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n17_recovery_in_arguments
    (b"proc f {a;} {\n# c\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n18_brace_in_name
    (b"proc f{ {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // n19_quoted_word_in_arguments
    (b"proc f {a \"b\"} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // p01_continued_comment
    (b"# c \\\nproc g {} {\n# inner\n}\nproc h {} {\n# doc h\n}\n", [7, 4, 2, 3, 0, 0], &[0, 2, 5]),
    // p02_bare_rows
    (b"# c \\\n\\\nproc g {} {\n# inner\n}\n", [5, 2, 1, 3, 0, 0], &[0, 1, 3]),
    // p03_blank_row_ends_line
    (b"# c \\\n\nproc g {} {\n# inner\n}\n", [5, 2, 1, 2, 1, 0], &[0, 3]),
    // p04_even_run
    (b"# c \\\\\nproc g {} {\n# inner\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p05_crlf_bare_row
    (b"# c \\\r\n\\\r\nproc g {} {\r\n# inner\r\n}\r\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // p06_crlf_continued_command
    (b"foo a \\\r\nproc g {} {\r\n# inner\r\n}\r\n", [4, 3, 2, 1, 0, 0], &[2]),
    // p16_continued_comment_then_command
    (b"# c \\\nJUNK\nproc g {} {\n# inner\n}\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // p17_plain_comment_then_command
    (b"# c\nJUNK\nproc g {} {\n# inner\n}\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // p19_continued_comment_alone
    (b"# c \\\nproc g {} {\n# inner\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p18_continued_comment_dynamic_name
    (b"# c \\\nproc $name {} {\n# x\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p20_semicolon_on_continued_row
    (b"# c \\\n;proc f {} {\n# doc\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p21_command_on_continued_row
    (b"# c \\\nputs a ; proc g {} {\n# doc\n}\n", [4, 2, 2, 2, 0, 0], &[0, 2]),
    // p22_two_continued_rows
    (b"# c \\\nset x 1 \\\n; proc g {} {\n# b\n}\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // p23_words_on_continued_rows
    (b"# c \\\nJUNK \\\nbar ; proc g {} {\n# b\n}\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // p24_continued_command_then_semicolon
    (b"set x 1 \\\n; proc g {} {\n# b\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // p25_semicolon_on_continued_row_crlf
    (b"# c \\\r\n;proc g {} {\r\n# b\r\n}\r\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p26_two_procedures_on_continued_row
    (b"# c \\\nproc a {} {\n# a\n};proc b {} {\n# b\n}\n", [6, 3, 2, 3, 0, 0], &[0, 2, 4]),
    // p27_two_procedures_on_one_row
    (b"# c\nproc a {} {\n# a\n};proc b {} {\n# b\n}\n", [6, 3, 2, 3, 0, 0], &[0, 2, 4]),
    // p28_continued_comment_after_semicolon
    (b";# c \\\nproc g {} {\n# b\n}\n", [4, 3, 1, 2, 0, 0], &[0, 2]),
    // p29_plain_comment_then_semicolon
    (b"# c\n;proc g {} {\n# b\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // p07_slots_cross_a_line
    (b"proc f\n{} {\n# audit\n}\nproc g {} {\n# doc g\n}\n", [7, 5, 2, 2, 0, 0], &[2, 5]),
    // p08_backslash_newline_between_slots
    (b"proc f {} \\\n{\n# audit\n}\n", [4, 3, 1, 1, 0, 0], &[2]),
    // p09_extra_word
    (b"proc f {} {\n# audit\n} extra\nproc g {} {\n# doc g\n}\n", [6, 4, 3, 2, 0, 0], &[1, 4]),
    // p10_comment_after_body
    (b"proc f {} {\n# audit\n} # trailing\n", [3, 2, 1, 2, 0, 0], &[1, 2]),
    // p11_semicolon_after_body
    (b"proc f {} {\n# audit\n};proc g {} {\n# doc g\n}\n", [5, 3, 2, 2, 0, 0], &[1, 3]),
    // p12_two_procedures_one_row
    (b"proc f {} {\n# a\n} proc g {} {\n# b\n}\n", [5, 3, 2, 2, 0, 0], &[1, 3]),
    // p13_command_then_procedure
    (b"set x 1 proc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // p14_semicolon_then_procedure
    (b"set x 1 ; proc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // p15_spaces_at_end_of_input
    (b"proc f {} {\n# a\n}   ", [3, 2, 1, 1, 0, 0], &[1]),
    // s01_namespace
    (b"namespace eval n {\nproc f {} {\n# audit\nreturn\n}\n}\n", [6, 5, 3, 1, 0, 0], &[2]),
    // s02_procedure_in_procedure
    (b"proc outer {} {\n# outer doc\nproc inner {} {\n# inner doc\n}\n}\n", [6, 4, 2, 2, 0, 0], &[1, 3]),
    // s03_under_inner_error
    (b"proc f {} {\n# doc f\nset x 1\n\nproc g {} {\n# doc g\nreturn 2\n}\n", [8, 5, 3, 2, 1, 0], &[1, 5]),
    // s05_nested_dynamic_name
    (b"namespace eval n {\nproc $name {} {\n# x\n}\n}\n", [5, 4, 2, 1, 0, 0], &[2]),
    // s04_root_error
    (b"proc f {{a0 { {a1 { {a2 { {a3 {} {\n# audit\n}\nproc g {} {\n# doc g\n}\n", [6, 4, 2, 2, 0, 0], &[1, 4]),
    // s08_root_error_after_the_procedure
    (b"proc g {} {\n# doc g\n}\nproc h {a} {set s  \"\"\"{\"{$a(proc k {} {\n\n\"\"{\"", [6, 4, 1, 1, 1, 0], &[1]),
    // a06_two_documented
    (b"proc f {} {\n# doc f\nreturn 1\n}\nproc g {a b} {\n# doc g\nreturn 2\n}\n", [8, 6, 4, 2, 0, 0], &[1, 5]),
    // a07_one_undocumented
    (b"proc f {} {\nreturn 1\n}\nproc g {} {\n# doc g\nreturn 2\n}\n", [7, 6, 4, 1, 0, 0], &[4]),
    // p31_command_below
    (b"proc g {} {\n# inner\n}\nJUNK\n", [4, 3, 2, 1, 0, 0], &[1]),
    // p30_word_after_body
    (b"proc f {} {\n# doc\n} extra\n", [3, 2, 2, 1, 0, 0], &[1]),
    // q01_two_comment_rows_crlf
    (b"# a\r\n# b\n", [2, 0, 0, 2, 0, 0], &[0, 1]),
    // q02_blank_row_short
    (b"proc f {} {\n# first\n\n# second\n}\n", [5, 2, 1, 2, 1, 0], &[1, 3]),
    // q03_continued_short
    (b"proc f {} {\n# a \\\n# b\n}\n", [4, 2, 1, 2, 0, 0], &[1, 2]),
    // q04_comment_above
    (b"# c\nproc g {} {\n# inner\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // q05_text_rows
    (b"proc f {} {\n# first\nset x 1\n#ruff later\n# a \\\\\n}\n", [6, 3, 2, 3, 0, 0], &[1, 3, 4]),
    // q06_variable_body
    (b"proc f {} $body\n", [1, 1, 1, 0, 0, 0], &[]),
    // c09_marker_on_continued_command
    (b"proc f {} {\nset x 1 \\\n#ruff notdoc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // c10_rows_on_continued_command
    (b"proc f {} {\nset x 1 \\\n#ruff b\n# c\n}\n", [5, 3, 2, 2, 0, 0], &[2, 3]),
    // c11_marker_on_continued_expr
    (b"proc f {} {\nexpr 1 \\\n#ruff b\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // c12_directive_on_continued_command
    (b"proc f {} {\nset x 1 \\\n#ruffopt a b\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // c13_marker_on_continued_command_crlf
    (b"proc f {} {\r\nset x 1 \\\r\n#ruff b\r\n}\r\n", [4, 3, 3, 1, 0, 0], &[2]),
    // c14_marker_below_command
    (b"proc f {} {\nset x 1\n#ruff b\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // c15_marker_after_semicolon_and_backslash
    (b"proc f {} {\n# lead\nset x 1 ;\\\n#ruff b\n}\n", [5, 3, 2, 2, 0, 0], &[1, 3]),
    // d01_qualified_proc
    (b"::proc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d02_quoted_proc
    (b"\"proc\" f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d03_braced_proc
    (b"{proc} f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d04_escaped_proc
    (b"pr\\oc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d05_expanded_proc
    (b"{*}proc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d06_evaluated_words
    (b"eval proc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // p32_semicolon_then_continued_line_end
    (b"set x 1 ;\\\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // p33_backslash_starts_the_source
    (b"\\\nproc f {} {\n# a\n}\n", [4, 3, 1, 1, 0, 0], &[2]),
    // p34_vertical_tab_after_body
    (b"proc f {} {\n# doc\n}\x0bextra\n", [3, 2, 2, 1, 0, 0], &[1]),
    // p35_crlf_continued_word_after_body
    (b"proc f {} {\n# a\n} \\\r\nextra\r\n", [4, 3, 3, 1, 0, 0], &[1]),
    // p36_continued_line_end_after_body
    (b"proc f {} {\n# doc\n} \\\n\n", [4, 2, 1, 1, 1, 0], &[1]),
    // p37_semicolon_then_continued_comment
    (b"proc f {} {\n# doc\n};# c \\\nextra\n", [4, 3, 2, 2, 0, 0], &[1, 2]),
    // e01_open_brace_on_the_line
    (b"set x {a ; proc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // e02_open_bracket_on_the_line
    (b"set x [a ; proc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // e03_recovered_comment_on_continued_row
    (b"set x 1 # c \\\n; proc g {} {\n# doc\n}\n", [4, 3, 2, 2, 0, 0], &[0, 2]),
    // e04_stray_brace_on_the_line
    (b"} ; proc g {} {\n# d\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // r01_carriage_return_between_words
    (b"proc f\r{} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // r02_carriage_return_after_body
    (b"proc f {} {\n# doc\n}\rextra\n", [3, 2, 2, 1, 0, 0], &[1]),
    // r03_carriage_return_before_procedure
    (b"set x 1\rproc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // r04_carriage_return_is_a_blank_row
    (b"proc f {} {\n# a\n\r# b\n}\n", [4, 2, 1, 2, 0, 0], &[1, 2]),
    // r05_carriage_returns_around_a_row
    (b"proc f {} {\r# a\n\rset x 1\r#ruff b\n}\n", [3, 3, 2, 2, 0, 0], &[0, 1]),
    // c19_directive_below_command
    (b"proc f {} {\nset x 1\n#ruffopt a b\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // k15_directive_above_whole_comment
    (b"proc f {} {\n#ruffopt a b\n# ab\nreturn\n}\n", [5, 3, 2, 2, 0, 0], &[1, 2]),
    // c16_backslash_row_between_rows
    (b"proc f {} {\n# lead\n\\\n# b\n}\n", [5, 2, 1, 3, 0, 0], &[1, 2, 3]),
    // c17_backslash_row_then_blank_row
    (b"proc f {} {\n# lead\n\\\n\n# b\n}\n", [6, 2, 1, 2, 2, 0], &[1, 4]),
    // c18_backslash_row_then_blank_row_crlf
    (b"proc f {} {\r\n# lead\r\n\\\r\n\r\n# b\r\n}\r\n", [6, 3, 2, 2, 1, 0], &[1, 4]),
    // z01_end_of_file_character
    (b"set x 1\n\x1a\nproc g {} {\n# doc\n}\n", [5, 4, 3, 1, 0, 0], &[3]),
    // z02_open_brace_on_the_line_above
    (b"set x {a ;\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // s06_procedure_in_command_substitution
    (b"[proc f {} {\n# doc\n}]\n", [3, 2, 2, 1, 0, 0], &[1]),
    // s07_procedure_in_eval_script
    (b"eval {proc f {} {\n# doc\n}}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // c20_blank_row_then_backslash_row
    (b"proc f {} {\n# lead\n\n\\\n# b\n}\n", [6, 2, 1, 3, 1, 0], &[1, 3, 4]),
    // p39_one_row_procedure_on_continued_row
    (b"# c \\\nproc a {} {};proc b {} {\n# b\n}\n", [4, 2, 2, 2, 0, 0], &[0, 2]),
    // p38_word_after_semicolon
    (b"set x 1 ; set y 2 proc g {} {\n# doc\n}\n", [3, 2, 3, 1, 0, 0], &[1]),
    // q30_form_feed_before
    (b"\x0cproc g {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // q31_no_break_space_before
    (b"\xc2\xa0proc g {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // p40_parameter_list_glued_to_body
    (b"proc f {}{\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // p41_named_parameters_glued_to_body
    (b"proc f {a b}{\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // p42_keyword_glued_to_braced_name
    (b"proc{f} {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // g01_body_ends_early
    (b"proc f {} {\n# a }\n# b\n}\n", [4, 2, 1, 2, 0, 0], &[1, 2]),
    // g02_body_ends_late
    (b"proc f {} {\n# doc {\n}\n# }\n", [4, 2, 1, 2, 0, 0], &[1, 3]),
    // g03_quoted_brace_in_body
    (b"proc f {} {\n# doc\nset s \"{\"\n}\n# more\n}\n", [6, 4, 2, 2, 0, 0], &[1, 4]),
    // g04_marker_row_opens_a_brace
    (b"proc f {} {\n#ruff a {\n}\n# }\n", [4, 2, 1, 2, 0, 0], &[1, 3]),
    // g05_parameter_list_ends_early
    (b"proc g {a {b \"}\"}} {\n# a\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // g06_directive_in_parted_body
    (b"proc f {} {\n#ruffopt a b\n# doc {\n}\n# }\n", [5, 2, 1, 3, 0, 0], &[1, 2, 4]),
    // g07_parted_body_without_rows
    (b"proc f {} {\nset s \"{\"\n}\n}\n", [4, 4, 2, 0, 0, 0], &[]),
    // g08_parted_body_and_computed_name
    (b"proc $name {} {\n# a }\n# b\n}\n", [4, 2, 1, 2, 0, 0], &[1, 2]),
    // g09_parted_body_on_a_continued_comment_line
    (b"# c \\\nproc f {} {\n# a }\n# b\n}\n", [5, 2, 1, 3, 0, 0], &[0, 2, 3]),
    // g10_bracket_holds_a_quoted_close_brace
    (b"proc f {} {\n# doc\nset s [list \"}\"]\n}\n", [4, 3, 2, 1, 0, 0], &[1]),
    // h01_brace_pair_over_two_procedures
    (b"proc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n", [7, 4, 2, 3, 0, 0], &[1, 4, 6]),
    // h02_quoted_brace_in_earlier_body
    (b"proc g {} {\nset s \"{\"\n}\nproc f {} {\n# doc\n}\n}\n", [7, 6, 3, 1, 0, 0], &[4]),
    // h03_bracket_holds_a_quoted_brace
    (b"proc g {} {\nset s [list \"{\"]\n}\nproc f {} {\n# doc\n}\n}\n", [7, 6, 3, 1, 0, 0], &[4]),
    // h04_escaped_backslash_before_brace
    (b"proc g {} {\n# \\\\{\n}\nproc f {} {\n# doc\n}\n}\n", [7, 5, 2, 2, 0, 0], &[1, 4]),
    // h05_earlier_word_of_another_command
    (b"if {1} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n", [7, 4, 2, 3, 0, 0], &[1, 4, 6]),
    // h06_comment_after_semicolon_in_earlier_body
    (b"proc g {} {\nset x 1 ;# {\n}\nproc f {} {\n# doc\n}\n# }\n", [7, 5, 3, 3, 0, 0], &[1, 4, 6]),
    // h07_earlier_word_never_ends
    (b"proc g {} {\n# if {\nputs a\n}\nproc f {} {\n# doc\n}\n", [7, 5, 3, 2, 0, 0], &[1, 5]),
    // h08_brace_left_open_by_the_grammar
    (b"set s $a# {\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // h09_word_after_semicolon
    (b"set a 1 ;{# {\n}\nproc g {} {\n# doc\n}\n# }\n", [6, 4, 3, 3, 0, 0], &[0, 3, 5]),
    // h10_word_after_line_end
    (b"set a 1\n{# {\n}\nproc g {} {\n# doc\n}\n# }\n", [7, 5, 3, 3, 0, 0], &[1, 4, 6]),
    // h11_word_after_bracket
    (b"set s [{# {\n} a]\nproc g {} {\n# doc\n}\n# }\n", [6, 4, 2, 3, 0, 0], &[0, 3, 5]),
    // h12_word_after_expansion_prefix
    (b"list {*}{# {\n}\nproc g {} {\n# doc\n}\n# }\n", [6, 4, 2, 3, 0, 0], &[0, 3, 5]),
    // h13_word_at_the_start_of_the_source
    (b"{# {\n}\nproc g {} {\n# doc\n}\n# }\n", [6, 4, 2, 3, 0, 0], &[0, 3, 5]),
    // h14_open_brace_inside_a_bracket
    (b"set x [a {b] ;\nproc g {} {\n# doc\n}\n", [4, 3, 3, 1, 0, 0], &[2]),
    // j01_balanced_pair_in_a_row
    (b"proc g {} {\n# {}\n}\nproc f {} {\n# doc\n}\n", [6, 4, 2, 2, 0, 0], &[1, 4]),
    // j02_brace_pair_over_two_rows
    (b"proc g {} {\n# {\n# }\n}\nproc f {} {\n# doc\n}\n", [7, 4, 2, 3, 0, 0], &[1, 2, 5]),
    // j03_escaped_brace_in_a_row
    (b"proc g {} {\n# \\{\n}\nproc f {} {\n# doc\n}\n", [6, 4, 2, 2, 0, 0], &[1, 4]),
    // j04_quoted_word_with_a_balanced_pair
    (b"proc g {} {\nset s \"{}\"\n}\nproc f {} {\n# doc\n}\n", [6, 5, 3, 1, 0, 0], &[4]),
    // j05_regular_expression_in_braces
    (b"set re {^[a-z]{1,4}$}\nproc f {} {\n# doc\nregexp {^[a-z]{1,4}$} $s\n}\n", [5, 4, 3, 1, 0, 0], &[2]),
    // j06_crlf_line_ends
    (b"set x {a b}\r\nproc f {} {\r\n# doc {}\r\n}\r\n", [4, 3, 2, 1, 0, 0], &[2]),
    // j07_procedure_after_a_word_that_ends_late
    (b"proc g {} {\n# {\n}\n# }\nproc f {} {\n# doc\n}\n", [7, 4, 2, 3, 0, 0], &[1, 3, 5]),
    // j15_procedure_inside_and_procedure_after
    (b"if {0} {\nset s \"{\"\n}\nproc g {} {\n# doc\n}\n}\nproc h {} {\n# later\n}\n", [10, 8, 4, 2, 0, 0], &[4, 8]),
    // j08_brace_inside_a_bare_word
    (b"set s a{b ;\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // j09_braces_in_a_recovered_bare_word
    (b"list ){{}\nproc f {} {\n# doc\n}\n", [4, 3, 1, 1, 0, 0], &[2]),
    // j10_quoted_open_brace
    (b"set s \"{\"\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // j11_escaped_open_brace
    (b"set s \\{\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // j12_brace_in_a_comment_of_the_file
    (b"# {\nproc f {} {\n# doc\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // j13_expansion_and_word_after_semicolon
    (b"set l [list {*}{a b}] ;{c} d\nproc g {} {\n# doc\n}\n", [4, 3, 5, 1, 0, 0], &[2]),
    // j14_braced_variable_name
    (b"set v ${name}\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // j16_braced_word_three_lines_above
    (b"set s {a b}\nset t 1\nset u 2\nproc g {} {\n# doc\n}\n", [6, 5, 4, 1, 0, 0], &[4]),
    // l01_text_after_a_close_brace
    (b"set s {a}b\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // l02_text_after_a_close_quote
    (b"set s \"a\"b\nproc f {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // l03_quoted_close_brace_in_earlier_body
    (b"proc g {} {\nset s \"}\"\n}\nproc f {} {\n# doc\n}\n", [6, 5, 3, 1, 0, 0], &[4]),
    // l04_hash_inside_a_command_opens_a_brace
    (b"set x 1 # c {\nproc f {} {\n# doc\n}\n}\n", [5, 4, 2, 2, 0, 0], &[0, 2]),
    // l05_array_index_over_lines
    (b"set s $a(\nproc f {} {\n# doc\n}\n)\n", [5, 4, 1, 1, 0, 0], &[2]),
    // l06_open_bracket_before_semicolon
    (b"set x [a ;\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // l07_command_tcl_does_not_reach
    (b"}\nproc f {} {\n# doc\n}\n", [4, 3, 1, 1, 0, 0], &[2]),
    // l09_hash_inside_a_command_opens_a_quote
    (b"set x 1 # \"\nproc f {} {\n# doc\n}\n\"\n", [5, 4, 2, 2, 0, 0], &[0, 2]),
    // l08_nested_procedure_whose_body_ends_early
    (b"namespace eval n {\nproc f {} {\n# a }\n# b\n}\n}\n", [6, 4, 2, 2, 0, 0], &[2, 3]),
    // t01_brace_in_continued_comment
    (b"# c \\\nproc g {} {\n# { \\\n}\nproc f {} {\n# doc\n}\n# }\n", [8, 4, 2, 4, 0, 0], &[0, 2, 5, 7]),
    // t02_no_brace_in_continued_comment
    (b"# c \\\nproc g {} {\n# x \\\n}\nproc f {} {\n# doc\n}\n# }\n", [8, 4, 2, 4, 0, 0], &[0, 2, 5, 7]),
    // t03_braced_row_in_continued_comment
    (b"# c \\\n{a \"{\"}\nproc f {} {\n# doc\n}\n", [5, 3, 3, 2, 0, 0], &[0, 3]),
    // t04_late_word_in_early_word
    (b"proc g {} {\n# }\nproc h {} {\n# {{\n}\n}\nproc f {} {\n# doc\n}\n# }\n", [10, 6, 3, 4, 0, 0], &[1, 3, 7, 9]),
    // t05_late_word_of_if_in_early_word
    (b"proc g {} {\n# }\nif {0} {\n# {{\n}\n}\nproc f {} {\n# doc\n}\n# }\n", [10, 6, 3, 4, 0, 0], &[1, 3, 7, 9]),
    // t06_balanced_nested_procedure
    (b"proc g {} {\n# x\nproc h {} {\n# y\n}\n}\nproc f {} {\n# doc\n}\n", [9, 6, 3, 3, 0, 0], &[1, 3, 7]),
    // t07_cr_in_comment_then_open_brace
    (b"# c\rset x {\nproc f {} {\n# doc\n}\n}\n", [5, 3, 1, 2, 0, 0], &[0, 2]),
    // t08_cr_in_comment_then_procedure
    (b"# c\rproc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n", [7, 3, 1, 4, 0, 0], &[0, 1, 4, 6]),
    // t09_cr_in_comment_after_semicolon
    (b"set x 1 ;# c\rset y {\nproc f {} {\n# doc\n}\n}\n", [5, 4, 2, 2, 0, 0], &[0, 2]),
    // t10_lf_in_comment_control
    (b"# c\nset x 1\nproc f {} {\n# doc\n}\n", [5, 3, 2, 2, 0, 0], &[0, 3]),
    // t11_byte_order_mark
    (b"\xef\xbb\xbfproc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // e05_semicolon_in_parens
    (b"puts a(;) ; proc g {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // e06_stray_bracket_on_the_line
    (b"] ; proc g {} {\n# d\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // l10_procedure_in_a_bracket_after_a_comment
    (b"[# c\nproc f {} {\n# doc\n};]", [4, 3, 2, 2, 0, 0], &[0, 2]),
    // l11_procedure_in_a_bracket_after_a_semicolon
    (b"[;proc f {} {\n# doc\n};]", [3, 2, 1, 1, 0, 0], &[1]),
    // l12_bracket_glued_before_proc
    (b"set x 1];[]proc f {} {\n# doc\n}\n", [3, 2, 2, 1, 0, 0], &[1]),
    // u01_word_and_crlf_above
    (b"JUNK\r\nproc g {} {\r\n# doc\r\n}\r\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u02_two_backslashes_and_line_end_above
    (b"JUN\\\\\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u03_word_and_two_backslashes_above
    (b"JUNK\\\\\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u04_quoted_word_above
    (b"set s \"a b\"\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u05_bracket_above
    (b"set s [a b]\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u06_array_index_above
    (b"set s $a(1)\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u07_braced_variable_name_above
    (b"set s ${a b}\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u08_end_of_file_byte_above
    (b"JUNK\x1a\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u09_continued_comment_above_and_word_below
    (b"# c \\\nproc g {} {\n# doc\n}\nJUNK\n", [5, 3, 2, 2, 0, 0], &[0, 2]),
    // u10_braced_word_and_text_after_body
    (b"proc f {} {\n# doc\n} {a}JUNK\n", [3, 2, 3, 1, 0, 0], &[1]),
    // u11_comment_above
    (b"# abc\nproc g {} {\n# doc\n}\n", [4, 2, 1, 2, 0, 0], &[0, 2]),
    // u12_word_above
    (b"X\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
    // u13_blank_row_above
    (b"JUNK\n\nproc g {} {\n# doc\n}\n", [5, 3, 2, 1, 1, 0], &[3]),
    // u14_word_after_body_and_blank
    (b"proc f {} {\n# doc\n} JUNK\n", [3, 2, 2, 1, 0, 0], &[1]),
    // d07_quoted_parameter_list
    (b"proc f \"x\" {\n# doc\n}\n", [3, 3, 0, 0, 0, 0], &[]),
    // d08_bracket_glued_before_proc
    (b"[]proc f {} {\n# doc\n}\n", [3, 2, 1, 1, 0, 0], &[1]),
    // d09_brace_in_the_middle_of_a_word_above
    (b"set s a{\nproc f {} {\n# doc\n}\nset t }\n", [5, 5, 1, 0, 0, 0], &[]),
    // d10_quote_in_the_middle_of_a_word_above
    (b"set s a\"\nproc f {} {\n# doc\n}\nset t b\"\n", [5, 5, 1, 0, 0, 0], &[]),
    // d11_quote_in_a_braced_word_above
    (b"set q {\"}\nproc f {} {\n# doc\n}\nset r {\"}\n", [5, 5, 1, 0, 0, 0], &[]),
    // d12_dollar_at_the_end_of_a_word_above
    (b"set s a$\nproc f {} {\n# doc\n}\n", [4, 3, 1, 0, 1, 0], &[]),
    // d13_carriage_returns_as_line_ends
    (b"proc f {} {\r# doc\r}\r", [1, 1, 0, 1, 0, 0], &[0]),
    // v01_a_line_end_ends_a_command
    (b"a b\nc", [2, 2, 1, 0, 0, 0], &[]),
    // v02_and_so_does
    (b"a;b ;c", [1, 1, 2, 0, 0, 0], &[]),
    // v03_a_comment_where_a_command_starts
    (b"# c\na", [2, 1, 0, 1, 0, 0], &[0]),
    // v04_a_in_a_command_is_a_word
    (b"a # b\nc", [2, 2, 1, 0, 0, 0], &[]),
    // v05_a_comment_after
    (b"a ;# c\nb", [2, 2, 1, 1, 0, 0], &[0]),
    // v06_a_backslash_continues_a_comment
    (b"# c \\\na\nb", [3, 2, 1, 1, 0, 0], &[0]),
    // v07_two_backslashes_do_not
    (b"# c \\\\\na", [2, 1, 0, 1, 0, 0], &[0]),
    // v08_more_than_four_words
    (b"a b c d e f", [1, 1, 0, 0, 0, 0], &[]),
    // v09_a_vertical_tab_a_form_feed_and_a_tab_separat
    (b"a\x0bb\x0cc\td", [1, 1, 0, 0, 0, 0], &[]),
    // v10_a_backslash_and_a_line_end_separate_words
    (b"a \\\nb\nc", [3, 3, 1, 0, 0, 0], &[]),
    // v11_also_directly_after_a_word
    (b"a\\\nb", [2, 2, 0, 0, 0, 0], &[]),
    // v12_and_they_are_white_space_before_a_command
    (b"\\\na", [2, 2, 0, 0, 0, 0], &[]),
    // v13_any_other_backslash_starts_a_word
    (b"\\a b", [1, 1, 0, 0, 0, 0], &[]),
    // v14_and_hides_a_blank_in_a_word
    (b"a\\ b c", [1, 1, 0, 0, 0, 0], &[]),
    // v15_a_backslash_at_the_end_of_the_file_is_a_word
    (b"a\\", [1, 1, 0, 0, 0, 0], &[]),
    // v16_a_carriage_return_alone_ends_a_line
    (b"a\rb", [1, 1, 0, 0, 0, 0], &[]),
    // v17_cr_lf_is_one_line_end
    (b"a\r\nb", [2, 2, 1, 0, 0, 0], &[]),
    // v18_a_carriage_return_ends_a_comment
    (b"# c\ra", [1, 1, 0, 1, 0, 0], &[0]),
    // v19_a_backslash_and_cr_lf_are_white_space
    (b"a \\\r\nb", [2, 2, 1, 0, 0, 0], &[]),
    // v20_the_byte_0x1a_ends_the_file
    (b"a\x1ab", [1, 1, 0, 0, 0, 0], &[]),
    // v21_a_byte_order_mark_is_part_of_the_first_word
    (b"\xef\xbb\xbfa b", [1, 1, 0, 0, 0, 0], &[]),
    // v22_a_braced_word
    (b"a {b c} d", [1, 1, 1, 0, 0, 0], &[]),
    // v23_braces_nest
    (b"a {b {c} d} e", [1, 1, 2, 0, 0, 0], &[]),
    // v24_a_backslash_hides_a_brace
    (b"a {b \\} c} d", [1, 1, 1, 0, 0, 0], &[]),
    // v25_a_line_end_in_a_braced_word_ends_nothing
    (b"a {b\nc}\nd", [3, 3, 3, 0, 0, 0], &[]),
    // v26_a_comment_is_not_special_in_a_braced_word
    (b"a {# }\nb", [2, 2, 0, 1, 0, 0], &[0]),
    // v27_nor_is_a_quote_the_word_ends_at_the_first_br
    (b"a {\"}\"} b", [1, 1, 1, 0, 0, 0], &[]),
    // v28_a_backslash_and_a_line_end_after_a_brace_sep
    (b"a {b}\\\nc", [2, 2, 1, 0, 0, 0], &[]),
    // v29_any_other_byte_after_a_close_brace_is_a_pars
    (b"a {b}c\nd", [2, 2, 2, 0, 0, 0], &[]),
    // v30_a_brace_that_never_closes_is_a_parse_error
    (b"a {b\nc", [2, 2, 1, 0, 0, 0], &[]),
    // v31_a_quoted_word
    (b"a \"b c\" d", [1, 1, 0, 0, 0, 0], &[]),
    // v32_a_line_end_in_a_quoted_word_ends_nothing
    (b"a \"b\nc\"\nd", [3, 3, 1, 0, 0, 0], &[]),
    // v33_a_backslash_hides_a_quote
    (b"a \"b \\\" c\" d", [1, 1, 0, 0, 0, 0], &[]),
    // v34_a_byte_after_a_close_quote_is_a_parse_error
    (b"a \"b\"c\nd", [2, 2, 1, 0, 0, 0], &[]),
    // v35_a_quote_in_a_bare_word_is_a_word_byte
    (b"a b\"c d\"", [1, 1, 0, 0, 0, 0], &[]),
    // v36_a_quote_that_never_closes_is_a_parse_error
    (b"a \"b", [1, 1, 0, 0, 0, 0], &[]),
    // v37_a_bracket_holds_a_script
    (b"a [b c] d", [1, 1, 0, 0, 0, 0], &[]),
    // v38_a_line_end_in_it_ends_a_command_of_that_scri
    (b"a [b\nc] d", [2, 2, 0, 0, 0, 0], &[]),
    // v39_a_comment_in_it_hides_a_bracket
    (b"a [b ;# c ]\n] d", [2, 2, 0, 1, 0, 0], &[0]),
    // v40_a_bracket_in_the_middle_of_a_word
    (b"a x[b]y z", [1, 1, 0, 0, 0, 0], &[]),
    // v41_a_bracket_in_a_quoted_word_with_a_quoted_wor
    (b"a \"b [c \"d\"] e\" f", [1, 1, 0, 0, 0, 0], &[]),
    // v42_a_braced_word_in_a_bracket_hides_a_bracket
    (b"a [b {c ]} d] e", [1, 1, 1, 0, 0, 0], &[]),
    // v43_a_word_that_starts_with_a_bracket
    (b"[a]b c", [1, 1, 0, 0, 0, 0], &[]),
    // v44_a_close_bracket_outside_a_bracket_is_a_word
    (b"a]b [c]]", [1, 1, 1, 0, 0, 0], &[]),
    // v45_a_bracket_that_never_closes_is_a_parse_error
    (b"a [b\nc", [2, 2, 0, 0, 0, 0], &[]),
    // v46_an_array_index_holds_white_space
    (b"a $a(c d) e", [1, 1, 0, 0, 0, 0], &[]),
    // v47_and_line_ends
    (b"a $a(c\nd) e", [2, 2, 1, 0, 0, 0], &[]),
    // v48_an_index_after_no_name
    (b"a $(c d) e", [1, 1, 0, 0, 0, 0], &[]),
    // v49_one_colon_ends_the_name_no_index
    (b"a $a:(c d)", [1, 1, 1, 0, 0, 0], &[]),
    // v50_two_colons_are_part_of_the_name
    (b"a $a::(c d) e", [1, 1, 0, 0, 0, 0], &[]),
    // v51_a_bracket_in_an_index
    (b"a $a(c [d e] f) g", [1, 1, 0, 0, 0, 0], &[]),
    // v52_an_index_in_an_index
    (b"a $a(c $a(d e) f) g", [1, 1, 0, 0, 0, 0], &[]),
    // v53_a_braced_variable_name_reads_to_the_next_clo
    (b"a ${v w} d", [1, 1, 0, 0, 0, 0], &[]),
    // v54_a_brace_after_a_name_is_a_word_byte
    (b"a $v{b c}", [1, 1, 2, 0, 0, 0], &[]),
    // v55_an_index_that_never_closes_is_a_parse_error
    (b"a $a(c", [1, 1, 0, 0, 0, 0], &[]),
    // v56_and_so_is_a_braced_name
    (b"a ${v", [1, 1, 0, 0, 0, 0], &[]),
    // v57_the_expansion_prefix_before_a_bare_word
    (b"{*}a b", [1, 1, 0, 0, 0, 0], &[]),
    // v58_before_a_braced_word
    (b"a {*}{b c} d", [1, 1, 1, 0, 0, 0], &[]),
    // v59_before_a_quoted_word
    (b"a {*}\"b c\" d", [1, 1, 0, 0, 0, 0], &[]),
    // v60_before_white_space_is_a_word_of_its_own
    (b"a {*} b", [1, 1, 0, 0, 0, 0], &[]),
    // v61_also_before_a_backslash_and_a_line_end
    (b"a {*}\\\nb", [2, 2, 0, 0, 0, 0], &[]),
    // v62_a_second_prefix_is_a_braced_word_and_a_byte
    (b"a {*}{*}b", [1, 1, 1, 0, 0, 0], &[]),
    // v63_no_other_braced_word_is_a_prefix
    (b"a {**}b", [1, 1, 1, 0, 0, 0], &[]),
    // y01_an_excluded_byte_in_a_command_and_one_in_the
    (b"a b\nc d", [2, 2, 1, 0, 0, 0], &[]),
    // y02_an_excluded_byte_in_a_braced_word
    (b"a {b c}\nd", [2, 2, 2, 0, 0, 0], &[]),
    // y03_an_excluded_first_byte
    (b"xa\nb", [2, 2, 1, 0, 0, 0], &[]),
    // y04_a_line_end_directly_after_an_excluded_byte
    (b"x\na\nb", [3, 3, 2, 0, 0, 0], &[]),
    // y05_an_escaped_line_end_after_an_excluded_byte
    (b"x \\\na\nb", [3, 3, 1, 0, 0, 0], &[]),
    // y06_carriage_returns_after_an_excluded_byte
    (b"x\ra\rb c", [1, 1, 0, 0, 0, 0], &[]),
    // y07_cr_lf_whose_line_feed_is_excluded
    (b"ab\r\nc\nd", [3, 3, 2, 0, 0, 0], &[]),
    // y08_the_byte_0x1a_after_an_excluded_byte
    (b"x\x1a\na", [2, 2, 1, 0, 0, 0], &[]),
    // q32_bracket_and_index_above
    (b"set s [list $a(1)]\nproc g {} {\n# doc\n}\n", [4, 3, 2, 1, 0, 0], &[2]),
];

/// The tuple of a source, its comment rows and its documentation rows.
#[cfg(feature = "tcl")]
fn tcl_loc(source: &[u8]) -> ([u64; 6], Vec<usize>, Vec<usize>) {
    let space = analyze(
        Source::new(LANG::Tcl, source),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .unwrap();
    let loc = &space.metrics.loc;
    let rows = |keep: &dyn Fn(usize) -> bool| (0..=source.len()).filter(|row| keep(*row)).collect();
    (
        [
            loc.sloc(),
            loc.ploc(),
            loc.lloc(),
            loc.cloc(),
            loc.blank(),
            loc.dloc(),
        ],
        rows(&|row| {
            loc.cloc.only_comment_line_starts.contains(row)
                || loc.cloc.code_comment_line_starts.contains(row)
        }),
        rows(&|row| loc.dloc.lines.contains(row)),
    )
}

#[cfg(feature = "tcl")]
#[test]
fn documentation_tcl_legacy_rows_hold_no_ruff_documentation() {
    for (source, tuple, comments) in TCL_RUFF_SOURCES {
        // expected: the comment rows of the table, and no documentation row. The Tcl role
        // table claims no documentation comment, so a body comment is a comment row and
        // nothing more.
        assert_eq!(
            tcl_loc(source),
            (tuple, comments.to_vec(), Vec::new()),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
fn documentation_tcl_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    let (mut documents, mut complete, mut associations) = (0, 0, 0);
    for (source, tuple, comments) in TCL_RUFF_SOURCES {
        let name = source.escape_ascii();
        let parser =
            crate::langs::AnyParser::parse(LANG::Tcl, source.to_vec(), None, None).unwrap();
        let idle = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let before = observations();
        let texts = index
            .documents()
            .filter(|document| index.complete_text(*document).unwrap().is_some())
            .count();
        // The collection moves both counters on this thread, so a zero across `analyze`
        // is a reading.
        // expected: one visit for each node of the tree, and one render for each text.
        assert_eq!(
            before.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{name}"
        );
        assert_eq!(before.renders - idle.renders, texts, "{name}");
        documents += index.documents().len();
        complete += texts;
        associations += index.associations().len();
        // expected: the tuple and the comment rows the metric gave before the collector
        // read Tcl, and no documentation row.
        assert_eq!(
            tcl_loc(source),
            (tuple, comments.to_vec(), Vec::new()),
            "{name}"
        );
        let after = observations();
        // expected: 0 for each. `analyze` does not run the collector.
        assert_eq!(after.visits - before.visits, 0, "{name}");
        assert_eq!(after.renders - before.renders, 0, "{name}");
    }
    // expected: the sums over the 327 sources of what the tests of the AST crate state
    // for each of them: documents, complete texts and associations. The render counter
    // moved 214 times in all, so its zero across `analyze` is a reading too.
    assert_eq!((documents, complete, associations), (235, 214, 224));
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

#[cfg(feature = "elixir")]
#[allow(clippy::large_const_arrays)]
const ELIXIR_ATTRIBUTE_SOURCES: [(&[u8], [u64; 6]); 291] = [
    // a01_primary
    (
        b"defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // b01_moduledoc
    (
        b"defmodule M do\n@moduledoc \"audit\"\nend",
        [3, 3, 2, 0, 0, 0],
    ),
    // b02_moduledoc_after_definition
    (
        b"defmodule M do\ndef f(), do: :ok\n@moduledoc \"audit\"\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // b03_moduledoc_then_doc
    (
        concat!(
            "defmodule M do\n@moduledoc \"m\"\n@doc \"d\"\ndef f(), do: :ok\n",
            "end"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // b04_doc_then_moduledoc
    (
        concat!(
            "defmodule M do\n@doc \"d\"\n@moduledoc \"m\"\ndef f(), do: :ok\n",
            "end"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // b05_nested_modules
    (
        concat!(
            "defmodule A do\n@moduledoc \"a\"\ndefmodule B do\n@moduledoc \"b",
            "\"\n@doc \"g\"\ndef g(), do: 1\nend\n@doc \"f\"\ndef f(), do: 2",
            "\nend"
        )
        .as_bytes(),
        [10, 10, 8, 0, 0, 0],
    ),
    // b06_undocumented
    (b"defmodule M do\ndef f(), do: :ok\nend", [3, 3, 2, 0, 0, 0]),
    // b07_empty_module
    (b"defmodule M do\nend", [2, 2, 1, 0, 0, 0]),
    // b08_two_modules
    (
        concat!(
            "defmodule A do\n@doc \"one\"\ndef f(), do: 1\nend\ndefmodule B d",
            "o\n@doc \"two\"\ndef f(), do: 2\nend\n"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // b09_module_name_in_parentheses
    (
        b"defmodule(M) do\n@moduledoc \"audit\"\nend",
        [3, 3, 2, 0, 0, 0],
    ),
    // b10_module_in_a_function_body
    (
        concat!(
            "defmodule M do\ndef g() do\ndefmodule N do\n@moduledoc \"n\"\nen",
            "d\nend\nend"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // b11_moduledoc_with_a_pending_value
    (b"defmodule M do\n@moduledoc false\nend", [3, 3, 2, 0, 0, 0]),
    // c01_head_and_clauses
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f",
            "(x), do: x\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // c02_arity
    (
        concat!(
            "defmodule M do\n@doc \"one\"\ndef f(x), do: x\n@doc \"two\"\ndef",
            " f(x,y), do: x+y\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // c03_default_argument
    (
        b"defmodule M do\n@doc \"audit\"\ndef f(x \\\\ 0), do: x\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // c04_clause_that_is_not_adjacent
    (
        concat!(
            "defmodule M do\ndef h(x), do: x\n@doc \"audit\"\ndef i(x), do: x",
            "\ndef h(y), do: y\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // c05_guarded_and_plain_clause
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
            "f(x), do: x\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // c06_bare_head_and_empty_parentheses
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f\ndef f(), do: :ok\ndef f w",
            "hen true, do: :no\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // c07_kinds_stay_apart
    (
        concat!(
            "defmodule M do\n@doc \"one\"\ndef f(x), do: x\n@doc \"two\"\ndef",
            "macro f(x), do: x\ndefp f(x), do: x\ndefmacrop f(x), do: x\nend"
        )
        .as_bytes(),
        [8, 8, 7, 0, 0, 0],
    ),
    // c08_written_arguments_are_counted
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(a, # c\n b), do: a\ndef f(",
            "a, b), do: b\ndef g(a, b: 1), do: a\ndef g(a, b), do: b\ndef f(a",
            "), do: a\nend"
        )
        .as_bytes(),
        [9, 9, 7, 1, 0, 0],
    ),
    // c09_names_of_one_length
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef ab(x), do: x\ndef ac(x), do:",
            " x\ndef ab(y), do: y\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // c10_do_on_the_next_row
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x)\ndo\nx\nend\ndef f(y) d",
            "o\ny\nend\nend"
        )
        .as_bytes(),
        [10, 10, 6, 0, 0, 0],
    ),
    // c11_unquoted_argument
    (
        b"defmodule M do\n@doc \"audit\"\ndef f(unquote(x)), do: x\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // c12_names_that_the_compiler_normalizes
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef \u{b5}(x), do: x\ndef ",
            "\u{3bc}(x), do: x\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // d01_defp
    (
        b"defmodule M do\n@doc \"audit\"\ndefp f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // d02_defmacrop
    (
        b"defmodule M do\n@doc \"audit\"\ndefmacrop f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // d03_defp_with_a_pending_value
    (
        b"defmodule M do\n@doc false\ndefp f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // e01_semicolon_comment_blank_row_and_spec
    (
        concat!(
            "defmodule M do\n@doc \"audit\"; # note\n\n@spec f() :: atom()\nd",
            "ef f(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 5, 4, 1, 1, 0],
    ),
    // e02_other_attributes_and_reads
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
            "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [9, 9, 8, 0, 0, 0],
    ),
    // e03_crlf
    (
        b"defmodule M do\r\n@doc \"audit\"\r\ndef f(), do: :ok\r\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // e04_row_with_a_lone_backslash
    (
        b"defmodule M do\n@doc \"audit\"\n\\\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // e05_semicolon_on_one_row
    (
        b"defmodule M do\n@doc \"audit\"; def f(), do: :ok\nend",
        [3, 3, 3, 0, 0, 0],
    ),
    // e06_comment_that_ends_with_a_backslash
    (
        b"defmodule M do\n@doc \"audit\" # c \\\ndef f(), do: :ok\nend",
        [4, 4, 3, 1, 0, 0],
    ),
    // e07_continued_row_then_comment
    (
        b"defmodule M do\n@doc \"audit\" \\\n# c\ndef f(), do: :ok\nend",
        [5, 4, 3, 1, 0, 0],
    ),
    // e08_space_after_the_operator
    (
        b"defmodule M do\n@ doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // e09_line_end_after_the_operator
    (
        b"defmodule M do\n@\ndoc \"audit\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // e10_comment_after_the_operator
    (
        b"defmodule M do\n@ # c\ndoc \"audit\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // e11_value_in_parentheses
    (
        b"defmodule M do\n@doc(\"audit\")\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // e13_moduledoc_and_reads_between
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@behaviour B\n@type t :: atom()",
            "\n@opaque o :: atom()\n@before_compile B\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [8, 8, 7, 0, 0, 0],
    ),
    // e12_typedoc_between
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@typedoc \"t\"\n@type t :: atom(",
            ")\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // f01_match_between
    (
        b"defmodule M do\n@doc \"audit\"\nx = 1\ndef f(), do: :ok\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // f02_alias_between
    (
        b"defmodule M do\n@doc \"audit\"\nalias A.B\ndef f(), do: :ok\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // f03_nested_module_between
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefmodule N do\ndef g(), do: 1\n",
            "end\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // f04_end_of_the_body
    (b"defmodule M do\n@doc \"audit\"\nend", [3, 3, 2, 0, 0, 0]),
    // f05_unary_statement_between
    (
        b"defmodule M do\n@doc \"audit\"\n- 1\ndef f(), do: :ok\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // f06_callback_takes_the_attribute
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@callback g() :: atom()\ndef f()",
            ", do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // f07_macrocallback_takes_the_attribute
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@macrocallback g() :: atom()\nde",
            "f f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // f08_two_attributes_released
    (
        concat!(
            "defmodule M do\n@doc \"a\"\n@doc \"b\"\nx = 1\ndef f(), do: :ok",
            "\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // f09_list_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\n[0]\ndef f(), do: :ok\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // f10_next_run_after_a_release
    (
        concat!(
            "defmodule M do\n@doc \"a\"\nx = 1\n@doc \"b\"\ndef f(), do: :ok",
            "\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // g01_defguard
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefguard is_ok(x) when x == :ok",
            "\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g02_defguardp
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefguardp is_ok(x) when x == :ok",
            "\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g03_defdelegate
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefdelegate g(x), to: N\ndef f()",
            ", do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g04_operator_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef a + b, do: a\ndef f(), do: :",
            "ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g05_unquote_head_with_arguments
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef unquote(name)(x), do: x\ndef",
            " f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g06_unquote_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef unquote(name), do: 1\ndef f(",
            "), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g07_spliced_arguments
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef g(unquote_splicing(args)), d",
            "o: 1\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g08_definition_call_in_parentheses
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef(g(x), do: x)\ndef f(), do: :",
            "ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g09_head_without_parentheses
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef g x, y do\nx\nend\ndef f(), ",
            "do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // g10_qualified_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef N.g(y), do: y\ndef f(), do: ",
            ":ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g11_guarded_operator_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef a + b when a > 0, do: a\ndef",
            " f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g12_private_operator_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefp a + b, do: a\ndef f(), do: ",
            ":ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g13_macro_with_an_unquote_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefmacro unquote(name)(x), do: x",
            "\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // g14_private_macro_with_a_qualified_head
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefmacrop N.g(y), do: y\ndef f()",
            ", do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // h01_empty_string
    (
        b"defmodule M do\n@doc \"\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h02_hash_without_brace
    (
        b"defmodule M do\n@doc \"a#b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h03_line_end_inside
    (
        b"defmodule M do\n@doc \"au\ndit\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // h04_crlf_inside
    (
        b"defmodule M do\n@doc \"au\r\ndit\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // h05_two_byte_character
    (
        "defmodule M do\n@doc \"\u{e9}t\u{e9}\"\ndef f(), do: :ok\nend".as_bytes(),
        [4, 4, 3, 0, 0, 0],
    ),
    // h06_tab_inside
    (
        b"defmodule M do\n@doc \"a\tb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h10_heredoc
    (
        concat!(
            "defmodule M do\n@doc \"\"\"\naudit\n\"\"\"\ndef f(), do: :ok\nen",
            "d"
        )
        .as_bytes(),
        [6, 6, 3, 0, 0, 0],
    ),
    // h11_escape
    (
        b"defmodule M do\n@doc \"a\\nb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h12_interpolation
    (
        b"defmodule M do\n@doc \"a#{x}b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h13_raw_sigil
    (
        b"defmodule M do\n@doc ~S(a)\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h14_cooked_sigil
    (
        b"defmodule M do\n@doc ~s(a)\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h15_charlist
    (
        b"defmodule M do\n@doc 'audit'\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h16_false
    (
        b"defmodule M do\n@doc false\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h17_nil
    (
        b"defmodule M do\n@doc nil\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h18_keyword_metadata
    (
        b"defmodule M do\n@doc since: \"1\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h19_concatenation
    (
        b"defmodule M do\n@doc \"a\" <> \"b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h20_text_and_metadata
    (
        concat!(
            "defmodule M do\n@doc \"audit\", since: \"1\"\ndef f(), do: :ok\n",
            "end"
        )
        .as_bytes(),
        [4, 4, 3, 0, 0, 0],
    ),
    // h21_block_after_a_space
    (
        b"defmodule M do\n@doc (\"audit\")\ndef f(), do: :ok\nend",
        [4, 4, 4, 0, 0, 0],
    ),
    // h22_do_block
    (
        b"defmodule M do\n@doc \"audit\" do\nend\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // h23_lone_cr_inside
    (
        b"defmodule M do\n@doc \"a\rb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h24_vertical_tab_inside
    (
        b"defmodule M do\n@doc \"a\x0bb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h25_form_feed_inside
    (
        b"defmodule M do\n@doc \"a\x0cb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h26_next_line_character
    (
        b"defmodule M do\n@doc \"a\xc2\x85b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h27_line_separator
    (
        b"defmodule M do\n@doc \"a\xe2\x80\xa8b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h28_right_to_left_override
    (
        b"defmodule M do\n@doc \"a\xe2\x80\xaeb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h29_pop_directional_isolate
    (
        b"defmodule M do\n@doc \"a\xe2\x81\xa9b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h30_comment_in_parentheses
    (
        b"defmodule M do\n@doc(# c\n\"audit\")\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // h31_atom
    (
        b"defmodule M do\n@doc :audit\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h32_number
    (
        b"defmodule M do\n@doc 1\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h33_neighbours_of_the_rejected
    (
        concat!(
            "defmodule M do\n@doc \"a\u{84}\u{2027}\u{202f}\u{2065}\u{206a}b",
            "\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [4, 4, 3, 0, 0, 0],
    ),
    // h35_backslash_before_a_line_end_inside
    (
        b"defmodule M do\n@doc \"a\\\nb\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // n01_pipe_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // n02_concatenation_on_the_next_row
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n<> \"more\"\ndef f(), do: :ok\ne",
            "nd"
        )
        .as_bytes(),
        [5, 5, 3, 0, 0, 0],
    ),
    // n03_when_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\nwhen x\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // n04_do_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\ndo\nend\ndef f(), do: :ok\nend",
        [6, 6, 3, 0, 0, 0],
    ),
    // n05_comment_row_then_pipe
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n# c\n|> g()\ndef f(), do: :ok\ne",
            "nd"
        )
        .as_bytes(),
        [6, 5, 3, 1, 0, 0],
    ),
    // n06_blank_rows_then_pipe
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n\n\n|> g()\ndef f(), do: :ok\nen",
            "d"
        )
        .as_bytes(),
        [7, 5, 3, 0, 2, 0],
    ),
    // n07_not_in_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\nnot in x\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // n08_dot_on_the_next_row
    (
        b"defmodule M do\n@doc \"audit\"\n.g()\ndef f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // i01_attribute_before_a_later_clause
    (
        concat!(
            "defmodule M do\n@doc \"old\"\ndef g(0), do: 0\n@doc \"new\"\ndef",
            " g(x), do: x\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // i02_two_in_one_run
    (
        concat!(
            "defmodule M do\n@doc \"old\"\n@doc \"new\"\ndef f(), do: :ok\nen",
            "d"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // i03_metadata_then_text
    (
        concat!(
            "defmodule M do\n@doc since: \"1\"\n@doc \"audit\"\ndef f(), do: ",
            ":ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // i04_false_then_text
    (
        concat!(
            "defmodule M do\n@doc false\n@doc \"audit\"\ndef f(), do: :ok\nen",
            "d"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // i05_two_moduledocs
    (
        concat!(
            "defmodule M do\n@moduledoc \"a\"\ndef f(), do: :ok\n@moduledoc ",
            "\"b\"\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // i06_binding_is_the_first_taker
    (
        concat!(
            "defmodule M do\ndef g(0), do: 0\n@doc \"old\"\ndef g(1), do: 1\n",
            "@doc \"new\"\ndef g(x), do: x\nend"
        )
        .as_bytes(),
        [7, 7, 6, 0, 0, 0],
    ),
    // j01_file_level
    (b"@doc \"audit\"\ndef f(), do: :ok", [2, 2, 2, 0, 0, 0]),
    // j02_function_body
    (
        b"defmodule M do\ndef g() do\n@doc \"audit\"\n:ok\nend\nend",
        [6, 6, 4, 0, 0, 0],
    ),
    // j03_block_in_a_module_body
    (
        concat!(
            "defmodule M do\nif true do\n@doc \"audit\"\ndef f(), do: :ok\nen",
            "d\nend"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // j04_keyword_form_of_defmodule
    (
        b"defmodule M, do: (@doc \"audit\"; def f(), do: :ok)",
        [1, 1, 3, 0, 0, 0],
    ),
    // j05_defprotocol
    (
        b"defprotocol P do\n@doc \"audit\"\ndef f(x)\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // j06_defimpl
    (
        b"defimpl P, for: M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // j07_moduledoc_and_typedoc_at_file_level
    (b"@moduledoc \"m\"\n@typedoc \"t\"\n", [2, 2, 2, 0, 0, 0]),
    // j08_attribute_as_an_operand
    (
        b"defmodule M do\nx = @doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // j09_section_of_a_module_block
    (
        concat!(
            "defmodule M do\ndef f(), do: :ok\nelse\n@doc \"audit\"\ndef g(),",
            " do: 1\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // k01_quote_in_a_module
    (
        concat!(
            "defmodule M do\nquote do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            "\nend"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // k02_module_in_a_quote
    (
        concat!(
            "quote do\ndefmodule M do\n@moduledoc \"audit\"\n@doc \"x\"\ndef ",
            "f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // k03_keyword_form_of_quote
    (
        b"quote do: (@doc \"audit\"; def f(), do: :ok)",
        [1, 1, 3, 0, 0, 0],
    ),
    // k04_definition_after_a_quote
    (
        concat!(
            "defmodule M do\nquote do\n@doc \"t\"\ndef g(), do: 1\nend\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // k05_module_after_a_quote
    (
        concat!(
            "quote do\nquote do\n@doc \"a\"\nend\nend\ndefmodule M do\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [9, 9, 6, 0, 0, 0],
    ),
    // l01_definition_with_an_error
    (
        concat!(
            "defmodule M do\n  @doc \"audit\"\n  def f(, do: :ok\n  @doc \"ne",
            "xt\"\n  def g(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // l07_defp_with_an_error
    (
        concat!(
            "defmodule M do\n  @doc \"audit\"\n  defp f(, do: :ok\n  @doc \"n",
            "ext\"\n  def g(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // l08_defmacro_with_an_error
    (
        concat!(
            "defmodule M do\n  @doc \"audit\"\n  defmacro f(, do: :ok\n  @doc",
            " \"next\"\n  def g(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // l09_defmacrop_with_an_error
    (
        concat!(
            "defmodule M do\n  @doc \"audit\"\n  defmacrop f(, do: :ok\n  @do",
            "c \"next\"\n  def g(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // l02_attribute_with_an_error
    (
        b"defmodule M do\n@doc \"a\" \"b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // l03_moduledoc_with_an_error_ends_the_run
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@moduledoc \"a\" \"b\"\ndef f(),",
            " do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // l04_module_without_its_end
    (
        concat!(
            "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :",
            "ok\n"
        )
        .as_bytes(),
        [4, 4, 4, 0, 0, 0],
    ),
    // l05_error_node_between
    (
        b"defmodule M do\n@doc \"audit\"\n1 def f(), do: :ok\nend",
        [4, 4, 4, 0, 0, 0],
    ),
    // l06_other_attribute_with_an_error
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@spec f( :: atom()\ndef f(), do:",
            " :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // l11_error_row_between
    (
        b"defmodule M do\n@doc \"audit\"\n)\ndef f(), do: :ok\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // l12_nul_byte_in_a_comment
    (
        b"defmodule M do\n@doc \"audit\" # c\x00 d\ndef f(), do: :ok\nend",
        [4, 4, 2, 1, 0, 0],
    ),
    // l10_nul_byte_in_a_string
    (
        b"defmodule M do\n@doc \"a\x00b\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // m01_definition_after_end_on_one_row
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do",
            ": :ok\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // m02_attribute_after_end_on_one_row
    (
        concat!(
            "defmodule M do\ndef g() do\n:ok\nend @doc \"audit\"\ndef f(), do",
            ": :ok\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // m03_definition_after_a_heredoc_on_one_row
    (
        b"defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
        [5, 5, 3, 0, 0, 0],
    ),
    // m04_attribute_after_a_continued_row
    (
        concat!(
            "defmodule M do\ndef g() do\n:ok\nend \\\n@doc \"audit\"\ndef f()",
            ", do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // m05_definition_after_a_continued_crlf_row
    (
        concat!(
            "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" \\\r\ndef f(), do: :o",
            "k\nend"
        )
        .as_bytes(),
        [6, 6, 3, 0, 0, 0],
    ),
    // m06_definition_after_a_moduledoc_on_one_row
    (
        concat!(
            "defmodule M do\n@moduledoc \"\"\"\naudit\n\"\"\" def f(), do: :o",
            "k\nend"
        )
        .as_bytes(),
        [5, 5, 3, 0, 0, 0],
    ),
    // p01_module_under_an_error_node
    (
        concat!(
            "defmodule A do\n) ) (\ndefmodule M do\n@moduledoc \"m\"\n@doc \"",
            "audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 5, 0, 0, 0],
    ),
    // q01_qualified_attribute_and_definition
    (
        concat!(
            "defmodule M do\nKernel.@(doc \"audit\")\nKernel.def f(), do: :ok",
            "\nend"
        )
        .as_bytes(),
        [4, 4, 3, 0, 0, 0],
    ),
    // q02_qualified_defmodule
    (
        b"Kernel.defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // q03_attribute_set_by_a_call
    (
        concat!(
            "defmodule M do\nModule.put_attribute(__MODULE__, :doc, {1, \"aud",
            "it\"})\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [4, 4, 3, 0, 0, 0],
    ),
    // q04_macros_that_document
    (
        concat!(
            "defmodule M do\ndocs \"audit\"\nuse Documented, doc: \"audit\"\n",
            "def f(), do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // q05_other_attribute_names
    (
        concat!(
            "defmodule M do\n@docs \"audit\"\n@moduledocs \"audit\"\n@typedoc",
            "s \"audit\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // q06_definitions_in_a_quote_and_under_an_operator
    (
        concat!(
            "quote do\ndef f(), do: :ok\ndefmodule M do\nend\nend\n@\ndef g()",
            ", do: 1\n"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // o01_gap_between_attribute_and_definition
    (
        b"defmodule M do\n@doc \"audit\"\n# x\ndef f(), do: :ok\nend",
        [5, 4, 3, 1, 0, 0],
    ),
    // o02_later_whole_pair
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n# x\ndef f(), do: :ok\n@doc \"la",
            "ter\"\ndef g(), do: 1\nend"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // o03_gap_between_two_clauses
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(0), do: 0\n# x\ndef f(x), ",
            "do: x\nend"
        )
        .as_bytes(),
        [6, 5, 4, 1, 0, 0],
    ),
    // o04_gap_inside_a_definition
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(0), do: (\n# x\n0)\ndef f(",
            "x), do: x\nend"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // o08_statement_after_the_gap_releases_the_attribute
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n# x\nx = 1\n@doc \"later\"\ndef ",
            "g(), do: 1\nend"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // o09_gap_with_no_attribute_waiting
    (
        concat!(
            "defmodule M do\ndef f(0), do: 0\n# x\n@doc \"later\"\ndef g(), d",
            "o: 1\nend"
        )
        .as_bytes(),
        [6, 5, 4, 1, 0, 0],
    ),
    // o05_gap_inside_the_attribute
    (
        b"defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // o06_gap_inside_the_module_word
    (
        b"defmoXXdule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // o10_gap_inside_an_attribute_at_file_level
    (b"@doc \"audit\"\ndef f(), do: :ok", [2, 2, 2, 0, 0, 0]),
    // o07_adjacent_ranges
    (
        b"defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // h34_byte_that_is_no_utf8
    (
        b"defmodule M do\n@doc \"a\xffb\"\ndef f(), do: :ok\nend",
        [4, 4, 3, 0, 0, 0],
    ),
    // r01_doc_read_with_a_comment
    (
        b"defmodule M do\n@doc( # c\n)\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // r02_moduledoc_read_with_a_comment
    (
        b"defmodule M do\n@moduledoc( # c\n)\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // r03_typedoc_read_with_a_comment
    (
        b"defmodule M do\n@typedoc( # c\n)\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // r04_read_with_two_comments
    (
        b"defmodule M do\n@doc( # a\n# b\n)\ndef f(), do: :ok\nend",
        [6, 5, 3, 2, 0, 0],
    ),
    // r05_read_before_a_real_attribute
    (
        concat!(
            "defmodule M do\n@doc( # c\n)\n@doc \"audit\"\ndef f(), do: :ok\n",
            "end"
        )
        .as_bytes(),
        [6, 6, 4, 1, 0, 0],
    ),
    // r06_read_after_a_real_attribute
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@doc( # c\n)\ndef f(), do: :ok\n",
            "end"
        )
        .as_bytes(),
        [6, 6, 4, 1, 0, 0],
    ),
    // r07_moduledoc_read_after_a_real_one
    (
        b"defmodule M do\n@moduledoc \"audit\"\n@moduledoc( # c\n)\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // r08_reads_at_file_level
    (
        b"@doc( # c\n)\n@moduledoc( # c\n)\n@typedoc( # c\n)\n",
        [6, 6, 3, 3, 0, 0],
    ),
    // r09_read_in_a_quote
    (b"quote do\n@doc( # c\n)\nend", [4, 4, 2, 1, 0, 0]),
    // t01_comment_before_the_do_of_a_module
    (
        b"defmodule M # c\ndo\n@doc \"audit\"\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // t02_comment_before_the_do_of_a_definition
    (
        b"defmodule M do\n@doc \"audit\"\ndef f() # c\ndo\n:ok\nend\nend",
        [7, 7, 4, 1, 0, 0],
    ),
    // t03_comment_after_when
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x) when # c\n is_atom(x), ",
            "do: x\nend"
        )
        .as_bytes(),
        [5, 5, 3, 1, 0, 0],
    ),
    // t04_comment_before_when
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when is_atom(x), ",
            "do: x\nend"
        )
        .as_bytes(),
        [5, 5, 3, 1, 0, 0],
    ),
    // t05_comment_before_the_do_of_a_guarded_definition
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x) when is_atom(x) # c\ndo",
            "\nx\nend\nend"
        )
        .as_bytes(),
        [7, 7, 4, 1, 0, 0],
    ),
    // t06_comment_after_the_value_in_parentheses
    (
        b"defmodule M do\n@doc(\"audit\" # c\n)\ndef f(), do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // t07_comment_between_the_head_and_the_keywords
    (
        b"defmodule M do\n@doc \"audit\"\ndef f(), # c\n do: :ok\nend",
        [5, 5, 3, 1, 0, 0],
    ),
    // t08_comment_before_the_do_of_a_bare_head
    (
        b"defmodule M do\n@doc \"audit\"\ndef f # c\n do\n:ok\nend\nend",
        [7, 7, 4, 1, 0, 0],
    ),
    // t09_comment_at_the_end_of_every_row
    (
        concat!(
            "defmodule M do # c\n@doc \"audit\" # c\ndef f(), do: :ok # c\nen",
            "d"
        )
        .as_bytes(),
        [4, 4, 3, 3, 0, 0],
    ),
    // t10_two_comments_in_a_guarded_macro
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndefmacro m(x) # c\n when is_atom",
            "(x) # d\n do\nx\nend\nend"
        )
        .as_bytes(),
        [8, 8, 4, 2, 0, 0],
    ),
    // t11_comment_in_the_parentheses_of_a_moduledoc
    (
        b"defmodule M do\n@moduledoc( # c\n\"audit\")\nend",
        [4, 4, 2, 1, 0, 0],
    ),
    // t12_comment_in_an_empty_argument_list
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f( # c\n), do: :ok\ndef f(),",
            " do: :error\nend"
        )
        .as_bytes(),
        [6, 6, 4, 1, 0, 0],
    ),
    // t13_guarded_clause_with_a_comment_joins_its_group
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when x > 0, do: x",
            "\ndef f(x), do: x\nend"
        )
        .as_bytes(),
        [6, 6, 4, 1, 0, 0],
    ),
    // s01_module_after_a_module_at_file_level
    (
        concat!(
            "defmodule A do\n@moduledoc \"a\"\nend defmodule X do\n@moduledoc",
            " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 6, 0, 0, 0],
    ),
    // s02_module_after_a_definition_in_a_body
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend defmodule X do\n@moduledoc ",
            "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [9, 9, 7, 0, 0, 0],
    ),
    // s03_module_after_an_expression_in_a_definition
    (
        concat!(
            "defmodule M do\ndef g() do\n:ok defmodule X do\n@moduledoc \"x\"",
            "\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
        )
        .as_bytes(),
        [9, 9, 7, 0, 0, 0],
    ),
    // s04_module_in_a_block_of_a_call
    (
        concat!(
            "if true do\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc",
            " \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 7, 0, 0, 0],
    ),
    // s05_module_in_an_after_block
    (
        concat!(
            "try do\n:ok\nafter\nfn -> :ok end defmodule X do\n@moduledoc \"x",
            "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [9, 9, 9, 0, 0, 0],
    ),
    // s06_module_in_an_else_block
    (
        concat!(
            "if true do\n:ok\nelse\nfn -> :ok end defmodule X do\n@moduledoc ",
            "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [9, 9, 9, 0, 0, 0],
    ),
    // s07_module_in_a_rescue_block
    (
        concat!(
            "try do\n:ok\nrescue\nfn -> :ok end defmodule X do\n@moduledoc \"",
            "x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [9, 9, 9, 0, 0, 0],
    ),
    // s08_module_in_a_catch_block
    (
        concat!(
            "try do\n:ok\ncatch\nfn -> :ok end defmodule X do\n@moduledoc \"x",
            "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [9, 9, 9, 0, 0, 0],
    ),
    // s09_module_in_the_body_of_a_clause
    (
        concat!(
            "f = fn ->\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 7, 0, 0, 0],
    ),
    // s10_module_in_a_parenthesized_block
    (
        concat!(
            "(\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
            "\"\ndef f(), do: :ok\nend\n)"
        )
        .as_bytes(),
        [7, 7, 7, 0, 0, 0],
    ),
    // s13_module_after_an_expression_at_file_level
    (
        concat!(
            "x = 1 defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
            " do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // s19_module_after_a_continued_row
    (
        concat!(
            "x = 1 \\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef ",
            "f(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // s11_clause_after_a_clause_of_an_anonymous_function
    (
        concat!(
            "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
            "\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // s12_module_below_a_joined_definition
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend def g() do\ndefmodule X do",
            "\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\n",
            "end"
        )
        .as_bytes(),
        [11, 11, 8, 0, 0, 0],
    ),
    // s14_module_after_a_string
    (
        concat!(
            "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
            "udit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // s15_joined_attribute_in_a_definition
    (
        b"defmodule M do\ndef g() do\n:ok @doc \"audit\"\nend\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // s16_quote_below_a_joined_statement
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend if true do\nquote do\n@doc ",
            "\"audit\"\nend\nend\nend"
        )
        .as_bytes(),
        [9, 9, 6, 0, 0, 0],
    ),
    // s22_joined_quote
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend quote do\n@doc \"audit\"\ne",
            "nd\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // s17_joined_module_in_a_quote
    (
        concat!(
            "quote do\n:ok defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\n",
            "def f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 6, 0, 0, 0],
    ),
    // s18_joined_protocol
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend defprotocol P do\n@doc \"au",
            "dit\"\ndef f(x)\nend\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // s20_joined_guard_definition
    (
        b"defmodule M do\ndef x() do\n:ok\nend defguard g(x) when x\nend",
        [5, 5, 4, 0, 0, 0],
    ),
    // s21_joined_attribute_at_file_level
    (b"x = 1 @doc \"audit\"\n", [1, 1, 1, 0, 0, 0]),
    // u01_module_after_a_line_end
    (
        concat!(
            "defmodule M do\ndef x() do\n:ok\nend\ndefmodule X do\n@moduledoc",
            " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [10, 10, 7, 0, 0, 0],
    ),
    // u02_module_after_a_semicolon
    (
        concat!(
            "x = 1; defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f()",
            ", do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 5, 0, 0, 0],
    ),
    // u03_module_after_a_comment_and_a_line_end
    (
        concat!(
            "x = 1 # c\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
            " f(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 6, 5, 1, 0, 0],
    ),
    // u04_module_as_the_right_side_of_a_match
    (
        concat!(
            "x = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), d",
            "o: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // u05_module_as_an_argument_of_a_call
    (
        concat!(
            "IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
            " f(), do: :ok\nend)"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // u07_module_after_a_semicolon_in_a_clause
    (
        concat!(
            "f = fn -> :ok; defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
            "\ndef f(), do: :ok\nend end"
        )
        .as_bytes(),
        [5, 5, 6, 0, 0, 0],
    ),
    // u06_module_in_a_rescue_section_on_the_row_of_a_statement
    (
        concat!(
            "try do :ok rescue _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"",
            "audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [6, 6, 7, 0, 0, 0],
    ),
    // u08_module_in_an_after_section_on_the_row_of_a_statement
    (
        concat!(
            "try do :ok after\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit",
            "\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 7, 0, 0, 0],
    ),
    // u09_module_in_a_catch_section_on_the_row_of_a_statement
    (
        concat!(
            "try do :ok catch _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"a",
            "udit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [6, 6, 7, 0, 0, 0],
    ),
    // u10_module_in_an_else_section_on_the_row_of_a_statement
    (
        concat!(
            "if true do :ok else\ndefmodule X do\n@moduledoc \"x\"\n@doc \"au",
            "dit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 7, 0, 0, 0],
    ),
    // v01_expression_read_into_a_match
    (
        concat!(
            "1 y = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
            " do: :ok\nend"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // v02_call_read_into_the_next_call
    (
        concat!(
            "f() if true do\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
            "\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // v03_expression_read_into_a_qualified_call
    (
        concat!(
            "1 IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\nd",
            "ef f(), do: :ok\nend)"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // v07_attribute_read_into_the_next_definition
    (
        concat!(
            "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
            "oc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // v06_recovery_among_the_clauses_of_a_function
    (
        concat!(
            "f = fn\n1 1 -> :ok\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // v04_definition_read_into_the_next_definition
    (
        concat!(
            "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
            "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
        )
        .as_bytes(),
        [9, 9, 6, 0, 0, 0],
    ),
    // v05_error_in_a_head_and_a_sound_statement_after_it
    (
        concat!(
            "defmodule M do\n@doc \"one\"\ndef a(, do: (\ndefmodule X do\n@mo",
            "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\n)\n@doc \"",
            "two\"\ndef b(), do: :ok\nend"
        )
        .as_bytes(),
        [12, 12, 9, 0, 0, 0],
    ),
    // w01_string_without_its_end
    (
        concat!(
            "defmodule M do\nx = \"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc \"",
            "REAL\"\ndef real, do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // w02_quoted_atom_without_its_end
    (
        concat!(
            "defmodule M do\nx = :\"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc ",
            "\"REAL\"\ndef real, do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // w03_statements_before_the_tear_are_read
    (
        concat!(
            "defmodule M do\n@doc \"one\"\ndef a(), do: :ok\nx = \"s\n@doc \"",
            "two\"\ndef b(), do: :ok\n@doc \"three\"\ndef c(), do: :ok\nend"
        )
        .as_bytes(),
        [9, 9, 6, 0, 0, 0],
    ),
    // w04_doubled_quote
    (
        concat!(
            "defmodule M do\n@doc \"\"audit\"\ndef f(), do: :ok\n@doc \"next",
            "\"\ndef g(), do: :ok\nend"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // w05_module_behind_a_tear_at_file_level
    (
        concat!(
            "x = \"a\ny = \"b\"\ndefmodule X do\n@moduledoc \"x\"\n@doc \"aud",
            "it\"\ndef f(), do: :ok\nend"
        )
        .as_bytes(),
        [7, 7, 4, 0, 0, 0],
    ),
    // w06_tear_in_the_body_of_a_definition
    (
        concat!(
            "defmodule M do\ndef h() do\nx = \"a\ny = \"b\"\nend\n@doc \"REAL",
            "\"\ndef real(), do: :ok\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // w07_two_torn_literals
    (
        concat!(
            "defmodule M do\nx = \"s\n@doc \"two\"\ndef b(), do: :ok\n@doc \"",
            "three\"\ndef c(), do: :ok\ny = \"t\n@doc \"four\"\ndef d(), do: ",
            ":ok\nend"
        )
        .as_bytes(),
        [10, 10, 5, 0, 0, 0],
    ),
    // o11_gap_before_a_module_at_file_level
    (
        concat!(
            ":ok\n# x\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef ",
            "f(), do: :ok\nend"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // o12_excluded_gap_after_an_escaped_line_feed
    (
        concat!(
            "defmodule M do\n@doc \"audit\"\n@foo ?\\\n# x\ndef f(), do: :ok",
            "\nend"
        )
        .as_bytes(),
        [6, 5, 4, 1, 0, 0],
    ),
    // x01_escaped_line_feed_at_file_level
    (
        concat!(
            "?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x02_escaped_line_feed_in_a_module_body
    (
        concat!(
            "defmodule O do\n  ?\\\n  defmodule M do\n    @moduledoc \"module",
            "\"\n    @doc \"function\"\n    def f(), do: :ok\n  end\nend\n"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // x03_escaped_line_feed_in_a_definition_body
    (
        concat!(
            "defmodule O do\n  def g() do\n    ?\\\ndefmodule M do\n  @module",
            "doc \"module\"\n  @doc \"function\"\n  def f(), do: :ok\nend\n  ",
            "end\nend\n"
        )
        .as_bytes(),
        [10, 10, 7, 0, 0, 0],
    ),
    // x04_escaped_line_feed_in_a_block_of_a_call
    (
        concat!(
            "if true do\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit",
            "\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // x05_escaped_line_feed_in_an_after_block
    (
        concat!(
            "try do\n:ok\nafter\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [10, 10, 8, 0, 0, 0],
    ),
    // x06_escaped_line_feed_in_an_else_block
    (
        concat!(
            "if true do\n:ok\nelse\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@d",
            "oc \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [10, 10, 8, 0, 0, 0],
    ),
    // x07_escaped_line_feed_in_a_rescue_block
    (
        concat!(
            "try do\n:ok\nrescue\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc",
            " \"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [10, 10, 8, 0, 0, 0],
    ),
    // x08_escaped_line_feed_in_a_catch_block
    (
        concat!(
            "try do\n:ok\ncatch\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc ",
            "\"audit\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [10, 10, 8, 0, 0, 0],
    ),
    // x09_escaped_line_feed_in_the_body_of_a_clause
    (
        concat!(
            "f = fn ->\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
            "\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // x10_escaped_line_feed_in_parentheses
    (
        concat!(
            "(?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n)\n"
        )
        .as_bytes(),
        [7, 7, 6, 0, 0, 0],
    ),
    // x11_escaped_line_feed_between_clauses_of_a_function
    (
        concat!(
            "fn\n1 -> ?\\\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audi",
            "t\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // x12_escaped_line_feed_before_doc
    (
        concat!(
            "defmodule O do\n  ?\\\n  @doc \"function\"\n  def f(), do: :ok\n",
            "end\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // x13_escaped_line_feed_after_moduledoc_before_doc
    (
        concat!(
            "defmodule M do\n  @moduledoc \"module\"\n  ?\\\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x14_escaped_line_feed_between_doc_and_definition
    (
        concat!(
            "defmodule M do\n  @doc \"function\"\n  ?\\\n  def f(), do: :ok\n",
            "end\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // x16_assigned_escaped_line_feed_between_doc_and_definition
    (
        concat!(
            "defmodule M do\n  @doc \"function\"\n  x = ?\\\n  def f(), do: :",
            "ok\nend\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // x15_escaped_line_feed_as_the_value_of_doc
    (
        b"defmodule M do\n  @doc ?\\\n  def f(), do: :ok\nend\n",
        [4, 4, 3, 0, 0, 0],
    ),
    // x17_assigned_escaped_line_feed_at_file_level
    (
        concat!(
            "x = ?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
            "ion\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x18_escaped_line_feed_then_a_continued_row
    (
        concat!(
            "?\\\n\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
            "ion\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // x19_bare_question_mark_and_line_feed_at_file_level
    (
        concat!(
            "?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x20_bare_question_mark_and_line_feed_in_a_module_body
    (
        concat!(
            "defmodule O do\n  ?\n  defmodule M do\n    @moduledoc \"module\"",
            "\n    @doc \"function\"\n    def f(), do: :ok\n  end\nend\n"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // x21_bare_question_mark_after_a_character_token
    (
        concat!(
            "???\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x22_bare_question_mark_and_line_feed_in_a_clause
    (
        concat!(
            "fn\n1 -> ?\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
            "\"\ndef f(), do: :ok\nend\nend"
        )
        .as_bytes(),
        [8, 8, 5, 0, 0, 0],
    ),
    // x23_line_feed_after_a_comma
    (
        concat!(
            "foo,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // x24_two_line_feeds_after_a_comma
    (
        concat!(
            "foo,\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
            "on\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 6, 5, 0, 1, 0],
    ),
    // x25_comment_after_a_comma
    (
        concat!(
            "foo, # c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
            "tion\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 1, 0, 0],
    ),
    // x26_semicolon_in_a_comment_after_a_comma
    (
        concat!(
            "foo, # c; d\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"f",
            "unction\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 1, 0, 0],
    ),
    // x27_semicolon_character_without_a_separator
    (
        concat!(
            "?;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"\n",
            "  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [5, 5, 5, 0, 0, 0],
    ),
    // x28_continued_row_after_a_question_mark_identifier
    (
        concat!(
            "foo?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
            "on\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // y01_second_line_feed_after_an_escaped_line_feed
    (
        concat!(
            "?\\\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 6, 5, 0, 1, 0],
    ),
    // y02_semicolon_after_an_escaped_line_feed
    (
        concat!(
            "?\\\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y03_comment_after_an_escaped_line_feed
    (
        concat!(
            "?\\\n# c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
            "tion\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // y04_escaped_backslash_character
    (
        concat!(
            "?\\\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y05_plain_character
    (
        concat!(
            "?a\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y06_identifier_ending_in_a_question_mark
    (
        concat!(
            "foo?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y07_atom_ending_in_a_question_mark
    (
        concat!(
            ":a?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y08_string_ending_in_question_mark_and_backslash
    (
        concat!(
            "\"a?\\\\\"\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"fu",
            "nction\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y09_escaped_line_feed_inside_a_call
    (
        concat!(
            "f(?\\\n)\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
            "tion\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 7, 5, 0, 0, 0],
    ),
    // y10_escaped_line_feed_then_an_operator_row
    (
        concat!(
            "x = ?\\\n+ 1\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"",
            "function\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 7, 6, 0, 0, 0],
    ),
    // y11_escaped_space_character
    (
        concat!(
            "?\\ \ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y12_space_character
    (
        concat!(
            "? \ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y13_two_escaped_line_feeds_then_a_blank_row
    (
        concat!(
            "?\\\n?\\\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"fu",
            "nction\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [8, 7, 6, 0, 1, 0],
    ),
    // y14_escaped_hash_character
    (
        concat!(
            "?\\#\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y15_hash_character
    (
        concat!(
            "?#\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y16_line_feed_then_a_semicolon
    (
        concat!(
            "1\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y17_leading_semicolon
    (
        concat!(
            ";defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"\n ",
            " def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // y18_two_semicolons
    (
        concat!(
            "1;;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [5, 5, 6, 0, 0, 0],
    ),
    // y19_second_line_feed_after_a_bare_question_mark
    (
        concat!(
            "?\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 6, 5, 0, 1, 0],
    ),
    // y20_semicolon_after_a_bare_question_mark
    (
        concat!(
            "?\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // y21_comment_after_a_bare_question_mark
    (
        concat!(
            "?\n# c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
            "on\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [7, 6, 5, 1, 0, 0],
    ),
    // y22_pipe_operator_behind_an_escaped_line_feed
    (
        concat!(
            "?\\\n|> defmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
            "on\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // y23_dot_call_behind_an_escaped_line_feed
    (
        concat!(
            "?\\\n.foo(defmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
            "tion\"\n  def f(), do: :ok\nend)\n"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // y24_minus_on_one_row
    (
        concat!(
            "?a - defmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // y25_access_on_one_row
    (
        concat!(
            "?a[defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
            "\n  def f(), do: :ok\nend]\n"
        )
        .as_bytes(),
        [5, 5, 4, 0, 0, 0],
    ),
    // y26_comma_in_a_list
    (
        concat!(
            "[1,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend]\n"
        )
        .as_bytes(),
        [6, 6, 4, 0, 0, 0],
    ),
    // y27_semicolon_after_a_comma
    (
        concat!(
            "foo,\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 6, 0, 0, 0],
    ),
    // y28_semicolon_after_a_comment_after_a_comma
    (
        concat!(
            "foo, # c\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"fun",
            "ction\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 6, 1, 0, 0],
    ),
    // y29_clause_with_a_body_before_a_module_clause
    (
        concat!(
            "case x do\n  a -> 1\n  b -> defmodule X do\n    @moduledoc \"m\"",
            "\n    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
        )
        .as_bytes(),
        [8, 8, 6, 0, 0, 0],
    ),
    // z01_comment_with_a_rejected_character
    (
        b"defmodule M do\n@doc \"audit\" # c\x0c d\ndef f(), do: :ok\nend",
        [4, 4, 3, 1, 0, 0],
    ),
    // z02_escaped_carriage_return_character
    (
        concat!(
            "?\\\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // z03_carriage_return_character
    (
        concat!(
            "?\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
            "\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
    // z04_empty_clause_body
    (
        concat!(
            "case x do\n  a ->\n  b -> defmodule X do\n    @moduledoc \"m\"\n",
            "    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
        )
        .as_bytes(),
        [8, 8, 5, 0, 0, 0],
    ),
    // z05_operator_row_behind_an_escaped_line_feed
    (
        concat!(
            "?\\\n- defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
            "n\"\n  def f(), do: :ok\nend\n"
        )
        .as_bytes(),
        [6, 6, 5, 0, 0, 0],
    ),
];

// The LOC tuple of raw Elixir bytes and its documentation rows.
#[cfg(feature = "elixir")]
fn elixir_loc(source: &[u8]) -> ([u64; 6], Vec<usize>) {
    let space = analyze(
        Source::new(LANG::Elixir, source),
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
        (0..source.len())
            .filter(|row| loc.dloc.lines.contains(*row))
            .collect(),
    )
}

#[cfg(feature = "elixir")]
#[test]
fn documentation_elixir_legacy_rows_hold_no_attribute_documentation() {
    for (source, loc) in ELIXIR_ATTRIBUTE_SOURCES {
        // expected: an attribute is code for the metric; no row is a documentation row.
        assert_eq!(
            elixir_loc(source),
            (loc, Vec::new()),
            "{}",
            String::from_utf8_lossy(source)
        );
    }
}
#[cfg(feature = "elixir")]
#[test]
fn documentation_elixir_source_and_legacy_views_are_independent() {
    use big_code_analysis_ast::documentation::*;
    let mut totals = [0; 4];
    for (source, loc) in ELIXIR_ATTRIBUTE_SOURCES {
        let name = String::from_utf8_lossy(source);
        let parser =
            crate::langs::AnyParser::parse(LANG::Elixir, source.to_vec(), None, None).unwrap();
        let idle = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let texts = index
            .documents()
            .filter(|document| matches!(document.get().content(), Content::Complete { .. }))
            .count();
        // The collection moves each counter on this thread, so a zero across `analyze`
        // is a reading.
        // expected: the walk enters every node once.
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{name}"
        );
        // expected: one render per complete text; a pending value is not rendered.
        assert_eq!(collected.renders - idle.renders, texts, "{name}");
        // expected: the file, and every module and group that became an owner.
        assert_eq!(
            collected.owner_projections - idle.owner_projections,
            index.facts().owners().len(),
            "{name}"
        );
        assert_ne!(collected.polls - idle.polls, 0, "{name}");
        totals[0] += index.documents().len();
        totals[1] += texts;
        totals[2] += index.associations().len();
        totals[3] += index.facts().owners().len();
        assert_eq!(elixir_loc(source), (loc, Vec::new()), "{name}");
        let analyzed = observations();
        // expected: `analyze` builds no documentation index.
        assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
        assert_eq!(analyzed.renders - collected.renders, 0, "{name}");
        assert_eq!(
            analyzed.owner_projections - collected.owner_projections,
            0,
            "{name}"
        );
        assert_eq!(analyzed.polls - collected.polls, 0, "{name}");
    }
    // expected: the documents, complete texts, associations and owners of all rows, as
    // the views of `collector/elixir_tests.rs` state them for a parse of the whole source.
    assert_eq!(totals, [232, 192, 232, 737]);
}

#[cfg(feature = "csharp")]
#[test]
#[allow(clippy::too_many_lines)]
fn documentation_csharp_unit2_declarations_keep_legacy_rows() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    struct Row {
        source: &'static [u8],
        loc: [u64; 6],
        dloc_rows: &'static [usize],
        documents: usize,
    }
    let rows = [
        Row {
            source: b"class C {\n///both\nint a, b;\n}",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            source: b"class C {\n///one\nconst int K = 1;\n}",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            source: b"class C {\n///both\nevent System.Action E1, E2;\n}",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            source: b"class C {\n///first\nint a;\n///second\nevent System.Action B;\n}",
            loc: [6, 4, 0, 2, 0, 2],
            dloc_rows: &[1, 3],
            documents: 2,
        },
        Row {
            source: b"class C {\n///broken\nconst int K = ;\n}",
            loc: [4, 3, 0, 1, 0, 1],
            dloc_rows: &[1],
            documents: 1,
        },
        Row {
            source: b"class C {\n#if X\n///branch\nint a;\n#endif\n}",
            loc: [6, 5, 0, 1, 0, 1],
            dloc_rows: &[2],
            documents: 1,
        },
        Row {
            source: b"class C { void M() { ///local\nint x = 1; } }",
            loc: [2, 2, 1, 1, 0, 0],
            dloc_rows: &[],
            documents: 0,
        },
    ];
    let mut total_renders = 0;
    for row in rows {
        let parser =
            crate::langs::AnyParser::parse(LANG::Csharp, row.source.to_vec(), None, None).unwrap();
        let idle = observations();
        let idle_evidence = evidence_built::observed();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let built = evidence_built::observed();
        // expected: the walker visits each node exactly once; one render per document.
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count()
        );
        assert_eq!(index.documents().len(), row.documents);
        assert_eq!(collected.renders - idle.renders, row.documents);
        total_renders += collected.renders - idle.renders;
        // expected: each selected leading XML comment builds one evidence fragment.
        assert_eq!(built - idle_evidence, row.documents);
        let space = analyze(
            Source::new(LANG::Csharp, row.source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = observations();
        assert_eq!(analyzed.visits - collected.visits, 0);
        assert_eq!(analyzed.renders - collected.renders, 0);
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
            row.loc,
            "{}",
            row.source.escape_ascii(),
        );
        assert_eq!(
            (0..row.source.len())
                .filter(|byte| loc.dloc.lines.contains(*byte))
                .collect::<Vec<_>>(),
            row.dloc_rows,
        );
    }
    assert_ne!(total_renders, 0);
}

#[cfg(feature = "ruby")]
#[test]
fn documentation_ruby_unit2_legacy_rows_stay_in_place() {
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    for (source, expected, rows) in [
        (
            b"class C\n# audit\nprivate def f; end\nend".as_slice(),
            [4, 3, 2, 1, 0, 1],
            &[1][..],
        ),
        (
            b"class C\n# a\npublic def self.g; end\n# b\nprivate_class_method def self.h; end\n# c\nmodule_function def k; end\n# d\nprotected def e; end\nend".as_slice(),
            [10, 6, 5, 4, 0, 4],
            &[1, 3, 5, 7],
        ),
    ] {
        let parser = crate::langs::AnyParser::parse(LANG::Ruby, source.to_vec(), None, None)
            .unwrap();
        let idle = observations();
        let index = parser
            .documentation(&DocumentationContext::default(), &DocumentationOptions::default())
            .unwrap();
        let collected = observations();
        assert_ne!(index.documents().len(), 0);
        assert!(collected.visits > idle.visits);
        assert!(collected.renders > idle.renders);
        let space = analyze(
            Source::new(LANG::Ruby, source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let loc = &space.metrics.loc;
        assert_eq!(
            [loc.sloc(), loc.ploc(), loc.lloc(), loc.cloc(), loc.blank(), loc.dloc()],
            expected,
        );
        assert_eq!(
            (0..source.len())
                .filter(|row| loc.dloc.lines.contains(*row))
                .collect::<Vec<_>>(),
            rows,
        );
        assert_eq!(observations().visits, collected.visits);
        assert_eq!(observations().renders, collected.renders);
    }
}
#[cfg(feature = "php")]
#[test]
fn documentation_php_unit2_legacy_guard_before_and_after_projection() {
    use big_code_analysis_ast::documentation::included_range_visits;
    use big_code_analysis_ast::documentation::streaming::markers::{
        evidence_built, stretches_collected,
    };
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    // Whole LOC tuples are the measured Unit 1 guard rows; DLOC rows are source-line indices.
    let cases: &[(&[u8], [u64; 6], &[usize])] = &[
        (b"<?php\nclass C {\n    /** d */\n    public $p;\n}\n", [5, 4, 1, 1, 0, 1], &[2]),
        (b"<?php\nclass C {\n    /** d */\n    const K = 1;\n}\n", [5, 4, 1, 1, 0, 1], &[2]),
        (b"<?php\nclass C {\n    /** ab */\n    public $a, $b = 2;\n    /** kl */\n    const A = 1, B = 2;\n}\n", [7, 5, 2, 2, 0, 2], &[2, 4]),
        (b"<?php class C { function __construct(/** audit */ public int $x) {} }\n", [1, 1, 0, 1, 0, 1], &[0]),
    ];
    let counters = || {
        [
            observations().renders,
            observations().visits,
            evidence_built::observed(),
            stretches_collected::observed(),
            included_range_visits::observed(),
            observations().intersection_searches,
        ]
    };
    let mut moved = 0;
    for (case, &(source, expected, rows)) in cases.iter().enumerate() {
        let parser =
            crate::langs::AnyParser::parse(LANG::Php, source.to_vec(), None, None).unwrap();
        let before = counters();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = counters();
        assert_eq!(after[0] - before[0], index.documents().len());
        assert!(after[1] > before[1]);
        assert!(after[2] > before[2]);
        assert!(after[3] > before[3]);
        assert!(after[4] > before[4]);
        assert!(after[5] > before[5]);
        moved += after[0] - before[0];
        let space = analyze(
            Source::new(LANG::Php, source),
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
            expected
        );
        assert_eq!(
            (0..source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            rows
        );
        let end = counters();
        // The legacy collector scans the bound member runs for the first three cases;
        // the promoted signature has no bound run. The other five source counters stay zero.
        assert_eq!(end[0] - after[0], 0);
        assert_eq!(end[1] - after[1], 0);
        assert_eq!(end[2] - after[2], 0);
        assert_eq!(end[3] - after[3], [1, 1, 2, 0][case]);
        assert_eq!(end[4] - after[4], 0);
        assert_eq!(end[5] - after[5], 0);
    }
    assert_eq!(moved, 5);
}

#[cfg(feature = "lua")]
#[test]
fn documentation_lua_unit2_sources_keep_their_legacy_rows() {
    let rows = [
        LuaLegacyRow {
            name: "tagged_following_function",
            source: b"module(\"m\")\n--- audit\n-- @param x thing\nfunction f(x) end\n",
            loc: [4, 2, 1, 2, 0, 2],
            dloc_rows: &[1, 2],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "single_local",
            source: b"module(\"m\")\n--- audit\nlocal a = 1\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "dotted_assignment",
            source: b"module(\"m\")\n--- audit\nM.a = 1\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "returned_table",
            source: b"module(\"m\")\n--- audit\nreturn {}\n",
            loc: [3, 2, 1, 1, 0, 1],
            dloc_rows: &[1],
            comment_runs: 1,
        },
        LuaLegacyRow {
            name: "named_field",
            source: b"module(\"m\")\nlocal t = {\n--- detail\nalpha = 1,\n}\n",
            loc: [5, 4, 1, 1, 0, 1],
            dloc_rows: &[2],
            comment_runs: 1,
        },
    ];
    for row in rows {
        // expected: each selected run renders one document, so the helper returns one.
        assert_eq!(check_lua_legacy_row(&row), 1);
    }
}

#[cfg(feature = "typescript")]
#[test]
fn documentation_ts_unit2_owners_do_not_change_legacy_rows() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::{AnyParser, documentation::*};

    let sources: &[(&[u8], &[usize], usize)] = &[
        (b"/**audit*/\nexport default function() {}\n", &[0], 1),
        (b"/**audit*/\nexport default function* () {}\n", &[0], 1),
        (b"/**audit*/\nexport default class {}\n", &[0], 1),
        (
            b"/** above */\ndeclare /** inside */ function f(): void;\n",
            &[0],
            1,
        ),
        (b"/** d */\nnamespace N {}\n", &[0], 1),
        (b"/** d */\nexport namespace N {}\n", &[0], 1),
        (b"/** d */\ndeclare namespace N {}\n", &[0], 1),
        (b"/**audit*/\nconst x = 1;\n", &[0], 1),
        (b"/**audit*/\nlet x: number;\n", &[0], 1),
        (b"/**audit*/\nvar x = 1;\n", &[0], 1),
        (b"/**audit*/\nexport const x = 1;\n", &[0], 1),
        (b"/**audit*/\ndeclare const x: number;\n", &[0], 1),
        (b"/**audit*/\nusing h = open();\n", &[0], 1),
        (b"/**audit*/\nawait using h = open();\n", &[0], 1),
        (b"/**audit*/\nawait /* gap */ using h = open();\n", &[0], 1),
        (b"/**audit*/\nconst { x } = o;\n", &[], 0),
        (b"/**audit*/\nx = 1;\n", &[], 0),
        (
            b"/**one*/\nconst a = 1;\n/**two*/\nnamespace N {}\n",
            &[0, 2],
            2,
        ),
    ];
    let mut rendered = 0;
    for language in [LANG::Typescript, LANG::Tsx] {
        for &(source, expected_rows, expected_documents) in sources {
            let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
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
            let space = analyze(
                Source::new(language, source),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            let analyzed = observations();
            let loc = &space.metrics.loc;
            assert_eq!(
                (0..source.len())
                    .filter(|at| loc.dloc.lines.contains(*at))
                    .collect::<Vec<_>>(),
                expected_rows,
                "{language:?}: {}",
                source.escape_ascii()
            );
            assert_eq!(loc.dloc(), expected_rows.len() as u64);
            assert_eq!(index.documents().len(), expected_documents);
            // expected: one selected block contributes one marker member.
            assert_eq!(built - unbuilt, expected_documents);
            // expected: the collector visits each tree node once on entry.
            assert_eq!(
                collected.visits - idle.visits,
                parser.root_node().preorder().count()
            );
            // expected: one completed render for each selected document.
            assert_eq!(collected.renders - idle.renders, expected_documents);
            rendered += collected.renders - idle.renders;
            // expected: legacy analyze never enters a source-view walk or renders text.
            assert_eq!(analyzed.visits, collected.visits);
            assert_eq!(analyzed.renders, collected.renders);
            assert_eq!(evidence_built::observed(), built);
        }
    }
    assert!(rendered > 0);
}

#[cfg(feature = "groovy")]
#[test]
fn documentation_groovy_new_field_spellings_keep_legacy_rows() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::{AnyParser, documentation::*};
    let rows: [(&[u8], [u64; 6], &[usize]); 5] = [
        (
            b"class C {\n/**audit*/\nint a = 1, b = 2\n}\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"class C {\n/**audit*/\nstatic final int X = 1\n}\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"trait T {\n/**audit*/\ndef x\n}\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"enum E { A;\n/**audit*/\nint x\n}\n",
            [4, 3, 0, 1, 0, 1],
            &[1],
        ),
        (
            b"class C {\n/**audit*/\nint a, /* plain */ b\n}\n",
            [4, 3, 0, 2, 0, 1],
            &[1],
        ),
    ];
    for (source, expected, doc_rows) in rows {
        let parser = AnyParser::parse(LANG::Groovy, source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error());
        let before = observations();
        let unbuilt = evidence_built::observed();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let built = evidence_built::observed();
        assert_eq!(index.documents().len(), 1, "{}", source.escape_ascii());
        assert_eq!(
            collected.visits - before.visits,
            parser.root_node().preorder().count()
        );
        assert_eq!(collected.renders - before.renders, 1);
        assert_eq!(built - unbuilt, 1);
        let space = analyze(
            Source::new(LANG::Groovy, source),
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
            expected,
            "{}",
            source.escape_ascii()
        );
        assert_eq!(
            (0..source.len())
                .filter(|row| loc.dloc.lines.contains(*row))
                .collect::<Vec<_>>(),
            doc_rows,
            "{}",
            source.escape_ascii()
        );
        let after = observations();
        assert_eq!(after.visits - collected.visits, 0);
        assert_eq!(after.renders - collected.renders, 0);
        assert_eq!(evidence_built::observed() - built, 0);
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[test]
fn documentation_c_family_unit2_legacy_rows_stay_independent() {
    use big_code_analysis_ast::documentation::streaming::markers::evidence_built;
    use big_code_analysis_ast::documentation::{
        DocumentationContext, DocumentationOptions, observations,
    };
    const LANGUAGES: [LANG; 4] = [LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc];
    let quoted = documentation::source("doxygen-quoted.h");
    let quote = documentation::source("doxygen-quote.h");
    let cdata = documentation::source("doxygen-cdata.h");
    let rows: [(&[u8], [u64; 6], &[usize]); 7] = [
        (&quoted, [9, 4, 1, 5, 0, 5], &[0, 2, 4, 6, 7]),
        (&quote, [2, 1, 0, 1, 0, 0], &[]),
        (&cdata, [2, 1, 0, 1, 0, 0], &[]),
        (b"enum E { A, /**< ta */ B };\n", [1, 1, 0, 1, 0, 1], &[0]),
        (b"int a, /**< ta */ b;\n", [1, 1, 1, 1, 0, 1], &[0]),
        (
            b"/** e */\nextern \"C\" void f(void);\n",
            [2, 1, 1, 1, 0, 1],
            &[0],
        ),
        (
            b"/** e */\nextern \"C\" { void f(void); void g(void); }\n",
            [2, 1, 2, 1, 0, 1],
            &[0],
        ),
    ];
    let mut ran = 0;
    let mut rendered = 0;
    for lang in LANGUAGES {
        if !lang.is_enabled() {
            continue;
        }
        for (source, expected, dloc_rows) in rows {
            let parser = crate::langs::AnyParser::parse(lang, source.to_vec(), None, None).unwrap();
            assert!(!parser.ts_tree().root_node().has_error());
            let idle = observations();
            let old_built = evidence_built::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let collected = observations();
            let built = evidence_built::observed();
            // expected: one primary visit for each node, root included.
            assert_eq!(
                collected.visits - idle.visits,
                parser.root_node().preorder().count()
            );
            // expected: each published document is rendered exactly once.
            assert_eq!(collected.renders - idle.renders, index.documents().len());
            // expected: every selected marker contributes one physical fragment.
            assert_eq!(
                built - old_built,
                index
                    .documents()
                    .map(|d| d.get().fragment_count())
                    .sum::<usize>()
            );
            rendered += collected.renders - idle.renders;
            let metrics = analyze(
                Source::new(lang, source),
                MetricsOptions::default().with_only(&[Metric::Loc]),
            )
            .unwrap();
            let after = observations();
            // expected: analyze reads only legacy LOC; it enters no source collector node.
            assert_eq!(after.visits - collected.visits, 0);
            // expected: legacy LOC builds no source document text.
            assert_eq!(after.renders - collected.renders, 0);
            // expected: legacy LOC builds no source evidence for marker runs.
            assert_eq!(evidence_built::observed() - built, 0);
            let loc = &metrics.metrics.loc;
            assert_eq!(
                [
                    loc.sloc(),
                    loc.ploc(),
                    loc.lloc(),
                    loc.cloc(),
                    loc.blank(),
                    loc.dloc()
                ],
                expected,
                "{lang:?}: {}",
                source.escape_ascii()
            );
            assert_eq!(
                (0..source.len())
                    .filter(|row| loc.dloc.lines.contains(*row))
                    .collect::<Vec<_>>(),
                dloc_rows,
                "{lang:?}: {}",
                source.escape_ascii()
            );
            ran += 1;
        }
    }
    assert!(ran > 0);
    assert!(rendered > 0);
}

#[cfg(feature = "python")]
#[test]
fn documentation_python_semicolon_rows_are_characterized() {
    // The legacy view only: a `;` between two statements is an unnamed sibling, and the
    // statement after it is still "directly after" the one before it.
    let sources: [(&[u8], [u64; 6], &[usize]); 5] = [
        // Two module docstrings, the first with a `;` after it.
        (b"\"a\";\n\"b\"\nx = 1\n", [3, 2, 3, 2, 0, 2], &[0, 1]),
        // An attribute docstring after an assignment that ends with `;`.
        (b"x = 1;\n\"x doc\"\n", [2, 1, 2, 1, 0, 1], &[1]),
        // The same on one row.
        (b"x = 1; \"attribute\"\n", [1, 1, 2, 1, 0, 1], &[0]),
        // Two triple-quoted strings that share the row of the `;`.
        (
            b"\"\"\"a\nA\"\"\"; \"\"\"b\nB\"\"\"\n",
            [3, 1, 2, 3, 0, 3],
            &[0, 1, 2],
        ),
        // The control without a `;`.
        (b"\"a\"\n\"b\"\nx = 1\n", [3, 1, 3, 2, 0, 2], &[0, 1]),
    ];
    for (source, expected, rows) in sources {
        let name = source.escape_ascii();
        let space = analyze(
            Source::new(LANG::Python, source),
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
            expected,
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

#[cfg(feature = "python")]
#[test]
#[allow(clippy::too_many_lines)] // One row for each fixture, and both views of each row.
fn documentation_python_docstring_source_and_legacy_views_are_independent() {
    use DocumentRole::{Additional, Primary};
    use big_code_analysis_ast::documentation::{
        AssessmentAxis, DocumentRole, DocumentationContext, DocumentationOptions, observations,
    };
    struct Row<'a> {
        fixture: &'a str,
        // The text of each document in index order; `None` where it is unavailable.
        documents: &'a [(DocumentRole, Option<&'a str>)],
        // The runs after an assignment, which give one pending assessment each.
        attribute_runs: usize,
        loc: [u64; 6],
        dloc_rows: &'a [usize],
    }
    let rows = [
        // Rows 7 and 9, the `#:` comment and the string after `x = 1`, are legacy
        // documentation rows and no source documents in this unit.
        Row {
            fixture: "python-runs.py",
            documents: &[(Primary, Some("")), (Additional, Some("extra"))],
            attribute_runs: 1,
            loc: [10, 5, 7, 4, 1, 4],
            dloc_rows: &[1, 2, 7, 9],
        },
        Row {
            fixture: "python-paren.py",
            documents: &[(Primary, Some("@audit"))],
            attribute_runs: 0,
            loc: [3, 1, 1, 2, 0, 2],
            dloc_rows: &[1, 2],
        },
        Row {
            fixture: "python-extras.py",
            documents: &[(Primary, Some("doc"))],
            attribute_runs: 0,
            loc: [6, 2, 2, 4, 0, 2],
            dloc_rows: &[1, 4],
        },
        // The named escape is not decoded in this unit: a document without text.
        Row {
            fixture: "python-named.py",
            documents: &[(Primary, None)],
            attribute_runs: 0,
            loc: [3, 2, 2, 1, 0, 1],
            dloc_rows: &[1],
        },
        Row {
            fixture: "python-continuation.py",
            documents: &[(Primary, Some("primary")), (Additional, Some("additional"))],
            attribute_runs: 0,
            loc: [6, 2, 3, 2, 2, 2],
            dloc_rows: &[2, 4],
        },
    ];
    for row in rows {
        let name = row.fixture;
        let source = documentation::source(name);
        let parser =
            crate::langs::AnyParser::parse(LANG::Python, source.clone(), None, None).unwrap();
        let idle = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let collected = observations();
        let space = analyze(
            Source::new(LANG::Python, &source),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .unwrap();
        let analyzed = observations();

        let documents: Vec<_> = index
            .documents()
            .map(|document| {
                let complete = index.complete_text(document).unwrap();
                (
                    document.get().role(),
                    complete.map(|complete| complete.text().to_owned()),
                )
            })
            .collect();
        let expected: Vec<_> = row
            .documents
            .iter()
            .map(|(role, text)| (*role, text.map(str::to_owned)))
            .collect();
        assert_eq!(documents, expected, "{name}");
        assert_eq!(index.assessments().len(), row.attribute_runs, "{name}");
        assert!(
            index
                .assessments()
                .all(|assessment| assessment.get().axis == AssessmentAxis::Owner),
            "{name}"
        );

        // The collection moves both counters on this thread, so a zero across `analyze`
        // is a reading.
        // expected: the walk enters every node of the tree once.
        assert_eq!(
            collected.visits - idle.visits,
            parser.root_node().preorder().count(),
            "{name}"
        );
        // expected: 3 text builds for each document with complete text (decoded value,
        // tab expansion, margin) and none for a document without it.
        let complete = row.documents.iter().filter(|document| document.1.is_some());
        assert_eq!(
            collected.renders - idle.renders,
            3 * complete.count(),
            "{name}"
        );
        // expected: 0 and 0, because `analyze` builds no documentation index.
        assert_eq!(analyzed.visits - collected.visits, 0, "{name}");
        assert_eq!(analyzed.renders - collected.renders, 0, "{name}");

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
            (0..source.len())
                .filter(|at| loc.dloc.lines.contains(*at))
                .collect::<Vec<_>>(),
            row.dloc_rows,
            "{name}"
        );
    }
}
