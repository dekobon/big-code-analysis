//! Ruby YARD source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the tree shapes the source-view tests rely on. Node queries are not part of the view.

#[cfg(feature = "ruby")]
use crate::documentation::*;
#[cfg(feature = "ruby")]
use crate::{AnyParser, LANG};
#[cfg(feature = "ruby")]
use std::ops::Range;

#[cfg(feature = "ruby")]
use Run::{Copied, Synthetic};

#[cfg(feature = "ruby")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "ruby")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "ruby")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    resolution: Resolution,
    owner: (&'static str, Range<usize>),
}

#[cfg(feature = "ruby")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and order-independent pending assessments
/// of a collection. Shared values are asserted by `published`; node queries are not included.
#[cfg(feature = "ruby")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Link>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "ruby")]
#[derive(Clone, Copy)]
enum Retain {
    Unbound,
    Directives,
}

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
fn run<const F: usize, const N: usize>(
    fragments: [Range<usize>; F],
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        role: DocumentRole::Primary,
        composition: Composition::CommentRun,
        fragments: fragments.into(),
        text: text.to_owned(),
        map: map.into(),
    }
}

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
fn pending(
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason,
        node,
        document,
    }
}

#[cfg(feature = "ruby")]
fn unordered(assessments: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = assessments.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "ruby")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "ruby")]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "ruby")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "ruby")]
fn fragment_span(fragment: &Fragment<'_>, source: &[u8]) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    let node = fragment.node().unwrap().node();
    assert_eq!((node.kind(), node.start_byte()), ("comment", range.start));
    // LF comments end with the spelling; on CRLF the tree node alone includes the CR.
    assert!(matches!(&source[range.end..node.end_byte()], b"" | b"\r"));
    range
}

#[cfg(feature = "ruby")]
fn published(index: &DocumentationIndex<'_>, source: &[u8]) -> View {
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
                    .map(|at| {
                        fragment_span(facts.document_fragment(document, at).unwrap().get(), source)
                    })
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
            assert_eq!(attachment.convention.name(), "YARD");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let first_comment = first.get().node().unwrap().node();
            let last = facts
                .document_fragment(document, document.get().fragment_count() - 1)
                .unwrap();
            let last_comment = last.get().node().unwrap().node();
            // Both the binding and scope follow from the comment nodes in the parse tree.
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                last_comment.next_sibling()
            );
            assert_eq!(
                Some(attachment.lexical_scope.node()),
                first_comment.parent()
            );
            let binding = attachment.binding.unwrap().node();
            let expected_owner = if binding.kind() == "body_statement" {
                binding.child(0).unwrap()
            } else {
                binding
            };
            let owner = facts.associated_owner(association, 0).unwrap();
            let Owner::Node(declared) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            assert_eq!(declared.node(), expected_owner);
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
            (
                candidate.get().disposition,
                fragment_span(fragment.get(), source),
            )
        })
        .collect();
    let assessments = unordered(index.assessments().map(|assessment| Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: assessment.get().node.map(|node| site(node.node())),
        document: index.assessed_document(assessment).unwrap().map(position),
    }));
    assert_eq!(index.capability(), Capability::Partial);
    // Qualifications belong to named-reference commands, which none of these YARD cases has.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "ruby")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    assert!(!parser.root_node().has_error(), "{}", source.escape_ascii());
    parser
}

#[cfg(feature = "ruby")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Ruby,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "ruby")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2], source: &[u8]) -> View {
    let [typed, raw] = indexes.each_ref().map(|index| published(index, source));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "ruby")]
fn view(source: &[u8]) -> View {
    let parser = clean(source);
    agreed(&indexes(&parser, DocumentationOptions::default()), source)
}

#[cfg(feature = "ruby")]
fn retained(source: &[u8], retain: Retain) -> View {
    let parser = clean(source);
    let options = DocumentationOptions {
        retain_unbound: matches!(retain, Retain::Unbound),
        retain_directives: matches!(retain, Retain::Directives),
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&parser, options), source)
}

