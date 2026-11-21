#![allow(
    clippy::single_range_in_vec_init,
    clippy::semicolon_if_nothing_returned,
    clippy::naive_bytecount
)]

use crate::documentation::*;
use crate::{AnyParser, LANG};
use std::ops::Range;

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
fn rust_literal_recovery_keeps_a_valid_neighbor_and_owner_uncertainty() {
    indexes(b"#[doc = \"tail\"]", |index| {
        assert_eq!(text(index).text(), "tail");
        assert_eq!(index.associations().len(), 0);
        assert!(
            index
                .assessments()
                .any(|a| a.get().axis == AssessmentAxis::Owner)
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

#[test]
#[cfg(feature = "rust")]
fn rust_literal_inner_root_module_function_and_private_nested_source_owners() {
    for (source, kinds, texts) in [
        ("#![doc = \"root\"]", vec!["source_file"], vec!["root"]),
        (
            "#[doc = \"outer\"] mod m { #![doc = \"inner\"] }",
            vec!["mod_item"],
            vec!["outer\ninner"],
        ),
        (
            "#[doc = \"outer\"] fn f() { #![doc = \"inner\"] }",
            vec!["function_item"],
            vec!["outer\ninner"],
        ),
        (
            "#[test] #[doc = \"test\"] fn f() { #[doc = \"nested\"] fn g() {} }",
            vec!["function_item", "function_item"],
            vec!["test", "nested"],
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), texts.len());
            assert_eq!(index.associations().len(), kinds.len());
            for ((document, association), (kind, expected)) in index
                .documents()
                .zip(index.associations())
                .zip(kinds.iter().zip(&texts))
            {
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
                assert_eq!(
                    index
                        .node_associations(node.node())
                        .unwrap()
                        .associations()
                        .count(),
                    1
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
fn rust_literal_many_adjacent_pieces_do_not_restart_global_intersections() {
    use crate::documentation::text::source_cursor::fragment_piece_visits;
    for size in [8, 32, 128] {
        let source = format!("#[doc = \"{}\"] fn f() {{}}", "\\x41".repeat(size));
        let before = fragment_piece_visits::observed();
        included(source.as_bytes(), &[], true, |index| {
            assert_eq!(text(index).text(), "A".repeat(size));
            assert_eq!(text(index).mappings().len(), size);
            // Nine prefix pieces, four bytes per escape, and one lookahead per escape.
            assert_eq!(fragment_piece_visits::observed() - before, 9 + 5 * size);
        });
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
