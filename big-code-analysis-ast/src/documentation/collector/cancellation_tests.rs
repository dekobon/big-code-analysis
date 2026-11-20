use crate::documentation::budget::{WorkPhase, cancel_in_phase};
use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(any(
    feature = "lua",
    feature = "javascript",
    feature = "ruby",
    feature = "cpp"
))]
fn classification_abort(language: LANG, source: Vec<u8>) {
    let parser = AnyParser::parse(language, source, None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations().renders;
    let result = cancel_in_phase(WorkPhase::Classification, || {
        parser.documentation(&context, &options)
    });
    assert!(matches!(result, Err(CollectionError::Cancelled)));
    assert_eq!(
        observations().renders - before,
        0,
        "abort must happen inside recognition before rendering"
    );
    let next = parser.documentation(&context, &options).unwrap();
    assert_eq!(next.documents().len(), 1);
    assert!(
        next.complete_text(next.documents().next().unwrap())
            .unwrap()
            .unwrap()
            .text()
            .len()
            > 20_000
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_single_line_cancels_inside_content_classifier() {
    classification_abort(
        LANG::Lua,
        format!("module(\"m\")\n---{}\nfunction f() end", "a".repeat(40_000)).into_bytes(),
    );
}

#[cfg(feature = "javascript")]
#[test]
fn jsdoc_single_line_cancels_inside_virtual_classifier() {
    classification_abort(
        LANG::Javascript,
        format!("/**{}*/\nfunction f() {{}}", "a".repeat(40_000)).into_bytes(),
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_single_line_cancels_inside_magic_classifier() {
    classification_abort(
        LANG::Ruby,
        format!("# {}\ndef f\nend", "a".repeat(40_000)).into_bytes(),
    );
}

#[cfg(feature = "cpp")]
#[test]
fn doxygen_single_line_cancels_inside_structural_classifier() {
    classification_abort(
        LANG::Cpp,
        format!("/**{}*/\nvoid f() {{}}", "a".repeat(40_000)).into_bytes(),
    );
}

#[cfg(feature = "rust")]
#[test]
fn legacy_recognition_never_polls_collector_budget() {
    use crate::{Checker, Search};
    let source = format!("///{}\nfn f() {{}}", "a".repeat(40_000));
    let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
    let before = observations();
    let mut markers = 0;
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if crate::RustCode::is_comment(node) {
            streaming::marker_comment::<crate::RustCode>(node, parser.code(), ancestors, |_| {
                markers += 1;
            });
        }
    });
    assert_eq!(markers, 2);
    let after = observations();
    assert_eq!(after.polls - before.polls, 0);
    assert_eq!(after.renders - before.renders, 0);
    assert_eq!(after.visits - before.visits, 0);
}

#[cfg(feature = "rust")]
#[test]
fn cancellation_reaches_wide_batch_copy_text_and_ordering() {
    let source = format!(
        "///{}\nfn f() {{}}\n{}",
        "a".repeat(40_000),
        "fn g() {}\n".repeat(1500)
    );
    let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
    for phase in [WorkPhase::Batch, WorkPhase::Text, WorkPhase::Ordering] {
        let result = cancel_in_phase(phase, || {
            parser.documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
        });
        assert!(
            matches!(result, Err(CollectionError::Cancelled)),
            "{phase:?}"
        );
    }
    assert_eq!(
        parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default()
            )
            .unwrap()
            .documents()
            .len(),
        1
    );
}

#[cfg(feature = "cpp")]
#[test]
fn giant_static_name_cancels_during_index_key_processing() {
    let source = format!("/** @fn missing */\nvoid {}();", "a".repeat(40_000));
    let parser = AnyParser::parse(LANG::Cpp, source.into_bytes(), None, None).unwrap();
    let result = cancel_in_phase(WorkPhase::Names, || {
        parser.documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
    });
    assert!(matches!(result, Err(CollectionError::Cancelled)));
    assert_eq!(
        parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default()
            )
            .unwrap()
            .documents()
            .len(),
        1
    );
}

#[cfg(feature = "rust")]
#[test]
fn many_included_ranges_cancel_inside_normalization() {
    let bytes = b" ".repeat(5000);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    let ranges: Vec<_> = (0..2500)
        .map(|at| tree_sitter::Range {
            start_byte: at * 2,
            end_byte: at * 2 + 1,
            start_point: tree_sitter::Point::new(0, at * 2),
            end_point: tree_sitter::Point::new(0, at * 2 + 1),
        })
        .collect();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    // The first two checkpoints bracket the dependency's range-array copy.
    // The third must be reached by the normalization loop itself.
    let result = crate::documentation::budget::cancel_after(3, || {
        SourceContext::with_budget(
            &tree,
            &bytes,
            &crate::documentation::budget::CollectionBudget::default(),
        )
    });
    assert!(matches!(result, Err(SourceError::Cancelled)));
    assert_eq!(
        collect(
            LANG::Rust,
            &tree,
            &bytes,
            &DocumentationContext::default(),
            &DocumentationOptions::default()
        )
        .unwrap()
        .documents()
        .len(),
        0
    );
}

#[cfg(feature = "rust")]
#[test]
fn range_intersection_cancels_after_prepared_context_exists() {
    use crate::documentation::budget::CollectionBudget;
    let bytes = b" ".repeat(5000);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    let ranges: Vec<_> = (0..2500)
        .map(|at| tree_sitter::Range {
            start_byte: at * 2,
            end_byte: at * 2 + 1,
            start_point: tree_sitter::Point::new(0, at * 2),
            end_point: tree_sitter::Point::new(0, at * 2 + 1),
        })
        .collect();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let context = SourceContext::new(&tree, &bytes);
    let result = cancel_in_phase(WorkPhase::Ranges, || {
        context.pieces_with_budget(0..bytes.len(), &CollectionBudget::default())
    });
    assert!(matches!(result, Err(SourceError::Cancelled)));
    assert_eq!(context.pieces(0..bytes.len()).unwrap().pieces().len(), 2500);
}
