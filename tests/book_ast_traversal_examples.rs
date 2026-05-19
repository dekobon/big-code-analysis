//! Lifts the runnable examples from
//! `big-code-analysis-book/src/library/ast-traversal.md` into a
//! cargo-tested module, so doc rot is caught by `cargo test` instead
//! of by readers trying to copy-paste broken snippets. If you change
//! the book, mirror the change here; if a refactor breaks an example
//! here, fix both.

#![cfg(feature = "rust")]
#![allow(clippy::float_cmp)]

use std::collections::HashMap;

use big_code_analysis::{Ast, AstNode, LANG, MetricsOptions, Source, tree_sitter};

/// Recursive `kind` finder used by the [`AstCallback`] test below.
fn ast_node_contains_kind(node: &AstNode, kind: &str) -> bool {
    node.r#type == kind
        || node
            .children
            .iter()
            .any(|c| ast_node_contains_kind(c, kind))
}

/// Visit every node in `tree` in pre-order, root first.
fn walk_preorder<F: FnMut(tree_sitter::Node<'_>)>(tree: &tree_sitter::Tree, mut visit: F) {
    let mut cursor = tree.walk();
    'walk: loop {
        visit(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

#[test]
fn count_nodes_by_kind() {
    let ast = Ast::parse(Source::new(
        LANG::Rust,
        b"fn a() { if true { 1 } else { 2 } } fn b() { for _ in 0..10 {} }",
    ))
    .expect("rust feature enabled");

    let mut counts: HashMap<&str, usize> = HashMap::new();
    walk_preorder(ast.as_tree_sitter(), |node| {
        *counts.entry(node.kind()).or_default() += 1;
    });

    assert_eq!(counts.get("if_expression").copied().unwrap_or(0), 1);
    assert_eq!(counts.get("for_expression").copied().unwrap_or(0), 1);
}

#[test]
fn find_unsafe_blocks() {
    let ast = Ast::parse(Source::new(
        LANG::Rust,
        b"fn safe() {} fn risky() { unsafe { } }",
    ))
    .expect("rust feature enabled");

    let source = ast.source();
    // Captured slices borrow from `source` — no per-hit allocation, and
    // the example demonstrates the idiomatic shape for AST scans.
    let mut hits: Vec<((usize, usize), &str)> = Vec::new();
    walk_preorder(ast.as_tree_sitter(), |node| {
        if node.kind() == "unsafe_block" {
            let span = (node.start_position().row, node.end_position().row);
            let text = node.utf8_text(source).expect("source is valid utf-8");
            hits.push((span, text));
        }
    });

    assert_eq!(hits.len(), 1);
    // Anchor on what was captured, not just how many — a regression in
    // `Node::utf8_text` (wrong byte range, wrong source buffer) would
    // otherwise sail past `hits.len() == 1`, and an unused `span` field
    // would let a future refactor break position reporting silently.
    let ((start_row, end_row), text) = hits[0];
    assert!(
        text.starts_with("unsafe"),
        "captured text {text:?} should start with `unsafe`",
    );
    assert_eq!((start_row, end_row), (0, 0));
}

#[test]
fn detect_parse_error_on_root() {
    let ast = Ast::parse(Source::new(LANG::Rust, b"fn broken(")).expect("rust feature enabled");
    assert!(ast.as_tree_sitter().root_node().has_error());
}

#[test]
fn enumerate_parse_error_lines() {
    let ast = Ast::parse(Source::new(LANG::Rust, b"fn broken(")).expect("rust feature enabled");

    let mut error_lines = Vec::new();
    walk_preorder(ast.as_tree_sitter(), |node| {
        if node.is_error() || node.is_missing() {
            error_lines.push(node.start_position().row);
        }
    });

    // The broken input is a single line, so the recovered error /
    // missing nodes must reference row 0. A mutation that flagged
    // every node as an error would still produce a non-empty vec but
    // would also report rows that are out of range here.
    assert!(
        error_lines.contains(&0),
        "expected an error on row 0, got {error_lines:?}",
    );
}

#[test]
fn metrics_plus_symbol_table_one_parse() {
    let ast = Ast::parse(Source::new(
        LANG::Rust,
        b"fn outer() { fn inner() {} } fn alone() {}",
    ))
    .expect("rust feature enabled");

    let space = ast
        .metrics(MetricsOptions::default())
        .expect("walker succeeds");

    let source = ast.source();
    let mut functions: Vec<&str> = Vec::new();
    walk_preorder(ast.as_tree_sitter(), |node| {
        if node.kind() == "function_item"
            && let Some(name_node) = node.child_by_field_name("name")
        {
            let name = name_node.utf8_text(source).expect("source is valid utf-8");
            functions.push(name);
        }
    });

    assert_eq!(space.metrics.nom.functions_sum(), 3.0);
    assert_eq!(functions, ["outer", "inner", "alone"]);
}

#[test]
fn ast_callback_produces_serializable_tree() {
    use std::path::PathBuf;

    use big_code_analysis::{AstCallback, AstCfg, AstPayload, action};

    let payload = AstPayload {
        id: "snippet".to_owned(),
        file_name: "snippet.rs".to_owned(),
        code: "fn f() {}".to_owned(),
        comment: false,
        span: true,
    };
    let cfg = AstCfg {
        id: payload.id.clone(),
        comment: payload.comment,
        span: payload.span,
    };
    let response = action::<AstCallback>(
        &LANG::Rust,
        payload.code.into_bytes(),
        &PathBuf::from(&payload.file_name),
        None,
        cfg,
    )
    .expect("rust feature enabled");

    // Walk the materialized tree structurally instead of substring-matching
    // the JSON — a substring check would false-pass on any node whose
    // `TextValue` happened to contain `function_item` (e.g. an identifier
    // by that name in a future fixture).
    let root = response.root.as_ref().expect("rust source parses");
    assert!(
        ast_node_contains_kind(root, "function_item"),
        "ast response should include a function_item node",
    );

    // Serialization shape sanity-check stays — the AST callback's stable
    // contract is the JSON layout consumed by the REST `/ast` endpoint.
    let json = serde_json::to_string(&response).expect("AstResponse serializes");
    assert!(json.contains("\"Type\":\"function_item\""));
}
