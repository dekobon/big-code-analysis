//! Groovydoc source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the tree shapes the source-view tests rely on. Node queries are not in the view.

#[cfg(feature = "groovy")]
use crate::documentation::*;
#[cfg(feature = "groovy")]
use crate::{AnyParser, LANG};
#[cfg(feature = "groovy")]
use std::ops::Range;

#[cfg(feature = "groovy")]
use Run::{Copied, Decoded};

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
    owners: Vec<(usize, Resolution, &'static str, Range<usize>)>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    pending: Vec<Pending>,
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
        owners: vec![(0, Resolution::Resolved, kind, range)],
        ..View::default()
    }
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
            assert_eq!(attachment.owner_count(), 1);
            assert_eq!(attachment.convention.name(), "Groovydoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owner = facts.associated_owner(association, 0).unwrap();
            let Owner::Node(declared) = owner.get() else {
                panic!("one node owner, found {:?}", owner.get())
            };
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                Some(declared.node())
            );
            let (kind, range) = site(declared.node());
            (position(document), attachment.resolution, kind, range)
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
    assert_eq!(view(source), expected);
    expected.evidence.extend([
        (CandidateDisposition::Superseded, 0..8),
        (CandidateDisposition::Superseded, 9..17),
    ]);
    assert_eq!(retained(source), expected);
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
                (0, Resolution::Resolved, "enum_constant", 26..27),
                (1, Resolution::Resolved, "enum_constant", 46..47),
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

/// Pins today's pending field form; this test is expected to move when fields gain owners.
#[cfg(feature = "groovy")]
#[test]
fn groovy_field_keeps_a_document_without_an_owner() {
    assert_eq!(
        view(b"class C {\n/**audit*/\nint x\n}\n"),
        View {
            documents: vec![comment(10..20, "audit", [(0..5, Copied(13..18))])],
            pending: unordered([
                Pending {
                    axis: AssessmentAxis::Owner,
                    reason: "Selected local or wrapper form awaits owner projection",
                    node: ("field_declaration", 21..26),
                    document: Some(0),
                },
                Pending {
                    axis: AssessmentAxis::Owner,
                    reason: "Recognized declaration requires a semantic projection",
                    node: ("field_declaration", 21..26),
                    document: None,
                },
            ]),
            ..View::default()
        }
    );
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
