//! Python body docstrings in the source view: which module, class or function owns a
//! string statement, the fragments and the text of each document, where each part of
//! the text comes from, and the pending assessments. Every source is collected
//! through both entry points and the two views are compared, with three exceptions.
//! A tree parsed with excluded ranges goes through `collect`, the one entry point
//! that takes such a tree. The scan-count tests and the cancellation tests read
//! thread-local counters around one collection.

#![allow(
    clippy::naive_bytecount,
    clippy::single_range_in_vec_init,
    clippy::type_complexity
)]

#[cfg(feature = "python")]
use crate::documentation::budget::{WorkPhase, cancel_in_phase};
#[cfg(feature = "python")]
use crate::documentation::*;
#[cfg(feature = "python")]
use crate::lang_helpers::python::docstring_runs_scanned;
#[cfg(feature = "python")]
use crate::{AnyParser, LANG};
#[cfg(feature = "python")]
use std::ops::Range;

#[cfg(feature = "python")]
use DocumentRole::{Additional, Primary};
#[cfg(feature = "python")]
use Text::{Complete, Pending, Unavailable};

#[cfg(feature = "python")]
const ATTRIBUTE_RUN: &str = "Python attribute docstring awaits binding-owner projection";
#[cfg(feature = "python")]
const CARRIAGE_RETURN: &str =
    "Python literal with a carriage return awaits line-ending normalization";
#[cfg(feature = "python")]
const PENDING_OWNER: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "python")]
const PENDING_NODE: &str = "Recognized declaration requires a semantic projection";

/// One run of a text: its output range, its origin and the source range it reads.
#[cfg(feature = "python")]
type Map = (Range<usize>, &'static str, Range<usize>);

/// The kind and the byte range of a node.
#[cfg(feature = "python")]
type Site = (&'static str, Range<usize>);

/// One pending assessment: axis, reason, node and the position of its document.
#[cfg(feature = "python")]
type Assessed = (AssessmentAxis, &'static str, Option<Site>, Option<usize>);

#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
enum Text {
    Complete(String, Vec<Map>),
    Pending,
    Unavailable(UnavailableReason),
}

/// One document: role, composition, each fragment as its included pieces, and the
/// state of its text.
#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Vec<Range<usize>>>,
    text: Text,
}

#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
enum Owned {
    File(Range<usize>),
    Node(Site),
}

/// One association: the position of its document, the resolution, its owners and
/// its lexical scope.
#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    resolution: Resolution,
    owners: Vec<Owned>,
    scope: Site,
}

/// What one collection published: the documents and the associations in index
/// order, and the pending assessments without their order. What is the same for
/// every source of this file is asserted by `published` and is in no expected
/// value. Node queries are not part of it.
#[cfg(feature = "python")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    links: Vec<Link>,
    assessments: Vec<Assessed>,
}

#[cfg(feature = "python")]
fn text(text: &str, maps: &[Map]) -> Text {
    Complete(text.to_owned(), maps.to_vec())
}

#[cfg(feature = "python")]
fn doc(role: DocumentRole, fragment: Range<usize>, text: Text) -> Doc {
    Doc {
        role,
        composition: Composition::Single,
        fragments: vec![vec![fragment]],
        text,
    }
}

#[cfg(feature = "python")]
fn concatenation(role: DocumentRole, fragments: &[Range<usize>], text: Text) -> Doc {
    Doc {
        role,
        composition: Composition::LiteralConcatenation,
        fragments: fragments.iter().map(|range| vec![range.clone()]).collect(),
        text,
    }
}

/// A resolved association of document `document` with one owner.
#[cfg(feature = "python")]
fn link(document: usize, owner: Owned, scope: Site) -> Link {
    Link {
        document,
        resolution: Resolution::Resolved,
        owners: vec![owner],
        scope,
    }
}

#[cfg(feature = "python")]
fn site(node: tree_sitter::Node<'_>) -> Site {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "python")]
fn sorted(mut assessments: Vec<Assessed>) -> Vec<Assessed> {
    // The contract states no order for assessments.
    assessments.sort_by_key(|assessment| format!("{assessment:?}"));
    assessments
}

#[cfg(feature = "python")]
fn assessed<'i, 's: 'i>(
    index: &'i DocumentationIndex<'s>,
    assessments: impl Iterator<Item = FactRef<'i, Assessment<'s>>>,
) -> Vec<Assessed> {
    sorted(
        assessments
            .map(|assessment| {
                let document = index
                    .assessed_document(assessment)
                    .unwrap()
                    .map(|document| {
                        index
                            .documents()
                            .position(|other| other == document)
                            .unwrap()
                    });
                let record = assessment.get();
                (
                    record.axis,
                    record.reason,
                    record.node.map(|node| site(node.node())),
                    document,
                )
            })
            .collect(),
    )
}

#[cfg(feature = "python")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the documents, associations and assessments.
    let facts = index.facts();
    assert_eq!(index.profile(), Profile::Python);
    assert_eq!(index.capability(), Capability::Partial);
    // Unit 1 retains no candidate, and no Python document has a qualification record.
    assert_eq!(index.evidence().len(), 0);
    assert_eq!(index.qualifications().len(), 0);
    let mut fragments = 0;
    let documents = index
        .documents()
        .map(|document| {
            fragments += document.get().fragment_count();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| pieces(facts.document_fragment(document, at).unwrap().get()))
                    .collect(),
                text: match document.get().content() {
                    Content::Complete { .. } => {
                        let complete = index.complete_text(document).unwrap().unwrap();
                        Complete(complete.text().to_owned(), maps(&complete))
                    }
                    Content::PendingAdapter => Pending,
                    Content::Unavailable(reason) => Unavailable(*reason),
                    other => panic!("a Python text state, found {other:?}"),
                },
            }
        })
        .collect();
    // Every fragment of the index belongs to a document.
    assert_eq!(facts.fragments().len(), fragments);
    let links = index
        .associations()
        .map(|association| {
            let record = association.get();
            assert_eq!(record.convention.name(), "Python");
            assert_eq!(record.publication, Publication::Unknown);
            assert!(record.binding.is_none());
            let document = facts.associated_document(association).unwrap();
            Link {
                document: index
                    .documents()
                    .position(|other| other == document)
                    .unwrap(),
                resolution: record.resolution,
                owners: (0..record.owner_count())
                    .map(|at| owned(facts.associated_owner(association, at).unwrap().get()))
                    .collect(),
                scope: site(record.lexical_scope.node()),
            }
        })
        .collect();
    View {
        documents,
        links,
        assessments: assessed(index, index.assessments()),
    }
}

#[cfg(feature = "python")]
fn owned(owner: &Owner<'_>) -> Owned {
    match owner {
        Owner::File(file) => {
            assert_eq!(file.node().kind(), "module");
            Owned::File(file.node().byte_range())
        }
        Owner::Node(node) => Owned::Node(site(node.node())),
        other => panic!("the file or a node, found {other:?}"),
    }
}

