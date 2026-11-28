use crate::documentation::*;
use crate::{AnyParser, LANG};

#[cfg(feature = "cpp")]
#[test]
fn narrow_signature_grammar_defers_malformed_and_unsupported_references() {
    for reference in [
        "void f(,)",
        "void f(int,)",
        "void f(,int)",
        "void f(int,,int)",
        "void f(int x int y)",
        "void f(12)",
        "void f(int 12)",
        "void f(int x y)",
        "void f(void x)",
        "void f(void,int)",
        "void f((int))",
        "void f(int",
        "void f(int x[2])",
        "void f(int x = 1)",
        "void A::f()",
        "void f(...)",
        "void f() const",
        "void int()",
        "static f()",
        "void f(unsigned float)",
        "void f(int & &&)",
        "void f(int * const const)",
        "void f(const)",
        "void f(void &)",
    ] {
        let source = format!("void f(int x);\n/**@fn {reference}*/");
        let parser = AnyParser::parse(LANG::Cpp, source.into_bytes(), None, None).unwrap();
        for retain_unbound in [false, true] {
            let context = DocumentationContext::default();
            let options = DocumentationOptions {
                retain_unbound,
                ..DocumentationOptions::default()
            };
            for index in [
                parser.documentation(&context, &options).unwrap(),
                collect(
                    LANG::Cpp,
                    parser.ts_tree(),
                    parser.code(),
                    &context,
                    &options,
                )
                .unwrap(),
            ] {
                assert_eq!(index.documents().len(), 1, "{reference}");
                assert_eq!(index.associations().len(), 0, "{reference}");
                let document = index.documents().next().unwrap();
                assert_eq!(
                    index.complete_text(document).unwrap().unwrap().text(),
                    format!("@fn {reference}")
                );
                assert!(
                    index
                        .assessments()
                        .any(|a| index.assessed_document(a).unwrap() == Some(document)),
                    "{reference}"
                );
            }
        }
    }
}

#[cfg(feature = "cpp")]
#[test]
fn narrow_signature_grammar_keeps_supported_types_exact() {
    for signature in [
        "void f()",
        "void f(void)",
        "int f(int)",
        "int f(int x)",
        "T f(T x)",
        "unsigned long long int f(const T * volatile x)",
        "long double f(void *p, T const &x, volatile int &&y)",
        "const T * const * f(T volatile * const p)",
        "bool f(signed char x, unsigned short int, char16_t)",
    ] {
        let source = format!("{signature};\n/**@fn {signature}*/");
        let parser = AnyParser::parse(LANG::Cpp, source.into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error(), "{signature}");
        for retain_unbound in [false, true] {
            let context = DocumentationContext::default();
            let options = DocumentationOptions {
                retain_unbound,
                ..DocumentationOptions::default()
            };
            for index in [
                parser.documentation(&context, &options).unwrap(),
                collect(
                    LANG::Cpp,
                    parser.ts_tree(),
                    parser.code(),
                    &context,
                    &options,
                )
                .unwrap(),
            ] {
                assert_eq!(index.associations().len(), 1, "{signature}");
                assert_eq!(
                    index.associations().next().unwrap().get().resolution,
                    Resolution::Resolved,
                    "{signature}"
                );
                assert_eq!(index.qualifications().len(), 0);
            }
        }
    }
}

#[cfg(feature = "cpp")]
#[test]
fn unsupported_declared_signature_keeps_owner_and_blocks_false_unique_match() {
    let source = "/**local*/\nint f(int values[2]);\nint f(int x);\n/**@fn int f(int x)*/";
    let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
    assert!(!parser.root_node().has_error());
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let associations: Vec<_> = index.associations().collect();
    assert_eq!(associations.len(), 2);
    assert_eq!(associations[0].get().resolution, Resolution::Resolved);
    assert_eq!(
        associations[1].get().resolution,
        Resolution::Unresolved(UnresolvedReason::Ambiguous)
    );
    assert_eq!(associations[1].get().owner_count(), 2);
    let reference = index.facts().associated_document(associations[1]).unwrap();
    assert!(
        index
            .assessments()
            .any(|a| index.assessed_document(a).unwrap() == Some(reference))
    );
    assert_eq!(index.qualifications().len(), 0);
}

