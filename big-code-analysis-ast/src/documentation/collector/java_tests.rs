#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "java")]
use crate::documentation::*;

#[cfg(feature = "java")]
use crate::documentation::budget::WorkBudget;

#[cfg(feature = "java")]
use crate::{AnyParser, LANG};

#[cfg(feature = "java")]
use std::ops::Range;

#[cfg(feature = "java")]
use std::cell::Cell;

#[cfg(feature = "java")]
const PENDING_DOCUMENT: &str = "Selected local or wrapper form awaits owner projection";

#[cfg(feature = "java")]
const PENDING_OWNER: &str = "Recognized declaration requires a semantic projection";

#[cfg(feature = "java")]
fn spelled(node: tree_sitter::Node<'_>) -> String {
    format!("{} {}..{}", node.kind(), node.start_byte(), node.end_byte())
}

#[cfg(feature = "java")]
fn owner_text<'s>(index: &DocumentationIndex<'s>, owner: FactRef<'_, Owner<'s>>) -> String {
    match owner.get() {
        Owner::Node(node) => format!("Node({})", spelled(node.node())),
        Owner::File(node) => format!("File({})", spelled(node.node())),
        Owner::Slot(slot) => {
            let pieces: Vec<_> = slot
                .source
                .pieces()
                .iter()
                .map(|piece| format!("{}..{}", piece.range().start, piece.range().end))
                .collect();
            assert!(!slot.source.has_excluded_gap());
            assert_eq!(
                index
                    .owner_associations(owner)
                    .unwrap()
                    .qualifications()
                    .len(),
                0
            );
            format!(
                "Slot({:?} #{} {} in {})",
                slot.role,
                slot.ordinal,
                pieces.join(","),
                owner_text(index, index.facts().slot_enclosing(owner).unwrap())
            )
        }
        _ => panic!("unexpected owner variant"),
    }
}

#[cfg(feature = "java")]
type Doc = (Vec<Range<usize>>, Option<String>);

#[cfg(feature = "java")]
fn documents(index: &DocumentationIndex<'_>) -> Vec<Doc> {
    index
        .documents()
        .map(|document| {
            assert_eq!(document.get().composition(), Composition::Single);
            assert_eq!(document.get().role(), DocumentRole::Primary);
            let fragments = (0..document.get().fragment_count())
                .map(|position| {
                    let fragment = index.facts().document_fragment(document, position).unwrap();
                    let pieces = fragment.get().source().pieces();
                    assert_eq!(pieces.len(), 1);
                    pieces[0].range()
                })
                .collect();
            let text = index
                .complete_text(document)
                .unwrap()
                .map(|text| text.text().to_owned());
            (fragments, text)
        })
        .collect()
}

#[cfg(feature = "java")]
type Edge = (usize, Resolution, Vec<String>, Option<String>, String);

#[cfg(feature = "java")]
fn associations(index: &DocumentationIndex<'_>) -> Vec<Edge> {
    index
        .associations()
        .map(|association| {
            assert_eq!(association.get().convention.name(), "Javadoc");
            let document = index.facts().associated_document(association).unwrap();
            let owners = (0..association.get().owner_count())
                .map(|position| {
                    owner_text(
                        index,
                        index
                            .facts()
                            .associated_owner(association, position)
                            .unwrap(),
                    )
                })
                .collect();
            (
                index
                    .documents()
                    .position(|known| known == document)
                    .unwrap(),
                association.get().resolution,
                owners,
                association.get().binding.map(|node| spelled(node.node())),
                spelled(association.get().lexical_scope.node()),
            )
        })
        .collect()
}

#[cfg(feature = "java")]
type Pending = (AssessmentAxis, &'static str, Option<String>, Option<usize>);

#[cfg(feature = "java")]
fn assessments<'i, 's: 'i>(
    index: &'i DocumentationIndex<'s>,
    found: impl Iterator<Item = FactRef<'i, Assessment<'s>>>,
) -> Vec<Pending> {
    found
        .map(|assessment| {
            (
                assessment.get().axis,
                assessment.get().reason,
                assessment.get().node.map(|node| spelled(node.node())),
                index
                    .assessed_document(assessment)
                    .unwrap()
                    .map(|document| {
                        index
                            .documents()
                            .position(|known| known == document)
                            .unwrap()
                    }),
            )
        })
        .collect()
}

#[cfg(feature = "java")]
fn evidence(index: &DocumentationIndex<'_>) -> Vec<(CandidateDisposition, Range<usize>)> {
    index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            let pieces = fragment.get().source().pieces();
            assert_eq!(pieces.len(), 1);
            (candidate.get().disposition, pieces[0].range())
        })
        .collect()
}

#[cfg(feature = "java")]
fn node<'t>(root: tree_sitter::Node<'t>, kind: &str, range: Range<usize>) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(current) = stack.pop() {
        if current.kind() == kind && current.byte_range() == range {
            found.push(current);
        }
        let mut cursor = current.walk();
        stack.extend(current.children(&mut cursor));
    }
    assert_eq!(found.len(), 1, "{kind} {range:?}");
    found[0]
}

#[cfg(feature = "java")]
fn both(
    source: &str,
    options: DocumentationOptions,
    check: impl Fn(tree_sitter::Node<'_>, &DocumentationIndex<'_>),
) {
    let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    for index in [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Java,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ] {
        assert_eq!(index.capability(), Capability::Partial);
        assert_eq!(index.qualifications().len(), 0);
        check(parser.ts_tree().root_node(), &index);
    }
}

#[cfg(feature = "java")]
fn restricted(source: &str, included: &[Range<usize>]) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let before = &source.as_bytes()[..offset];
        tree_sitter::Point::new(
            before.iter().filter(|byte| **byte == b'\n').count(),
            before
                .iter()
                .rev()
                .take_while(|byte| **byte != b'\n')
                .count(),
        )
    };
    let ranges: Vec<_> = included
        .iter()
        .map(|range| tree_sitter::Range {
            start_byte: range.start,
            end_byte: range.end,
            start_point: point(range.start),
            end_point: point(range.end),
        })
        .collect();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Java.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

#[cfg(feature = "java")]
fn unbound() -> DocumentationOptions {
    DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    }
}

