#![allow(clippy::reversed_empty_ranges)]

use super::*;
use crate::{AnyParser, LANG};

mod model_cases;
mod ownership;
mod provenance;
mod ranges;
mod streaming;

#[cfg(feature = "rust")]
#[test]
fn spelling_excludes_crlf_while_node_retains_it_and_equal_text_is_not_identity() {
    let source = b"///same\r\n///same\r\nfn f() {}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let mut facts = Facts::default();
    for (node, range) in parser.root_node().children().take(2).zip([0..7, 9..16]) {
        facts.fragments.push(Fragment {
            source: context.pieces(range).unwrap(),
            node: Some(context.node(node.as_tree_sitter()).unwrap()),
            form: FragmentForm::Comment,
        });
    }
    let fragments: Vec<_> = facts.fragments().collect();
    assert_eq!(fragments.len(), 2);
    assert_ne!(fragments[0], fragments[1]);
    assert_eq!(fragments[0], facts.fragments().next().unwrap());
    assert_eq!(fragments[0].get().source().pieces()[0].range(), 0..7);
    assert_eq!(fragments[1].get().source().pieces()[0].range(), 9..16);
    assert_eq!(fragments[0].get().source().pieces()[0].bytes(), b"///same");
    assert_eq!(fragments[1].get().source().pieces()[0].bytes(), b"///same");
    assert_eq!(fragments[0].get().node().unwrap().node().byte_range(), 0..9);
    assert_eq!(
        fragments[1].get().node().unwrap().node().byte_range(),
        9..18
    );
}

#[cfg(feature = "rust")]
#[test]
fn included_pieces_keep_raw_invalid_bytes_and_never_return_the_gap() {
    let source = b"///a\xff\n///b\nfn f() {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    parser
        .set_included_ranges(&[
            tree_sitter::Range {
                start_byte: 0,
                end_byte: 5,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(0, 5),
            },
            tree_sitter::Range {
                start_byte: 6,
                end_byte: 20,
                start_point: tree_sitter::Point::new(1, 0),
                end_point: tree_sitter::Point::new(2, 9),
            },
        ])
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let context = SourceContext::new(&tree, source);
    let pieces = context.pieces(0..10).unwrap();
    assert!(pieces.has_excluded_gap());
    assert_eq!(pieces.pieces().len(), 2);
    assert_eq!(pieces.pieces()[0].range(), 0..5);
    assert_eq!(pieces.pieces()[0].bytes(), b"///a\xff");
    assert_eq!(pieces.pieces()[1].range(), 6..10);
    assert_eq!(pieces.pieces()[1].bytes(), b"///b");
    let mut facts = Facts::default();
    facts.fragments.push(Fragment {
        source: pieces,
        node: None,
        form: FragmentForm::Comment,
    });
    facts.documents.push(Document {
        fragments: vec![FragmentId(0)],
        composition: Composition::Single,
        role: DocumentRole::LeadingComment,
        content: Content::Unavailable(UnavailableReason::ExcludedGap),
    });
    assert_eq!(
        facts.document_order(facts.documents().next().unwrap()),
        Ok((0, 10, 4, 0))
    );
    assert!(!context.pieces(6..10).unwrap().has_excluded_gap());
    assert_eq!(context.pieces(5..6), Err(SourceError::Excluded));
    for range in [0..0, 8..7, 0..21] {
        assert_eq!(context.pieces(range), Err(SourceError::InvalidRange));
    }
}

#[cfg(feature = "perl")]
#[test]
fn disjoint_pod_occurrences_can_share_one_opaque_node() {
    let source = b"__END__\n=head1 A\n\na\n\n=cut\n\n=head1 B\n\nb\n\n=cut\n";
    let parser = AnyParser::parse(LANG::Perl, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let opaque = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "data_not_for_compiler")
        .unwrap();
    let mut facts = Facts::default();
    for range in [8..25, 27..44] {
        facts.fragments.push(Fragment {
            source: context.pieces(range).unwrap(),
            node: Some(context.node(opaque.as_tree_sitter()).unwrap()),
            form: FragmentForm::Pod,
        });
    }
    let fragments: Vec<_> = facts.fragments().collect();
    assert_eq!(fragments.len(), 2);
    assert_ne!(fragments[0], fragments[1]);
    assert_eq!(
        fragments[0].get().source().pieces()[0].bytes(),
        b"=head1 A\n\na\n\n=cut"
    );
    assert_eq!(
        fragments[1].get().source().pieces()[0].bytes(),
        b"=head1 B\n\nb\n\n=cut"
    );
    assert_eq!(
        fragments[0].get().node().unwrap().node(),
        fragments[1].get().node().unwrap().node()
    );
}

