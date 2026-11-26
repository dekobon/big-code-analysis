#![allow(
    clippy::single_range_in_vec_init,
    clippy::semicolon_if_nothing_returned,
    clippy::naive_bytecount
)]

use crate::documentation::*;
use crate::{AnyParser, LANG};
use std::ops::Range;

#[path = "rust_remediation_tests.rs"]
mod remediation;

#[cfg(feature = "rust")]
fn indexes(source: &[u8], check: impl Fn(&DocumentationIndex<'_>)) {
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    for index in [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Rust,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ] {
        check(&index);
    }
}

#[cfg(feature = "rust")]
fn text<'i, 's>(index: &'i DocumentationIndex<'s>) -> CompleteText<'i, 's> {
    assert_eq!(index.documents().len(), 1);
    index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap()
}

#[cfg(feature = "rust")]
type ExpectedMapping = (Range<usize>, &'static str, Vec<Range<usize>>);

#[cfg(feature = "rust")]
fn maps(complete: CompleteText<'_, '_>) -> Vec<ExpectedMapping> {
    complete
        .mappings()
        .iter()
        .map(|mapping| {
            let (kind, pieces) = match &mapping.origin {
                Origin::Copied(pieces) => ("copied", pieces.pieces()),
                Origin::Decoded(pieces) => ("decoded", pieces.pieces()),
                Origin::Expanded(pieces) => ("expanded", pieces.pieces()),
                Origin::Synthetic => ("synthetic", &[][..]),
            };
            (
                mapping.output.range(),
                kind,
                pieces.iter().map(SourcePiece::range).collect(),
            )
        })
        .collect()
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_complete_has_exact_owner_raw_fragments_and_prepared_queries() {
    let source = b"/// first\n#[doc = r\"second\"]\nfn f() {}";
    indexes(source, |index| {
        let complete = text(index);
        assert_eq!(complete.text(), "first\nsecond");
        assert_eq!(
            maps(complete),
            vec![
                (0..5, "copied", vec![4..9]),
                (5..6, "synthetic", vec![]),
                (6..12, "copied", vec![20..26])
            ]
        );
        assert_eq!(index.associations().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(document.get().composition(), Composition::AttributeSequence);
        assert_eq!(document.get().fragment_count(), 2);
        for (position, span, form) in [
            (0, 0..9, FragmentForm::Comment),
            (1, 10..28, FragmentForm::Attribute),
        ] {
            let fragment = index.facts().document_fragment(document, position).unwrap();
            assert_eq!(fragment.get().form(), form);
            assert_eq!(fragment.get().source().pieces()[0].range(), span);
            assert_eq!(fragment.get().source().pieces()[0].bytes(), &source[span]);
        }
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().publication, Publication::Unknown);
        assert_eq!(association.get().owner_count(), 1);
        assert_eq!(
            association.get().binding.unwrap().node().byte_range(),
            29..38
        );
        assert_eq!(association.get().lexical_scope.node().byte_range(), 0..38);
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("function owner")
        };
        assert_eq!(node.node().kind(), "function_item");
        assert_eq!(node.node().byte_range(), 29..38);
        let query = index.node_associations(node.node()).unwrap();
        assert_eq!(query.associations().collect::<Vec<_>>(), vec![association]);
        assert_eq!(
            index
                .owner_associations(owner)
                .unwrap()
                .associations()
                .collect::<Vec<_>>(),
            vec![association]
        );
        assert!(!index.assessments().any(|a| matches!(
            a.get().axis,
            AssessmentAxis::Occurrence | AssessmentAxis::Composition
        )));
        assert_eq!(complete.origins(12..12).unwrap().count(), 0);
        assert_eq!(
            complete.origins(13..13).unwrap_err(),
            TextRangeError::Bounds
        );
    });
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_cleanup_pins_mixed_margin_empty_joins_and_trailing_spaces() {
    for (source, expected) in [
        (
            "///     hello\n#[doc = \"another\"]\nfn f() {}",
            "    hello\nanother",
        ),
        (
            "#[doc = \"another\"]\n///     hello\nfn f() {}",
            "another\n    hello",
        ),
        ("#[doc = \"\"] fn f() {}", ""),
        ("#[doc = \"\"] #[doc = \"\"] fn f() {}", "\n"),
        ("#[doc = \"a\\n\"] fn f() {}", "a"),
        ("#[doc = \"a\\n\\n\"] fn f() {}", "a\n"),
        ("#[doc = \"a\\r\"] fn f() {}", "a\r"),
        ("#[doc = \"a\\r\nb\"] fn f() {}", "a\nb"),
        ("#[doc = \"a\\n\\t b\"] fn f() {}", "a\n\t b"),
        ("#[doc = r\"a\n\nb\"] fn f() {}", "a\n\nb"),
        ("#[doc = \" \"] #[doc = \"\"] fn f() {}", " \n"),
        ("#[doc = \"\\t a  \"] fn f() {}", "a  "),
        ("#[doc = \"\\u{2003}\\n a\"] fn f() {}", "\u{2003}\na"),
        (
            "/**\n * first  \n */\n#[doc = \"second\"] fn f() {}",
            "first  \nsecond",
        ),
        ("/*! a */ /*! b */ fn f() { work()?; }", "a \nb "),
        (
            "#[/* extra */ doc /* after */ = \"audit\"] fn f() {}",
            "audit",
        ),
        ("#[doc = r###\"a\"#b\"###] fn f() {}", "a\"#b"),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), expected, "{source:?}")
        });
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_escape_origins_are_complete_tokens_after_cleanup() {
    indexes(b"#[doc = r\"a\n\nb\"] fn f() {}", |index| {
        assert_eq!(
            maps(text(index)),
            vec![
                (0..1, "copied", vec![10..11]),
                (1..2, "copied", vec![11..12]),
                (2..3, "copied", vec![12..13]),
                (3..4, "copied", vec![13..14])
            ]
        );
    });
    for (source, expected, origins) in [
        (
            r#"#[doc = "  \u{1F600}\n  z"] fn f() {}"#,
            "😀\nz",
            vec![
                (0..4, "decoded", vec![11..20]),
                (4..5, "decoded", vec![20..22]),
                (5..6, "copied", vec![24..25]),
            ],
        ),
        (
            r#"#[doc = "a\r\nb"] fn f() {}"#,
            "a\nb",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "decoded", vec![12..14]),
                (2..3, "copied", vec![14..15]),
            ],
        ),
        (
            "#[doc = \"a\r\nb\"] fn f() {}",
            "a\nb",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "decoded", vec![10..12]),
                (2..3, "copied", vec![12..13]),
            ],
        ),
        (
            "#[doc = r\"a\r\nb\"] fn f() {}",
            "a\nb",
            vec![
                (0..1, "copied", vec![10..11]),
                (1..2, "decoded", vec![11..13]),
                (2..3, "copied", vec![13..14]),
            ],
        ),
        (
            "#[doc = \"a\\\n \t\r\nb\"] fn f() {}",
            "ab",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "copied", vec![16..17]),
            ],
        ),
        (
            "#[doc = \"a\\\r\n \t\nb\"] fn f() {}",
            "ab",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "copied", vec![16..17]),
            ],
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            let complete = text(index);
            assert_eq!(complete.text(), expected, "{source:?}");
            assert_eq!(maps(complete), origins, "{source:?}");
            if expected.starts_with('😀') {
                assert_eq!(
                    complete.origins(1..4).unwrap_err(),
                    TextRangeError::ScalarBoundary
                );
                let origin = complete.origins(0..4).unwrap().next().unwrap();
                let OriginView::Decoded(pieces) = origin.origin else {
                    panic!("escape")
                };
                assert_eq!(
                    pieces.iter().map(SourcePiece::range).collect::<Vec<_>>(),
                    vec![11..20]
                );
            }
        });
    }
    indexes(br#"#[doc = "\"\'\\\n\r\t\0\x41"] fn f() {}"#, |index| {
        assert_eq!(text(index).text(), "\"'\\\n\r\t\0A")
    });
}

