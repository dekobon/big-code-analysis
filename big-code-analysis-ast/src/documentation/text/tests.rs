#![allow(clippy::similar_names)]

use super::*;
use crate::documentation::budget::{CollectionBudget, NoBudget};
use crate::documentation::{CompleteText, OriginView, TextRangeError};
use crate::{AnyParser, LANG};

fn complete<'i, 's>(content: &'i Content<'s>) -> CompleteText<'i, 's> {
    match content {
        Content::Complete { text, mappings } => CompleteText { text, mappings },
        other => panic!("expected complete local text, got {other:?}"),
    }
}

#[cfg(feature = "rust")]
#[test]
fn copied_decoded_expanded_and_synthetic_origins_keep_exact_responsibility() {
    let source = b"abcX\\x40Y\t";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let mut builder = TextBuilder::default();
    builder.source(&context, 0..3, &NoBudget).unwrap();
    builder
        .transformed(
            "@",
            Origin::Decoded(context.pieces(4..8).unwrap()),
            &NoBudget,
        )
        .unwrap();
    builder
        .transformed(
            "   ",
            Origin::Expanded(context.pieces(9..10).unwrap()),
            &NoBudget,
        )
        .unwrap();
    builder
        .transformed("\n", Origin::Synthetic, &NoBudget)
        .unwrap();
    let content = builder.finish();
    let view = complete(&content);
    assert_eq!(view.text(), "abc@   \n");
    let portions: Vec<_> = view.origins(1..8).unwrap().collect();
    assert_eq!(portions.len(), 4);
    assert_eq!(portions[0].output, 1..3);
    let OriginView::Copied(piece) = &portions[0].origin else {
        panic!("copied origin")
    };
    assert_eq!(piece.range(), 1..3);
    assert_eq!(piece.bytes(), b"bc");
    let OriginView::Decoded(pieces) = &portions[1].origin else {
        panic!("decoded origin")
    };
    assert_eq!(pieces[0].range(), 4..8);
    assert_eq!(pieces[0].bytes(), b"\\x40");
    let OriginView::Expanded(pieces) = &view.origins(5..6).unwrap().next().unwrap().origin else {
        panic!("expanded origin")
    };
    assert_eq!(pieces[0].range(), 9..10);
    assert_eq!(pieces[0].bytes(), b"\t");
    assert_eq!(portions[3].origin, OriginView::Synthetic);
    assert_eq!(portions[3].output, 7..8);
}

#[cfg(feature = "rust")]
#[test]
fn transformed_queries_return_every_disjoint_responsible_piece() {
    let source = b"abcdefgh";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let pieces = || {
        SourcePieces::from_included_pieces(vec![
            context.pieces(0..2).unwrap().pieces()[0].clone(),
            context.pieces(5..8).unwrap().pieces()[0].clone(),
        ])
    };
    let mut builder = TextBuilder::default();
    builder
        .transformed("é", Origin::Decoded(pieces()), &NoBudget)
        .unwrap();
    builder
        .transformed("   ", Origin::Expanded(pieces()), &NoBudget)
        .unwrap();
    let content = builder.finish();
    let view = complete(&content);
    for range in [0..2, 3..4] {
        let part = view.origins(range.clone()).unwrap().next().unwrap();
        assert_eq!(part.output, range);
        let (OriginView::Decoded(pieces) | OriginView::Expanded(pieces)) = part.origin else {
            panic!("transformed")
        };
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].range(), 0..2);
        assert_eq!(pieces[1].range(), 5..8);
        assert_eq!(pieces[0].bytes(), b"ab");
        assert_eq!(pieces[1].bytes(), b"fgh");
    }
}

#[cfg(feature = "rust")]
#[test]
fn utf8_carry_and_invalid_source_are_not_lossily_decoded() {
    let source = format!("{}é🦀", "a".repeat(BYTE_INTERVAL - 1));
    let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
    let mut builder = TextBuilder::default();
    builder
        .source(&context, 0..source.len(), &NoBudget)
        .unwrap();
    let content = builder.finish();
    let view = complete(&content);
    assert_eq!(view.text(), source);
    assert_eq!(view.mappings().len(), 1);
    assert_eq!(
        view.origins(BYTE_INTERVAL..BYTE_INTERVAL + 1).unwrap_err(),
        TextRangeError::ScalarBoundary
    );
    let mut builder = TextBuilder::default();
    assert!(matches!(
        builder.append_utf8(b"a\xffb", &NoBudget),
        Err(TextError::Unavailable(UnavailableReason::InvalidEncoding))
    ));
}

#[test]
fn late_and_empty_queries_do_not_scan_or_copy_preceding_runs() {
    let mut builder = TextBuilder::default();
    for _ in 0..4096 {
        builder
            .transformed("a", Origin::Synthetic, &NoBudget)
            .unwrap();
    }
    let content = builder.finish();
    let view = complete(&content);
    let before = crate::documentation::content::map_searches::observed();
    let portions: Vec<_> = view.origins(4095..4096).unwrap().collect();
    assert_eq!(portions.len(), 1);
    assert_eq!(portions[0].output, 4095..4096);
    // expected: Rust slice::partition_point performs log2(4096) + 1 comparisons.
    assert_eq!(
        crate::documentation::content::map_searches::observed() - before,
        13
    );
    let before = crate::documentation::content::map_searches::observed();
    assert_eq!(view.origins(4096..4096).unwrap().count(), 0);
    assert_eq!(
        crate::documentation::content::map_searches::observed() - before,
        0
    );
    assert_eq!(
        view.origins(4097..4097).unwrap_err(),
        TextRangeError::Bounds
    );
}

