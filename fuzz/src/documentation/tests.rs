use super::*;
use big_code_analysis_ast::parser::ParseCompletion;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

fn number(value: &Value) -> usize {
    usize::try_from(value.as_u64().unwrap()).unwrap()
}
fn range(value: &Value) -> std::ops::Range<usize> {
    number(&value[0])..number(&value[1])
}

fn nodes(tree: &tree_sitter::Tree) -> Vec<tree_sitter::Node<'_>> {
    let mut nodes = Vec::new();
    let mut cursor = tree.walk();
    loop {
        nodes.push(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return nodes;
            }
        }
    }
}

fn text<'i, 's>(index: &'i DocumentationIndex<'s>) -> CompleteText<'i, 's> {
    index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap()
}

fn positive(index: &DocumentationIndex<'_>, row: &Value) {
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let document = index.documents().next().unwrap();
    let fragment = index.facts().document_fragment(document, 0).unwrap();
    assert_eq!(document.get().fragment_count(), 1);
    assert_eq!(fragment.get().source().pieces().len(), 1);
    assert_eq!(
        fragment.get().source().pieces()[0].range(),
        range(&row["fragments"][0])
    );
    let complete = text(index);
    assert_eq!(complete.text(), row["text"].as_str().unwrap());
    let association = index.associations().next().unwrap();
    assert_eq!(association.get().resolution, Resolution::Resolved);
    assert_eq!(association.get().owner_count(), 1);
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("positive must have a real declaration")
    };
    assert_eq!(node.node().byte_range(), range(&row["owner"]));
    assert_eq!(
        node.node().kind(),
        row["selected_owner_kind"].as_str().unwrap()
    );
    assert_eq!(
        index
            .node_associations(node.node())
            .unwrap()
            .associations()
            .len(),
        1
    );
    let portions: Vec<_> = complete
        .origins(0..complete.text().len())
        .unwrap()
        .collect();
    assert_eq!(portions.len(), 1);
    let OriginView::Copied(piece) = &portions[0].origin else {
        panic!("positive needs copied source mapping")
    };
    assert_eq!(piece.bytes(), complete.text().as_bytes());
}