#[test]
#[cfg(feature = "rust")]
#[allow(clippy::too_many_lines)] // The table pins priority between contributor states in both source orders.
fn rust_literal_state_priority_is_pending_gap_boundary_then_source_order() {
    use UnavailableReason::{GrammarBoundary, InvalidEncoding, NonScalar, UnsupportedDecoding};
    for (value, reason) in [
        (r#"b"\xff""#, UnsupportedDecoding),
        (r#"c"a""#, UnsupportedDecoding),
        (r##"br#"a"#"##, UnsupportedDecoding),
        (r##"cr#"a"#"##, UnsupportedDecoding),
        (r#""\q""#, InvalidEncoding),
        (r#""\x80""#, InvalidEncoding),
        (r#""\u0041""#, InvalidEncoding),
        (r#""\u{D800}""#, NonScalar),
        (r#""\u{110000}""#, NonScalar),
        (r#""\u{1_f600}""#, GrammarBoundary),
        (r#""\x1""#, GrammarBoundary),
        (r#""\xGG""#, GrammarBoundary),
        (r#""\xé1""#, GrammarBoundary),
        (r#""\u{}""#, GrammarBoundary),
        (r#""\u{1234567}""#, InvalidEncoding),
        ("\"a\rb\"", InvalidEncoding),
        ("\"a\\\n \rb\"", InvalidEncoding),
        ("r\"a\rb\"", InvalidEncoding),
    ] {
        let source = format!("#[doc = {value}] fn f() {{}}");
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), 1, "{source}");
            assert!(
                matches!(index.documents().next().unwrap().get().content(), Content::Unavailable(actual) if *actual == reason),
                "{source}"
            );
            assert_eq!(index.associations().len(), 1);
        });
    }
    for (first, second, reason) in [
        (r#""\x80""#, r#""\u{D800}""#, InvalidEncoding),
        (r#""\u{D800}""#, r#""\x80""#, NonScalar),
        (r#"b"x""#, r#""\x80""#, UnsupportedDecoding),
        (r#""\x80""#, r#"b"x""#, InvalidEncoding),
    ] {
        let source = format!("#[doc = {first}] #[doc = {second}] fn f() {{}}");
        indexes(source.as_bytes(), |index| {
            assert!(
                matches!(index.documents().next().unwrap().get().content(), Content::Unavailable(actual) if *actual == reason),
                "{source}"
            )
        });
    }
    for source in [
        "#[doc = b\"x\"] #[doc(hidden)] fn f() {}",
        "#[doc(hidden)] #[doc = b\"x\"] fn f() {}",
        "#[doc(alias = \"not prose\")] #[doc = \"known\"] fn f() {}",
        "#[doc = \"known\"] #[doc = concat!(\"computed\")] fn f() {}",
    ] {
        indexes(source.as_bytes(), |index| {
            assert!(
                matches!(
                    index.documents().next().unwrap().get().content(),
                    Content::PendingAdapter
                ),
                "{source}"
            )
        });
    }
    for (source, reason) in [
        (
            b"#[doc = b\"\xff\"] fn f() {}".as_slice(),
            UnsupportedDecoding,
        ),
        (b"#[doc = \"\xff\"] fn f() {}", InvalidEncoding),
    ] {
        indexes(source, |index| {
            assert!(
                matches!(index.documents().next().unwrap().get().content(), Content::Unavailable(actual) if *actual == reason)
            )
        });
    }
    for source in [
        b"/// a\xff\n#[doc = b\"x\"] fn f() {}".as_slice(),
        b"#[doc = \"\xff\\u{D800}\"] fn f() {}",
    ] {
        indexes(source, |index| {
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::Unavailable(InvalidEncoding)
            ))
        });
    }
    indexes(b"#[doc = \"\\u{D800}\xff\"] fn f() {}", |index| {
        assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(NonScalar)
        ))
    });
    for source in [
        "#[doc = \"\\x80\"] #[doc = ] fn f() {}",
        "#[doc = ] #[doc = \"\\x80\"] fn f() {}",
    ] {
        indexes(source.as_bytes(), |index| {
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::Unavailable(GrammarBoundary)
            ))
        });
    }
}

#[test]
#[cfg(feature = "rust")]
#[allow(clippy::too_many_lines)] // Recovery controls keep content, exact neighbor links and pending-owner evidence together.
fn rust_literal_recovery_keeps_a_valid_neighbor_and_owner_uncertainty() {
    indexes(b"#[doc = \"tail\"]", |index| {
        assert_eq!(text(index).text(), "tail");
        assert_eq!(index.associations().len(), 0);
        let document = index.documents().next().unwrap();
        let linked: Vec<_> = index
            .assessments()
            .filter(|a| {
                a.get().axis == AssessmentAxis::Owner
                    && index.assessed_document(*a).unwrap() == Some(document)
            })
            .collect();
        assert_eq!(linked.len(), 1);
        assert!(linked[0].get().node.is_none());
        let root = index.canonical.nodes[0].evidence.node();
        assert_eq!(root.byte_range(), 0..15);
        assert!(
            index
                .node_associations(root)
                .unwrap()
                .assessments()
                .any(|a| a == linked[0])
        );
    });
    for broken in [
        "#[doc = ]",
        "#[doc \"a\"]",
        "#[doc = \"a\"",
        "#[doc = \"a]",
        "#[doc]",
    ] {
        let source = format!("{broken} fn f() {{}}\n/// good\nfn g() {{}}");
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), 2, "{source}");
            assert!(
                matches!(
                    index.documents().next().unwrap().get().content(),
                    Content::Unavailable(UnavailableReason::GrammarBoundary)
                ),
                "{source}"
            );
            let last = index.documents().last().unwrap();
            assert_eq!(index.complete_text(last).unwrap().unwrap().text(), "good");
            // expected: the fixed suffix places the good comment at B+11 and g at B+20.
            let comment = index.facts().document_fragment(last, 0).unwrap();
            assert_eq!(
                comment.get().source().pieces()[0].range(),
                broken.len() + 11..broken.len() + 19
            );
            let good: Vec<_> = index
                .associations()
                .filter(|association| {
                    association.get().binding.is_some_and(|node| {
                        node.node().byte_range() == (broken.len() + 20..broken.len() + 29)
                    })
                })
                .collect();
            assert_eq!(good.len(), 1);
            let association = good[0];
            assert_eq!(
                index.facts().associated_document(association).unwrap(),
                last
            );
            assert_eq!(
                association.get().lexical_scope.node().byte_range(),
                0..source.len()
            );
            let owner = index.facts().associated_owner(association, 0).unwrap();
            let Owner::Node(node) = owner.get() else {
                panic!("good function")
            };
            assert_eq!(
                node.node().byte_range(),
                broken.len() + 20..broken.len() + 29
            );
            assert_eq!(
                index
                    .node_associations(node.node())
                    .unwrap()
                    .associations()
                    .collect::<Vec<_>>(),
                good
            );
            assert_eq!(
                index
                    .owner_associations(owner)
                    .unwrap()
                    .associations()
                    .collect::<Vec<_>>(),
                good
            );
        });
    }
    indexes(b"struct S(#[doc = \"slot\"] u32);", |index| {
        assert_eq!(text(index).text(), "slot");
        assert_eq!(index.associations().len(), 0);
        assert!(
            index
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Owner)
        );
        let document = index.documents().next().unwrap();
        let linked: Vec<_> = index
            .assessments()
            .filter(|a| {
                a.get().axis == AssessmentAxis::Owner
                    && index.assessed_document(*a).unwrap() == Some(document)
            })
            .collect();
        assert_eq!(linked.len(), 1);
        let candidate = linked[0].get().node.unwrap().node();
        assert_eq!(candidate.byte_range(), 25..28);
        assert_eq!(candidate.kind(), "primitive_type");
        let query = index.node_associations(candidate).unwrap();
        assert!(query.projection_pending());
        assert_eq!(query.associations().len(), 0);
        assert!(query.assessments().any(|a| a == linked[0]));
        assert!(
            !index
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Composition)
        );
    });
}

#[cfg(feature = "rust")]
fn included(
    source: &[u8],
    gaps: &[Range<usize>],
    split: bool,
    check: impl Fn(&DocumentationIndex<'_>),
) {
    // bca: suppress(halstead) -- exact included-byte and physical-point construction keeps the supplied parse reproducible.
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    let point = |at| {
        let prefix = &source[..at];
        tree_sitter::Point::new(
            prefix.iter().filter(|&&b| b == b'\n').count(),
            prefix
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(at, |lf| at - lf - 1),
        )
    };
    let mut spans: Vec<Range<usize>> = Vec::new();
    for at in 0..source.len() {
        if gaps.iter().any(|gap| gap.contains(&at)) {
            continue;
        }
        if !split
            && let Some(last) = spans.last_mut()
            && last.end == at
        {
            last.end += 1;
        } else {
            spans.push(at..at + 1);
        }
    }
    let ranges: Vec<_> = spans
        .into_iter()
        .map(|span| tree_sitter::Range {
            start_byte: span.start,
            end_byte: span.end,
            start_point: point(span.start),
            end_point: point(span.end),
        })
        .collect();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let index = collect(
        LANG::Rust,
        &tree,
        source,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    check(&index);
}

#[test]
#[cfg(feature = "rust")]
#[allow(clippy::too_many_lines)] // Both gap orderings pin contributor ranges, state priority and graph links.
fn rust_literal_excluded_path_stays_scope_only_and_value_gap_contributes() {
    let source = b"#[doc = \"\\x80\"] #[doc = \"bXc\"] fn f() {}";
    included(source, &[26..27], false, |index| {
        assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(UnavailableReason::ExcludedGap)
        ))
    });
    included(
        b"/// a\n#[doXc = \"b\"] fn f() {}",
        &[10..11],
        false,
        |index| {
            assert_eq!(text(index).text(), "a");
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 1);
            assert!(
                index
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Occurrence)
            );
            assert_path_gap_links(index, 6..19, 20..29, 0..5);
        },
    );
    included(
        b"#[doXc = \"b\"]\n/// a\nfn f() {}",
        &[4..5],
        false,
        |index| {
            assert_eq!(text(index).text(), "a");
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 1);
            assert!(
                index
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Occurrence)
            );
            assert_path_gap_links(index, 0..13, 20..29, 14..19);
        },
    );
    included(
        b"#[doc = \"bXc\"]\n/// a\nfn f() {}",
        &[10..11],
        false,
        |index| {
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::Unavailable(UnavailableReason::ExcludedGap)
            ));
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 2);
        },
    );
    for suffix in ["", " #[doc = \"\\x80\"]", " #[doc = ]"] {
        let source = format!("/// a\n#[doc = \"bXc\"]{suffix} fn f() {{}}");
        included(source.as_bytes(), &[16..17], false, |index| {
            assert_eq!(index.documents().len(), 1);
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::Unavailable(UnavailableReason::ExcludedGap)
            ));
            let fragment = index
                .facts()
                .document_fragment(index.documents().next().unwrap(), 1)
                .unwrap();
            assert_eq!(
                fragment
                    .get()
                    .source()
                    .pieces()
                    .iter()
                    .map(SourcePiece::range)
                    .collect::<Vec<_>>(),
                vec![6..16, 17..20]
            );
            let attribute = fragment.get().node().unwrap().node();
            let attribute = attribute.named_child(0).unwrap();
            let path = attribute.named_child(0).unwrap();
            assert_eq!(path.byte_range(), 8..11);
            assert!(index.source.covers(path.byte_range()));
            assert_eq!(&index.source()[path.byte_range()], b"doc");
            let value = attribute.child_by_field_name("value").unwrap();
            assert_eq!(value.byte_range(), 14..19);
            assert!(!index.source.covers(value.byte_range()));
            assert_eq!(index.associations().len(), 1);
            let association = index.associations().next().unwrap();
            assert_eq!(
                index.facts().associated_document(association).unwrap(),
                index.documents().next().unwrap()
            );
            assert_eq!(
                association.get().binding.unwrap().node().byte_range(),
                21 + suffix.len()..30 + suffix.len()
            );
            assert_eq!(
                association.get().lexical_scope.node().byte_range(),
                0..source.len()
            );
        });
    }
    for (source, gap) in [
        ("#[doc = \"bXc\"] #[doc(hidden)] fn f() {}", 10..11),
        ("#[doc(hidden)] #[doc = \"bXc\"] fn f() {}", 25..26),
    ] {
        included(source.as_bytes(), &[gap], false, |index| {
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::PendingAdapter
            ))
        });
    }
    included(
        "#[doc = \"é\\u{1F600}\"] fn f() {}".as_bytes(),
        &[],
        true,
        |index| {
            let complete = text(index);
            assert_eq!(complete.text(), "é😀");
            assert_eq!(
                maps(complete),
                vec![
                    (0..1, "copied", vec![9..10]),
                    (1..2, "copied", vec![10..11]),
                    (2..6, "decoded", (11..20).map(|at| at..at + 1).collect())
                ]
            );
        },
    );
}