/// The included pieces of a fragment. The same holds for every fragment here, so no
/// expected view states it: the form is `Literal`, and the node starts with the
/// first piece and ends with the last.
#[cfg(feature = "python")]
fn pieces(fragment: &Fragment<'_>) -> Vec<Range<usize>> {
    let pieces: Vec<_> = fragment
        .source()
        .pieces()
        .iter()
        .map(SourcePiece::range)
        .collect();
    assert_eq!(fragment.form(), FragmentForm::Literal);
    let node = fragment.node().unwrap().node();
    assert_eq!(
        node.byte_range(),
        pieces[0].start..pieces[pieces.len() - 1].end
    );
    assert_eq!(fragment.source().has_excluded_gap(), pieces.len() > 1);
    pieces
}

#[cfg(feature = "python")]
fn maps(complete: &CompleteText<'_, '_>) -> Vec<Map> {
    complete
        .mappings()
        .iter()
        .map(|mapping| {
            let (kind, source) = match &mapping.origin {
                Origin::Copied(source) => ("copied", source),
                Origin::Decoded(source) => ("decoded", source),
                Origin::Expanded(source) => ("expanded", source),
                Origin::Synthetic => panic!("no Python run is synthetic"),
            };
            let [piece] = source.pieces() else {
                panic!("one source piece, found {:?}", source.pieces())
            };
            (mapping.output.range(), kind, piece.range())
        })
        .collect()
}

/// The kind of the node of every fragment of the index, in index order.
#[cfg(feature = "python")]
fn fragment_kinds(index: &DocumentationIndex<'_>) -> Vec<&'static str> {
    index
        .facts()
        .fragments()
        .map(|fragment| fragment.get().node().unwrap().node().kind())
        .collect()
}

/// The one node of `kind` with exactly `range`.
#[cfg(feature = "python")]
fn node<'t>(tree: &'t tree_sitter::Tree, kind: &str, range: Range<usize>) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut cursor = tree.walk();
    loop {
        if cursor.node().kind() == kind && cursor.node().byte_range() == range {
            found.push(cursor.node());
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                assert_eq!(found.len(), 1, "{kind} {range:?}");
                return found[0];
            }
        }
    }
}

