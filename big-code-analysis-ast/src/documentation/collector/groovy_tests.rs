//! Groovydoc source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the tree shapes the source-view tests rely on. Field node queries are tested separately.
#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "groovy")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "groovy")]
use crate::documentation::*;
#[cfg(feature = "groovy")]
use crate::{AnyParser, LANG};
#[cfg(feature = "groovy")]
use Run::{Copied, Decoded};
#[cfg(feature = "groovy")]
use std::cell::Cell;
#[cfg(feature = "groovy")]
use std::ops::Range;

#[cfg(feature = "groovy")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "groovy")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "groovy")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: (&'static str, Range<usize>),
    document: Option<usize>,
}

/// Documents, associations (with document positions and node owners), retained candidates
/// and order-independent pending assessments. Shared values are asserted by `published`;
/// node queries are not included. An empty view has nothing published or pending.
#[cfg(feature = "groovy")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    owners: Vec<(usize, Resolution, Vec<ExpectedOwner>)>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    pending: Vec<Pending>,
}

#[cfg(feature = "groovy")]
#[derive(Debug, PartialEq, Eq)]
enum ExpectedOwner {
    Node(&'static str, Range<usize>),
    Field(usize, Range<usize>, Range<usize>),
}

#[cfg(feature = "groovy")]
fn comment<const N: usize>(
    fragment: Range<usize>,
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        role: DocumentRole::Primary,
        composition: Composition::Single,
        fragments: vec![fragment],
        text: text.to_owned(),
        map: map.into(),
    }
}

#[cfg(feature = "groovy")]
fn owned(document: Doc, kind: &'static str, range: Range<usize>) -> View {
    View {
        documents: vec![document],
        owners: vec![(
            0,
            Resolution::Resolved,
            vec![ExpectedOwner::Node(kind, range)],
        )],
        ..View::default()
    }
}

#[cfg(feature = "groovy")]
fn field(ordinal: usize, source: Range<usize>, declaration: Range<usize>) -> ExpectedOwner {
    ExpectedOwner::Field(ordinal, source, declaration)
}

#[cfg(feature = "groovy")]
fn unordered(entries: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = entries.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "groovy")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "groovy")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "groovy")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("groovydoc_comment", range.clone()))
    );
    range
}

#[cfg(feature = "groovy")]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "groovy")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the four published lists and their shared rules.
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
    let position = |document| ordered.iter().position(|known| *known == document).unwrap();
    let documents = ordered
        .iter()
        .map(|&document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| fragment_span(facts.document_fragment(document, at).unwrap().get()))
                    .collect(),
                text: complete.text().to_owned(),
                map: complete
                    .mappings()
                    .iter()
                    .map(|run| (run.output.range(), run_of(&run.origin)))
                    .collect(),
            }
        })
        .collect();
    let owners = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.convention.name(), "Groovydoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owners = (0..attachment.owner_count())
                .map(|at| {
                    let owner = facts.associated_owner(association, at).unwrap();
                    match owner.get() {
                        Owner::Node(declared) => {
                            assert_eq!(
                                attachment.binding.map(NodeEvidence::node),
                                Some(declared.node())
                            );
                            let (kind, range) = site(declared.node());
                            ExpectedOwner::Node(kind, range)
                        }
                        Owner::Slot(slot) => {
                            assert_eq!(slot.role, SlotRole::Field);
                            let enclosing = facts.slot_enclosing(owner).unwrap();
                            let Owner::Node(declared) = enclosing.get() else {
                                panic!("field enclosing owner must be a node")
                            };
                            assert_eq!(declared.node().kind(), "field_declaration");
                            assert_eq!(
                                attachment.binding.map(NodeEvidence::node),
                                Some(declared.node())
                            );
                            ExpectedOwner::Field(
                                slot.ordinal,
                                span(&slot.source),
                                declared.node().byte_range(),
                            )
                        }
                        other => panic!("unexpected owner: {other:?}"),
                    }
                })
                .collect();
            (position(document), attachment.resolution, owners)
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    let pending = unordered(index.assessments().map(|assessment| Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: site(assessment.get().node.unwrap().node()),
        document: index.assessed_document(assessment).unwrap().map(position),
    }));
    assert_eq!(index.capability(), Capability::Partial);
    // Qualifications belong to named-reference commands, which none of these cases has.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        owners,
        evidence,
        pending,
    }
}

#[cfg(feature = "groovy")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Groovy, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        source.escape_ascii()
    );
    parser
}

#[cfg(feature = "groovy")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Groovy,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "groovy")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2]) -> View {
    let [typed, raw] = indexes.each_ref().map(published);
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "groovy")]
fn view(source: &[u8]) -> View {
    agreed(&indexes(&clean(source), DocumentationOptions::default()))
}

#[cfg(feature = "groovy")]
fn retained(source: &[u8]) -> View {
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&clean(source), options))
}

#[cfg(feature = "groovy")]
fn node_at<'a>(parser: &'a AnyParser, kind: &str, range: Range<usize>) -> tree_sitter::Node<'a> {
    let found: Vec<_> = parser
        .root_node()
        .preorder()
        .map(|candidate| candidate.as_tree_sitter())
        .filter(|candidate| candidate.kind() == kind && candidate.byte_range() == range)
        .collect();
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