#[test]
#[allow(clippy::too_many_lines)] // Registration checks bytes, parse shape, and executed semantic oracles together.
fn registry_executes_every_dispatch_and_edge_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let registry: Vec<Value> =
        serde_json::from_str(include_str!("../../documentation-seeds.json")).unwrap();
    let inventory: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/inventory.json"
    ))
    .unwrap();
    let mut names = BTreeSet::new();
    let mut dispatch = BTreeSet::new();
    let mut counts = [0; 3];
    for seed in &registry {
        let name = seed["name"].as_str().unwrap();
        assert!(names.insert(name.to_owned()), "duplicate seed {name}");
        let bytes = std::fs::read(root.join("corpus/documentation").join(name)).unwrap();
        let header: Vec<_> = seed["header"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| u8::try_from(number(value)).unwrap())
            .collect();
        assert_eq!(&bytes[..3], header);
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes[3..])),
            seed["sha256"].as_str().unwrap()
        );
        if let Some(fixture) = seed["fixture"].as_str() {
            assert_eq!(
                &bytes[3..],
                std::fs::read(
                    root.join("../tests/fixtures/documentation/sources")
                        .join(fixture)
                )
                .unwrap()
            );
        }
        let mut inspected = false;
        let result = exercise_with(&bytes, |index, tree| {
            inspected = true;
            assert_eq!(
                tree.root_node().has_error(),
                seed["has_error"].as_bool().unwrap(),
                "{name}"
            );
            if let Some(language) = seed["inventory"].as_str() {
                let row = &inventory[language];
                assert_eq!(
                    format!("{:?}", Input::decode(&bytes).unwrap().language).to_lowercase(),
                    language
                );
                assert!(dispatch.insert(header[0]));
                if let Some(kind) = row["selected_owner_kind"].as_str() {
                    assert!(
                        nodes(tree).iter().any(|node| node.kind() == kind),
                        "{name}: expected parsed declaration"
                    );
                }
                match seed["class"].as_str().unwrap() {
                    "positive" => {
                        positive(index.unwrap(), row);
                        counts[0] += 1;
                    }
                    "deferred" => {
                        assert!(index.is_none());
                        counts[1] += 1;
                    }
                    "helper" => {
                        assert_eq!(index.unwrap().capability(), Capability::NoDeclarationModel);
                        counts[2] += 1;
                    }
                    _ => panic!("unknown dispatch assertion"),
                }
            } else {
                edge(seed["class"].as_str().unwrap(), index, tree);
            }
            Ok(())
        });
        assert!(result.is_ok(), "{name}: {result:?}");
        assert!(inspected, "{name}: registry seed skipped execution");
        match seed["class"].as_str().unwrap() {
            "deferred" | "deferred-profile" => assert!(matches!(
                result,
                Ok(Outcome::Rejected(
                    CollectionError::AdapterUnavailable { .. }
                ))
            )),
            "incompatible-profile" => assert!(matches!(
                result,
                Ok(Outcome::Rejected(
                    CollectionError::UnsupportedProfile { .. }
                ))
            )),
            "helper" => assert_eq!(result, Ok(Outcome::Helper)),
            _ => assert_eq!(result, Ok(Outcome::Checked)),
        }
    }
    assert_eq!(counts, [17, 6, 2]);
    assert_eq!(dispatch, (0..25).collect());
    let actual: BTreeSet<_> = std::fs::read_dir(root.join("corpus/documentation"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(actual, names);
}

#[allow(clippy::too_many_lines)] // Independent expected outcomes for the finite registered edge families.
fn edge(class: &str, index: Option<&DocumentationIndex<'_>>, tree: &tree_sitter::Tree) {
    if matches!(class, "incompatible-profile" | "deferred-profile") {
        assert!(index.is_none());
        return;
    }
    let index = index.unwrap();
    match class {
        "eof" => {
            assert!(!index.source().ends_with(b"\n"));
            assert_eq!(index.documents().len(), 0);
        }
        "crlf" => {
            assert!(index.source().windows(2).any(|bytes| bytes == b"\r\n"));
            assert_eq!(text(index).text(), "kept");
        }
        "pending-content" => {
            assert_eq!(index.documents().len(), 1);
            assert!(matches!(
                index.documents().next().unwrap().get().content(),
                Content::PendingAdapter
            ));
            assert!(
                index
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Composition)
            );
        }
        "join" => {
            assert_eq!(text(index).text(), "a\nb");
            assert_eq!(text(index).mappings().len(), 3);
            assert!(matches!(
                text(index).mappings()[1].origin,
                Origin::Synthetic
            ));
        }
        "shared" => {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.associations().next().unwrap().get().owner_count(), 2);
            assert_eq!(text(index).text(), "audit");
        }
        "gathered" => {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 2);
            assert_eq!(text(index).text(), "a\nb");
        }
        "candidate-qualification" | "reference-qualification" => {
            let candidate = class == "candidate-qualification";
            let expected = if candidate {
                vec![
                    UnresolvedReason::Conditional,
                    UnresolvedReason::GrammarBoundary,
                ]
            } else {
                vec![UnresolvedReason::Conditional]
            };
            assert_eq!(
                index
                    .qualifications()
                    .map(|q| q.get().reason)
                    .collect::<Vec<_>>(),
                expected
            );
            for qualification in index.qualifications() {
                assert_eq!(
                    qualification.get().subject,
                    if candidate {
                        QualificationSubject::Candidate
                    } else {
                        QualificationSubject::Reference
                    }
                );
                assert_eq!(
                    index.qualified_owner(qualification).unwrap().is_some(),
                    candidate
                );
            }
        }
        "missing-name" => assert_eq!(
            index.associations().next().unwrap().get().resolution,
            Resolution::Unresolved(UnresolvedReason::MissingName)
        ),
        "pending-name" => {
            assert_eq!(index.associations().len(), 0);
            assert_eq!(index.documents().len(), 1);
            assert!(index.assessments().next().is_some());
        }
        "pending-owner" => {
            let node = nodes(tree)
                .into_iter()
                .find(|node| node.kind() == "class_parameter")
                .unwrap();
            assert!(index.node_projection(node).unwrap().pending());
            assert!(index.node_associations(node).unwrap().projection_pending());
            assert_eq!(text(index).text(), "audit");
        }
        "known-and-pending" => {
            assert_eq!(index.documents().len(), 2);
            let node = nodes(tree)
                .into_iter()
                .find(|node| node.kind() == "function_declaration")
                .unwrap();
            let query = index.node_associations(node).unwrap();
            assert_eq!(query.associations().len(), 1);
            assert!(
                query
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Owner)
            );
            assert_eq!(text(index).text(), "first");
            assert_eq!(
                index
                    .complete_text(index.documents().last().unwrap())
                    .unwrap()
                    .unwrap()
                    .text(),
                "audit"
            );
        }
        "invalid-encoding" => assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(UnavailableReason::InvalidEncoding)
        )),
        "partial" => assert!(tree.root_node().has_error()),
        "scalar-pieces" => {
            assert_eq!(text(index).text(), "é");
            assert!(
                text(index)
                    .mappings()
                    .iter()
                    .any(|m| !text(index).text().is_char_boundary(m.output.range().end))
            );
        }
        "decoded-pieces" => {
            let complete = text(index);
            assert_eq!(complete.text(), "a\nb");
            let mapping = complete
                .mappings()
                .iter()
                .find(|mapping| matches!(mapping.origin, Origin::Decoded(_)))
                .unwrap();
            let Origin::Decoded(pieces) = &mapping.origin else {
                unreachable!()
            };
            assert_eq!(pieces.pieces().len(), 2);
            assert_eq!(
                pieces
                    .pieces()
                    .iter()
                    .flat_map(|piece| piece.bytes().iter().copied())
                    .collect::<Vec<_>>(),
                b"\r\n"
            );
            let portion = complete.origins(1..2).unwrap().next().unwrap();
            assert_eq!(portion.origin, OriginView::Decoded(pieces.pieces()));
        }
        "source-gap" => {
            assert!(
                index
                    .facts()
                    .fragments()
                    .any(|fragment| fragment.get().source().has_excluded_gap())
            );
            assert!(index.documents().any(|document| matches!(
                document.get().content(),
                Content::Unavailable(UnavailableReason::ExcludedGap)
            )));
        }
        "empty" | "evidence" => {
            assert_eq!(text(index).text(), "");
            assert_eq!(index.evidence().len(), usize::from(class == "evidence"));
        }
        "directive-off" | "directive-on" => {
            assert_eq!(index.documents().len(), 0);
            assert_eq!(index.evidence().len(), usize::from(class == "directive-on"));
            for evidence in index.evidence() {
                assert_eq!(evidence.get().disposition, CandidateDisposition::Directive);
            }
        }
        "cancelled-scope" | "active-scope" => assert_eq!(text(index).text(), "audit"),
        "neighbor" => {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(text(index).text(), "neighbor");
            let association = index.associations().next().unwrap();
            assert_eq!(association.get().resolution, Resolution::Resolved);
            let Owner::Node(owner) = index
                .facts()
                .associated_owner(association, 0)
                .unwrap()
                .get()
            else {
                panic!("neighbor owner")
            };
            assert_eq!(
                &index.source()[owner.node().byte_range()],
                b"fn neighbor() {}"
            );
        }
        _ => panic!("unknown edge assertion {class}"),
    }
}