#[cfg(feature = "ruby")]
fn assert_default_then_retained(
    source: &[u8],
    mut expected: View,
    retain: Retain,
    candidates: &[(CandidateDisposition, Range<usize>)],
) {
    assert_eq!(view(source), expected, "{}", source.escape_ascii());
    expected.evidence.extend_from_slice(candidates);
    assert_eq!(
        retained(source, retain),
        expected,
        "{}",
        source.escape_ascii()
    );
}

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
#[test]
fn ruby_hash_run_documents_the_declaration_below_it() {
    let sources: [(&[u8], &str, Range<usize>); 4] = [
        (b"# audit\ndef f\nend\n", "method", 8..17),
        (b"# audit\ndef self.f; end\n", "singleton_method", 8..23),
        (b"# audit\nclass C\nend\n", "class", 8..19),
        (b"# audit\nmodule M\nend\n", "module", 8..20),
    ];
    for (source, kind, owner) in sources {
        assert_eq!(
            view(source),
            owned(comment(0..7, "audit", [(0..5, Copied(2..7))]), kind, owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_hash_rows_on_consecutive_rows_join_into_one_run() {
    let sources: [(&[u8], Doc, Range<usize>); 3] = [
        (
            b"# a\n# b\ndef f; end\n",
            run(
                [0..3, 4..7],
                "a\nb",
                [
                    (0..1, Copied(2..3)),
                    (1..2, Synthetic),
                    (2..3, Copied(6..7)),
                ],
            ),
            8..18,
        ),
        (
            b"# a\n#\n# b\ndef f; end\n",
            run(
                [0..3, 4..5, 6..9],
                "a\n\nb",
                [
                    (0..1, Copied(2..3)),
                    (1..2, Synthetic),
                    (2..3, Synthetic),
                    (3..4, Copied(8..9)),
                ],
            ),
            10..20,
        ),
        (
            b"#\n# audit\n#\ndef f; end\n",
            run([0..1, 2..9, 10..11], "audit", [(0..5, Copied(4..9))]),
            12..22,
        ),
    ];
    for (source, document, owner) in sources {
        assert_eq!(view(source), owned(document, "method", owner));
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_begin_block_alone_documents_the_method_below_it() {
    assert_eq!(
        view(b"=begin\naudit\n=end\ndef f\nend\n"),
        owned(
            comment(0..17, "audit", [(0..5, Copied(7..12))]),
            "method",
            18..27
        )
    );
    assert_eq!(
        view(b"=begin\na\n  b\n=end\ndef f; end\n"),
        owned(
            comment(
                0..17,
                "a\n  b",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..5, Copied(9..12)),
                ]
            ),
            "method",
            18..28
        )
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_blank_row_splits_a_run() {
    // Only the lower run documents the method; the upper row is retained on request.
    assert_default_then_retained(
        b"# a\n\n# b\ndef f; end\n",
        owned(comment(5..8, "b", [(0..1, Copied(7..8))]), "method", 9..19),
        Retain::Unbound,
        &[(CandidateDisposition::Unattached, 0..3)],
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_shebang_and_magic_comment_are_no_text() {
    assert_default_then_retained(
        b"#!/usr/bin/env ruby\n# frozen_string_literal: true\n# audit\ndef f; end\n",
        owned(
            comment(50..57, "audit", [(0..5, Copied(52..57))]),
            "method",
            58..68,
        ),
        Retain::Directives,
        &[
            (CandidateDisposition::Directive, 0..19),
            (CandidateDisposition::Directive, 20..49),
        ],
    );
    let alone = b"#!/usr/bin/env ruby\ndef f; end\n";
    assert_default_then_retained(
        alone,
        View::default(),
        Retain::Directives,
        &[(CandidateDisposition::Directive, 0..19)],
    );
    // Keeping unbound comments alone does not retain a directive.
    assert_eq!(retained(alone, Retain::Unbound), View::default());
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_comment_on_a_code_row_is_no_document() {
    let alone = b"x = 1 # t\ndef f; end\n";
    assert_default_then_retained(
        alone,
        View::default(),
        Retain::Unbound,
        &[(CandidateDisposition::Unattached, 6..9)],
    );
    // Keeping directives alone does not retain the comment after code.
    assert_eq!(retained(alone, Retain::Directives), View::default());
    assert_default_then_retained(
        b"x = 1 # t\n# d\ndef f; end\n",
        owned(
            comment(10..13, "d", [(0..1, Copied(12..13))]),
            "method",
            14..24,
        ),
        Retain::Unbound,
        &[(CandidateDisposition::Unattached, 6..9)],
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_run_inside_a_class_body_documents_the_method_below_it() {
    assert_eq!(
        view(b"class C\n# audit\ndef f; end\nend\n"),
        owned(
            comment(8..15, "audit", [(0..5, Copied(10..15))]),
            "method",
            16..26
        )
    );
    assert_eq!(
        view(b"class C\ndef a; end\n# audit\ndef f; end\nend\n"),
        owned(
            comment(19..26, "audit", [(0..5, Copied(21..26))]),
            "method",
            27..37
        )
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_run_documents_a_method_inside_a_singleton_class() {
    assert_eq!(
        view(b"class C\nclass << self\n# audit\ndef f; end\nend\nend\n"),
        owned(
            comment(22..29, "audit", [(0..5, Copied(24..29))]),
            "method",
            30..40
        )
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_hash_marker_and_one_space_are_removed() {
    let sources: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"#audit\ndef f; end\n",
            comment(0..6, "audit", [(0..5, Copied(1..6))]),
            7..17,
        ),
        (
            b"#  audit\ndef f; end\n",
            comment(0..8, " audit", [(0..6, Copied(2..8))]),
            9..19,
        ),
        (
            b"#\taudit\ndef f; end\n",
            comment(0..7, "\taudit", [(0..6, Copied(1..7))]),
            8..18,
        ),
        (
            b"# audit  \ndef f; end\n",
            comment(0..9, "audit  ", [(0..7, Copied(2..9))]),
            10..20,
        ),
    ];
    for (source, document, owner) in sources {
        assert_eq!(view(source), owned(document, "method", owner));
    }
}

/// Pins today's double-hash outcome; a later unit changes it on purpose.
#[cfg(feature = "ruby")]
#[test]
fn ruby_double_hash_rows_keep_their_outcome() {
    let awaiting = "Selected local or wrapper form awaits owner projection";
    assert_eq!(
        view(b"##\n# audit\ndef f; end\n"),
        View {
            documents: vec![run([0..2, 3..10], "audit", [(0..5, Copied(5..10))])],
            assessments: unordered([pending(awaiting, None, Some(0))]),
            ..View::default()
        }
    );
    assert_eq!(
        view(b"## audit\ndef f; end\n"),
        View {
            documents: vec![comment(0..8, "audit", [(0..5, Copied(3..8))])],
            assessments: unordered([pending(awaiting, None, Some(0))]),
            ..View::default()
        }
    );
    assert_eq!(
        view(b"# a\n## b\ndef f; end\n"),
        owned(
            run(
                [0..3, 4..8],
                "a\nb",
                [
                    (0..1, Copied(2..3)),
                    (1..2, Synthetic),
                    (2..3, Copied(7..8)),
                ]
            ),
            "method",
            9..19
        )
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_crlf_line_endings_stay_out_of_fragments_and_text() {
    assert_eq!(
        view(b"# audit\r\ndef f\r\nend\r\n"),
        owned(
            comment(0..7, "audit", [(0..5, Copied(2..7))]),
            "method",
            9..19
        )
    );
    assert_eq!(
        view(b"# a\r\n# b\r\ndef f; end\r\n"),
        owned(
            run(
                [0..3, 5..8],
                "a\nb",
                [
                    (0..1, Copied(2..3)),
                    (1..2, Synthetic),
                    (2..3, Copied(7..8)),
                ]
            ),
            "method",
            10..20
        )
    );
}

/// Pins today's pending forms; a later unit changes their owner projections on purpose.
#[cfg(feature = "ruby")]
#[test]
fn ruby_pending_forms_keep_their_outcome() {
    let awaiting = "Selected local or wrapper form awaits owner projection";
    let projection = "Recognized declaration requires a semantic projection";
    assert_eq!(
        view(b"class C\n# audit\nprivate def f; end\nend\n"),
        View {
            documents: vec![comment(8..15, "audit", [(0..5, Copied(10..15))])],
            assessments: unordered([
                pending(awaiting, Some(("body_statement", 16..34)), Some(0)),
                pending(projection, Some(("body_statement", 16..34)), None),
                pending(projection, Some(("call", 16..34)), None),
            ]),
            ..View::default()
        }
    );
    for (source, kind, range) in [
        (b"# audit\nattr_reader :x\n".as_slice(), "call", 8..22),
        (b"# audit\nX = 1\n".as_slice(), "assignment", 8..13),
    ] {
        assert_eq!(
            view(source),
            View {
                documents: vec![comment(0..7, "audit", [(0..5, Copied(2..7))])],
                assessments: unordered([
                    pending(awaiting, Some((kind, range.clone())), Some(0)),
                    pending(projection, Some((kind, range)), None),
                ]),
                ..View::default()
            }
        );
    }
}

#[cfg(feature = "ruby")]
#[test]
#[allow(clippy::too_many_lines)]
fn ruby_grammar_shapes_the_unit_relies_on() {
    let source = b"# a\n# b\ndef f\nend\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("program", 0..18));
    assert_eq!(
        parts(root),
        [
            (None, "comment", 0..3),
            (None, "comment", 4..7),
            (None, "method", 8..17),
        ]
    );
    for range in [0..3, 4..7] {
        let comment = node_at(&parser, "comment", range.clone());
        assert!(comment.is_extra());
        assert_eq!(comment.parent(), Some(root));
        assert_eq!(source[range.end], b'\n');
    }
    let method = node_at(&parser, "method", 8..17);
    assert_eq!(
        method.child_by_field_name("name").map(site),
        Some(("identifier", 12..13))
    );

    let source = b"=begin\naudit\n=end\ndef f\nend\n";
    let parser = clean(source);
    let comment = node_at(&parser, "comment", 0..17);
    assert_eq!(comment.parent(), Some(parser.ts_tree().root_node()));
    assert_eq!(comment.next_sibling().map(site), Some(("method", 18..27)));
    assert_eq!(&source[comment.end_byte()..18], b"\n");
    assert_eq!(comment.end_position().row - comment.start_position().row, 2);

    let parser = clean(b"class C\n# a\ndef f; end\n# b\ndef g; end\nend\n");
    let class = node_at(&parser, "class", 0..41);
    assert_eq!(
        parts(class),
        [
            (None, "class", 0..5),
            (Some("name"), "constant", 6..7),
            (None, "comment", 8..11),
            (Some("body"), "body_statement", 12..37),
            (None, "end", 38..41),
        ]
    );
    assert!(node_at(&parser, "comment", 8..11).is_extra());
    assert_eq!(
        parts(class.child_by_field_name("body").unwrap()),
        [
            (None, "method", 12..22),
            (None, "comment", 23..26),
            (None, "method", 27..37),
        ]
    );
    assert!(node_at(&parser, "comment", 23..26).is_extra());

    let parser = clean(b"class C\nclass << self\n# audit\ndef f; end\nend\nend\n");
    let singleton = node_at(&parser, "singleton_class", 8..44);
    assert_eq!(
        parts(singleton),
        [
            (None, "class", 8..13),
            (None, "<<", 14..16),
            (Some("value"), "self", 17..21),
            (None, "comment", 22..29),
            (Some("body"), "body_statement", 30..40),
            (None, "end", 41..44),
        ]
    );
    assert_eq!(
        singleton
            .child_by_field_name("body")
            .unwrap()
            .child(0)
            .map(site),
        Some(("method", 30..40))
    );

    let parser = clean(b"class C\n# audit\nprivate def f; end\nend\n");
    let class = node_at(&parser, "class", 0..38);
    assert_eq!(
        parts(class)[2..4],
        [
            (None, "comment", 8..15),
            (Some("body"), "body_statement", 16..34),
        ]
    );
    let body = class.child_by_field_name("body").unwrap();
    assert_eq!(parts(body), [(None, "call", 16..34)]);
    let call = node_at(&parser, "call", 16..34);
    assert_eq!(
        parts(call),
        [
            (Some("method"), "identifier", 16..23),
            (Some("arguments"), "argument_list", 24..34),
        ]
    );
    assert_eq!(
        parts(call.child_by_field_name("arguments").unwrap()),
        [(None, "method", 24..34)]
    );

    let parser = clean(b"# audit\nattr_reader :x\n");
    let call = node_at(&parser, "call", 8..22);
    assert_eq!(
        parts(call),
        [
            (Some("method"), "identifier", 8..19),
            (Some("arguments"), "argument_list", 20..22),
        ]
    );
    assert_eq!(
        parts(call.child_by_field_name("arguments").unwrap()),
        [(None, "simple_symbol", 20..22)]
    );

    let parser = clean(b"# audit\nX = 1\n");
    let assignment = node_at(&parser, "assignment", 8..13);
    assert_eq!(
        assignment.child_by_field_name("left").map(site),
        Some(("constant", 8..9))
    );

    let parser = clean(b"x = 1 # t\n# d\ndef f; end\n");
    let assignment = node_at(&parser, "assignment", 0..5);
    assert_eq!(
        assignment.child_by_field_name("left").map(site),
        Some(("identifier", 0..1))
    );
    let after_code = assignment.next_sibling().unwrap();
    assert_eq!(site(after_code), ("comment", 6..9));
    assert_eq!(
        after_code.start_position().row,
        assignment.start_position().row
    );
    assert_eq!(
        after_code.next_sibling().map(site),
        Some(("comment", 10..13))
    );
    assert_eq!(
        after_code.next_sibling().unwrap().next_sibling().map(site),
        Some(("method", 14..24))
    );

    let parser = clean(b"def self.f; end\nmodule M\nend\nclass C\nend\n");
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "singleton_method", 0..15),
            (None, "module", 16..28),
            (None, "class", 29..40),
        ]
    );

    let parser = clean(b"#!/usr/bin/env ruby\n##\ndef f; end\n");
    assert_eq!(
        parts(parser.ts_tree().root_node())[0..2],
        [(None, "comment", 0..19), (None, "comment", 20..22)]
    );

    let source = b"# audit\r\ndef f\r\nend\r\n";
    let parser = clean(source);
    let root = parser.ts_tree().root_node();
    let comment = node_at(&parser, "comment", 0..8);
    let method = node_at(&parser, "method", 9..19);
    assert_eq!(site(root), ("program", 0..21));
    assert_eq!(&source[comment.byte_range()], b"# audit\r");
    assert_eq!(&source[comment.end_byte()..method.start_byte()], b"\n");
    assert_eq!(&source[method.end_byte()..root.end_byte()], b"\r\n");
}
