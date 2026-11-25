//! PHP DocBlock source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the parse-tree shapes the source-view tests rely on. Node queries are not in the view.

#[cfg(feature = "php")]
use crate::documentation::*;
#[cfg(feature = "php")]
use crate::{AnyParser, LANG};
#[cfg(feature = "php")]
use std::ops::Range;

#[cfg(feature = "php")]
use Run::{Copied, Decoded};

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    Node(&'static str, Range<usize>),
    File(Range<usize>),
}

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    resolution: Resolution,
    owner: Holder,
}

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and pending assessments from a collection.
/// Assessment order is unspecified; shared values are asserted in `published`. Node queries
/// are not included. An empty view has nothing published and nothing pending.
#[cfg(feature = "php")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Link>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "php")]
const AWAITS_OWNER: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "php")]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";

#[cfg(feature = "php")]
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

#[cfg(feature = "php")]
fn file(document: Doc) -> Doc {
    Doc {
        role: DocumentRole::FilePackage,
        ..document
    }
}

#[cfg(feature = "php")]
fn owned(document: Doc, owner: Holder) -> View {
    View {
        documents: vec![document],
        associations: vec![Link {
            document: 0,
            resolution: Resolution::Resolved,
            owner,
        }],
        ..View::default()
    }
}

#[cfg(feature = "php")]
fn node_owner(document: Doc, kind: &'static str, range: Range<usize>) -> View {
    owned(document, Holder::Node(kind, range))
}

#[cfg(feature = "php")]
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

#[cfg(feature = "php")]
fn owner_pending(kind: &'static str, range: Range<usize>) -> Vec<Pending> {
    unordered([
        pending(
            AssessmentAxis::Owner,
            AWAITS_OWNER,
            Some((kind, range.clone())),
            Some(0),
        ),
        pending(
            AssessmentAxis::Owner,
            NEEDS_PROJECTION,
            Some((kind, range)),
            None,
        ),
    ])
}

#[cfg(feature = "php")]
fn unordered(entries: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = entries.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "php")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "php")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "php")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("comment", range.clone()))
    );
    range
}

#[cfg(feature = "php")]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "php")]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
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
            assert_eq!(attachment.convention.name(), "BcaPhpSource");
            assert_eq!(attachment.publication, Publication::Unknown);
            assert_eq!(attachment.resolution, Resolution::Resolved);
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owner = facts.associated_owner(association, 0).unwrap();
            let holder = match owner.get() {
                Owner::Node(declared) => {
                    assert_eq!(
                        attachment.binding.map(NodeEvidence::node),
                        Some(declared.node())
                    );
                    let (kind, range) = site(declared.node());
                    Holder::Node(kind, range)
                }
                Owner::File(file) => {
                    assert_eq!(file.node(), root);
                    assert!(attachment.binding.is_none());
                    Holder::File(file.node().byte_range())
                }
                other => panic!("a node or file owner, found {other:?}"),
            };
            Link {
                document: ordered.iter().position(|known| *known == document).unwrap(),
                resolution: attachment.resolution,
                owner: holder,
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
    let assessments = unordered(index.assessments().map(|assessment| {
        Pending {
            axis: assessment.get().axis,
            reason: assessment.get().reason,
            node: assessment.get().node.map(|node| site(node.node())),
            document: index
                .assessed_document(assessment)
                .unwrap()
                .map(|document| ordered.iter().position(|known| *known == document).unwrap()),
        }
    }));
    // The PHP source adapter is partial for all these cases.
    assert_eq!(index.capability(), Capability::Partial);
    // Qualifications belong to named-reference commands (the Doxygen path), absent here.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "php")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Php, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        source.escape_ascii()
    );
    parser
}

#[cfg(feature = "php")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Php,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "php")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2], root: tree_sitter::Node<'_>) -> View {
    let [typed, raw] = indexes.each_ref().map(|index| published(index, root));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "php")]
fn view(source: &[u8]) -> View {
    let parser = clean(source);
    agreed(
        &indexes(&parser, DocumentationOptions::default()),
        parser.ts_tree().root_node(),
    )
}

