//! Cross-language parity test for the two **AST export** walks.
//!
//! `Ast::dump` walks the tree with one cursor; `Ast::dump_indexed`
//! reaches each child by index. They share the alterator and the
//! comment filter, and differ only in how they enumerate children and
//! read grammar fields — which is where they can disagree: a cursor
//! answers a field from the path it descended, an index lookup from the
//! parent's field map, and the two resolve a field inherited through a
//! hidden grammar node by different algorithms (#1684). A disagreement
//! is a correctness finding to investigate, never something to
//! normalise away.
//!
//! The comparator is iterative: `AstNode` has no `PartialEq`, and a
//! recursive one would reintroduce the depth-sensitive operation #1056
//! removed. Serialized output is compared separately, on the shallow
//! fixtures only, so deep parity does not depend on the serialization
//! depth limit.

use big_code_analysis::{Ast, AstCfg, AstNode, AstResponse, LANG, Source};

use super::ops_metrics_space_parity::fixture;

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn cfg(span: bool, comment: bool) -> AstCfg {
    AstCfg {
        id: "parity".to_owned(),
        language: "echo".to_owned(),
        comment,
        span,
    }
}

/// Asserts the two responses describe the same tree, node for node,
/// without recursing. `what` names the fixture in a failure.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn assert_same_export(what: &str, left: &AstResponse, right: &AstResponse) -> usize {
    assert_eq!(
        (&left.id, &left.language),
        (&right.id, &right.language),
        "{what}: envelope"
    );
    let (Some(left_root), Some(right_root)) = (&left.root, &right.root) else {
        assert!(
            left.root.is_none() && right.root.is_none(),
            "{what}: one strategy exported a root and the other did not"
        );
        return 0;
    };
    let mut compared = 0;
    let mut pending: Vec<(&AstNode, &AstNode, String)> =
        vec![(left_root, right_root, String::from("root"))];
    while let Some((a, b, path)) = pending.pop() {
        compared += 1;
        assert_eq!(
            (a.r#type, &a.value, a.span, a.field_name, a.children.len()),
            (b.r#type, &b.value, b.span, b.field_name, b.children.len()),
            "{what}: node at {path} differs between the cursor and indexed exports"
        );
        pending.extend(
            a.children
                .iter()
                .zip(&b.children)
                .enumerate()
                .map(|(i, (x, y))| (x, y, format!("{path}/{i}:{}", x.r#type))),
        );
    }
    compared
}

/// Every enabled language's fixture, under every span/comment
/// combination, exports the same tree through the default, the explicit
/// cursor, and the explicit indexed entry points.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
#[test]
fn both_export_strategies_agree_in_every_language() {
    let mut checked = 0;
    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;
        let (source, _ext) = fixture(lang);
        let ast = Ast::parse(Source::new(lang, source.as_bytes())).expect("language is enabled");
        for (span, comment) in [(false, false), (true, false), (false, true), (true, true)] {
            let what = format!("{lang:?} span={span} comment={comment}");
            let default = ast.dump(cfg(span, comment));
            let cursor = ast.dump_cursor(cfg(span, comment));
            let indexed = ast.dump_indexed(cfg(span, comment));
            let compared = assert_same_export(&what, &cursor, &indexed);
            assert_same_export(&what, &default, &cursor);
            // `Ccomment` / `Preproc` parse a non-trivial tree too; every
            // fixture has more than a root, or the comparison above
            // held over nothing.
            assert!(compared > 1, "{what}: fixture exported {compared} node(s)");
            // The fixtures are shallow, so the serialized form is a
            // second, independent comparison that also covers ordering
            // of the keys the structural one reads field by field.
            assert_eq!(
                serde_json::to_string(&cursor).expect("serialize"),
                serde_json::to_string(&indexed).expect("serialize"),
                "{what}: serialized exports differ"
            );
        }
    }
    assert!(
        checked > 0,
        "at least one language feature must be enabled for this test to mean anything"
    );
}

/// The shapes the sweep's fixtures do not reach, each named for what it
/// exercises in the cursor walk.
#[cfg(any(
    feature = "perl",
    feature = "rust",
    feature = "tcl",
    feature = "csharp"
))]
#[test]
fn both_export_strategies_agree_on_the_shapes_the_fixtures_lack() {
    let wide = "fn f() {}\n".repeat(400);
    let flat = format!("my @r = ({}", "{\nlast,\nname => 1,\n".repeat(200));
    let nested = format!("{}puts hi{}", "eval {".repeat(40), "}".repeat(40));
    // Each row names the node kind that proves the shape was reached,
    // how many of it the export must hold, and whether that node is a
    // leaf: a fixture that lost its construct would still agree with
    // itself.
    let cases: [(LANG, &str, &str, &str, usize, bool); 7] = [
        // Empty input: the root has no first child, so the walk
        // finishes it without ever descending.
        (LANG::Rust, "empty", "", "source_file", 1, true),
        // One leaf under the root: descend once, finish, climb, finish.
        (LANG::Rust, "leaf", ";", "empty_statement", 1, false),
        // A wide `repeat()` list, stored balanced: the cursor steps
        // through hidden levels the index lookup skips by count.
        (LANG::Rust, "wide-list", &wide, "function_item", 400, false),
        // A flat recovery `ERROR`: the shape the issue measured.
        (LANG::Perl, "flat-error", &flat, "ERROR", 1, false),
        // A zero-width MISSING token the parser inserted: the cursor
        // visits it as a sibling and the index lookup by position.
        (
            LANG::Rust,
            "missing-token",
            "fn f() { let a = 1 }",
            ";",
            1,
            true,
        ),
        // Tcl script bodies keep their children through
        // `keeps_children`, which reads the ancestor chain (#1381).
        (LANG::Tcl, "script-body", &nested, "braced_word", 40, false),
        // A C# interpolated string the alterator flattens to one leaf.
        (
            LANG::Csharp,
            "interpolated",
            "class C { string s = $\"a{1}b\"; }",
            "interpolated_string_expression",
            1,
            true,
        ),
    ];
    let mut checked = 0;
    for (lang, name, source, kind, count, leaf) in cases {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;
        let ast = Ast::parse(Source::new(lang, source.as_bytes())).expect("language is enabled");
        for (span, comment) in [(false, false), (true, true)] {
            let what = format!("{lang:?}/{name} span={span} comment={comment}");
            let cursor = ast.dump_cursor(cfg(span, comment));
            let indexed = ast.dump_indexed(cfg(span, comment));
            assert_same_export(&what, &cursor, &indexed);
            let found = nodes_of_kind(&cursor, kind);
            assert_eq!(
                found.len(),
                count,
                "{what}: the fixture must export {count} `{kind}` node(s)"
            );
            assert!(
                found.iter().all(|node| node.children.is_empty() == leaf),
                "{what}: every `{kind}` must {} children",
                if leaf { "have no" } else { "keep its" }
            );
        }
    }
    assert!(checked > 0, "no case's language is enabled in this build");
}

/// The exported nodes of type `kind`, found iteratively.
#[cfg(any(
    feature = "perl",
    feature = "rust",
    feature = "tcl",
    feature = "csharp"
))]
fn nodes_of_kind<'a>(resp: &'a AstResponse, kind: &str) -> Vec<&'a AstNode> {
    let mut pending: Vec<&AstNode> = resp.root.iter().collect();
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        if node.r#type == kind {
            found.push(node);
        }
        pending.extend(&node.children);
    }
    found
}