#[cfg(feature = "python")]
fn count(tree: &tree_sitter::Tree, kind: &str) -> usize {
    let mut found = 0;
    let mut cursor = tree.walk();
    loop {
        found += usize::from(cursor.node().kind() == kind);
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

/// What the owner query of a node gives: whether its projection is pending, the
/// position of the document of each association, and the assessments.
#[cfg(feature = "python")]
fn queried(
    index: &DocumentationIndex<'_>,
    node: tree_sitter::Node<'_>,
) -> (bool, Vec<usize>, Vec<Assessed>) {
    let query = index.node_associations(node).unwrap();
    let documents = query
        .associations()
        .map(|association| {
            let document = index.facts().associated_document(association).unwrap();
            index
                .documents()
                .position(|other| other == document)
                .unwrap()
        })
        .collect();
    (
        query.projection_pending(),
        documents,
        assessed(index, query.assessments()),
    )
}

/// Collects `source` through both entry points, compares the two views with each
/// other and gives the typed index, its tree and its view to `check`.
#[cfg(feature = "python")]
fn collected(source: &[u8], check: impl Fn(&DocumentationIndex<'_>, &tree_sitter::Tree, View)) {
    let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let typed = parser.documentation(&context, &options).unwrap();
    let raw = collect(
        LANG::Python,
        parser.ts_tree(),
        parser.code(),
        &context,
        &options,
    )
    .unwrap();
    assert_eq!(published(&typed), published(&raw));
    check(&typed, parser.ts_tree(), published(&typed));
    check(&raw, parser.ts_tree(), published(&raw));
}

#[cfg(feature = "python")]
fn view(source: &[u8]) -> View {
    let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let typed = published(&parser.documentation(&context, &options).unwrap());
    let raw = collect(
        LANG::Python,
        parser.ts_tree(),
        parser.code(),
        &context,
        &options,
    )
    .unwrap();
    assert_eq!(typed, published(&raw));
    typed
}

/// Parses `source` with exactly the `included` byte ranges, collects it through
/// `collect` and gives the index, the tree and the view to `check`. A point is the
/// number of line feeds before the offset and the bytes since the last one.
#[cfg(feature = "python")]
fn restricted(
    source: &[u8],
    included: &[Range<usize>],
    check: impl Fn(&DocumentationIndex<'_>, &tree_sitter::Tree, View),
) {
    let point = |at: usize| {
        let before = &source[..at];
        tree_sitter::Point::new(
            before.iter().filter(|&&byte| byte == b'\n').count(),
            before
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(at, |feed| at - feed - 1),
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
        .set_language(&LANG::Python.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let index = collect(
        LANG::Python,
        &tree,
        source,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    check(&index, &tree, published(&index));
}

/// How many backward scans the reader makes during one collection through each
/// entry point.
#[cfg(feature = "python")]
fn scans(source: &[u8]) -> [usize; 2] {
    let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let start = docstring_runs_scanned::observed();
    parser.documentation(&context, &options).unwrap();
    let typed = docstring_runs_scanned::observed();
    collect(
        LANG::Python,
        parser.ts_tree(),
        parser.code(),
        &context,
        &options,
    )
    .unwrap();
    [typed - start, docstring_runs_scanned::observed() - typed]
}

#[cfg(feature = "python")]
const P1: &[u8] = b"def f():\n    \"audit\"\n    pass";
#[cfg(feature = "python")]
const P9A: &[u8] = b"\"\"\"a\"\"\"\n\"\"\"b\"\"\"\n# c\n\"\"\"c\"\"\"\nx = 1\n\"\"\"d\"\"\"\n";

#[cfg(feature = "python")]
#[test]
fn python_function_primary_has_exact_owner_fragment_text_and_queries() {
    collected(P1, |index, tree, view| {
        let function = node(tree, "function_definition", 0..29);
        assert_eq!(
            view,
            View {
                documents: vec![doc(
                    Primary,
                    13..20,
                    text("audit", &[(0..5, "copied", 14..19)])
                )],
                links: vec![link(0, Owned::Node(site(function)), ("block", 13..29))],
                assessments: vec![],
            }
        );
        assert_eq!(fragment_kinds(index), ["string"]);
        let document = index.documents().next().unwrap();
        let fragment = index.facts().document_fragment(document, 0).unwrap();
        assert_eq!(fragment.get().source().pieces()[0].bytes(), b"\"audit\"");
        assert_eq!(queried(index, function), (false, vec![0], vec![]));
        let owner = index.node_owners(function).unwrap().next().unwrap();
        assert!(matches!(owner.get(), Owner::Node(_)));
        let by_owner = index.owner_associations(owner).unwrap();
        assert_eq!(by_owner.associations().len(), 1);
        assert_eq!(by_owner.assessments().count(), 0);
        assert_eq!(queried(index, tree.root_node()), (false, vec![], vec![]));
        // The same result as the cross-language fixture `primary.py` requires.
        let complete = index.complete_text(document).unwrap().unwrap();
        crate::test_support::documentation::assert_primary_extraction(
            LANG::Python,
            Ok(serde_json::json!([{
                "fragments": [[13, 20]],
                "owner": [function.start_byte(), function.end_byte()],
                "owner_kind": function.kind(),
                "text": complete.text(),
            }])),
        );
    });
}

#[cfg(feature = "python")]
#[test]
fn python_class_and_module_primaries_have_their_own_owners() {
    collected(b"class C:\n    \"audit\"\n    pass", |index, tree, view| {
        let class = node(tree, "class_definition", 0..29);
        assert_eq!(
            view,
            View {
                documents: vec![doc(
                    Primary,
                    13..20,
                    text("audit", &[(0..5, "copied", 14..19)])
                )],
                links: vec![link(0, Owned::Node(site(class)), ("block", 13..29))],
                assessments: vec![],
            }
        );
        assert_eq!(queried(index, class), (false, vec![0], vec![]));
    });
    collected(b"\"audit\"\nx = 1", |index, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![doc(Primary, 0..7, text("audit", &[(0..5, "copied", 1..6)]))],
                links: vec![link(0, Owned::File(0..13), ("module", 0..13))],
                assessments: vec![],
            }
        );
        assert_eq!(queried(index, tree.root_node()), (false, vec![0], vec![]));
    });
}

#[cfg(feature = "python")]
#[test]
fn python_empty_primary_and_additional_are_two_documents_for_one_owner() {
    let source = b"def f():\n    \"\"\n    \"extra\"\n    return 1\n";
    collected(source, |index, tree, view| {
        let function = node(tree, "function_definition", 0..40);
        let owner = || Owned::Node(site(function));
        assert_eq!(
            view,
            View {
                documents: vec![
                    // The empty docstring is a document: its text is `""` with no run.
                    doc(Primary, 13..15, text("", &[])),
                    doc(
                        Additional,
                        20..27,
                        text("extra", &[(0..5, "copied", 21..26)])
                    ),
                ],
                links: vec![
                    link(0, owner(), ("block", 13..40)),
                    link(1, owner(), ("block", 13..40)),
                ],
                assessments: vec![],
            }
        );
        let empty = index.documents().next().unwrap();
        let complete = index.complete_text(empty).unwrap().unwrap();
        assert_eq!(complete.origins(0..0).unwrap().count(), 0);
        let fragment = index.facts().document_fragment(empty, 0).unwrap();
        assert_eq!(fragment.get().source().pieces()[0].bytes(), b"\"\"");
        assert_eq!(queried(index, function), (false, vec![0, 1], vec![]));
    });
}

#[cfg(feature = "python")]
#[test]
fn python_concatenation_is_one_document_without_separator() {
    let source = b"def f():\n    (\"@au\" # ordinary\n     \"dit\")\n";
    collected(source, |index, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![concatenation(
                    Primary,
                    &[14..19, 36..41],
                    text(
                        "@audit",
                        &[(0..3, "copied", 15..18), (3..6, "copied", 37..40)]
                    )
                )],
                links: vec![link(
                    0,
                    Owned::Node(site(node(tree, "function_definition", 0..42))),
                    ("block", 13..42)
                )],
                assessments: vec![],
            }
        );
        // The comment between the two literals is no fragment.
        assert_eq!(fragment_kinds(index), ["string", "string"]);
    });
    // A raw literal keeps its backslash; a `u` literal joins it with nothing between.
    let view = view(b"def f():\n    R\"a\\nb\" u\"c\"\n");
    assert_eq!(
        view.documents,
        [concatenation(
            Primary,
            &[13..20, 21..25],
            text(
                "a\\nbc",
                &[(0..4, "copied", 15..19), (4..5, "copied", 23..24)]
            )
        )]
    );
}

#[cfg(feature = "python")]
#[test]
fn python_escapes_decode_to_whole_tokens_and_raw_stays_raw() {
    let rows: [(&[u8], Range<usize>, &str, &[Map]); 6] = [
        (
            b"\"a\\x40b\"\nx = 1",
            0..8,
            "a@b",
            &[
                (0..1, "copied", 1..2),
                (1..2, "decoded", 2..6),
                (2..3, "copied", 6..7),
            ],
        ),
        (
            b"def f():\n    r\"a\\nb\"\n    pass",
            13..20,
            "a\\nb",
            &[(0..4, "copied", 15..19)],
        ),
        // A backslash before a line feed is removed with it and leaves no run.
        (
            b"def f():\n    \"a\\\nb\"\n",
            13..19,
            "ab",
            &[(0..1, "copied", 14..15), (1..2, "copied", 17..18)],
        ),
        // An unknown escape is kept, as a run of its own.
        (
            b"def f():\n    \"a\\qb\"\n",
            13..19,
            "a\\qb",
            &[
                (0..1, "copied", 14..15),
                (1..3, "copied", 15..17),
                (3..4, "copied", 17..18),
            ],
        ),
        // `\r` spelled as an escape is a decoded carriage return, not a CR byte.
        (
            b"def f():\n    \"a\\rb\"\n",
            13..19,
            "a\rb",
            &[
                (0..1, "copied", 14..15),
                (1..2, "decoded", 15..17),
                (2..3, "copied", 17..18),
            ],
        ),
        // A `u` prefix is a cooked literal.
        (
            b"def f():\n    u\"a\\x40b\"\n",
            13..22,
            "a@b",
            &[
                (0..1, "copied", 15..16),
                (1..2, "decoded", 16..20),
                (2..3, "copied", 20..21),
            ],
        ),
    ];
    for (source, fragment, expected, maps) in rows {
        let view = view(source);
        assert_eq!(
            view.documents,
            [doc(Primary, fragment, text(expected, maps))],
            "{}",
            source.escape_ascii()
        );
        assert_eq!(view.assessments, [], "{}", source.escape_ascii());
    }
}

#[cfg(feature = "python")]
#[test]
fn python_undecodable_text_is_unavailable_and_keeps_raw_bytes() {
    // CPython decodes the first source to `@audit`. Named escapes are deliberately
    // not decoded in this unit, and the document says so by its text state.
    let source = b"def f():\n    \"\\N{COMMERCIAL AT}audit\"\n    return 1\n";
    collected(source, |index, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![doc(
                    Primary,
                    13..37,
                    Unavailable(UnavailableReason::UnsupportedDecoding)
                )],
                links: vec![link(
                    0,
                    Owned::Node(site(node(tree, "function_definition", 0..50))),
                    ("block", 13..50)
                )],
                assessments: vec![],
            }
        );
        let document = index.documents().next().unwrap();
        assert!(index.complete_text(document).unwrap().is_none());
        let fragment = index.facts().document_fragment(document, 0).unwrap();
        assert_eq!(fragment.get().source().pieces()[0].bytes(), &source[13..37]);
    });
    // A lone surrogate, and a `\x` escape with one digit.
    assert_eq!(
        view(b"def f():\n    \"\\ud800\"\n").documents,
        [doc(
            Primary,
            13..21,
            Unavailable(UnavailableReason::NonScalar)
        )]
    );
    assert_eq!(
        view(b"def f():\n    \"\\x4\"\n").documents,
        [doc(
            Primary,
            13..18,
            Unavailable(UnavailableReason::InvalidEncoding)
        )]
    );
}

