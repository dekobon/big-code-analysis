//! Static Ruff! documentation of Tcl procedures in the source view: which comment rows of
//! a body form its document, the text of the document and where each part of that text
//! comes from, and whether the procedure owns it. A source is collected through both entry
//! points, except in the tests of restricted parses and of cost. The last test collects
//! nothing and pins the parse shapes the reader relies on.

#![allow(clippy::single_range_in_vec_init)]

#[cfg(feature = "tcl")]
use super::tcl::{CommandScan, Standing, body_rows, read_procedure};
#[cfg(feature = "tcl")]
use crate::documentation::budget::{WorkBudget, cancel_after};
#[cfg(feature = "tcl")]
use crate::documentation::*;
#[cfg(feature = "tcl")]
use crate::lang_helpers::tcl::ruff_row;
#[cfg(feature = "tcl")]
use crate::lang_helpers::tcl_family::{
    RowMark, braced_word_end, comment_spelling_end, line_continues, unescaped_line_ends,
};
#[cfg(feature = "tcl")]
use crate::lang_helpers::tcl_scan::{Command, Listed};
#[cfg(feature = "tcl")]
use crate::{AnyParser, LANG};
#[cfg(feature = "tcl")]
use std::cell::{Cell, RefCell};
#[cfg(feature = "tcl")]
use std::fmt::Write as _;
#[cfg(feature = "tcl")]
use std::ops::Range;

#[cfg(feature = "tcl")]
use Run::{Copied, Synthetic};

#[cfg(feature = "tcl")]
const PENDING_DOCUMENT: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "tcl")]
const PENDING_OWNER: &str = "Recognized declaration requires a semantic projection";
#[cfg(feature = "tcl")]
const OBSCURED: &str = "Recovered Tcl body rows may hold Ruff documentation";
#[cfg(feature = "tcl")]
const EXCLUDED: &str = "Excluded bytes prevent complete native-carrier assessment";

/// The origin of one run of a document's text, with the source byte range it reads.
#[cfg(feature = "tcl")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

/// The text state of one document: the text with each run as an output range and its
/// origin, or the reason there is none.
#[cfg(feature = "tcl")]
#[derive(Debug, PartialEq, Eq)]
enum Text {
    Complete(String, Vec<(Range<usize>, Run)>),
    Unavailable(UnavailableReason),
}

/// What carries the rows of a document: each row its own `comment` node, or the body
/// word all of them. The body carries them where Tcl ends a braced word of the procedure
/// elsewhere than the grammar does: there a comment node is no evidence of a comment.
#[cfg(feature = "tcl")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carrier {
    Rows,
    Body,
}

/// One document: its composition, its rows as source byte ranges, what carries them, and
/// its text.
#[cfg(feature = "tcl")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    composition: Composition,
    fragments: Vec<Range<usize>>,
    carrier: Carrier,
    text: Text,
}

/// One association: the position of its document, its resolution, and the byte range of
/// each procedure it lists.
#[cfg(feature = "tcl")]
#[derive(Debug, PartialEq, Eq)]
struct Edge {
    document: usize,
    resolution: Resolution,
    owners: Vec<Range<usize>>,
}

/// One pending assessment: its axis and reason, its node as kind and byte range, and the
/// position of the document it is linked to.
#[cfg(feature = "tcl")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// What one collection published: the procedures that are owners, the documents, the
/// associations, each retained candidate as disposition and byte range, and the pending
/// assessments in a fixed order, because the contract gives them none. The default value
/// is an index that holds none of them. What is the same for every source here is
/// asserted by `published` and is in no expected value. Node queries are not part of it.
#[cfg(feature = "tcl")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    procedures: Vec<Range<usize>>,
    documents: Vec<Doc>,
    associations: Vec<Edge>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

/// The expected document of these rows. One row is a single fragment, several are a run.
#[cfg(feature = "tcl")]
fn rows<const F: usize, const N: usize>(
    fragments: [Range<usize>; F],
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        composition: if F == 1 {
            Composition::Single
        } else {
            Composition::CommentRun
        },
        fragments: fragments.into(),
        carrier: Carrier::Rows,
        text: Text::Complete(text.to_owned(), map.into()),
    }
}

/// The expected document of rows of which one continues on the next row.
#[cfg(feature = "tcl")]
fn unavailable<const F: usize>(fragments: [Range<usize>; F]) -> Doc {
    Doc {
        text: Text::Unavailable(UnavailableReason::GrammarBoundary),
        ..rows(fragments, "", [])
    }
}

/// The expected document of rows in a body that Tcl ends elsewhere than the grammar does:
/// the body word carries the rows, and they have no text.
#[cfg(feature = "tcl")]
fn parted<const F: usize>(fragments: [Range<usize>; F]) -> Doc {
    Doc {
        carrier: Carrier::Body,
        ..unavailable(fragments)
    }
}

/// The expected view of a procedure with such a body: its document has an association
/// that lists no owner.
#[cfg(feature = "tcl")]
fn withheld<const F: usize>(fragments: [Range<usize>; F], procedure: Range<usize>) -> View {
    unowned(
        parted(fragments),
        procedure,
        Some(UnresolvedReason::GrammarBoundary),
    )
}

#[cfg(feature = "tcl")]
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

/// One association: its document, its resolution, and the procedures it lists.
#[cfg(feature = "tcl")]
fn edge(
    document: usize,
    resolution: Resolution,
    owners: impl IntoIterator<Item = Range<usize>>,
) -> Edge {
    Edge {
        document,
        resolution,
        owners: owners.into_iter().collect(),
    }
}

/// The expected view of a procedure that is an owner and has no document.
#[cfg(feature = "tcl")]
fn undocumented(procedure: Range<usize>) -> View {
    View {
        procedures: vec![procedure],
        ..View::default()
    }
}

/// The expected view of one document that its procedure owns.
#[cfg(feature = "tcl")]
fn owned(document: Doc, procedure: Range<usize>) -> View {
    View {
        documents: vec![document],
        associations: vec![Edge {
            document: 0,
            resolution: Resolution::Resolved,
            owners: vec![procedure.clone()],
        }],
        ..undocumented(procedure)
    }
}

/// The expected view of one document whose procedure is pending. With a reason the
/// document has an association that lists no owner.
#[cfg(feature = "tcl")]
fn unowned(document: Doc, procedure: Range<usize>, reason: Option<UnresolvedReason>) -> View {
    View {
        documents: vec![document],
        associations: reason
            .map(|reason| Edge {
                document: 0,
                resolution: Resolution::Unresolved(reason),
                owners: Vec::new(),
            })
            .into_iter()
            .collect(),
        assessments: vec![
            pending(
                AssessmentAxis::Owner,
                PENDING_OWNER,
                Some(("procedure", procedure)),
                None,
            ),
            pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
        ],
        ..View::default()
    }
}

/// The expected view of a pending node whose body is not read.
#[cfg(feature = "tcl")]
fn unread(kind: &'static str, node: Range<usize>) -> View {
    View {
        assessments: vec![pending(
            AssessmentAxis::Owner,
            PENDING_OWNER,
            Some((kind, node)),
            None,
        )],
        ..View::default()
    }
}

/// The expected view of an owner whose body rows a recovery node may hold.
#[cfg(feature = "tcl")]
fn obscured(procedure: Range<usize>, kind: &'static str, node: Range<usize>) -> View {
    View {
        assessments: vec![pending(
            AssessmentAxis::Selection,
            OBSCURED,
            Some((kind, node)),
            None,
        )],
        ..undocumented(procedure)
    }
}

#[cfg(feature = "tcl")]
fn in_order(mut assessments: Vec<Pending>) -> Vec<Pending> {
    assessments.sort_by_key(|pending| {
        (
            format!("{:?}", pending.axis),
            pending.reason,
            pending
                .node
                .as_ref()
                .map(|(kind, range)| (*kind, range.start, range.end)),
            pending.document,
        )
    });
    assessments
}

/// The views of several procedures of one source, in source order, as one view.
#[cfg(feature = "tcl")]
fn joined<const N: usize>(views: [View; N]) -> View {
    let mut whole = View::default();
    for view in views {
        let shift = whole.documents.len();
        whole.procedures.extend(view.procedures);
        whole.documents.extend(view.documents);
        whole
            .associations
            .extend(view.associations.into_iter().map(|edge| Edge {
                document: edge.document + shift,
                ..edge
            }));
        whole.evidence.extend(view.evidence);
        whole
            .assessments
            .extend(view.assessments.into_iter().map(|pending| Pending {
                document: pending.document.map(|document| document + shift),
                ..pending
            }));
    }
    whole.assessments = in_order(whole.assessments);
    whole
}

/// The byte range of a spelling whose included pieces touch, as every spelling here does.
#[cfg(feature = "tcl")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    assert!(!pieces.has_excluded_gap());
    let (Some(first), Some(last)) = (pieces.pieces().first(), pieces.pieces().last()) else {
        panic!("a source piece")
    };
    first.range().start..last.range().end
}

#[cfg(feature = "tcl")]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

/// The byte range of a fragment, and what carries it. The spelling of every fragment
/// here is a comment. Its node is the `comment` node that starts with it, which may end
/// one byte later, where it holds the CR of a CRLF; or it is the `braced_word` between
/// whose braces the row stands.
#[cfg(feature = "tcl")]
fn carried(fragment: &Fragment<'_>) -> (Range<usize>, Carrier) {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    let node = fragment.node().unwrap().node();
    let carrier = match node.kind() {
        "comment" => {
            assert_eq!(node.start_byte(), range.start);
            Carrier::Rows
        }
        "braced_word" => {
            assert!(node.start_byte() < range.start && range.end < node.end_byte());
            Carrier::Body
        }
        kind => panic!("a comment or a body, found {kind}"),
    };
    (range, carrier)
}

/// The body word that holds a row: the parent of its `comment` node, or its carrier.
#[cfg(feature = "tcl")]
fn body_of<'s>(fragment: &Fragment<'s>) -> tree_sitter::Node<'s> {
    let node = fragment.node().unwrap().node();
    match carried(fragment).1 {
        Carrier::Rows => node.parent().unwrap(),
        Carrier::Body => node,
    }
}

#[cfg(feature = "tcl")]
fn spelled(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "tcl")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one flat reading of everything the index publishes, in the order of the view's fields.
    let facts = index.facts();
    let position = |document| index.documents().position(|known| known == document);
    let mut owners = facts.owners();
    // The same for every source here: the first owner is the file, and every other
    // owner is a `procedure` node.
    assert!(matches!(owners.next().unwrap().get(), Owner::File(_)));
    let procedures = owners
        .map(|owner| {
            let Owner::Node(declared) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            assert_eq!(declared.node().kind(), "procedure");
            declared.node().byte_range()
        })
        .collect();
    let documents = index
        .documents()
        .map(|document| {
            // The same for every document here: it is documentation in a body.
            assert_eq!(document.get().role(), DocumentRole::BodyComment);
            let complete = index.complete_text(document).unwrap();
            let (fragments, carriers): (Vec<_>, Vec<_>) = (0..document.get().fragment_count())
                .map(|at| carried(facts.document_fragment(document, at).unwrap().get()))
                .unzip();
            // One carrier for all rows of a document.
            assert!(carriers.iter().all(|carrier| *carrier == carriers[0]));
            Doc {
                composition: document.get().composition(),
                fragments,
                carrier: carriers[0],
                text: match (document.get().content(), complete) {
                    (Content::Complete { .. }, Some(complete)) => Text::Complete(
                        complete.text().to_owned(),
                        complete
                            .mappings()
                            .iter()
                            .map(|run| (run.output.range(), run_of(&run.origin)))
                            .collect(),
                    ),
                    (Content::Unavailable(reason), None) => Text::Unavailable(*reason),
                    (content, _) => panic!("complete or unavailable text, found {content:?}"),
                },
            }
        })
        .collect();
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            let document = facts.associated_document(association).unwrap();
            // The same for every association here, so no expected view states them. A
            // body document has no binding, and its lexical scope is the body: the
            // `braced_word` that is the parent of its first row, or that carries it.
            assert_eq!(attachment.convention.name(), "Ruff");
            assert_eq!(attachment.publication, Publication::Unknown);
            assert!(attachment.binding.is_none());
            let body = body_of(facts.document_fragment(document, 0).unwrap().get());
            assert_eq!(attachment.lexical_scope.node(), body);
            assert_eq!(body.kind(), "braced_word");
            Edge {
                document: position(document).unwrap(),
                resolution: attachment.resolution,
                owners: (0..attachment.owner_count())
                    .map(|at| {
                        let owner = facts.associated_owner(association, at).unwrap();
                        let Owner::Node(declared) = owner.get() else {
                            panic!("a node owner, found {:?}", owner.get())
                        };
                        assert_eq!(declared.node().kind(), "procedure");
                        declared.node().byte_range()
                    })
                    .collect(),
            }
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, carried(fragment.get()).0)
        })
        .collect();
    let assessments = index
        .assessments()
        .map(|assessment| Pending {
            axis: assessment.get().axis,
            reason: assessment.get().reason,
            node: assessment.get().node.map(|node| spelled(node.node())),
            document: index
                .assessed_document(assessment)
                .unwrap()
                .and_then(position),
        })
        .collect();
    // The same for every source here: the Tcl adapter is partial, and a qualification
    // record belongs to a documentation command that names its declaration, which Ruff
    // does not have.
    assert_eq!(index.profile(), Profile::Ruff);
    assert_eq!(index.capability(), Capability::Partial);
    assert_eq!(index.qualifications().len(), 0);
    View {
        procedures,
        documents,
        associations,
        evidence,
        assessments: in_order(assessments),
    }
}

/// The candidates a collection keeps beside its documents.
#[cfg(feature = "tcl")]
#[derive(Clone, Copy)]
enum Retain {
    Nothing,
    Unbound,
    Directives,
}

#[cfg(feature = "tcl")]
fn options(retain: Retain) -> DocumentationOptions {
    DocumentationOptions {
        retain_unbound: matches!(retain, Retain::Unbound),
        retain_directives: matches!(retain, Retain::Directives),
        ..DocumentationOptions::default()
    }
}

#[cfg(feature = "tcl")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Tcl, source.to_vec(), None, None).unwrap()
}

/// Both entry points over one parse: the typed parser's method, then `collect`.
#[cfg(feature = "tcl")]
fn indexes(parser: &AnyParser, retain: Retain) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    let options = options(retain);
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Tcl,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

/// The view that both entry points publish.
#[cfg(feature = "tcl")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2]) -> View {
    let [typed, raw] = indexes.each_ref().map(published);
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "tcl")]
fn retained(source: &[u8], retain: Retain) -> View {
    agreed(&indexes(&parsed(source), retain))
}

/// The view under the default options.
#[cfg(feature = "tcl")]
fn view(source: &[u8]) -> View {
    retained(source, Retain::Nothing)
}

#[cfg(feature = "tcl")]
fn assert_views<const N: usize>(cases: [(&[u8], View); N]) {
    for (source, expected) in cases {
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
    }
}

