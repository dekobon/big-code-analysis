//! Ruby YARD source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the tree shapes the source-view tests rely on. Node queries are not part of the view.

#[cfg(feature = "ruby")]
use super::projections::{ruby_attribute_name, ruby_call_comment_cost, ruby_slot_cost};
#[cfg(feature = "ruby")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "ruby")]
use crate::documentation::*;
#[cfg(feature = "ruby")]
use crate::{AnyParser, LANG};
#[cfg(feature = "ruby")]
use std::cell::Cell;
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
#[derive(Debug, PartialEq, Eq)]
struct Edge {
    document: usize,
    resolution: Resolution,
    binding: Option<(&'static str, Range<usize>)>,
    scope: (&'static str, Range<usize>),
    owners: Vec<Owned>,
}

#[cfg(feature = "ruby")]
#[derive(Debug, PartialEq, Eq)]
enum Owned {
    Node(&'static str, Range<usize>),
    Property {
        enclosing: (&'static str, Range<usize>),
        ordinal: usize,
        source: Range<usize>,
    },
}

#[cfg(feature = "ruby")]
#[derive(Debug, Default, PartialEq, Eq)]
struct CompleteView {
    documents: Vec<Doc>,
    associations: Vec<Edge>,
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
fn complete(index: &DocumentationIndex<'_>, source: &[u8]) -> CompleteView {
    // bca: suppress(halstead) -- one complete view of the published facts for Ruby Unit 2.
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
    let position = |document| ordered.iter().position(|known| *known == document).unwrap();
    let documents = ordered
        .iter()
        .map(|&document| {
            let text = index.complete_text(document).unwrap().unwrap();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| {
                        fragment_span(facts.document_fragment(document, at).unwrap().get(), source)
                    })
                    .collect(),
                text: text.text().to_owned(),
                map: text
                    .mappings()
                    .iter()
                    .map(|run| (run.output.range(), origin_run(&run.origin)))
                    .collect(),
            }
        })
        .collect();
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.convention.name(), "YARD");
            assert_eq!(attachment.publication, Publication::Unknown);
            let owners = (0..attachment.owner_count())
                .map(|at| {
                    let owner = facts.associated_owner(association, at).unwrap();
                    match owner.get() {
                        Owner::Node(node) => {
                            Owned::Node(node.node().kind(), node.node().byte_range())
                        }
                        Owner::Slot(slot) => {
                            assert_eq!(slot.role, SlotRole::Property);
                            assert_eq!(slot.ordinal, at);
                            assert!(!slot.source.has_excluded_gap());
                            let enclosing = facts.slot_enclosing(owner).unwrap();
                            let Owner::Node(node) = enclosing.get() else {
                                panic!("attribute slot enclosed by call")
                            };
                            assert_eq!(node.node().kind(), "call");
                            assert_eq!(
                                index
                                    .owner_associations(owner)
                                    .unwrap()
                                    .qualifications()
                                    .len(),
                                0
                            );
                            Owned::Property {
                                enclosing: site(node.node()),
                                ordinal: at,
                                source: span(&slot.source),
                            }
                        }
                        other => panic!("Ruby node or property slot, got {other:?}"),
                    }
                })
                .collect();
            Edge {
                document: position(facts.associated_document(association).unwrap()),
                resolution: attachment.resolution,
                binding: attachment.binding.map(|node| site(node.node())),
                scope: site(attachment.lexical_scope.node()),
                owners,
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
    assert_eq!(index.qualifications().len(), 0);
    CompleteView {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "ruby")]
fn complete_agreed(indexes: &[DocumentationIndex<'_>; 2], source: &[u8]) -> CompleteView {
    let [typed, raw] = indexes.each_ref().map(|index| complete(index, source));
    assert_eq!(typed, raw, "Ruby typed and raw collection must agree");
    typed
}

#[cfg(feature = "ruby")]
fn complete_view(source: &[u8]) -> CompleteView {
    let parser = clean(source);
    complete_agreed(&indexes(&parser, DocumentationOptions::default()), source)
}

#[cfg(feature = "ruby")]
fn one_doc(fragment: Range<usize>, text: &str) -> Vec<Doc> {
    let copied = fragment.start + 2..fragment.end;
    vec![comment(fragment, text, [(0..text.len(), Copied(copied))])]
}

#[cfg(feature = "ruby")]
fn edge(
    document: usize,
    binding: (&'static str, Range<usize>),
    scope: (&'static str, Range<usize>),
    owners: Vec<Owned>,
) -> Edge {
    Edge {
        document,
        resolution: Resolution::Resolved,
        binding: Some(binding),
        scope,
        owners,
    }
}

#[cfg(feature = "ruby")]
fn property(call: Range<usize>, ordinal: usize, source: Range<usize>) -> Owned {
    Owned::Property {
        enclosing: ("call", call),
        ordinal,
        source,
    }
}

#[cfg(feature = "ruby")]
fn pending_call(call: Range<usize>) -> CompleteView {
    CompleteView {
        documents: one_doc(0..7, "audit"),
        assessments: unordered([
            pending(
                "Selected local or wrapper form awaits owner projection",
                Some(("call", call.clone())),
                Some(0),
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("call", call)),
                None,
            ),
        ]),
        ..CompleteView::default()
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_visibility_wrapped_methods_resolve_to_the_inner_method() {
    let source = b"class C\n# audit\nprivate def f; end\nend";
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        complete_agreed(&indexes, source),
        CompleteView {
            documents: one_doc(8..15, "audit"),
            associations: vec![edge(
                0,
                ("body_statement", 16..34),
                ("class", 0..38),
                vec![Owned::Node("method", 24..34)],
            )],
            ..CompleteView::default()
        }
    );
    for index in &indexes {
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().convention.name(), "YARD");
        assert_eq!(association.get().publication, Publication::Unknown);
        assert_eq!(
            association.get().binding.unwrap().node().kind(),
            "body_statement"
        );
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(method) = owner.get() else {
            panic!("method owner")
        };
        assert_eq!(site(method.node()), ("method", 24..34));
        for kind in ["call", "method"] {
            let node = parser
                .root_node()
                .preorder()
                .find(|n| n.kind() == kind)
                .unwrap();
            let projection = index.node_projection(node.as_tree_sitter()).unwrap();
            assert!(!projection.pending());
            assert_eq!(projection.owners().len(), 1);
            assert_eq!(
                index
                    .node_associations(node.as_tree_sitter())
                    .unwrap()
                    .associations()
                    .len(),
                1
            );
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_attribute_names_have_ordered_property_slots() {
    let source = b"# audit\nattr_accessor :a, :b";
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        complete_agreed(&indexes, source),
        CompleteView {
            documents: one_doc(0..7, "audit"),
            associations: vec![edge(
                0,
                ("call", 8..28),
                ("program", 0..28),
                vec![property(8..28, 0, 22..24), property(8..28, 1, 26..28)],
            )],
            ..CompleteView::default()
        }
    );
    for index in &indexes {
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().owner_count(), 2);
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().publication, Publication::Unknown);
        assert_eq!(association.get().convention.name(), "YARD");
        let call = parser
            .root_node()
            .preorder()
            .find(|n| n.kind() == "call")
            .unwrap();
        let projection = index.node_projection(call.as_tree_sitter()).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 2);
        for (ordinal, expected) in [22..24, 26..28].into_iter().enumerate() {
            let slot = index
                .facts()
                .associated_owner(association, ordinal)
                .unwrap();
            let Owner::Slot(slot) = slot.get() else {
                panic!("property slot")
            };
            assert_eq!(slot.role, SlotRole::Property);
            assert_eq!(slot.ordinal, ordinal);
            assert_eq!(span(&slot.source), expected);
            assert_eq!(
                index
                    .owner_associations(
                        index
                            .facts()
                            .associated_owner(association, ordinal)
                            .unwrap()
                    )
                    .unwrap()
                    .associations()
                    .len(),
                1
            );
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_dynamic_attribute_and_alias_stay_pending() {
    for (source, end) in [
        (b"# audit\nattr_reader :x, *names".as_slice(), 30),
        (b"# audit\nattr_reader \"#{name}\"".as_slice(), 29),
        (b"# audit\nalias_method :x, :y".as_slice(), 27),
    ] {
        assert_eq!(complete_view(source), pending_call(8..end));
        let parser = clean(source);
        for index in indexes(&parser, DocumentationOptions::default()) {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.associations().len(), 0);
            let call = parser
                .root_node()
                .preorder()
                .find(|n| n.kind() == "call")
                .unwrap();
            assert!(
                index
                    .node_projection(call.as_tree_sitter())
                    .unwrap()
                    .pending()
            );
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
#[allow(clippy::too_many_lines)]
fn ruby_attribute_forms_and_visibility_variants_keep_their_owners() {
    let source = b"class C\n# a\nattr_reader :x\n# b\nattr_writer :y\n# c\nattr_accessor :a, :b\n# d\nattr :z\nend";
    let expected = CompleteView {
        documents: vec![
            comment(8..11, "a", [(0..1, Copied(10..11))]),
            comment(27..30, "b", [(0..1, Copied(29..30))]),
            comment(46..49, "c", [(0..1, Copied(48..49))]),
            comment(71..74, "d", [(0..1, Copied(73..74))]),
        ],
        associations: vec![
            edge(
                0,
                ("body_statement", 12..82),
                ("class", 0..86),
                vec![property(12..26, 0, 24..26)],
            ),
            edge(
                1,
                ("call", 31..45),
                ("body_statement", 12..82),
                vec![property(31..45, 0, 43..45)],
            ),
            edge(
                2,
                ("call", 50..70),
                ("body_statement", 12..82),
                vec![property(50..70, 0, 64..66), property(50..70, 1, 68..70)],
            ),
            edge(
                3,
                ("call", 75..82),
                ("body_statement", 12..82),
                vec![property(75..82, 0, 80..82)],
            ),
        ],
        ..CompleteView::default()
    };
    assert_eq!(complete_view(source), expected);
    let parser = clean(source);
    for index in indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(index.documents().len(), 4);
        assert_eq!(index.associations().len(), 4);
        let names = [
            [Some(24..26), None],
            [Some(43..45), None],
            [Some(64..66), Some(68..70)],
            [Some(80..82), None],
        ];
        for (association, expected) in index.associations().zip(names) {
            assert_eq!(
                association.get().owner_count(),
                expected.iter().flatten().count()
            );
            for (ordinal, range) in expected.into_iter().flatten().enumerate() {
                let owner = index
                    .facts()
                    .associated_owner(association, ordinal)
                    .unwrap();
                let Owner::Slot(slot) = owner.get() else {
                    panic!("property slot")
                };
                assert_eq!(slot.role, SlotRole::Property);
                assert_eq!(slot.ordinal, ordinal);
                assert_eq!(span(&slot.source), range);
            }
        }
    }
    let source = b"class C\n# a\npublic def self.g; end\n# b\nprivate_class_method def self.h; end\n# c\nmodule_function def k; end\n# d\nprotected def e; end\nend";
    assert_eq!(
        complete_view(source),
        CompleteView {
            documents: vec![
                comment(8..11, "a", [(0..1, Copied(10..11))]),
                comment(35..38, "b", [(0..1, Copied(37..38))]),
                comment(76..79, "c", [(0..1, Copied(78..79))]),
                comment(107..110, "d", [(0..1, Copied(109..110))]),
            ],
            associations: vec![
                edge(
                    0,
                    ("body_statement", 12..131),
                    ("class", 0..135),
                    vec![Owned::Node("singleton_method", 19..34)]
                ),
                edge(
                    1,
                    ("call", 39..75),
                    ("body_statement", 12..131),
                    vec![Owned::Node("singleton_method", 60..75)]
                ),
                edge(
                    2,
                    ("call", 80..106),
                    ("body_statement", 12..131),
                    vec![Owned::Node("method", 96..106)]
                ),
                edge(
                    3,
                    ("call", 111..131),
                    ("body_statement", 12..131),
                    vec![Owned::Node("method", 121..131)]
                ),
            ],
            ..CompleteView::default()
        }
    );
    let parser = clean(source);
    for index in indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(index.documents().len(), 4);
        let owners = [
            ("singleton_method", 19..34),
            ("singleton_method", 60..75),
            ("method", 96..106),
            ("method", 121..131),
        ];
        assert_eq!(index.associations().len(), owners.len());
        for (association, expected) in index.associations().zip(owners) {
            let owner = index.facts().associated_owner(association, 0).unwrap();
            let Owner::Node(node) = owner.get() else {
                panic!("method owner")
            };
            assert_eq!(site(node.node()), expected);
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_attribute_comment_is_not_a_name_and_bad_names_stay_pending() {
    let source = b"# audit\nattr_accessor(\n# inside\n:a, :b\n)";
    assert_eq!(
        complete_view(source),
        CompleteView {
            documents: one_doc(0..7, "audit"),
            associations: vec![edge(
                0,
                ("call", 8..40),
                ("program", 0..40),
                vec![property(8..40, 0, 32..34), property(8..40, 1, 36..38)]
            )],
            ..CompleteView::default()
        }
    );
    let parser = clean(source);
    for index in indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(index.documents().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().owner_count(), 2);
        for (ordinal, range) in [32..34, 36..38].into_iter().enumerate() {
            let owner = index
                .facts()
                .associated_owner(association, ordinal)
                .unwrap();
            let Owner::Slot(slot) = owner.get() else {
                panic!("property slot")
            };
            assert_eq!(span(&slot.source), range);
        }
    }
    for (source, end) in [
        (b"# audit\nattr_reader :x, true".as_slice(), 28),
        (b"# audit\nattr_reader \"#{name}\"".as_slice(), 29),
    ] {
        assert_eq!(complete_view(source), pending_call(8..end));
        let parser = clean(source);
        for index in indexes(&parser, DocumentationOptions::default()) {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.associations().len(), 0);
            let call = parser
                .root_node()
                .preorder()
                .find(|n| n.kind() == "call")
                .unwrap();
            assert!(
                index
                    .node_projection(call.as_tree_sitter())
                    .unwrap()
                    .pending()
            );
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_visibility_receiver_does_not_project() {
    let source = b"class C\nx = 1\n# audit\nself.private def g; end\nend";
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    assert_eq!(complete_agreed(&indexes, source), CompleteView::default());
    // Selection rejects this call before projection. Force the same registered
    // pending shape to test the projection's independent receiver guard.
    let budget = RubyWork::default();
    assert_eq!(ruby_cost(source, "call", &budget).unwrap(), 0);
    let retained = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    assert_eq!(
        complete_agreed(&self::indexes(&parser, retained), source),
        CompleteView {
            evidence: vec![(CandidateDisposition::Unattached, 14..21)],
            ..CompleteView::default()
        }
    );
    for index in &indexes {
        assert_eq!(index.documents().len(), 0);
        assert_eq!(index.associations().len(), 0);
        let call = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "call")
            .unwrap();
        assert!(matches!(
            index.node_associations(call.as_tree_sitter()),
            Err(OwnerError::UnsupportedOwner)
        ));
        let method = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "method")
            .unwrap();
        let query = index.node_associations(method.as_tree_sitter()).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 0);
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_visibility_comment_is_not_a_method() {
    let source = b"class C\nprivate(\n# audit\ndef f; end\n)\nend";
    assert_eq!(
        complete_view(source),
        CompleteView {
            documents: one_doc(17..24, "audit"),
            associations: vec![edge(
                0,
                ("method", 25..35),
                ("argument_list", 15..37),
                vec![Owned::Node("method", 25..35)]
            )],
            ..CompleteView::default()
        }
    );
    let parser = clean(source);
    // Recognition attaches the inner comment to the method; the projection
    // must still skip that comment when asked about the outer call directly.
    let budget = RubyWork::default();
    assert_eq!(ruby_cost(source, "call", &budget).unwrap(), 1);
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let call = node_at(&parser, "call", 8..37);
    let comment = node_at(&parser, "comment", 17..24);
    // The grammar puts this comment inside the argument list. A canonical
    // call-level comment before the list must likewise not replace the list.
    assert_eq!(
        ruby_call_comment_cost(index, call, comment, &RubyWork::default()).unwrap(),
        1
    );
    for index in indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let call = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "call")
            .unwrap();
        assert!(matches!(
            index.node_associations(call.as_tree_sitter()),
            Err(OwnerError::UnsupportedOwner)
        ));
        let association = index.associations().next().unwrap();
        let Owner::Node(owner) = index
            .facts()
            .associated_owner(association, 0)
            .unwrap()
            .get()
        else {
            panic!("method owner")
        };
        assert_eq!(site(owner.node()), ("method", 25..35));
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_attribute_boolean_and_string_names_resolve() {
    for (source, range) in [
        (b"# audit\nattr :x, true".as_slice(), 13..15),
        (b"# audit\nattr :x, false".as_slice(), 13..15),
        (b"# audit\nattr_reader \"x\"".as_slice(), 20..23),
        (b"# audit\nattr_reader :\"x\"".as_slice(), 20..24),
        (b"# audit\nattr_reader \"x\\ny\"".as_slice(), 20..26),
    ] {
        // YARD rejects a delimited_symbol; R3 deliberately keeps this source-backed slot.
        assert_eq!(
            complete_view(source),
            CompleteView {
                documents: one_doc(0..7, "audit"),
                associations: vec![edge(
                    0,
                    ("call", 8..source.len()),
                    ("program", 0..source.len()),
                    vec![property(8..source.len(), 0, range.clone())]
                )],
                ..CompleteView::default()
            }
        );
        let parser = clean(source);
        for index in indexes(&parser, DocumentationOptions::default()) {
            assert_eq!(index.documents().len(), 1);
            let association = index.associations().next().unwrap();
            assert_eq!(association.get().owner_count(), 1);
            let Owner::Slot(slot) = index
                .facts()
                .associated_owner(association, 0)
                .unwrap()
                .get()
            else {
                panic!("property slot")
            };
            assert_eq!(slot.role, SlotRole::Property);
            assert_eq!(span(&slot.source), range);
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_two_symbol_attr_keeps_both_slots_despite_yard_difference() {
    let source = b"# audit\nattr :x, :y";
    // YARD pops the second argument of two-argument `attr`; R3 deliberately keeps both.
    assert_eq!(
        complete_view(source),
        CompleteView {
            documents: one_doc(0..7, "audit"),
            associations: vec![edge(
                0,
                ("call", 8..19),
                ("program", 0..19),
                vec![property(8..19, 0, 13..15), property(8..19, 1, 17..19)]
            )],
            ..CompleteView::default()
        }
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_visibility_and_attribute_deferred_spellings_are_characterized() {
    let multi = b"# audit\nprivate def f; end, def g; end";
    assert_eq!(
        complete_view(multi),
        CompleteView {
            documents: one_doc(0..7, "audit"),
            associations: vec![edge(
                0,
                ("call", 8..38),
                ("program", 0..38),
                vec![Owned::Node("method", 16..26)]
            )],
            ..CompleteView::default()
        }
    );
    let mixed = b"# audit\nprivate :f, def g; end";
    assert_eq!(complete_view(mixed), CompleteView::default());
    let chained = b"# audit\nattr_reader \"x\" \"y\"";
    let heredoc = b"# audit\nattr_reader <<~E\nx\nE\n";
    assert_eq!(complete_view(chained), pending_call(8..chained.len()));
    assert_eq!(complete_view(heredoc), pending_call(8..24));

    let source = b"class C\n# audit\nprivate attr_accessor :a, :b\nend\n";
    let parser = clean(source);
    assert_eq!(
        complete_agreed(&indexes(&parser, DocumentationOptions::default()), source),
        CompleteView::default()
    );
    let retained = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    assert_eq!(
        complete_agreed(&indexes(&parser, retained), source),
        CompleteView {
            evidence: vec![(CandidateDisposition::Unattached, 8..15)],
            ..CompleteView::default()
        }
    );
    for index in indexes(&parser, DocumentationOptions::default()) {
        let inner = node_at(&parser, "call", 24..44);
        let projection = index.node_projection(inner).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 2);
        for (ordinal, range) in [38..40, 42..44].into_iter().enumerate() {
            let slot = projection.owners().nth(ordinal).unwrap();
            let Owner::Slot(slot) = slot.get() else {
                panic!("property slot")
            };
            assert_eq!(
                (slot.role, slot.ordinal, span(&slot.source)),
                (SlotRole::Property, ordinal, range)
            );
        }
    }

    // V3: EOF commas are sibling ERROR nodes; the clean calls still project.
    for (source, call, owned) in [
        (
            b"# audit\nattr_reader :x,\n".as_slice(),
            8..22,
            property(8..22, 0, 20..22),
        ),
        (
            b"# audit\nprivate def f; end,\n".as_slice(),
            8..26,
            Owned::Node("method", 16..26),
        ),
    ] {
        let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
        assert!(parser.root_node().has_error());
        let result = complete_agreed(&indexes(&parser, DocumentationOptions::default()), source);
        assert_eq!(result.documents, one_doc(0..7, "audit"));
        assert_eq!(
            result.associations,
            [edge(
                0,
                ("call", call),
                ("program", 0..source.len()),
                vec![owned]
            )]
        );
        assert_eq!(result.assessments.len(), 0);
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_visibility_error_forms_do_not_gain_owners() {
    let source = b"class C\nx = 1\n# audit\nprivate def f(a = ); end\nend";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    let indexes = indexes(&parser, DocumentationOptions::default());
    let expected = CompleteView {
        documents: one_doc(14..21, "audit"),
        associations: vec![Edge {
            document: 0,
            resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
            binding: Some(("call", 22..46)),
            scope: ("body_statement", 8..46),
            owners: Vec::new(),
        }],
        assessments: unordered([
            pending(
                "Selected local or wrapper form awaits owner projection",
                Some(("call", 22..46)),
                Some(0),
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("class", 0..50)),
                None,
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("call", 22..46)),
                None,
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("method", 30..46)),
                None,
            ),
        ]),
        ..CompleteView::default()
    };
    assert_eq!(complete_agreed(&indexes, source), expected);
    let call = node_at(&parser, "call", 22..46);
    for index in &indexes {
        assert!(index.node_projection(call).unwrap().pending());
    }

    let source = b"class C\n# audit\nprivate(def f; end, =)\nend\n";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let call = node_at(&parser, "call", 16..38);
    assert!(call.has_error());
    let method = node_at(&parser, "method", 24..34);
    assert!(!method.has_error());
    let actual = complete_agreed(
        &self::indexes(&parser, DocumentationOptions::default()),
        source,
    );
    assert_eq!(actual.documents, one_doc(8..15, "audit"));
    assert_eq!(
        actual.associations,
        [Edge {
            document: 0,
            resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
            binding: Some(("body_statement", 16..38)),
            scope: ("class", 0..42),
            owners: Vec::new(),
        }]
    );
    assert_eq!(
        actual.assessments,
        unordered([
            pending(
                "Selected local or wrapper form awaits owner projection",
                Some(("body_statement", 16..38)),
                Some(0)
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("class", 0..42)),
                None
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("body_statement", 16..38)),
                None
            ),
            pending(
                "Recognized declaration requires a semantic projection",
                Some(("call", 16..38)),
                None
            ),
        ])
    );
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert!(index.node_projection(call).unwrap().pending());
    let method_query = index.node_associations(method).unwrap();
    assert!(!method_query.projection_pending());
    assert_eq!(method_query.associations().len(), 0);
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_attribute_error_forms_do_not_gain_owners() {
    let source = b"# audit\nattr_reader :x, :y = 1";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    assert_eq!(
        complete_agreed(
            &self::indexes(&parser, DocumentationOptions::default()),
            source
        ),
        CompleteView::default()
    );
    assert!(
        parser
            .root_node()
            .preorder()
            .any(|node| node.kind() == "ERROR")
    );

    let source = b"# audit\nattr_reader(:x, :y,\n";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let call = node_at(&parser, "call", 8..27);
    assert!(call.has_error());
    let args = call.child_by_field_name("arguments").unwrap();
    let mut cursor = args.walk();
    assert_eq!(
        args.named_children(&mut cursor)
            .filter(|node| node.kind() == "simple_symbol")
            .count(),
        2
    );
    let actual = complete_agreed(&indexes(&parser, DocumentationOptions::default()), source);
    assert_eq!(
        actual,
        CompleteView {
            documents: one_doc(0..7, "audit"),
            associations: vec![Edge {
                document: 0,
                resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                binding: Some(("call", 8..27)),
                scope: ("program", 0..28),
                owners: Vec::new(),
            }],
            assessments: unordered([
                pending(
                    "Selected local or wrapper form awaits owner projection",
                    Some(("call", 8..27)),
                    Some(0)
                ),
                pending(
                    "Recognized declaration requires a semantic projection",
                    Some(("call", 8..27)),
                    None
                ),
            ]),
            ..CompleteView::default()
        }
    );
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert!(index.node_projection(call).unwrap().pending());
}

#[cfg(feature = "ruby")]
fn ruby_restricted(source: &[u8], gap: Range<usize>) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let mut rows = source[..offset].split(|byte| *byte == b'\n');
        let last = rows.next_back().unwrap();
        tree_sitter::Point::new(rows.count(), last.len())
    };
    let ranges = [0..gap.start, gap.end..source.len()].map(|range| tree_sitter::Range {
        start_byte: range.start,
        end_byte: range.end,
        start_point: point(range.start),
        end_point: point(range.end),
    });
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Ruby.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_excluded_visibility_gaps_are_barriers() {
    let source = b"class C\n# audit\nprivate def f; end\nend";
    let tree = ruby_restricted(source, 28..29);
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let index = collect(LANG::Ruby, &tree, source, &context, &options).unwrap();
    let result = complete(&index, source);
    assert_eq!(result.documents.len(), 0);
    assert_eq!(result.associations.len(), 0);
    assert_eq!(result.assessments.len(), 2);
    assert_eq!(result.evidence.len(), 0);
    let retained = DocumentationOptions {
        retain_unbound: true,
        ..options
    };
    let index = collect(LANG::Ruby, &tree, source, &context, &retained).unwrap();
    let result = complete(&index, source);
    assert_eq!(result.documents.len(), 0);
    assert_eq!(result.associations.len(), 0);
    assert_eq!(result.assessments.len(), 2);
    assert_eq!(result.evidence, [(CandidateDisposition::Unattached, 8..15)]);

    // The grammar retains a call when a letter of `private` is excluded.
    // The exit coverage guard still withholds its semantic projection.
    let tree = ruby_restricted(source, 17..18);
    let mut stack = vec![tree.root_node()];
    let mut calls = Vec::new();
    while let Some(node) = stack.pop() {
        if node.kind() == "call" && node.byte_range() == (16..34) {
            calls.push(node);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    let [call] = calls[..] else {
        panic!("one visibility call")
    };
    let index = collect(LANG::Ruby, &tree, source, &context, &options).unwrap();
    assert!(index.node_projection(call).unwrap().pending());
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_excluded_attribute_gaps_are_barriers() {
    let source = b"# audit\nattr_accessor :a, :b";
    let tree = ruby_restricted(source, 25..26);
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let index = collect(LANG::Ruby, &tree, source, &context, &options).unwrap();
    let result = complete(&index, source);
    assert_eq!(result.documents.len(), 0);
    assert_eq!(result.associations.len(), 0);
    assert_eq!(result.assessments.len(), 2);
    assert_eq!(result.evidence.len(), 0);
    let retained = DocumentationOptions {
        retain_unbound: true,
        ..options
    };
    let index = collect(LANG::Ruby, &tree, source, &context, &retained).unwrap();
    let result = complete(&index, source);
    assert_eq!(result.documents.len(), 0);
    assert_eq!(result.associations.len(), 0);
    assert_eq!(result.assessments.len(), 2);
    assert_eq!(result.evidence, [(CandidateDisposition::Unattached, 0..7)]);
}

#[cfg(feature = "ruby")]
#[derive(Default)]
struct RubyWork {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_step: Option<usize>,
    fail_bytes: Option<usize>,
}

#[cfg(feature = "ruby")]
impl WorkBudget for RubyWork {
    type Error = SourceError;

    #[cfg(feature = "ruby")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }

    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        if self.fail_step == Some(self.steps.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "ruby")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        if self.fail_bytes.is_some_and(|at| self.bytes.get() >= at) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "ruby")]
fn ruby_cost(source: &[u8], kind: &str, budget: &RubyWork) -> Result<usize, CollectionError> {
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let call = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == kind)
        .unwrap();
    ruby_slot_cost(index, call.as_tree_sitter(), budget)
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_projection_charges_exact_steps_and_bytes() {
    for (source, steps, bytes, slots) in [
        // expected: 2 call children, 1 argument, 1 owner-copy; "private" = 7 bytes.
        (b"# d\nprivate def f; end".as_slice(), 4, 7, 1),
        // expected: 2 call children, 3 argument children, 2 slots ×
        // (1 loop + 2 range-search steps); "attr_accessor" = 13 bytes.
        (b"# d\nattr_accessor :a, :b".as_slice(), 11, 13, 2),
        // expected: 2 call children, 1 argument, 3 string children, 1 slot +
        // 2 range-search steps; "attr_reader" = 11 bytes.
        (b"# d\nattr_reader \"x\"".as_slice(), 9, 11, 1),
    ] {
        let budget = RubyWork::default();
        assert_eq!(ruby_cost(source, "call", &budget).unwrap(), slots);
        assert_eq!(budget.steps.get(), steps, "{}", source.escape_ascii());
        assert_eq!(budget.bytes.get(), bytes, "{}", source.escape_ascii());
        for fail_step in 1..=steps {
            let budget = RubyWork {
                fail_step: Some(fail_step),
                ..RubyWork::default()
            };
            assert!(
                matches!(
                    ruby_cost(source, "call", &budget),
                    Err(CollectionError::Cancelled)
                ),
                "step {fail_step}"
            );
        }
        let budget = RubyWork {
            fail_bytes: Some(1),
            ..RubyWork::default()
        };
        assert!(matches!(
            ruby_cost(source, "call", &budget),
            Err(CollectionError::Cancelled)
        ));
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_string_child_scan_is_budgeted_on_both_sides() {
    let source = b"# d\nattr_reader \"x\\ny\"";
    let parser = clean(source);
    let name = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "string")
        .unwrap();
    // expected: quote, content, escape, content, quote = five children.
    let budget = RubyWork::default();
    assert!(ruby_attribute_name(name.as_tree_sitter(), &budget).unwrap());
    assert_eq!(budget.steps.get(), 5);
    assert_eq!(budget.bytes.get(), 0);
    for at in 1..=5 {
        let budget = RubyWork {
            fail_step: Some(at),
            ..RubyWork::default()
        };
        assert!(matches!(
            ruby_attribute_name(name.as_tree_sitter(), &budget),
            Err(CollectionError::Cancelled)
        ));
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_attribute_width_collection_observes_cancellation() {
    let width = 1200;
    let names = (0..width)
        .map(|at| format!(":x{at}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("# audit\nattr_accessor {names}");
    let parser = clean(source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    let index = parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    assert_eq!(
        index.associations().next().unwrap().get().owner_count(),
        width
    );
    let mut scanning = 0;
    let mut slots = 0;
    for checkpoint in 1..=polls {
        let before = observations();
        let result = crate::documentation::budget::cancel_after(checkpoint, || {
            parser.documentation(&context, &options)
        });
        let after = observations();
        assert!(matches!(result, Err(CollectionError::Cancelled)));
        let owners = after.owner_projections - before.owner_projections;
        let searches = after.intersection_searches - before.intersection_searches;
        if owners == 1 && searches < width + 1 {
            scanning += 1;
        }
        if owners == 2 && searches < width + 1 {
            slots += 1;
        }
    }
    // V3 expected: the current walk's 2399 argument children and 1200 slots,
    // together with existing assembly charges, give scanning == 35 and slots == 3.
    // A sibling unit changing walk steps must stop and re-derive these exact bounds.
    assert_eq!(scanning, 35, "{scanning}");
    assert_eq!(slots, 3, "{slots}");
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
        assert_eq!(
            view(source),
            owned(document, "method", owner),
            "{}",
            source.escape_ascii()
        );
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
    assert_eq!(
        view(b"class C\n# a\ndef f; end\n# b\ndef g; end\nend\n"),
        View {
            documents: vec![
                comment(8..11, "a", [(0..1, Copied(10..11))]),
                comment(23..26, "b", [(0..1, Copied(25..26))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owner: ("method", 12..22),
                },
                Link {
                    document: 1,
                    resolution: Resolution::Resolved,
                    owner: ("method", 27..37),
                },
            ],
            ..View::default()
        }
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
        assert_eq!(
            view(source),
            owned(document, "method", owner),
            "{}",
            source.escape_ascii()
        );
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
    for (source, kind, range) in [
        (b"# audit\nalias_method :x, :y\n".as_slice(), "call", 8..27),
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
            },
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"class C\n# audit\nalias_method :x, :y\nend\n"),
        View {
            documents: vec![comment(8..15, "audit", [(0..5, Copied(10..15))])],
            assessments: unordered([
                pending(awaiting, Some(("body_statement", 16..35)), Some(0)),
                pending(projection, Some(("body_statement", 16..35)), None),
                pending(projection, Some(("call", 16..35)), None),
            ]),
            ..View::default()
        }
    );
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
        let comment = node_at(&parser, "comment", range);
        assert!(comment.is_extra());
        assert_eq!(comment.parent(), Some(root));
        assert_eq!(source[comment.end_byte()], b'\n');
    }
    let method = node_at(&parser, "method", 8..17);
    assert_eq!(
        method.child_by_field_name("name").map(site),
        Some(("identifier", 12..13))
    );

    let source = b"=begin\naudit\n=end\ndef f\nend\n";
    let parser = clean(source);
    let comment = node_at(&parser, "comment", 0..17);
    let method = comment.next_sibling().unwrap();
    assert_eq!(comment.parent(), Some(parser.ts_tree().root_node()));
    assert_eq!(site(method), ("method", 18..27));
    assert_eq!(&source[comment.end_byte()..method.start_byte()], b"\n");
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