#[cfg(feature = "python")]
#[test]
fn python_triple_quoted_text_follows_cleandoc_with_exact_runs() {
    assert_eq!(
        view(b"def f():\n    \"\"\"First.\n\n    Second.\n    \"\"\"\n").documents,
        [doc(
            Primary,
            13..43,
            text(
                "First.\n\nSecond.",
                &[
                    (0..6, "copied", 16..22),
                    (6..7, "copied", 22..23),
                    (7..8, "copied", 23..24),
                    (8..15, "copied", 28..35),
                ]
            )
        )]
    );
    assert_eq!(
        view(b"def f():\n\t\"\"\"a\n\tb\"\"\"\n").documents,
        [doc(
            Primary,
            10..20,
            text(
                "a\nb",
                &[
                    (0..1, "copied", 13..14),
                    (1..2, "copied", 14..15),
                    (2..3, "copied", 16..17),
                ]
            )
        )]
    );
}

#[cfg(feature = "python")]
#[test]
fn python_carriage_return_in_a_literal_is_pending_with_one_assessment() {
    // Pending outcomes: the later targets are `First.\n\nSecond.`, `a\nb` and `ab\nc`.
    let composition = |node: Site| {
        (
            AssessmentAxis::Composition,
            CARRIAGE_RETURN,
            Some(node),
            Some(0),
        )
    };
    let source = b"def f():\r\n    \"\"\"First.\r\n\r\n    Second.\r\n    \"\"\"\r\n";
    collected(source, |index, tree, view| {
        let function = node(tree, "function_definition", 0..47);
        assert_eq!(
            view,
            View {
                documents: vec![doc(Primary, 14..47, Pending)],
                links: vec![link(0, Owned::Node(site(function)), ("block", 14..47))],
                assessments: vec![composition(("string", 14..47))],
            }
        );
        let document = index.documents().next().unwrap();
        assert!(index.complete_text(document).unwrap().is_none());
        assert_eq!(
            queried(index, function),
            (false, vec![0], vec![composition(("string", 14..47))])
        );
    });
    collected(b"def f():\n    \"\"\"a\rb\"\"\"\n", |_, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![doc(Primary, 13..22, Pending)],
                links: vec![link(
                    0,
                    Owned::Node(site(node(tree, "function_definition", 0..22))),
                    ("block", 13..22)
                )],
                assessments: vec![composition(("string", 13..22))],
            }
        );
    });
    // The CR is in the second fragment; the assessment names the first.
    let source = b"def f():\n    (\"a\"\n     \"\"\"b\rc\"\"\")\n";
    collected(source, |_, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![concatenation(Primary, &[14..17, 23..32], Pending)],
                links: vec![link(
                    0,
                    Owned::Node(site(node(tree, "function_definition", 0..33))),
                    ("block", 13..33)
                )],
                assessments: vec![composition(("string", 14..17))],
            }
        );
    });
    // The assessment of an owner does not reach a definition nested in it.
    let source = b"def f():\n    \"\"\"a\rb\"\"\"\n    def g():\n        return 1\n";
    collected(source, |index, tree, view| {
        let outer = node(tree, "function_definition", 0..52);
        assert_eq!(view.documents, [doc(Primary, 13..22, Pending)]);
        assert_eq!(
            view.links,
            [link(0, Owned::Node(site(outer)), ("block", 13..52))]
        );
        assert_eq!(view.assessments, [composition(("string", 13..22))]);
        assert_eq!(
            queried(index, outer),
            (false, vec![0], vec![composition(("string", 13..22))])
        );
        let inner = node(tree, "function_definition", 27..52);
        assert_eq!(queried(index, inner), (false, vec![], vec![]));
    });
}

#[cfg(feature = "python")]
#[test]
fn python_additional_and_attribute_runs_are_told_apart() {
    // Pending outcomes: a run after an assignment gives one assessment and no document.
    let attribute = |range: Range<usize>| {
        (
            AssessmentAxis::Owner,
            ATTRIBUTE_RUN,
            Some(("expression_statement", range)),
            None,
        )
    };
    collected(P9A, |index, tree, view| {
        let one =
            |start: usize, letter: &str| text(letter, &[(0..1, "copied", start + 3..start + 4)]);
        assert_eq!(
            view,
            View {
                documents: vec![
                    doc(Primary, 0..7, one(0, "a")),
                    doc(Additional, 8..15, one(8, "b")),
                    doc(Additional, 20..27, one(20, "c")),
                ],
                links: vec![
                    link(0, Owned::File(0..42), ("module", 0..42)),
                    link(1, Owned::File(0..42), ("module", 0..42)),
                    link(2, Owned::File(0..42), ("module", 0..42)),
                ],
                assessments: vec![attribute(34..41)],
            }
        );
        // The string after the assignment is no fragment.
        assert_eq!(fragment_kinds(index).len(), 3);
        assert_eq!(
            queried(index, tree.root_node()),
            (false, vec![0, 1, 2], vec![attribute(34..41)])
        );
    });
    let source = b"class C:\n    x = 1\n    \"x doc\"\n    def __init__(self):\n        \
self.a = 1\n        \"a doc\"\n";
    collected(source, |index, tree, view| {
        assert_eq!(
            view,
            View {
                assessments: sorted(vec![attribute(23..30), attribute(82..89)]),
                ..View::default()
            }
        );
        // Each owner's query yields its own run and no other.
        let class = node(tree, "class_definition", 0..89);
        assert_eq!(
            queried(index, class),
            (false, vec![], vec![attribute(23..30)])
        );
        let init = node(tree, "function_definition", 35..89);
        assert_eq!(
            queried(index, init),
            (false, vec![], vec![attribute(82..89)])
        );
        assert_eq!(queried(index, tree.root_node()), (false, vec![], vec![]));
    });
    // Two members after an assignment are one run: the assessment names the first.
    let source = b"x = 1\n\"x doc\"\n\"more\"\ndef f():\n    return 1\n";
    collected(source, |index, tree, view| {
        assert_eq!(
            view,
            View {
                assessments: vec![attribute(6..13)],
                ..View::default()
            }
        );
        assert_eq!(
            queried(index, tree.root_node()),
            (false, vec![], vec![attribute(6..13)])
        );
        let function = node(tree, "function_definition", 21..42);
        assert_eq!(queried(index, function), (false, vec![], vec![]));
    });
    // A class that holds a syntax error is a pending owner: the assessment waits on
    // its node, beside the one the core adds for the class.
    let source = b"class C:\n    x = 1\n    \"x doc\"\n    y = = 2\n";
    collected(source, |index, tree, view| {
        let class = node(tree, "class_definition", 0..42);
        let pending = sorted(vec![
            attribute(23..30),
            (AssessmentAxis::Owner, PENDING_NODE, Some(site(class)), None),
        ]);
        assert_eq!(
            view,
            View {
                assessments: pending.clone(),
                ..View::default()
            }
        );
        assert_eq!(queried(index, class), (true, vec![], pending));
        assert_eq!(queried(index, tree.root_node()), (false, vec![], vec![]));
    });
}

