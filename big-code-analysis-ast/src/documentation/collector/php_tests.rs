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
    Slot(SlotRole, usize, Range<usize>, Range<usize>),
}

#[cfg(feature = "php")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
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
        associations: vec![Link { document: 0, owner }],
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
    // bca: suppress(halstead) -- one reading of the four published lists and the rules they share.
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
                Owner::Slot(slot) => {
                    let enclosing = facts.slot_enclosing(owner).unwrap();
                    let Owner::Node(declaration) = enclosing.get() else {
                        panic!("node enclosure")
                    };
                    assert_eq!(
                        attachment.binding.map(NodeEvidence::node),
                        Some(declaration.node())
                    );
                    Holder::Slot(
                        slot.role,
                        slot.ordinal,
                        span(&slot.source),
                        declaration.node().byte_range(),
                    )
                }
                other => panic!("a node, slot or file owner, found {other:?}"),
            };
            Link {
                document: ordered.iter().position(|known| *known == document).unwrap(),
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
                        owner: Holder::File(root),
                    },
                    Link {
                        document: 1,
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
                    owner: Holder::File(0..53),
                },
                Link {
                    document: 1,
                    owner: Holder::Node("function_definition", 37..52),
                },
            ],
            ..View::default()
        }
    );
}

/// A promoted parameter stays pending; property and constant declarations now get slots.
#[cfg(feature = "php")]
#[test]
fn php_promoted_parameter_keeps_a_document_without_an_owner() {
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
    assert_eq!(site(root), ("program", 0..41));
    assert_eq!(root.end_byte(), source.len());
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

#[cfg(feature = "php")]
#[test]
#[allow(clippy::too_many_lines)]
fn php_unit2_single_property_and_constant_slots() {
    let rows: &[(&[u8], SlotRole, &str, &str)] = &[
        (
            b"<?php\nclass C {\n/** d */\npublic int $x = 1;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\nconst K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nclass C {\n/** d */\nconst B = A;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "B",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic $x = $y;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\ninterface I {\n/** d */\nconst K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nenum E {\ncase A;\n/** d */\nconst K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\ntrait T {\n/** d */\nconst K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nfunction z() {}\n/** d */\nconst K = 1;\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nfunction z() {}\n/** d */\nconst K = E_ALL;\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic static ?int $x;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic readonly int $x;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic int $x { get => 1; }\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\n#[A]\npublic int $x = 1;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic /* middle */ int $x /* right */ = 1;\n}\n",
            SlotRole::Property,
            "property_declaration",
            "$x",
        ),
        (
            b"<?php\nclass C {\n/** d */\npublic const int K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nclass C {\n/** d */\nfinal public const K = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
        (
            b"<?php\nclass C {\n/** d */\nconst /* middle */ K /* right */ = 1;\n}\n",
            SlotRole::Binding,
            "const_declaration",
            "K",
        ),
    ];
    for &(source, role, kind, spelling) in rows {
        let parser = clean(source);
        let declaration = parser
            .root_node()
            .preorder()
            .map(|node| node.as_tree_sitter())
            .find(|node| node.kind() == kind)
            .unwrap();
        let start = source
            .windows(spelling.len())
            .position(|part| part == spelling.as_bytes())
            .unwrap();
        let found = agreed(
            &indexes(&parser, DocumentationOptions::default()),
            parser.ts_tree().root_node(),
        );
        let comment_start = source
            .windows(8)
            .position(|part| part == b"/** d */")
            .unwrap();
        assert_eq!(
            found,
            owned(
                comment(
                    comment_start..comment_start + 8,
                    "d",
                    [(0..1, Copied(comment_start + 4..comment_start + 5))],
                ),
                Holder::Slot(
                    role,
                    0,
                    start..start + spelling.len(),
                    declaration.byte_range()
                ),
            ),
            "{}",
            source.escape_ascii(),
        );
    }
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_multiple_names_and_neighbors_stay_as_before() {
    // The two-name shapes pin today's pending result; the contract does not decide their owners.
    let source =
        b"<?php\nclass C {\n/** d */\npublic $a, $b = 2;\n/** e */\nconst A = 1, B = 2;\n}\n";
    assert_eq!(
        view(source),
        View {
            documents: vec![
                comment(16..24, "d", [(0..1, Copied(20..21))]),
                comment(44..52, "e", [(0..1, Copied(48..49))]),
            ],
            assessments: unordered([
                pending(
                    AssessmentAxis::Owner,
                    AWAITS_OWNER,
                    Some(("property_declaration", 25..43)),
                    Some(0)
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("property_declaration", 25..43)),
                    None
                ),
                pending(
                    AssessmentAxis::Owner,
                    AWAITS_OWNER,
                    Some(("const_declaration", 53..72)),
                    Some(1)
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("const_declaration", 53..72)),
                    None
                ),
            ]),
            ..View::default()
        }
    );
    // A one-name control proves that an empty association list is not vacuous.
    assert_eq!(
        view(b"<?php\nclass C {\n/** d */\npublic $a;\n}\n"),
        owned(
            comment(16..24, "d", [(0..1, Copied(20..21))]),
            Holder::Slot(SlotRole::Property, 0, 32..34, 25..35)
        )
    );
    assert_eq!(
        view(b"<?php\nfunction z() {}\n/** d */\ndefine(\"X\", 1);\n"),
        View {
            documents: vec![comment(22..30, "d", [(0..1, Copied(26..27))])],
            assessments: owner_pending("expression_statement", 31..46),
            ..View::default()
        }
    );
    assert_eq!(
        view(b"<?php\nclass C { #[A]\n/** d */\npublic int $x;\n}\n"),
        View {
            documents: vec![comment(21..29, "d", [(0..1, Copied(25..26))])],
            assessments: unordered([pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0))]),
            ..View::default()
        }
    );
    assert_eq!(
        view(b"<?php\nenum E {\n/** d */\ncase A;\n}\n"),
        node_owner(
            comment(15..23, "d", [(0..1, Copied(19..20))]),
            "enum_case",
            24..31
        )
    );
    assert_eq!(
        view(b"<?php\nclass C {\n/** a */\npublic $a;\n/** b */\nconst B = 1;\n}\n"),
        View {
            documents: vec![
                comment(16..24, "a", [(0..1, Copied(20..21))]),
                comment(36..44, "b", [(0..1, Copied(40..41))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    owner: Holder::Slot(SlotRole::Property, 0, 32..34, 25..35)
                },
                Link {
                    document: 1,
                    owner: Holder::Slot(SlotRole::Binding, 0, 51..52, 45..57)
                },
            ],
            ..View::default()
        }
    );
    // A DocBlock after the modifier belongs to the declaration, but stays local.
    assert_eq!(
        view(b"<?php\nclass C {\npublic\n/** d */\nint $x;\n}\n"),
        View {
            documents: vec![comment(23..31, "d", [(0..1, Copied(27..28))])],
            assessments: unordered([pending(AssessmentAxis::Owner, AWAITS_OWNER, None, Some(0))]),
            ..View::default()
        }
    );
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_errors_keep_both_declaration_kinds_pending() {
    for source in [
        b"<?php\nclass C {\n/** d */\npublic $x = ;\n}\n".as_slice(),
        b"<?php\nclass C {\n/** d */\nconst K = ;\n}\n".as_slice(),
    ] {
        let parser = AnyParser::parse(LANG::Php, source.to_vec(), None, None).unwrap();
        assert!(parser.ts_tree().root_node().has_error());
        let result = agreed(
            &indexes(&parser, DocumentationOptions::default()),
            parser.ts_tree().root_node(),
        );
        let (kind, decl, class) = if source.windows(6).any(|w| w == b"public") {
            ("property_declaration", 25..38, 6..40)
        } else {
            ("const_declaration", 25..36, 6..38)
        };
        assert_eq!(
            result,
            View {
                documents: vec![comment(16..24, "d", [(0..1, Copied(20..21))])],
                assessments: unordered([
                    pending(
                        AssessmentAxis::Owner,
                        AWAITS_OWNER,
                        Some((kind, decl.clone())),
                        Some(0)
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        NEEDS_PROJECTION,
                        Some(("class_declaration", class)),
                        None
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        NEEDS_PROJECTION,
                        Some((kind, decl)),
                        None
                    ),
                ]),
                ..View::default()
            }
        );
    }
    let source = b"<?php\nclass C {\n/** d */\npublic $x;\nconst K = ;\n}\n";
    let parser = AnyParser::parse(LANG::Php, source.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    assert_eq!(
        agreed(
            &indexes(&parser, DocumentationOptions::default()),
            parser.ts_tree().root_node()
        ),
        View {
            documents: vec![comment(16..24, "d", [(0..1, Copied(20..21))])],
            associations: vec![Link {
                document: 0,
                owner: Holder::Slot(SlotRole::Property, 0, 32..34, 25..35)
            }],
            assessments: unordered([
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("class_declaration", 6..49)),
                    None
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("const_declaration", 36..47)),
                    None
                ),
            ]),
            ..View::default()
        }
    );
}