/// The one node of that kind and byte range.
#[cfg(feature = "tcl")]
fn node_at<'a>(
    tree: &'a tree_sitter::Tree,
    kind: &str,
    range: Range<usize>,
) -> tree_sitter::Node<'a> {
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == kind && node.byte_range() == range {
            found.push(node);
        }
        stack.extend(node.children(&mut node.walk()));
    }
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_primary_body_comment_belongs_to_its_procedure() {
    let primary: &[u8] = b"proc f {} {\n# audit\nreturn\n}";
    assert_eq!(
        crate::test_support::documentation::source("primary.tcl"),
        primary
    );
    let expected = || owned(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..28);
    assert_eq!(view(primary), expected());
    assert_eq!(view(b"proc f {} {\n# audit\nreturn\n}\n"), expected());
    let parser = parsed(primary);
    // Without a final line end the parser invents one, so the root holds an error. The
    // procedure spans the whole file and is still told apart from the root.
    let root = parser.ts_tree().root_node();
    assert!(root.has_error());
    assert_eq!(spelled(root), ("source_file", 0..28));
    let procedure = node_at(parser.ts_tree(), "procedure", 0..28);
    for index in indexes(&parser, Retain::Nothing) {
        let association = index.associations().next().unwrap();
        let query = index.node_associations(procedure).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().collect::<Vec<_>>(), [association]);
        assert_eq!(query.assessments().count(), 0);
        let file = index.node_associations(root).unwrap();
        assert!(!file.projection_pending());
        assert_eq!(file.associations().count(), 0);
        let document = index.documents().next().unwrap();
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let Owner::Node(declared) = owner.get() else {
            panic!("a node owner")
        };
        let fragment = index.facts().document_fragment(document, 0).unwrap();
        let fragment = span(fragment.get().source());
        crate::test_support::documentation::assert_primary_extraction(
            LANG::Tcl,
            Ok(serde_json::json!([{
                "fragments": [[fragment.start, fragment.end]],
                "owner": [declared.node().start_byte(), declared.node().end_byte()],
                "owner_kind": declared.node().kind(),
                "text": index.complete_text(document).unwrap().unwrap().text(),
            }])),
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_lead_block_runs_from_the_first_comment_row_to_a_blank_row_or_a_command() {
    assert_views([
        (
            b"proc f {} {\n\n\n# audit\n# more\nreturn\n}\n",
            owned(
                rows(
                    [14..21, 22..28],
                    "audit\nmore",
                    [
                        (0..5, Copied(16..21)),
                        (5..6, Synthetic),
                        (6..10, Copied(24..28)),
                    ],
                ),
                0..37,
            ),
        ),
        (
            b"proc f {} {\n# first\n\n# second\nreturn\n}\n",
            owned(rows([12..19], "first", [(0..5, Copied(14..19))]), 0..38),
        ),
        (
            b"proc f {} {\n# a\n   \n# b\nreturn\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..32),
        ),
        (
            b"proc f {} {\n# first\nset x 1\n# plain\n# later\n}",
            owned(rows([12..19], "first", [(0..5, Copied(14..19))]), 0..45),
        ),
        (
            b"proc f {} {\n    # a\n    # b\n    return\n}\n",
            owned(
                rows(
                    [16..19, 24..27],
                    "a\nb",
                    [
                        (0..1, Copied(18..19)),
                        (1..2, Synthetic),
                        (2..3, Copied(26..27)),
                    ],
                ),
                0..40,
            ),
        ),
        (
            b"proc f {} { # audit\nreturn\n}\n",
            owned(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..28),
        ),
        (
            b"proc f {} {# audit\n# more\nreturn\n}\n",
            owned(
                rows(
                    [11..18, 19..25],
                    "audit\nmore",
                    [
                        (0..5, Copied(13..18)),
                        (5..6, Synthetic),
                        (6..10, Copied(21..25)),
                    ],
                ),
                0..34,
            ),
        ),
        (
            b"proc f {} {\n# a\n#\n# b\nreturn\n}\n",
            owned(
                rows(
                    [12..15, 16..17, 18..21],
                    "a\n\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Synthetic),
                        (3..4, Copied(20..21)),
                    ],
                ),
                0..30,
            ),
        ),
        // A row that holds only a backslash joins the rows around it: it is no blank row.
        (
            b"proc f {} {\n# a\n\\\n# b\nreturn\n}\n",
            owned(
                rows(
                    [12..15, 18..21],
                    "a\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(20..21)),
                    ],
                ),
                0..30,
            ),
        ),
        (
            b"proc f {} {\n# a\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..17),
        ),
        // A comment after a command is no documentation.
        (b"proc f {} {\nset x 1\n# later\n}\n", undocumented(0..29)),
    ]);
    // No retention option keeps that comment: it is no candidate.
    for retain in [Retain::Unbound, Retain::Directives] {
        assert_eq!(
            retained(b"proc f {} {\nset x 1\n# later\n}\n", retain),
            undocumented(0..29)
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_ruff_marker_opens_a_block_at_its_own_row() {
    assert_views([
        (
            b"proc f {} {\n# first\nset x 1\n#ruff\n# later\n}",
            owned(
                rows(
                    [12..19, 28..33, 34..41],
                    "first\n\nlater",
                    [
                        (0..5, Copied(14..19)),
                        (5..6, Synthetic),
                        (6..7, Synthetic),
                        (7..12, Copied(36..41)),
                    ],
                ),
                0..43,
            ),
        ),
        (
            b"proc f {} {\n# first\nset x 1\n#ruff later\n}",
            owned(
                rows(
                    [12..19, 28..39],
                    "first\nlater",
                    [
                        (0..5, Copied(14..19)),
                        (5..6, Synthetic),
                        (6..11, Copied(34..39)),
                    ],
                ),
                0..41,
            ),
        ),
        // The block opens at the marker: the ordinary row above it is no documentation.
        (
            b"proc f {} {\n# first\nset x 1\n# plain\n#ruff\n# later\n}\n",
            owned(
                rows(
                    [12..19, 36..41, 42..49],
                    "first\n\nlater",
                    [
                        (0..5, Copied(14..19)),
                        (5..6, Synthetic),
                        (6..7, Synthetic),
                        (7..12, Copied(44..49)),
                    ],
                ),
                0..51,
            ),
        ),
        (
            b"proc f {} {\nset x 1\n#ruff\n# later\n}\n",
            owned(
                rows([20..25, 26..33], "later", [(0..5, Copied(28..33))]),
                0..35,
            ),
        ),
        (
            b"proc f {} {\nset x 1\n#ruff a\nset y 2\n#ruff b\n# c\n}\n",
            owned(
                rows(
                    [20..27, 36..43, 44..47],
                    "a\nb\nc",
                    [
                        (0..1, Copied(26..27)),
                        (1..2, Synthetic),
                        (2..3, Copied(42..43)),
                        (3..4, Synthetic),
                        (4..5, Copied(46..47)),
                    ],
                ),
                0..49,
            ),
        ),
        (
            b"proc f {} {\n# a\n#ruff b\n# c\nreturn\n}\n",
            owned(
                rows(
                    [12..15, 16..23, 24..27],
                    "a\nb\nc",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(22..23)),
                        (3..4, Synthetic),
                        (4..5, Copied(26..27)),
                    ],
                ),
                0..36,
            ),
        ),
        (
            b"proc f {} {\n#ruff\n# a\n\n# b\n}\n",
            owned(rows([12..17, 18..21], "a", [(0..1, Copied(20..21))]), 0..28),
        ),
        (
            b"proc f {} {\n# a\n\n#ruff b\n}\n",
            owned(
                rows(
                    [12..15, 17..24],
                    "a\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(23..24)),
                    ],
                ),
                0..26,
            ),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_ruff_rows_lose_one_marker_and_one_separator() {
    assert_views([
        (
            b"proc f {} {\n## doubled\n#x\n#  two\n# trail  \n#ruff  w\n#ruff \n#end\n}\n",
            owned(
                rows(
                    [12..22, 23..25, 26..32, 33..42, 43..51, 52..58, 59..63],
                    "# doubled\nx\n two\ntrail  \n w\n\nend",
                    [
                        (0..9, Copied(13..22)),
                        (9..10, Synthetic),
                        (10..11, Copied(24..25)),
                        (11..12, Synthetic),
                        (12..16, Copied(28..32)),
                        (16..17, Synthetic),
                        (17..24, Copied(35..42)),
                        (24..25, Synthetic),
                        (25..27, Copied(49..51)),
                        (27..28, Synthetic),
                        (28..29, Synthetic),
                        (29..32, Copied(60..63)),
                    ],
                ),
                0..65,
            ),
        ),
        // A tab separates only after the marker `#ruff`, and one separator is removed.
        (
            b"proc f {} {\n#\ty\n#ruff\tz\n#ruff\t w\n}\n",
            owned(
                rows(
                    [12..15, 16..23, 24..32],
                    "\ty\nz\n w",
                    [
                        (0..2, Copied(13..15)),
                        (2..3, Synthetic),
                        (3..4, Copied(22..23)),
                        (4..5, Synthetic),
                        (5..7, Copied(30..32)),
                    ],
                ),
                0..34,
            ),
        ),
        // Empty rows at either end of the document are no text.
        (
            b"proc f {} {\n#ruff\n# a\n#ruff\n}\n",
            owned(
                rows([12..17, 18..21, 22..27], "a", [(0..1, Copied(20..21))]),
                0..29,
            ),
        ),
        (b"proc f {} {\n#\n}\n", owned(rows([12..13], "", []), 0..15)),
        // An empty row starts the lead block, so the blank row below it ends the block.
        // Ruff! reads on to `after` here: this is a known difference.
        (
            b"proc f {} {\n#\n\n# after\n}\n",
            owned(rows([12..13], "", []), 0..24),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_ruffopt_rows_are_directives_and_no_boundary() {
    let check = |source: &[u8], mut expected: View, directives: &[Range<usize>]| {
        let name = source.escape_ascii();
        assert_eq!(view(source), expected, "{name}");
        assert_eq!(retained(source, Retain::Unbound), expected, "{name}");
        expected.evidence = directives
            .iter()
            .map(|row| (CandidateDisposition::Directive, row.clone()))
            .collect();
        assert_eq!(retained(source, Retain::Directives), expected, "{name}");
    };
    check(
        b"proc f {} {\n# a\n#ruffopt includedformats html\n# b\nset x 1\n# plain\n#ruffopt excludedformats {html}\n}\n",
        owned(
            rows(
                [12..15, 46..49],
                "a\nb",
                [
                    (0..1, Copied(14..15)),
                    (1..2, Synthetic),
                    (2..3, Copied(48..49)),
                ],
            ),
            0..99,
        ),
        &[16..45, 66..97],
    );
    check(
        b"proc f {} {\n#ruffopt includedformats html\nreturn\n}\n",
        undocumented(0..50),
        &[12..41],
    );
    check(
        b"proc f {} {\n#ruffopt a b\n\n# lead\n}\n",
        owned(rows([26..32], "lead", [(0..4, Copied(28..32))]), 0..34),
        &[12..24],
    );
    check(
        b"proc f {} {\n# a\n#ruffopt x y\n\n# b\n}\n",
        owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..35),
        &[16..28],
    );
    // A blank row ends the block also where a directive row follows it.
    check(
        b"proc f {} {\n# a\n\n#ruffopt x y\n# b\n}\n",
        owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..35),
        &[17..29],
    );
    // A longer word is an ordinary row, and a comment after `;` is no row.
    check(
        b"proc f {} {\n#ruffoptions x\n}\n",
        owned(
            rows([12..26], "ruffoptions x", [(0..13, Copied(13..26))]),
            0..28,
        ),
        &[],
    );
    check(
        b"proc f {} {\nset x 1 ;#ruffopt a b\n}\n",
        undocumented(0..35),
        &[],
    );
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_other_words_that_start_with_ruff_are_ordinary_rows() {
    assert_views([
        (
            b"proc f {} {\n#rufftail x\n# ruff spaced\n#ruffian\n}\n",
            owned(
                rows(
                    [12..23, 24..37, 38..46],
                    "rufftail x\nruff spaced\nruffian",
                    [
                        (0..10, Copied(13..23)),
                        (10..11, Synthetic),
                        (11..22, Copied(26..37)),
                        (22..23, Synthetic),
                        (23..30, Copied(39..46)),
                    ],
                ),
                0..48,
            ),
        ),
        // After a command none of them opens a block. A form feed is no separator.
        (
            b"proc f {} {\n# a\nset x 1\n#rufftail\n# ruff\n#ruff\x0cff\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..51),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_comment_that_does_not_start_its_row_is_no_row() {
    assert_views([
        (
            b"proc f {} {\n# a\nset x 1 ;# after\n;# lead semi\n#ruff m\n}\n",
            owned(
                rows(
                    [12..15, 46..53],
                    "a\nm",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(52..53)),
                    ],
                ),
                0..55,
            ),
        ),
        (b"proc f {} {\nset x 1\n;#ruff x\n}\n", undocumented(0..30)),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_rows_nested_in_a_command_are_not_read() {
    // Ruff! reads rows of the body text and finds both nested markers. This unit reads the
    // direct children of the body only, which is a known difference and no final design.
    assert_views([
        (
            b"proc f {} {\n# lead\nif {$x} {\n#ruff nested\n}\nputs {\n#ruff string\n}\n}\n",
            owned(rows([12..18], "lead", [(0..4, Copied(14..18))]), 0..67),
        ),
        (
            b"proc f {} {\nif {$x} {\n#ruff nested\n}\n}\n",
            undocumented(0..38),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_row_that_continues_is_text_of_unknown_extent() {
    assert_views([
        // Tcl reads the next row as part of the comment; the grammar reads a command.
        (
            b"proc f {} {\n# audit \\\nmore text\nreturn\n}\n",
            owned(unavailable([12..21]), 0..40),
        ),
        (
            b"proc f {} {\n# a \\\n# b\nreturn\n}\n",
            owned(unavailable([12..17, 18..21]), 0..30),
        ),
        (
            b"proc f {} {\r\n# audit \\\r\nreturn\r\n}\r\n",
            owned(unavailable([13..22]), 0..33),
        ),
        // An even run is escaped backslashes and continues nothing.
        (
            b"proc f {} {\n# a \\\\\n# b\n}\n",
            owned(
                rows(
                    [12..18, 19..22],
                    "a \\\\\nb",
                    [
                        (0..4, Copied(14..18)),
                        (4..5, Synthetic),
                        (5..6, Copied(21..22)),
                    ],
                ),
                0..24,
            ),
        ),
        // A row below a continued line is the rest of that line: it opens no block.
        (
            b"proc f {} {\nset x 1\n# plain \\\n#ruff\n# later\n}\n",
            undocumented(0..45),
        ),
        (
            b"proc f {} {\nset x 1\n# plain \\\n\\\n#ruff x\n}\n",
            undocumented(0..41),
        ),
        // A blank row ends the continued line, so the marker row is a row again.
        (
            b"proc f {} {\nset x 1\n# plain \\\n\n#ruff x\n}\n",
            owned(rows([31..38], "x", [(0..1, Copied(37..38))]), 0..40),
        ),
    ]);
    // A continued `#ruffopt` row takes the marker row below it.
    let source: &[u8] = b"proc f {} {\n#ruffopt a \\\n#ruff\n# later\n}\n";
    let mut expected = owned(rows([31..38], "later", [(0..5, Copied(33..38))]), 0..40);
    assert_eq!(view(source), expected);
    expected
        .evidence
        .push((CandidateDisposition::Directive, 12..24));
    assert_eq!(retained(source, Retain::Directives), expected);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_crlf_row_ends_before_its_carriage_return() {
    assert_views([
        (
            b"proc f {} {\r\n# audit\r\n#ruff\r\n# more\r\nreturn\r\n}\r\n",
            owned(
                rows(
                    [13..20, 22..27, 29..35],
                    "audit\n\nmore",
                    [
                        (0..5, Copied(15..20)),
                        (5..6, Synthetic),
                        (6..7, Synthetic),
                        (7..11, Copied(31..35)),
                    ],
                ),
                0..46,
            ),
        ),
        // A carriage return that no line feed follows is content of the row.
        (
            b"proc f {} {\n# a\rb\nreturn\n}\n",
            owned(rows([12..17], "a\rb", [(0..3, Copied(14..17))]), 0..26),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_recovery_among_the_rows_ends_a_block_as_a_command_does() {
    let stray: &[u8] = b"proc f {} {\n# audit\nputs ]\nreturn\n}\nproc g {} {\n# doc g\n}\n";
    // `puts ]` is valid Tcl and a recovery node here. The procedure holds an error and
    // is an owner all the same.
    assert!(node_at(parsed(stray).ts_tree(), "procedure", 0..35).has_error());
    assert_views([
        (
            stray,
            joined([
                owned(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..35),
                owned(rows([48..55], "doc g", [(0..5, Copied(50..55))]), 36..57),
            ]),
        ),
        (
            b"proc f {} {\n# a\nreturn;# done\n#ruff b\n}\n",
            owned(
                rows(
                    [12..15, 30..37],
                    "a\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(36..37)),
                    ],
                ),
                0..39,
            ),
        ),
        (b"proc f {} {;# c\nreturn\n}\n", undocumented(0..24)),
        (
            b"proc f {} {\n# a\nset x 1; set y 2\n#ruff b\n}\n",
            owned(
                rows(
                    [12..15, 33..40],
                    "a\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(39..40)),
                    ],
                ),
                0..42,
            ),
        ),
        // A recovery node on a row of its own ends the block, with no command there.
        (
            b"proc f {} {\n# a\n]\n# b\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..23),
        ),
        // A missing token deep in a body command leaves both procedures owners.
        (
            b"proc f {} {\n# a\nputs [x\n}\nproc g {} {\n# b\n}\n",
            joined([
                owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..25),
                owned(rows([38..41], "b", [(0..1, Copied(40..41))]), 26..43),
            ]),
        ),
        // A recovery node on the row of a command starts no row and ends on that row:
        // it can hold no row of documentation.
        (
            b"proc f {} {\n# a\n# b\nset x 1 # text\n}\n",
            owned(
                rows(
                    [12..15, 16..19],
                    "a\nb",
                    [
                        (0..1, Copied(14..15)),
                        (1..2, Synthetic),
                        (2..3, Copied(18..19)),
                    ],
                ),
                0..36,
            ),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_recovery_that_may_hold_rows_withholds_the_document() {
    // Ruff! reads `a`, an empty line and `later` in the first source. The parser hides
    // the first row in a recovery node, so this unit publishes no part of the document
    // and says so. A recovery node may hold rows when it starts a row with `#` or
    // reaches a later row. This pins a proposed rule, not a final design.
    let check = |source: &[u8], procedure: Range<usize>, kind: &'static str, node| {
        let name = source.escape_ascii();
        let expected = obscured(procedure.clone(), kind, node);
        assert_eq!(view(source), expected, "{name}");
        // No row of the body is kept as a candidate either.
        assert_eq!(retained(source, Retain::Unbound), expected, "{name}");
        assert_eq!(retained(source, Retain::Directives), expected, "{name}");
        let parser = parsed(source);
        let declared = node_at(parser.ts_tree(), "procedure", procedure);
        for index in indexes(&parser, Retain::Nothing) {
            let query = index.node_associations(declared).unwrap();
            assert!(!query.projection_pending(), "{name}");
            assert!(
                !index.node_projection(declared).unwrap().pending(),
                "{name}"
            );
            assert_eq!(query.associations().count(), 0, "{name}");
            assert_eq!(
                query
                    .assessments()
                    .map(|assessment| assessment.get().reason)
                    .collect::<Vec<_>>(),
                [OBSCURED],
                "{name}"
            );
        }
    };
    check(
        b"proc f {} {\n# a\nset x 1 # text\n#ruff\n# later\n}\n",
        0..46,
        "ERROR",
        12..23,
    );
    check(
        b"proc f {} {\n# a\x00b\nreturn\n}\n",
        0..26,
        "ERROR",
        12..16,
    );
    // The NUL byte cuts the second comment short: the rest of its row is not read.
    check(
        b"proc f {} {\n# a\n# b\x00c\nreturn\n}\n",
        0..30,
        "comment",
        16..19,
    );
    // After a command too: the recovery node reaches the row of the second marker, and
    // a NUL byte cuts a marker row.
    check(
        b"proc f {} {\n# lead\nset x 1\n#ruff a\nset y 2 # text\n#ruff c\n}\n",
        0..59,
        "ERROR",
        43..57,
    );
    check(
        b"proc f {} {\n# lead\nset x 1\n#ruff b\x00c\nreturn\n}\n",
        0..45,
        "comment",
        27..34,
    );
    // The grammar reads the row below a continued `if` as a recovery node that starts a
    // row with `#`. Tcl reads an argument of `if` there.
    check(
        b"proc f {} {\nif {1} {} \\\n#ruff b\n}\n",
        0..33,
        "ERROR",
        24..31,
    );
    // A directive row above the node that withholds the document is withheld with it.
    check(
        b"proc f {} {\n#ruffopt a b\n# a\x00b\nreturn\n}\n",
        0..39,
        "comment",
        25..28,
    );
    // The same body without the NUL byte keeps the directive row.
    let whole: &[u8] = b"proc f {} {\n#ruffopt a b\n# ab\nreturn\n}\n";
    let document = owned(rows([25..29], "ab", [(0..2, Copied(27..29))]), 0..38);
    assert_eq!(retained(whole, Retain::Unbound), document);
    assert_eq!(
        retained(whole, Retain::Directives),
        View {
            evidence: vec![(CandidateDisposition::Directive, 12..24)],
            ..document
        }
    );
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_procedure_name_decides_whether_the_procedure_is_an_owner() {
    assert_views([
        (
            b"proc ::ns::f {} {\n# n\n}\n",
            owned(rows([18..21], "n", [(0..1, Copied(20..21))]), 0..23),
        ),
        (
            b"proc f args {\n# a\n}\n",
            owned(rows([14..17], "a", [(0..1, Copied(16..17))]), 0..19),
        ),
        (
            b"proc $name {} {\n# dyn\n}\n",
            unowned(
                rows([16..21], "dyn", [(0..3, Copied(18..21))]),
                0..23,
                Some(UnresolvedReason::Dynamic),
            ),
        ),
        (
            b"proc [mk] {} {\n# dyn\n}\n",
            unowned(
                rows([15..20], "dyn", [(0..3, Copied(17..20))]),
                0..22,
                Some(UnresolvedReason::Dynamic),
            ),
        ),
        // A quoted or braced name is a static word that this unit does not read yet. The
        // procedure is pending, which is not a final design.
        (
            b"proc \"g\" {} {\n# q\n}\n",
            unowned(rows([14..17], "q", [(0..1, Copied(16..17))]), 0..19, None),
        ),
        (
            b"proc {h} {} {\n# b\n}\n",
            unowned(rows([14..17], "b", [(0..1, Copied(16..17))]), 0..19, None),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_procedure_without_four_readable_words_is_pending_and_unread() {
    assert_views([
        (b"proc a$b {} {\n# c\n}\n", unread("procedure", 0..19)),
        (
            b"proc {*}$spec\nproc g {} {\n# doc g\n}\n",
            unread("procedure", 0..20),
        ),
        (b"proc f {} \"\n# q\n\"\n", unread("procedure", 0..17)),
        (b"proc f {} return\n", unread("procedure", 0..16)),
        (
            b"proc f {}\nproc g {} {\n# doc g\nreturn\n}\n",
            unread("procedure", 0..14),
        ),
        // A recovery node in the name or in the parameter list: the words are not read.
        (b"proc [a;] {} {\n# c\n}\n", unread("procedure", 0..20)),
        (b"proc f {a;} {\n# c\n}\n", unread("procedure", 0..19)),
        // Tcl defines the procedure in these two: `f{` is a name, and a quoted word in a
        // parameter list is text. The grammar gives a recovery node in the name, and a
        // procedure that ends inside its parameter list. This pins today's outcome.
        (b"proc f{ {} {\n# doc\n}\n", unread("procedure", 0..20)),
        (
            b"proc f {a \"b\"} {\n# doc\n}\n",
            unread("procedure", 0..13),
        ),
        // The broken parameter list of `bad` runs into the next procedure, whose body
        // becomes the body of `bad`: `# doc c` is in no document.
        (
            b"proc a {} {\n# doc a\nreturn\n}\nproc bad {x {\n# doc bad\n}\nproc c {} {\n# doc c\nreturn\n}\n",
            joined([
                owned(rows([12..19], "doc a", [(0..5, Copied(14..19))]), 0..28),
                unread("procedure", 29..83),
            ]),
        ),
    ]);
    // No retention option changes the view of an unread body: it gives no candidate.
    for retain in [Retain::Unbound, Retain::Directives] {
        assert_eq!(
            retained(b"proc f {} \"\n# q\n\"\n", retain),
            unread("procedure", 0..17)
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_row_below_a_continued_line_is_no_procedure() {
    let phantom = |document: Doc, procedure: Range<usize>| {
        unowned(document, procedure, Some(UnresolvedReason::GrammarBoundary))
    };
    assert_views([
        (
            b"# c \\\nproc g {} {\n# inner\n}\nproc h {} {\n# doc h\n}\n",
            joined([
                phantom(rows([18..25], "inner", [(0..5, Copied(20..25))]), 6..27),
                owned(rows([40..47], "doc h", [(0..5, Copied(42..47))]), 28..49),
            ]),
        ),
        (
            b"# c \\\n\\\nproc g {} {\n# inner\n}\n",
            phantom(rows([20..27], "inner", [(0..5, Copied(22..27))]), 8..29),
        ),
        (
            b"# c \\\n\nproc g {} {\n# inner\n}\n",
            owned(rows([19..26], "inner", [(0..5, Copied(21..26))]), 7..28),
        ),
        (
            b"# c \\\\\nproc g {} {\n# inner\n}\n",
            owned(rows([19..26], "inner", [(0..5, Copied(21..26))]), 7..28),
        ),
        (
            b"# c \\\nproc g {} {\n# inner\n}\n",
            phantom(rows([18..25], "inner", [(0..5, Copied(20..25))]), 6..27),
        ),
        // A row that Tcl does not run as a command is uncertain for that reason, whatever
        // its name is.
        (
            b"# c \\\nproc $name {} {\n# x\n}\n",
            phantom(rows([22..25], "x", [(0..1, Copied(24..25))]), 6..27),
        ),
        // The continued line takes the row `JUNK`, which the grammar reads as a command.
        // The procedure below it is a row of its own.
        (
            b"# c \\\nJUNK\nproc g {} {\n# inner\n}\n",
            owned(rows([23..30], "inner", [(0..5, Copied(25..30))]), 11..32),
        ),
        (
            b"# c\nJUNK\nproc g {} {\n# inner\n}\n",
            owned(rows([21..28], "inner", [(0..5, Copied(23..28))]), 9..30),
        ),
        // With CRLF line ends the grammar reads the bare row and the continued command
        // as commands that end in a backslash.
        (
            b"# c \\\r\n\\\r\nproc g {} {\r\n# inner\r\n}\r\n",
            phantom(rows([23..30], "inner", [(0..5, Copied(25..30))]), 10..33),
        ),
        (
            b"foo a \\\r\nproc g {} {\r\n# inner\r\n}\r\n",
            phantom(rows([22..29], "inner", [(0..5, Copied(24..29))]), 9..32),
        ),
        // A `;` or a command before the procedure, on a row that Tcl reads as the rest
        // of a comment, starts nothing: Tcl defines no procedure in these five sources.
        (
            b"# c \\\n;proc f {} {\n# doc\n}\n",
            phantom(rows([19..24], "doc", [(0..3, Copied(21..24))]), 7..26),
        ),
        (
            b"# c \\\nputs a ; proc g {} {\n# doc\n}\n",
            phantom(rows([27..32], "doc", [(0..3, Copied(29..32))]), 15..34),
        ),
        (
            b"# c \\\nset x 1 \\\n; proc g {} {\n# b\n}\n",
            phantom(rows([30..33], "b", [(0..1, Copied(32..33))]), 18..35),
        ),
        (
            b"# c \\\nJUNK \\\nbar ; proc g {} {\n# b\n}\n",
            phantom(rows([31..34], "b", [(0..1, Copied(33..34))]), 19..36),
        ),
        (
            b"# c \\\r\n;proc g {} {\r\n# b\r\n}\r\n",
            phantom(rows([21..24], "b", [(0..1, Copied(23..24))]), 8..27),
        ),
        // The same rows without the comment: the `;` ends `set`, and Tcl defines `g`.
        (
            b"set x 1 \\\n; proc g {} {\n# b\n}\n",
            owned(rows([24..27], "b", [(0..1, Copied(26..27))]), 12..29),
        ),
        // The first procedure of the continued row is no command for Tcl. The comment
        // ends at the line end inside its body, and Tcl defines `b` after the `;`. On a
        // plain row both are owners.
        (
            b"# c \\\nproc a {} {\n# a\n};proc b {} {\n# b\n}\n",
            joined([
                unowned(
                    rows([18..21], "a", [(0..1, Copied(20..21))]),
                    6..23,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                owned(rows([36..39], "b", [(0..1, Copied(38..39))]), 24..41),
            ]),
        ),
        (
            b"# c\nproc a {} {\n# a\n};proc b {} {\n# b\n}\n",
            joined([
                owned(rows([16..19], "a", [(0..1, Copied(18..19))]), 4..21),
                owned(rows([34..37], "b", [(0..1, Copied(36..37))]), 22..39),
            ]),
        ),
        // Tcl reads the whole continued row as comment text here and defines nothing.
        // Above, where the body of `a` spans rows, it defines `b`; the unit does not
        // tell the two apart.
        (
            b"# c \\\nproc a {} {};proc b {} {\n# b\n}\n",
            joined([
                unread("procedure", 6..18),
                phantom(rows([31..34], "b", [(0..1, Copied(33..34))]), 19..36),
            ]),
        ),
        // A comment after `;` continues as one at the start of a row does.
        (
            b";# c \\\nproc g {} {\n# b\n}\n",
            phantom(rows([19..22], "b", [(0..1, Copied(21..22))]), 7..24),
        ),
        (
            b"# c\n;proc g {} {\n# b\n}\n",
            owned(rows([17..20], "b", [(0..1, Copied(19..20))]), 5..22),
        ),
        // An escaped line end is white space: after `;`, and at the start of the source,
        // the procedure is the first word of a command.
        (
            b"set x 1 ;\\\nproc g {} {\n# doc\n}\n",
            owned(rows([23..28], "doc", [(0..3, Copied(25..28))]), 11..30),
        ),
        (
            b"\\\nproc f {} {\n# a\n}\n",
            owned(rows([14..17], "a", [(0..1, Copied(16..17))]), 2..19),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_procedure_that_is_not_one_command_is_no_owner() {
    let phantom = |document: Doc, procedure: Range<usize>| {
        unowned(document, procedure, Some(UnresolvedReason::GrammarBoundary))
    };
    assert_views([
        (
            b"proc f\n{} {\n# audit\n}\nproc g {} {\n# doc g\n}\n",
            joined([
                phantom(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..21),
                owned(rows([34..41], "doc g", [(0..5, Copied(36..41))]), 22..43),
            ]),
        ),
        (
            b"proc f {} \\\n{\n# audit\n}\n",
            owned(rows([14..21], "audit", [(0..5, Copied(16..21))]), 0..23),
        ),
        (
            b"proc f {} {\n# audit\n} extra\nproc g {} {\n# doc g\n}\n",
            joined([
                phantom(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..21),
                owned(rows([40..47], "doc g", [(0..5, Copied(42..47))]), 28..49),
            ]),
        ),
        (
            b"proc f {} {\n# audit\n} # trailing\n",
            phantom(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..21),
        ),
        (
            b"proc f {} {\n# audit\n};proc g {} {\n# doc g\n}\n",
            joined([
                owned(rows([12..19], "audit", [(0..5, Copied(14..19))]), 0..21),
                owned(rows([34..41], "doc g", [(0..5, Copied(36..41))]), 22..43),
            ]),
        ),
        (
            b"proc f {} {\n# a\n} proc g {} {\n# b\n}\n",
            joined([
                phantom(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..17),
                phantom(rows([30..33], "b", [(0..1, Copied(32..33))]), 18..35),
            ]),
        ),
        (
            b"set x 1 proc g {} {\n# doc\n}\n",
            phantom(rows([20..25], "doc", [(0..3, Copied(22..25))]), 8..27),
        ),
        (
            b"set x 1 ; proc g {} {\n# doc\n}\n",
            owned(rows([22..27], "doc", [(0..3, Copied(24..27))]), 10..29),
        ),
        (
            b"proc f {} {\n# a\n}   ",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..17),
        ),
        (
            b"proc f {} {\n# doc\n} extra\n",
            phantom(rows([12..17], "doc", [(0..3, Copied(14..17))]), 0..19),
        ),
        (
            b"proc g {} {\n# inner\n}\nJUNK\n",
            owned(rows([12..19], "inner", [(0..5, Copied(14..19))]), 0..21),
        ),
        // A vertical tab separates words, and so does an escaped line end.
        (
            b"proc f {} {\n# doc\n}\x0bextra\n",
            phantom(rows([12..17], "doc", [(0..3, Copied(14..17))]), 0..19),
        ),
        (
            b"proc f {} {\n# a\n} \\\r\nextra\r\n",
            phantom(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..17),
        ),
        // The line end after an escaped one ends the command, and so does a `;`,
        // whatever follows it.
        (
            b"proc f {} {\n# doc\n} \\\n\n",
            owned(rows([12..17], "doc", [(0..3, Copied(14..17))]), 0..19),
        ),
        (
            b"proc f {} {\n# doc\n};# c \\\nextra\n",
            owned(rows([12..17], "doc", [(0..3, Copied(14..17))]), 0..19),
        ),
        // The `;` starts the command `set y 2`, not the procedure.
        (
            b"set x 1 ; set y 2 proc g {} {\n# doc\n}\n",
            phantom(rows([30..35], "doc", [(0..3, Copied(32..35))]), 18..37),
        ),
        // Nothing separates the parameter list and the body: Tcl reports extra
        // characters after close-brace. The grammar reports no error.
        (
            b"proc f {}{\n# doc\n}\n",
            phantom(rows([11..16], "doc", [(0..3, Copied(13..16))]), 0..18),
        ),
        (
            b"proc f {a b}{\n# doc\n}\n",
            phantom(rows([14..19], "doc", [(0..3, Copied(16..19))]), 0..21),
        ),
        // Nothing separates `proc` and a braced name: Tcl reads the word `proc{f}`.
        (
            b"proc{f} {} {\n# doc\n}\n",
            phantom(rows([13..18], "doc", [(0..3, Copied(15..18))]), 0..20),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_nested_procedure_keeps_its_document_and_stays_pending() {
    // A procedure in another scope than the file is pending until a later unit decides
    // its owner. This pins a pending outcome, not a final design.
    let namespace: &[u8] = b"namespace eval n {\nproc f {} {\n# audit\nreturn\n}\n}\n";
    assert_views([
        (
            namespace,
            unowned(
                rows([31..38], "audit", [(0..5, Copied(33..38))]),
                19..47,
                None,
            ),
        ),
        (
            b"proc outer {} {\n# outer doc\nproc inner {} {\n# inner doc\n}\n}\n",
            joined([
                owned(rows([16..27], "outer doc", [(0..9, Copied(18..27))]), 0..59),
                unowned(
                    rows([44..55], "inner doc", [(0..9, Copied(46..55))]),
                    28..57,
                    None,
                ),
            ]),
        ),
        // A nested procedure is pending whatever its name is.
        (
            b"namespace eval n {\nproc $name {} {\n# x\n}\n}\n",
            unowned(rows([35..38], "x", [(0..1, Copied(37..38))]), 19..40, None),
        ),
        // The first body is never closed. The second procedure survives in a recovery
        // node and is nested in it.
        (
            b"proc f {} {\n# doc f\nset x 1\n\nproc g {} {\n# doc g\nreturn 2\n}\n",
            unowned(
                rows([41..48], "doc g", [(0..5, Copied(43..48))]),
                29..59,
                None,
            ),
        ),
        // Tcl defines `f` in these two. The procedure is nested in a command substitution
        // and in the braced script of `eval`.
        (
            b"[proc f {} {\n# doc\n}]\n",
            unowned(rows([13..18], "doc", [(0..3, Copied(15..18))]), 1..20, None),
        ),
        (
            b"eval {proc f {} {\n# doc\n}}\n",
            unowned(rows([18..23], "doc", [(0..3, Copied(20..23))]), 6..25, None),
        ),
    ]);
    let parser = parsed(namespace);
    let nested = node_at(parser.ts_tree(), "procedure", 19..47);
    for index in indexes(&parser, Retain::Nothing) {
        assert!(index.node_projection(nested).unwrap().pending());
        assert!(matches!(
            index.node_owners(nested),
            Err(OwnerError::PendingOwner)
        ));
        let query = index.node_associations(nested).unwrap();
        assert!(query.projection_pending());
        assert_eq!(
            in_order(
                query
                    .assessments()
                    .map(|assessment| pending(
                        assessment.get().axis,
                        assessment.get().reason,
                        None,
                        None
                    ))
                    .collect()
            ),
            [
                pending(AssessmentAxis::Owner, PENDING_OWNER, None, None),
                pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, None)
            ]
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_procedure_under_a_root_error_is_an_unresolved_candidate() {
    // The procedure stands before every brace that the grammar leaves open, so Tcl's
    // brace count has nothing against it: Tcl defines `g` and then reports a missing
    // close-brace. The core lists it as the candidate.
    let source: &[u8] =
        b"proc g {} {\n# doc g\n}\nproc h {a} {set s  \"\"\"{\"{$a(proc k {} {\n\n\"\"{\"";
    let parser = parsed(source);
    assert!(parser.ts_tree().root_node().is_error());
    assert_eq!(
        agreed(&indexes(&parser, Retain::Nothing)),
        View {
            procedures: vec![0..21],
            documents: vec![rows([12..19], "doc g", [(0..5, Copied(14..19))])],
            associations: vec![Edge {
                document: 0,
                resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                owners: vec![0..21],
            }],
            ..View::default()
        }
    );
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_documents_keep_source_order_and_each_procedure_answers_for_itself() {
    let source: &[u8] =
        b"proc f {} {\n# doc f\nreturn 1\n}\nproc g {a b} {\n# doc g\nreturn 2\n}\n";
    let parser = parsed(source);
    let both = indexes(&parser, Retain::Nothing);
    assert_eq!(
        agreed(&both),
        joined([
            owned(rows([12..19], "doc f", [(0..5, Copied(14..19))]), 0..30),
            owned(rows([46..53], "doc g", [(0..5, Copied(48..53))]), 31..64),
        ])
    );
    for index in &both {
        let associations: Vec<_> = index.associations().collect();
        for (at, procedure) in [(0, 0..30), (1, 31..64)] {
            let declared = node_at(parser.ts_tree(), "procedure", procedure);
            let query = index.node_associations(declared).unwrap();
            assert!(!query.projection_pending());
            assert_eq!(query.associations().collect::<Vec<_>>(), [associations[at]]);
        }
    }
    let source: &[u8] = b"proc f {} {\nreturn 1\n}\nproc g {} {\n# doc g\nreturn 2\n}\n";
    let parser = parsed(source);
    let both = indexes(&parser, Retain::Nothing);
    assert_eq!(
        agreed(&both),
        joined([
            undocumented(0..22),
            owned(rows([35..42], "doc g", [(0..5, Copied(37..42))]), 23..53),
        ])
    );
    let silent = node_at(parser.ts_tree(), "procedure", 0..22);
    for index in &both {
        let query = index.node_associations(silent).unwrap();
        assert!(!query.projection_pending());
        assert!(!index.node_projection(silent).unwrap().pending());
        assert_eq!(query.associations().count(), 0);
        assert_eq!(query.assessments().count(), 0);
    }
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_comment_on_the_continued_line_of_a_command_is_no_row() {
    // Tcl reads each `#` here as a word of the command above it: `info body` shows one
    // line. The grammar ends the command before the backslash and reads a comment, so
    // where the comment stands is read from the bytes between the two.
    for (source, procedure) in [
        (&b"proc f {} {\nset x 1 \\\n#ruff notdoc\n}\n"[..], 0..36),
        (b"proc f {} {\nset x 1 \\\n#ruff b\n# c\n}\n", 0..35),
        (b"proc f {} {\nexpr 1 \\\n#ruff b\n}\n", 0..30),
        (b"proc f {} {\nset x 1 \\\n#ruffopt a b\n}\n", 0..36),
        (b"proc f {} {\r\nset x 1 \\\r\n#ruff b\r\n}\r\n", 0..34),
    ] {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                retained(source, retain),
                undocumented(procedure.clone()),
                "{}",
                source.escape_ascii()
            );
        }
    }
    // Without the backslash the same rows are a marked block and a directive.
    assert_eq!(
        view(b"proc f {} {\nset x 1\n#ruff b\n}\n"),
        owned(rows([20..27], "b", [(0..1, Copied(26..27))]), 0..29)
    );
    assert_eq!(
        retained(
            b"proc f {} {\nset x 1\n#ruffopt a b\n}\n",
            Retain::Directives
        ),
        View {
            evidence: vec![(CandidateDisposition::Directive, 20..32)],
            ..undocumented(0..34)
        }
    );
    assert_views([
        // After `;` and an escaped line end Tcl reads a comment, and `info body` still
        // shows it on the line of `set`: Ruff! reads no row there.
        (
            b"proc f {} {\n# lead\nset x 1 ;\\\n#ruff b\n}\n",
            owned(rows([12..18], "lead", [(0..4, Copied(14..18))]), 0..39),
        ),
        // A row that holds only a backslash is no blank row. With a blank row before or
        // after it the block ends.
        (
            b"proc f {} {\n# lead\n\\\n# b\n}\n",
            owned(
                rows(
                    [12..18, 21..24],
                    "lead\nb",
                    [
                        (0..4, Copied(14..18)),
                        (4..5, Synthetic),
                        (5..6, Copied(23..24)),
                    ],
                ),
                0..26,
            ),
        ),
        (
            b"proc f {} {\n# lead\n\\\n\n# b\n}\n",
            owned(rows([12..18], "lead", [(0..4, Copied(14..18))]), 0..27),
        ),
        (
            b"proc f {} {\n# lead\n\n\\\n# b\n}\n",
            owned(rows([12..18], "lead", [(0..4, Copied(14..18))]), 0..27),
        ),
        (
            b"proc f {} {\r\n# lead\r\n\\\r\n\r\n# b\r\n}\r\n",
            owned(rows([13..19], "lead", [(0..4, Copied(15..19))]), 0..32),
        ),
    ]);
}

/// A spelling of `proc` that the grammar reads as a plain command: the tree holds no
/// `procedure` node, and nothing is published under any retention option.
#[cfg(feature = "tcl")]
fn assert_deferred(source: &[u8]) {
    let name = source.escape_ascii();
    let parser = parsed(source);
    assert!(
        parser
            .root_node()
            .preorder()
            .all(|node| node.as_tree_sitter().kind() != "procedure"),
        "{name}"
    );
    for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
        assert_eq!(retained(source, retain), View::default(), "{name}");
    }
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_qualified_proc_command_is_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"::proc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_quoted_proc_command_is_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"\"proc\" f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_braced_proc_command_is_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"{proc} f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_escaped_proc_command_is_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"pr\\oc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_expanded_proc_command_is_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"{*}proc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_words_of_eval_are_deferred() {
    // Tcl defines `f` here. Today's outcome, pinned: no document and no assessment.
    assert_deferred(b"eval proc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_quoted_parameter_list_is_deferred() {
    // Tcl defines `f` here. The grammar gives an `ERROR` and no procedure for a quoted
    // parameter list. This pins today's outcome and is no design.
    assert_deferred(b"proc f \"x\" {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_glued_first_word_is_deferred() {
    // Tcl defines `f` here: the bracket gives an empty string, and the first word is
    // `proc`. The grammar gives a plain command. This pins today's outcome and is no
    // design.
    assert_deferred(b"[]proc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_after_a_brace_in_a_word_is_deferred() {
    // Tcl defines `f` here: `a{` and `}` are words, because a brace opens a word only at
    // its start. The grammar reads a braced word from the first brace to the last, and
    // no procedure. This pins today's outcome and is no design.
    assert_deferred(b"set s a{\nproc f {} {\n# doc\n}\nset t }\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_after_a_quote_in_a_word_is_deferred() {
    // Tcl defines `f` here: a quote opens a word only at its start. The grammar reads a
    // quoted word from the first quote to the second, and no procedure. This pins
    // today's outcome and is no design.
    assert_deferred(b"set s a\"\nproc f {} {\n# doc\n}\nset t b\"\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_after_a_quote_in_a_braced_word_is_deferred() {
    // Tcl defines `f` here: a quote in a braced word is text. The grammar reads a quoted
    // word from the quote in the first braced word to the quote in the second, reports
    // no error, and gives no procedure. This pins today's outcome and is no design.
    let source: &[u8] = b"set q {\"}\nproc f {} {\n# doc\n}\nset r {\"}\n";
    assert!(!parsed(source).root_node().has_error());
    assert_deferred(source);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_after_a_dollar_at_the_end_of_a_word_is_deferred() {
    // Tcl defines `f` here: a `$` that no name follows is text. The grammar reads a
    // variable that reaches over the rows below, and no procedure. This pins today's
    // outcome and is no design.
    assert_deferred(b"set s a$\nproc f {} {\n# doc\n}\n");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_proc_in_a_file_with_carriage_return_line_ends_is_deferred() {
    // Tcl defines `f` here: `source` reads a carriage return alone as a line end. The
    // grammar reads the comment to the end of the file, and no procedure. This pins
    // today's outcome and is no design.
    assert_deferred(b"proc f {} {\r# doc\r}\r");
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_carriage_return_alone_ends_a_line_between_two_nodes() {
    // Tcl reads a CR that no LF follows as a line end when it reads a script file. The
    // grammar reads white space. Between two nodes the unit reads what Tcl reads.
    assert_views([
        // Between two words of the procedure: Tcl ends the command there.
        (
            b"proc f\r{} {\n# doc\n}\n",
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // After the body and before the procedure: the command is one of its own.
        (
            b"proc f {} {\n# doc\n}\rextra\n",
            owned(rows([12..17], "doc", [(0..3, Copied(14..17))]), 0..19),
        ),
        (
            b"set x 1\rproc g {} {\n# doc\n}\n",
            owned(rows([20..25], "doc", [(0..3, Copied(22..25))]), 8..27),
        ),
        // After a line end it is a blank row, which ends the block.
        (
            b"proc f {} {\n# a\n\r# b\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..22),
        ),
        // Inside a node the grammar's reading stands. After `set x 1` and a CR it reads
        // the marker row as a recovery node, so the row is not published.
        (
            b"proc f {} {\r# a\n\rset x 1\r#ruff b\n}\n",
            owned(rows([12..15], "a", [(0..1, Copied(14..15))]), 0..34),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_named_limits_keep_todays_outcome() {
    // Named limits. Each case pins today's outcome and is no design. In the first five Tcl
    // defines no procedure when it runs the file, and the unit names an owner. In the last
    // three Tcl defines the procedure, and the unit names no owner.
    assert_views([
        // `never-reached`: the scan lists the command, and a plain `source` stops at a run-time
        // error before it: an invalid command name, or a wrong number of words of `set`.
        (
            b"}\nproc f {} {\n# doc\n}\n",
            owned(rows([14..19], "doc", [(0..3, Copied(16..19))]), 2..21),
        ),
        (
            b"set x 1 # c \\\n; proc g {} {\n# doc\n}\n",
            owned(rows([28..33], "doc", [(0..3, Copied(30..33))]), 16..35),
        ),
        (
            b"} ; proc g {} {\n# d\n}\n",
            owned(rows([16..19], "d", [(0..1, Copied(18..19))]), 4..21),
        ),
        (
            b"puts a(;) ; proc g {} {\n# doc\n}\n",
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        (
            b"] ; proc g {} {\n# d\n}\n",
            owned(rows([16..19], "d", [(0..1, Copied(18..19))]), 4..21),
        ),
        // `nested-body-ends-elsewhere`: a nested procedure is pending, and rule B25 is not
        // asked for it. Tcl ends this body after `# a `; the text is that of the grammar's rows.
        (
            b"namespace eval n {\nproc f {} {\n# a }\n# b\n}\n}\n",
            View {
                documents: vec![rows(
                    [31..36, 37..40],
                    "a }\nb",
                    [
                        (0..3, Copied(33..36)),
                        (3..4, Synthetic),
                        (4..5, Copied(39..40)),
                    ],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 19..42)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        // `in-a-bracket`: Tcl defines `f`. It runs the `proc` as a command of the bracket's
        // script, and the grammar puts the node at the root. Only a command of the file's own
        // level is an owner.
        (
            b"[# c\nproc f {} {\n# doc\n};]",
            unowned(
                rows([17..22], "doc", [(0..3, Copied(19..22))]),
                5..24,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"[;proc f {} {\n# doc\n};]",
            unowned(
                rows([14..19], "doc", [(0..3, Copied(16..19))]),
                2..21,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // `glued-first-word`: Tcl defines `f`. The first word of its command is `[]proc`,
        // which only ends with the node's `proc`.
        (
            b"set x 1];[]proc f {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_procedure_whose_braced_word_tcl_ends_elsewhere_has_no_owner_and_no_text() {
    // Tcl ends a word that starts with `{` where the raw brace count returns to zero. A
    // backslash hides the next byte, and nothing else is special. The grammar reads a
    // comment, a quoted word and a bracket inside the word first, so a brace in one of
    // them moves the end of the node away from the end of Tcl's word. The rows the unit
    // reads are then not known to be rows of Tcl's body.
    assert_views([
        // Tcl ends the body after `# a ` and then runs the command `}`.
        (
            b"proc f {} {\n# a }\n# b\n}\n",
            withheld([12..17, 18..21], 0..23),
        ),
        // Tcl's body reaches to the brace of the comment after the node.
        (b"proc f {} {\n# doc {\n}\n# }\n", withheld([12..19], 0..21)),
        // A brace in a quoted word counts for Tcl: its body ends after `# more`.
        (
            b"proc f {} {\n# doc\nset s \"{\"\n}\n# more\n}\n",
            withheld([12..17], 0..29),
        ),
        // The same with a marker row.
        (
            b"proc f {} {\n#ruff a {\n}\n# }\n",
            withheld([12..21], 0..23),
        ),
        // The parameter list: Tcl ends it at the quoted brace and reports extra
        // characters after it.
        (
            b"proc g {a {b \"}\"}} {\n# a\n}\n",
            withheld([21..24], 0..26),
        ),
        // A quoted brace in a bracket ends the body for Tcl.
        (
            b"proc f {} {\n# doc\nset s [list \"}\"]\n}\n",
            withheld([12..17], 0..36),
        ),
        // The count comes before the name: a computed name does not bring the text back.
        (
            b"proc $name {} {\n# a }\n# b\n}\n",
            withheld([16..21, 22..25], 0..27),
        ),
        // Nor does the continued line of a comment, on which the procedure is no command.
        (
            b"# c \\\nproc f {} {\n# a }\n# b\n}\n",
            withheld([18..23, 24..27], 6..29),
        ),
    ]);
    // Without rows there is no document, and the procedure is no owner.
    assert_eq!(
        view(b"proc f {} {\nset s \"{\"\n}\n}\n"),
        unread("procedure", 0..23)
    );
    // A directive row of such a body is kept as a candidate, and the body carries it.
    let directive: &[u8] = b"proc f {} {\n#ruffopt a b\n# doc {\n}\n# }\n";
    assert_eq!(view(directive), withheld([25..32], 0..34));
    let parser = parsed(directive);
    for index in indexes(&parser, Retain::Directives) {
        assert_eq!(
            published(&index),
            View {
                evidence: vec![(CandidateDisposition::Directive, 12..24)],
                ..withheld([25..32], 0..34)
            }
        );
        let candidate = index.evidence().next().unwrap();
        let fragment = index.evidence_fragment(candidate).unwrap();
        assert_eq!(
            spelled(fragment.get().node().unwrap().node()),
            ("braced_word", 10..34)
        );
    }
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_procedure_inside_an_earlier_braced_word_is_no_owner() {
    // The count of an earlier word of the root reaches past the start of the procedure:
    // for Tcl the procedure is text of that word. Its own rows keep their text.
    assert_views([
        // No grammar error and no Tcl error: Tcl defines `g` with the text of `f` in its
        // body, and no `f`. The body of `g` ends elsewhere for Tcl, so its row has no
        // text either.
        (
            b"proc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n",
            joined([
                unowned(
                    parted([12..15]),
                    0..17,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                unowned(
                    rows([30..35], "doc", [(0..3, Copied(32..35))]),
                    18..37,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        // A brace in a quoted word, and in a quoted word in a bracket.
        (
            b"proc g {} {\nset s \"{\"\n}\nproc f {} {\n# doc\n}\n}\n",
            joined([
                unread("procedure", 0..23),
                unowned(
                    rows([36..41], "doc", [(0..3, Copied(38..41))]),
                    24..43,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        (
            b"proc g {} {\nset s [list \"{\"]\n}\nproc f {} {\n# doc\n}\n}\n",
            joined([
                unread("procedure", 0..30),
                unowned(
                    rows([43..48], "doc", [(0..3, Copied(45..48))]),
                    31..50,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        // The second backslash is hidden by the first, so the brace counts.
        (
            b"proc g {} {\n# \\\\{\n}\nproc f {} {\n# doc\n}\n}\n",
            joined([
                unowned(
                    parted([12..17]),
                    0..19,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                unowned(
                    rows([32..37], "doc", [(0..3, Copied(34..37))]),
                    20..39,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        // The earlier word belongs to another command than `proc`.
        (
            b"if {1} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n",
            unowned(
                rows([27..32], "doc", [(0..3, Copied(29..32))]),
                15..34,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // A comment after `;` in the earlier body.
        (
            b"proc g {} {\nset x 1 ;# {\n}\nproc f {} {\n# doc\n}\n# }\n",
            joined([
                unread("procedure", 0..26),
                unowned(
                    rows([39..44], "doc", [(0..3, Copied(41..44))]),
                    27..46,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        // The count of the earlier word never returns to zero: Tcl reports a missing
        // close-brace and defines nothing.
        (
            b"proc g {} {\n# if {\nputs a\n}\nproc f {} {\n# doc\n}\n",
            joined([
                unowned(
                    parted([12..18]),
                    0..27,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                unowned(
                    rows([40..45], "doc", [(0..3, Copied(42..45))]),
                    28..47,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        // The grammar closes the word at the `;` with a missing token.
        (
            b"set x {a ;\nproc g {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // The grammar leaves the brace open: it is a token of a recovery node, with no
        // braced word around it.
        (
            b"set s $a# {\nproc f {} {\n# doc\n}\n",
            unowned(
                rows([24..29], "doc", [(0..3, Copied(26..29))]),
                12..31,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // A brace that is open inside a bracket.
        (
            b"set x [a {b] ;\nproc g {} {\n# doc\n}\n",
            unowned(
                rows([27..32], "doc", [(0..3, Copied(29..32))]),
                15..34,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // A braced word starts after `;`, after a line end, after `[`, after the
        // expansion prefix, and at the start of the source.
        (
            b"set a 1 ;{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            unowned(
                rows([28..33], "doc", [(0..3, Copied(30..33))]),
                16..35,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set a 1\n{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            unowned(
                rows([27..32], "doc", [(0..3, Copied(29..32))]),
                15..34,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set s [{# {\n} a]\nproc g {} {\n# doc\n}\n# }\n",
            unowned(
                rows([29..34], "doc", [(0..3, Copied(31..34))]),
                17..36,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"list {*}{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            unowned(
                rows([27..32], "doc", [(0..3, Copied(29..32))]),
                15..34,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            unowned(
                rows([19..24], "doc", [(0..3, Copied(21..24))]),
                7..26,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
    ]);
    // The root is a recovery node, and the count from its first open brace never returns
    // to zero: the procedure is no candidate either.
    let source: &[u8] =
        b"proc f {{a0 { {a1 { {a2 { {a3 {} {\n# audit\n}\nproc g {} {\n# doc g\n}\n";
    let parser = parsed(source);
    assert!(parser.ts_tree().root_node().is_error());
    assert_eq!(
        agreed(&indexes(&parser, Retain::Nothing)),
        unowned(
            rows([57..64], "doc g", [(0..5, Copied(59..64))]),
            45..66,
            Some(UnresolvedReason::GrammarBoundary),
        )
    );
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_braces_that_tcl_and_the_grammar_count_alike_withhold_nothing() {
    // Tcl defines every procedure that is an owner here, with the body the unit reads.
    assert_views([
        // A balanced pair in a comment row, and a pair over two comment rows of one body.
        (
            b"proc g {} {\n# {}\n}\nproc f {} {\n# doc\n}\n",
            joined([
                owned(rows([12..16], "{}", [(0..2, Copied(14..16))]), 0..18),
                owned(rows([31..36], "doc", [(0..3, Copied(33..36))]), 19..38),
            ]),
        ),
        (
            b"proc g {} {\n# {\n# }\n}\nproc f {} {\n# doc\n}\n",
            joined([
                owned(
                    rows(
                        [12..15, 16..19],
                        "{\n}",
                        [
                            (0..1, Copied(14..15)),
                            (1..2, Synthetic),
                            (2..3, Copied(18..19)),
                        ],
                    ),
                    0..21,
                ),
                owned(rows([34..39], "doc", [(0..3, Copied(36..39))]), 22..41),
            ]),
        ),
        // A backslash hides the brace.
        (
            b"proc g {} {\n# \\{\n}\nproc f {} {\n# doc\n}\n",
            joined([
                owned(rows([12..16], "\\{", [(0..2, Copied(14..16))]), 0..18),
                owned(rows([31..36], "doc", [(0..3, Copied(33..36))]), 19..38),
            ]),
        ),
        // A quoted word with a balanced pair.
        (
            b"proc g {} {\nset s \"{}\"\n}\nproc f {} {\n# doc\n}\n",
            joined([
                undocumented(0..24),
                owned(rows([37..42], "doc", [(0..3, Copied(39..42))]), 25..44),
            ]),
        ),
        // A regular expression in braces, before the procedure and in its body.
        (
            b"set re {^[a-z]{1,4}$}\nproc f {} {\n# doc\nregexp {^[a-z]{1,4}$} $s\n}\n",
            owned(rows([34..39], "doc", [(0..3, Copied(36..39))]), 22..66),
        ),
        // CRLF line ends.
        (
            b"set x {a b}\r\nproc f {} {\r\n# doc {}\r\n}\r\n",
            owned(rows([26..34], "doc {}", [(0..6, Copied(28..34))]), 13..37),
        ),
        // Tcl ends the body of `g` in the comment row after its node, before `f` starts:
        // `g` is no owner, and `f` is the same command for Tcl.
        (
            b"proc g {} {\n# {\n}\n# }\nproc f {} {\n# doc\n}\n",
            joined([
                unowned(
                    parted([12..15]),
                    0..17,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                owned(rows([34..39], "doc", [(0..3, Copied(36..39))]), 22..41),
            ]),
        ),
        // `g` lies inside the word of `if` for Tcl; `h` starts after that word ends.
        (
            b"if {0} {\nset s \"{\"\n}\nproc g {} {\n# doc\n}\n}\nproc h {} {\n# later\n}\n",
            joined([
                unowned(
                    rows([33..38], "doc", [(0..3, Copied(35..38))]),
                    21..40,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                owned(rows([55..62], "later", [(0..5, Copied(57..62))]), 43..64),
            ]),
        ),
        // After a part of a word a brace starts no word. The grammar closes this one
        // with a missing token, and reads the second as words in a recovery node.
        (
            b"set s a{b ;\nproc g {} {\n# doc\n}\n",
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        (
            b"list ){{}\nproc f {} {\n# doc\n}\n",
            owned(rows([22..27], "doc", [(0..3, Copied(24..27))]), 10..29),
        ),
        // A brace in a quoted word, an escaped brace, and a brace in a comment of the
        // file open no word.
        (
            b"set s \"{\"\nproc f {} {\n# doc\n}\n",
            owned(rows([22..27], "doc", [(0..3, Copied(24..27))]), 10..29),
        ),
        (
            b"set s \\{\nproc f {} {\n# doc\n}\n",
            owned(rows([21..26], "doc", [(0..3, Copied(23..26))]), 9..28),
        ),
        (
            b"# {\nproc f {} {\n# doc\n}\n",
            owned(rows([16..21], "doc", [(0..3, Copied(18..21))]), 4..23),
        ),
        // Words that end where their nodes end: after the expansion prefix and after `;`.
        (
            b"set l [list {*}{a b}] ;{c} d\nproc g {} {\n# doc\n}\n",
            owned(rows([41..46], "doc", [(0..3, Copied(43..46))]), 29..48),
        ),
        // The braces of a variable name are no word.
        (
            b"set v ${name}\nproc f {} {\n# doc\n}\n",
            owned(rows([26..31], "doc", [(0..3, Copied(28..31))]), 14..33),
        ),
        // A braced word that ends where its node ends, three lines above.
        (
            b"set s {a b}\nset t 1\nset u 2\nproc g {} {\n# doc\n}\n",
            owned(rows([40..45], "doc", [(0..3, Copied(42..45))]), 28..47),
        ),
    ]);
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_owner_is_read_from_the_bytes_where_the_tokens_differ_from_tcl() {
    // The grammar reads a comment, a quoted word and a bracket wherever they stand, and it
    // reads a comment to the line feed. Where Tcl reads the bytes otherwise, the scan of
    // the file decides whether the procedure is the command Tcl would run.
    assert_views([
        // A brace in a comment that continues is comment text for Tcl: it defines `f`.
        (
            b"# c \\\nproc g {} {\n# { \\\n}\nproc f {} {\n# doc\n}\n# }\n",
            joined([
                unowned(
                    parted([18..23]),
                    6..25,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                owned(rows([38..43], "doc", [(0..3, Copied(40..43))]), 26..45),
            ]),
        ),
        (
            b"# c \\\nproc g {} {\n# x \\\n}\nproc f {} {\n# doc\n}\n# }\n",
            joined([
                unowned(
                    unavailable([18..23]),
                    6..25,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                owned(rows([38..43], "doc", [(0..3, Copied(40..43))]), 26..45),
            ]),
        ),
        // A braced row below a comment that continues is comment text as well.
        (
            b"# c \\\n{a \"{\"}\nproc f {} {\n# doc\n}\n",
            owned(rows([26..31], "doc", [(0..3, Copied(28..31))]), 14..33),
        ),
        // The body of `g` ends at the `}` of its first row for Tcl, and the body of `h` holds the
        // text of `f`. The grammar nests `h`, and no node shows where its body ends for Tcl.
        (
            b"proc g {} {\n# }\nproc h {} {\n# {{\n}\n}\nproc f {} {\n# doc\n}\n# }\n",
            View {
                documents: vec![
                    parted([12..15]),
                    rows([28..32], "{{", [(0..2, Copied(30..32))]),
                    rows([49..54], "doc", [(0..3, Copied(51..54))]),
                ],
                associations: vec![
                    edge(
                        0,
                        Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                        [],
                    ),
                    edge(
                        2,
                        Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                        [],
                    ),
                ],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 0..36)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 16..34)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 37..56)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(1)),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(2)),
                ],
                ..View::default()
            },
        ),
        (
            b"proc g {} {\n# }\nif {0} {\n# {{\n}\n}\nproc f {} {\n# doc\n}\n# }\n",
            joined([
                unowned(
                    parted([12..15]),
                    0..33,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
                unowned(
                    rows([46..51], "doc", [(0..3, Copied(48..51))]),
                    34..53,
                    Some(UnresolvedReason::GrammarBoundary),
                ),
            ]),
        ),
        (
            b"proc g {} {\n# x\nproc h {} {\n# y\n}\n}\nproc f {} {\n# doc\n}\n",
            View {
                procedures: vec![0..35, 36..55],
                documents: vec![
                    rows([12..15], "x", [(0..1, Copied(14..15))]),
                    rows([28..31], "y", [(0..1, Copied(30..31))]),
                    rows([48..53], "doc", [(0..3, Copied(50..53))]),
                ],
                associations: vec![
                    edge(0, Resolution::Resolved, [0..35]),
                    edge(2, Resolution::Resolved, [36..55]),
                ],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 16..33)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(1)),
                ],
                ..View::default()
            },
        ),
        // A carriage return alone ends a comment for Tcl. The grammar reads the comment on to
        // the line feed, so the brace after the CR is no token of the tree.
        (
            b"# c\rset x {\nproc f {} {\n# doc\n}\n}\n",
            unowned(
                rows([24..29], "doc", [(0..3, Copied(26..29))]),
                12..31,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"# c\rproc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n",
            unowned(
                rows([34..39], "doc", [(0..3, Copied(36..39))]),
                22..41,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set x 1 ;# c\rset y {\nproc f {} {\n# doc\n}\n}\n",
            unowned(
                rows([33..38], "doc", [(0..3, Copied(35..38))]),
                21..40,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"# c\nset x 1\nproc f {} {\n# doc\n}\n",
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        // Tcl 8.6 reads a byte order mark as part of the first word.
        (
            b"\xef\xbb\xbfproc f {} {\n# doc\n}\n",
            unowned(
                rows([15..20], "doc", [(0..3, Copied(17..20))]),
                3..22,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // Tcl reports a parse error in the command before the procedure and reads nothing
        // after it: a missing close-brace and a missing close-bracket,
        (
            b"set x {a ; proc g {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set x [a ; proc g {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set x [a ;\nproc g {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // extra characters after a close-brace and after a close-quote,
        (
            b"set s {a}b\nproc f {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set s \"a\"b\nproc f {} {\n# doc\n}\n",
            unowned(
                rows([23..28], "doc", [(0..3, Copied(25..28))]),
                11..30,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"proc g {} {\nset s \"}\"\n}\nproc f {} {\n# doc\n}\n",
            View {
                documents: vec![rows([36..41], "doc", [(0..3, Copied(38..41))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 0..23)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 24..43)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        // For Tcl a `#` in the middle of a command is a word, and the brace or the quote after it
        // opens a word that holds the procedure.
        (
            b"set x 1 # c {\nproc f {} {\n# doc\n}\n}\n",
            unowned(
                rows([26..31], "doc", [(0..3, Copied(28..31))]),
                14..33,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"set x 1 # \"\nproc f {} {\n# doc\n}\n\"\n",
            unowned(
                rows([24..29], "doc", [(0..3, Copied(26..29))]),
                12..31,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // The index of an array element reaches to its `)`.
        (
            b"set s $a(\nproc f {} {\n# doc\n}\n)\n",
            unowned(
                rows([22..27], "doc", [(0..3, Copied(24..27))]),
                10..29,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // `source` stops at the byte 0x1a.
        (
            b"set x 1\n\x1a\nproc g {} {\n# doc\n}\n",
            unowned(
                rows([22..27], "doc", [(0..3, Copied(24..27))]),
                10..29,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
    ]);
}

/// A source, its included ranges, and what a raw collection of that parse publishes.
#[cfg(feature = "tcl")]
type Restricted = (&'static [u8], &'static [Range<usize>], View);

/// A raw parse of the included byte ranges only.
#[cfg(feature = "tcl")]
fn restricted(source: &[u8], ranges: &[Range<usize>]) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let mut rows = source[..offset].rsplit(|byte| *byte == b'\n');
        let column = rows.next().map_or(0, <[u8]>::len);
        tree_sitter::Point::new(rows.count(), column)
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
        .set_language(&LANG::Tcl.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_excluded_bytes_withhold_what_they_cut() {
    let context = DocumentationContext::default();
    // A source, its included ranges, and what a raw collection of the restricted parse
    // publishes, whatever is retained.
    let cases: [Restricted; 46] = [
        // An excluded byte in the procedure: it is not read, and it and its body are pending.
        (
            b"proc f {} {\n# audit\nreturn\n}\n",
            &[0..20, 21..29],
            View {
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("braced_word", 10..28)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 0..28)),
                        None,
                    ),
                ],
                ..View::default()
            },
        ),
        // Two included ranges that meet inside a row exclude nothing.
        (
            b"proc f {} {\n# audit\nreturn\n}\n",
            &[0..15, 15..29],
            owned(
                rows(
                    [12..19],
                    "audit",
                    [(0..1, Copied(14..15)), (1..5, Copied(15..19))],
                ),
                0..28,
            ),
        ),
        // At an excluded byte the state of the scan is not known. It is known again after the
        // first later line end that the included bytes prove unescaped. A procedure that starts
        // before that has no owner: the excluded bytes may hold a word of its command, a `;`,
        // or the end of a comment that continues. The line end is not proven where the byte
        // before it, or before the backslashes that stand before it, is excluded.
        (
            b"# c\nJUNK\nproc g {} {\n# inner\n}\n",
            &[0..4, 9..31],
            unowned(
                rows([21..28], "inner", [(0..5, Copied(23..28))]),
                9..30,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"# c\nJUNK\nproc g {} {\n# inner\n}\n",
            &[9..31],
            unowned(
                rows([21..28], "inner", [(0..5, Copied(23..28))]),
                9..30,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"# c \\\nJUNK\nproc g {} {\n# inner\n}\n",
            &[0..6, 11..33],
            unowned(
                rows([23..30], "inner", [(0..5, Copied(25..30))]),
                11..32,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"# c \\\nproc g {} {\n# inner\n}\n",
            &[0..2, 3..28],
            View {
                documents: vec![rows([18..25], "inner", [(0..5, Copied(20..25))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 6..27)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                    pending(
                        AssessmentAxis::Selection,
                        EXCLUDED,
                        Some(("comment", 0..5)),
                        None,
                    ),
                ],
                ..View::default()
            },
        ),
        (
            b"set x 1 proc g {} {\n# doc\n}\n",
            &[8..28],
            unowned(
                rows([20..25], "doc", [(0..3, Copied(22..25))]),
                8..27,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"set x 1 ; proc g {} {\n# doc\n}\n",
            &[0..4, 5..30],
            View {
                documents: vec![rows([22..27], "doc", [(0..3, Copied(24..27))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("ERROR", 0..7)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 10..29)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"set x 1 ; proc g {} {\n# doc\n}\n",
            &[0..8, 9..30],
            unowned(
                rows([22..27], "doc", [(0..3, Copied(24..27))]),
                10..29,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"set x 1 ; set y 2 proc g {} {\n# doc\n}\n",
            &[0..4, 5..38],
            View {
                documents: vec![rows([30..35], "doc", [(0..3, Copied(32..35))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("ERROR", 0..7)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 18..37)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"X\nproc g {} {\n# doc\n}\n",
            &[1..22],
            unowned(
                rows([14..19], "doc", [(0..3, Copied(16..19))]),
                2..21,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"JUN\\\\\nproc g {} {\n# doc\n}\n",
            &[3..26],
            unowned(
                rows([18..23], "doc", [(0..3, Copied(20..23))]),
                6..25,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        // Behind such a line end a procedure is read as in a whole parse: after an excluded
        // word, after an excluded byte two lines above, after CR LF, after an even run of
        // backslashes that follows an included byte, after a second line end, and after the
        // line end of a comment that was cut.
        (
            b"# c\nJUNK\nproc g {} {\n# inner\n}\n",
            &[0..4, 7..31],
            owned(rows([21..28], "inner", [(0..5, Copied(23..28))]), 9..30),
        ),
        (
            b"set s {a b}\nset t 1\nset u 2\nproc g {} {\n# doc\n}\n",
            &[0..16, 17..48],
            View {
                procedures: vec![28..47],
                documents: vec![rows([40..45], "doc", [(0..3, Copied(42..45))])],
                associations: vec![edge(0, Resolution::Resolved, [28..47])],
                assessments: vec![pending(
                    AssessmentAxis::Owner,
                    PENDING_OWNER,
                    Some(("ERROR", 12..19)),
                    None,
                )],
                ..View::default()
            },
        ),
        (
            b"JUNK\r\nproc g {} {\r\n# doc\r\n}\r\n",
            &[3..29],
            owned(rows([19..24], "doc", [(0..3, Copied(21..24))]), 6..27),
        ),
        (
            b"JUNK\\\\\nproc g {} {\n# doc\n}\n",
            &[3..27],
            owned(rows([19..24], "doc", [(0..3, Copied(21..24))]), 7..26),
        ),
        (
            b"JUNK\n\nproc g {} {\n# doc\n}\n",
            &[4..26],
            owned(rows([18..23], "doc", [(0..3, Copied(20..23))]), 6..25),
        ),
        (
            b"# abc\nproc g {} {\n# doc\n}\n",
            &[0..3, 4..26],
            View {
                procedures: vec![6..25],
                documents: vec![rows([18..23], "doc", [(0..3, Copied(20..23))])],
                associations: vec![edge(0, Resolution::Resolved, [6..25])],
                assessments: vec![pending(
                    AssessmentAxis::Selection,
                    EXCLUDED,
                    Some(("comment", 0..5)),
                    None,
                )],
                ..View::default()
            },
        ),
        // The state is never known again if a brace, a quote, a bracket, an index or the brace
        // of a variable name was open at the excluded byte.
        (
            b"set s {a b}\nset t 1\nset u 2\nproc g {} {\n# doc\n}\n",
            &[0..7, 8..48],
            View {
                documents: vec![rows([40..45], "doc", [(0..3, Copied(42..45))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("braced_word_simple", 6..11)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 28..47)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("set", 0..11)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"set s \"a b\"\nproc g {} {\n# doc\n}\n",
            &[0..7, 8..32],
            View {
                documents: vec![rows([24..29], "doc", [(0..3, Copied(26..29))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 12..31)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("quoted_word", 6..11)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("set", 0..11)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"set s [a b]\nproc g {} {\n# doc\n}\n",
            &[0..7, 8..32],
            View {
                documents: vec![rows([24..29], "doc", [(0..3, Copied(26..29))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("command_substitution", 6..11)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 12..31)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("set", 0..11)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"set s $a(1)\nproc g {} {\n# doc\n}\n",
            &[0..9, 10..32],
            View {
                documents: vec![rows([24..29], "doc", [(0..3, Copied(26..29))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("ERROR", 0..11)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 12..31)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        (
            b"set s ${a b}\nproc g {} {\n# doc\n}\n",
            &[0..8, 9..33],
            View {
                documents: vec![rows([25..30], "doc", [(0..3, Copied(27..30))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 13..32)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("set", 0..12)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("variable_substitution", 6..12)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        // The command of the procedure reaches an excluded byte before it ends: a word after
        // the body, the rest of the source, the line end, white space, and a second word.
        (
            b"proc f {} {\n# doc\n} extra\n",
            &[0..19, 25..26],
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"proc f {} {\n# doc\n} extra\n",
            &[0..19],
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"proc f {} {\n# audit\nreturn\n}\n",
            &[0..28],
            unowned(
                rows([12..19], "audit", [(0..5, Copied(14..19))]),
                0..28,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"proc f {} {\n# doc\n} JUNK\n",
            &[0..20, 24..25],
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::ExcludedGap),
            ),
        ),
        (
            b"proc f {} {\n# doc\n} {a}JUNK\n",
            &[0..23, 26..28],
            View {
                documents: vec![rows([12..17], "doc", [(0..3, Copied(14..17))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("command", 20..27)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 0..19)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        // Excluded bytes behind the end of the command change nothing.
        (
            b"proc g {} {\n# inner\n}\nJUNK\n",
            &[0..22, 23..27],
            owned(rows([12..19], "inner", [(0..5, Copied(14..19))]), 0..21),
        ),
        // A reason that the included bytes prove stands: the comment that continues before
        // the procedure, and the byte 0x1a, which ends the file whatever the state.
        (
            b"# c \\\nproc g {} {\n# doc\n}\nJUNK\n",
            &[0..26, 29..31],
            unowned(
                rows([18..23], "doc", [(0..3, Copied(20..23))]),
                6..25,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"JUNK\x1a\nproc g {} {\n# doc\n}\n",
            &[3..26],
            unowned(
                rows([18..23], "doc", [(0..3, Copied(20..23))]),
                6..25,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        // An excluded byte in an open brace makes the scan state unknown. Even an
        // included end-of-file byte cannot establish a later command in that state.
        (
            b"set x {ab}\n\x1a\nproc f {} {\n# doc\n}\n",
            &[0..7, 8..33],
            View {
                documents: vec![rows([25..30], "doc", [(0..3, Copied(27..30))])],
                associations: vec![edge(
                    0,
                    Resolution::Unresolved(UnresolvedReason::ExcludedGap),
                    [],
                )],
                assessments: vec![
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("braced_word_simple", 6..10)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("procedure", 13..32)),
                        None,
                    ),
                    pending(
                        AssessmentAxis::Owner,
                        PENDING_OWNER,
                        Some(("set", 0..10)),
                        None,
                    ),
                    pending(AssessmentAxis::Owner, PENDING_DOCUMENT, None, Some(0)),
                ],
                ..View::default()
            },
        ),
        // The sources of the rows above that no other test parses whole, with every byte
        // included. Tcl stops at a run-time error before most of these procedures; the scan
        // lists them (the named limit `never-reached`).
        (
            b"JUNK\r\nproc g {} {\r\n# doc\r\n}\r\n",
            &[0..29],
            owned(rows([19..24], "doc", [(0..3, Copied(21..24))]), 6..27),
        ),
        (
            b"JUN\\\\\nproc g {} {\n# doc\n}\n",
            &[0..26],
            owned(rows([18..23], "doc", [(0..3, Copied(20..23))]), 6..25),
        ),
        (
            b"JUNK\\\\\nproc g {} {\n# doc\n}\n",
            &[0..27],
            owned(rows([19..24], "doc", [(0..3, Copied(21..24))]), 7..26),
        ),
        (
            b"set s \"a b\"\nproc g {} {\n# doc\n}\n",
            &[0..32],
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        (
            b"set s [a b]\nproc g {} {\n# doc\n}\n",
            &[0..32],
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        (
            b"set s $a(1)\nproc g {} {\n# doc\n}\n",
            &[0..32],
            owned(rows([24..29], "doc", [(0..3, Copied(26..29))]), 12..31),
        ),
        (
            b"set s ${a b}\nproc g {} {\n# doc\n}\n",
            &[0..33],
            owned(rows([25..30], "doc", [(0..3, Copied(27..30))]), 13..32),
        ),
        (
            b"# abc\nproc g {} {\n# doc\n}\n",
            &[0..26],
            owned(rows([18..23], "doc", [(0..3, Copied(20..23))]), 6..25),
        ),
        (
            b"X\nproc g {} {\n# doc\n}\n",
            &[0..22],
            owned(rows([14..19], "doc", [(0..3, Copied(16..19))]), 2..21),
        ),
        (
            b"JUNK\n\nproc g {} {\n# doc\n}\n",
            &[0..26],
            owned(rows([18..23], "doc", [(0..3, Copied(20..23))]), 6..25),
        ),
        // In these four no command of Tcl is the procedure: the byte 0x1a ends the file, the
        // comment continues, text follows a close-brace, and a fifth word follows the body.
        (
            b"JUNK\x1a\nproc g {} {\n# doc\n}\n",
            &[0..26],
            unowned(
                rows([18..23], "doc", [(0..3, Copied(20..23))]),
                6..25,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"# c \\\nproc g {} {\n# doc\n}\nJUNK\n",
            &[0..31],
            unowned(
                rows([18..23], "doc", [(0..3, Copied(20..23))]),
                6..25,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"proc f {} {\n# doc\n} {a}JUNK\n",
            &[0..28],
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
        (
            b"proc f {} {\n# doc\n} JUNK\n",
            &[0..25],
            unowned(
                rows([12..17], "doc", [(0..3, Copied(14..17))]),
                0..19,
                Some(UnresolvedReason::GrammarBoundary),
            ),
        ),
    ];
    for (source, ranges, expected) in cases {
        let tree = restricted(source, ranges);
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            let index = collect(LANG::Tcl, &tree, source, &context, &options(retain)).unwrap();
            assert_eq!(
                published(&index),
                expected,
                "{} {ranges:?}",
                source.escape_ascii()
            );
        }
    }
    let whole: &[u8] = b"proc f {} {\n# audit\nreturn\n}\n";
    // The row that two ranges split keeps both pieces.
    let tree = restricted(whole, &[0..15, 15..29]);
    let index = collect(
        LANG::Tcl,
        &tree,
        whole,
        &context,
        &DocumentationOptions::default(),
    )
    .unwrap();
    let document = index.documents().next().unwrap();
    let fragment = index.facts().document_fragment(document, 0).unwrap();
    assert_eq!(
        fragment
            .get()
            .source()
            .pieces()
            .iter()
            .map(SourcePiece::range)
            .collect::<Vec<_>>(),
        [12..15, 15..19]
    );
}

#[cfg(feature = "tcl")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Charge {
    Step,
    Bytes(usize),
}

/// A budget that counts what is charged and keeps the charges in order, and fails every
/// charge from the one that brings the count to `fail_at`.
#[cfg(feature = "tcl")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    made: RefCell<Vec<Charge>>,
    fail_at: Option<usize>,
}

#[cfg(feature = "tcl")]
impl Counting {
    fn charge(&self) -> Result<(), SourceError> {
        if self
            .fail_at
            .is_some_and(|at| self.steps.get() + self.bytes.get() >= at)
        {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "tcl")]
    fn charged(&self) -> (usize, usize) {
        (self.steps.get(), self.bytes.get())
    }
}

#[cfg(feature = "tcl")]
impl WorkBudget for Counting {
    type Error = SourceError;

    #[cfg(feature = "tcl")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }

    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        self.made.borrow_mut().push(Charge::Step);
        self.charge()
    }

    #[cfg(feature = "tcl")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        self.made.borrow_mut().push(Charge::Bytes(count));
        self.charge()
    }
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_byte_scans_charge_every_byte_they_read() {
    // expected: the run of backslashes at the end, and the byte before it if there is one.
    let continues: [(&[u8], bool, usize); 7] = [
        (b"# c \\", true, 2),
        (b"# c \\\\", false, 3),
        (b"# c \\\\\\", true, 4),
        (b"# c", false, 1),
        (b"# c \\ ", false, 1),
        (b"\\", true, 1),
        (b"", false, 0),
    ];
    for (spelling, answer, bytes) in continues {
        let budget = Counting::default();
        assert_eq!(line_continues(spelling, &budget), Ok(answer));
        assert_eq!(budget.charged(), (0, bytes), "{}", spelling.escape_ascii());
        // One charge for each byte, made before the byte is read.
        assert_eq!(budget.made.into_inner(), vec![Charge::Bytes(1); bytes]);
    }
    // The gap between two nodes, whether the node before it ends in an odd run of
    // backslashes, and the limit; then the count and the bytes read.
    // expected: every byte up to and including the line end that brings the count to the
    // limit, or the whole gap. A line end is LF, CR LF, or a CR alone.
    let gaps: [(&[u8], bool, usize, usize, usize); 22] = [
        (b"", false, 2, 0, 0),
        (b" ", false, 2, 0, 1),
        (b"\n", false, 2, 1, 1),
        (b"\n    ", false, 2, 1, 5),
        (b"\n\n", false, 2, 2, 2),
        (b"\n   \n  ", false, 2, 2, 5),
        // A row that holds only a backslash joins its neighbours.
        (b"\n\\\n", false, 2, 1, 3),
        (b"\n\\\n\n ", false, 2, 2, 4),
        (b" \\\n", false, 2, 0, 3),
        (b" \\\r\n", false, 2, 0, 4),
        (b"\r\n\r\n", false, 2, 2, 3),
        // The node before the gap continues: the first line end is escaped.
        (b"\n", true, 2, 0, 1),
        (b"\r\n", true, 2, 0, 2),
        (b"\n\n", true, 2, 1, 2),
        (b"\n\n\n", true, 2, 2, 3),
        // A carriage return that no line feed follows.
        (b"\r", false, 2, 1, 1),
        (b"\r\r\n", false, 2, 2, 2),
        (b"\n\r", false, 2, 2, 2),
        (b"\r", true, 2, 0, 1),
        // With the limit 1 the scan stops at the first line end.
        (b" \n \n", false, 1, 1, 2),
        (b"\\\n\n ", false, 1, 1, 3),
        (b"\t\r\n", false, 1, 1, 2),
    ];
    for (gap, escaped, limit, count, bytes) in gaps {
        let budget = Counting::default();
        assert_eq!(
            unescaped_line_ends(gap, escaped, limit, &budget),
            Ok(count),
            "{} {escaped} {limit}",
            gap.escape_ascii()
        );
        assert_eq!(budget.charged(), (0, bytes), "{}", gap.escape_ascii());
        assert_eq!(budget.made.into_inner(), vec![Charge::Bytes(1); bytes]);
    }
    // expected: the row, up to the nine bytes of `#ruffopt` and the byte after it.
    let marks: [(&[u8], RowMark, usize); 11] = [
        (b"#ruff", RowMark::Opens, 5),
        (b"#ruff x", RowMark::Opens, 7),
        (b"#ruff\tx", RowMark::Opens, 7),
        (b"#ruff\x0cx", RowMark::Text, 7),
        (b"#ruffopt", RowMark::Silent, 8),
        (b"#ruffopt a b", RowMark::Silent, 9),
        (b"#ruffopt\ta", RowMark::Silent, 9),
        (b"#ruffoptions", RowMark::Text, 9),
        (b"#rufftail", RowMark::Text, 9),
        (b"# ruff", RowMark::Text, 6),
        (b"#", RowMark::Text, 1),
    ];
    for (spelling, answer, bytes) in marks {
        let budget = Counting::default();
        assert_eq!(ruff_row(spelling, &budget), Ok(answer));
        assert_eq!(budget.charged(), (0, bytes), "{}", spelling.escape_ascii());
    }
    // Where Tcl ends the braced word that starts at that byte of the source.
    // expected: every byte from the brace up to and including the `}` that brings the
    // count to zero, or to the end of the source where none does.
    let words: [(&[u8], usize, Option<usize>, usize); 16] = [
        (b"{}", 0, Some(2), 2),
        (b"{a b} c", 0, Some(5), 5),
        (b"{a {b} c} d", 0, Some(9), 9),
        (b"x {a} {b}", 2, Some(5), 3),
        (b"x {a} {b}", 6, Some(9), 3),
        // A backslash hides the next byte: a brace, and another backslash.
        (b"{a \\} b} c", 0, Some(8), 8),
        (b"{a \\{ b} c", 0, Some(8), 8),
        (b"{a \\\\} b}", 0, Some(6), 6),
        // Nothing else is special: a comment, a quoted word, a bracket, a line end.
        (b"{\n# }\n}", 0, Some(5), 5),
        (b"{\"}\" }", 0, Some(3), 3),
        (b"{[}] }", 0, Some(3), 3),
        (b"{\n# {\n}\n# }\n", 0, Some(11), 11),
        // The count does not return to zero.
        (b"{\n# {\n}\n", 0, None, 8),
        (b"{", 0, None, 1),
        (b"{a \\}", 0, None, 5),
        (b"{\\", 0, None, 2),
    ];
    for (code, start, end, bytes) in words {
        let budget = Counting::default();
        assert_eq!(
            braced_word_end(code, start, &budget),
            Ok(end),
            "{} {start}",
            code.escape_ascii()
        );
        assert_eq!(budget.charged(), (0, bytes), "{}", code.escape_ascii());
        // One charge for each byte, made before the byte is read.
        assert_eq!(budget.made.into_inner(), vec![Charge::Bytes(1); bytes]);
    }
    for fail_at in 1..=9 {
        let budget = Counting {
            fail_at: Some(fail_at),
            ..Counting::default()
        };
        assert_eq!(
            braced_word_end(b"{a {b} c} d", 0, &budget),
            Err(SourceError::Cancelled)
        );
        assert_eq!(budget.charged(), (0, fail_at));
    }
    for fail_at in 1..=3 {
        let budget = Counting {
            fail_at: Some(fail_at),
            ..Counting::default()
        };
        assert_eq!(
            line_continues(b"# c \\\\", &budget),
            Err(SourceError::Cancelled)
        );
        assert_eq!(budget.charged(), (0, fail_at));
        let budget = Counting {
            fail_at: Some(fail_at),
            ..Counting::default()
        };
        assert_eq!(
            unescaped_line_ends(b" \\\n ", false, 1, &budget),
            Err(SourceError::Cancelled)
        );
        assert_eq!(budget.charged(), (0, fail_at));
    }
    let budget = Counting {
        fail_at: Some(5),
        ..Counting::default()
    };
    assert_eq!(ruff_row(b"#ruff", &budget), Err(SourceError::Cancelled));
    // The spelling of a row ends before the CR of a CRLF. A CR alone is content.
    let ends: [(&[u8], usize, usize); 4] = [
        (b"# a\r\n", 4, 3),
        (b"# a\n", 3, 3),
        (b"# a\r", 4, 4),
        (b"# a\rb\n", 5, 5),
    ];
    for (code, end, spelling) in ends {
        assert_eq!(comment_spelling_end(code, 0, end), spelling);
    }
    // The same on a parse: the comment token of a CRLF row holds the CR, and the
    // spelling of the row ends before it.
    let rows: &[u8] = b"# a\r\n# b\n";
    let parser = parsed(rows);
    let root = parser.ts_tree().root_node();
    let spellings: Vec<_> = root
        .children(&mut root.walk())
        .filter(|node| node.kind() == "comment")
        .map(|node| {
            let end = comment_spelling_end(rows, node.start_byte(), node.end_byte());
            (node.byte_range(), end)
        })
        .collect();
    assert_eq!(spellings, [(0..4, 3), (5..8, 8)]);
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_body_reader_charges_one_step_for_each_child_and_the_bytes_it_reads() {
    // Per call: the source, the steps and bytes of `body_rows`, the rows it finds, and
    // the steps and bytes of `read_procedure`.
    let check = |source: &[u8], pass: (usize, usize), members: &[Range<usize>], header| {
        let name = source.escape_ascii();
        let parser = parsed(source);
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let procedure = parser
            .root_node()
            .preorder()
            .map(|node| node.as_tree_sitter())
            .find(|node| node.kind() == "procedure")
            .unwrap();
        let id = index.canonical.lookup(procedure).unwrap();
        let budget = Counting::default();
        let (slots, standing) = read_procedure(&index, id, &mut CommandScan::default(), &budget)
            .unwrap()
            .unwrap();
        assert_eq!(standing, Standing::Owner, "{name}");
        assert_eq!(budget.charged(), header, "{name}");
        let budget = Counting::default();
        let rows = body_rows(&index, slots.body, &budget).unwrap();
        assert_eq!(budget.charged(), pass, "{name}");
        assert_eq!(
            rows.members
                .iter()
                .map(|&(comment, end)| {
                    index.canonical.nodes[comment.0]
                        .evidence
                        .node()
                        .start_byte()..end
                })
                .collect::<Vec<_>>(),
            members,
            "{name}"
        );
        assert!(
            rows.directives.is_empty() && rows.obscured.is_none(),
            "{name}"
        );
        // A charge of several bytes can pass a count, so the failing charge is the first
        // that reaches it.
        let limited = |fail_at| Counting {
            fail_at: Some(fail_at),
            ..Counting::default()
        };
        let total = pass.0 + pass.1;
        for fail_at in 1..=total {
            assert!(
                matches!(
                    body_rows(&index, slots.body, &limited(fail_at)),
                    Err(SourceError::Cancelled)
                ),
                "{name} {fail_at}"
            );
        }
        assert!(
            body_rows(&index, slots.body, &limited(total + 1)).is_ok(),
            "{name}"
        );
        let (steps, bytes): (usize, usize) = header;
        for fail_at in 1..=steps + bytes {
            assert!(
                matches!(
                    read_procedure(&index, id, &mut CommandScan::default(), &limited(fail_at)),
                    Err(SourceError::Cancelled)
                ),
                "{name} {fail_at}"
            );
        }
        assert!(
            read_procedure(
                &index,
                id,
                &mut CommandScan::default(),
                &limited(steps + bytes + 1)
            )
            .is_ok(),
            "{name}"
        );
    };
    // expected: children `# audit`, line end, `return`, line end: 4 steps. The last byte
    // of the comment and of the command, and the 7 bytes of the row for the marker test:
    // 9 bytes. `read_procedure`: the coverage of the procedure is 1 step with one
    // included range. Rule B25 takes 1 step for each of the two braced words and reads
    // each from its first byte to its last, 2 and 18 bytes. The scan takes 2 steps for
    // the included bytes, one for the search among one range and one for that range, and
    // reads the source once, 28 bytes: 5 steps, 48 bytes.
    check(
        b"proc f {} {\n# audit\nreturn\n}",
        (4, 9),
        &[12..19],
        (5, 48),
    );
    // expected: 3 comments, `set x 1` and 4 line ends: 8 steps. Each comment and the
    // command reads its last byte: 4. The second and the third comment read the line
    // end before them: 2. The marker test reads the rows of 7, 5 and 7 bytes: 19.
    // 25 bytes. `read_procedure`: as above with a body of 33 bytes and a source of 43:
    // 5 steps, 78 bytes.
    check(
        b"proc f {} {\n# first\nset x 1\n#ruff\n# later\n}",
        (8, 25),
        &[12..19, 28..33, 34..41],
        (5, 78),
    );
    // expected: 2 comments and 2 line-end tokens, of which the first covers the blank
    // row: 4 steps. Last bytes: 2. Marker tests: 7 + 8. The gap `\n\n` before the second
    // comment, read to the line end that ends the blank row: 2. 19 bytes.
    // `read_procedure`: a body of 21 bytes, and the scan reads all 32 bytes, the last
    // of which ends the command: 5 steps, 55 bytes.
    check(
        b"proc f {} {\n# first\n\n# second\n}\n",
        (4, 19),
        &[12..19],
        (5, 55),
    );
    // expected: 2 comments, 2 line ends: 4 steps. The first row ends in one backslash:
    // 2 bytes. The second: 1, and the line end before it: 1. The marker test reads the
    // first row only, 5 bytes, because the second row is the rest of the first: 9 bytes.
    // `read_procedure`: a body of 13 bytes and a source of 24: 5 steps, 39 bytes.
    check(
        b"proc f {} {\n# a \\\n# b\n}\n",
        (4, 9),
        &[12..17, 18..21],
        (5, 39),
    );
    // expected: the comment and one line end: 2 steps. The comment's last byte and the
    // 7 bytes of the row: 8 bytes. `read_procedure`: a body of 11 bytes; the scan reads
    // the comment above the procedure as well, 26 bytes in all: 5 steps, 39 bytes.
    check(
        b"# c\nproc g {} {\n# inner\n}\n",
        (2, 8),
        &[16..23],
        (5, 39),
    );
    // expected: the body as above with a row of 3 bytes: 2 steps, 4 bytes.
    // `read_procedure`: a body of 7 bytes and a source of 20: 5 steps, 29 bytes.
    check(b"\\\nproc f {} {\n# a\n}\n", (2, 4), &[14..17], (5, 29));
    // expected: the body with a row of 5 bytes: 2 steps, 6 bytes. `read_procedure`: a
    // body of 9 bytes and a source of 30: 5 steps, 41 bytes.
    check(
        b"set x 1 ; proc g {} {\n# doc\n}\n",
        (2, 6),
        &[22..27],
        (5, 41),
    );
    // expected: the same body. `read_procedure`: the scan keeps one entry for the
    // bracket and one for the array index of the command above, a step each; a body of
    // 9 bytes and a source of 39: 7 steps, 50 bytes.
    check(
        b"set s [list $a(1)]\nproc g {} {\n# doc\n}\n",
        (2, 6),
        &[31..36],
        (7, 50),
    );
}

/// `count` procedures, each with one row of documentation.
#[cfg(feature = "tcl")]
fn many(count: usize) -> Vec<u8> {
    let mut source = String::new();
    for at in 0..count {
        writeln!(source, "proc p{at} {{}} {{\n# doc {at}\nreturn\n}}").unwrap();
    }
    source.into_bytes()
}

/// `count` procedures on one row, with a `;` between two of them.
#[cfg(feature = "tcl")]
fn row(count: usize) -> Vec<u8> {
    let procedures: Vec<_> = (0..count)
        .map(|at| format!("proc p{at} {{}} {{}}"))
        .collect();
    let mut source = procedures.join(";");
    source.push('\n');
    source.into_bytes()
}

/// One procedure with a lead row, `count` commands and a marked block of one row.
#[cfg(feature = "tcl")]
fn wide(count: usize) -> Vec<u8> {
    let mut source = String::from("proc f {} {\n# lead\n");
    for at in 0..count {
        writeln!(source, "set v{at} {at}").unwrap();
    }
    source.push_str("#ruff tail\n}\n");
    source.into_bytes()
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_procedures_and_rows_cost_linear_work() {
    for count in [64, 512, 2048] {
        let parser = parsed(&many(count));
        assert!(!parser.root_node().has_error());
        let nodes = parser.root_node().preorder().count();
        // expected: the root; per procedure the node, `proc`, the name, `arguments` with
        // two braces, the body with two braces, the comment, the `return` command with
        // its word, two line ends in the body and one after it: 15.
        assert_eq!(nodes, 15 * count + 1);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        // expected: one visit and one child batch for each node.
        assert_eq!(after.visits - before.visits, nodes);
        assert_eq!(after.child_batches - before.child_batches, nodes);
        // expected: the file and each procedure are one owner each.
        assert_eq!(
            after.owner_projections - before.owner_projections,
            count + 1
        );
        // expected: the reader asks the parser for no field name.
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        // expected: one row and so one source intersection for each document, and one
        // for the included bytes that the scan reads, with one comparison each, because
        // the parse has one included range.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            count + 1
        );
        // expected: one text, one document and one resolved association for each
        // procedure, and no assessment.
        assert_eq!(after.renders - before.renders, count);
        assert_eq!(index.documents().len(), count);
        assert_eq!(index.associations().len(), count);
        assert_eq!(index.assessments().len(), 0);
        let last = index.documents().last().unwrap();
        // expected: the row of the last procedure without `# `.
        assert_eq!(
            index.complete_text(last).unwrap().unwrap().text(),
            format!("doc {}", count - 1)
        );
    }
    for count in [64, 512, 2048] {
        let parser = parsed(&wide(count));
        assert!(!parser.root_node().has_error());
        let nodes = parser.root_node().preorder().count();
        // expected: the root, the procedure with its 9 fixed nodes (`proc`, name,
        // `arguments` and two braces, body and two braces, the line end after it), the
        // two comments with a line end each, and per command the `set` node, the
        // keyword, the name, the value and the line end: 5.
        assert_eq!(nodes, 5 * count + 15);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        // expected: one visit for each node; the file and the procedure are the two
        // owners; no field name is requested.
        assert_eq!(after.visits - before.visits, nodes);
        assert_eq!(after.owner_projections - before.owner_projections, 2);
        assert_eq!(after.field_requests - before.field_requests, 0);
        // expected: one intersection for each of the two rows, and one for the scan.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            3
        );
        // expected: one document, so one text.
        assert_eq!(after.renders - before.renders, 1);
        let document = index.documents().next().unwrap();
        // expected: its two rows are `# lead` and `#ruff tail`, joined by a line feed.
        assert_eq!(document.get().fragment_count(), 2);
        assert_eq!(
            index.complete_text(document).unwrap().unwrap().text(),
            "lead\ntail"
        );
    }
    for count in [64, 512, 2048] {
        let source = row(count);
        let parser = parsed(&source);
        assert!(!parser.root_node().has_error());
        let nodes = parser.root_node().preorder().count();
        // expected: the root and the line end; per procedure the node, `proc`, the name,
        // `arguments` with two braces and the body with two braces: 9; and a `;` between
        // two procedures.
        assert_eq!(nodes, 10 * count + 1);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        // expected: one visit for each node; the file and each procedure are one owner
        // each.
        assert_eq!(after.visits - before.visits, nodes);
        assert_eq!(
            after.owner_projections - before.owner_projections,
            count + 1
        );
        // expected: no comment, so the only bytes a scan reads are those of the owner
        // test. Rule B25 reads the two words `{}` of each procedure: 4. The scan reads
        // every byte of the source once, however many procedures stand on the row. A
        // scan that started anew for each procedure would read the row up to that
        // procedure once more for each of them.
        assert_eq!(
            after.classifier_bytes - before.classifier_bytes,
            4 * count + source.len()
        );
        // expected: the bodies are empty, so no document; every procedure is an owner,
        // so no assessment.
        assert_eq!(index.documents().len(), 0);
        assert_eq!(index.assessments().len(), 0);
    }
}

/// `count` procedures whose body Tcl ends in the comment row after the node.
#[cfg(feature = "tcl")]
fn late(count: usize) -> Vec<u8> {
    let mut source = String::new();
    for at in 0..count {
        writeln!(source, "proc p{at} {{}} {{\n# {{\n}}\n# }}").unwrap();
    }
    source.into_bytes()
}

/// A command with brackets nested `count` deep, and one documented procedure below it.
#[cfg(feature = "tcl")]
fn deep(count: usize) -> Vec<u8> {
    let mut source = String::from("set x ");
    source.push_str(&"[".repeat(count));
    source.push('a');
    source.push_str(&"]".repeat(count));
    source.push_str("\nproc g {} {\n# doc\n}\n");
    source.into_bytes()
}

/// What the scan answers for every byte of a source of which `ranges` are included,
/// asked in order: the commands of the file's own level, the bytes at which excluded
/// bytes hide the answer, and what was charged.
#[cfg(feature = "tcl")]
fn scanned(source: &[u8], ranges: &[Range<usize>]) -> (Vec<Command>, Vec<usize>, (usize, usize)) {
    let tree = restricted(source, ranges);
    let context = SourceContext::new(&tree, source);
    let (mut scan, budget) = (CommandScan::default(), Counting::default());
    let (mut commands, mut unknown) = (Vec::new(), Vec::new());
    for at in 0..source.len() {
        match scan.command_at(&context, at, &budget).unwrap() {
            Listed::Command(command) => {
                assert_eq!(command.start, at);
                commands.push(command);
            }
            Listed::Unknown => unknown.push(at),
            Listed::Nothing => {}
        }
    }
    (commands, unknown, budget.charged())
}

/// A command as a table states it: its word count, where five stands for five or more,
/// whether a word has the expansion prefix, and its first four words.
#[cfg(feature = "tcl")]
fn command(count: u8, expands: bool, spans: &[(usize, usize)]) -> Command {
    let mut words = [(0, 0); 4];
    words[..spans.len()].copy_from_slice(spans);
    Command {
        start: spans[0].0,
        words,
        count,
        expands,
    }
}

#[cfg(feature = "tcl")]
type Listing = &'static [(u8, bool, &'static [(usize, usize)])];

/// A source, its included ranges, the commands the scan lists, and the bytes at which
/// excluded bytes hide the answer.
#[cfg(feature = "tcl")]
type Hidden = (
    &'static [u8],
    &'static [Range<usize>],
    Listing,
    &'static [usize],
);

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_scan_lists_the_commands_of_a_file_as_tcl_reads_them() {
    // A source and the commands of its own level, in order. A source whose last command
    // has a parse error lists the commands before it.
    let sources: [(&[u8], Listing); 63] = [
        // A line end ends a command.
        (
            b"a b\nc",
            &[(2, false, &[(0, 1), (2, 3)]), (1, false, &[(4, 5)])],
        ),
        // And so does `;`.
        (
            b"a;b ;c",
            &[
                (1, false, &[(0, 1)]),
                (1, false, &[(2, 3)]),
                (1, false, &[(5, 6)]),
            ],
        ),
        // A comment where a command starts.
        (b"# c\na", &[(1, false, &[(4, 5)])]),
        // A `#` in a command is a word.
        (
            b"a # b\nc",
            &[(3, false, &[(0, 1), (2, 3), (4, 5)]), (1, false, &[(6, 7)])],
        ),
        // A comment after `;`.
        (
            b"a ;# c\nb",
            &[(1, false, &[(0, 1)]), (1, false, &[(7, 8)])],
        ),
        // A backslash continues a comment.
        (b"# c \\\na\nb", &[(1, false, &[(8, 9)])]),
        // Two backslashes do not.
        (b"# c \\\\\na", &[(1, false, &[(7, 8)])]),
        // More than four words.
        (
            b"a b c d e f",
            &[(5, false, &[(0, 1), (2, 3), (4, 5), (6, 7)])],
        ),
        // A vertical tab, a form feed and a tab separate words.
        (
            b"a\x0bb\x0cc\td",
            &[(4, false, &[(0, 1), (2, 3), (4, 5), (6, 7)])],
        ),
        // A backslash and a line end separate words.
        (
            b"a \\\nb\nc",
            &[(2, false, &[(0, 1), (4, 5)]), (1, false, &[(6, 7)])],
        ),
        // Also directly after a word.
        (b"a\\\nb", &[(2, false, &[(0, 1), (3, 4)])]),
        // And they are white space before a command.
        (b"\\\na", &[(1, false, &[(2, 3)])]),
        // Any other backslash starts a word.
        (b"\\a b", &[(2, false, &[(0, 2), (3, 4)])]),
        // And hides a blank in a word.
        (b"a\\ b c", &[(2, false, &[(0, 4), (5, 6)])]),
        // A backslash at the end of the file is a word byte.
        (b"a\\", &[(1, false, &[(0, 2)])]),
        // A carriage return alone ends a line.
        (b"a\rb", &[(1, false, &[(0, 1)]), (1, false, &[(2, 3)])]),
        // CR LF is one line end.
        (b"a\r\nb", &[(1, false, &[(0, 1)]), (1, false, &[(3, 4)])]),
        // A carriage return ends a comment.
        (b"# c\ra", &[(1, false, &[(4, 5)])]),
        // A backslash and CR LF are white space.
        (b"a \\\r\nb", &[(2, false, &[(0, 1), (5, 6)])]),
        // The byte 0x1a ends the file.
        (b"a\x1ab", &[(1, false, &[(0, 1)])]),
        // A byte order mark is part of the first word.
        (b"\xef\xbb\xbfa b", &[(2, false, &[(0, 4), (5, 6)])]),
        // A braced word.
        (b"a {b c} d", &[(3, false, &[(0, 1), (2, 7), (8, 9)])]),
        // Braces nest.
        (
            b"a {b {c} d} e",
            &[(3, false, &[(0, 1), (2, 11), (12, 13)])],
        ),
        // A backslash hides a brace.
        (
            b"a {b \\} c} d",
            &[(3, false, &[(0, 1), (2, 10), (11, 12)])],
        ),
        // A line end in a braced word ends nothing.
        (
            b"a {b\nc}\nd",
            &[(2, false, &[(0, 1), (2, 7)]), (1, false, &[(8, 9)])],
        ),
        // A comment is not special in a braced word.
        (
            b"a {# }\nb",
            &[(2, false, &[(0, 1), (2, 6)]), (1, false, &[(7, 8)])],
        ),
        // Nor is a quote: the word ends at the first brace.
        (b"a {\"}\"} b", &[]),
        // A backslash and a line end after a brace separate words.
        (b"a {b}\\\nc", &[(3, false, &[(0, 1), (2, 5), (7, 8)])]),
        // Any other byte after a close-brace is a parse error.
        (b"a {b}c\nd", &[]),
        // A brace that never closes is a parse error.
        (b"a {b\nc", &[]),
        // A quoted word.
        (b"a \"b c\" d", &[(3, false, &[(0, 1), (2, 7), (8, 9)])]),
        // A line end in a quoted word ends nothing.
        (
            b"a \"b\nc\"\nd",
            &[(2, false, &[(0, 1), (2, 7)]), (1, false, &[(8, 9)])],
        ),
        // A backslash hides a quote.
        (
            b"a \"b \\\" c\" d",
            &[(3, false, &[(0, 1), (2, 10), (11, 12)])],
        ),
        // A byte after a close-quote is a parse error.
        (b"a \"b\"c\nd", &[]),
        // A quote in a bare word is a word byte.
        (b"a b\"c d\"", &[(3, false, &[(0, 1), (2, 5), (6, 8)])]),
        // A quote that never closes is a parse error.
        (b"a \"b", &[]),
        // A bracket holds a script.
        (b"a [b c] d", &[(3, false, &[(0, 1), (2, 7), (8, 9)])]),
        // A line end in it ends a command of that script only.
        (b"a [b\nc] d", &[(3, false, &[(0, 1), (2, 7), (8, 9)])]),
        // A comment in it hides a bracket.
        (
            b"a [b ;# c ]\n] d",
            &[(3, false, &[(0, 1), (2, 13), (14, 15)])],
        ),
        // A bracket in the middle of a word.
        (b"a x[b]y z", &[(3, false, &[(0, 1), (2, 7), (8, 9)])]),
        // A bracket in a quoted word, with a quoted word in it.
        (
            b"a \"b [c \"d\"] e\" f",
            &[(3, false, &[(0, 1), (2, 15), (16, 17)])],
        ),
        // A braced word in a bracket hides a bracket.
        (
            b"a [b {c ]} d] e",
            &[(3, false, &[(0, 1), (2, 13), (14, 15)])],
        ),
        // A word that starts with a bracket.
        (b"[a]b c", &[(2, false, &[(0, 4), (5, 6)])]),
        // A close-bracket outside a bracket is a word byte.
        (b"a]b [c]]", &[(2, false, &[(0, 3), (4, 8)])]),
        // A bracket that never closes is a parse error.
        (b"a [b\nc", &[]),
        // An array index holds white space.
        (b"a $a(c d) e", &[(3, false, &[(0, 1), (2, 9), (10, 11)])]),
        // And line ends.
        (b"a $a(c\nd) e", &[(3, false, &[(0, 1), (2, 9), (10, 11)])]),
        // An index after no name.
        (b"a $(c d) e", &[(3, false, &[(0, 1), (2, 8), (9, 10)])]),
        // One colon ends the name: no index.
        (b"a $a:(c d)", &[(3, false, &[(0, 1), (2, 7), (8, 10)])]),
        // Two colons are part of the name.
        (
            b"a $a::(c d) e",
            &[(3, false, &[(0, 1), (2, 11), (12, 13)])],
        ),
        // A bracket in an index.
        (
            b"a $a(c [d e] f) g",
            &[(3, false, &[(0, 1), (2, 15), (16, 17)])],
        ),
        // An index in an index.
        (
            b"a $a(c $a(d e) f) g",
            &[(3, false, &[(0, 1), (2, 17), (18, 19)])],
        ),
        // A braced variable name reads to the next close-brace.
        (b"a ${v w} d", &[(3, false, &[(0, 1), (2, 8), (9, 10)])]),
        // A brace after a name is a word byte.
        (b"a $v{b c}", &[(3, false, &[(0, 1), (2, 6), (7, 9)])]),
        // An index that never closes is a parse error.
        (b"a $a(c", &[]),
        // And so is a braced name.
        (b"a ${v", &[]),
        // The expansion prefix before a bare word.
        (b"{*}a b", &[(2, true, &[(0, 4), (5, 6)])]),
        // Before a braced word.
        (b"a {*}{b c} d", &[(3, true, &[(0, 1), (2, 10), (11, 12)])]),
        // Before a quoted word.
        (
            b"a {*}\"b c\" d",
            &[(3, true, &[(0, 1), (2, 10), (11, 12)])],
        ),
        // `{*}` before white space is a word of its own.
        (b"a {*} b", &[(3, false, &[(0, 1), (2, 5), (6, 7)])]),
        // Also before a backslash and a line end.
        (b"a {*}\\\nb", &[(3, false, &[(0, 1), (2, 5), (7, 8)])]),
        // A second prefix is a braced word, and a byte after it a parse error.
        (b"a {*}{*}b", &[]),
        // No other braced word is a prefix.
        (b"a {**}b", &[]),
    ];
    for (source, listing) in sources {
        let name = source.escape_ascii();
        let expected: Vec<_> = listing
            .iter()
            .map(|&(count, expands, spans)| command(count, expands, spans))
            .collect();
        let (commands, unknown, _) = scanned(source, &[0..source.len()]);
        assert_eq!(commands, expected, "{name}");
        assert!(unknown.is_empty(), "{name}");
    }
    // What the scan charges: steps and bytes.
    let costs: [(&[u8], (usize, usize)); 9] = [
        // expected: the included bytes are found with one step for the search among one
        // range and one step for that range. Every byte is read once.
        (b"a b\nc", (2, 5)),
        // expected: one more step for each bracket and each array index that opens.
        (b"a [b c] d", (3, 9)),
        (b"a [b {c ]} d] e", (3, 15)),
        (b"a \"b [c \"d\"] e\" f", (3, 17)),
        (b"a $a(c $a(d e) f) g", (4, 19)),
        (b"a $a(c [d e] f) g", (4, 17)),
        // expected: the scan stops at the byte that ends the file, and at the byte after
        // a close-brace that is a parse error. A word that never closes is read to the
        // end of the source.
        (b"a\x1ab", (2, 2)),
        (b"a {b}c\nd", (2, 6)),
        (b"a {b\nc", (2, 6)),
    ];
    for (source, charged) in costs {
        let name = source.escape_ascii();
        assert_eq!(scanned(source, &[0..source.len()]).2, charged, "{name}");
        // Every charge is observed: the scan ends cancelled for every count below the
        // total, and reads on with one more.
        let tree = restricted(source, &[0..source.len()]);
        let context = SourceContext::new(&tree, source);
        let ask = |fail_at: usize| {
            let budget = Counting {
                fail_at: Some(fail_at),
                ..Counting::default()
            };
            let mut scan = CommandScan::default();
            (0..source.len()).try_for_each(|at| scan.command_at(&context, at, &budget).map(drop))
        };
        for fail_at in 1..=charged.0 + charged.1 {
            assert_eq!(
                ask(fail_at),
                Err(SourceError::Cancelled),
                "{name} {fail_at}"
            );
        }
        assert_eq!(ask(charged.0 + charged.1 + 1), Ok(()), "{name}");
    }
    // Excluded bytes: a source, its included ranges, the commands the scan lists, and
    // the bytes at which the answer is not known.
    let hidden: [Hidden; 10] = [
        // expected: the command `a` reaches the excluded byte 1 before it ends. The state
        // is known again after the line end at 3, which the included byte `b` before it
        // proves unescaped.
        (
            b"a b\nc d",
            &[0..1, 2..7],
            &[(2, false, &[(4, 5), (6, 7)])],
            &[0, 1, 2, 3],
        ),
        // expected: a brace is open at the excluded byte 4, so the state is never known
        // again. The bytes 1 to 3 stand in the command and are known.
        (b"a {b c}\nd", &[0..4, 5..9], &[], &[0, 4, 5, 6, 7, 8]),
        // expected: a brace is open at the excluded byte 7 (`Dead`), so the 0x1a byte at
        // 11 does not restore known state: byte 13 (`proc`) stays `Listed::Unknown`.
        (
            b"set x {ab}\n\x1a\nproc f {} {\n# doc\n}\n",
            &[0..7, 8..33],
            &[],
            &[
                0, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
                28, 29, 30, 31, 32,
            ],
        ),
        // expected: the source starts with an excluded byte; the line end at 2 is proven.
        (b"xa\nb", &[1..4], &[(1, false, &[(3, 4)])], &[0, 1, 2]),
        // expected: the byte before the line end at 1 is excluded, and may be a
        // backslash. The line end at 3 is proven.
        (b"x\na\nb", &[1..5], &[(1, false, &[(4, 5)])], &[0, 1, 2, 3]),
        // expected: one backslash stands before the line end at 3: it is escaped. The
        // line end at 5 is proven.
        (
            b"x \\\na\nb",
            &[1..7],
            &[(1, false, &[(6, 7)])],
            &[0, 1, 2, 3, 4, 5],
        ),
        // expected: the carriage return at 1 has an excluded byte before it. The one at
        // 3 is a proven line end, and an included byte that is no line feed follows it.
        (
            b"x\ra\rb c",
            &[1..7],
            &[(2, false, &[(4, 5), (6, 7)])],
            &[0, 1, 2, 3],
        ),
        // expected: the line feed after the carriage return at 2 is excluded, so the
        // pair is no proven line end. The line end at 5 is.
        (
            b"ab\r\nc\nd",
            &[1..3, 4..7],
            &[(1, false, &[(6, 7)])],
            &[0, 1, 2, 3, 4, 5],
        ),
        // expected: the byte 0x1a ends the file whatever the state. Nothing is listed
        // after it, and the bytes after it are not unknown.
        (b"x\x1a\na", &[1..4], &[], &[0, 1]),
        // expected: `a b` has ended before the excluded byte 5. The command `c` reaches
        // it, and no line end follows.
        (
            b"a b\nc d",
            &[0..5, 6..7],
            &[(2, false, &[(0, 1), (2, 3)])],
            &[4, 5, 6],
        ),
    ];
    for (source, ranges, listing, unknown) in hidden {
        let expected: Vec<_> = listing
            .iter()
            .map(|&(count, expands, spans)| command(count, expands, spans))
            .collect();
        let (commands, bytes, _) = scanned(source, ranges);
        assert_eq!(
            (commands, bytes.as_slice()),
            (expected, unknown),
            "{} {ranges:?}",
            source.escape_ascii()
        );
    }
    // A question about a byte that the scan has passed reads nothing. Only the command
    // that started last is still known.
    let source: &[u8] = b"a b\nc";
    let tree = restricted(source, &[0..5]);
    let context = SourceContext::new(&tree, source);
    let (mut scan, budget) = (CommandScan::default(), Counting::default());
    let last = scan.command_at(&context, 4, &budget);
    assert_eq!(last, Ok(Listed::Command(command(1, false, &[(4, 5)]))));
    let charged = budget.charged();
    assert_eq!(scan.command_at(&context, 0, &budget), Ok(Listed::Nothing));
    assert_eq!(scan.command_at(&context, 4, &budget), last);
    assert_eq!(budget.charged(), charged);
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_owner_test_reads_the_file_once_however_many_procedures_it_holds() {
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    // The standing of each procedure of the root and the charges of all of them. With
    // `shared` one scan answers in source order, as the collection keeps it; without, a
    // new scan answers for every procedure.
    let pass = |source: &[u8], shared: bool| {
        let parser = parsed(source);
        let index = parser.documentation(&context, &options).unwrap();
        let root = parser.ts_tree().root_node();
        let (mut scan, budget) = (CommandScan::default(), Counting::default());
        let standings: Vec<_> = root
            .children(&mut root.walk())
            .filter(|node| node.kind() == "procedure")
            .map(|node| {
                if !shared {
                    scan = CommandScan::default();
                }
                let id = index.canonical.lookup(node).unwrap();
                read_procedure(&index, id, &mut scan, &budget)
                    .unwrap()
                    .unwrap()
                    .1
            })
            .collect();
        (standings, budget.charged())
    };
    let all = |standings: &[Standing], count: usize, standing: &Standing| {
        standings.len() == count && standings.iter().all(|found| found == standing)
    };
    let boundary = Standing::Uncertain(UnresolvedReason::GrammarBoundary);
    for count in [8, 64, 512] {
        // Many procedures on one row.
        let source = row(count);
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, count, &Standing::Owner));
        // expected: per procedure 1 step for its coverage and 1 for each of its two
        // braced words, which are `{}` twice: 3 steps, 4 bytes. The scan: 2 steps for
        // the included bytes, and every byte of the source once.
        assert_eq!(charged, (3 * count + 2, 4 * count + source.len()));
        let ends: usize = source
            .iter()
            .enumerate()
            .filter(|(_, byte)| matches!(byte, b';' | b'\n'))
            .map(|(at, _)| at + 1)
            .sum();
        // expected: a new scan for every procedure takes its 2 steps each time and reads
        // the source from its start to the `;` or the line end after that procedure: a
        // quadratic sum.
        assert_eq!(pass(&source, false).1, (5 * count, 4 * count + ends));
        // Many procedures on many rows.
        let source = many(count);
        let outside: usize = (0..count).map(|at| format!("proc p{at} ").len() + 2).sum();
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, count, &Standing::Owner));
        // expected: the same steps. Rule B25 reads all of the source but `outside`: per
        // procedure `proc pN `, the space between its two braced words and the line end
        // after it. The scan reads every byte once.
        assert_eq!(charged, (3 * count + 2, 2 * source.len() - outside));
        // One long body.
        let source = wide(count);
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, 1, &Standing::Owner));
        // expected: the 3 steps of one procedure and the 2 of the scan, whatever the
        // body holds. Rule B25 reads all of the source but `proc f `, the space between
        // the two words and the last line end.
        assert_eq!(charged, (5, 2 * source.len() - 9));
        // Many bodies that Tcl ends elsewhere than their nodes end.
        let source = late(count);
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, count, &Standing::Parted));
        // expected: 3 steps per procedure. Rule B25 reads `{}` and the 7 bytes of the
        // body node, at whose end the count has not returned to zero: it reads no byte
        // behind the node. The scan is not asked for such a procedure.
        assert_eq!(charged, (3 * count, 9 * count));
        // A parse error of Tcl before all of them.
        let mut source = b"set s {a}b\n".to_vec();
        source.extend(many(count));
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, count, &boundary));
        // expected: the scan stops at the byte after the close-brace, the tenth byte of
        // the source, and reads nothing behind it. Rule B25 reads as for `many`.
        assert_eq!(charged, (3 * count + 2, many(count).len() - outside + 10));
        // All of them in a braced word that the grammar does not see: for Tcl the
        // carriage return ends the comment.
        let mut source = b"# c\rset s {\n".to_vec();
        source.extend(many(count));
        source.extend(b"}\n");
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, count, &boundary));
        let last = source.windows(4).rposition(|word| word == b"proc").unwrap();
        // expected: the scan reads up to the first byte of the last procedure and no
        // further: no command starts there. Rule B25 reads as for `many`.
        assert_eq!(
            charged,
            (3 * count + 2, many(count).len() - outside + last + 1)
        );
        // Brackets nested `count` deep in the command above one procedure.
        let source = deep(count);
        let (standings, charged) = pass(&source, true);
        assert!(all(&standings, 1, &Standing::Owner));
        // expected: one step for each bracket that opens, beside the 5 of one
        // procedure. Rule B25 reads `{}` and the body of 9 bytes.
        assert_eq!(charged, (count + 5, source.len() + 11));
    }
}

/// Cancels one collection at its first checkpoint, at its second, and so on, until
/// `past` holds for the counters read around a cancelled run or no checkpoint is left.
/// Every run must end cancelled. The result is the number of runs for which `inside`
/// holds.
#[cfg(feature = "tcl")]
fn cancelled_where(
    parser: &AnyParser,
    options: DocumentationOptions,
    inside: impl Fn(&Observations, &Observations) -> bool,
    past: impl Fn(&Observations, &Observations) -> bool,
) -> usize {
    let context = DocumentationContext::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut count = 0;
    for checkpoint in 1..=polls {
        let before = observations();
        let result = cancel_after(checkpoint, || parser.documentation(&context, &options));
        let after = observations();
        assert!(
            matches!(result, Err(CollectionError::Cancelled)),
            "{checkpoint}"
        );
        count += usize::from(inside(&before, &after));
        if past(&before, &after) {
            break;
        }
    }
    assert!(cancel_after(polls + 1, || parser.documentation(&context, &options)).is_ok());
    count
}

/// One procedure whose body is `count` times that row.
#[cfg(feature = "tcl")]
fn tall(count: usize, row: &str) -> Vec<u8> {
    let mut source = String::from("proc f {} {\n");
    for _ in 0..count {
        writeln!(source, "{row}").unwrap();
    }
    source.push_str("}\n");
    source.into_bytes()
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_collection_observes_cancellation_in_its_loops() {
    let parser = parsed(&wide(2048));
    let nodes = parser.root_node().preorder().count();
    // The walk leaves the procedure before it enters the line end after it, the last
    // node. Between the procedure's owner record and its first row fragment only the
    // body pass runs. The one intersection before it is that of the scan.
    let pass = cancelled_where(
        &parser,
        DocumentationOptions::default(),
        |before, after| {
            after.visits - before.visits == nodes - 1
                && after.owner_projections - before.owner_projections == 2
                && after.intersection_searches - before.intersection_searches == 1
        },
        |_, _| false,
    );
    // expected: the body has 2 * 2048 + 4 children between its braces, and the pass
    // charges one step for each. A checkpoint falls on every 1024th step, so 4096 steps
    // hold exactly 4; the 4 other steps and the few before the first search can hold
    // one more. A pass that charged more for a later child would hold more.
    assert!((4..=5).contains(&pass), "{pass}");
    // The owner test: every node but the line end after the procedure is entered, and
    // the procedure is no owner yet.
    let before_owner = |source: &[u8]| {
        let parser = parsed(source);
        let nodes = parser.root_node().preorder().count();
        cancelled_where(
            &parser,
            DocumentationOptions::default(),
            |before, after| {
                after.visits - before.visits == nodes - 1
                    && after.owner_projections - before.owner_projections == 1
            },
            |_, _| false,
        )
    };
    // The scan over one long word of an earlier command.
    let mut long = b"set s {".to_vec();
    long.extend(b"aaaaaaa\n".repeat(8192));
    long.extend(b"}\nproc g {} {\n# doc\n}\n");
    let scan = before_owner(&long);
    // expected: the source is 65,565 bytes, and a checkpoint falls on every 16,384th byte
    // that is read: 4. The 11 bytes of rule B25 and the bytes charged before can hold one
    // more. A scan that did not charge its bytes would hold none.
    assert!((4..=5).contains(&scan), "{scan}");
    // Rule B25 over one long body, and the scan over the same bytes.
    let body = before_owner(&tall(8192, "aaaaaaa"));
    // expected: the body is 65,539 bytes and the source 65,550. Rule B25 reads `{}` and
    // the body, the scan reads the source: 131,091 bytes hold exactly 8 checkpoints.
    // The bytes charged before can hold one more, and so can the few steps between the
    // two. Without the charge of either the count is at most 6.
    assert!((8..=10).contains(&body), "{body}");
    // The entries the scan keeps for open brackets.
    let stack = before_owner(&deep(2048));
    // expected: one step for each of the 2048 brackets: exactly 2 checkpoints, and the
    // steps charged before can hold one more. The source is too short for a checkpoint
    // of its bytes. A scan that did not charge the entries would hold at most one.
    assert!((2..=3).contains(&stack), "{stack}");
    // A fragment is one source intersection, so the loop over the rows runs while some
    // and not all of them are searched. The first intersection of a collection is that
    // of the scan, before the rows. The text of the rows has checkpoints of its own,
    // which come later and are not tried.
    let searched = |before: &Observations, after: &Observations| {
        (after.intersection_searches - before.intersection_searches).saturating_sub(1)
    };
    let rows =
        |before: &Observations, after: &Observations| (1..2048).contains(&searched(before, after));
    let done = |before: &Observations, after: &Observations| searched(before, after) == 2048;
    // expected: each row charges the loop's step and the two steps of its intersection:
    // 3 * 2048 charges hold exactly 6 checkpoints. At most one of them falls before the
    // first search or after the last.
    // A loop that charged more for a later row would hold more than 6.
    let parser = parsed(&tall(2048, "# row"));
    let members = cancelled_where(&parser, DocumentationOptions::default(), rows, done);
    assert!((5..=6).contains(&members), "{members}");
    let parser = parsed(&tall(2048, "#ruffopt a b"));
    let directives = cancelled_where(&parser, options(Retain::Directives), rows, done);
    assert!((5..=6).contains(&directives), "{directives}");
    // A collection after a cancelled one is whole.
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &options(Retain::Directives),
        )
        .unwrap();
    assert_eq!((index.documents().len(), index.evidence().len()), (0, 2048));
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_ruff_text_charges_a_step_and_the_bytes_it_reads_for_each_row() {
    use crate::documentation::text::{TextError, comments};
    use Charge::{Bytes, Step};
    // The rows of the one document of a source, rendered with a recording budget.
    let render = |source: &[u8], fail_at| {
        let parser = parsed(source);
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let document = index.documents().next().unwrap();
        let rows: Vec<_> = (0..document.get().fragment_count())
            .map(|at| index.facts().document_fragment(document, at).unwrap().get())
            .collect();
        let budget = Counting {
            fail_at,
            ..Counting::default()
        };
        let content = comments::render(Profile::Ruff, &index.source, &rows, &budget);
        (content.map(|_| ()), budget.made.into_inner())
    };
    // expected: before any text is built, each row charges one step, then one byte for
    // each backslash at its end and for the byte before them, then the bytes of the
    // marker test: the row, up to the nine bytes of `#ruffopt` and the byte after it.
    // The rows have 7, 11 and 6 bytes, and the last ends in two backslashes.
    let rows: &[u8] = b"proc f {} {\n# first\nset x 1\n#ruff later\n# a \\\\\n}\n";
    let read = [
        Step,
        Bytes(1),
        Bytes(7),
        Step,
        Bytes(1),
        Bytes(9),
        Step,
        Bytes(1),
        Bytes(1),
        Bytes(1),
        Bytes(6),
    ];
    let (content, made) = render(rows, None);
    assert!(content.is_ok());
    assert_eq!(made[..read.len()], read);
    // expected: the shared emission of line comments follows, with at least one step for
    // each of the three rows.
    let emitted = made[read.len()..]
        .iter()
        .filter(|charge| **charge == Step)
        .count();
    assert!(emitted >= 3, "{emitted}");
    // A charge fails when the steps and bytes charged so far reach the limit: the run
    // ends at that charge, and the charges before it are the ones above.
    let mut reached = 0;
    for (at, charge) in read.iter().enumerate() {
        let before = reached;
        reached += match charge {
            Step => 1,
            Bytes(count) => *count,
        };
        for fail_at in before + 1..=reached {
            let (content, made) = render(rows, Some(fail_at));
            assert!(matches!(
                content,
                Err(TextError::Source(SourceError::Cancelled))
            ));
            assert_eq!(made, read[..=at], "{fail_at}");
        }
    }
    // expected: one step, the backslash and the byte before it. The row continues, so
    // nothing else is read and no text is built.
    let (content, made) = render(b"proc f {} {\n# audit \\\nmore text\nreturn\n}\n", None);
    assert!(matches!(
        content,
        Err(TextError::Unavailable(UnavailableReason::GrammarBoundary))
    ));
    assert_eq!(made, [Step, Bytes(1), Bytes(1)]);
}

/// The children of a node as the cursor names them: field, kind, byte range, and
/// whether the parser invented the token.
#[cfg(feature = "tcl")]
fn children(node: tree_sitter::Node<'_>) -> Vec<String> {
    let mut cursor = node.walk();
    let mut lines = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            lines.push(format!(
                "{}{} {}..{}{}{}",
                cursor
                    .field_name()
                    .map(|field| format!("{field}: "))
                    .unwrap_or_default(),
                child.kind().escape_default(),
                child.start_byte(),
                child.end_byte(),
                if child.is_missing() { " missing" } else { "" },
                if child.has_error() && !child.is_missing() {
                    " has_error"
                } else {
                    ""
                },
            ));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    lines
}

#[cfg(feature = "tcl")]
#[test]
#[allow(clippy::too_many_lines)]
fn tcl_parse_shapes_the_ruff_reader_relies_on() {
    let shape = |source: &[u8], kind: &str, range: Range<usize>| {
        let parser = parsed(source);
        children(node_at(parser.ts_tree(), kind, range))
    };
    // Without a final line end the parser invents a zero-width one after the procedure.
    let primary: &[u8] = b"proc f {} {\n# audit\nreturn\n}";
    assert_eq!(
        shape(primary, "source_file", 0..28),
        ["procedure 0..28", "\\n 28..28 missing"]
    );
    assert_eq!(
        shape(primary, "procedure", 0..28),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..28"
        ]
    );
    assert_eq!(
        shape(primary, "braced_word", 10..28),
        [
            "{ 10..11",
            "comment 12..19",
            "\\n 19..20",
            "command 20..26",
            "\\n 26..27",
            "} 27..28"
        ]
    );
    // The comment token of a CRLF row holds the CR.
    let crlf: &[u8] = b"proc f {} {\r\n# audit\r\n#ruff\r\n# more\r\nreturn\r\n}\r\n";
    assert_eq!(
        shape(crlf, "braced_word", 10..46),
        [
            "{ 10..11",
            "comment 13..21",
            "\\n 21..22",
            "comment 22..28",
            "\\n 28..29",
            "comment 29..36",
            "\\n 36..37",
            "command 37..43",
            "\\n 44..45",
            "} 45..46"
        ]
    );
    // One line-end token covers a blank row. A row of spaces gives two tokens.
    assert_eq!(
        shape(
            b"proc f {} {\n# first\n\n# second\nreturn\n}\n",
            "braced_word",
            10..38
        )[1..4],
        ["comment 12..19", "\\n 19..21", "comment 21..29"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n   \n# b\nreturn\n}\n",
            "braced_word",
            10..32
        )[1..5],
        [
            "comment 12..15",
            "\\n 15..16",
            "\\n 19..20",
            "comment 20..23"
        ]
    );
    // A row that holds only a backslash is no node with LF line ends, and a command
    // with CRLF line ends (#1802). The row below a continued comment is a `procedure`
    // without any recovery node.
    let bare: &[u8] = b"# c \\\n\\\nproc g {} {\n# inner\n}\n";
    assert!(!parsed(bare).root_node().has_error());
    assert_eq!(
        shape(bare, "source_file", 0..30),
        ["comment 0..5", "\\n 5..6", "procedure 8..29", "\\n 29..30"]
    );
    let bare: &[u8] = b"# c \\\r\n\\\r\nproc g {} {\r\n# inner\r\n}\r\n";
    assert!(!parsed(bare).root_node().has_error());
    assert_eq!(
        shape(bare, "source_file", 0..35),
        [
            "comment 0..6",
            "\\n 6..7",
            "command 7..9",
            "\\n 9..10",
            "procedure 10..33",
            "\\n 34..35"
        ]
    );
    assert_eq!(
        shape(bare, "command", 7..9),
        ["name: escaped_character 7..9"]
    );
    // `proc f` and its other words on the next row are one clean `procedure`.
    let split: &[u8] = b"proc f\n{} {\n# audit\n}\nproc g {} {\n# doc g\n}\n";
    assert!(!parsed(split).root_node().has_error());
    assert_eq!(
        shape(split, "procedure", 0..21),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..21"
        ]
    );
    // A word after the body, or a second command on the row, follows an invented
    // zero-width line end.
    assert_eq!(
        shape(
            b"proc f {} {\n# audit\n} extra\nproc g {} {\n# doc g\n}\n",
            "source_file",
            0..50
        )[..3],
        ["procedure 0..21", "\\n 21..21 missing", "command 22..27"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n} proc g {} {\n# b\n}\n",
            "source_file",
            0..36
        )[..3],
        ["procedure 0..17", "\\n 17..17 missing", "procedure 18..35"]
    );
    assert_eq!(
        shape(b"proc f {} {\n# a\n}   ", "source_file", 0..20),
        ["procedure 0..17", "\\n 17..17 missing"]
    );
    // A `;` after a word is a recovery node or a missing token, though valid Tcl.
    assert_eq!(
        shape(
            b"proc f {} {\n# a\nreturn;# done\n#ruff b\n}\n",
            "braced_word",
            10..39
        )[1..6],
        [
            "comment 12..15",
            "\\n 15..16",
            "ERROR 16..22 has_error",
            "; 22..23",
            "comment 23..29"
        ]
    );
    // A recovery node can hold the first rows of a body, and a NUL byte cuts a comment.
    let hidden: &[u8] = b"proc f {} {\n# a\nset x 1 # text\n#ruff\n# later\n}\n";
    assert_eq!(
        shape(hidden, "braced_word", 10..46)[1..3],
        ["ERROR 12..23 has_error", "comment 24..30"]
    );
    assert_eq!(
        shape(hidden, "ERROR", 12..23),
        ["comment 12..15", "\\n 15..16", "set 16..23"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# a\n# b\x00c\nreturn\n}\n",
            "braced_word",
            10..30
        )[1..5],
        [
            "comment 12..15",
            "\\n 15..16",
            "comment 16..19",
            "ERROR 19..28 has_error"
        ]
    );
    // A recovery node can start on the row of a command and reach the next row, where
    // it holds a marker row. A NUL byte cuts a marker row after a command as well.
    assert_eq!(
        shape(
            b"proc f {} {\n# lead\nset x 1\n#ruff a\nset y 2 # text\n#ruff c\n}\n",
            "braced_word",
            10..59
        )[5..9],
        [
            "comment 27..34",
            "\\n 34..35",
            "set 35..42",
            "ERROR 43..57 has_error"
        ]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# lead\nset x 1\n#ruff b\x00c\nreturn\n}\n",
            "braced_word",
            10..45
        )[5..7],
        ["comment 27..34", "ERROR 34..43 has_error"]
    );
    // A stray `]` on a row of its own is a recovery node of one row.
    assert_eq!(
        shape(b"proc f {} {\n# a\n]\n# b\n}\n", "braced_word", 10..23)[1..6],
        [
            "comment 12..15",
            "\\n 15..16",
            "ERROR 16..17 has_error",
            "\\n 17..18",
            "comment 18..21"
        ]
    );
    // A comment row in a nested body is no child of the procedure's body.
    assert_eq!(
        shape(
            b"proc f {} {\nif {$x} {\n#ruff nested\n}\n}\n",
            "braced_word",
            10..38
        ),
        ["{ 10..11", "if 12..36", "\\n 36..37", "} 37..38"]
    );
    // A name of two parts gives two `name` children, so the procedure has five.
    assert_eq!(
        shape(b"proc a$b {} {\n# c\n}\n", "procedure", 0..19),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "name: variable_substitution 6..8",
            "arguments: arguments 9..11",
            "body: braced_word 12..19"
        ]
    );
    // A procedure of four children is `proc`, one name word, `arguments` and one body
    // word, in that order, and a body starts and ends with a brace exactly when it is a
    // `braced_word`. The reader tells the words apart by this and asks for no field name.
    for (source, range, name, body) in [
        (
            &b"proc f {} {\n# audit\nreturn\n}"[..],
            0..28,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc ::ns::f {} {\n# n\n}\n",
            0..23,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc f args {\n# a\n}\n",
            0..19,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc $name {} {\n# dyn\n}\n",
            0..23,
            "variable_substitution",
            "braced_word",
        ),
        (
            b"proc [mk] {} {\n# dyn\n}\n",
            0..22,
            "command_substitution",
            "braced_word",
        ),
        (
            b"proc [a;] {} {\n# c\n}\n",
            0..20,
            "command_substitution",
            "braced_word",
        ),
        (
            b"proc \"g\" {} {\n# q\n}\n",
            0..19,
            "quoted_word",
            "braced_word",
        ),
        (
            b"proc {h} {} {\n# b\n}\n",
            0..19,
            "braced_word",
            "braced_word",
        ),
        (
            b"proc f {a;} {\n# c\n}\n",
            0..19,
            "simple_word",
            "braced_word",
        ),
        (
            b"proc f {} \"\n# q\n\"\n",
            0..17,
            "simple_word",
            "quoted_word",
        ),
        (b"proc f {} return\n", 0..16, "simple_word", "simple_word"),
        (
            b"proc f {} $body\n",
            0..15,
            "simple_word",
            "variable_substitution",
        ),
        (
            b"proc f {}\nproc g {} {\n# doc g\nreturn\n}\n",
            0..14,
            "simple_word",
            "simple_word",
        ),
    ] {
        let parser = parsed(source);
        let procedure = node_at(parser.ts_tree(), "procedure", range);
        let words: Vec<_> = procedure.children(&mut procedure.walk()).collect();
        let kinds = |nodes: &[tree_sitter::Node<'_>]| -> Vec<_> {
            nodes.iter().map(tree_sitter::Node::kind).collect()
        };
        assert_eq!(kinds(&words), ["proc", name, "arguments", body]);
        assert_eq!(procedure.child_by_field_name("name"), Some(words[1]));
        assert_eq!(procedure.child_by_field_name("arguments"), Some(words[2]));
        assert_eq!(procedure.child_by_field_name("body"), Some(words[3]));
        let inner = kinds(&words[3].children(&mut words[3].walk()).collect::<Vec<_>>());
        let ends = (inner.first().copied(), inner.last().copied());
        assert_eq!(ends == (Some("{"), Some("}")), body == "braced_word");
    }
    // A backslash with its line end between two children of a body is in no node. The
    // grammar ends a recovery node before it and reads a comment after it; below a
    // continued `if` the comment is itself a recovery node.
    let continued: &[u8] = b"proc f {} {\nset x 1 \\\n#ruff notdoc\n}\n";
    assert_eq!(
        shape(continued, "braced_word", 10..36)[1..3],
        ["ERROR 12..19 has_error", "comment 22..34"]
    );
    assert_eq!(continued[19..22], *b" \\\n");
    assert_eq!(
        shape(
            b"proc f {} {\nif {1} {} \\\n#ruff b\n}\n",
            "braced_word",
            10..33
        )[1..3],
        ["if 12..21", "ERROR 24..31 has_error"]
    );
    // With CRLF the backslash and the CR are a command of their own.
    assert_eq!(
        shape(
            b"proc f {} {\r\nset x 1 \\\r\n#ruff b\r\n}\r\n",
            "braced_word",
            10..34
        )[1..5],
        [
            "ERROR 13..20 has_error",
            "command 21..23",
            "\\n 23..24",
            "comment 24..32"
        ]
    );
    // A row that holds only a backslash, between two comment rows, is no node.
    assert_eq!(
        shape(b"proc f {} {\n# lead\n\\\n# b\n}\n", "braced_word", 10..26)[1..4],
        ["comment 12..18", "\\n 18..19", "comment 21..24"]
    );
    // After a comment that continues the grammar reads `;` and a procedure, with no
    // recovery node. After `;` and an escaped line end it reads a procedure as well.
    let swallowed: &[u8] = b"# c \\\n;proc f {} {\n# doc\n}\n";
    assert!(!parsed(swallowed).root_node().has_error());
    assert_eq!(
        shape(swallowed, "source_file", 0..27),
        [
            "comment 0..5",
            "\\n 5..6",
            "; 6..7",
            "procedure 7..26",
            "\\n 26..27"
        ]
    );
    assert_eq!(
        shape(
            b"set x 1 ;\\\nproc g {} {\n# doc\n}\n",
            "source_file",
            0..31
        ),
        ["set 0..7", "; 8..9", "procedure 11..30", "\\n 30..31"]
    );
    // An unclosed brace gives a command that holds a recovery node, and a `#` that
    // starts no command gives a recovery node that holds a comment. A `;` and a clean
    // procedure follow both.
    assert_eq!(
        shape(b"set x {a ; proc g {} {\n# doc\n}\n", "source_file", 0..31),
        [
            "set 0..8 has_error",
            "; 9..10",
            "procedure 11..30",
            "\\n 30..31"
        ]
    );
    assert_eq!(
        shape(
            b"set x 1 # c \\\n; proc g {} {\n# doc\n}\n",
            "source_file",
            0..36
        ),
        [
            "set 0..7",
            "ERROR 8..13 has_error",
            "\\n 13..14",
            "; 14..15",
            "procedure 16..35",
            "\\n 35..36"
        ]
    );
    // White space for the grammar is a space, a tab, CR, LF, a vertical tab and a form
    // feed, and it is in no node. A CR alone ends no command: the grammar invents a
    // line end before the procedure, and reads `proc f`, CR and the other words as one
    // clean procedure. A no-break space is a byte of a word, as it is for Tcl.
    assert_eq!(
        shape(b"set x 1\rproc g {} {\n# doc\n}\n", "source_file", 0..28),
        [
            "set 0..7",
            "\\n 7..7 missing",
            "procedure 8..27",
            "\\n 27..28"
        ]
    );
    let returned: &[u8] = b"proc f\r{} {\n# doc\n}\n";
    assert!(!parsed(returned).root_node().has_error());
    assert_eq!(
        shape(returned, "procedure", 0..19),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 10..19"
        ]
    );
    assert_eq!(
        shape(b"\x0cproc g {} {\n# doc\n}\n", "source_file", 1..21),
        ["procedure 1..20", "\\n 20..21"]
    );
    assert_eq!(
        shape(b"\xc2\xa0proc g {} {\n# doc\n}\n", "source_file", 0..22),
        ["command 0..21", "\\n 21..22"]
    );
    // An unclosed parameter list makes the root an `ERROR` that holds a procedure.
    let parser =
        parsed(b"proc f {{a0 { {a1 { {a2 { {a3 {} {\n# audit\n}\nproc g {} {\n# doc g\n}\n");
    assert!(parser.ts_tree().root_node().is_error());
    assert_eq!(
        node_at(parser.ts_tree(), "procedure", 45..66).parent(),
        Some(parser.ts_tree().root_node())
    );
    // Where a braced word ends. The grammar reads a comment and a quoted word inside the
    // word first: a brace in one of them does not count for it, as it does for Tcl. The
    // first body ends at the `}` on the third row, and the root has no error.
    let pair: &[u8] = b"proc g {} {\n# {\n}\nproc f {} {\n# doc\n}\n# }\n";
    assert!(!parsed(pair).root_node().has_error());
    assert_eq!(
        shape(pair, "source_file", 0..42),
        [
            "procedure 0..17",
            "\\n 17..18",
            "procedure 18..37",
            "\\n 37..38",
            "comment 38..41",
            "\\n 41..42"
        ]
    );
    assert_eq!(
        shape(pair, "braced_word", 10..17),
        ["{ 10..11", "comment 12..15", "\\n 15..16", "} 16..17"]
    );
    assert_eq!(
        shape(
            b"proc f {} {\n# doc\nset s \"{\"\n}\n# more\n}\n",
            "braced_word",
            10..29
        ),
        [
            "{ 10..11",
            "comment 12..17",
            "\\n 17..18",
            "set 18..27",
            "\\n 27..28",
            "} 28..29"
        ]
    );
    // A braced word is a node whose first child is `{`, also where the parser invents
    // the `}`. In a recovery node an open `{` is a token with no word around it.
    assert_eq!(
        shape(
            b"set x {a ;\nproc g {} {\n# doc\n}\n",
            "braced_word_simple",
            6..8
        ),
        ["{ 6..7", "simple_word 7..8", "} 8..8 missing"]
    );
    assert_eq!(
        shape(b"set s $a# {\nproc f {} {\n# doc\n}\n", "ERROR", 0..11),
        ["set 0..9", "{ 10..11"]
    );
    // Tokens that stand directly before a `{`: a part of a word, where the brace starts
    // no word for Tcl, and `{*}`, `[` and `$`.
    assert_eq!(
        shape(b"set s a{b ;\nproc g {} {\n# doc\n}\n", "set", 0..9),
        [
            "set 0..3",
            "id 4..5",
            "simple_word 6..7",
            "braced_word_simple 7..9 has_error"
        ]
    );
    assert_eq!(
        shape(
            b"list {*}{# {\n}\nproc g {} {\n# doc\n}\n# }\n",
            "word_list",
            5..14
        ),
        ["unpack 5..8", "braced_word 8..14"]
    );
    let bracket: &[u8] = b"set s [{# {\n} a]\nproc g {} {\n# doc\n}\n# }\n";
    assert_eq!(
        shape(bracket, "command_substitution", 6..16),
        ["[ 6..7", "command 7..15", "] 15..16"]
    );
    assert_eq!(
        shape(bracket, "command", 7..15),
        ["name: braced_word 7..13", "arguments: word_list 14..15"]
    );
    assert_eq!(
        shape(
            b"set v ${name}\nproc f {} {\n# doc\n}\n",
            "variable_substitution",
            6..13
        ),
        ["$ 6..7", "{ 7..8", "} 12..13"]
    );
    // Two braced words of a procedure that nothing separates are a clean procedure.
    let glued: &[u8] = b"proc f {}{\n# doc\n}\n";
    assert!(!parsed(glued).root_node().has_error());
    assert_eq!(
        shape(glued, "procedure", 0..18),
        [
            "proc 0..4",
            "name: simple_word 5..6",
            "arguments: arguments 7..9",
            "body: braced_word 9..18"
        ]
    );
    // After a command of fixed word count the grammar reads `#` as the start of a
    // comment. For Tcl it is a word, and the `{` after it opens a word.
    assert_eq!(
        shape(
            b"set x 1 # c {\nproc f {} {\n# doc\n}\n}\n",
            "source_file",
            0..36
        ),
        [
            "ERROR 0..7 has_error",
            "comment 8..13",
            "\\n 13..14",
            "procedure 14..33",
            "\\n 33..34",
            "ERROR 34..35 has_error"
        ]
    );
    // The grammar reads a comment to the line feed. For Tcl a carriage return alone ends
    // it, so the brace after the CR is a token of no node.
    assert_eq!(
        shape(
            b"# c\rset x {\nproc f {} {\n# doc\n}\n}\n",
            "source_file",
            0..34
        ),
        [
            "comment 0..11",
            "\\n 11..12",
            "procedure 12..31",
            "\\n 31..32",
            "ERROR 32..33 has_error"
        ]
    );
    // The grammar skips a byte order mark: the root starts behind it.
    assert_eq!(
        shape(b"\xef\xbb\xbfproc f {} {\n# doc\n}\n", "source_file", 3..23),
        ["procedure 3..22", "\\n 22..23"]
    );
    // A comment that continues, and the rows Tcl reads as its text: the grammar gives
    // them nodes of their own, and no error.
    let continued: &[u8] = b"# c \\\nproc g {} {\n# { \\\n}\nproc f {} {\n# doc\n}\n# }\n";
    assert!(!parsed(continued).root_node().has_error());
    assert_eq!(
        shape(continued, "source_file", 0..50),
        [
            "comment 0..5",
            "\\n 5..6",
            "procedure 6..25",
            "\\n 25..26",
            "procedure 26..45",
            "\\n 45..46",
            "comment 46..49",
            "\\n 49..50"
        ]
    );
    // A bracket whose script starts with a comment: the grammar closes the bracket at
    // the end of the row and puts the procedure at the root.
    assert_eq!(
        shape(b"[# c\nproc f {} {\n# doc\n};]", "source_file", 0..26),
        [
            "command 0..4 has_error",
            "\\n 4..5",
            "procedure 5..24",
            "; 24..25",
            "ERROR 25..26 has_error"
        ]
    );
    // A bracket glued before `proc`: a plain command, or, behind a recovery node, a
    // procedure of the root that starts after the bracket.
    assert_eq!(
        shape(b"[]proc f {} {\n# doc\n}\n", "source_file", 0..22),
        ["command 0..21 has_error", "\\n 21..22"]
    );
    assert_eq!(
        shape(b"set x 1];[]proc f {} {\n# doc\n}\n", "source_file", 0..31),
        ["ERROR 0..11 has_error", "procedure 11..30", "\\n 30..31"]
    );
    // A quoted parameter list: a recovery node and no procedure.
    assert_eq!(
        shape(b"proc f \"x\" {\n# doc\n}\n", "source_file", 0..21),
        ["ERROR 0..20 has_error", "\\n 20..21"]
    );
}
