use super::*;

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn outer_attribute_merges_binding_into_inner_comment_document() {
    for (source, kind, owner, raw, inner) in [
        (
            "#[doc = \"outer\"]\nfn f() { //! inner\n}",
            "function_item",
            17..37,
            26..35,
            30..35,
        ),
        (
            "#[doc = \"outer\"]\nmod m { //! inner\n}",
            "mod_item",
            17..36,
            25..34,
            29..34,
        ),
        (
            "#[doc = \"outer\"]\nfn f() { /*! inner*/ }",
            "function_item",
            17..39,
            26..37,
            30..35,
        ),
        (
            "#[doc = \"outer\"]\nmod m { /*! inner*/ }",
            "mod_item",
            17..38,
            25..36,
            29..34,
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            let document = index.documents().next().unwrap();
            assert_eq!(document.get().fragment_count(), 2);
            assert_eq!(document.get().composition(), Composition::AttributeSequence);
            assert_eq!(
                document.get().role(),
                if kind == "mod_item" {
                    DocumentRole::FilePackage
                } else {
                    DocumentRole::Primary
                }
            );
            assert_eq!(text(index).text(), "outer\ninner");
            assert_eq!(
                maps(text(index)),
                vec![
                    (0..5, "copied", vec![9..14]),
                    (5..6, "synthetic", vec![]),
                    (6..11, "copied", vec![inner.clone()]),
                ]
            );
            for (at, span, form) in [
                (0, 0..16, FragmentForm::Attribute),
                (1, raw.clone(), FragmentForm::Comment),
            ] {
                let fragment = index.facts().document_fragment(document, at).unwrap();
                assert_eq!(fragment.get().form(), form);
                assert_eq!(fragment.get().source().pieces()[0].range(), span);
                assert_eq!(
                    fragment.get().source().pieces()[0].bytes(),
                    &source.as_bytes()[span]
                );
            }
            assert_bound_document(index, kind, owner.clone());
        });
    }
}