#[cfg(feature = "php")]
#[test]
#[allow(clippy::too_many_lines)]
fn php_unit2_single_name_queries_and_excluded_gap() {
    let source = b"<?php\nclass C {\n/** d */\npublic  $x = 1;\n}\n";
    let parser = clean(source);
    let declaration = node_at(&parser, "property_declaration", 25..40);
    let index = &indexes(&parser, DocumentationOptions::default())[0];
    let projected = index.node_projection(declaration).unwrap();
    assert!(!projected.pending());
    let names: Vec<_> = index.node_owners(declaration).unwrap().collect();
    assert_eq!(names.len(), 1);
    let Owner::Slot(slot) = names[0].get() else {
        panic!("slot")
    };
    assert_eq!(
        (slot.role, slot.ordinal, span(&slot.source)),
        (SlotRole::Property, 0, 33..35)
    );
    assert_eq!(
        agreed(
            &indexes(&parser, DocumentationOptions::default()),
            parser.ts_tree().root_node()
        ),
        owned(
            comment(16..24, "d", [(0..1, Copied(20..21))]),
            Holder::Slot(SlotRole::Property, 0, 33..35, 25..40),
        )
    );
    assert_eq!(
        index
            .node_associations(declaration)
            .unwrap()
            .associations()
            .len(),
        1
    );

    let undocumented = clean(b"<?php\nclass C {\npublic int $x;\n}\n");
    let property = node_at(&undocumented, "property_declaration", 16..30);
    let index = &indexes(&undocumented, DocumentationOptions::default())[0];
    assert_eq!(index.documents().len(), 0);
    assert_eq!(index.associations().len(), 0);
    assert!(!index.node_projection(property).unwrap().pending());
    let owners: Vec<_> = index.node_owners(property).unwrap().collect();
    let [slot] = owners.as_slice() else {
        panic!("one projected slot, found {}", owners.len());
    };
    let Owner::Slot(slot_value) = slot.get() else {
        panic!("property projects to a slot");
    };
    assert_eq!(
        (
            slot_value.role,
            slot_value.ordinal,
            span(&slot_value.source)
        ),
        (SlotRole::Property, 0, 27..29)
    );
    let enclosing = index.facts().slot_enclosing(*slot).unwrap();
    let Owner::Node(node) = enclosing.get() else {
        panic!("slot is enclosed by a declaration");
    };
    assert_eq!(site(node.node()), ("property_declaration", 16..30));
    assert_eq!(
        index
            .node_associations(property)
            .unwrap()
            .associations()
            .len(),
        0
    );

    // Excluding only whitespace keeps the parse clean and withholds the owner.
    let mut raw = tree_sitter::Parser::new();
    raw.set_language(&LANG::Php.tree_sitter_language().unwrap())
        .unwrap();
    raw.set_included_ranges(&[
        tree_sitter::Range {
            start_byte: 0,
            end_byte: 32,
            start_point: tree_sitter::Point::new(0, 0),
            end_point: tree_sitter::Point::new(3, 7),
        },
        tree_sitter::Range {
            start_byte: 33,
            end_byte: source.len(),
            start_point: tree_sitter::Point::new(3, 8),
            end_point: tree_sitter::Point::new(5, 0),
        },
    ])
    .unwrap();
    let tree = raw.parse(source, None).unwrap();
    let result = published(
        &collect(
            LANG::Php,
            &tree,
            source,
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap(),
        tree.root_node(),
    );
    assert!(!tree.root_node().has_error());
    assert_eq!(
        result,
        View {
            assessments: unordered([
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("class_declaration", 6..42)),
                    None
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("declaration_list", 14..42)),
                    None
                ),
                pending(
                    AssessmentAxis::Owner,
                    NEEDS_PROJECTION,
                    Some(("property_declaration", 25..40)),
                    None
                ),
            ]),
            ..View::default()
        }
    );
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_name_scan_charges_and_ignores_comments() {
    use super::projections::php_single_name;
    use crate::documentation::budget::NoBudget;
    let source = b"<?php\nclass C {\n/** d */\nconst /* middle */ K /* right */ = 1;\n}\n";
    let parser = clean(source);
    let declaration = node_at(&parser, "const_declaration", 25..62);
    let index = &indexes(&parser, DocumentationOptions::default())[0];
    let id = index.canonical.lookup(declaration).unwrap();
    let name = php_single_name(&index.canonical, id, false, &NoBudget)
        .unwrap()
        .unwrap();
    assert_eq!(
        site(index.canonical.nodes[name.0].evidence.node()),
        ("name", 44..45)
    );
}