#[test]
fn header_roster_skips_and_resource_boundaries_are_explicit() {
    let actual: BTreeSet<_> = header::LANGUAGES.iter().map(|lang| lang.name()).collect();
    let expected: BTreeSet<_> = LANG::into_enum_iter().map(|lang| lang.name()).collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), header::LANGUAGES.len());
    for (bytes, skip) in [
        (&[0, 0][..], Skip::ShortHeader),
        (&[25, 0, 0], Skip::UnknownLanguage),
        (&[255, 0, 0], Skip::UnknownLanguage),
        (&[0, 16, 0], Skip::ReservedBits),
        (&[0, 0, 128], Skip::ReservedBits),
        (&[0, 0, 31], Skip::UnknownProfile),
    ] {
        assert_eq!(exercise(bytes), Ok(Outcome::Skipped(skip)));
    }
    let mut bytes = vec![0, 0, 0];
    bytes.extend(vec![b' '; MAX_SOURCE]);
    assert_eq!(exercise(&bytes), Ok(Outcome::Checked));
    bytes.push(b' ');
    assert_eq!(exercise(&bytes), Ok(Outcome::Skipped(Skip::Oversize)));
    let ast = Ast::parse(big_code_analysis::Source::from_bytes(
        LANG::Rust,
        bytes[3..].to_vec(),
    ))
    .unwrap();
    assert_eq!(check_ast(&ast), Ok(Outcome::Skipped(Skip::Oversize)));
    assert_eq!(ast.source().len(), MAX_SOURCE + 1);
    assert!(included_ranges(&vec![b'a'; MAX_STRUCTURE]).is_some());
    assert!(included_ranges(&vec![b'a'; MAX_STRUCTURE + 1]).is_none());
    assert_eq!(
        exercise(&[0, 4, 0, 0]),
        Ok(Outcome::Skipped(Skip::IncludedRangeLimit))
    );
}