// test-lang-gates: hand-written(rust) — shared only by Rust parser-backed regressions.
#[cfg(feature = "rust")]
fn assert_bound_document(index: &DocumentationIndex<'_>, kind: &str, span: Range<usize>) {
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let document = index.documents().next().unwrap();
    let association = index.associations().next().unwrap();
    assert_eq!(
        index.facts().associated_document(association).unwrap(),
        document
    );
    assert_eq!(
        association
            .get()
            .binding
            .map(|node| node.node().byte_range()),
        Some(span.clone())
    );
    assert_eq!(
        association.get().lexical_scope.node().byte_range(),
        0..index.source().len()
    );
    assert_eq!(association.get().owner_count(), 1);
    assert_eq!(association.get().publication, Publication::Unknown);
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("item owner")
    };
    assert_eq!(node.node().kind(), kind);
    assert_eq!(node.node().byte_range(), span);
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
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn outer_metadata_keeps_binding_on_pending_composition() {
    indexes(b"#[doc(hidden)]\nfn f() { //! inner\n}", |index| {
        let document = index.documents().next().unwrap();
        assert!(matches!(document.get().content(), Content::PendingAdapter));
        assert_eq!(document.get().fragment_count(), 2);
        assert_eq!(document.get().role(), DocumentRole::Primary);
        for (at, span) in [(0, 0..14), (1, 24..33)] {
            assert_eq!(
                index
                    .facts()
                    .document_fragment(document, at)
                    .unwrap()
                    .get()
                    .source()
                    .pieces()[0]
                    .range(),
                span
            );
        }
        let assessments: Vec<_> = index
            .assessments()
            .filter(|a| a.get().axis == AssessmentAxis::Composition)
            .collect();
        assert_eq!(assessments.len(), 1);
        assert_eq!(
            assessments[0].get().node.map(|n| n.node().byte_range()),
            Some(15..35)
        );
        assert_eq!(
            index.assessed_document(assessments[0]).unwrap(),
            Some(document)
        );
        assert_bound_document(index, "function_item", 15..35);
    });
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn inner_only_and_existing_outer_bindings_remain_distinct() {
    for (source, binding, expected) in [
        ("fn f() { //! inner\n}", None, "inner"),
        ("fn f() { #![doc = \"inner\"] }", None, "inner"),
        (
            "/// outer\nfn f() { //! inner\n}",
            Some(10..30),
            "outer\ninner",
        ),
        (
            "/// outer\nfn f() { #![doc = \"inner\"] }",
            Some(10..38),
            "outer\ninner",
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), expected);
            let association = index.associations().next().unwrap();
            assert_eq!(
                association.get().binding.map(|n| n.node().byte_range()),
                binding
            );
        });
    }
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn changed_beautification_reinterprets_one_cr_before_terminal_empty() {
    // Rust 1.99.0's unchanged cleanup bodies independently establish these texts.
    // Escape offsets below are counted from the source spelling, not oracle spans.
    for (source, expected, origins) in [
        (
            r#"#[doc = "\na\r\r\nb"] fn f() {}"#,
            "a\nb",
            vec![
                (0..1, "copied", vec![11..12]),
                (1..2, "decoded", vec![16..18]),
                (2..3, "copied", vec![18..19]),
            ],
        ),
        (
            r#"#[doc = "a\r\r\nb"] fn f() {}"#,
            "a\r\nb",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "decoded", vec![10..12]),
                (2..3, "decoded", vec![14..16]),
                (3..4, "copied", vec![16..17]),
            ],
        ),
        (
            r#"#[doc = "\na\r\n"] fn f() {}"#,
            "a",
            vec![(0..1, "copied", vec![11..12])],
        ),
        (
            r#"#[doc = "\na\r\r\n\n"] fn f() {}"#,
            "a",
            vec![(0..1, "copied", vec![11..12])],
        ),
        (
            r#"#[doc = "\na\r\r\r\nb"] fn f() {}"#,
            "a\r\nb",
            vec![
                (0..1, "copied", vec![11..12]),
                (1..2, "decoded", vec![12..14]),
                (2..3, "decoded", vec![18..20]),
                (3..4, "copied", vec![20..21]),
            ],
        ),
        (
            r#"#[doc = "a\r\r\r\nb"] fn f() {}"#,
            "a\r\r\nb",
            vec![
                (0..1, "copied", vec![9..10]),
                (1..2, "decoded", vec![10..12]),
                (2..3, "decoded", vec![12..14]),
                (3..4, "decoded", vec![16..18]),
                (4..5, "copied", vec![18..19]),
            ],
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), expected, "{source}");
            assert_eq!(maps(text(index)), origins, "{source}");
        });
    }
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn second_line_interpretation_preserves_comment_newline_origins() {
    // Bare CR in comments is collector policy, not accepted Rust/rustdoc parity.
    for (source, expected, origins) in [
        (
            "/**\na\r\r\nb*/ fn f() {}",
            "a\nb",
            vec![
                (0..1, "copied", vec![4..5]),
                (1..2, "decoded", vec![6..8]),
                (2..3, "copied", vec![8..9]),
            ],
        ),
        (
            "/**\na\r\r\nb*/ #[doc = \"z\"] fn f() {}",
            "a\nb\nz",
            vec![
                (0..1, "copied", vec![4..5]),
                (1..2, "decoded", vec![6..8]),
                (2..3, "copied", vec![8..9]),
                (3..4, "synthetic", vec![]),
                (4..5, "copied", vec![21..22]),
            ],
        ),
        (
            "/// z\n#[doc = \"\\na\\r\\r\\r\\nb\"] fn f() {}",
            "z\na\r\nb",
            vec![
                (0..1, "copied", vec![4..5]),
                (1..2, "synthetic", vec![]),
                (2..3, "copied", vec![17..18]),
                (3..4, "decoded", vec![18..20]),
                (4..5, "decoded", vec![24..26]),
                (5..6, "copied", vec![26..27]),
            ],
        ),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), expected, "{source:?}");
            assert_eq!(maps(text(index)), origins, "{source:?}");
        });
    }
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn unicode_first_line_star_keeps_ascii_decoration_and_exact_origins() {
    for space in ["\u{2003}", "\u{a0}"] {
        for mixed in [false, true] {
            let suffix = if mixed { "\n#[doc = \"z\"]" } else { "" };
            let source = format!("/**{space}* a\n * b*/{suffix}\nfn f() {{}}");
            let width = space.len();
            indexes(source.as_bytes(), |index| {
                let expected = format!("{space}* a\n * b{}", if mixed { "\nz" } else { "" });
                assert_eq!(text(index).text(), expected);
                let mut origins = vec![
                    (0..width + 3, "copied", vec![3..width + 6]),
                    (width + 3..width + 4, "copied", vec![width + 6..width + 7]),
                    (width + 4..width + 8, "copied", vec![width + 7..width + 11]),
                ];
                if mixed {
                    origins.extend([
                        (width + 8..width + 9, "synthetic", vec![]),
                        (
                            width + 9..width + 10,
                            "copied",
                            vec![width + 23..width + 24],
                        ),
                    ]);
                }
                assert_eq!(maps(text(index)), origins);
                assert_bound_document(
                    index,
                    "function_item",
                    width + 14 + suffix.len()..source.len(),
                );
                let document = index.documents().next().unwrap();
                assert_eq!(document.get().fragment_count(), if mixed { 2 } else { 1 });
                assert_eq!(
                    index
                        .facts()
                        .document_fragment(document, 0)
                        .unwrap()
                        .get()
                        .source()
                        .pieces()[0]
                        .range(),
                    0..width + 13
                );
                let star = text(index)
                    .origins(width + 5..width + 6)
                    .unwrap()
                    .next()
                    .unwrap();
                let OriginView::Copied(pieces) = star.origin else {
                    panic!("second star copied")
                };
                assert_eq!(pieces.range(), width + 8..width + 9);
            });
        }
    }
}