#[cfg(feature = "php")]
fn retained(source: &[u8]) -> View {
    let parser = clean(source);
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&parser, options), parser.ts_tree().root_node())
}

#[cfg(feature = "php")]
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

#[cfg(feature = "php")]
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

#[cfg(feature = "php")]
#[test]
fn php_leading_docblock_documents_the_declaration_after_it() {
    let declarations: [(&[u8], &str, Range<usize>); 5] = [
        (
            b"<?php\n$x = 0;\n/** d */\nfunction f() {}\n",
            "function_definition",
            23..38,
        ),
        (
            b"<?php\n$x = 0;\n/** d */\nclass C {}\n",
            "class_declaration",
            23..33,
        ),
        (
            b"<?php\n$x = 0;\n/** d */\ninterface I {}\n",
            "interface_declaration",
            23..37,
        ),
        (
            b"<?php\n$x = 0;\n/** d */\ntrait T {}\n",
            "trait_declaration",
            23..33,
        ),
        (
            b"<?php\n$x = 0;\n/** d */\nenum E { case A; }\n",
            "enum_declaration",
            23..41,
        ),
    ];
    for (source, kind, range) in declarations {
        assert_eq!(
            view(source),
            node_owner(comment(14..22, "d", [(0..1, Copied(18..19))]), kind, range),
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"<?php\nenum E {\n    /** d */\n    case A;\n}\n"),
        node_owner(
            comment(19..27, "d", [(0..1, Copied(23..24))]),
            "enum_case",
            32..39
        )
    );
    assert_eq!(
        view(b"<?php\nclass C {\n    /** d */\n    function m() {}\n}\n"),
        node_owner(
            comment(20..28, "d", [(0..1, Copied(24..25))]),
            "method_declaration",
            33..48
        )
    );
}