#[cfg(feature = "python")]
#[test]
fn python_collection_scans_each_docstring_run_once() {
    // expected: 2 for each entry point: the run `a`, `b`, `c` is scanned at `c`, and
    // the run `d` at `d`.
    assert_eq!(scans(P9A), [2, 2]);
}

#[cfg(feature = "python")]
#[test]
fn python_owners_are_definitions_not_wrappers_or_bodies() {
    let primary = |fragment: Range<usize>, word: &str| {
        let interior = fragment.start + 1..fragment.end - 1;
        doc(
            Primary,
            fragment,
            text(word, &[(0..word.len(), "copied", interior)]),
        )
    };
    let source = b"@d\ndef f():\n    \"outer\"\n    def g():\n        \"inner\"";
    collected(source, |index, tree, view| {
        let outer = node(tree, "function_definition", 3..52);
        let inner = node(tree, "function_definition", 28..52);
        assert_eq!(
            view,
            View {
                documents: vec![primary(16..23, "outer"), primary(45..52, "inner")],
                links: vec![
                    link(0, Owned::Node(site(outer)), ("block", 16..52)),
                    link(1, Owned::Node(site(inner)), ("block", 45..52)),
                ],
                assessments: vec![],
            }
        );
        assert_eq!(queried(index, outer), (false, vec![0], vec![]));
        assert_eq!(queried(index, inner), (false, vec![1], vec![]));
        // The decorator wrapper is no owner.
        let wrapper = node(tree, "decorated_definition", 0..52);
        assert!(matches!(
            index.node_associations(wrapper),
            Err(OwnerError::UnsupportedOwner)
        ));
        assert_eq!(queried(index, tree.root_node()), (false, vec![], vec![]));
    });
    collected(
        b"def f():\n    class C:\n        \"inner\"\n",
        |index, tree, view| {
            let class = node(tree, "class_definition", 13..37);
            assert_eq!(view.documents, [primary(30..37, "inner")]);
            assert_eq!(
                view.links,
                [link(0, Owned::Node(site(class)), ("block", 30..37))]
            );
            let function = node(tree, "function_definition", 0..37);
            assert_eq!(queried(index, function), (false, vec![], vec![]));
        },
    );
    collected(b"async def f():\n    \"audit\"\n", |_, tree, view| {
        let function = node(tree, "function_definition", 0..26);
        assert_eq!(view.documents, [primary(19..26, "audit")]);
        assert_eq!(
            view.links,
            [link(0, Owned::Node(site(function)), ("block", 19..26))]
        );
    });
    collected(
        b"class C:\n    def m(self):\n        \"audit\"\n",
        |index, tree, view| {
            let method = node(tree, "function_definition", 13..41);
            assert_eq!(view.documents, [primary(34..41, "audit")]);
            assert_eq!(
                view.links,
                [link(0, Owned::Node(site(method)), ("block", 34..41))]
            );
            let class = node(tree, "class_definition", 0..41);
            assert_eq!(queried(index, class), (false, vec![], vec![]));
        },
    );
}

#[cfg(feature = "python")]
#[test]
fn python_strings_outside_documenting_positions_produce_nothing() {
    let sources: [&[u8]; 11] = [
        b"def f():\n    b\"audit\"\n    pass",
        b"def f():\n    f\"audit\"\n    pass",
        b"def f():\n    t\"audit\"\n    pass",
        b"x = 1\nprint(x)\n\"audit\"",
        b"import os\n\"audit\"",
        b"def f():\n    x = 1\n    \"no\"\n    return x",
        b"def f():\n    \"a\", \"b\"\n    pass",
        b"if True:\n    \"no\"",
        b"def f():\n    \"a\" b\"b\"\n    pass\n",
        b"x = \"audit\"",
        b"x = 1\nf\"audit\"\n",
    ];
    for source in sources {
        collected(source, |_, tree, view| {
            // The control: the source holds a string the collector could have taken.
            assert_ne!(count(tree, "string"), 0, "{}", source.escape_ascii());
            assert_eq!(view, View::default(), "{}", source.escape_ascii());
        });
    }
    // The control of the whole list: the same statement in a documenting position.
    assert_eq!(view(P1).documents.len(), 1);
}

#[cfg(feature = "python")]
#[test]
fn python_profiles_resolve_and_an_empty_file_is_an_empty_index() {
    let context = DocumentationContext::default();
    let parser = AnyParser::parse(LANG::Python, P1.to_vec(), None, None).unwrap();
    for profile in [Profile::Default, Profile::Python] {
        let options = DocumentationOptions {
            profile,
            ..DocumentationOptions::default()
        };
        for index in [
            parser.documentation(&context, &options).unwrap(),
            collect(
                LANG::Python,
                parser.ts_tree(),
                parser.code(),
                &context,
                &options,
            )
            .unwrap(),
        ] {
            assert_eq!(index.profile(), Profile::Python);
            assert_eq!(index.capability(), Capability::Partial);
            assert_eq!(index.documents().len(), 1);
        }
    }
    let options = DocumentationOptions {
        profile: Profile::Rust,
        ..DocumentationOptions::default()
    };
    let unsupported = CollectionError::UnsupportedProfile {
        language: LANG::Python,
        profile: Profile::Rust,
    };
    assert_eq!(
        parser.documentation(&context, &options).map(|_| ()),
        Err(unsupported)
    );
    assert_eq!(
        collect(
            LANG::Python,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options
        )
        .map(|_| ()),
        Err(unsupported)
    );
    collected(b"", |index, tree, view| {
        assert_eq!(view, View::default());
        let owners: Vec<_> = index.facts().owners().collect();
        assert_eq!(owners.len(), 1);
        assert!(matches!(owners[0].get(), Owner::File(_)));
        assert_eq!(index.facts().fragments().len(), 0);
        assert_eq!(queried(index, tree.root_node()), (false, vec![], vec![]));
    });
}