// test-lang-gates: hand-written(rust) — parent indexes helper parses Rust through use super::*.
#[test]
#[cfg(feature = "rust")]
fn first_line_eligibility_controls_ascii_blank_nonstar_and_invalid_utf8() {
    for (source, expected) in [
        ("/** * a\n * b*/ fn f() {}", "a\nb"),
        ("/** title\n * b*/ fn f() {}", "title\n b"),
        ("/**\n * a\n * b\n */ fn f() {}", "a\nb"),
        ("/**\u{2003}\n * b*/ fn f() {}", "\u{2003}\nb"),
    ] {
        indexes(source.as_bytes(), |index| {
            assert_eq!(text(index).text(), expected, "{source:?}")
        });
    }
    // Comment-only rendering reaches eligibility without mixed-renderer prevalidation.
    indexes(b"/**\xff* a\n * b*/ fn f() {}", |index| {
        assert_eq!(index.documents().len(), 1);
        assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(UnavailableReason::InvalidEncoding)
        ));
    });
}

#[test]
#[cfg(feature = "rust")]
fn reported_late_boundaries_are_grammar_controls_not_priority_counterexamples() {
    let hashes = "#".repeat(256);
    let raw = format!("#[doc = r{hashes}\"a\"{hashes}]");
    for source in [
        format!(r#"#[doc = "\x80"] {raw} fn f() {{}}"#),
        format!(r#"{raw} #[doc = "\x80"] fn f() {{}}"#),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let attribute = parser
            .root_node()
            .preorder()
            .find(|n| {
                n.kind() == "attribute_item"
                    && source.as_bytes()[n.as_tree_sitter().byte_range()].starts_with(b"#[doc = r")
            })
            .unwrap();
        assert!(attribute.as_tree_sitter().has_error());
        assert!(attribute.preorder().any(|n| n.kind() == "ERROR"));
        indexes(source.as_bytes(), |index| {
            assert_eq!(index.documents().len(), 1);
            let document = index.documents().next().unwrap();
            assert_eq!(document.get().fragment_count(), 2);
            assert!(matches!(
                document.get().content(),
                Content::Unavailable(UnavailableReason::GrammarBoundary)
            ));
        });
    }
    let source = b"#![doc = \"\\x80\"]\n/*! unclosed";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().preorder().any(|n| n.kind() == "ERROR"));
    assert!(
        !parser
            .root_node()
            .preorder()
            .any(|n| n.kind() == "block_comment")
    );
    indexes(source, |index| {
        assert_eq!(index.documents().len(), 1);
        let document = index.documents().next().unwrap();
        assert_eq!(document.get().fragment_count(), 1);
        assert!(matches!(
            document.get().content(),
            Content::Unavailable(UnavailableReason::InvalidEncoding)
        ));
    });
}
