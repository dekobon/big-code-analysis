use super::super::{SourceContext, SourceError};
use crate::cancel::ParseCancel;
use crate::parser::ParseCompletion;
use crate::{AnyParser, LANG, ParserTrait, RustParser};

#[cfg(feature = "rust")]
#[test]
fn actual_abort_survives_scope_exit_and_next_parse_completes() {
    let source = "fn f() { let x = 1; }\n".repeat(200).into_bytes();
    let cancel = ParseCancel::new();
    cancel.cancel();
    let parsed = cancel.run(|| AnyParser::parse(LANG::Rust, source, None, None).unwrap());
    assert!(!crate::cancel::requested());
    assert_eq!(parsed.parse_completion(), ParseCompletion::Cancelled);
    assert_eq!(parsed.root_node().start_byte(), 0);
    assert_eq!(parsed.root_node().end_byte(), 0);
    assert_eq!(parsed.root_node().child_count(), 0);

    let next = AnyParser::parse(LANG::Rust, b"fn g() {}".to_vec(), None, None).unwrap();
    assert_eq!(next.parse_completion(), ParseCompletion::Complete);
    assert_eq!(next.root_node().end_byte(), 9);
    assert_eq!(
        next.root_node().children().next().unwrap().kind(),
        "function_item"
    );
}

#[cfg(feature = "rust")]
#[test]
fn empty_and_error_parses_are_complete_and_later_flags_do_not_change_them() {
    let empty = RustParser::new(vec![], std::path::Path::new(""), None);
    let malformed = RustParser::new(b"fn {".to_vec(), std::path::Path::new(""), None);
    assert_eq!(empty.parse_completion(), ParseCompletion::Complete);
    assert_eq!(empty.root().end_byte(), 0);
    assert!(malformed.root().has_error());
    assert_eq!(malformed.parse_completion(), ParseCompletion::Complete);
    let cancel = ParseCancel::new();
    cancel.cancel();
    cancel.run(|| assert_eq!(empty.parse_completion(), ParseCompletion::Complete));
    // This input completes before the runtime's first progress callback.
    let under_flag = cancel.run(|| RustParser::new(vec![], std::path::Path::new(""), None));
    assert_eq!(under_flag.parse_completion(), ParseCompletion::Complete);
}

#[cfg(feature = "rust")]
#[test]
fn adopted_restricted_parse_retains_caller_provenance() {
    let source = b"///x\n\nfn f() {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    parser
        .set_included_ranges(&[
            tree_sitter::Range {
                start_byte: 0,
                end_byte: 4,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(0, 4),
            },
            tree_sitter::Range {
                start_byte: 6,
                end_byte: 15,
                start_point: tree_sitter::Point::new(2, 0),
                end_point: tree_sitter::Point::new(2, 9),
            },
        ])
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let adopted = AnyParser::from_tree(LANG::Rust, tree, source.to_vec()).unwrap();
    assert_eq!(adopted.parse_completion(), ParseCompletion::Adopted);
    assert_eq!(adopted.ts_tree().included_ranges().len(), 2);
    assert_eq!(adopted.root_node().end_byte(), 15);
    let empty = parser.parse(b"", None).unwrap();
    let empty = RustParser::from_tree(empty, vec![]);
    assert_eq!(empty.parse_completion(), ParseCompletion::Adopted);
}

#[cfg(feature = "rust")]
#[test]
fn canonical_evidence_rejects_clones_shifted_roots_and_edited_values() {
    let parser = AnyParser::parse(LANG::Rust, b"fn f() {}".to_vec(), None, None).unwrap();
    let second = AnyParser::parse(LANG::Rust, b"fn f() {}".to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let root = parser.ts_tree().root_node();
    let canonical = context.node(root).unwrap();
    assert_eq!(canonical.validate(root), Ok(()));
    assert_eq!(
        canonical.validate(second.ts_tree().root_node()),
        Err(SourceError::ForeignTree)
    );
    let cloned = parser.ts_tree().clone();
    assert_eq!(
        canonical.validate(cloned.root_node()),
        Err(SourceError::ForeignTree)
    );
    assert!(matches!(
        context.node(cloned.root_node()),
        Err(SourceError::ForeignTree)
    ));
    let shifted = parser
        .ts_tree()
        .root_node_with_offset(5, tree_sitter::Point::new(1, 2));
    assert_eq!(
        canonical.validate(shifted),
        Err(SourceError::NoncanonicalNode)
    );
    let mut edited = root;
    edited.edit(&tree_sitter::InputEdit {
        start_byte: 0,
        old_end_byte: 0,
        new_end_byte: 1,
        start_position: tree_sitter::Point::new(0, 0),
        old_end_position: tree_sitter::Point::new(0, 0),
        new_end_position: tree_sitter::Point::new(0, 1),
    });
    assert_eq!(
        canonical.validate(edited),
        Err(SourceError::NoncanonicalNode)
    );
}

#[test]
fn retained_parser_and_facts_are_send_sync() {
    fn require<T: Send + Sync>() {}
    require::<AnyParser>();
    require::<super::super::Facts<'static>>();
    require::<super::super::FactRef<'static, super::super::Document<'static>>>();
    require::<SourceContext<'static>>();
}