#[cfg(feature = "php")]
#[test]
fn php_first_file_rule_before_a_class_a_function_and_a_namespace() {
    let single: [(&[u8], Doc, &str, Range<usize>); 2] = [
        (
            b"<?php\n/** one */\nclass C {}\n",
            comment(6..16, "one", [(0..3, Copied(10..13))]),
            "class_declaration",
            17..27,
        ),
        (
            b"<?php\n/** audit */\nfunction f() {}\n",
            comment(6..18, "audit", [(0..5, Copied(10..15))]),
            "function_definition",
            19..34,
        ),
    ];
    for (source, document, kind, range) in single {
        assert_eq!(
            view(source),
            node_owner(document, kind, range),
            "{}",
            source.escape_ascii()
        );
    }
    let two = [
        (
            &b"<?php\n/** file */\n/** item */\nclass C {}\n"[..],
            "class_declaration",
            30..40,
            0..41,
        ),
        (
            &b"<?php\n/** file */\n/** item */\nfunction f() {}\n"[..],
            "function_definition",
            30..45,
            0..46,
        ),
    ];
    for (source, kind, range, root) in two {
        assert_eq!(
            view(source),
            View {
                documents: vec![
                    file(comment(6..17, "file", [(0..4, Copied(10..14))])),
                    comment(18..29, "item", [(0..4, Copied(22..26))]),
                ],
                associations: vec![
                    Link {
                        document: 0,
                        resolution: Resolution::Resolved,
                        owner: Holder::File(root)
                    },
                    Link {
                        document: 1,
                        resolution: Resolution::Resolved,
                        owner: Holder::Node(kind, range),
                    },
                ],
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
    let three: [(&[u8], &str, Range<usize>); 2] = [
        (
            b"<?php\n/** a */\n/** b */\n/** c */\nclass C {}\n",
            "class_declaration",
            33..43,
        ),
        (
            b"<?php\n/** a */\n/** b */\n/** c */\nfunction f() {}\n",
            "function_definition",
            33..48,
        ),
    ];
    for (source, kind, range) in three {
        let mut expected = node_owner(comment(24..32, "c", [(0..1, Copied(28..29))]), kind, range);
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        expected.evidence.extend([
            (CandidateDisposition::Superseded, 6..14),
            (CandidateDisposition::Superseded, 15..23),
        ]);
        assert_eq!(retained(source), expected, "{}", source.escape_ascii());
    }
    assert_eq!(
        view(b"<?php\n/** audit */\nnamespace N;\n"),
        owned(
            file(comment(6..18, "audit", [(0..5, Copied(10..15))])),
            Holder::File(0..32)
        )
    );
}

/// The `define()` document without an owner pins today's pending outcome, not the later design.
#[cfg(feature = "php")]
#[test]
fn php_one_docblock_before_a_first_enum_or_define_goes_to_the_declaration() {
    assert_eq!(
        view(b"<?php\n/** audit */\nenum E { case A; }\n"),
        node_owner(
            comment(6..18, "audit", [(0..5, Copied(10..15))]),
            "enum_declaration",
            19..37
        )
    );
    assert_eq!(
        view(b"<?php\n/** audit */\ndefine('X', 1);\n"),
        View {
            documents: vec![comment(6..18, "audit", [(0..5, Copied(10..15))])],
            assessments: owner_pending("expression_statement", 19..34),
            ..View::default()
        }
    );
}

#[cfg(feature = "php")]
#[test]
fn php_attribute_between_docblock_and_declaration_does_not_detach() {
    let declarations = [
        (
            &b"<?php\n$x = 0;\n/** d */\n#[A]\nclass C {}\n"[..],
            14..22,
            18..19,
            "class_declaration",
            23..38,
        ),
        (
            &b"<?php\n$x = 0;\n/** d */\n#[A]\nfunction f() {}\n"[..],
            14..22,
            18..19,
            "function_definition",
            23..43,
        ),
        (
            &b"<?php\nclass C {\n    /** d */\n    #[A]\n    function m() {}\n}\n"[..],
            20..28,
            24..25,
            "method_declaration",
            33..57,
        ),
    ];
    for (source, fragment, text, kind, range) in declarations {
        assert_eq!(
            view(source),
            node_owner(comment(fragment, "d", [(0..1, Copied(text))]), kind, range),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "php")]
#[test]
fn php_plain_comments_are_no_documentation() {
    let plain: [&[u8]; 4] = [
        b"<?php\n$x = 0;\n/* d */\nfunction f() {}\n",
        b"<?php\n$x = 0;\n// d\nfunction f() {}\n",
        b"<?php\n$x = 0;\n# d\nfunction f() {}\n",
        b"<?php\n$x = 0;\n/* a */\n// b\n# c\nfunction f() {}\n",
    ];
    for source in plain {
        assert_eq!(view(source), View::default(), "{}", source.escape_ascii());
        assert_eq!(
            retained(source),
            View::default(),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "php")]
#[test]
fn php_docblock_in_a_function_body() {
    let source = b"<?php\nfunction f() {\n    /** note */\n    return 1;\n}\n";
    let mut expected = View::default();
    assert_eq!(view(source), expected);
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 25..36));
    assert_eq!(retained(source), expected);
    assert_eq!(
        view(b"<?php\nfunction f() {\n    /** d */\n    function g() {}\n}\n"),
        node_owner(
            comment(25..33, "d", [(0..1, Copied(29..30))]),
            "function_definition",
            38..53
        )
    );
}

#[cfg(feature = "php")]
#[test]
fn php_docblock_text_over_several_lines() {
    let rows: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"<?php\n$x = 0;\n/**\n * a\n * b\n */\nfunction f() {}\n",
            comment(
                14..31,
                "a\nb",
                [
                    (0..1, Copied(21..22)),
                    (1..2, Copied(22..23)),
                    (2..3, Copied(26..27)),
                ],
            ),
            32..47,
        ),
        (
            b"<?php\n$x = 0;\n/**\n * a  \n *   b\n */\nfunction f() {}\n",
            comment(
                14..35,
                "a  \n  b",
                [
                    (0..3, Copied(21..24)),
                    (3..4, Copied(24..25)),
                    (4..7, Copied(28..31)),
                ],
            ),
            36..51,
        ),
        (
            b"<?php\n$x = 0;\n/**\n * Sum.\n *\n * @return int\n */\nfunction f() {}\n",
            comment(
                14..47,
                "Sum.\n\n@return int",
                [
                    (0..4, Copied(21..25)),
                    (4..5, Copied(25..26)),
                    (5..6, Copied(28..29)),
                    (6..17, Copied(32..43)),
                ],
            ),
            48..63,
        ),
        (
            b"<?php\n$x = 0;\n/** a\n * b */\nfunction f() {}\n",
            comment(
                14..27,
                "a\nb",
                [
                    (0..1, Copied(18..19)),
                    (1..2, Copied(19..20)),
                    (2..3, Copied(23..24)),
                ],
            ),
            28..43,
        ),
    ];
    for (source, document, range) in rows {
        assert_eq!(
            view(source),
            node_owner(document, "function_definition", range),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "php")]
#[test]
fn php_docblock_with_crlf_line_endings() {
    assert_eq!(
        view(b"<?php\r\n$x = 0;\r\n/** d */\r\nfunction f() {}\r\n"),
        node_owner(
            comment(16..24, "d", [(0..1, Copied(20..21))]),
            "function_definition",
            26..41
        )
    );
    assert_eq!(
        view(b"<?php\r\n$x = 0;\r\n/**\r\n * a\r\n * b\r\n */\r\nfunction f() {}\r\n"),
        node_owner(
            comment(
                16..36,
                "a\nb",
                [
                    (0..1, Copied(24..25)),
                    (1..2, Decoded(25..27)),
                    (2..3, Copied(30..31)),
                ]
            ),
            "function_definition",
            38..53
        )
    );
}

#[cfg(feature = "php")]
#[test]
fn php_inline_html_before_the_opening_tag_is_preamble() {
    assert_eq!(
        view(b"<html>\n<?php\n/** d */\nfunction f() {}\n"),
        node_owner(
            comment(13..21, "d", [(0..1, Copied(17..18))]),
            "function_definition",
            22..37
        )
    );
    assert_eq!(
        view(b"<html>\n<?php\n/** file */\n/** item */\nfunction f() {}\n"),
        View {
            documents: vec![
                file(comment(13..24, "file", [(0..4, Copied(17..21))])),
                comment(25..36, "item", [(0..4, Copied(29..33))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owner: Holder::File(0..53)
                },
                Link {
                    document: 1,
                    resolution: Resolution::Resolved,
                    owner: Holder::Node("function_definition", 37..52),
                },
            ],
            ..View::default()
        }
    );
}

/// These documents without owners pin today's pending outcome; a later unit changes it.
#[cfg(feature = "php")]
#[test]
fn php_property_constant_and_promoted_parameter_keep_a_document_without_an_owner() {
    let members: [(&[u8], &str, Range<usize>); 2] = [
        (
            b"<?php\nclass C {\n    /** d */\n    public $p;\n}\n",
            "property_declaration",
            33..43,
        ),
        (
            b"<?php\nclass C {\n    /** d */\n    const K = 1;\n}\n",
            "const_declaration",
            33..45,
        ),
    ];
    for (source, kind, range) in members {
        assert_eq!(
            view(source),
            View {
                documents: vec![comment(20..28, "d", [(0..1, Copied(24..25))])],
                assessments: owner_pending(kind, range),
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"<?php class C { function __construct(/** audit */ public int $x) {} }\n"),
        View {
            documents: vec![comment(37..49, "audit", [(0..5, Copied(41..46))])],
            assessments: unordered([
                pending(
                    AssessmentAxis::Selection,
                    "PHP signature/promoted slot awaits owner projection",
                    Some(("comment", 37..49)),
                    None,
                ),
                pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0)),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("property_promotion_parameter", 50..63)),
                    None,
                ),
            ]),
            ..View::default()
        }
    );
}