#[test]
fn typed_parse_cancellation_retains_actual_abort_after_scope_restoration() {
    let flag = ParseCancel::new();
    flag.cancel();
    let parser = flag
        .run(|| AnyParser::parse(LANG::Rust, b"fn f() {}\n".repeat(10_000), None, None).unwrap());
    assert_eq!(parser.parse_completion(), ParseCompletion::Cancelled);
    assert!(matches!(
        parser.documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default()
        ),
        Err(CollectionError::Cancelled)
    ));
    let next = AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
    assert_eq!(next.parse_completion(), ParseCompletion::Complete);
    assert_eq!(
        text(
            &next
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default()
                )
                .unwrap()
        )
        .text(),
        "audit"
    );
}

#[test]
fn registry_oracles_reject_actual_wrong_payloads_and_foreign_nodes() {
    let inventory: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/documentation/inventory.json"
    ))
    .unwrap();
    let bytes = b"\0\x01\0///other\nfn f() {}";
    let result = std::panic::catch_unwind(|| {
        exercise_with(bytes, |index, _| {
            positive(index.unwrap(), &inventory["rust"]);
            Ok(())
        })
    });
    assert!(
        result.is_err(),
        "same-shaped wrong text must fail the real positive oracle"
    );
    let bytes = b"\0\x01\0#[doc = \"audit\"]\nfn f() {}";
    let result = std::panic::catch_unwind(|| {
        exercise_with(bytes, |index, tree| {
            edge("active-scope", index, tree);
            Ok(())
        })
    });
    assert!(
        result.is_err(),
        "pending text must not pass a complete-text assertion"
    );
    let first = AnyParser::parse(LANG::Rust, b"fn f() {}".to_vec(), None, None).unwrap();
    let foreign = AnyParser::parse(LANG::Rust, b"fn f() {}".to_vec(), None, None).unwrap();
    let index = first
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        index.node_projection(foreign.ts_tree().root_node()),
        Err(OwnerError::Source(SourceError::ForeignTree))
    ));
}

#[test]
fn ordinary_fanout_borrows_the_preprocessed_ast_buffer() {
    use big_code_analysis::{PreprocResults, Source, preprocess};
    use std::sync::Arc;
    let path = Path::new("fuzz_input.cpp");
    let bytes = b"#define API\n/**audit*/\nAPI void f() {}";
    let mut results = PreprocResults::default();
    preprocess(bytes.to_vec(), path, &mut results);
    let ast = Ast::parse(
        Source::from_bytes(LANG::Cpp, bytes.to_vec())
            .with_preproc_path(Some(path))
            .with_preproc(Some(Arc::new(results))),
    )
    .unwrap();
    assert_ne!(ast.source(), bytes);
    assert_eq!(check_ast(&ast), Ok(Outcome::Checked));
    let index = collect(
        ast.language(),
        ast.as_tree_sitter(),
        ast.source(),
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    assert_eq!(index.source(), ast.source());
    assert_eq!(text(&index).text(), "audit");
}