#[cfg(feature = "java")]
fn slot(ordinal: usize, name: Range<usize>, declaration: &str) -> String {
    format!(
        "Slot(Field #{ordinal} {}..{} in Node({declaration}))",
        name.start, name.end
    )
}

#[cfg(feature = "java")]
#[test]
#[allow(clippy::too_many_lines)]
fn java_declarations_own_their_leading_documentation() {
    for (source, fragment, owner, scope, others) in [
        (
            "/**audit*/\ninterface I {}",
            0..10,
            "interface_declaration 11..25",
            "program 0..25",
            vec![],
        ),
        (
            "/**audit*/\nenum E { A }",
            0..10,
            "enum_declaration 11..23",
            "program 0..23",
            vec![("enum_constant", 20..21)],
        ),
        (
            "/**audit*/\nrecord R(int a) {}",
            0..10,
            "record_declaration 11..29",
            "program 0..29",
            vec![],
        ),
        (
            "/**audit*/\n@interface A {}",
            0..10,
            "annotation_type_declaration 11..26",
            "program 0..26",
            vec![],
        ),
        (
            "/**audit*/\nmodule m { }",
            0..10,
            "module_declaration 11..23",
            "program 0..23",
            vec![],
        ),
        (
            "class C { /**audit*/ void f() {} }",
            10..20,
            "method_declaration 21..32",
            "class_body 8..34",
            vec![("class_declaration", 0..34)],
        ),
        (
            "interface I { /**audit*/ void g(); }",
            14..24,
            "method_declaration 25..34",
            "interface_body 12..36",
            vec![("interface_declaration", 0..36)],
        ),
        (
            "interface I { /**audit*/ private void h() {} }",
            14..24,
            "method_declaration 25..44",
            "interface_body 12..46",
            vec![("interface_declaration", 0..46)],
        ),
        (
            "@interface A { /**audit*/ int v() default 1; }",
            15..25,
            "annotation_type_element_declaration 26..44",
            "annotation_type_body 13..46",
            vec![("annotation_type_declaration", 0..46)],
        ),
        (
            "class C { /**audit*/ C() {} }",
            10..20,
            "constructor_declaration 21..27",
            "class_body 8..29",
            vec![("class_declaration", 0..29)],
        ),
        (
            "record R(int x) { /**audit*/ R {} }",
            18..28,
            "compact_constructor_declaration 29..33",
            "class_body 16..35",
            vec![("record_declaration", 0..35)],
        ),
        (
            "enum E { /**audit*/ A }",
            9..19,
            "enum_constant 20..21",
            "enum_body 7..23",
            vec![("enum_declaration", 0..23)],
        ),
        (
            "interface I { /**audit*/ class N {} }",
            14..24,
            "class_declaration 25..35",
            "interface_body 12..37",
            vec![("interface_declaration", 0..37)],
        ),
        (
            "class C { void m() { /**audit*/ class L {} } }",
            21..31,
            "class_declaration 32..42",
            "block 19..44",
            vec![("class_declaration", 0..46), ("method_declaration", 10..44)],
        ),
        (
            "class C { void f() { new Object() { /**audit*/ void m() {} }; } }",
            36..46,
            "method_declaration 47..58",
            "class_body 34..60",
            vec![("class_declaration", 0..65), ("method_declaration", 10..63)],
        ),
    ] {
        both(source, DocumentationOptions::default(), |root, index| {
            assert!(!root.has_error(), "{source}");
            assert_eq!(
                documents(index),
                [(vec![fragment.clone()], Some("audit".to_owned()))],
                "{source}"
            );
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    vec![format!("Node({owner})")],
                    Some(owner.to_owned()),
                    scope.to_owned()
                )],
                "{source}"
            );
            assert_eq!(assessments(index, index.assessments()), [], "{source}");
            let (kind, range) = owner.split_once(' ').unwrap();
            let (start, end) = range.split_once("..").unwrap();
            let target = node(root, kind, start.parse().unwrap()..end.parse().unwrap());
            let query = index.node_associations(target).unwrap();
            assert!(query.associations().eq(index.associations()), "{source}");
            for (kind, range) in &others {
                let query = index
                    .node_associations(node(root, kind, range.clone()))
                    .unwrap();
                assert!(!query.projection_pending(), "{source}");
                assert_eq!(query.associations().len(), 0, "{source}");
            }
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_record_documentation_belongs_to_the_type() {
    for (source, record, constructor) in [
        (
            "/** Record doc */\nrecord R(int a) {}\n",
            "record_declaration 18..36",
            None,
        ),
        (
            "/** Record doc */\nrecord R(int a) { R {} }\n",
            "record_declaration 18..42",
            Some(36..40),
        ),
    ] {
        both(source, DocumentationOptions::default(), |root, index| {
            assert_eq!(
                documents(index),
                [(vec![0..17], Some("Record doc".to_owned()))]
            );
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    vec![format!("Node({record})")],
                    Some(record.to_owned()),
                    format!("program 0..{}", source.len())
                )]
            );
            assert!(matches!(
                index.node_associations(node(root, "formal_parameters", 26..33)),
                Err(OwnerError::UnsupportedOwner)
            ));
            if let Some(range) = constructor.clone() {
                let query = index
                    .node_associations(node(root, "compact_constructor_declaration", range))
                    .unwrap();
                assert!(!query.projection_pending());
                assert_eq!(query.associations().len(), 0);
            }
            assert_eq!(assessments(index, index.assessments()), []);
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_comments_without_a_declaration_produce_no_document() {
    for (source, kept, dropped) in [
        (
            "/**a*/\npublic /**b*/ static class C {}",
            vec![(vec![0..6], Some("a".to_owned()))],
            vec![14..20],
        ),
        ("class C { void f(/**p*/ int x) {} }", vec![], vec![17..23]),
        (
            "/** r */\nrecord R(/** a */ int a) { }\n",
            vec![(vec![0..8], Some("r".to_owned()))],
            vec![18..26],
        ),
        (
            "class C</**t*/ T> { </**u*/ U> void f() {} }",
            vec![],
            vec![8..14, 21..27],
        ),
        (
            "class C { /**i*/ static {} /**j*/ {} }",
            vec![],
            vec![10..16, 27..33],
        ),
        (
            "class C { void f() { /**v*/ int x; } }",
            vec![],
            vec![21..27],
        ),
        (
            "class C { void f() {}\n  /**d*/\n  ;\n  void g() {} }",
            vec![],
            vec![24..30],
        ),
    ] {
        both(source, DocumentationOptions::default(), |root, index| {
            assert!(!root.has_error(), "{source}");
            assert_eq!(documents(index), kept, "{source}");
            assert_eq!(index.associations().len(), kept.len(), "{source}");
            assert_eq!(evidence(index), [], "{source}");
            assert_eq!(assessments(index, index.assessments()), [], "{source}");
        });
        both(source, unbound(), |_, index| {
            assert_eq!(documents(index), kept, "{source}");
            assert_eq!(
                evidence(index),
                dropped
                    .iter()
                    .map(|range| (CandidateDisposition::Unattached, range.clone()))
                    .collect::<Vec<_>>(),
                "{source}"
            );
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_grammar_names_declarators_by_kind_and_first_child() {
    let mut declarations = 0;
    let mut declarators = 0;
    for source in [
        "class C { /**audit*/ int a = 1, b[] = {2}, c; }",
        "class C {\n  /** a */\n  int x,\n  /** b */\n  y;\n}\n",
        "class C { /**audit*/ @Deprecated int x; }",
        "class C { /**audit*/ int x /*c*/ = 1; }",
        "class C { /**audit*/ int /*c*/ x, /*d*/ y; }",
        "class C { /**audit*/ java.util.Map<String, int[]> m, n[]; }",
        "class C { /**audit*/ int record, open; }",
        "class C { /**audit*/ int a, _; }",
        "interface I { /**audit*/ int K = 1; }",
        "@interface A { /**audit*/ int K = 1; }",
        "enum E { /**a*/ A, /**b*/ B; /**f*/ int x; }",
        "class C { Object o = new Object() { /**audit*/ int x; }; }",
    ] {
        let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
        let root = parser.ts_tree().root_node();
        assert!(!root.has_error(), "{source}");
        let mut stack = vec![root];
        while let Some(current) = stack.pop() {
            let mut cursor = current.walk();
            stack.extend(current.children(&mut cursor));
            if !matches!(current.kind(), "field_declaration" | "constant_declaration") {
                continue;
            }
            declarations += 1;
            let mut by_field = Vec::new();
            let mut by_kind = Vec::new();
            let mut cursor = current.walk();
            assert!(cursor.goto_first_child());
            loop {
                if cursor.field_name() == Some("declarator") {
                    by_field.push(cursor.node().id());
                }
                if cursor.node().kind() == "variable_declarator" {
                    by_kind.push(cursor.node().id());
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            assert!(!by_field.is_empty(), "{source}");
            assert_eq!(by_field, by_kind, "{source}");
            let mut cursor = current.walk();
            for child in current.children(&mut cursor) {
                if child.kind() != "variable_declarator" {
                    continue;
                }
                declarators += 1;
                let name = child.child_by_field_name("name").unwrap();
                assert_eq!(child.child(0).unwrap().id(), name.id(), "{source}");
                assert!(
                    matches!(name.kind(), "identifier" | "underscore_pattern"),
                    "{source}"
                );
                assert_eq!(
                    name.kind() == "underscore_pattern",
                    &source.as_bytes()[name.byte_range()] == b"_",
                    "{source}"
                );
            }
        }
    }
    // expected: one declaration per source, two in the anonymous-class source.
    assert_eq!(declarations, 13);
    // expected: 3 + 2 + 1 + 1 + 2 + 2 + 2 + 2 + 1 + 1 + 1 + 2.
    assert_eq!(declarators, 20);
}

#[cfg(feature = "java")]
#[test]
fn java_field_declarators_each_own_a_field_slot() {
    for (source, fragment, declaration, names) in [
        (
            "class C { /**audit*/ int x; }",
            10..20,
            "field_declaration 21..27",
            vec![25..26],
        ),
        (
            "class C { /**audit*/ int a, b; }",
            10..20,
            "field_declaration 21..30",
            vec![25..26, 28..29],
        ),
        (
            "class C { /**audit*/ int a[], b; }",
            10..20,
            "field_declaration 21..32",
            vec![25..26, 30..31],
        ),
        (
            "class C { /**audit*/ int a = 1, b[] = {2}, c; }",
            10..20,
            "field_declaration 21..45",
            vec![25..26, 32..33, 43..44],
        ),
        (
            "class C { /**audit*/ @Deprecated int x; }",
            10..20,
            "field_declaration 21..39",
            vec![37..38],
        ),
        (
            "class C { /**audit*/ public static final int X = 1; }",
            10..20,
            "field_declaration 21..51",
            vec![45..46],
        ),
    ] {
        both(source, DocumentationOptions::default(), |_, index| {
            assert_eq!(
                documents(index),
                [(vec![fragment.clone()], Some("audit".to_owned()))],
                "{source}"
            );
            let owners: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(ordinal, name)| slot(ordinal, name.clone(), declaration))
                .collect();
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    owners,
                    Some(declaration.to_owned()),
                    format!("class_body 8..{}", source.len())
                )],
                "{source}"
            );
            assert_eq!(assessments(index, index.assessments()), [], "{source}");
            // expected: file, class, declaration, one slot per declarator.
            assert_eq!(index.facts().owners().len(), 3 + names.len(), "{source}");
            for (ordinal, name) in names.iter().enumerate() {
                let owner = index
                    .facts()
                    .associated_owner(index.associations().next().unwrap(), ordinal)
                    .unwrap();
                let Owner::Slot(found) = owner.get() else {
                    panic!("field slot")
                };
                assert_eq!(
                    found.source.pieces()[0].bytes(),
                    &source.as_bytes()[name.clone()]
                );
            }
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_queries_answer_per_slot_and_per_declaration() {
    both(
        "class C { /**audit*/ int x; }",
        DocumentationOptions::default(),
        |root, index| {
            let field = node(root, "field_declaration", 21..27);
            let association = index.associations().next().unwrap();
            let owner = index.facts().associated_owner(association, 0).unwrap();
            let projection = index.node_projection(field).unwrap();
            assert!(!projection.pending());
            assert!(projection.owners().eq([owner]));
            assert!(index.node_owners(field).unwrap().eq([owner]));
            let query = index.node_associations(field).unwrap();
            assert!(!query.projection_pending());
            assert!(query.associations().eq([association]));
            assert_eq!(query.assessments().count(), 0);
            assert!(
                index
                    .owner_associations(owner)
                    .unwrap()
                    .associations()
                    .eq([association])
            );
            let enclosing = index.facts().slot_enclosing(owner).unwrap();
            assert_eq!(
                owner_text(index, enclosing),
                "Node(field_declaration 21..27)"
            );
            let declaration = index.owner_associations(enclosing).unwrap();
            assert_eq!(declaration.associations().len(), 0);
            assert_eq!(declaration.assessments().count(), 0);
            for (kind, range) in [
                ("integral_type", 21..24),
                ("variable_declarator", 25..26),
                ("identifier", 25..26),
                (";", 26..27),
            ] {
                assert!(matches!(
                    index.node_associations(node(root, kind, range.clone())),
                    Err(OwnerError::UnsupportedOwner)
                ));
                assert!(matches!(
                    index.node_projection(node(root, kind, range.clone())),
                    Err(OwnerError::UnsupportedOwner)
                ));
                assert!(matches!(
                    index.node_owners(node(root, kind, range)),
                    Err(OwnerError::UnsupportedOwner)
                ));
            }
        },
    );
    both(
        "class C { /**audit*/ int a, b; }",
        DocumentationOptions::default(),
        |root, index| {
            let field = node(root, "field_declaration", 21..30);
            let association = index.associations().next().unwrap();
            let owners = [
                index.facts().associated_owner(association, 0).unwrap(),
                index.facts().associated_owner(association, 1).unwrap(),
            ];
            assert!(matches!(
                index.node_associations(field),
                Err(OwnerError::MultipleOwners)
            ));
            assert!(index.node_owners(field).unwrap().eq(owners));
            let projection = index.node_projection(field).unwrap();
            assert!(!projection.pending());
            assert!(projection.owners().eq(owners));
            assert_eq!(projection.assessments().count(), 0);
            for owner in owners {
                let query = index.owner_associations(owner).unwrap();
                assert!(query.associations().eq([association]));
                assert_eq!(query.assessments().count(), 0);
                assert_eq!(
                    owner_text(index, index.facts().slot_enclosing(owner).unwrap()),
                    "Node(field_declaration 21..30)"
                );
            }
            assert_ne!(owners[0], owners[1]);
        },
    );
}

#[cfg(feature = "java")]
#[test]
fn java_constants_own_field_slots() {
    for (source, fragment, declaration, name, scope) in [
        (
            "interface I { /**audit*/ int K = 1; }",
            14..24,
            "constant_declaration 25..35",
            29..30,
            "interface_body 12..37",
        ),
        (
            "@interface A { /**audit*/ int K = 1; }",
            15..25,
            "constant_declaration 26..36",
            30..31,
            "annotation_type_body 13..38",
        ),
    ] {
        both(source, DocumentationOptions::default(), |_, index| {
            assert_eq!(
                documents(index),
                [(vec![fragment.clone()], Some("audit".to_owned()))]
            );
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    vec![slot(0, name.clone(), declaration)],
                    Some(declaration.to_owned()),
                    scope.to_owned()
                )]
            );
            assert_eq!(assessments(index, index.assessments()), []);
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_enum_constants_and_enum_field_have_their_own_owners() {
    both(
        "enum E { /**a*/ A, /**b*/ B; /**f*/ int x; }",
        DocumentationOptions::default(),
        |_, index| {
            assert_eq!(
                documents(index),
                [
                    (vec![9..15], Some("a".to_owned())),
                    (vec![19..25], Some("b".to_owned())),
                    (vec![29..35], Some("f".to_owned())),
                ]
            );
            assert_eq!(
                associations(index),
                [
                    (
                        0,
                        Resolution::Resolved,
                        vec!["Node(enum_constant 16..17)".to_owned()],
                        Some("enum_constant 16..17".to_owned()),
                        "enum_body 7..44".to_owned()
                    ),
                    (
                        1,
                        Resolution::Resolved,
                        vec!["Node(enum_constant 26..27)".to_owned()],
                        Some("enum_constant 26..27".to_owned()),
                        "enum_body 7..44".to_owned()
                    ),
                    (
                        2,
                        Resolution::Resolved,
                        vec![slot(0, 40..41, "field_declaration 36..42")],
                        Some("field_declaration 36..42".to_owned()),
                        "enum_body_declarations 27..42".to_owned()
                    ),
                ]
            );
            assert_eq!(assessments(index, index.assessments()), []);
            // expected: file, enum, two constants, declaration, slot.
            assert_eq!(index.facts().owners().len(), 6);
        },
    );
}

#[cfg(feature = "java")]
#[test]
fn java_fields_of_records_anonymous_and_local_classes_own_field_slots() {
    for (source, fragment, declaration, name, scope, owners) in [
        (
            "record R(int a) { /**audit*/ static int x; }",
            18..28,
            "field_declaration 29..42",
            40..41,
            "class_body 16..44",
            // expected: file, record, declaration, slot.
            4,
        ),
        (
            "class C { Object o = new Object() { /**audit*/ int x; }; }",
            36..46,
            "field_declaration 47..53",
            51..52,
            "class_body 34..55",
            // expected: file, class, inner declaration and slot, outer declaration and slot.
            6,
        ),
        (
            "class C { void f() { class L { /**audit*/ int x; } } }",
            31..41,
            "field_declaration 42..48",
            46..47,
            "class_body 29..50",
            // expected: file, class C, method, class L, declaration, slot.
            6,
        ),
    ] {
        both(source, DocumentationOptions::default(), |_, index| {
            assert_eq!(
                documents(index),
                [(vec![fragment.clone()], Some("audit".to_owned()))]
            );
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    vec![slot(0, name.clone(), declaration)],
                    Some(declaration.to_owned()),
                    scope.to_owned()
                )]
            );
            assert_eq!(assessments(index, index.assessments()), []);
            assert_eq!(index.facts().owners().len(), owners);
        });
    }
    both(
        "class C { Object o = new Object() { /**audit*/ int x; }; }",
        DocumentationOptions::default(),
        |root, index| {
            let outer = index
                .node_owners(node(root, "field_declaration", 10..56))
                .unwrap()
                .map(|owner| owner_text(index, owner))
                .collect::<Vec<_>>();
            assert_eq!(outer, [slot(0, 17..18, "field_declaration 10..56")]);
        },
    );
}

#[cfg(feature = "java")]
#[test]
fn java_second_comment_inside_a_field_declaration_is_not_documentation() {
    let source = "class C {\n  /** a */\n  int x,\n  /** b */\n  y;\n}\n";
    assert_eq!(source.len(), 48);
    for (options, kept) in [
        (DocumentationOptions::default(), vec![]),
        (unbound(), vec![(CandidateDisposition::Unattached, 32..40)]),
    ] {
        both(source, options, |_, index| {
            assert_eq!(documents(index), [(vec![12..20], Some("a".to_owned()))]);
            assert_eq!(
                associations(index),
                [(
                    0,
                    Resolution::Resolved,
                    vec![
                        slot(0, 27..28, "field_declaration 23..45"),
                        slot(1, 43..44, "field_declaration 23..45")
                    ],
                    Some("field_declaration 23..45".to_owned()),
                    "class_body 8..47".to_owned()
                )]
            );
            assert_eq!(evidence(index), kept);
            assert_eq!(assessments(index, index.assessments()), []);
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_undocumented_fields_have_slots_and_no_assessment() {
    both("class C { int x; int a, b; }", unbound(), |root, index| {
        assert_eq!(documents(index), []);
        assert_eq!(associations(index), []);
        assert_eq!(assessments(index, index.assessments()), []);
        assert_eq!(evidence(index), []);
        let owners: Vec<_> = index
            .facts()
            .owners()
            .map(|owner| owner_text(index, owner))
            .collect();
        assert_eq!(
            owners,
            [
                "File(program 0..28)".to_owned(),
                "Node(class_declaration 0..28)".to_owned(),
                "Node(field_declaration 10..16)".to_owned(),
                slot(0, 14..15, "field_declaration 10..16"),
                "Node(field_declaration 17..26)".to_owned(),
                slot(0, 21..22, "field_declaration 17..26"),
                slot(1, 24..25, "field_declaration 17..26"),
            ]
        );
        let single = index
            .node_associations(node(root, "field_declaration", 10..16))
            .unwrap();
        assert!(!single.projection_pending());
        assert_eq!(single.associations().len(), 0);
        assert_eq!(single.assessments().count(), 0);
        for owner in index.facts().owners() {
            let query = index.owner_associations(owner).unwrap();
            assert_eq!(query.associations().len(), 0);
            assert_eq!(query.assessments().count(), 0);
        }
    });
}

#[cfg(feature = "java")]
#[test]
fn java_comment_inside_a_field_declaration_does_not_document_it() {
    for (source, declaration, name, dropped) in [
        (
            "class C { @Deprecated /**audit*/ int x; }",
            "field_declaration 10..39",
            37..38,
            22..32,
        ),
        (
            "class C { public /**audit*/ int x; }",
            "field_declaration 10..34",
            32..33,
            17..27,
        ),
        (
            "class C {\n  int x;\n  /**d*/\n  ;\n  void f() {}\n}\n",
            "field_declaration 12..18",
            16..17,
            21..27,
        ),
    ] {
        both(source, unbound(), |root, index| {
            assert_eq!(documents(index), []);
            assert_eq!(associations(index), []);
            assert_eq!(
                evidence(index),
                [(CandidateDisposition::Unattached, dropped.clone())]
            );
            assert_eq!(assessments(index, index.assessments()), []);
            let (kind, range) = declaration.split_once(' ').unwrap();
            let (start, end) = range.split_once("..").unwrap();
            let field = node(root, kind, start.parse().unwrap()..end.parse().unwrap());
            let owners: Vec<_> = index
                .node_owners(field)
                .unwrap()
                .map(|owner| owner_text(index, owner))
                .collect();
            assert_eq!(owners, [slot(0, name.clone(), declaration)]);
            let query = index.node_associations(field).unwrap();
            assert_eq!(query.associations().len(), 0);
            assert_eq!(query.assessments().count(), 0);
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_with_a_syntax_error_stays_pending() {
    // javac recovers from the missing `;` and attaches `d` to `x` and `e` to `y`.
    // The pinned grammar leaves an error in the first declaration, which stays pending.
    let source = "class C {\n/** d */\nint x\n/** e */\nint y;\n}\n";
    assert_eq!(source.len(), 43);
    both(source, unbound(), |root, index| {
        assert!(node(root, "field_declaration", 19..33).has_error());
        assert_eq!(documents(index), [(vec![10..18], Some("d".to_owned()))]);
        assert_eq!(associations(index), []);
        assert_eq!(
            assessments(index, index.assessments()),
            [
                (
                    AssessmentAxis::Owner,
                    PENDING_DOCUMENT,
                    Some("field_declaration 19..33".to_owned()),
                    Some(0)
                ),
                (
                    AssessmentAxis::Owner,
                    PENDING_OWNER,
                    Some("class_declaration 0..42".to_owned()),
                    None
                ),
                (
                    AssessmentAxis::Owner,
                    PENDING_OWNER,
                    Some("field_declaration 19..33".to_owned()),
                    None
                ),
            ]
        );
        assert_eq!(
            evidence(index),
            [(CandidateDisposition::Unattached, 25..33)]
        );
        let broken = node(root, "field_declaration", 19..33);
        assert!(index.node_projection(broken).unwrap().pending());
        assert!(matches!(
            index.node_owners(broken),
            Err(OwnerError::PendingOwner)
        ));
        let clean = index
            .node_owners(node(root, "field_declaration", 34..40))
            .unwrap()
            .map(|owner| owner_text(index, owner))
            .collect::<Vec<_>>();
        assert_eq!(clean, [slot(0, 38..39, "field_declaration 34..40")]);
        // expected: file, the clean declaration and its slot;
        // the class and first field are pending.
        assert_eq!(index.facts().owners().len(), 3);
    });
    let source = "class C { /**audit*/ int x = ; }";
    both(source, DocumentationOptions::default(), |root, index| {
        let field = node(root, "field_declaration", 21..30);
        assert!(field.has_error());
        assert_eq!(node(root, "ERROR", 27..28).parent().unwrap(), field);
        assert_eq!(documents(index), [(vec![10..20], Some("audit".to_owned()))]);
        assert_eq!(associations(index), []);
        assert!(index.node_projection(field).unwrap().pending());
        // expected: the file only.
        assert_eq!(index.facts().owners().len(), 1);
    });
    // The same guard keeps a constant pending: `=` without a value is an `ERROR` child.
    let source = "interface I { /**audit*/ int K = ; }";
    both(source, DocumentationOptions::default(), |root, index| {
        let constant = node(root, "constant_declaration", 25..34);
        assert!(constant.has_error());
        assert_eq!(node(root, "ERROR", 31..32).parent().unwrap(), constant);
        assert_eq!(documents(index), [(vec![14..24], Some("audit".to_owned()))]);
        assert_eq!(associations(index), []);
        assert!(index.node_projection(constant).unwrap().pending());
        // expected: the file only; the interface holds the error too.
        assert_eq!(index.facts().owners().len(), 1);
    });
}

#[cfg(feature = "java")]
#[test]
fn java_field_with_an_unnamed_declarator_stays_pending_as_a_whole() {
    // javac rejects `_` as a field name and still attaches the comment to every
    // declarator. Here the declaration has no plain name for one slot and stays pending.
    for (source, field) in [
        ("class C { /**audit*/ int a, _; }", 21..30),
        ("class C { /**audit*/ int _, a; }", 21..30),
    ] {
        both(source, DocumentationOptions::default(), |root, index| {
            assert!(!root.has_error());
            let declaration = node(root, "field_declaration", field.clone());
            assert_eq!(documents(index), [(vec![10..20], Some("audit".to_owned()))]);
            assert_eq!(associations(index), []);
            assert_eq!(
                assessments(index, index.assessments()),
                [
                    (
                        AssessmentAxis::Owner,
                        PENDING_DOCUMENT,
                        Some("field_declaration 21..30".to_owned()),
                        Some(0)
                    ),
                    (
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some("field_declaration 21..30".to_owned()),
                        None
                    ),
                ]
            );
            assert!(index.node_projection(declaration).unwrap().pending());
            assert!(matches!(
                index.node_owners(declaration),
                Err(OwnerError::PendingOwner)
            ));
            // expected: file and class; no declaration owner and no slot.
            assert_eq!(index.facts().owners().len(), 2);
        });
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_beside_recovery_keeps_its_slots() {
    let source = "class C { /**audit*/ int x; static {";
    both(source, DocumentationOptions::default(), |root, index| {
        assert!(root.is_error());
        assert!(!node(root, "field_declaration", 21..27).has_error());
        assert_eq!(documents(index), [(vec![10..20], Some("audit".to_owned()))]);
        assert_eq!(
            associations(index),
            [(
                0,
                Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                vec![slot(0, 25..26, "field_declaration 21..27")],
                Some("field_declaration 21..27".to_owned()),
                "ERROR 0..36".to_owned()
            )]
        );
        assert_eq!(assessments(index, index.assessments()), []);
    });
    let source = "class C { /**audit*/ int x; ";
    both(source, DocumentationOptions::default(), |root, index| {
        assert!(!root.is_error());
        assert!(node(root, "class_declaration", 0..27).has_error());
        assert!(!node(root, "field_declaration", 21..27).has_error());
        assert_eq!(
            associations(index),
            [(
                0,
                Resolution::Resolved,
                vec![slot(0, 25..26, "field_declaration 21..27")],
                Some("field_declaration 21..27".to_owned()),
                "class_body 8..27".to_owned()
            )]
        );
        let pending = [(
            AssessmentAxis::Owner,
            PENDING_OWNER,
            Some("class_declaration 0..27".to_owned()),
            None,
        )];
        assert_eq!(assessments(index, index.assessments()), pending);
        let query = index
            .node_associations(node(root, "field_declaration", 21..27))
            .unwrap();
        assert_eq!(assessments(index, query.assessments()), pending);
    });
}

#[cfg(feature = "java")]
#[test]
#[allow(clippy::too_many_lines)]
fn java_field_and_excluded_ranges() {
    let source = "class C { /**audit*/ int a, b; }";
    let context = DocumentationContext::default();
    // A gap inside the declaration: the space at 27..28.
    let tree = restricted(source, &[0..27, 28..32]);
    let index = collect(LANG::Java, &tree, source.as_bytes(), &context, &unbound()).unwrap();
    let root = tree.root_node();
    assert!(!root.has_error());
    let field = node(root, "field_declaration", 21..30);
    assert_eq!(documents(&index), []);
    assert_eq!(associations(&index), []);
    assert_eq!(
        evidence(&index),
        [(CandidateDisposition::Unattached, 10..20)]
    );
    assert!(index.node_projection(field).unwrap().pending());
    assert!(matches!(
        index.node_owners(field),
        Err(OwnerError::PendingOwner)
    ));
    // expected: the file only; class, body and field hold the gap.
    assert_eq!(index.facts().owners().len(), 1);
    assert_eq!(
        assessments(&index, index.assessments()),
        [
            (
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some("class_declaration 0..32".to_owned()),
                None
            ),
            (
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some("class_body 8..32".to_owned()),
                None
            ),
            (
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some("field_declaration 21..30".to_owned()),
                None
            ),
        ]
    );
    // A gap between the comment and the declaration: the space at 20..21.
    let tree = restricted(source, &[0..20, 21..32]);
    let index = collect(LANG::Java, &tree, source.as_bytes(), &context, &unbound()).unwrap();
    let root = tree.root_node();
    assert!(!root.has_error());
    let field = node(root, "field_declaration", 21..30);
    assert_eq!(documents(&index), []);
    assert_eq!(associations(&index), []);
    assert_eq!(
        evidence(&index),
        [(CandidateDisposition::Unattached, 10..20)]
    );
    let owners: Vec<_> = index
        .node_owners(field)
        .unwrap()
        .map(|owner| owner_text(&index, owner))
        .collect();
    assert_eq!(
        owners,
        [
            slot(0, 25..26, "field_declaration 21..30"),
            slot(1, 28..29, "field_declaration 21..30")
        ]
    );
    for owner in index.node_owners(field).unwrap() {
        assert_eq!(
            index
                .owner_associations(owner)
                .unwrap()
                .associations()
                .len(),
            0
        );
    }
    assert_eq!(
        assessments(&index, index.assessments()),
        [
            (
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some("class_declaration 0..32".to_owned()),
                None
            ),
            (
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some("class_body 8..32".to_owned()),
                None
            ),
        ]
    );
    // No gap: two adjacent included ranges meet inside the name, after `al`.
    let source = "class C { /**audit*/ int alpha; }";
    assert_eq!(source.len(), 33);
    let tree = restricted(source, &[0..27, 27..33]);
    let index = collect(LANG::Java, &tree, source.as_bytes(), &context, &unbound()).unwrap();
    assert!(!tree.root_node().has_error());
    assert_eq!(
        documents(&index),
        [(vec![10..20], Some("audit".to_owned()))]
    );
    assert_eq!(
        associations(&index),
        [(
            0,
            Resolution::Resolved,
            vec!["Slot(Field #0 25..27,27..30 in Node(field_declaration 21..31))".to_owned()],
            Some("field_declaration 21..31".to_owned()),
            "class_body 8..33".to_owned()
        )]
    );
    assert_eq!(assessments(&index, index.assessments()), []);
    assert_eq!(evidence(&index), []);
    // expected: file, class, declaration, slot.
    assert_eq!(index.facts().owners().len(), 4);
    let owner = index
        .facts()
        .associated_owner(index.associations().next().unwrap(), 0)
        .unwrap();
    let Owner::Slot(found) = owner.get() else {
        panic!("field slot")
    };
    assert_eq!(found.source.pieces()[0].bytes(), b"al");
    assert_eq!(found.source.pieces()[1].bytes(), b"pha");
}

#[cfg(feature = "java")]
#[test]
fn java_field_comment_with_a_raw_unicode_escape_lists_its_slot_unresolved() {
    // javac translates the escape first and attaches `@audit ` to `x`.
    let source = r"class C { /** \u0040audit */ int x; }";
    assert_eq!(source.len(), 37);
    both(source, DocumentationOptions::default(), |root, index| {
        assert!(!root.has_error());
        assert_eq!(documents(index), [(vec![10..28], None)]);
        assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(UnavailableReason::UnsupportedDecoding)
        ));
        assert_eq!(
            associations(index),
            [(
                0,
                Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                vec![slot(0, 33..34, "field_declaration 29..35")],
                Some("field_declaration 29..35".to_owned()),
                "class_body 8..37".to_owned()
            )]
        );
        assert_eq!(assessments(index, index.assessments()), []);
        let field = node(root, "field_declaration", 29..35);
        let query = index.node_associations(field).unwrap();
        assert!(!query.projection_pending());
        assert!(query.associations().eq(index.associations()));
        assert_eq!(query.assessments().count(), 0);
        // expected: file, class, declaration, slot.
        assert_eq!(index.facts().owners().len(), 4);
    });
}

#[cfg(feature = "java")]
#[test]
fn java_pending_field_comment_with_a_raw_unicode_escape_keeps_an_ownerless_association() {
    // The unnamed declarator keeps the declaration pending; the escape alone makes the
    // association unresolved, so it exists without an owner.
    let source = r"class C { /** \u0040audit */ int a, _; }";
    assert_eq!(source.len(), 40);
    both(source, DocumentationOptions::default(), |root, index| {
        assert!(!root.has_error());
        assert_eq!(documents(index), [(vec![10..28], None)]);
        assert!(matches!(
            index.documents().next().unwrap().get().content(),
            Content::Unavailable(UnavailableReason::UnsupportedDecoding)
        ));
        assert_eq!(
            associations(index),
            [(
                0,
                Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                vec![],
                Some("field_declaration 29..38".to_owned()),
                "class_body 8..40".to_owned()
            )]
        );
        assert_eq!(
            assessments(index, index.assessments()),
            [
                (
                    AssessmentAxis::Owner,
                    PENDING_DOCUMENT,
                    Some("field_declaration 29..38".to_owned()),
                    Some(0)
                ),
                (
                    AssessmentAxis::Owner,
                    PENDING_OWNER,
                    Some("field_declaration 29..38".to_owned()),
                    None
                ),
            ]
        );
        let field = node(root, "field_declaration", 29..38);
        assert!(index.node_projection(field).unwrap().pending());
        assert!(matches!(
            index.node_owners(field),
            Err(OwnerError::PendingOwner)
        ));
        // expected: file and class; no declaration owner and no slot.
        assert_eq!(index.facts().owners().len(), 2);
    });
}

#[cfg(feature = "java")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "java")]
impl WorkBudget for Counting {
    type Error = SourceError;
    #[cfg(feature = "java")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }
    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        if self.fail_at == Some(self.steps.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
    #[cfg(feature = "java")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        Ok(())
    }
}

#[cfg(feature = "java")]
fn wide(width: usize) -> String {
    let names: Vec<_> = (0..width).map(|at| format!("v{at}")).collect();
    format!("class C {{ /**audit*/ int {}; }}", names.join(", "))
}

#[cfg(feature = "java")]
#[test]
#[allow(clippy::too_many_lines)]
fn java_declarator_scan_charges_one_step_per_child() {
    let wide_source = wide(300);
    for (source, kind, steps, names) in [
        (
            "class C { /**audit*/ int x; }",
            "field_declaration",
            // expected: type, declarator, `;`.
            3,
            Some(vec![25..26]),
        ),
        (
            "class C { /**audit*/ int a, b; }",
            "field_declaration",
            // expected: type, declarator, `,`, declarator, `;`.
            5,
            Some(vec![25..26, 28..29]),
        ),
        (
            "class C { /**audit*/ int a = 1, b[] = {2}, c; }",
            "field_declaration",
            // expected: type, three declarators with two `,`, `;`.
            7,
            Some(vec![25..26, 32..33, 43..44]),
        ),
        (
            "class C {\n  /** a */\n  int x,\n  /** b */\n  y;\n}\n",
            "field_declaration",
            // expected: type, declarator, `,`, comment, declarator, `;`.
            6,
            Some(vec![27..28, 43..44]),
        ),
        (
            "class C { /**audit*/ @Deprecated int x; }",
            "field_declaration",
            // expected: modifiers, type, declarator, `;`.
            4,
            Some(vec![37..38]),
        ),
        (
            "interface I { /**audit*/ int K = 1; }",
            "constant_declaration",
            // expected: type, declarator, `;`.
            3,
            Some(vec![29..30]),
        ),
        (
            "class C { /**audit*/ int a, _; }",
            "field_declaration",
            // expected: type, declarator, `,`, and the unnamed declarator that ends the scan.
            4,
            None,
        ),
        (
            "class C { /**audit*/ int _, a; }",
            "field_declaration",
            // expected: type and the unnamed declarator that ends the scan.
            2,
            None,
        ),
        (
            wide_source.as_str(),
            "field_declaration",
            // expected: type, 300 declarators, 299 `,`, `;`.
            601,
            None::<Vec<Range<usize>>>,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let declaration = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == kind)
            .unwrap();
        let id = index
            .canonical
            .lookup(declaration.as_tree_sitter())
            .unwrap();
        let budget = Counting::default();
        let found =
            super::projections::java_declarator_names(&index.canonical, id, &budget).unwrap();
        assert_eq!(budget.steps.get(), steps, "{source}");
        assert_eq!(budget.bytes.get(), 0, "{source}");
        let found = found.map(|names| {
            names
                .iter()
                .map(|name| index.canonical.nodes[name.0].evidence.node().byte_range())
                .collect::<Vec<_>>()
        });
        if source.len() < 100 {
            assert_eq!(found, names, "{source}");
        } else {
            let found = found.unwrap();
            assert_eq!(found.len(), 300);
            assert_eq!(found[0], 25..27);
            assert_eq!(found[299], 1709..1713);
        }
        if source.len() >= 100 || source == "class C { /**audit*/ int a, b; }" {
            for fail_at in 1..=steps {
                let budget = Counting {
                    fail_at: Some(fail_at),
                    ..Counting::default()
                };
                assert!(matches!(
                    super::projections::java_declarator_names(&index.canonical, id, &budget),
                    Err(SourceError::Cancelled)
                ));
                assert_eq!(budget.steps.get(), fail_at);
            }
            let budget = Counting {
                fail_at: Some(steps + 1),
                ..Counting::default()
            };
            assert!(
                super::projections::java_declarator_names(&index.canonical, id, &budget)
                    .unwrap()
                    .is_some()
            );
        }
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_width_costs_linear_work() {
    for width in [64, 512, 2048] {
        let source = wide(width);
        let parser = AnyParser::parse(LANG::Java, source.clone().into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let context = DocumentationContext::default();
        let options = DocumentationOptions::default();
        let before = observations();
        let index = parser.documentation(&context, &options).unwrap();
        let after = observations();
        // expected: 12 nodes beside the declarators (`program`, `class_declaration`,
        // `class`, `identifier`, `class_body`, `{`, `block_comment`, `field_declaration`,
        // `integral_type`, `int`, `;`, `}`), a declarator and its name per field, and
        // one comma less than fields.
        assert_eq!(parser.root_node().preorder().count(), 3 * width + 11);
        // expected: one visit and one child batch per node.
        assert_eq!(after.visits - before.visits, 3 * width + 11);
        assert_eq!(after.child_batches - before.child_batches, 3 * width + 11);
        // expected: every node but the root, which returns first.
        assert_eq!(
            after.target_classifications - before.target_classifications,
            3 * width + 10
        );
        // expected: the file, the class and the declaration; a slot is pushed without
        // an owner projection.
        assert_eq!(after.owner_projections - before.owner_projections, 3);
        // expected: one search for the comment fragment and one per slot; each is one
        // comparison with one included range.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            width + 1
        );
        // expected: the profile requests no field name, and nothing looks one up.
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        // expected: one document.
        assert_eq!(after.renders - before.renders, 1);
        // expected: file, class, declaration and one slot per field.
        assert_eq!(index.facts().owners().len(), width + 3);
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().owner_count(), width);
        for position in [0, width - 1] {
            let owner = index
                .facts()
                .associated_owner(association, position)
                .unwrap();
            let Owner::Slot(found) = owner.get() else {
                panic!("field slot")
            };
            assert_eq!(found.ordinal, position);
            assert_eq!(
                found.source.pieces()[0].bytes(),
                format!("v{position}").as_bytes()
            );
        }
    }
}

#[cfg(feature = "java")]
#[test]
fn java_field_width_collection_observes_cancellation_in_the_slot_loop() {
    let width = 2048;
    let parser = AnyParser::parse(LANG::Java, wide(width).into_bytes(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut inside = 0;
    let mut scanning = 0;
    for checkpoint in 1..=polls {
        let before = observations();
        let result = crate::documentation::budget::cancel_after(checkpoint, || {
            parser.documentation(&context, &options)
        });
        let after = observations();
        assert!(
            matches!(result, Err(CollectionError::Cancelled)),
            "{checkpoint}"
        );
        let owners = after.owner_projections - before.owner_projections;
        let searches = after.intersection_searches - before.intersection_searches;
        if owners == 3 && searches < width + 1 {
            inside += 1;
        }
        // Every node but the closing brace of the class body is entered and the
        // declaration has no owner yet: the walk is between the declaration's last
        // child and its slots, where only the declarator scan is long.
        if owners == 2 && after.visits - before.visits == 3 * width + 10 {
            scanning += 1;
        }
    }
    // expected: 6 checkpoints fall in the 6144 step charges of the slot loop
    // (3 per slot, one checkpoint per 1024); the last may follow the last search.
    assert!(inside >= 5, "{inside}");
    // expected: no more than those 6; a loop that charges more steps for a later
    // slot holds more.
    assert!(inside <= 6, "{inside}");
    // expected: the scan charges one step for each of the 4097 children of the
    // declaration (the type, 2048 declarators, 2047 commas and `;`), which hold 4
    // checkpoints, or 5 with the few steps before it. Without those charges fewer
    // than 1024 steps lie there, which hold at most one.
    assert!((4..=5).contains(&scanning), "{scanning}");
    let index = crate::documentation::budget::cancel_after(polls + 1, || {
        parser.documentation(&context, &options)
    })
    .unwrap();
    assert_eq!(
        index.associations().next().unwrap().get().owner_count(),
        width
    );
}