#[cfg(feature = "php")]
#[test]
#[allow(clippy::too_many_lines)]
fn php_grammar_shapes_the_unit_relies_on() {
    let source = b"<?php\n/** file */\n/** item */\nclass C {}\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(source.len(), 41);
    assert_eq!(site(root), ("program", 0..41));
    assert_eq!(
        parts(root),
        [
            (None, "php_tag", 0..5),
            (None, "comment", 6..17),
            (None, "comment", 18..29),
            (None, "class_declaration", 30..40),
        ]
    );
    assert!(node_at(&parser, "comment", 6..17).is_extra());
    assert!(node_at(&parser, "comment", 18..29).is_extra());
    assert_eq!(
        &source[node_at(&parser, "class_declaration", 30..40).end_byte()..],
        b"\n"
    );

    let parser = clean(
        b"<?php\nfunction f() {}\nclass C {}\ninterface I {}\ntrait T {}\nenum E { case A; }\n",
    );
    let root = parser.ts_tree().root_node();
    assert_eq!(
        parts(root),
        [
            (None, "php_tag", 0..5),
            (None, "function_definition", 6..21),
            (None, "class_declaration", 22..32),
            (None, "interface_declaration", 33..47),
            (None, "trait_declaration", 48..58),
            (None, "enum_declaration", 59..77),
        ]
    );
    let enum_body = node_at(&parser, "enum_declaration", 59..77)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(enum_body), ("enum_declaration_list", 66..77));
    assert_eq!(
        node_at(&parser, "enum_case", 68..75).parent(),
        Some(enum_body)
    );

    let parser =
        clean(b"<?php\nclass C {\n    public $p;\n    const K = 1;\n    function m() {}\n}\n");
    let body = node_at(&parser, "class_declaration", 6..69)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(body), ("declaration_list", 14..69));
    assert_eq!(
        parts(body),
        [
            (None, "{", 14..15),
            (None, "property_declaration", 20..30),
            (None, "const_declaration", 35..47),
            (None, "method_declaration", 52..67),
            (None, "}", 68..69),
        ]
    );
    let property = node_at(&parser, "property_element", 27..29);
    assert_eq!(
        property.parent().map(site),
        Some(("property_declaration", 20..30))
    );
    assert_eq!(
        property.child_by_field_name("name").map(site),
        Some(("variable_name", 27..29))
    );
    let constant = node_at(&parser, "const_element", 41..46);
    assert_eq!(
        constant.parent().map(site),
        Some(("const_declaration", 35..47))
    );
    assert_eq!(parts(constant)[0], (None, "name", 41..42));

    let parser = clean(b"<?php\n/** d */\n#[A]\nclass C {}\n");
    let root = parser.ts_tree().root_node();
    let comment = node_at(&parser, "comment", 6..14);
    let class = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(root));
    assert_eq!(site(class), ("class_declaration", 15..30));
    assert_eq!(
        class.child_by_field_name("attributes").map(site),
        Some(("attribute_list", 15..19))
    );
    assert_eq!(class.child(0).map(site), Some(("attribute_list", 15..19)));

    let parser = clean(b"<?php\n#[A]\n/** d */\nfunction f() {}\n");
    let function = node_at(&parser, "function_definition", 6..35);
    assert_eq!(
        parts(function)[..3],
        [
            (Some("attributes"), "attribute_list", 6..10),
            (None, "comment", 11..19),
            (None, "function", 20..28),
        ]
    );
    assert!(function.child(1).unwrap().is_extra());
    assert!(!function.child(2).unwrap().is_named());

    let parser = clean(b"<?php\nclass C { #[Attr]\n/** d */\npublic int $p = 1; }\n");
    let property = node_at(&parser, "property_declaration", 16..51);
    assert_eq!(
        parts(property)[..3],
        [
            (Some("attributes"), "attribute_list", 16..23),
            (None, "comment", 24..32),
            (None, "visibility_modifier", 33..39),
        ]
    );
    assert!(property.child(1).unwrap().is_extra());

    let parser = clean(b"<?php class C { function __construct(/** audit */ public int $x) {} }\n");
    let method = node_at(&parser, "method_declaration", 16..67);
    let parameters = method.child_by_field_name("parameters").unwrap();
    assert_eq!(site(parameters), ("formal_parameters", 36..64));
    let comment = node_at(&parser, "comment", 37..49);
    let promoted = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(parameters));
    assert_eq!(site(promoted), ("property_promotion_parameter", 50..63));
    assert_eq!(promoted.parent(), Some(parameters));
    assert!(
        promoted
            .children(&mut promoted.walk())
            .all(|child| child.kind() != "comment")
    );

    let parser = clean(b"<?php\n/** audit */\ndefine('X', 1);\n");
    let expression = node_at(&parser, "expression_statement", 19..34);
    let call = node_at(&parser, "function_call_expression", 19..33);
    assert_eq!(call.parent(), Some(expression));
    assert_eq!(
        call.child_by_field_name("function").map(site),
        Some(("name", 19..25))
    );

    let parser = clean(b"<?php\n/** audit */\nnamespace N;\n");
    let root = parser.ts_tree().root_node();
    assert_eq!(node_at(&parser, "comment", 6..18).parent(), Some(root));
    assert_eq!(
        node_at(&parser, "namespace_definition", 19..31).parent(),
        Some(root)
    );

    let source = b"<?php\n/* a */\n// b\n# c\n/** d */\nfunction f() {}\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(
        parts(root),
        [
            (None, "php_tag", 0..5),
            (None, "comment", 6..13),
            (None, "comment", 14..18),
            (None, "comment", 19..22),
            (None, "comment", 23..31),
            (None, "function_definition", 32..47),
        ]
    );
    for range in [6..13, 14..18, 19..22, 23..31] {
        assert_eq!(node_at(&parser, "comment", range).parent(), Some(root));
    }
    for range in [14..18, 19..22] {
        assert_eq!(source[node_at(&parser, "comment", range).end_byte()], b'\n');
    }

    let parser = clean(b"<?php\nfunction f() {\n    /** note */\n    return 1;\n}\n");
    let function = node_at(&parser, "function_definition", 6..52);
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(site(body), ("compound_statement", 19..52));
    let comment = node_at(&parser, "comment", 25..36);
    assert_eq!(comment.parent(), Some(body));
    assert_eq!(
        comment.next_sibling().map(site),
        Some(("return_statement", 41..50))
    );

    let source = b"<html>\n<?php\n/** d */\nfunction f() {}\n?>\n<p>\n<?php\nfunction g() {}\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("program", 0..67));
    assert_eq!(
        parts(root),
        [
            (None, "text", 0..7),
            (None, "php_tag", 7..12),
            (None, "comment", 13..21),
            (None, "function_definition", 22..37),
            (None, "text_interpolation", 38..50),
            (None, "function_definition", 51..66),
        ]
    );
    assert_eq!(source[node_at(&parser, "text", 0..7).end_byte() - 1], b'\n');
    let interpolation = node_at(&parser, "text_interpolation", 38..50);
    assert!(interpolation.is_extra());
    assert_eq!(
        parts(interpolation),
        [
            (None, "php_end_tag", 38..40),
            (None, "text", 41..45),
            (None, "php_tag", 45..50),
        ]
    );

    let source = b"<?php\r\n$x = 0;\r\n/**\r\n * a\r\n * b\r\n */\r\nfunction f() {}\r\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("program", 0..55));
    assert_eq!(
        parts(root),
        [
            (None, "php_tag", 0..5),
            (None, "expression_statement", 7..14),
            (None, "comment", 16..36),
            (None, "function_definition", 38..53),
        ]
    );
    let comment = node_at(&parser, "comment", 16..36);
    assert_eq!(comment.end_position().row - comment.start_position().row, 3);
    assert_eq!(
        source[comment.byte_range()]
            .windows(2)
            .filter(|pair| *pair == b"\r\n")
            .count(),
        3
    );
    let function = node_at(&parser, "function_definition", 38..53);
    assert_eq!(&source[comment.end_byte()..function.start_byte()], b"\r\n");
    assert_eq!(&source[function.end_byte()..root.end_byte()], b"\r\n");
}
