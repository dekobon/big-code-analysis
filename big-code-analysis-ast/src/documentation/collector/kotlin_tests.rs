//! Kotlin KDoc source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the tree shapes the unit relies on. Node queries are checked beside the view.

#[cfg(feature = "kotlin")]
use crate::documentation::*;
#[cfg(feature = "kotlin")]
use crate::{AnyParser, LANG};
#[cfg(feature = "kotlin")]
use std::ops::Range;

#[cfg(feature = "kotlin")]
use Run::{Copied, Decoded};

#[cfg(feature = "kotlin")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "kotlin")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "kotlin")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    resolution: Resolution,
    owner: (&'static str, Range<usize>),
}

#[cfg(feature = "kotlin")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and order-independent pending assessments.
/// Shared values are asserted by `published`; node queries are checked separately.
#[cfg(feature = "kotlin")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Link>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "kotlin")]
const AWAITS_OWNER: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "kotlin")]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";
#[cfg(feature = "kotlin")]
const SIGNATURE: &str = "KDoc signature or parameter form awaits owner projection";

#[cfg(feature = "kotlin")]
type RetainedPair<'a> = (
    &'a [u8],
    Doc,
    &'static str,
    Range<usize>,
    &'a [Range<usize>],
);

#[cfg(feature = "kotlin")]
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

#[cfg(feature = "kotlin")]
fn owned(document: Doc, kind: &'static str, range: Range<usize>) -> View {
    View {
        documents: vec![document],
        associations: vec![Link {
            document: 0,
            resolution: Resolution::Resolved,
            owner: (kind, range),
        }],
        ..View::default()
    }
}

#[cfg(feature = "kotlin")]
fn pending(
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
) -> Pending {
    Pending {
        axis,
        reason,
        node,
        document,
    }
}

#[cfg(feature = "kotlin")]
fn unordered(entries: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = entries.into_iter().collect();
    // Assessment order is unspecified; both the observed and expected lists use this key.
    sorted.sort_by_cached_key(|entry| {
        (
            entry.reason,
            entry.node.as_ref().map(|(_, range)| range.start),
            entry.node.as_ref().map(|(_, range)| range.end),
            entry.document,
        )
    });
    sorted
}

#[cfg(feature = "kotlin")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "kotlin")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "kotlin")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("block_comment", range.clone()))
    );
    range
}

#[cfg(feature = "kotlin")]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "kotlin")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the four lists and their shared invariants.
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
                    .map(|entry| (entry.output.range(), origin_run(&entry.origin)))
                    .collect(),
            }
        })
        .collect();
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.owner_count(), 1);
            assert_eq!(attachment.convention.name(), "KDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owner = facts.associated_owner(association, 0).unwrap();
            let Owner::Node(declared) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                Some(declared.node())
            );
            Link {
                document: position(document),
                resolution: attachment.resolution,
                owner: site(declared.node()),
            }
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    let assessments = unordered(index.assessments().map(|assessment| Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: assessment.get().node.map(|node| site(node.node())),
        document: index.assessed_document(assessment).unwrap().map(position),
    }));
    assert_eq!(index.capability(), Capability::Partial);
    // Qualifications belong to named-reference commands, absent from all these KDoc sources.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "kotlin")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Kotlin, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        source.escape_ascii()
    );
    parser
}

#[cfg(feature = "kotlin")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Kotlin,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "kotlin")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2]) -> View {
    let [typed, raw] = indexes.each_ref().map(published);
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "kotlin")]
fn view(source: &[u8]) -> View {
    let parser = clean(source);
    agreed(&indexes(&parser, DocumentationOptions::default()))
}

#[cfg(feature = "kotlin")]
fn retained(source: &[u8]) -> View {
    let parser = clean(source);
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&parser, options))
}

#[cfg(feature = "kotlin")]
fn node_at<'a>(parser: &'a AnyParser, kind: &str, range: Range<usize>) -> tree_sitter::Node<'a> {
    let found: Vec<_> = parser
        .root_node()
        .preorder()
        .map(|node| node.as_tree_sitter())
        .filter(|node| node.kind() == kind && node.byte_range() == range)
        .collect();
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