#[cfg(feature = "rust")]
fn assert_path_gap_links(
    index: &DocumentationIndex<'_>,
    attribute: Range<usize>,
    owner_span: Range<usize>,
    comment: Range<usize>,
) {
    // bca: suppress(halstead) -- the same occurrence/document/owner handles are checked through both prepared views.
    let occurrences: Vec<_> = index
        .assessments()
        .filter(|a| a.get().axis == AssessmentAxis::Occurrence)
        .collect();
    assert_eq!(occurrences.len(), 1);
    let occurrence = occurrences[0];
    assert_eq!(
        occurrence.get().node.unwrap().node().byte_range(),
        attribute
    );
    assert_eq!(index.assessed_document(occurrence).unwrap(), None);
    let document = index.documents().next().unwrap();
    assert_eq!(
        index
            .facts()
            .document_fragment(document, 0)
            .unwrap()
            .get()
            .source()
            .pieces()[0]
            .range(),
        comment
    );
    assert_eq!(index.associations().len(), 1);
    let association = index.associations().next().unwrap();
    assert_eq!(
        index.facts().associated_document(association).unwrap(),
        document
    );
    assert_eq!(
        association.get().binding.unwrap().node().byte_range(),
        owner_span
    );
    assert_eq!(
        association.get().lexical_scope.node().byte_range(),
        0..index.source().len()
    );
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("documented function")
    };
    assert_eq!(node.node().byte_range(), owner_span);
    let query = index.node_associations(node.node()).unwrap();
    assert_eq!(query.associations().collect::<Vec<_>>(), vec![association]);
    assert!(query.assessments().any(|a| a == occurrence));
    assert_eq!(
        index
            .owner_associations(owner)
            .unwrap()
            .associations()
            .collect::<Vec<_>>(),
        vec![association]
    );
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_inner_root_module_function_and_private_nested_source_owners() {
    for (source, kinds, texts, owners, bindings, scopes) in [
        (
            "#![doc = \"root\"]",
            vec!["source_file"],
            vec!["root"],
            vec![0..16],
            vec![None],
            vec![0..16],
        ),
        (
            "#[doc = \"outer\"] mod m { #![doc = \"inner\"] }",
            vec!["mod_item"],
            vec!["outer\ninner"],
            vec![17..44],
            vec![Some(17..44)],
            vec![0..44],
        ),
        (
            "#[doc = \"outer\"] fn f() { #![doc = \"inner\"] }",
            vec!["function_item"],
            vec!["outer\ninner"],
            vec![17..45],
            vec![Some(17..45)],
            vec![0..45],
        ),
        (
            "#[test] #[doc = \"test\"] fn f() { #[doc = \"nested\"] fn g() {} }",
            vec!["function_item", "function_item"],
            vec!["test", "nested"],
            vec![24..62, 51..60],
            vec![Some(24..62), Some(51..60)],
            vec![0..62, 31..62],
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), texts.len());
            assert_eq!(index.associations().len(), kinds.len());
            for (position, ((document, association), (kind, expected))) in index
                .documents()
                .zip(index.associations())
                .zip(kinds.iter().zip(&texts))
                .enumerate()
            {
                assert_eq!(
                    index.facts().associated_document(association).unwrap(),
                    document
                );
                assert_eq!(
                    index.complete_text(document).unwrap().unwrap().text(),
                    *expected
                );
                assert_eq!(
                    document.get().role(),
                    if *kind == "function_item" {
                        DocumentRole::Primary
                    } else {
                        DocumentRole::FilePackage
                    }
                );
                let owner = index.facts().associated_owner(association, 0).unwrap();
                let (Owner::Node(node) | Owner::File(node)) = owner.get() else {
                    panic!("owner")
                };
                assert_eq!(node.node().kind(), *kind);
                assert_eq!(node.node().byte_range(), owners[position]);
                assert_eq!(
                    association
                        .get()
                        .binding
                        .map(|node| node.node().byte_range()),
                    bindings[position]
                );
                assert_eq!(
                    association.get().lexical_scope.node().byte_range(),
                    scopes[position]
                );
                assert_eq!(
                    index
                        .node_associations(node.node())
                        .unwrap()
                        .associations()
                        .collect::<Vec<_>>(),
                    vec![association]
                );
                assert_eq!(
                    index
                        .owner_associations(owner)
                        .unwrap()
                        .associations()
                        .collect::<Vec<_>>(),
                    vec![association]
                );
                let linked = index.facts().associated_document(association).unwrap();
                assert_eq!(
                    index.complete_text(linked).unwrap().unwrap().text(),
                    *expected
                );
            }
        });
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_same_row_declarations_keep_distinct_source_owners() {
    indexes(
        b"#[doc = \"a\"] fn f() {} #[doc = \"b\"] fn f() {}",
        |index| {
            assert_eq!(index.documents().len(), 2);
            assert_eq!(index.associations().len(), 2);
            for ((document, association), (expected, span)) in index
                .documents()
                .zip(index.associations())
                .zip([("a", 13..22), ("b", 36..45)])
            {
                assert_eq!(
                    index.complete_text(document).unwrap().unwrap().text(),
                    expected
                );
                let owner = index.facts().associated_owner(association, 0).unwrap();
                let Owner::Node(node) = owner.get() else {
                    panic!("function")
                };
                assert_eq!(node.node().byte_range(), span);
                assert_eq!(association.get().binding.unwrap().node().byte_range(), span);
                assert_eq!(
                    index
                        .node_associations(node.node())
                        .unwrap()
                        .associations()
                        .collect::<Vec<_>>(),
                    vec![association]
                );
            }
        },
    );
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_plain_trivia_and_real_interstitial_gaps_do_not_become_prose() {
    indexes(
        b"/// a\n#[inline]\n// plain\n#[doc = \"b\"]\n/// c\nfn f() {}",
        |index| {
            assert_eq!(text(index).text(), "a\nb\nc");
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 3);
        },
    );
    included(b"#[doc = \"a\"] X fn f() {}", &[13..14], false, |index| {
        assert_eq!(text(index).text(), "a");
        assert_eq!(index.associations().len(), 0);
        assert!(
            index
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Owner)
        );
    });
    let source = b"#[doc = \"a\"] /* plXain */ fn f() {}";
    let gap = source.iter().position(|&byte| byte == b'X').unwrap();
    included(source, &[gap..gap + 1], false, |index| {
        assert_eq!(text(index).text(), "a");
        assert_eq!(index.associations().len(), 0);
        assert!(
            index
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Owner)
        );
    });
}

