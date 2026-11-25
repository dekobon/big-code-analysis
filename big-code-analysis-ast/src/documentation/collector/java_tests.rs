#![allow(clippy::single_range_in_vec_init)]

#[cfg(feature = "java")]
use crate::documentation::*;

#[cfg(feature = "java")]
use crate::{AnyParser, LANG};

#[cfg(feature = "java")]
use std::ops::Range;

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
        check(parser.ts_tree().root_node(), &index);
    }
}

#[cfg(feature = "java")]
fn unbound() -> DocumentationOptions {
    DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    }
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