#[test]
fn active_text_cancellation_is_an_error_and_the_next_operation_succeeds() {
    let cancel = crate::cancel::ParseCancel::new();
    cancel.cancel();
    let mut builder = TextBuilder::default();
    let result = cancel.run(|| builder.append_utf8(b"audit", &CollectionBudget::default()));
    assert!(matches!(
        result,
        Err(TextError::Source(SourceError::Cancelled))
    ));
    builder
        .append_utf8(b"audit", &CollectionBudget::default())
        .unwrap();
    assert_eq!(builder.text, "audit");
}

#[cfg(feature = "rust")]
#[test]
fn comment_cleanup_preserves_markup_hard_breaks_and_phase_two_splices() {
    use crate::documentation::{FragmentForm, Profile};
    for (profile, source, expected) in [
        (Profile::Jsdoc, "/**\n * a  \n *   b\n */", "a  \n  b"),
        (Profile::Javadoc, "/**/", ""),
        (Profile::Javadoc, "////", "/"),
        (Profile::Rust, "/*!*/", ""),
        (Profile::Rust, "/**\n * a  \n *   b\n */", "a  \n  b"),
        (Profile::GoDoc, "/* a\n * b */", "a\n * b"),
        (Profile::Doxygen, "///a\\\nb", "ab"),
        (Profile::Doxygen, "/**a\\\r\nb*/", "ab"),
        (Profile::Doxygen, "///a\\\\\nb", "a\\b"),
        (Profile::CsharpXml, "///a\\\nb", "a\\\nb"),
        (Profile::Ldoc, "--[==[-audit]==]", "audit"),
        (Profile::Yard, "=begin\naudit\n=end", "audit"),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
        let fragment = crate::documentation::Fragment {
            source: context.pieces(0..source.len()).unwrap(),
            node: None,
            form: FragmentForm::Comment,
        };
        let content = comments::render(profile, &context, &[&fragment], &NoBudget).unwrap();
        let view = complete(&content);
        assert_eq!(view.text(), expected, "{profile:?}: {source:?}");
        let portions: Vec<_> = view.origins(0..view.text().len()).unwrap().collect();
        let mut offset = 0;
        for portion in portions {
            assert_eq!(portion.output.start, offset);
            offset = portion.output.end;
            if let OriginView::Copied(piece) = portion.origin {
                assert_eq!(piece.bytes(), &view.text().as_bytes()[portion.output]);
                assert_eq!(piece.bytes(), &source.as_bytes()[piece.range()]);
            }
        }
        assert_eq!(offset, expected.len());
    }
}

#[cfg(feature = "rust")]
#[test]
fn copied_utf8_scalar_can_cross_adjacent_included_pieces() {
    let source = "é".as_bytes();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    parser
        .set_included_ranges(&[
            tree_sitter::Range {
                start_byte: 0,
                end_byte: 1,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(0, 1),
            },
            tree_sitter::Range {
                start_byte: 1,
                end_byte: 2,
                start_point: tree_sitter::Point::new(0, 1),
                end_point: tree_sitter::Point::new(0, 2),
            },
        ])
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let context = SourceContext::new(&tree, source);
    let pieces = context.pieces(0..2).unwrap();
    assert_eq!(pieces.pieces().len(), 2);
    let mut builder = TextBuilder::default();
    builder.copied(&pieces, &NoBudget).unwrap();
    let content = builder.finish();
    let view = complete(&content);
    assert_eq!(view.text(), "é");
    let portions: Vec<_> = view.origins(0..2).unwrap().collect();
    assert_eq!(portions.len(), 2);
    for (portion, expected) in portions.iter().zip([0..1, 1..2]) {
        let OriginView::Copied(piece) = &portion.origin else {
            panic!("copied piece")
        };
        assert_eq!(piece.range(), expected);
        assert_eq!(portion.output, expected);
    }
    assert_eq!(
        view.origins(0..1).unwrap_err(),
        TextRangeError::ScalarBoundary
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_cleanup_keeps_unicode_blank_lines_and_removes_only_assembly_newline() {
    for (source, expected) in [
        ("/**\n * a  \n *   b\n */\nfn f() {}", "a  \n  b"),
        ("///   a\n///\u{a0}\n///     b\nfn f() {}", "a\n\u{a0}\n  b"),
        ("/**a /* nested */ b*/\nfn f() {}", "a /* nested */ b"),
        ("/**a\n\n*/\nfn f() {}", "a\n"),
        ("/**\n\n*/\nfn f() {}", ""),
        ("/**\n *a\n *b\n */\nfn f() {}", "*a\n*b"),
        ("/**\n * a\n ***/\nfn f() {}", " a\n*"),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &crate::documentation::DocumentationContext::default(),
                &crate::documentation::DocumentationOptions::default(),
            )
            .unwrap();
        let document = index.documents().next().unwrap();
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            expected,
            "{source}"
        );
    }
}