#[cfg(feature = "python")]
#[test]
fn python_syntax_errors_keep_text_and_mark_only_the_broken_owner() {
    // An error inside the function: the text is complete, and the owner is pending.
    collected(
        b"def f():\n    \"audit\"\n    x = = 1\n",
        |index, tree, view| {
            let function = node(tree, "function_definition", 0..32);
            let pending = sorted(vec![
                (AssessmentAxis::Owner, PENDING_OWNER, None, Some(0)),
                (
                    AssessmentAxis::Owner,
                    PENDING_NODE,
                    Some(site(function)),
                    None,
                ),
            ]);
            assert_eq!(
                view,
                View {
                    documents: vec![doc(
                        Primary,
                        13..20,
                        text("audit", &[(0..5, "copied", 14..19)])
                    )],
                    links: vec![],
                    assessments: pending.clone(),
                }
            );
            assert_eq!(queried(index, function), (true, vec![], pending));
        },
    );
    // The root is an `ERROR`: there is no module to hold a docstring.
    collected(b"def f():\n    \"audit\"\n    x = (\n", |_, tree, view| {
        assert!(tree.root_node().is_error());
        assert_ne!(count(tree, "string"), 0);
        assert_eq!(view, View::default());
    });
    // An error elsewhere in the file leaves the module docstring and a clean
    // definition resolved.
    collected(b"\"doc\"\nx = = 1\n", |_, tree, view| {
        assert!(tree.root_node().has_error());
        assert_eq!(
            view,
            View {
                documents: vec![doc(Primary, 0..5, text("doc", &[(0..3, "copied", 1..4)]))],
                links: vec![link(0, Owned::File(0..14), ("module", 0..14))],
                assessments: vec![],
            }
        );
    });
    // An error inside a class: the same as for the function above.
    collected(
        b"class C:\n    \"doc\"\n    x = = 1\n",
        |index, tree, view| {
            let class = node(tree, "class_definition", 0..30);
            let pending = sorted(vec![
                (AssessmentAxis::Owner, PENDING_OWNER, None, Some(0)),
                (AssessmentAxis::Owner, PENDING_NODE, Some(site(class)), None),
            ]);
            assert_eq!(
                view,
                View {
                    documents: vec![doc(
                        Primary,
                        13..18,
                        text("doc", &[(0..3, "copied", 14..17)])
                    )],
                    links: vec![],
                    assessments: pending.clone(),
                }
            );
            assert_eq!(queried(index, class), (true, vec![], pending));
        },
    );
    collected(b"x = [\ndef f():\n    \"doc\"\n", |_, tree, view| {
        assert!(tree.root_node().has_error());
        let function = node(tree, "function_definition", 6..24);
        assert_eq!(
            view.documents,
            [doc(
                Primary,
                19..24,
                text("doc", &[(0..3, "copied", 20..23)])
            )]
        );
        assert_eq!(
            view.links,
            [link(0, Owned::Node(site(function)), ("block", 19..24))]
        );
    });
}

#[cfg(feature = "python")]
#[test]
fn python_long_run_is_one_scan_and_one_text_build_per_document() {
    let lines: Vec<_> = (0..300).map(|at| format!("\"d{at}\"")).collect();
    let source = format!("{}\n", lines.join("\n"));
    let parser = AnyParser::parse(LANG::Python, source.into_bytes(), None, None).unwrap();
    let scans = docstring_runs_scanned::observed();
    let renders = observations().renders;
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    // expected: 1, the scan at the last member of the one run.
    assert_eq!(docstring_runs_scanned::observed() - scans, 1);
    // expected: 900 = 300 documents x 3 TextBuilder::finish calls (decoded value, tab
    // expansion, margin).
    assert_eq!(observations().renders - renders, 900);
    let view = published(&index);
    assert_eq!(view.documents.len(), 300);
    for (at, document) in view.documents.iter().enumerate() {
        assert_eq!(
            document.role,
            if at == 0 { Primary } else { Additional },
            "{at}"
        );
        let Complete(text, _) = &document.text else {
            panic!("complete text, found {:?}", document.text)
        };
        assert_eq!(*text, format!("d{at}"));
    }
    assert_eq!(view.links.len(), 300);
    for (at, link) in view.links.iter().enumerate() {
        assert_eq!(link.document, at);
        assert_eq!(link.resolution, Resolution::Resolved);
        assert_eq!(link.owners.len(), 1);
        assert!(matches!(link.owners[0], Owned::File(_)));
    }
    assert_eq!(view.assessments, []);
}

