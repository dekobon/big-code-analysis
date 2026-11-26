use super::*;
use big_code_analysis_ast::documentation::*;
use big_code_analysis_ast::parser::ParseCompletion;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;
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
        if let Some(expected) = seed["raw_sha256"].as_str() {
            let actual = Sha256::digest(&bytes)
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    write!(hex, "{byte:02x}").unwrap();
                    hex
                });
            assert_eq!(actual, expected);
        }
        assert_eq!(
            Sha256::digest(&bytes[3..])
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    write!(hex, "{byte:02x}").unwrap();
                    hex
                }),
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
                "{name}: {}",
                tree.root_node().to_sexp()
            );
            if let Some(language) = seed["inventory"].as_str() {
                let row = &inventory[language];
                if matches!(language, "tcl" | "irules") {
                    assert!(
                        nodes(tree)
                            .iter()
                            .any(|node| node.is_missing() && node.kind() == "\n")
                    );
                }
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
    assert_eq!(registry.len(), 67);
    assert_eq!(
        registry
            .iter()
            .filter(|seed| seed["header"][0] == 0)
            .count(),
        26
    );
}

#[allow(clippy::too_many_lines)] // Exact included-tree, raw-fragment, owner and token oracle for the two registered seeds.
fn rust_included_literal_seed(
    class: &str,
    index: &DocumentationIndex<'_>,
    tree: &tree_sitter::Tree,
) {
    let gap = class == "rust-literal-value-gap";
    let source: &[u8] = if gap {
        b"#[doc = \"a\0b\"] fn f() {}"
    } else {
        b"#[doc = r\"a\r\nb\"] fn f() {}"
    };
    assert_eq!(index.source(), source);
    let mut point = tree_sitter::Point::new(0, 0);
    let mut expected_ranges = Vec::new();
    for (at, byte) in source.iter().enumerate() {
        let start_point = point;
        if *byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
        if *byte != 0 {
            expected_ranges.push(tree_sitter::Range {
                start_byte: at,
                end_byte: at + 1,
                start_point,
                end_point: point,
            });
        }
    }
    assert_eq!(tree.included_ranges(), expected_ranges);
    let canonical = nodes(tree);
    assert!(
        canonical
            .iter()
            .all(|node| !node.has_error() && !node.is_missing())
    );
    let function = canonical
        .iter()
        .find(|node| node.kind() == "function_item")
        .unwrap();
    let attribute = canonical
        .iter()
        .find(|node| node.kind() == "attribute_item")
        .unwrap();
    let end = if gap { 14 } else { 16 };
    assert_eq!(attribute.byte_range(), 0..end);
    assert_eq!(function.byte_range(), end + 1..end + 10);
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let document = index.documents().next().unwrap();
    assert_eq!(document.get().fragment_count(), 1);
    assert_eq!(document.get().role(), DocumentRole::Primary);
    assert_eq!(document.get().composition(), Composition::AttributeSequence);
    let fragment = index.facts().document_fragment(document, 0).unwrap();
    assert_eq!(fragment.get().form(), FragmentForm::Attribute);
    fragment.get().node().unwrap().validate(*attribute).unwrap();
    assert_eq!(fragment.get().source().has_excluded_gap(), gap);
    let spans: Vec<_> = (0..end)
        .filter(|at| !gap || *at != 10)
        .map(|at| at..at + 1)
        .collect();
    assert_eq!(
        fragment
            .get()
            .source()
            .pieces()
            .iter()
            .map(SourcePiece::range)
            .collect::<Vec<_>>(),
        spans
    );
    for piece in fragment.get().source().pieces() {
        assert_eq!(piece.bytes(), &source[piece.range()]);
    }
    let association = index.associations().next().unwrap();
    assert_eq!(
        index.facts().associated_document(association).unwrap(),
        document
    );
    assert_eq!(association.get().resolution, Resolution::Resolved);
    assert_eq!(association.get().publication, Publication::Unknown);
    association
        .get()
        .binding
        .unwrap()
        .validate(*function)
        .unwrap();
    association
        .get()
        .lexical_scope
        .validate(tree.root_node())
        .unwrap();
    assert_eq!(association.get().owner_count(), 1);
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("function owner")
    };
    node.validate(*function).unwrap();
    assert_eq!(index.facts().owners().len(), 2);
    let file = index.facts().owners().next().unwrap();
    let Owner::File(node) = file.get() else {
        panic!("file owner")
    };
    node.validate(tree.root_node()).unwrap();
    let mut publications = index
        .assessments()
        .filter(|assessment| assessment.get().axis == AssessmentAxis::Publication);
    let publication = publications.next().unwrap();
    assert!(publications.next().is_none());
    publication
        .get()
        .node
        .unwrap()
        .validate(*attribute)
        .unwrap();
    assert_eq!(
        index
            .node_associations(*function)
            .unwrap()
            .associations()
            .collect::<Vec<_>>(),
        [association]
    );
    assert_eq!(
        index
            .owner_associations(owner)
            .unwrap()
            .associations()
            .collect::<Vec<_>>(),
        [association]
    );
    if gap {
        let value = canonical
            .iter()
            .find(|node| node.kind() == "string_literal")
            .unwrap();
        assert_eq!(value.byte_range(), 8..13);
        assert!(
            canonical
                .iter()
                .any(|node| node.kind() == "identifier" && node.byte_range() == (2..5))
        );
        assert!(matches!(
            document.get().content(),
            Content::Unavailable(UnavailableReason::ExcludedGap)
        ));
        assert!(index.complete_text(document).unwrap().is_none());
    } else {
        let complete = text(index);
        assert_eq!(complete.text(), "a\nb");
        let portions: Vec<_> = complete.origins(0..3).unwrap().collect();
        assert_eq!(complete.mappings().len(), 3);
        assert_eq!(portions.len(), 3);
        for (at, portion) in portions.iter().enumerate() {
            assert_eq!(portion.output, at..at + 1);
        }
        for (at, span) in [(0, 10..11), (2, 13..14)] {
            let OriginView::Copied(piece) = &portions[at].origin else {
                panic!("copied")
            };
            assert_eq!(piece.range(), span.clone());
            assert_eq!(piece.bytes(), &source[span]);
        }
        let OriginView::Decoded(pieces) = &portions[1].origin else {
            panic!("physical CRLF")
        };
        assert_eq!(
            pieces.iter().map(SourcePiece::range).collect::<Vec<_>>(),
            [11..12, 12..13]
        );
        assert_eq!(
            pieces.iter().map(SourcePiece::bytes).collect::<Vec<_>>(),
            [b"\r", b"\n"]
        );
    }
}