#[test]
fn text_coordinates_distinguish_scalars_empty_queries_and_origin_kinds() {
    assert_eq!(TextRange::new("é!", 0..2).unwrap().range(), 0..2);
    assert_eq!(
        TextRange::new("é!", 0..1),
        Err(TextRangeError::ScalarBoundary)
    );
    assert_eq!(
        TextRange::new("é!", 1..2),
        Err(TextRangeError::ScalarBoundary)
    );
    assert_eq!(TextRange::new("é!", 3..3).unwrap().range(), 3..3);
    assert_eq!(TextRange::new("", 0..0).unwrap().range(), 0..0);
    for range in [3..2, 4..4, 0..4] {
        assert_eq!(TextRange::new("é!", range), Err(TextRangeError::Bounds));
    }
}

#[cfg(feature = "python")]
#[test]
fn primary_additional_documents_differ_from_literal_concatenation() {
    let source = b"def f():\n    \"\"\n    \"extra\"\n    (\"@au\" # ordinary\n     \"dit\")\n";
    let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let mut facts = Facts::default();
    for range in [13..15, 20..27, 33..38, 55..60] {
        facts.fragments.push(Fragment {
            source: context.pieces(range).unwrap(),
            node: None,
            form: FragmentForm::Literal,
        });
    }
    facts.documents = vec![
        Document {
            fragments: vec![FragmentId(0)],
            composition: Composition::Single,
            role: DocumentRole::Primary,
            content: Content::Complete {
                text: String::new(),
                mappings: vec![],
            },
        },
        Document {
            fragments: vec![FragmentId(1)],
            composition: Composition::Single,
            role: DocumentRole::Additional,
            content: Content::Complete {
                text: "extra".into(),
                mappings: vec![],
            },
        },
        Document {
            fragments: vec![FragmentId(2), FragmentId(3)],
            composition: Composition::LiteralConcatenation,
            role: DocumentRole::Additional,
            content: Content::Complete {
                text: "@audit".into(),
                mappings: vec![],
            },
        },
    ];
    let docs: Vec<_> = facts.documents().collect();
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[0].get().fragment_count(), 1);
    assert_eq!(docs[2].get().fragment_count(), 2);
    assert_eq!(docs[0].get().role(), DocumentRole::Primary);
    assert_eq!(docs[1].get().role(), DocumentRole::Additional);
    assert_eq!(
        docs[2].get().composition(),
        Composition::LiteralConcatenation
    );
    assert_eq!(
        facts
            .document_fragment(docs[0], 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"\"\""
    );
    assert_eq!(
        facts
            .document_fragment(docs[1], 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"\"extra\""
    );
    assert_eq!(
        facts
            .document_fragment(docs[2], 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"\"@au\""
    );
    assert_eq!(
        facts
            .document_fragment(docs[2], 1)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"\"dit\""
    );
    assert_eq!(facts.document_order(docs[0]), Ok((13, 15, 0, 0)));
    assert_eq!(facts.document_order(docs[1]), Ok((20, 27, 1, 1)));
    assert_eq!(facts.document_order(docs[2]), Ok((33, 38, 1, 2)));
}

#[cfg(feature = "elixir")]
#[test]
fn absent_disabled_metadata_empty_and_unavailable_have_physical_evidence() {
    let source = b"@doc false\n@doc nil\n@doc since: \"1\"\n@doc \"\"\n@doc \"a#{x}b\"";
    let parser = AnyParser::parse(LANG::Elixir, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let mut facts = Facts::default();
    let states = [
        Content::Disabled,
        Content::ExplicitAbsence,
        Content::MetadataOnly,
        Content::Complete {
            text: String::new(),
            mappings: vec![],
        },
        Content::Unavailable(UnavailableReason::Dynamic),
    ];
    for (ordinal, (range, content)) in [0..10, 11..19, 20..35, 36..43, 44..57]
        .into_iter()
        .zip(states)
        .enumerate()
    {
        facts.fragments.push(Fragment {
            source: context.pieces(range).unwrap(),
            node: None,
            form: FragmentForm::Attribute,
        });
        facts.documents.push(Document {
            fragments: vec![FragmentId(ordinal)],
            composition: Composition::Single,
            role: DocumentRole::Primary,
            content,
        });
    }
    let docs: Vec<_> = facts.documents().collect();
    assert_eq!(docs.len(), 5);
    assert!(matches!(docs[0].get().content(), Content::Disabled));
    assert!(matches!(docs[1].get().content(), Content::ExplicitAbsence));
    assert!(matches!(docs[2].get().content(), Content::MetadataOnly));
    assert!(
        matches!(docs[3].get().content(), Content::Complete { text, mappings } if text.is_empty() && mappings.is_empty())
    );
    assert!(matches!(
        docs[4].get().content(),
        Content::Unavailable(UnavailableReason::Dynamic)
    ));
    assert_eq!(
        facts
            .document_fragment(docs[4], 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .bytes(),
        b"@doc \"a#{x}b\""
    );
    assert_eq!(Facts::default().documents().len(), 0);
}