#[cfg(feature = "cpp")]
#[test]
#[allow(clippy::too_many_lines)] // Candidate and reference matrices pin independent qualification axes.
fn ambiguous_candidates_keep_structured_qualification_and_reference_identity() {
    for (source, expected) in [
        ("void f(void); void f(void);\n/**@fn f*/", vec![]),
        (
            "#if FLAG\nvoid f(void);\n#endif\nvoid f(void);\n/**@fn f*/",
            vec![UnresolvedReason::Conditional],
        ),
        (
            "void f(int x = );\nvoid f(void);\n/**@fn f*/",
            vec![UnresolvedReason::GrammarBoundary],
        ),
        (
            "#if FLAG\nvoid f(int x = );\n#endif\nvoid f(void);\n/**@fn f*/",
            vec![
                UnresolvedReason::Conditional,
                UnresolvedReason::GrammarBoundary,
            ],
        ),
        (
            "#if FLAG\nvoid f(void) {}\n#endif\nvoid f(void) {}\n/**@fn f*/",
            vec![UnresolvedReason::Conditional],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
        assert_eq!(parser.root_node().has_error(), source.contains("= )"));
        for retain_unbound in [false, true] {
            let context = DocumentationContext::default();
            let options = DocumentationOptions {
                retain_unbound,
                ..DocumentationOptions::default()
            };
            for index in [
                parser.documentation(&context, &options).unwrap(),
                collect(
                    LANG::Cpp,
                    parser.ts_tree(),
                    parser.code(),
                    &context,
                    &options,
                )
                .unwrap(),
            ] {
                let association = index.associations().next().unwrap();
                assert_eq!(
                    association.get().resolution,
                    Resolution::Unresolved(UnresolvedReason::Ambiguous)
                );
                assert_eq!(association.get().owner_count(), 2);
                let document = index.facts().associated_document(association).unwrap();
                let qualifications: Vec<_> =
                    index.document_qualifications(document).unwrap().collect();
                assert_eq!(
                    qualifications
                        .iter()
                        .map(|q| q.get().reason)
                        .collect::<Vec<_>>(),
                    expected,
                    "{source}"
                );
                assert_eq!(index.qualifications().collect::<Vec<_>>(), qualifications);
                for qualification in qualifications {
                    assert_eq!(qualification.get().subject, QualificationSubject::Candidate);
                    assert_eq!(index.qualified_document(qualification).unwrap(), document);
                    let owner = index.qualified_owner(qualification).unwrap().unwrap();
                    let Owner::Node(node) = owner.get() else {
                        panic!("candidate")
                    };
                    assert_eq!(node.node(), qualification.get().node.node());
                    assert!(
                        index
                            .node_associations(node.node())
                            .unwrap()
                            .qualifications()
                            .any(|q| q == qualification)
                    );
                    assert!(
                        index
                            .owner_associations(owner)
                            .unwrap()
                            .qualifications()
                            .any(|q| q == qualification)
                    );
                }
                let ordinary = index.facts().associated_owner(association, 0).unwrap();
                let ordinary = if expected.is_empty() {
                    ordinary
                } else {
                    index.facts().associated_owner(association, 1).unwrap()
                };
                assert_eq!(
                    index
                        .owner_associations(ordinary)
                        .unwrap()
                        .qualifications()
                        .count(),
                    0
                );
                if !source.contains("= )") {
                    assert!(
                        !index
                            .owner_associations(ordinary)
                            .unwrap()
                            .projection_pending()
                    );
                    assert_eq!(
                        index.document_qualifications(document).unwrap().len(),
                        expected.len()
                    );
                    assert!(
                        !index
                            .assessments()
                            .any(|a| index.assessed_document(a).unwrap() == Some(document))
                    );
                }
                let foreign = parser.documentation(&context, &options).unwrap();
                assert!(matches!(
                    foreign.document_qualifications(document),
                    Err(FactError::ForeignIndex)
                ));
                if let Some(q) = index.qualifications().next() {
                    assert!(matches!(
                        foreign.qualified_document(q),
                        Err(FactError::ForeignIndex)
                    ));
                    assert!(matches!(
                        foreign.qualified_owner(q),
                        Err(FactError::ForeignIndex)
                    ));
                }
            }
        }
    }
    let parser = AnyParser::parse(
        LANG::Cpp,
        b"void f(); void f();\n#if FLAG\n/**@fn f*/\n#endif".to_vec(),
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
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().resolution,
        Resolution::Unresolved(UnresolvedReason::Ambiguous)
    );
    let q = index.qualifications().next().unwrap();
    assert_eq!(q.get().subject, QualificationSubject::Reference);
    assert_eq!(q.get().reason, UnresolvedReason::Conditional);
    assert_eq!(
        &index.source()[q.get().node.node().byte_range()],
        b"/**@fn f*/"
    );
    assert!(index.qualified_owner(q).unwrap().is_none());
    for position in 0..2 {
        let owner = index
            .facts()
            .associated_owner(association, position)
            .unwrap();
        assert_eq!(
            index
                .owner_associations(owner)
                .unwrap()
                .qualifications()
                .count(),
            0
        );
    }
}

#[cfg(feature = "cpp")]
#[test]
fn qualification_order_and_prepared_owner_memberships_follow_document_order() {
    fn send_sync<T: Send + Sync>() {}
    let source = "/**@fn f*/\n#if FLAG\nvoid f();\n#endif\n/**@fn f*/";
    let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
    assert!(!parser.root_node().has_error());
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let documents: Vec<_> = index.documents().collect();
    let qualifications: Vec<_> = index.qualifications().collect();
    assert_eq!(documents.len(), 2);
    assert_eq!(qualifications.len(), 2);
    let owner = index.qualified_owner(qualifications[0]).unwrap().unwrap();
    for (qualification, document) in qualifications.iter().zip(&documents) {
        assert_eq!(index.qualified_document(*qualification).unwrap(), *document);
        assert_eq!(
            index
                .document_qualifications(*document)
                .unwrap()
                .collect::<Vec<_>>(),
            [*qualification]
        );
        assert_eq!(index.qualified_owner(*qualification).unwrap(), Some(owner));
    }
    assert_eq!(
        index
            .owner_associations(owner)
            .unwrap()
            .qualifications()
            .collect::<Vec<_>>(),
        qualifications
    );
    send_sync::<Qualification<'static>>();
    send_sync::<DocumentationIndex<'static>>();
}

#[cfg(feature = "cpp")]
#[test]
fn overlapping_single_candidate_keeps_both_reasons_and_complex_assessment() {
    let source = "#if FLAG\nvoid f(int x = );\n#endif\n/**@fn f*/";
    let parser = AnyParser::parse(LANG::Cpp, source.as_bytes().to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().resolution,
        Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
    );
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let query = index.owner_associations(owner).unwrap();
    assert_eq!(query.associations().len(), 1);
    assert!(query.assessments().next().is_some());
    assert_eq!(
        query
            .qualifications()
            .map(|q| q.get().reason)
            .collect::<Vec<_>>(),
        [
            UnresolvedReason::Conditional,
            UnresolvedReason::GrammarBoundary
        ]
    );
    let mut moved = Vec::new();
    moved.push(index);
    let index = moved.pop().unwrap();
    assert_eq!(index.qualifications().len(), 2);
}
