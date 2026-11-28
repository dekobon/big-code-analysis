use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_local_pending_owner_keeps_complete_text_and_known_leading_association() {
    for (source, expected_docs, local_start) in [
        ("fun f() /**audit*/ {}", 1, 8),
        ("/**first*/\nfun f() /**audit*/ {}", 2, 19),
        ("class C { fun f() /**audit*/ {} }", 1, 18),
    ] {
        let parser =
            AnyParser::parse(LANG::Kotlin, source.as_bytes().to_vec(), None, None).unwrap();
        for retain_unbound in [false, true] {
            let context = DocumentationContext::default();
            let options = DocumentationOptions {
                retain_unbound,
                ..DocumentationOptions::default()
            };
            for index in [
                parser.documentation(&context, &options).unwrap(),
                collect(
                    LANG::Kotlin,
                    parser.ts_tree(),
                    parser.code(),
                    &context,
                    &options,
                )
                .unwrap(),
            ] {
                assert_eq!(index.documents().len(), expected_docs);
                let local = index.documents().last().unwrap();
                let fragment = index.facts().document_fragment(local, 0).unwrap();
                assert_eq!(
                    fragment.get().source().pieces()[0].range(),
                    local_start..local_start + 10
                );
                let text = index.complete_text(local).unwrap().unwrap();
                assert_eq!(text.text(), "audit");
                let origin = text.origins(0..5).unwrap().next().unwrap();
                let OriginView::Copied(piece) = origin.origin else {
                    panic!("copied KDoc")
                };
                assert_eq!(piece.range(), local_start + 3..local_start + 8);
                assert_eq!(index.associations().len(), expected_docs - 1);
                let function = parser
                    .root_node()
                    .preorder()
                    .find(|node| node.kind() == "function_declaration")
                    .unwrap();
                let query = index.node_associations(function.as_tree_sitter()).unwrap();
                assert_eq!(query.associations().len(), expected_docs - 1);
                assert!(
                    query
                        .assessments()
                        .any(|assessment| assessment.get().axis == AssessmentAxis::Owner
                            && index.assessed_document(assessment).unwrap() == Some(local))
                );
            }
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn pending_go_field_query_exposes_assessment_instead_of_unusable_error() {
    let parser = AnyParser::parse(
        LANG::Go,
        b"package p\ntype S struct {\n// both\nX, Y %% int\n}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let field = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap();
    let query = index.node_associations(field.as_tree_sitter()).unwrap();
    assert!(query.projection_pending());
    assert_eq!(query.associations().len(), 0);
    assert!(
        query
            .assessments()
            .any(|assessment| assessment.get().axis == AssessmentAxis::Owner)
    );
    assert_eq!(
        index
            .complete_text(index.documents().next().unwrap())
            .unwrap()
            .unwrap()
            .text(),
        "both"
    );
}

#[cfg(feature = "php")]
#[test]
fn php_default_precedence_preserves_original_candidate_sequence() {
    for (source, texts, files) in [
        ("<?php\n/** one */\nclass C {}", vec!["one"], vec![false]),
        (
            "<?php\n/** file */\n/** item */\nclass C {}",
            vec!["file", "item"],
            vec![true, false],
        ),
        (
            "<?php\n/** a */\n/** b */\n/** c */\nclass C {}",
            vec!["c"],
            vec![false],
        ),
        (
            "<?php\n/** audit */\nenum E { case A; }",
            vec!["audit"],
            vec![false],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Php, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), texts.len(), "{source}");
        assert_eq!(index.associations().len(), texts.len());
        for ((association, text), file) in index.associations().zip(texts).zip(files) {
            let document = index.facts().associated_document(association).unwrap();
            assert_eq!(index.complete_text(document).unwrap().unwrap().text(), text);
            let owner = index.facts().associated_owner(association, 0).unwrap();
            assert_eq!(matches!(owner.get(), Owner::File(_)), file);
        }
    }
}

#[cfg(all(
    feature = "java",
    feature = "kotlin",
    feature = "groovy",
    feature = "csharp"
))]
#[test]
fn annotation_bearing_declarations_retain_semantic_type_or_function() {
    for (language, source, kind) in [
        (
            LANG::Java,
            "/**audit*/\n@Deprecated class C {}",
            "class_declaration",
        ),
        (
            LANG::Kotlin,
            "/**audit*/\n@Deprecated(\"x\") fun f() {}",
            "function_declaration",
        ),
        (
            LANG::Groovy,
            "/**audit*/\n@Deprecated class C {}",
            "class_declaration",
        ),
        (
            LANG::Csharp,
            "///audit\n[System.Obsolete] class C {}",
            "class_declaration",
        ),
    ] {
        let parser = AnyParser::parse(language, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        assert_eq!(index.documents().len(), 1, "{language:?}");
        let association = index.associations().next().unwrap();
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(node) = owner.get() else {
            panic!("declaration")
        };
        assert_eq!(node.node().kind(), kind);
        assert_eq!(
            index
                .complete_text(index.documents().next().unwrap())
                .unwrap()
                .unwrap()
                .text(),
            "audit"
        );
    }
}

#[cfg(feature = "java")]
#[test]
fn java_prelex_keeps_source_cleaned_evidence_and_uncertain_association() {
    let parser = AnyParser::parse(
        LANG::Java,
        b"/** \\u0040audit */\nclass C {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let document = index.documents().next().unwrap();
    assert!(matches!(
        document.get().content(),
        Content::Unavailable(UnavailableReason::UnsupportedDecoding)
    ));
    assert!(index.complete_text(document).unwrap().is_none());
    let spelling = index.source_cleaned_text(document).unwrap().unwrap();
    assert_eq!(spelling.text(), r"\u0040audit");
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().resolution,
        Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
    );
    assert_eq!(association.get().owner_count(), 1);
}

#[cfg(any(feature = "java", feature = "groovy", feature = "csharp"))]
fn no_local<T: crate::Checker + crate::LanguageInfo>(source: &[u8]) {
    use crate::Search;
    let parser = AnyParser::parse(T::lang(), source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), source);
    let mut selected = 0;
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            streaming::markers::source_comment::<T>(
                node,
                ancestors,
                &mut streaming::markers::SourceRequest {
                    context: &context,
                    retain_unbound: true,
                    sink: &mut |run| {
                        for member in run.members {
                            assert!(!matches!(
                                member.placement,
                                streaming::markers::MarkerPlacement::Local
                            ));
                            selected += usize::from(
                                member.selection == streaming::markers::MarkerSelection::Selected,
                            );
                        }
                    },
                },
            )
            .unwrap();
        }
    });
    assert_eq!(selected, 1);
}