#[cfg(feature = "python")]
#[test]
fn python_text_construction_observes_cancellation() {
    let source = format!("def f():\n    \"\"\"{}\"\"\"\n", "a".repeat(40_000));
    let parser = AnyParser::parse(LANG::Python, source.into_bytes(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let result = cancel_in_phase(WorkPhase::Text, || {
        parser.documentation(&context, &options).map(|_| ())
    });
    assert_eq!(result, Err(CollectionError::Cancelled));
    // The control: the same collection without a cancellation.
    let index = parser.documentation(&context, &options).unwrap();
    let document = index.documents().next().unwrap();
    let complete = index.complete_text(document).unwrap().unwrap();
    assert_eq!(complete.text().len(), 40_000);
}

#[cfg(feature = "python")]
#[test]
fn python_excluded_gap_makes_text_unavailable_and_pending_wins() {
    // The NUL byte stands for bytes of another language that the parse leaves out.
    restricted(b"\"au\0dit\"\nx = 1", &[0..3, 4..14], |_, _, view| {
        assert_eq!(
            view.documents,
            [Doc {
                role: Primary,
                composition: Composition::Single,
                fragments: vec![vec![0..3, 4..8]],
                text: Unavailable(UnavailableReason::ExcludedGap),
            }]
        );
        // The position before the value is empty, so the attachment is resolved.
        assert_eq!(view.links, [link(0, Owned::File(0..14), ("module", 0..14))]);
    });
    // The fragment with the gap comes first: the pending state wins by its rank,
    // not by the order of the fragments.
    let source = b"\"c\0d\" \"\"\"a\rb\"\"\"\nx = 1";
    restricted(source, &[0..2, 3..21], |_, _, view| {
        assert_eq!(
            view.documents,
            [Doc {
                role: Primary,
                composition: Composition::LiteralConcatenation,
                fragments: vec![vec![0..2, 3..5], vec![6..15]],
                text: Pending,
            }]
        );
        assert_eq!(view.links, [link(0, Owned::File(0..21), ("module", 0..21))]);
        // Nodes that hold the gap are pending owners of their own; only the
        // composition assessment is this unit's.
        let composition: Vec<_> = view
            .assessments
            .iter()
            .filter(|assessment| assessment.0 == AssessmentAxis::Composition)
            .collect();
        assert_eq!(
            composition,
            [&(
                AssessmentAxis::Composition,
                CARRIAGE_RETURN,
                Some(("string", 0..5)),
                Some(0)
            )]
        );
    });
}

#[cfg(feature = "python")]
#[test]
fn python_semicolon_between_statements_does_not_break_a_run() {
    let letter = |start: usize, letter: &str| {
        doc(
            if start == 0 || start == 13 {
                Primary
            } else {
                Additional
            },
            start..start + 3,
            text(letter, &[(0..1, "copied", start + 1..start + 2)]),
        )
    };
    let attribute = |range: Range<usize>| {
        (
            AssessmentAxis::Owner,
            ATTRIBUTE_RUN,
            Some(("expression_statement", range)),
            None,
        )
    };
    let source = b"\"a\";\n\"b\"\nx = 1\n";
    assert_eq!(
        view(source),
        View {
            documents: vec![letter(0, "a"), letter(5, "b")],
            links: vec![
                link(0, Owned::File(0..15), ("module", 0..15)),
                link(1, Owned::File(0..15), ("module", 0..15)),
            ],
            assessments: vec![],
        }
    );
    // expected: 1 scan for each source, at its last string.
    assert_eq!(scans(source), [1, 1]);
    let source = b"x = 1;\n\"x doc\"\n";
    assert_eq!(
        view(source),
        View {
            assessments: vec![attribute(7..14)],
            ..View::default()
        }
    );
    assert_eq!(scans(source), [1, 1]);
    let source = b"def f():\n    \"a\"; \"b\"\n    return 1\n";
    collected(source, |_, tree, view| {
        let function = node(tree, "function_definition", 0..34);
        assert_eq!(
            view,
            View {
                documents: vec![letter(13, "a"), letter(18, "b")],
                links: vec![
                    link(0, Owned::Node(site(function)), ("block", 13..34)),
                    link(1, Owned::Node(site(function)), ("block", 13..34)),
                ],
                assessments: vec![],
            }
        );
    });
    assert_eq!(scans(source), [1, 1]);
    let source = b"\"\"\"a\nA\"\"\"; \"\"\"b\nB\"\"\"\n";
    let lines = |start: usize, first: &str, second: &str| {
        text(
            &format!("{first}\n{second}"),
            &[
                (0..1, "copied", start + 3..start + 4),
                (1..2, "copied", start + 4..start + 5),
                (2..3, "copied", start + 5..start + 6),
            ],
        )
    };
    assert_eq!(
        view(source).documents,
        [
            doc(Primary, 0..9, lines(0, "a", "A")),
            doc(Additional, 11..20, lines(11, "b", "B")),
        ]
    );
    assert_eq!(scans(source), [1, 1]);
    let source = b"x = 1; \"attribute\"\n";
    assert_eq!(
        view(source),
        View {
            assessments: vec![attribute(7..18)],
            ..View::default()
        }
    );
    assert_eq!(scans(source), [1, 1]);
}

#[cfg(feature = "python")]
#[test]
fn python_recognition_observes_cancellation_before_text() {
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let cancelled = |source: &[u8]| {
        let parser = AnyParser::parse(LANG::Python, source.to_vec(), None, None).unwrap();
        let before = observations();
        let result = cancel_in_phase(WorkPhase::Classification, || {
            parser.documentation(&context, &options).map(|_| ())
        });
        assert_eq!(result, Err(CollectionError::Cancelled));
        // expected: 0 renders, as the cancellation arrives before any text is built.
        assert_eq!(observations().renders - before.renders, 0);
        // expected: 3 visits: `module`, `expression_statement` and the first `string`,
        // whose hook charges more than one poll interval.
        assert_eq!(observations().visits - before.visits, 3);
        // The control of the zero above: the same collection, not cancelled, builds
        // text. expected: 3 text builds for each document (decoded value, tab
        // expansion, margin).
        let before = observations().renders;
        let index = parser.documentation(&context, &options).unwrap();
        assert_ne!(index.documents().len(), 0);
        assert_eq!(observations().renders - before, 3 * index.documents().len());
    };
    // The look-ahead at `"doc"` tests the next statement, a concatenation of 1,500
    // fragments: more than the 1,024 steps of one poll interval.
    let wide = format!("\"doc\"\n{}\n", vec!["\"a\""; 1_500].join(" "));
    assert_eq!(wide.len(), 6_006);
    cancelled(wide.as_bytes());
    let view = view(wide.as_bytes());
    assert_eq!(view.documents.len(), 2);
    assert_eq!(
        view.documents[0],
        doc(Primary, 0..5, text("doc", &[(0..3, "copied", 1..4)]))
    );
    assert_eq!(
        (view.documents[1].role, view.documents[1].composition),
        (Additional, Composition::LiteralConcatenation)
    );
    assert_eq!(view.documents[1].fragments.len(), 1_500);
    let Complete(joined, _) = &view.documents[1].text else {
        panic!("complete text, found {:?}", view.documents[1].text)
    };
    assert_eq!(*joined, "a".repeat(1_500));
    // The prefix test of the string reads 20,000 letters: more than the 16,384
    // bytes of one poll interval.
    let prefixed = format!("{}\"x\"\n", "r".repeat(20_000));
    assert_eq!(prefixed.len(), 20_004);
    cancelled(prefixed.as_bytes());
    assert_eq!(
        self::view(prefixed.as_bytes()).documents,
        [doc(
            Primary,
            0..20_003,
            text("x", &[(0..1, "copied", 20_001..20_002)])
        )]
    );
}

#[cfg(feature = "python")]
#[test]
fn python_excluded_gap_before_a_member_leaves_attachment_unresolved() {
    let gap = Resolution::Unresolved(UnresolvedReason::ExcludedGap);
    // The gap `4..10` lies before the second member: the first stays resolved.
    restricted(
        b"\"a\"\nx = 1\n\"b\"\ny = 2\n",
        &[0..4, 10..20],
        |index, tree, view| {
            assert_eq!(
                view,
                View {
                    documents: vec![
                        doc(Primary, 0..3, text("a", &[(0..1, "copied", 1..2)])),
                        doc(Additional, 10..13, text("b", &[(0..1, "copied", 11..12)])),
                    ],
                    links: vec![
                        link(0, Owned::File(0..20), ("module", 0..20)),
                        Link {
                            resolution: gap,
                            ..link(1, Owned::File(0..20), ("module", 0..20))
                        },
                    ],
                    assessments: vec![],
                }
            );
            assert_eq!(
                queried(index, tree.root_node()),
                (false, vec![0, 1], vec![])
            );
        },
    );
    // The file starts with excluded bytes; the function after them is the control.
    restricted(
        b"x = 1\n\"doc\"\ndef f():\n    \"inner\"\n",
        &[6..33],
        |_, tree, view| {
            let function = node(tree, "function_definition", 12..32);
            let root = site(tree.root_node());
            assert_eq!(
                view,
                View {
                    documents: vec![
                        doc(Primary, 6..11, text("doc", &[(0..3, "copied", 7..10)])),
                        doc(Primary, 25..32, text("inner", &[(0..5, "copied", 26..31)])),
                    ],
                    links: vec![
                        Link {
                            resolution: gap,
                            ..link(0, Owned::File(root.1.clone()), root)
                        },
                        link(1, Owned::Node(site(function)), ("block", 25..32)),
                    ],
                    assessments: vec![],
                }
            );
        },
    );
    // The function holds the gap and is a pending owner: the core gives the marked
    // document an association with no owner.
    restricted(
        b"def f():\n    x = 1\n    \"doc\"\n    return 1\n",
        &[0..9, 19..42],
        |index, tree, view| {
            let function = node(tree, "function_definition", 0..41);
            let pending = sorted(vec![
                (AssessmentAxis::Owner, PENDING_OWNER, None, Some(0)),
                (
                    AssessmentAxis::Owner,
                    PENDING_NODE,
                    Some(site(function)),
                    None,
                ),
            ]);
            assert_eq!(
                view,
                View {
                    documents: vec![doc(
                        Primary,
                        23..28,
                        text("doc", &[(0..3, "copied", 24..27)])
                    )],
                    links: vec![Link {
                        document: 0,
                        resolution: gap,
                        owners: vec![],
                        scope: ("block", 23..41),
                    }],
                    assessments: pending.clone(),
                }
            );
            assert_eq!(queried(index, function), (true, vec![], pending));
        },
    );
}

#[cfg(feature = "python")]
#[test]
fn python_excluded_gap_after_a_value_is_a_named_limitation() {
    // This pins today's outcome and not a design. The position check covers the
    // bytes before a member, not the bytes after its value. With those excluded, the
    // statement may be longer in the full source than the parse shows: here it is
    // `"doc" .upper()`, which is no docstring, and the document is still resolved.
    restricted(
        b"\"doc\" .upper()\nx = 1\n",
        &[0..5, 14..21],
        |_, _, view| {
            assert_eq!(
                view,
                View {
                    documents: vec![doc(Primary, 0..5, text("doc", &[(0..3, "copied", 1..4)]))],
                    links: vec![link(0, Owned::File(0..21), ("module", 0..21))],
                    assessments: vec![],
                }
            );
        },
    );
    // The definition ends inside the excluded gap, so the covered definition
    // remains a resolved owner even though the full statement calls `.upper()`.
    restricted(
        b"def f():\n    \"doc\" .upper()\n",
        &[0..18, 27..28],
        |_, _, view| {
            assert_eq!(
                view,
                View {
                    documents: vec![doc(
                        Primary,
                        13..18,
                        text("doc", &[(0..3, "copied", 14..17)])
                    )],
                    links: vec![link(
                        0,
                        Owned::Node(("function_definition", 0..18)),
                        ("block", 13..18)
                    )],
                    assessments: vec![],
                }
            );
        },
    );
    // The gap `3..6` hides `.x` after the first member. The first member stays
    // resolved; the second lies after the gap and is unresolved by the position
    // check.
    restricted(b"\"a\" .x\n\"b\"\ny = 1\n", &[0..3, 6..17], |_, _, view| {
        assert_eq!(
            view,
            View {
                documents: vec![
                    doc(Primary, 0..3, text("a", &[(0..1, "copied", 1..2)])),
                    doc(Additional, 7..10, text("b", &[(0..1, "copied", 8..9)])),
                ],
                links: vec![
                    link(0, Owned::File(0..17), ("module", 0..17)),
                    Link {
                        resolution: Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                        ..link(1, Owned::File(0..17), ("module", 0..17))
                    },
                ],
                assessments: vec![],
            }
        );
    });
}

#[cfg(feature = "python")]
#[test]
fn python_excluded_gap_between_fragments_makes_text_unavailable() {
    // In the full source the statement is `"a" + "b"`, which is no docstring. The
    // text `ab` must not be produced across the excluded `+`.
    restricted(
        b"\"a\" + \"b\"\nx = 1\n",
        &[0..4, 5..16],
        |index, _, view| {
            assert_eq!(
                view,
                View {
                    documents: vec![concatenation(
                        Primary,
                        &[0..3, 6..9],
                        Unavailable(UnavailableReason::ExcludedGap)
                    )],
                    links: vec![link(0, Owned::File(0..16), ("module", 0..16))],
                    // Both nodes hold the gap; the core marks each as a pending owner.
                    assessments: sorted(vec![
                        (
                            AssessmentAxis::Owner,
                            PENDING_NODE,
                            Some(("expression_statement", 0..9)),
                            None
                        ),
                        (
                            AssessmentAxis::Owner,
                            PENDING_NODE,
                            Some(("concatenated_string", 0..9)),
                            None
                        ),
                    ]),
                }
            );
            let document = index.documents().next().unwrap();
            assert!(index.complete_text(document).unwrap().is_none());
            for (at, bytes) in [b"\"a\"", b"\"b\""].into_iter().enumerate() {
                let fragment = index.facts().document_fragment(document, at).unwrap();
                assert_eq!(fragment.get().source().pieces()[0].bytes(), bytes);
            }
        },
    );
}

#[cfg(feature = "python")]
#[test]
fn python_concatenation_is_cleaned_as_one_document() {
    // Cleaned alone, the first fragment would keep its four spaces after the line
    // feed. The margin is that of the joined value, so they are removed.
    let source = b"def f():\n    (\"First.\\n    \" \"Second.\")\n";
    collected(source, |_, tree, view| {
        assert_eq!(
            view,
            View {
                documents: vec![concatenation(
                    Primary,
                    &[14..28, 29..38],
                    text(
                        "First.\nSecond.",
                        &[
                            (0..6, "copied", 15..21),
                            (6..7, "decoded", 21..23),
                            (7..14, "copied", 30..37),
                        ]
                    )
                )],
                links: vec![link(
                    0,
                    Owned::Node(site(node(tree, "function_definition", 0..39))),
                    ("block", 13..39)
                )],
                assessments: vec![],
            }
        );
    });
}

#[cfg(feature = "python")]
#[test]
fn python_string_past_a_recovered_error_is_not_a_run_member() {
    // The parser leaves `"b"` inside the parentheses after an `ERROR`. It is not
    // the value of its statement and gives nothing.
    let source = b"\"\"\"a\"\"\"\n(1 . \"b\")\ny = 2\n";
    collected(source, |index, tree, view| {
        assert_eq!(count(tree, "string"), 2);
        assert_eq!(
            view,
            View {
                documents: vec![doc(Primary, 0..7, text("a", &[(0..1, "copied", 3..4)]))],
                links: vec![link(0, Owned::File(0..24), ("module", 0..24))],
                assessments: vec![],
            }
        );
        assert_eq!(fragment_kinds(index), ["string"]);
    });
    // expected: 2 scans: one at `"""a"""`, and one at `"b"`, whose statement the
    // reader finds upward. The second yields nothing.
    assert_eq!(scans(source), [2, 2]);
    let source = b"x = 1\n(1 . \"b\")\n";
    collected(source, |_, tree, view| {
        assert_eq!(count(tree, "string"), 1);
        assert_eq!(view, View::default());
    });
    // expected: 1 scan, at `"b"`: it ends at the assignment and yields no member.
    assert_eq!(scans(source), [1, 1]);
}

#[cfg(feature = "python")]
#[test]
fn python_value_with_a_syntax_error_is_one_fragment_without_text() {
    // An `ERROR` between the two literals: the value is one fragment, whole.
    let source = b"(\"a\" 1 \"b\")\nx = 1\n";
    collected(source, |index, _, view| {
        assert_eq!(
            view,
            View {
                documents: vec![doc(
                    Primary,
                    1..10,
                    Unavailable(UnavailableReason::GrammarBoundary)
                )],
                links: vec![link(0, Owned::File(0..18), ("module", 0..18))],
                assessments: vec![],
            }
        );
        assert_eq!(fragment_kinds(index), ["concatenated_string"]);
        let document = index.documents().next().unwrap();
        assert!(index.complete_text(document).unwrap().is_none());
        let fragment = index.facts().document_fragment(document, 0).unwrap();
        assert_eq!(fragment.get().source().pieces()[0].bytes(), &source[1..10]);
    });
    // The control: an `ERROR` beside the value, inside the same parentheses.
    collected(b"(\"a\" \"b\" 1)\nx = 1\n", |index, _, view| {
        assert_eq!(
            view,
            View {
                documents: vec![concatenation(
                    Primary,
                    &[1..4, 5..8],
                    text("ab", &[(0..1, "copied", 2..3), (1..2, "copied", 6..7)])
                )],
                links: vec![link(0, Owned::File(0..18), ("module", 0..18))],
                assessments: vec![],
            }
        );
        assert_eq!(fragment_kinds(index), ["string", "string"]);
    });
}