#[cfg(feature = "php")]
#[derive(Default)]
struct PhpCounting(std::cell::Cell<usize>, Option<usize>);

#[cfg(feature = "php")]
impl crate::documentation::budget::WorkBudget for PhpCounting {
    type Error = crate::documentation::SourceError;
    #[cfg(feature = "php")]
    fn checkpoint(&self) -> Result<(), crate::documentation::SourceError> {
        Ok(())
    }
    #[cfg(feature = "php")]
    fn bytes(&self, _: usize) -> Result<(), crate::documentation::SourceError> {
        Ok(())
    }
    fn step(&self) -> Result<(), crate::documentation::SourceError> {
        self.0.set(self.0.get() + 1);
        if self.1 == Some(self.0.get()) {
            Err(crate::documentation::SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_child_budget_has_both_bounds() {
    use super::projections::php_single_name;
    for (source, kind, property, expected) in [
        (
            b"<?php class C { public $x; }".as_slice(),
            "property_declaration",
            true,
            "$x",
        ),
        (
            b"<?php class C { const K = 1; }".as_slice(),
            "const_declaration",
            false,
            "K",
        ),
        (
            b"<?php class C { const K = FOO; }".as_slice(),
            "const_declaration",
            false,
            "K",
        ),
        (
            b"<?php class C { const /* middle */ K /* right */ = 1; }".as_slice(),
            "const_declaration",
            false,
            "K",
        ),
    ] {
        let parser = clean(source);
        let declaration = parser
            .root_node()
            .preorder()
            .map(|n| n.as_tree_sitter())
            .find(|n| n.kind() == kind)
            .unwrap();
        let index = &indexes(&parser, DocumentationOptions::default())[0];
        let id = index.canonical.lookup(declaration).unwrap();
        let outer = index.canonical.nodes[id.0].children.clone();
        let count_outer = outer.len();
        let element_kind = if property {
            "property_element"
        } else {
            "const_element"
        };
        let element = index.canonical.children[outer]
            .iter()
            .find(|child| index.canonical.nodes[child.0].evidence.node().kind() == element_kind)
            .unwrap();
        let count_inner = index.canonical.children
            [index.canonical.nodes[element.0].children.clone()]
        .iter()
        .take_while(|child| index.canonical.nodes[child.0].evidence.node().kind() != "=")
        .count()
            + usize::from(
                index.canonical.children[index.canonical.nodes[element.0].children.clone()]
                    .iter()
                    .any(|child| index.canonical.nodes[child.0].evidence.node().kind() == "="),
            );
        let budget = PhpCounting::default();
        let name = php_single_name(&index.canonical, id, property, &budget)
            .unwrap()
            .unwrap();
        let measured = budget.0.get();
        // Both bounds: each structural child (comments included) is read once.
        assert!(measured >= count_outer + count_inner);
        assert!(measured <= count_outer + count_inner);
        let spelling = index.canonical.nodes[name.0].evidence.node().byte_range();
        assert_eq!(&source[spelling], expected.as_bytes());
        for at in 1..=measured {
            let stopped = PhpCounting(std::cell::Cell::new(0), Some(at));
            assert!(matches!(
                php_single_name(&index.canonical, id, property, &stopped),
                Err(crate::documentation::SourceError::Cancelled)
            ));
        }
    }
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_collection_charges_the_name_scan_and_source_pieces() {
    let width = 4100;
    let source = format!(
        "<?php\nclass C {{\n/** d */\npublic {}$x;\n}}\n",
        "/*c*/ ".repeat(width)
    );
    let parser = clean(source.as_bytes());
    let declaration = parser
        .root_node()
        .preorder()
        .map(|node| node.as_tree_sitter())
        .find(|node| node.kind() == "property_declaration")
        .unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let baseline = observations();
    let index = parser.documentation(&context, &options).unwrap();
    let after = observations();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let polls = after.polls - baseline.polls;
    let expected_searches = after.intersection_searches - baseline.intersection_searches;
    let mut scan = 0;
    for at in 1..=polls {
        let before = observations();
        let result = crate::documentation::budget::cancel_after(at, || {
            parser.documentation(&context, &options)
        });
        let observed = observations();
        assert!(matches!(result, Err(CollectionError::Cancelled)), "{at}");
        // Name scan: all declaration children entered, but no declaration owner yet.
        if observed.visits - before.visits + 1 == after.visits - baseline.visits
            && observed.owner_projections - before.owner_projections == 2
        {
            scan += 1;
        }
    }
    assert!((3..=6).contains(&scan), "{scan} polls in scan");
    // The target's source span is looked up twice: fragment and slot.
    assert_eq!(expected_searches, 2);
    // With a cancelled flag, the last poll in collection must still reject the index.
    let cancelled = crate::cancel::ParseCancel::new();
    cancelled.cancel();
    assert!(matches!(
        cancelled.run(|| parser.documentation(&context, &options)),
        Err(CollectionError::Cancelled)
    ));
    let projected = index.node_projection(declaration).unwrap();
    assert!(!projected.pending());
}

#[cfg(feature = "php")]
#[test]
fn php_unit2_split_name_ranges_charge_source_pieces() {
    // Adjacent included ranges split the name without excluding a byte. Its 2,101
    // pieces require three budget checkpoints in the slot intersection alone.
    let source = format!(
        "<?php\nclass C {{\n/** d */\npublic ${};\n}}\n",
        "a".repeat(2100),
    );
    let start = source.find("$a").unwrap();
    let point = |at: usize| {
        let prior = &source[..at];
        tree_sitter::Point::new(
            prior.bytes().filter(|byte| *byte == b'\n').count(),
            prior.rsplit('\n').next().unwrap().len(),
        )
    };
    let mut included: Vec<Range<usize>> = Vec::new();
    included.push(0..start);
    included.extend((start..start + 2101).map(|at| at..at + 1));
    included.push(start + 2101..source.len());
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
        .set_language(&LANG::Php.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source.as_bytes(), None).unwrap();
    assert!(!tree.root_node().has_error());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    let index = collect(LANG::Php, &tree, source.as_bytes(), &context, &options).unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    let polls = observations().polls - before.polls;
    // Both bounds: removing the budget at the slot-piece call yields 15, not 17.
    assert!(polls >= 17, "{polls}");
    assert!(polls <= 17, "{polls}");
    for checkpoint in 1..=polls {
        let result = crate::documentation::budget::cancel_after(checkpoint, || {
            collect(LANG::Php, &tree, source.as_bytes(), &context, &options)
        });
        assert!(
            matches!(result, Err(CollectionError::Cancelled)),
            "{checkpoint}"
        );
    }
    assert_eq!(
        crate::documentation::budget::cancel_after(polls + 1, || {
            collect(LANG::Php, &tree, source.as_bytes(), &context, &options)
        })
        .unwrap()
        .associations()
        .len(),
        1,
    );
}