#[test]
fn unit1_additions_preserve_every_previous_tagged_and_raw_rust_seed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (directory, count, expected) in [
        (
            "documentation",
            65,
            "e0b4b8f37651094400115a6e7a3a7b0fe36d6f772c81c9394897d19995ede301",
        ),
        (
            "parse_rust",
            12,
            "9467333cd232ce99f65f924cc4a9e6c66b859d49ecc2f208e03dffe1f0212fba",
        ),
    ] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join("corpus").join(directory))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                !matches!(
                    path.file_name().unwrap().to_str().unwrap(),
                    "rust-literal-value-gap" | "rust-literal-raw-crlf-pieces"
                )
            })
            .collect();
        paths.sort();
        assert_eq!(paths.len(), count);
        let mut hash = Sha256::new();
        for path in paths {
            hash.update(path.file_name().unwrap().to_str().unwrap().as_bytes());
            hash.update([0]);
            hash.update(std::fs::read(path).unwrap());
            hash.update([0]);
        }
        // Frozen from clean writer starting HEAD 0fb38da5, before the two additions.
        assert_eq!(
            hash.finalize().iter().fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").unwrap();
                hex
            }),
            expected
        );
    }
}

#[allow(clippy::single_range_in_vec_init)]
#[allow(clippy::too_many_lines)] // Independent text/origin tables for registered Rust literal and cleanup seeds.
fn rust_literal_seed(class: &str, index: &DocumentationIndex<'_>) {
    let (expected, fragments, origins) = match class {
        "rust-outer-binding" => (
            "outer\ninner",
            2,
            vec![
                (0..5, false, vec![9..14]),
                (5..6, false, vec![]),
                (6..11, false, vec![30..35]),
            ],
        ),
        "rust-second-lines-cr" => (
            "a\nb",
            1,
            vec![
                (0..1, false, vec![11..12]),
                (1..2, true, vec![16..18]),
                (2..3, false, vec![18..19]),
            ],
        ),
        "rust-second-lines-terminal" => ("a", 1, vec![(0..1, false, vec![11..12])]),
        "rust-second-lines-triple-cr" => (
            "a\r\nb",
            1,
            vec![
                (0..1, false, vec![11..12]),
                (1..2, true, vec![12..14]),
                (2..3, true, vec![18..20]),
                (3..4, false, vec![20..21]),
            ],
        ),
        "rust-unicode-first-star" => (
            "\u{2003}* a\n * b\nz",
            2,
            vec![
                (0..6, false, vec![3..9]),
                (6..7, false, vec![9..10]),
                (7..11, false, vec![10..14]),
                (11..12, false, vec![]),
                (12..13, false, vec![26..27]),
            ],
        ),
        "rust-literal-mixed" => (
            "first\nsecond",
            2,
            vec![
                (0..5, false, vec![4..9]),
                (5..6, false, vec![]),
                (6..12, false, vec![20..26]),
            ],
        ),
        "rust-literal-empty" => ("", 1, vec![]),
        "rust-literal-raw" => ("a\"#b", 1, vec![(0..4, false, vec![13..17])]),
        "rust-literal-scalar" => (
            "😀",
            1,
            vec![(0..4, true, (9..18).map(|at| at..at + 1).collect())],
        ),
        "rust-literal-continuation" => (
            "ab",
            1,
            vec![(0..1, false, vec![9..10]), (1..2, false, vec![16..17])],
        ),
        "rust-literal-margin" => (
            "    hello\nanother",
            2,
            vec![
                (0..9, false, vec![4..13]),
                (9..10, false, vec![]),
                (10..17, false, vec![23..30]),
            ],
        ),
        _ => panic!("registered literal class"),
    };
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let document = index.documents().next().unwrap();
    assert_eq!(document.get().fragment_count(), fragments);
    assert_eq!(text(index).text(), expected);
    let actual: Vec<_> = text(index)
        .mappings()
        .iter()
        .map(|mapping| {
            let (decoded, pieces) = match &mapping.origin {
                Origin::Copied(pieces) => (false, pieces.pieces()),
                Origin::Decoded(pieces) => (true, pieces.pieces()),
                Origin::Synthetic => (false, &[][..]),
                Origin::Expanded(_) => panic!("Rust does not expand indentation"),
                _ => panic!("unexpected Rust origin"),
            };
            (
                mapping.output.range(),
                decoded,
                pieces.iter().map(SourcePiece::range).collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(actual, origins);
    let association = index.associations().next().unwrap();
    assert_eq!(association.get().resolution, Resolution::Resolved);
    assert_eq!(association.get().publication, Publication::Unknown);
    if class == "rust-outer-binding" {
        assert_rust_inner_binding(index, 17..37);
    }
    assert!(!index.assessments().any(|a| matches!(
        a.get().axis,
        AssessmentAxis::Occurrence | AssessmentAxis::Composition
    )));
}

fn assert_rust_inner_binding(index: &DocumentationIndex<'_>, span: std::ops::Range<usize>) {
    assert_eq!(index.associations().len(), 1);
    let association = index.associations().next().unwrap();
    assert_eq!(
        association.get().binding.map(|n| n.node().byte_range()),
        Some(span.clone())
    );
    let document = index.documents().next().unwrap();
    assert_eq!(
        index.facts().associated_document(association).unwrap(),
        document
    );
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Node(node) = owner.get() else {
        panic!("function owner")
    };
    assert_eq!(node.node().kind(), "function_item");
    assert_eq!(node.node().byte_range(), span);
}

#[allow(clippy::too_many_lines)] // Independent expected outcomes for the finite registered edge families.
fn edge(class: &str, index: Option<&DocumentationIndex<'_>>, tree: &tree_sitter::Tree) {
    if matches!(class, "incompatible-profile" | "deferred-profile") {
        assert!(index.is_none());
        return;
    }
    let index = index.unwrap();
    match class {
        "rust-literal-value-gap" | "rust-literal-raw-crlf-pieces" => {
            rust_included_literal_seed(class, index, tree);
        }
        "eof" => {
            assert!(!index.source().ends_with(b"\n"));
            assert_eq!(index.documents().len(), 0);
        }
        "crlf" => {
            assert!(index.source().windows(2).any(|bytes| bytes == b"\r\n"));
            assert_eq!(text(index).text(), "kept");
        }
        "pending-content" | "rust-pending-inner-binding" => {
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
            if class == "rust-pending-inner-binding" {
                assert_rust_inner_binding(index, 15..35);
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
                    index.documents().next()
                );
            }
        }
        "rust-literal-mixed"
        | "rust-literal-empty"
        | "rust-literal-raw"
        | "rust-literal-scalar"
        | "rust-literal-continuation"
        | "rust-literal-margin"
        | "rust-outer-binding"
        | "rust-second-lines-cr"
        | "rust-second-lines-terminal"
        | "rust-second-lines-triple-cr"
        | "rust-unicode-first-star" => {
            rust_literal_seed(class, index);
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
            let association = index.associations().next().unwrap();
            assert_eq!(association.get().resolution, Resolution::Resolved);
            assert_eq!(association.get().owner_count(), 2);
            for position in 0..2 {
                let owner = index
                    .facts()
                    .associated_owner(association, position)
                    .unwrap();
                let Owner::Slot(slot) = owner.get() else {
                    panic!("shared Field slot")
                };
                assert_eq!(slot.role, SlotRole::Field);
                assert_eq!(slot.ordinal, position);
            }
            assert_eq!(text(index).text(), "audit");
        }
        "gathered" => {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.documents().next().unwrap().get().fragment_count(), 2);
            assert_eq!(text(index).text(), "a\nb");
        }
        "candidate-qualification" | "reference-qualification" => {
            let candidate = class == "candidate-qualification";
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.associations().len(), 1);
            let association = index.associations().next().unwrap();
            assert_eq!(
                association.get().resolution,
                Resolution::Unresolved(if candidate {
                    UnresolvedReason::Ambiguous
                } else {
                    UnresolvedReason::Conditional
                }),
                "seed association resolution"
            );
            assert_eq!(
                association.get().owner_count(),
                if candidate { 2 } else { 1 }
            );
            let document = index.facts().associated_document(association).unwrap();
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
                assert_eq!(index.qualified_document(qualification).unwrap(), document);
                let node = qualification.get().node.node();
                if candidate {
                    let owner = index.facts().associated_owner(association, 0).unwrap();
                    assert_eq!(
                        index.qualified_owner(qualification).unwrap(),
                        Some(owner),
                        "candidate owner identity"
                    );
                    assert_eq!(
                        &index.source()[node.byte_range()],
                        b"void f(int x = );",
                        "candidate source evidence"
                    );
                } else {
                    assert_eq!(
                        &index.source()[node.byte_range()],
                        b"/**@fn f*/",
                        "reference source evidence"
                    );
                }
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
            let local = index.documents().last().unwrap();
            assert!(
                query
                    .assessments()
                    .any(|a| a.get().axis == AssessmentAxis::Owner
                        && index.assessed_document(a).unwrap() == Some(local))
            );
            assert_eq!(text(index).text(), "first");
            assert_eq!(index.complete_text(local).unwrap().unwrap().text(), "audit");
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
            assert_eq!(index.documents().len(), 0);
            assert_eq!(index.assessments().len(), 1);
            let assessment = index.assessments().next().unwrap();
            assert_eq!(assessment.get().axis, AssessmentAxis::Selection);
            assert_eq!(
                assessment.get().reason,
                "Excluded bytes prevent complete native-carrier assessment"
            );
            let node = assessment.get().node.unwrap().node();
            assert_eq!(node.byte_range(), 0..8);
            let pieces = SourceContext::new(tree, index.source())
                .pieces(node.byte_range())
                .unwrap();
            assert!(pieces.has_excluded_gap());
            assert_eq!(
                pieces
                    .pieces()
                    .iter()
                    .map(SourcePiece::range)
                    .collect::<Vec<_>>(),
                vec![0..1, 1..2, 2..3, 3..4, 5..6, 6..7, 7..8]
            );
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
    let actual: BTreeSet<_> = header::LANGUAGES.iter().map(LANG::name).collect();
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
    assert_eq!(
        exercise(&[0, 4, 0]),
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
    let bytes = b"\0\x01\0#[doc(hidden)]\nfn f() {}";
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

#[test]
fn qualification_oracle_rejects_same_reasons_on_the_wrong_candidate() {
    let bytes = b"\x0c\x01\0void f(void);\n#if FLAG\nvoid f(int x = );\n#endif\n/**@fn f*/";
    let result = exercise_with(bytes, |index, tree| {
        let index = index.unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().resolution,
            Resolution::Unresolved(UnresolvedReason::Ambiguous),
            "wrong-candidate setup resolution"
        );
        assert_eq!(association.get().owner_count(), 2);
        let document = index.facts().associated_document(association).unwrap();
        let owner = index.facts().associated_owner(association, 1).unwrap();
        assert_eq!(
            index
                .qualifications()
                .map(|q| q.get().reason)
                .collect::<Vec<_>>(),
            [
                UnresolvedReason::Conditional,
                UnresolvedReason::GrammarBoundary
            ]
        );
        for qualification in index.qualifications() {
            assert_eq!(qualification.get().subject, QualificationSubject::Candidate);
            assert_eq!(index.qualified_document(qualification).unwrap(), document);
            assert_eq!(index.qualified_owner(qualification).unwrap(), Some(owner));
            assert_eq!(
                &index.source()[qualification.get().node.node().byte_range()],
                b"void f(int x = );"
            );
        }
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            edge("candidate-qualification", Some(index), tree);
        }));
        assert!(
            rejected.is_err(),
            "qualification identity oracle accepted the other candidate"
        );
        let panic = rejected.unwrap_err();
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert_eq!(
            message.lines().next(),
            Some("assertion `left == right` failed: candidate owner identity"),
            "wrong-candidate rejection site"
        );
        Ok(())
    });
    assert_eq!(result, Ok(Outcome::Checked));
}

mod remediation;