#[cfg(feature = "java")]
#[test]
fn java_current_hooks_never_emit_local_placement() {
    no_local::<crate::JavaCode>(b"/**audit*/\nclass C { void f() { /** note */ } }");
}
#[cfg(feature = "groovy")]
#[test]
fn groovy_current_hooks_never_emit_local_placement() {
    no_local::<crate::GroovyCode>(b"/**audit*/\nclass C { void f() { /** note */ } }");
}
#[cfg(feature = "csharp")]
#[test]
fn csharp_current_hooks_never_emit_local_placement() {
    no_local::<crate::CsharpCode>(b"///audit\nclass C { void F() { /** note */ } }");
}

#[cfg(feature = "rust")]
#[test]
fn retained_parse_abort_is_rejected_after_flag_scope_and_next_parse_succeeds() {
    let flag = crate::cancel::ParseCancel::new();
    flag.cancel();
    let source = "fn f() {}\n".repeat(10_000).into_bytes();
    let parser = flag.run(|| AnyParser::parse(LANG::Rust, source, None, None).unwrap());
    assert_eq!(
        parser.parse_completion(),
        crate::parser::ParseCompletion::Cancelled
    );
    assert!(matches!(
        parser.documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default()
        ),
        Err(CollectionError::Cancelled)
    ));
    let next = AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
    assert_eq!(
        next.documentation(
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
fn nested_collection_cancellation_restores_outer_flag_after_unwind() {
    let parser = AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
    let outer = crate::cancel::ParseCancel::new();
    let inner = crate::cancel::ParseCancel::new();
    inner.cancel();
    outer.run(|| {
        inner.run(|| {
            assert!(matches!(
                parser.documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default()
                ),
                Err(CollectionError::Cancelled)
            ));
        });
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
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.run(|| panic!("unwind cancellation scope"))
        }));
        assert!(stopped.is_err());
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
    });
}

#[cfg(feature = "rust")]
#[test]
fn excluded_attribute_name_keeps_pending_occurrence_without_establishing_doc_text() {
    let source = b"#[doc = \"audit\"]\nfn f() {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    parser
        .set_included_ranges(&[
            tree_sitter::Range {
                start_byte: 0,
                end_byte: 3,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(0, 3),
            },
            tree_sitter::Range {
                start_byte: 4,
                end_byte: source.len(),
                start_point: tree_sitter::Point::new(0, 4),
                end_point: tree_sitter::Point::new(1, 9),
            },
        ])
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let index = collect(
        LANG::Rust,
        &tree,
        source,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    assert_eq!(index.documents().len(), 0);
    let function = tree.root_node().named_child(1).unwrap();
    assert_eq!(function.kind(), "function_item");
    let query = index.node_associations(function).unwrap();
    assert!(
        query
            .assessments()
            .any(|assessment| assessment.get().axis == AssessmentAxis::Occurrence)
    );
}