#[cfg(feature = "groovy")]
fn named_children(parent: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    parent.named_children(&mut parent.walk()).collect()
}

#[cfg(feature = "groovy")]
fn shapes(nodes: &[tree_sitter::Node<'_>]) -> Vec<(&'static str, Range<usize>)> {
    nodes.iter().map(|node| site(*node)).collect()
}

#[cfg(feature = "groovy")]
fn parts(parent: tree_sitter::Node<'_>) -> Vec<(&'static str, Range<usize>)> {
    parent.children(&mut parent.walk()).map(site).collect()
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_type_declarations_own_the_block_above_them() {
    let types: [(&[u8], Doc, &str, Range<usize>); 7] = [
        (
            b"/** d */\nclass C {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..19,
        ),
        (
            b"/**audit*/\nclass C {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            "class_declaration",
            11..21,
        ),
        (
            b"/** d */\ntrait T {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "trait_declaration",
            9..19,
        ),
        (
            b"/** d */\ninterface I {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "interface_declaration",
            9..23,
        ),
        (
            b"/** d */\nenum E { A }\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "enum_declaration",
            9..21,
        ),
        (
            b"/** d */\nrecord R(int x) {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "record_declaration",
            9..27,
        ),
        (
            b"/** d */\n@interface A {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "annotation_type_declaration",
            9..24,
        ),
    ];
    for (source, document, kind, owner) in types {
        assert_eq!(
            view(source),
            owned(document, kind, owner),
            "{}",
            source.escape_ascii()
        );
    }
    // Only the nearest block documents the class; the two before it are retained on request.
    let source = b"/** a */\n/** b */\n/** c */\nclass C {}\n";
    let mut expected = owned(
        comment(18..26, "c", [(0..1, Copied(22..23))]),
        "class_declaration",
        27..37,
    );
    assert_eq!(view(source), expected, "{}", source.escape_ascii());
    expected.evidence.extend([
        (CandidateDisposition::Superseded, 0..8),
        (CandidateDisposition::Superseded, 9..17),
    ]);
    assert_eq!(retained(source), expected, "{}", source.escape_ascii());
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_method_constructor_and_enum_constant_own_the_block_above_them() {
    let members: [(&[u8], Doc, &str, Range<usize>); 3] = [
        (
            b"class C {\n    /** d */\n    void m() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "method_declaration",
            27..38,
        ),
        (
            b"class C {\n    /** d */\n    C() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "constructor_declaration",
            27..33,
        ),
        (
            b"enum E {\n    /** d */\n    A\n}\n",
            comment(13..21, "d", [(0..1, Copied(17..18))]),
            "enum_constant",
            26..27,
        ),
    ];
    for (source, document, kind, owner) in members {
        assert_eq!(
            view(source),
            owned(document, kind, owner),
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"enum E {\n    /** a */\n    A,\n    /** b */\n    B\n}\n"),
        View {
            documents: vec![
                comment(13..21, "a", [(0..1, Copied(17..18))]),
                comment(33..41, "b", [(0..1, Copied(37..38))]),
            ],
            owners: vec![
                (
                    0,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("enum_constant", 26..27)]
                ),
                (
                    1,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("enum_constant", 46..47)]
                ),
            ],
            ..View::default()
        }
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_annotation_between_block_and_declaration_does_not_detach() {
    let rows: [(&[u8], Doc, &str, Range<usize>); 3] = [
        (
            b"/**audit*/\n@Deprecated class C {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            "class_declaration",
            11..33,
        ),
        (
            b"/** d */\n@Deprecated\nclass C {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..31,
        ),
        (
            b"class C {\n    /** d */\n    @Deprecated\n    void m() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "method_declaration",
            27..54,
        ),
    ];
    for (source, document, kind, owner) in rows {
        assert_eq!(
            view(source),
            owned(document, kind, owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_blank_row_between_block_and_declaration_does_not_detach() {
    assert_eq!(
        view(b"/** d */\n\nclass C {}\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            10..20
        )
    );
    assert_eq!(
        view(b"class C {\n    /** d */\n\n    void m() {}\n}\n"),
        owned(
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "method_declaration",
            28..39
        )
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_plain_comment_between_block_and_declaration_detaches() {
    for source in [
        b"/** d */\n// plain\nclass C {}\n".as_slice(),
        b"/** d */\n/* plain */\nclass C {}\n",
    ] {
        let mut expected = View::default();
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        expected
            .evidence
            .push((CandidateDisposition::Unattached, 0..8));
        assert_eq!(retained(source), expected, "{}", source.escape_ascii());
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_block_above_package_documents_the_first_declaration() {
    assert_eq!(
        view(b"/** d */\npackage p\nclass C {}\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            19..29
        )
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_script_level_method_owns_the_block_above_it() {
    assert_eq!(
        view(b"/** d */\ndef f() {}\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "method_declaration",
            9..19
        )
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_plain_comments_are_no_documentation() {
    let plain: [(&[u8], &str, Range<usize>); 2] = [
        (b"/* d */\nclass C {}\n", "block_comment", 0..7),
        (b"// d\nclass C {}\n", "line_comment", 0..4),
    ];
    for (source, kind, range) in plain {
        let parser = clean(source);
        // The tree holds the plain comment, so each empty view is the collector's answer to it.
        assert_eq!(parts(parser.ts_tree().root_node())[0], (kind, range));
        assert_eq!(view(source), View::default(), "{}", source.escape_ascii());
        assert_eq!(
            retained(source),
            View::default(),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_block_text_over_several_lines() {
    let rows: [(&[u8], Doc, &str, Range<usize>); 5] = [
        (
            b"/**\n * a\n * b\n */\nclass C {}\n",
            comment(
                0..17,
                "a\nb",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..3, Copied(12..13)),
                ],
            ),
            "class_declaration",
            18..28,
        ),
        (
            b"/**\n * a  \n *   b\n */\nclass C {}\n",
            comment(
                0..21,
                "a  \n  b",
                [
                    (0..3, Copied(7..10)),
                    (3..4, Copied(10..11)),
                    (4..7, Copied(14..17)),
                ],
            ),
            "class_declaration",
            22..32,
        ),
        (
            b"/**\n * Sum.\n *\n * @since 1\n */\nclass C {}\n",
            comment(
                0..30,
                "Sum.\n\n@since 1",
                [
                    (0..4, Copied(7..11)),
                    (4..5, Copied(11..12)),
                    (5..6, Copied(14..15)),
                    (6..14, Copied(18..26)),
                ],
            ),
            "class_declaration",
            31..41,
        ),
        (
            b"/** a\n * b */\nclass C {}\n",
            comment(
                0..13,
                "a\nb",
                [
                    (0..1, Copied(4..5)),
                    (1..2, Copied(5..6)),
                    (2..3, Copied(9..10)),
                ],
            ),
            "class_declaration",
            14..24,
        ),
        (
            b"class C {\n    /**\n     * a\n     */\n    void m() {}\n}\n",
            comment(14..34, "a", [(0..1, Copied(25..26))]),
            "method_declaration",
            39..50,
        ),
    ];
    for (source, document, kind, owner) in rows {
        assert_eq!(
            view(source),
            owned(document, kind, owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_block_with_crlf_line_endings() {
    assert_eq!(
        view(b"/** d */\r\nclass C {}\r\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            10..20
        )
    );
    assert_eq!(
        view(b"/**\r\n * a\r\n * b\r\n */\r\nclass C {}\r\n"),
        owned(
            comment(
                0..20,
                "a\nb",
                [
                    (0..1, Copied(8..9)),
                    (1..2, Decoded(9..11)),
                    (2..3, Copied(14..15)),
                ],
            ),
            "class_declaration",
            22..32
        )
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_field_document_owns_its_name_slot() {
    assert_eq!(
        view(b"class C {\n/**audit*/\nint x\n}\n"),
        View {
            documents: vec![comment(10..20, "audit", [(0..5, Copied(13..18))])],
            owners: vec![(0, Resolution::Resolved, vec![field(0, 25..26, 21..26)])],
            ..View::default()
        }
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_spaces_fixture_resolves_all_five_documents() {
    let source = crate::test_support::documentation::source("groovy-spaces.groovy");
    let parser = clean(&source);
    let index = indexes(&parser, DocumentationOptions::default());
    let actual = agreed(&index);
    assert_eq!(actual.documents.len(), 5);
    assert_eq!(actual.owners.len(), 5);
    assert_eq!(
        actual,
        View {
            documents: vec![
                comment(0..13, "record", [(0..6, Copied(4..10))]),
                comment(33..44, "type", [(0..4, Copied(37..41))]),
                comment(59..70, "ctor", [(0..4, Copied(63..67))]),
                comment(86..98, "field", [(0..5, Copied(90..95))]),
                comment(113..126, "method", [(0..6, Copied(117..123))]),
            ],
            owners: vec![
                (
                    0,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("record_declaration", 14..32)]
                ),
                (
                    1,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("class_declaration", 45..159)]
                ),
                (
                    2,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("constructor_declaration", 75..81)]
                ),
                (3, Resolution::Resolved, vec![field(0, 107..108, 103..108)]),
                (
                    4,
                    Resolution::Resolved,
                    vec![ExpectedOwner::Node("method_declaration", 131..157)]
                ),
            ],
            ..View::default()
        }
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_two_superseded_field_blocks_keep_source_order() {
    let source = b"class C {\n/** a */\n/** b */\n/** c */\nint x\n}\n";
    let actual = retained(source);
    assert_eq!(
        actual.documents,
        [comment(28..36, "c", [(0..1, Copied(32..33))])]
    );
    assert_eq!(
        actual.owners,
        [(0, Resolution::Resolved, vec![field(0, 41..42, 37..42)])]
    );
    assert_eq!(
        actual.evidence,
        [
            (CandidateDisposition::Superseded, 10..18),
            (CandidateDisposition::Superseded, 19..27),
        ]
    );
    assert_eq!(actual.pending, []);
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_field_and_property_slots_cover_positive_forms() {
    type FieldRow<'a> = (&'a [u8], Range<usize>, &'a [Range<usize>]);
    let rows: [FieldRow<'_>; 6] = [
        (b"class C {\n/**audit*/\nint x\n}\n", 21..26, &[25..26]),
        (b"class C {\n/**audit*/\ndef x\n}\n", 21..26, &[25..26]),
        (
            b"class C {\n/**audit*/\nstatic final int X = 1\n}\n",
            21..43,
            &[38..39],
        ),
        (
            b"class C {\n/**audit*/\nint a = 1, b = 2\n}\n",
            21..37,
            &[25..26, 32..33],
        ),
        (
            b"class C {\n/**audit*/\n@Deprecated static int x\n}\n",
            21..45,
            &[44..45],
        ),
        (
            "class C {\n/**audit*/\ndef café = 1\n}\n".as_bytes(),
            21..34,
            &[25..30],
        ),
    ];
    for (source, declaration, names) in rows {
        let parser = clean(source);
        let indices = indexes(&parser, DocumentationOptions::default());
        let actual = agreed(&indices);
        assert_eq!(
            actual.documents,
            [comment(10..20, "audit", [(0..5, Copied(13..18))])]
        );
        assert_eq!(actual.pending, [], "{}", source.escape_ascii());
        let expected: Vec<_> = names
            .iter()
            .cloned()
            .enumerate()
            .map(|(ordinal, name)| field(ordinal, name, declaration.clone()))
            .collect();
        assert_eq!(actual.owners, [(0, Resolution::Resolved, expected)]);
        let field_node = node_at(&parser, "field_declaration", declaration);
        let index = &indices[0];
        let projection = index.node_projection(field_node).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), names.len());
        let association = index.associations().next().unwrap();
        for owner in projection.owners() {
            assert_eq!(
                index
                    .owner_associations(owner)
                    .unwrap()
                    .associations()
                    .next(),
                Some(association)
            );
        }
        if names.len() == 2 {
            assert!(matches!(
                index.node_associations(field_node),
                Err(OwnerError::MultipleOwners)
            ));
        }
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_enum_and_trait_fields_have_slots_in_their_own_scope() {
    for (source, declaration, name) in [
        (
            b"enum E { A;\n/**audit*/\nint x\n}\n".as_slice(),
            23..28,
            27..28,
        ),
        (b"trait T {\n/**audit*/\ndef x\n}\n", 21..26, 25..26),
    ] {
        let actual = view(source);
        assert_eq!(actual.documents.len(), 1);
        assert_eq!(
            actual.owners,
            [(0, Resolution::Resolved, vec![field(0, name, declaration)])]
        );
        assert_eq!(actual.pending, []);
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_comments_inside_fields_are_not_new_documents() {
    let source = b"class C {\n/**audit*/\nint a, /* plain */ b\n}\n";
    let actual = view(source);
    assert_eq!(
        actual.documents,
        [comment(10..20, "audit", [(0..5, Copied(13..18))])]
    );
    assert_eq!(
        actual.owners,
        [(
            0,
            Resolution::Resolved,
            vec![field(0, 25..26, 21..41), field(1, 40..41, 21..41)]
        )]
    );
    assert_eq!(actual.pending, []);
    let source = b"class C {\n/**audit*/\nint /** inner */ x\n}\n";
    let actual = retained(source);
    assert_eq!(
        actual.documents,
        [comment(10..20, "audit", [(0..5, Copied(13..18))])]
    );
    assert_eq!(
        actual.owners,
        [(0, Resolution::Resolved, vec![field(0, 38..39, 21..39)])]
    );
    assert_eq!(
        actual.evidence,
        [(CandidateDisposition::Unattached, 25..37)]
    );
    assert_eq!(actual.pending.len(), 1);
    assert_eq!(actual.pending[0].axis, AssessmentAxis::Selection);
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_undocumented_field_still_gets_both_slots() {
    let source = b"class C { int a, b }";
    let parser = clean(source);
    let indices = indexes(&parser, DocumentationOptions::default());
    let actual = agreed(&indices);
    assert_eq!(actual, View::default());
    let field_node = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    let owners = indices[0].node_owners(field_node).unwrap();
    assert_eq!(owners.len(), 2);
    for (ordinal, owner) in owners.enumerate() {
        let Owner::Slot(slot) = owner.get() else {
            panic!("slot expected")
        };
        assert_eq!(slot.ordinal, ordinal);
        assert_eq!(slot.role, SlotRole::Field);
        assert_eq!(span(&slot.source), [14..15, 17..18][ordinal]);
        let query = indices[0].owner_associations(owner).unwrap();
        assert!(!query.projection_pending());
        assert!(query.associations().next().is_none());
    }
}

#[cfg(feature = "groovy")]
#[test]
#[allow(clippy::too_many_lines)]
fn groovy_literal_and_comment_lines_do_not_make_field_documents() {
    const PENDING_OWNER: &str = "Recognized declaration requires a semantic projection";
    for (source, field_range, slot_range, pending_nodes) in [
        (
            b"class C { def s = '\n  /** doc */\n  int x\n' }\n".as_slice(),
            10..38,
            None,
            &[("field_declaration", 10..38)][..],
        ),
        (
            b"class C { def s = \"\n  /** doc */\n  int x\n\" }\n",
            10..38,
            None,
            &[("field_declaration", 10..38)],
        ),
        (
            b"class C { def s = '''\n  /** doc */\n  int x\n''' }\n",
            10..46,
            Some(14..15),
            &[],
        ),
        (
            b"class C { def s = \"\"\"\n  /** doc */\n  int x\n\"\"\" }\n",
            10..46,
            Some(14..15),
            &[],
        ),
        (
            b"class C { def s = /\n  /** doc */\n  int x\n/ }\n",
            10..38,
            None,
            &[("class_declaration", 0..44), ("field_declaration", 10..38)],
        ),
        (
            b"class C { def s = $/\n  /** doc */\n  int x\n/$ }\n",
            10..44,
            Some(14..15),
            &[],
        ),
        (
            b"class C { /*\n  /** doc */\n  int x\n*/ }\n",
            28..33,
            Some(32..33),
            &[("class_declaration", 0..38)],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Groovy, source.to_vec(), None, None).unwrap();
        let indices = indexes(&parser, DocumentationOptions::default());
        let actual = agreed(&indices);
        let pending = pending_nodes
            .iter()
            .map(|(kind, range)| Pending {
                axis: AssessmentAxis::Owner,
                reason: PENDING_OWNER,
                node: (kind, range.clone()),
                document: None,
            })
            .collect::<Vec<_>>();
        let expected = View {
            pending: unordered(pending),
            ..View::default()
        };
        assert_eq!(actual, expected, "{}", source.escape_ascii());
        let field_node = node_at(&parser, "field_declaration", field_range.clone());
        for index in &indices {
            let projection = index.node_projection(field_node).unwrap();
            assert_eq!(
                projection.pending(),
                slot_range.is_none(),
                "{}",
                source.escape_ascii()
            );
            if let Some(name) = &slot_range {
                let [owner] = projection.owners().collect::<Vec<_>>()[..] else {
                    panic!("one field slot: {}", source.escape_ascii())
                };
                let Owner::Slot(slot) = owner.get() else {
                    panic!("field slot expected")
                };
                assert_eq!(slot.role, SlotRole::Field);
                assert_eq!(slot.ordinal, 0);
                assert_eq!(span(&slot.source), name.clone());
                let enclosing = index.facts().slot_enclosing(owner).unwrap();
                let Owner::Node(declaration) = enclosing.get() else {
                    panic!("enclosing declaration expected")
                };
                assert_eq!(
                    site(declaration.node()),
                    ("field_declaration", field_range.clone())
                );
            }
        }
        let retained = agreed(&indexes(
            &parser,
            DocumentationOptions {
                retain_unbound: true,
                ..DocumentationOptions::default()
            },
        ));
        let evidence = if slot_range.is_none() {
            vec![(CandidateDisposition::Unattached, 22..32)]
        } else {
            vec![]
        };
        let expected = View {
            evidence,
            pending: expected.pending,
            ..View::default()
        };
        assert_eq!(retained, expected, "{}", source.escape_ascii());
    }
    // Characterization: grammar recovery exposes a Groovydoc inside what Groovy
    // 4.0.32's lexer reads as slashy strings. This Resolved owner is today's
    // named lexer/grammar limitation, not an intended Groovy attachment.
    let slashy = b"class C { def s = /a\n  /** doc */\n  int x\n/ }\n";
    let parser = AnyParser::parse(LANG::Groovy, slashy.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let expected = View {
        documents: vec![comment(23..33, "doc", [(0..3, Copied(27..30))])],
        owners: vec![(0, Resolution::Resolved, vec![field(0, 40..41, 36..41)])],
        pending: unordered([
            Pending {
                axis: AssessmentAxis::Owner,
                reason: PENDING_OWNER,
                node: ("class_declaration", 0..45),
                document: None,
            },
            Pending {
                axis: AssessmentAxis::Owner,
                reason: PENDING_OWNER,
                node: ("field_declaration", 10..20),
                document: None,
            },
        ]),
        ..View::default()
    };
    assert_eq!(
        agreed(&indexes(&parser, DocumentationOptions::default())),
        expected
    );
    assert_eq!(
        agreed(&indexes(
            &parser,
            DocumentationOptions {
                retain_unbound: true,
                ..DocumentationOptions::default()
            }
        )),
        expected
    );
    let control = b"class C {\n  /** doc */\n  int x\n}\n";
    assert_eq!(
        view(control),
        View {
            documents: vec![comment(12..22, "doc", [(0..3, Copied(16..19))])],
            owners: vec![(0, Resolution::Resolved, vec![field(0, 29..30, 25..30)])],
            ..View::default()
        }
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_local_variable_forms_are_not_field_targets() {
    // Characterization: these local declarations are outside the field adapter's scope.
    for (source, comment) in [
        (b"/**audit*/\ndef x\n".as_slice(), 0..10),
        (b"class C {\nvoid f() {\n/**audit*/\nint x\n}\n}\n", 21..31),
        (
            b"class C {\ndef f() { def q = {\n/**audit*/\nint x\n} }\n}\n",
            30..40,
        ),
    ] {
        let parser = clean(source);
        assert_eq!(
            agreed(&indexes(&parser, DocumentationOptions::default())),
            View::default(),
            "{}",
            source.escape_ascii()
        );
        let mut expected = View::default();
        expected
            .evidence
            .push((CandidateDisposition::Unattached, comment));
        assert_eq!(
            agreed(&indexes(
                &parser,
                DocumentationOptions {
                    retain_unbound: true,
                    ..DocumentationOptions::default()
                }
            )),
            expected,
            "{}",
            source.escape_ascii()
        );
        assert!(
            !parser
                .root_node()
                .preorder()
                .any(|node| node.kind() == "field_declaration")
        );
    }
}

#[cfg(feature = "groovy")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "groovy")]
impl WorkBudget for Counting {
    type Error = SourceError;
    #[cfg(feature = "groovy")]
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
    #[cfg(feature = "groovy")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        Ok(())
    }
}

#[cfg(feature = "groovy")]
fn wide(width: usize) -> String {
    let names: Vec<_> = (0..width).map(|at| format!("v{at}")).collect();
    format!("class C {{\n/**audit*/\nint {}\n}}", names.join(", "))
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_field_declarator_scan_charges_each_child_once() {
    for (source, steps, names) in [
        (
            b"class C {\n/**audit*/\nint x\n}\n".as_slice(),
            2,
            vec![25..26],
        ),
        (
            b"class C {\n/**audit*/\nint a = 1, b = 2\n}\n",
            4,
            vec![25..30, 32..37],
        ),
        (
            b"class C {\n/**audit*/\nint a, /* plain */ b\n}\n",
            5,
            vec![25..26, 40..41],
        ),
    ] {
        let parser = clean(source);
        let index = indexes(&parser, DocumentationOptions::default());
        let field_node = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "field_declaration")
            .unwrap()
            .as_tree_sitter();
        let id = index[0].canonical.lookup(field_node).unwrap();
        let budget = Counting::default();
        let found = super::projections::java_declarator_names(&index[0].canonical, id, &budget)
            .unwrap()
            .unwrap();
        assert_eq!(budget.steps.get(), steps);
        assert_eq!(budget.bytes.get(), 0);
        let actual: Vec<_> = found
            .iter()
            .map(|id| index[0].canonical.nodes[id.0].evidence.node().byte_range())
            .collect();
        // expected: the first identifier child of each variable_declarator is the name.
        let names = names
            .iter()
            .map(|range| range.start..range.start + 1)
            .collect::<Vec<_>>();
        assert_eq!(actual, names, "{}", source.escape_ascii());
        for checkpoint in 1..=steps {
            let budget = Counting {
                fail_at: Some(checkpoint),
                ..Counting::default()
            };
            assert!(matches!(
                super::projections::java_declarator_names(&index[0].canonical, id, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), checkpoint);
        }
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_field_width_has_linear_collection_cost_and_cancellation() {
    for width in [64, 512] {
        let source = wide(width);
        let parser = clean(source.as_bytes());
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        assert_eq!(
            after.visits - before.visits,
            parser.root_node().preorder().count()
        );
        assert_eq!(
            after.child_batches - before.child_batches,
            parser.root_node().preorder().count()
        );
        assert_eq!(after.owner_projections - before.owner_projections, 3);
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            width + 1
        );
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        assert_eq!(
            index.associations().next().unwrap().get().owner_count(),
            width
        );
        assert_eq!(index.facts().owners().len(), width + 3);
    }
    let width = 2048;
    let source = wide(width);
    let parser = clean(source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut scanning = 0;
    let mut slotting = 0;
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
        if after.owner_projections - before.owner_projections == 2
            && after.visits - before.visits == parser.root_node().preorder().count() - 1
        {
            scanning += 1;
        }
        if after.owner_projections - before.owner_projections == 3
            && after.intersection_searches - before.intersection_searches < width + 1
        {
            slotting += 1;
        }
    }
    // expected: 1 type + 2048 declarators + 2047 commas = 4096 child charges;
    // four checkpoints per 1024 steps, plus at most one from preceding walk steps.
    assert!((4..=5).contains(&scanning), "scan checkpoints: {scanning}");
    // expected: each of 2048 slots charges its loop and two range steps: 6144
    // steps, six checkpoints per 1024, plus at most one from preceding work.
    assert!((6..=7).contains(&slotting), "slot checkpoints: {slotting}");
    assert_eq!(
        crate::documentation::budget::cancel_after(polls + 1, || parser
            .documentation(&context, &options))
        .unwrap()
        .associations()
        .next()
        .unwrap()
        .get()
        .owner_count(),
        width
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_field_excluded_bytes_remain_pending_and_gap_blocks_attachment() {
    let source = b"class C {\n/**audit*/\nint a, b\n}\n";
    let parse = |ranges: &[Range<usize>]| {
        let point = |offset: usize| {
            let before = &source[..offset];
            tree_sitter::Point::new(
                before.iter().filter(|byte| **byte == b'\n').count(),
                before
                    .iter()
                    .rev()
                    .take_while(|byte| **byte != b'\n')
                    .count(),
            )
        };
        let ranges: Vec<_> = ranges
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
            .set_language(&LANG::Groovy.tree_sitter_language().unwrap())
            .unwrap();
        parser.set_included_ranges(&ranges).unwrap();
        parser.parse(source, None).unwrap()
    };
    let tree = parse(&[0..27, 28..source.len()]);
    let adopted = AnyParser::from_tree(LANG::Groovy, tree, source.to_vec()).unwrap();
    let indices = indexes(
        &adopted,
        DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        },
    );
    let actual = agreed(&indices);
    assert_eq!(actual.documents, []);
    assert_eq!(actual.owners, []);
    let field_node = adopted
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    assert!(indices[0].node_projection(field_node).unwrap().pending());
    let tree = parse(&[0..20, 21..source.len()]);
    let adopted = AnyParser::from_tree(LANG::Groovy, tree, source.to_vec()).unwrap();
    let indices = indexes(
        &adopted,
        DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        },
    );
    let actual = agreed(&indices);
    assert_eq!(actual.documents, []);
    assert_eq!(actual.owners, []);
    assert_eq!(
        actual.evidence,
        [(CandidateDisposition::Unattached, 10..20)]
    );
    let field_node = adopted
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    assert_eq!(indices[0].node_owners(field_node).unwrap().len(), 2);
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_same_row_field_block_is_unselected_but_the_field_has_a_slot() {
    let source = b"class C { /**audit*/ int x }";
    let parser = clean(source);
    let indices = indexes(
        &parser,
        DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        },
    );
    let actual = agreed(&indices);
    assert_eq!(actual.documents, []);
    assert_eq!(actual.owners, []);
    assert_eq!(
        actual.evidence,
        [(CandidateDisposition::Unattached, 10..20)]
    );
    assert_eq!(actual.pending.len(), 1);
    assert_eq!(actual.pending[0].axis, AssessmentAxis::Selection);
    let declaration = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    let mut owners = indices[0].node_owners(declaration).unwrap();
    assert_eq!(owners.len(), 1);
    let owner = owners.next().unwrap();
    let Owner::Slot(slot) = owner.get() else {
        panic!("a field slot is required")
    };
    assert_eq!(slot.role, SlotRole::Field);
    assert_eq!(span(&slot.source), 25..26);
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_slashy_recovery_field_is_pending_and_clean_fields_beside_errors_have_slots() {
    let pending_owner = |node| Pending {
        axis: AssessmentAxis::Owner,
        reason: "Recognized declaration requires a semantic projection",
        node,
        document: None,
    };
    let slashy = b"class C { def s = /\n  /** doc */\n  int x\n/ }\n";
    let parser = AnyParser::parse(LANG::Groovy, slashy.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let indices = indexes(&parser, DocumentationOptions::default());
    let actual = agreed(&indices);
    let expected = View {
        pending: unordered([
            pending_owner(("class_declaration", 0..44)),
            pending_owner(("field_declaration", 10..38)),
        ]),
        ..View::default()
    };
    assert_eq!(actual, expected);
    let mut retained = expected;
    retained
        .evidence
        .push((CandidateDisposition::Unattached, 22..32));
    assert_eq!(
        agreed(&indexes(
            &parser,
            DocumentationOptions {
                retain_unbound: true,
                ..DocumentationOptions::default()
            }
        )),
        retained
    );
    let field_node = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    assert!(field_node.has_error());
    assert!(indices[0].node_projection(field_node).unwrap().pending());
    // In this recovered parse the invalid initializer is a sibling ERROR, not part of the field.
    let source = b"class C {\n/**audit*/\nint x = ;\n}\n";
    let parser = AnyParser::parse(LANG::Groovy, source.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let indices = indexes(&parser, DocumentationOptions::default());
    let actual = agreed(&indices);
    let expected = View {
        documents: vec![comment(10..20, "audit", [(0..5, Copied(13..18))])],
        owners: vec![(0, Resolution::Resolved, vec![field(0, 25..26, 21..26)])],
        pending: vec![pending_owner(("class_declaration", 0..32))],
        ..View::default()
    };
    assert_eq!(actual, expected);
    assert_eq!(
        agreed(&indexes(
            &parser,
            DocumentationOptions {
                retain_unbound: true,
                ..DocumentationOptions::default()
            }
        )),
        expected
    );
    let declaration = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field_declaration")
        .unwrap()
        .as_tree_sitter();
    assert!(!declaration.has_error());
    assert_eq!(indices[0].node_owners(declaration).unwrap().len(), 1);
}

#[cfg(feature = "groovy")]
#[test]
#[allow(clippy::too_many_lines)]
fn groovy_grammar_shapes_the_unit_relies_on() {
    let source = b"class C {}\ntrait T {}\ninterface I {}\nenum E { A }\n\
                   record R(int x) {}\n@interface A {}\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("source_file", 0..85));
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        shapes(&named_children(root)),
        [
            ("class_declaration", 0..10),
            ("trait_declaration", 11..21),
            ("interface_declaration", 22..36),
            ("enum_declaration", 37..49),
            ("record_declaration", 50..68),
            ("annotation_type_declaration", 69..84),
        ]
    );
    let annotation = node_at(&parser, "annotation_type_declaration", 69..84);
    let at = annotation.child(0).unwrap();
    assert_eq!(site(at), ("@", 69..70));
    assert!(!at.is_named());
    let body = node_at(&parser, "enum_declaration", 37..49)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(body), ("enum_body", 44..49));
    assert_eq!(
        node_at(&parser, "enum_constant", 46..47).parent(),
        Some(body)
    );

    let parser =
        clean(b"class C {\n    C() {}\n    void m() {}\n    int x\n    def y\n    int a, b\n}\n");
    let body = node_at(&parser, "class_declaration", 0..71)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(body), ("class_body", 8..71));
    assert_eq!(
        shapes(&named_children(body)),
        [
            ("constructor_declaration", 14..20),
            ("method_declaration", 25..36),
            ("field_declaration", 41..46),
            ("field_declaration", 51..56),
            ("field_declaration", 61..69),
        ]
    );
    let fields = [
        (41..46, 45..46, None),
        (51..56, 55..56, None),
        (61..69, 65..66, Some(68..69)),
    ];
    for (field, first, second) in fields {
        let declaration = node_at(&parser, "field_declaration", field);
        let actual: Vec<_> = named_children(declaration)
            .into_iter()
            .filter(|node| node.kind() == "variable_declarator")
            .map(|node| node.byte_range())
            .collect();
        assert_eq!(
            actual,
            std::iter::once(first).chain(second).collect::<Vec<_>>()
        );
    }

    let parser = clean(b"enum E {\n    /** a */\n    A,\n    /** b */\n    B\n}\n");
    let body = node_at(&parser, "enum_declaration", 0..49)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(body), ("enum_body", 7..49));
    assert_eq!(
        parts(body),
        [
            ("{", 7..8),
            ("groovydoc_comment", 13..21),
            ("enum_constant", 26..27),
            (",", 27..28),
            ("groovydoc_comment", 33..41),
            ("enum_constant", 46..47),
            ("}", 48..49),
        ]
    );
    let comma = node_at(&parser, "enum_constant", 26..27)
        .next_sibling()
        .unwrap();
    assert_eq!(site(comma), (",", 27..28));
    assert!(!comma.is_named());

    let parser = clean(
        b"/** d */\n@Deprecated\nclass C {\n    /** e */\n    @Deprecated\n    void m() {}\n}\n",
    );
    let comment = node_at(&parser, "groovydoc_comment", 0..8);
    let class = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(parser.ts_tree().root_node()));
    assert_eq!(site(class), ("class_declaration", 9..77));
    assert_eq!(class.child(0).map(site), Some(("annotation", 9..20)));
    let body = class.child_by_field_name("body").unwrap();
    assert_eq!(site(body), ("class_body", 29..77));
    let comment = node_at(&parser, "groovydoc_comment", 35..43);
    let method = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(body));
    assert_eq!(site(method), ("method_declaration", 48..75));
    assert_eq!(method.child(0).map(site), Some(("annotation", 48..59)));

    let parser = clean(b"/** d */\npackage p\nclass C {}\n");
    assert_eq!(
        shapes(&named_children(parser.ts_tree().root_node())),
        [
            ("groovydoc_comment", 0..8),
            ("package_declaration", 9..18),
            ("class_declaration", 19..29),
        ]
    );

    let parser = clean(b"/** d */\n/* b */\n// l\nclass C {}\n");
    let children = named_children(parser.ts_tree().root_node());
    assert_eq!(
        shapes(&children),
        [
            ("groovydoc_comment", 0..8),
            ("block_comment", 9..16),
            ("line_comment", 17..21),
            ("class_declaration", 22..32),
        ]
    );
    assert!(children[..3].iter().all(tree_sitter::Node::is_extra));
    let source = parser.code();
    assert_eq!(source[children[2].end_byte()], b'\n');

    let parser = clean(b"/**a*/ class C { /**b*/ C() {} }\n");
    let comment = node_at(&parser, "groovydoc_comment", 0..6);
    let class = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(parser.ts_tree().root_node()));
    assert_eq!(site(class), ("class_declaration", 7..32));
    assert_eq!(comment.end_position().row, class.start_position().row);
    let body = class.child_by_field_name("body").unwrap();
    assert_eq!(site(body), ("class_body", 15..32));
    let comment = node_at(&parser, "groovydoc_comment", 17..23);
    let ctor = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(body));
    assert_eq!(site(ctor), ("constructor_declaration", 24..30));
    assert_eq!(comment.end_position().row, ctor.start_position().row);

    let parser = clean(b"/** d */\ndef f() {}\ndef x = 1\n");
    assert_eq!(
        shapes(&named_children(parser.ts_tree().root_node())),
        [
            ("groovydoc_comment", 0..8),
            ("method_declaration", 9..19),
            ("local_variable_declaration", 20..29),
        ]
    );

    let parser = clean(b"/** d */\n\nclass C {}\n");
    let comment = node_at(&parser, "groovydoc_comment", 0..8);
    assert_eq!(
        comment.next_sibling().map(site),
        Some(("class_declaration", 10..20))
    );

    let parser = clean(b"/** a */\n// plain\nclass C {}\n/** b */\n/* plain */\nclass D {}\n");
    assert_eq!(
        shapes(&named_children(parser.ts_tree().root_node())),
        [
            ("groovydoc_comment", 0..8),
            ("line_comment", 9..17),
            ("class_declaration", 18..28),
            ("groovydoc_comment", 29..37),
            ("block_comment", 38..49),
            ("class_declaration", 50..60),
        ]
    );

    let source = b"/**\r\n * a\r\n * b\r\n */\r\nclass C {}\r\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("source_file", 0..34));
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        shapes(&named_children(root)),
        [("groovydoc_comment", 0..20), ("class_declaration", 22..32)]
    );
    let comment = node_at(&parser, "groovydoc_comment", 0..20);
    let class = comment.next_sibling().unwrap();
    assert_eq!(&source[comment.end_byte()..class.start_byte()], b"\r\n");
    assert_eq!(comment.end_position().row - comment.start_position().row, 3);
    assert_eq!(
        source[comment.byte_range()]
            .windows(2)
            .filter(|pair| *pair == b"\r\n")
            .count(),
        3
    );
}