#[test]
#[cfg(feature = "rust")]
fn ordinary_rust_sibling_spacing_adds_no_binding_coverage_searches() {
    use crate::documentation::source::included_range_visits;

    for count in [8, 32, 128] {
        let mut work = Vec::new();
        let mut nodes = Vec::new();
        for unit in ["fn f(){}", "fn f ( ) { } \n"] {
            let parser =
                AnyParser::parse(LANG::Rust, unit.repeat(count).into_bytes(), None, None).unwrap();
            assert!(!parser.ts_tree().root_node().has_error());
            assert_eq!(parser.ts_tree().root_node().named_child_count(), count);
            nodes.push(parser.root_node().preorder().count());
            let before = included_range_visits::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            work.push(included_range_visits::observed() - before);
            assert_eq!(index.facts().owners().len(), count + 1);
            assert_eq!(index.documents().len(), 0);
        }
        assert_eq!(nodes[0], nodes[1]);
        // expected: spacing changes no binding work when every sibling is ordinary syntax.
        assert_eq!(work[1], work[0]);
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_materializes_once_and_prices_decoding_projection_and_retained_reads() {
    use crate::documentation::text::{cleanup::projection_visits, rust::literal::decoded_units};
    for size in [8, 32, 128] {
        for (source, expected, units, copies, projections) in [
            (
                format!("{}fn f() {{}}", "/// a\n#[doc = \"b\"]\n".repeat(size)),
                String::new(),
                size,
                5 * size - 1,
                size,
            ),
            (
                format!("#[doc = \"{}\"] fn f() {{}}", "a\\x42".repeat(size)),
                "aB".repeat(size),
                2 * size,
                2 * size + 1,
                2 * size,
            ),
            (
                format!("#[doc = r\"{}\"] fn f() {{}}", "ab".repeat(size)),
                "ab".repeat(size),
                2 * size,
                2,
                1,
            ),
        ] {
            let expected = if expected.is_empty() {
                ["a", "b"].repeat(size).join("\n")
            } else {
                expected
            };
            let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
            let before = observations();
            let scanned = decoded_units::observed();
            let projected = projection_visits::observed();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let after = observations();
            let complete = text(&index);
            assert_eq!(complete.text(), expected);
            // Each source scalar/escape is visited once; each decoded map is projected once.
            // Mixed rows copy two content runs and one temporary literal, plus 2N-1 joins.
            // Cooked values copy 2N temporary runs plus one projected line; raw values copy twice.
            assert_eq!(decoded_units::observed() - scanned, units);
            assert_eq!(projection_visits::observed() - projected, projections);
            assert_eq!(after.copy_chunks - before.copy_chunks, copies);
            assert_eq!(after.renders - before.renders, 1);
            let retained = observations();
            for _ in 0..20 {
                let last = complete.text().len();
                assert_eq!(complete.origins(last - 1..last).unwrap().count(), 1);
                let association = index.associations().next().unwrap();
                let owner = index.facts().associated_owner(association, 0).unwrap();
                assert_eq!(
                    index
                        .owner_associations(owner)
                        .unwrap()
                        .associations()
                        .count(),
                    1
                );
            }
            assert_eq!(observations().renders, retained.renders);
            assert_eq!(observations().copy_chunks, retained.copy_chunks);
        }
        let source = format!(
            "{}{}fn f() {{}}{}",
            "mod m {".repeat(size),
            "#[doc = \"a\"]\n".repeat(size),
            "}".repeat(size)
        );
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), vec!["a"; size].join("\n"));
            assert_eq!(
                index.documents().next().unwrap().get().fragment_count(),
                size
            );
        });
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_carrier_child_work_depends_on_width_not_depth() {
    use super::rust::{attribute_children, carrier_children};
    for size in [8, 32, 128] {
        for (depth, width) in [(0, size), (size, 1), (size, size)] {
            let source = format!(
                "{}{}fn f() {{}}{}",
                "mod m {".repeat(depth),
                "#[doc = \"a\"]\n".repeat(width),
                "}".repeat(depth)
            );
            let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
            assert!(!parser.ts_tree().root_node().has_error());
            let context = DocumentationContext::default();
            let options = DocumentationOptions::default();
            for raw in [false, true] {
                let before = (carrier_children::observed(), attribute_children::observed());
                let index = if raw {
                    collect(
                        LANG::Rust,
                        parser.ts_tree(),
                        parser.code(),
                        &context,
                        &options,
                    )
                } else {
                    parser.documentation(&context, &options)
                }
                .unwrap();
                // expected: #, [, attribute (return before ]); path, =, value. No extras in this fixture.
                assert_eq!(carrier_children::observed() - before.0, 3 * width);
                assert_eq!(attribute_children::observed() - before.1, 3 * width);
                assert_eq!(text(&index).text(), vec!["a"; width].join("\n"));
                let document = index.documents().next().unwrap();
                assert_eq!(document.get().fragment_count(), width);
                for at in 0..width {
                    let fragment = index.facts().document_fragment(document, at).unwrap();
                    assert_eq!(fragment.get().form(), FragmentForm::Attribute);
                    assert_eq!(
                        fragment.get().source().pieces()[0].bytes(),
                        b"#[doc = \"a\"]"
                    );
                }
            }
        }
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_many_adjacent_pieces_do_not_restart_global_intersections() {
    use crate::documentation::source::piece_copies;
    use crate::documentation::text::source_cursor::fragment_piece_visits;
    for size in [8, 32, 128] {
        let source = format!("#[doc = \"{}\"] fn f() {{}}", "\\x41".repeat(size));
        for split in [true, false] {
            let before = fragment_piece_visits::observed();
            let copies = piece_copies::observed();
            included(source.as_bytes(), &[], split, |index| {
                assert_eq!(text(index).text(), "A".repeat(size));
                assert_eq!(text(index).mappings().len(), size);
                // expected: split tokens clone four responsible pieces; contiguous tokens clone one.
                assert_eq!(
                    piece_copies::observed() - copies,
                    if split { 4 * size } else { size }
                );
                // Split cursor: nine prefix pieces, four bytes and one lookahead per escape.
                assert_eq!(
                    fragment_piece_visits::observed() - before,
                    if split { 9 + 5 * size } else { size }
                );
                let expected: Vec<_> = (0..size)
                    .map(|at| {
                        let range = 9 + 4 * at..13 + 4 * at;
                        let pieces = if split {
                            range.map(|byte| byte..byte + 1).collect()
                        } else {
                            vec![range]
                        };
                        (at..at + 1, "decoded", pieces)
                    })
                    .collect();
                assert_eq!(maps(text(index)), expected);
                let document = index.documents().next().unwrap();
                assert_eq!(document.get().fragment_count(), 1);
                let fragment = index.facts().document_fragment(document, 0).unwrap();
                assert_eq!(
                    fragment.get().node().unwrap().node().byte_range(),
                    0..11 + 4 * size
                );
                assert_eq!(
                    fragment.get().source().pieces().len(),
                    if split { 11 + 4 * size } else { 1 }
                );
                let retained = piece_copies::observed();
                for _ in 0..20 {
                    let complete = text(index);
                    let portions: Vec<_> = complete.origins(size - 1..size).unwrap().collect();
                    assert_eq!(portions.len(), 1);
                    let OriginView::Decoded(pieces) = &portions[0].origin else {
                        panic!("last escape")
                    };
                    assert_eq!(
                        pieces.iter().map(SourcePiece::range).collect::<Vec<_>>(),
                        expected[size - 1].2
                    );
                    let association = index.associations().next().unwrap();
                    let owner = index.facts().associated_owner(association, 0).unwrap();
                    assert_eq!(
                        index
                            .owner_associations(owner)
                            .unwrap()
                            .associations()
                            .collect::<Vec<_>>(),
                        vec![association]
                    );
                }
                assert_eq!(piece_copies::observed() - retained, 0);
            });
        }
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_literal_text_phase_cancellation_restores_next_collection() {
    use crate::documentation::budget::{WorkPhase, cancel_in_phase};
    for source in [
        format!("#[doc = \"{}\"] fn f() {{}}", "\\x41".repeat(20_000)),
        format!("#[doc = r\"{}\"] fn f() {{}}", "raw".repeat(20_000)),
        format!("{}fn f() {{}}", "#[doc = \"a\"]\n".repeat(5000)),
        format!("/**{}* a\n * b*/ fn f() {{}}", "\u{2003}".repeat(20_000)),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.into_bytes(), None, None).unwrap();
        let collect = || {
            parser.documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
        };
        assert!(matches!(
            cancel_in_phase(WorkPhase::Text, collect),
            Err(CollectionError::Cancelled)
        ));
        assert_eq!(collect().unwrap().documents().len(), 1);
    }
}

#[test]
#[cfg(feature = "rust")]
fn rust_raw_identifier_attribute_names_are_documentation_carriers() {
    // rustc reads `r#doc` and `r#cfg_attr` as `doc` and `cfg_attr`.
    type Expected = (
        &'static str,
        &'static str,
        Vec<ExpectedMapping>,
        Vec<Range<usize>>,
        Option<Range<usize>>,
    );
    let cases: [Expected; 3] = [
        (
            "#[r#doc = \"raw name\"]\nfn f() {}",
            "raw name",
            vec![(0..8, "copied", vec![11..19])],
            vec![0..21],
            Some(22..31),
        ),
        (
            "#[doc = \"first\"]\n#[r#doc = \"second\"]\nfn f() {}",
            "first\nsecond",
            vec![
                (0..5, "copied", vec![9..14]),
                (5..6, "synthetic", vec![]),
                (6..12, "copied", vec![28..34]),
            ],
            vec![0..16, 17..36],
            Some(37..46),
        ),
        (
            "#![r#doc = \"inner\"]",
            "inner",
            vec![(0..5, "copied", vec![12..17])],
            vec![0..19],
            None,
        ),
    ];
    for (source, expected, mappings, fragments, function) in cases {
        indexes(source.as_bytes(), |index| {
            let complete = text(index);
            assert_eq!(complete.text(), expected, "{source}");
            assert_eq!(maps(complete), mappings, "{source}");
            let document = index.documents().next().unwrap();
            assert_eq!(document.get().fragment_count(), fragments.len());
            for (position, span) in fragments.iter().enumerate() {
                let fragment = index.facts().document_fragment(document, position).unwrap();
                assert_eq!(fragment.get().form(), FragmentForm::Attribute);
                assert_eq!(fragment.get().source().pieces()[0].range(), *span);
            }
            assert_eq!(index.associations().len(), 1);
            let association = index.associations().next().unwrap();
            assert_eq!(association.get().resolution, Resolution::Resolved);
            assert_eq!(association.get().owner_count(), 1);
            let owner = index.facts().associated_owner(association, 0).unwrap();
            match (owner.get(), &function) {
                (Owner::Node(node), Some(range)) => {
                    assert_eq!(node.node().kind(), "function_item");
                    assert_eq!(node.node().byte_range(), *range);
                }
                (Owner::File(_), None) => {}
                (other, _) => panic!("{source}: {other:?}"),
            }
            assert!(!index.assessments().any(|a| matches!(
                a.get().axis,
                AssessmentAxis::Occurrence | AssessmentAxis::Composition
            )));
        });
    }
    for source in [
        "#[doc = \"first\"]\n#[r#cfg_attr(all(), doc = \"second\")]\nfn f() {}",
        "#[doc = \"first\"]\n#[r#doc(hidden)]\nfn f() {}",
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), 1, "{source}");
            assert!(
                matches!(
                    index.documents().next().unwrap().get().content(),
                    Content::PendingAdapter
                ),
                "{source}"
            );
        });
    }
    // Controls: a name that only starts with the raw prefix or with `doc` is no carrier.
    for source in ["#[r#docs = \"x\"]\nfn f() {}", "#[rdoc = \"x\"]\nfn f() {}"] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), 0, "{source}");
            assert_eq!(index.assessments().len(), 0, "{source}");
        });
    }
}
