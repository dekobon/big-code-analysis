//! Lifts the runnable examples from the book's *Using as a Library*
//! chapter (`big-code-analysis-book/src/library/in-memory.md`,
//! `walking-funcspace.md`, and `reuse-tree.md`) into a cargo-tested
//! module, so doc rot is caught by `cargo test` instead of by readers
//! trying to copy-paste broken snippets. If you change the book,
//! mirror the change here; if a refactor breaks an example here, fix
//! both. (`ast-traversal.md` is pinned separately by
//! `book_ast_traversal_examples.rs`.)

use big_code_analysis::{FuncSpace, LANG, MetricsOptions, Source, SpaceKind, analyze};

/// `in-memory.md` — "Reading from a buffer".
#[cfg(feature = "python")]
fn analyze_buffer(source: &[u8]) -> Option<u64> {
    let space = analyze(
        Source::new(LANG::Python, source).with_name(Some("<stdin>".to_owned())),
        MetricsOptions::default(),
    )
    .ok()?;

    Some(space.metrics.cognitive.cognitive_sum())
}

#[cfg(feature = "python")]
#[test]
fn in_memory_analyze_buffer() {
    let source = b"def f(x):\n    if x:\n        return 1\n    return 0\n";
    assert_eq!(analyze_buffer(source), Some(1));
}

/// `walking-funcspace.md` — "Recursive walk".
fn hotspots(space: &FuncSpace, threshold: u64, out: &mut Vec<String>) {
    if space.kind == SpaceKind::Function
        && space.metrics.cognitive.cognitive_sum() > threshold
        && let Some(name) = &space.name
    {
        out.push(format!(
            "{name} (lines {}\u{2013}{})",
            space.start_line, space.end_line,
        ));
    }
    for child in &space.spaces {
        hotspots(child, threshold, out);
    }
}

#[test]
fn walking_funcspace_hotspots() {
    let source = b"\
fn easy() { let _ = 1; }
fn hard(x: i32) -> i32 {
    if x > 0 { if x > 10 { 1 } else { 2 } } else { 3 }
}
";
    let space = analyze(
        Source::new(LANG::Rust, source).with_name(Some("snippet.rs".to_owned())),
        MetricsOptions::default(),
    )
    .expect("parses");

    let mut hits = Vec::new();
    hotspots(&space, 2, &mut hits);
    assert_eq!(hits, ["hard (lines 2\u{2013}4)"]);
}

/// `reuse-tree.md` — "Working example".
#[test]
fn reuse_tree_working_example() {
    use big_code_analysis::{Ast, tree_sitter};

    let source_code = "fn main() { if true { 1 } else { 2 }; }";
    let source = source_code.as_bytes().to_vec();

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Rust
                .tree_sitter_language()
                .expect("rust feature enabled"),
        )
        .expect("rust grammar pinned to a compatible version");
    let tree = parser
        .parse(&source, None)
        .expect("parser has a language set");

    let from_tree =
        Ast::from_tree_sitter(LANG::Rust, tree, source.clone(), Some("foo.rs".to_owned()))
            .expect("rust feature enabled")
            .metrics(MetricsOptions::default())
            .expect("non-empty input");

    let from_bytes = analyze(
        Source::new(LANG::Rust, &source).with_name(Some("foo.rs".to_owned())),
        MetricsOptions::default(),
    )
    .expect("non-empty input");

    assert_eq!(
        from_tree.metrics.cyclomatic.cyclomatic_sum(),
        from_bytes.metrics.cyclomatic.cyclomatic_sum(),
    );
    // expected: unit base (+1), fn main (+1), the if/else (+1) = 3.
    assert_eq!(from_tree.metrics.cyclomatic.cyclomatic_sum(), 3);
}