#[cfg(feature = "kotlin")]
fn parts(parent: tree_sitter::Node<'_>) -> Vec<(Option<&'static str>, &'static str, Range<usize>)> {
    let mut cursor = parent.walk();
    let mut result = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            result.push((cursor.field_name(), child.kind(), child.byte_range()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    result
}

#[cfg(feature = "kotlin")]
fn flagged(root: tree_sitter::Node<'_>) -> Vec<(&'static str, Range<usize>, [bool; 5])> {
    let mut unvisited = vec![root];
    let mut nodes = Vec::new();
    while let Some(node) = unvisited.pop() {
        nodes.push((
            node.kind(),
            node.byte_range(),
            [
                node.has_error(),
                node.is_error(),
                node.is_missing(),
                node.is_extra(),
                node.is_named(),
            ],
        ));
        let children: Vec<_> = node.children(&mut node.walk()).collect();
        unvisited.extend(children.into_iter().rev());
    }
    nodes
}

#[cfg(feature = "kotlin")]
fn grammar_parse(source: &[u8], has_error: bool) -> AnyParser {
    let parser = AnyParser::parse(LANG::Kotlin, source.to_vec(), None, None).unwrap();
    let root = parser.ts_tree().root_node();
    assert_eq!(root.has_error(), has_error, "{}", source.escape_ascii());
    assert_eq!(root.end_byte(), source.len());
    let mut raw = tree_sitter::Parser::new();
    raw.set_language(&LANG::Kotlin.tree_sitter_language().unwrap())
        .unwrap();
    let typed = flagged(root);
    assert_eq!(typed, flagged(raw.parse(source, None).unwrap().root_node()));
    if !has_error {
        assert!(
            typed
                .iter()
                .all(|(_, _, [error, is_error, missing, ..])| !error && !is_error && !missing)
        );
    }
    parser
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_kdoc_before_a_top_level_declaration_documents_it() {
    let declarations: [(&[u8], Doc, &str, Range<usize>); 4] = [
        (
            b"/**audit*/\nfun f() {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            "function_declaration",
            11..21,
        ),
        (
            b"/** d */\nclass C {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..19,
        ),
        (
            b"/** d */\nobject O {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "object_declaration",
            9..20,
        ),
        (
            b"/** d */\ntypealias T = Int\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "type_alias",
            9..26,
        ),
    ];
    for (source, document, kind, range) in declarations {
        assert_eq!(
            view(source),
            owned(document, kind, range),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_kdoc_before_a_member_documents_the_member_not_the_class() {
    let members: [(&[u8], Doc, &str, Range<usize>); 4] = [
        (
            b"class C {\n    /** d */\n    companion object {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "companion_object",
            27..46,
        ),
        (
            b"class C {\n    /** d */\n    constructor() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "secondary_constructor",
            27..43,
        ),
        (
            b"class C {\n    /** d */\n    fun m() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "function_declaration",
            27..37,
        ),
        (
            b"class C {\n    /** d */\n    class N {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "class_declaration",
            27..37,
        ),
    ];
    for (source, document, kind, range) in members {
        assert_eq!(
            view(source),
            owned(document, kind, range),
            "{}",
            source.escape_ascii()
        );
    }
    let mut expected = owned(
        comment(14..24, "audit", [(0..5, Copied(17..22))]),
        "secondary_constructor",
        29..51,
    );
    // The undocumented parameter pins today's pending projection, not its later design.
    expected.assessments = unordered([pending(
        AssessmentAxis::Owner,
        NEEDS_PROJECTION,
        Some(("parameter", 41..47)),
        None,
    )]);
    assert_eq!(
        view(b"class C {\n    /**audit*/\n    constructor(x: Int) {}\n}\n"),
        expected
    );
    let parser = clean(b"enum class E {\n    /** a */\n    A,\n    /** b */\n    B\n}\n");
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        View {
            documents: vec![
                comment(19..27, "a", [(0..1, Copied(23..24))]),
                comment(39..47, "b", [(0..1, Copied(43..44))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owner: ("enum_entry", 32..33),
                },
                Link {
                    document: 1,
                    resolution: Resolution::Resolved,
                    owner: ("enum_entry", 52..53),
                },
            ],
            ..View::default()
        }
    );
    // The control for `check_query`, where no query holds an association: the query of an
    // entry holds the association of its own document and not that of the other entry.
    for index in &both {
        for (at, range) in [32..33, 52..53].into_iter().enumerate() {
            let query = index
                .node_associations(node_at(&parser, "enum_entry", range))
                .unwrap();
            let documents: Vec<_> = query
                .associations()
                .map(|association| index.facts().associated_document(association).unwrap())
                .collect();
            assert_eq!(documents, [index.documents().nth(at).unwrap()]);
            assert!(!query.projection_pending());
            assert_eq!(query.assessments().count(), 0);
        }
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_annotation_in_the_modifier_list_does_not_detach_the_kdoc() {
    let declarations: [(&[u8], Doc, &str, Range<usize>); 3] = [
        (
            b"/**audit*/\n@Deprecated(\"x\") fun f() {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            "function_declaration",
            11..38,
        ),
        (
            b"/** d */\n@Deprecated(\"x\") class C {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..36,
        ),
        (
            b"class C {\n    /** d */\n    @JvmStatic fun m() {}\n}\n",
            comment(14..22, "d", [(0..1, Copied(18..19))]),
            "function_declaration",
            27..48,
        ),
    ];
    for (source, document, kind, range) in declarations {
        assert_eq!(
            view(source),
            owned(document, kind, range),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_last_kdoc_before_a_declaration_wins_and_older_blocks_are_superseded() {
    #[allow(clippy::single_range_in_vec_init)]
    let pairs: [RetainedPair<'_>; 4] = [
        (
            b"/**old*/\n/**new*/\nfun f() {}\n",
            comment(9..17, "new", [(0..3, Copied(12..15))]),
            "function_declaration",
            18..28,
            &[0..8],
        ),
        (
            b"/**old*/\n// plain\n/**new*/\nfun f() {}\n",
            comment(18..26, "new", [(0..3, Copied(21..24))]),
            "function_declaration",
            27..37,
            &[0..8],
        ),
        (
            b"/**old*/\n\n/**new*/\n\nfun f() {}\n",
            comment(10..18, "new", [(0..3, Copied(13..16))]),
            "function_declaration",
            20..30,
            &[0..8],
        ),
        (
            b"/**a*/\n/**b*/\n/**c*/\nclass C {}\n",
            comment(14..20, "c", [(0..1, Copied(17..18))]),
            "class_declaration",
            21..31,
            &[0..6, 7..13],
        ),
    ];
    for (source, document, kind, range, candidates) in pairs {
        let mut expected = owned(document, kind, range);
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        expected.evidence.extend(
            candidates
                .iter()
                .cloned()
                .map(|range| (CandidateDisposition::Superseded, range)),
        );
        assert_eq!(retained(source), expected, "{}", source.escape_ascii());
    }
    for (source, range) in [
        (b"/** d */\n// plain\nfun f() {}\n".as_slice(), 18..28),
        (b"/** d */\n\nfun f() {}\n".as_slice(), 10..20),
    ] {
        assert_eq!(
            view(source),
            owned(
                comment(0..8, "d", [(0..1, Copied(4..5))]),
                "function_declaration",
                range,
            ),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_plain_comments_are_no_documentation() {
    for source in [b"/* d */\nfun f() {}\n".as_slice(), b"// d\nfun f() {}\n"] {
        assert_eq!(view(source), View::default(), "{}", source.escape_ascii());
        assert_eq!(
            retained(source),
            View::default(),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_kdoc_text_over_several_lines() {
    let rows: [(&[u8], Doc, Range<usize>); 5] = [
        (
            b"/**\n * a\n * b\n */\nfun f() {}\n",
            comment(
                0..17,
                "a\nb",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..3, Copied(12..13)),
                ],
            ),
            18..28,
        ),
        (
            b"/**\n * a  \n *   b\n */\nfun f() {}\n",
            comment(
                0..21,
                "a  \n  b",
                [
                    (0..3, Copied(7..10)),
                    (3..4, Copied(10..11)),
                    (4..7, Copied(14..17)),
                ],
            ),
            22..32,
        ),
        (
            b"/**\n * Sum.\n *\n * @return x\n */\nfun f() {}\n",
            comment(
                0..31,
                "Sum.\n\n@return x",
                [
                    (0..4, Copied(7..11)),
                    (4..5, Copied(11..12)),
                    (5..6, Copied(14..15)),
                    (6..15, Copied(18..27)),
                ],
            ),
            32..42,
        ),
        (
            b"/** a\n * b */\nfun f() {}\n",
            comment(
                0..13,
                "a\nb",
                [
                    (0..1, Copied(4..5)),
                    (1..2, Copied(5..6)),
                    (2..3, Copied(9..10)),
                ],
            ),
            14..24,
        ),
        (
            b"class C {\n    /**\n     * a\n     * b\n     */\n    fun m() {}\n}\n",
            comment(
                14..43,
                "a\nb",
                [
                    (0..1, Copied(25..26)),
                    (1..2, Copied(26..27)),
                    (2..3, Copied(34..35)),
                ],
            ),
            48..58,
        ),
    ];
    for (source, document, range) in rows {
        assert_eq!(
            view(source),
            owned(document, "function_declaration", range),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_kdoc_with_crlf_line_endings() {
    assert_eq!(
        view(b"/** d */\r\nfun f() {}\r\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "function_declaration",
            10..20,
        )
    );
    assert_eq!(
        view(b"/**\r\n * a\r\n * b\r\n */\r\nfun f() {}\r\n"),
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
            "function_declaration",
            22..32,
        )
    );
}

#[cfg(feature = "kotlin")]
fn check_query(
    indexes: &[DocumentationIndex<'_>; 2],
    node: tree_sitter::Node<'_>,
    pending_owner: bool,
    expected: &[(AssessmentAxis, &str)],
) {
    let mut expected = expected.to_vec();
    expected.sort_by_key(|(_, reason)| *reason);
    for index in indexes {
        let projection = index.node_projection(node).unwrap();
        assert_eq!(projection.pending(), pending_owner);
        if pending_owner {
            assert_eq!(projection.owners().count(), 0);
        } else {
            let owners: Vec<_> = projection.owners().collect();
            let [owner] = owners[..] else {
                panic!("one owner, found {owners:?}")
            };
            let Owner::Node(declared) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            assert_eq!(declared.node(), node);
        }
        let query = index.node_associations(node).unwrap();
        assert_eq!(query.associations().count(), 0);
        assert_eq!(query.projection_pending(), pending_owner);
        let mut actual: Vec<_> = query
            .assessments()
            .map(|assessment| (assessment.get().axis, assessment.get().reason))
            .collect();
        actual.sort_by_key(|(_, reason)| *reason);
        assert_eq!(actual, expected);
    }
}

/// These documents and assessments pin today's pending forms, not their later owner projection.
#[cfg(feature = "kotlin")]
#[test]
#[allow(clippy::too_many_lines)]
fn kotlin_property_and_parameter_kdoc_is_a_document_with_pending_assessments() {
    assert_eq!(
        view(b"class C {\n    /** d */\n    val p = 1\n}\n"),
        View {
            documents: vec![comment(14..22, "d", [(0..1, Copied(18..19))])],
            assessments: unordered([
                pending(
                    AssessmentAxis::Owner,
                    AWAITS_OWNER,
                    Some(("property_declaration", 27..36)),
                    Some(0),
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("property_declaration", 27..36)),
                    None,
                ),
            ]),
            ..View::default()
        }
    );
    let class_parameter = b"class C(/**audit*/ val x: Int)\n";
    let class_expected = View {
        documents: vec![comment(8..18, "audit", [(0..5, Copied(11..16))])],
        assessments: unordered([
            pending(
                AssessmentAxis::Selection,
                SIGNATURE,
                Some(("block_comment", 8..18)),
                None,
            ),
            pending(
                AssessmentAxis::Owner,
                AWAITS_OWNER,
                Some(("class_parameter", 19..29)),
                Some(0),
            ),
            pending(
                AssessmentAxis::Owner,
                NEEDS_PROJECTION,
                Some(("primary_constructor", 7..30)),
                None,
            ),
            pending(
                AssessmentAxis::Owner,
                NEEDS_PROJECTION,
                Some(("class_parameter", 19..29)),
                None,
            ),
        ]),
        ..View::default()
    };
    let function_parameter = b"fun f(/**p*/ x: Int) {}\n";
    let function_expected = View {
        documents: vec![comment(6..12, "p", [(0..1, Copied(9..10))])],
        assessments: unordered([
            pending(
                AssessmentAxis::Selection,
                SIGNATURE,
                Some(("block_comment", 6..12)),
                None,
            ),
            pending(
                AssessmentAxis::Owner,
                AWAITS_OWNER,
                Some(("parameter", 13..19)),
                Some(0),
            ),
            pending(
                AssessmentAxis::Owner,
                NEEDS_PROJECTION,
                Some(("parameter", 13..19)),
                None,
            ),
        ]),
        ..View::default()
    };

    let parser = clean(b"/** d */\nval x = 1\n");
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        View {
            documents: vec![comment(0..8, "d", [(0..1, Copied(4..5))])],
            assessments: unordered([
                pending(
                    AssessmentAxis::Owner,
                    AWAITS_OWNER,
                    Some(("property_declaration", 9..18)),
                    Some(0)
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("property_declaration", 9..18)),
                    None
                ),
            ]),
            ..View::default()
        }
    );
    check_query(
        &both,
        node_at(&parser, "property_declaration", 9..18),
        true,
        &[
            (AssessmentAxis::Owner, AWAITS_OWNER),
            (AssessmentAxis::Owner, NEEDS_PROJECTION),
        ],
    );

    let parser = clean(class_parameter);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(agreed(&both), class_expected);
    check_query(
        &both,
        node_at(&parser, "class_declaration", 0..30),
        false,
        &[(AssessmentAxis::Selection, SIGNATURE)],
    );
    check_query(
        &both,
        node_at(&parser, "primary_constructor", 7..30),
        true,
        &[
            (AssessmentAxis::Owner, NEEDS_PROJECTION),
            (AssessmentAxis::Selection, SIGNATURE),
        ],
    );
    check_query(
        &both,
        node_at(&parser, "class_parameter", 19..29),
        true,
        &[
            (AssessmentAxis::Owner, AWAITS_OWNER),
            (AssessmentAxis::Owner, NEEDS_PROJECTION),
            (AssessmentAxis::Owner, NEEDS_PROJECTION),
            (AssessmentAxis::Selection, SIGNATURE),
        ],
    );

    let parser = clean(function_parameter);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(agreed(&both), function_expected);
    check_query(
        &both,
        node_at(&parser, "function_declaration", 0..23),
        false,
        &[(AssessmentAxis::Selection, SIGNATURE)],
    );
    check_query(
        &both,
        node_at(&parser, "parameter", 13..19),
        true,
        &[
            (AssessmentAxis::Owner, AWAITS_OWNER),
            (AssessmentAxis::Owner, NEEDS_PROJECTION),
            (AssessmentAxis::Selection, SIGNATURE),
        ],
    );
}

/// These documents and assessments pin today's pending forms, not their later owner projection.
#[cfg(feature = "kotlin")]
#[test]
#[allow(clippy::too_many_lines)]
fn kotlin_kdoc_inside_a_declaration_is_a_document_with_pending_assessments() {
    let rows: [(&[u8], Doc, Vec<Pending>); 3] = [
        (
            b"public /**audit*/ fun f() {}\n",
            comment(7..17, "audit", [(0..5, Copied(10..15))]),
            unordered([pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0))]),
        ),
        (
            b"class C /**audit*/ constructor()\n",
            comment(8..18, "audit", [(0..5, Copied(11..16))]),
            unordered([
                pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0)),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("primary_constructor", 19..32)),
                    None,
                ),
            ]),
        ),
        (
            b"class C {\n    fun f() /**audit*/ {}\n}\n",
            comment(22..32, "audit", [(0..5, Copied(25..30))]),
            unordered([pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0))]),
        ),
    ];
    for (source, document, assessments) in rows {
        assert_eq!(
            view(source),
            View {
                documents: vec![document],
                assessments,
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
    let leading = b"/**first*/\nfun f() /**audit*/ {}\n";
    assert_eq!(
        view(leading),
        View {
            documents: vec![
                comment(0..10, "first", [(0..5, Copied(3..8))]),
                comment(19..29, "audit", [(0..5, Copied(22..27))]),
            ],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Resolved,
                owner: ("function_declaration", 11..32),
            }],
            assessments: unordered([pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(1))]),
            ..View::default()
        }
    );
    for (source, expected, queries) in [
        (
            b"public /**audit*/ inline fun f() {}\n".as_slice(),
            View {
                documents: vec![comment(7..17, "audit", [(0..5, Copied(10..15))])],
                assessments: unordered([
                    pending(
                        AssessmentAxis::Selection,
                        SIGNATURE,
                        Some(("block_comment", 7..17)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0)),
                ]),
                ..View::default()
            },
            vec![(
                "function_declaration",
                0..35,
                false,
                vec![
                    (AssessmentAxis::Selection, SIGNATURE),
                    (AssessmentAxis::Owner, AWAITS_OWNER),
                ],
            )],
        ),
        (
            b"class C private /**audit*/ constructor()\n".as_slice(),
            View {
                documents: vec![comment(16..26, "audit", [(0..5, Copied(19..24))])],
                assessments: unordered([
                    pending(
                        AssessmentAxis::Selection,
                        SIGNATURE,
                        Some(("block_comment", 16..26)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0)),
                    pending(
                        AssessmentAxis::Owner,
                        NEEDS_PROJECTION,
                        Some(("primary_constructor", 8..40)),
                        None,
                    ),
                ]),
                ..View::default()
            },
            vec![
                (
                    "class_declaration",
                    0..40,
                    false,
                    vec![
                        (AssessmentAxis::Selection, SIGNATURE),
                        (AssessmentAxis::Owner, AWAITS_OWNER),
                    ],
                ),
                (
                    "primary_constructor",
                    8..40,
                    true,
                    vec![
                        (AssessmentAxis::Owner, NEEDS_PROJECTION),
                        (AssessmentAxis::Selection, SIGNATURE),
                        (AssessmentAxis::Owner, AWAITS_OWNER),
                    ],
                ),
            ],
        ),
        (
            b"fun f() /**audit*/ {}\n".as_slice(),
            View {
                documents: vec![comment(8..18, "audit", [(0..5, Copied(11..16))])],
                assessments: unordered([pending(
                    AssessmentAxis::Owner,
                    AWAITS_OWNER,
                    None,
                    Some(0),
                )]),
                ..View::default()
            },
            vec![(
                "function_declaration",
                0..21,
                false,
                vec![(AssessmentAxis::Owner, AWAITS_OWNER)],
            )],
        ),
    ] {
        let parser = clean(source);
        let both = indexes(&parser, DocumentationOptions::default());
        assert_eq!(agreed(&both), expected);
        for (kind, range, pending_owner, assessments) in queries {
            check_query(
                &both,
                node_at(&parser, kind, range),
                pending_owner,
                &assessments,
            );
        }
    }
}

#[cfg(feature = "kotlin")]
#[test]
#[allow(clippy::too_many_lines)]
fn kotlin_grammar_shapes_the_unit_relies_on() {
    let parser = grammar_parse(b"/**audit*/\nfun f() {}\n", false);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("source_file", 0..22));
    assert_eq!(
        parts(root),
        [
            (None, "block_comment", 0..10),
            (None, "function_declaration", 11..21)
        ]
    );
    assert!(node_at(&parser, "block_comment", 0..10).is_extra());

    let parser = grammar_parse(
        b"class C {}\nobject O {}\ntypealias T = Int\nfun f() {}\nval x = 1\n",
        false,
    );
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "class_declaration", 0..10),
            (None, "object_declaration", 11..22),
            (None, "type_alias", 23..40),
            (None, "function_declaration", 41..51),
            (None, "property_declaration", 52..61),
        ]
    );

    let parser = grammar_parse(
        b"class C {\n    companion object {}\n    constructor() {}\n    fun m() {}\n    \
          class N {}\n    val p = 1\n}\n",
        false,
    );
    assert_eq!(
        parts(node_at(&parser, "class_body", 8..100)),
        [
            (None, "{", 8..9),
            (None, "companion_object", 14..33),
            (None, "secondary_constructor", 38..54),
            (None, "function_declaration", 59..69),
            (None, "class_declaration", 74..84),
            (None, "property_declaration", 89..98),
            (None, "}", 99..100),
        ]
    );

    let parser = grammar_parse(
        b"enum class E {\n    /** a */\n    A,\n    /** b */\n    B\n}\n",
        false,
    );
    let body = node_at(&parser, "enum_class_body", 13..55);
    assert_eq!(
        parts(body),
        [
            (None, "{", 13..14),
            (None, "block_comment", 19..27),
            (None, "enum_entry", 32..33),
            (None, ",", 33..34),
            (None, "block_comment", 39..47),
            (None, "enum_entry", 52..53),
            (None, "}", 54..55),
        ]
    );
    assert!(!body.child(3).unwrap().is_named());

    let parser = grammar_parse(b"/**audit*/\n@Deprecated(\"x\") fun f() {}\n", false);
    let root = parser.ts_tree().root_node();
    let comment = node_at(&parser, "block_comment", 0..10);
    let function = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(root));
    assert_eq!(site(function), ("function_declaration", 11..38));
    let modifiers = function.child(0).unwrap();
    assert_eq!(site(modifiers), ("modifiers", 11..27));
    assert_eq!(parts(modifiers), [(None, "annotation", 11..27)]);

    let parser = grammar_parse(b"class C(/**audit*/ val x: Int)\n", false);
    let class = node_at(&parser, "class_declaration", 0..30);
    let constructor = node_at(&parser, "primary_constructor", 7..30);
    let parameters = node_at(&parser, "class_parameters", 7..30);
    assert_eq!(constructor.parent(), Some(class));
    assert_eq!(parameters.parent(), Some(constructor));
    assert_eq!(
        parts(parameters),
        [
            (None, "(", 7..8),
            (None, "block_comment", 8..18),
            (None, "class_parameter", 19..29),
            (None, ")", 29..30),
        ]
    );
    assert!(
        parts(node_at(&parser, "class_parameter", 19..29))
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );

    let parser = grammar_parse(b"fun f(/**p*/ x: Int) {}\n", false);
    let parameters = node_at(&parser, "function_value_parameters", 5..20);
    assert_eq!(
        parts(parameters),
        [
            (None, "(", 5..6),
            (None, "block_comment", 6..12),
            (None, "parameter", 13..19),
            (None, ")", 19..20),
        ]
    );
    assert!(
        parts(node_at(&parser, "parameter", 13..19))
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );

    let parser = grammar_parse(b"public /**audit*/ inline fun f() {}\n", false);
    let function = node_at(&parser, "function_declaration", 0..35);
    let modifiers = function.child(0).unwrap();
    assert_eq!(site(modifiers), ("modifiers", 0..24));
    assert_eq!(
        parts(modifiers),
        [
            (None, "visibility_modifier", 0..6),
            (None, "block_comment", 7..17),
            (None, "function_modifier", 18..24),
        ]
    );

    let parser = grammar_parse(b"public /**audit*/ fun f() {}\n", false);
    let function = node_at(&parser, "function_declaration", 0..28);
    assert_eq!(
        parts(function)[..3],
        [
            (None, "modifiers", 0..6),
            (None, "block_comment", 7..17),
            (None, "fun", 18..21),
        ]
    );
    assert!(!function.child(2).unwrap().is_named());
    assert!(
        parts(function.child(0).unwrap())
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );

    let parser = grammar_parse(b"class C private /**audit*/ constructor()\n", false);
    let constructor = node_at(&parser, "primary_constructor", 8..40);
    assert_eq!(
        parts(constructor),
        [
            (None, "modifiers", 8..15),
            (None, "block_comment", 16..26),
            (None, "constructor", 27..38),
            (None, "class_parameters", 38..40),
        ]
    );
    assert!(!constructor.child(2).unwrap().is_named());

    let parser = grammar_parse(b"class C /**audit*/ constructor()\n", false);
    let class = node_at(&parser, "class_declaration", 0..32);
    assert_eq!(
        parts(class)[2..4],
        [
            (None, "block_comment", 8..18),
            (None, "primary_constructor", 19..32),
        ]
    );
    assert!(
        parts(node_at(&parser, "primary_constructor", 19..32))
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );

    let parser = grammar_parse(b"fun f() /**audit*/ {}\n", false);
    assert_eq!(
        parts(node_at(&parser, "function_declaration", 0..21))[2..5],
        [
            (None, "function_value_parameters", 5..7),
            (None, "block_comment", 8..18),
            (None, "function_body", 19..21),
        ]
    );

    let parser = grammar_parse(b"/* a */\n// b\n/** c */\nfun f() {}\n", false);
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "block_comment", 0..7),
            (None, "line_comment", 8..12),
            (None, "block_comment", 13..21),
            (None, "function_declaration", 22..32),
        ]
    );
    let source = b"/**\r\n * a\r\n * b\r\n */\r\nfun f() {}\r\n";
    let parser = grammar_parse(source, false);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("source_file", 0..34));
    assert_eq!(
        parts(root),
        [
            (None, "block_comment", 0..20),
            (None, "function_declaration", 22..32),
        ]
    );
    let comment = node_at(&parser, "block_comment", 0..20);
    assert_eq!(comment.end_position().row - comment.start_position().row, 3);
    assert_eq!(
        source[comment.byte_range()]
            .windows(2)
            .filter(|pair| *pair == b"\r\n")
            .count(),
        3
    );
    let function = node_at(&parser, "function_declaration", 22..32);
    assert_eq!(&source[comment.end_byte()..function.start_byte()], b"\r\n");

    let parser = grammar_parse(
        b"fun g() {}\n/** d */\n@Suppress(\"x\")\nfun f() {}\n",
        false,
    );
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "function_declaration", 0..10),
            (None, "block_comment", 11..19),
            (None, "annotated_expression", 20..34),
            (None, "function_declaration", 35..45),
        ]
    );
    let parser = grammar_parse(b"/** d */\ncontext(Foo)\nfun f() {}\n", false);
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "block_comment", 0..8),
            (None, "call_expression", 9..21),
            (None, "function_declaration", 22..32),
        ]
    );
    let parser = grammar_parse(
        b"class C {\n    val q: Int\n        /** g */\n        get() = 1\n}\n",
        false,
    );
    let property = node_at(&parser, "property_declaration", 14..59);
    assert_eq!(
        parts(property),
        [
            (None, "val", 14..17),
            (None, "variable_declaration", 18..24),
            (None, "block_comment", 33..41),
            (None, "getter", 50..59),
        ]
    );
    assert!(
        parts(node_at(&parser, "getter", 50..59))
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );
    let parser = grammar_parse(b"@Deprecated(\"x\")\n/** d */\nfun f() {}\n", false);
    let function = node_at(&parser, "function_declaration", 0..36);
    assert_eq!(
        parts(function)[..3],
        [
            (None, "modifiers", 0..16),
            (None, "block_comment", 17..25),
            (None, "fun", 26..29),
        ]
    );
    assert!(!function.child(2).unwrap().is_named());
    assert!(
        parts(function.child(0).unwrap())
            .iter()
            .all(|(_, kind, _)| *kind != "block_comment")
    );

    // The pinned grammar flags an error for a class body whose member shares the row of the
    // closing brace, here and in the source after this loop. No source-view test collects such
    // a source; the flag is pinned so that a grammar bump that removes it fails this test.
    for (source, error) in [
        (
            b"class C { /**audit*/ constructor(x: Int) {} }\n".as_slice(),
            true,
        ),
        (
            b"class C { /**audit*/ constructor(x: Int) {}\n}\n".as_slice(),
            false,
        ),
    ] {
        let parser = grammar_parse(source, error);
        let class = node_at(&parser, "class_declaration", 0..45);
        let body = node_at(&parser, "class_body", 8..45);
        assert_eq!(class.has_error(), error);
        assert_eq!(body.has_error(), error);
        assert_eq!(
            parts(body),
            [
                (None, "{", 8..9),
                (None, "block_comment", 10..20),
                (None, "secondary_constructor", 21..43),
                (None, "}", 44..45),
            ]
        );
        assert!(parser.root_node().preorder().all(|node| {
            let node = node.as_tree_sitter();
            !node.is_error() && !node.is_missing() && (error || !node.has_error())
        }));
        assert!(
            body.children(&mut body.walk())
                .all(|child| !child.has_error())
        );
    }
    let parser = grammar_parse(b"class C { fun m() {} }\n", true);
    let body = node_at(&parser, "enum_class_body", 8..22);
    assert_eq!(
        parts(body),
        [
            (None, "{", 8..9),
            (None, "ERROR", 10..20),
            (None, "}", 21..22),
        ]
    );
    let error = node_at(&parser, "ERROR", 10..20);
    assert!(error.is_error());
    assert_eq!(parts(error), [(None, "function_declaration", 10..20)]);
    assert_eq!(
        parser
            .root_node()
            .preorder()
            .filter(|node| node.as_tree_sitter().is_error())
            .count(),
        1
    );
    assert!(
        parser
            .root_node()
            .preorder()
            .all(|node| !node.as_tree_sitter().is_missing())
    );
}
