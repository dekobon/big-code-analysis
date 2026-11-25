//! Go documentation in the source view: which declaration, field name or interface
//! element owns a comment, the text of the document and where each part of that text
//! comes from. Every source is collected through both entry points, with three
//! exceptions. The grammar tests collect nothing and pin the parse-tree shapes the
//! other tests rely on. A tree parsed with excluded ranges goes through `collect`, the
//! one entry point that takes such a tree. The cost tests read the counters around one
//! collection through the typed entry point.

#![allow(clippy::single_range_in_vec_init, clippy::type_complexity)]

#[cfg(feature = "go")]
use crate::documentation::*;
#[cfg(feature = "go")]
use crate::{AnyParser, LANG};
#[cfg(feature = "go")]
use std::ops::Range;

#[cfg(feature = "go")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "go")]
use std::cell::Cell;

#[cfg(feature = "go")]
use Run::{Copied, Decoded, Synthetic};

/// The origin of one run of a document's text, with the source byte range it reads.
#[cfg(feature = "go")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

/// One document: its role, its fragments as source byte ranges, its text, and each run
/// of the text as an output range with its origin. A package document has no role here.
#[cfg(feature = "go")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: Option<DocumentRole>,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

/// What one collection published: the documents, the owner of each association as
/// resolution, node kind and byte range, each retained candidate as disposition and byte
/// range, and the number of pending assessments. Where field slots own an association,
/// `owners` holds the declaration that encloses them, and `slots` holds the position of
/// the association and the byte range of each name, in the association's order. The
/// default value is an index that holds none of them. What is the same for every source
/// here is asserted by `published` and is in no expected value. Node queries are not
/// part of it.
#[cfg(feature = "go")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    owners: Vec<(Resolution, &'static str, Range<usize>)>,
    slots: Vec<(usize, Vec<Range<usize>>)>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: usize,
}

/// The candidates a collection keeps beside its documents.
#[cfg(feature = "go")]
#[derive(Clone, Copy)]
enum Retain {
    Unbound,
    Directives,
    Both,
}

/// The expected document of one comment above a declaration other than the package
/// clause.
#[cfg(feature = "go")]
fn comment<const N: usize>(
    fragment: Range<usize>,
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        role: Some(DocumentRole::Primary),
        composition: Composition::Single,
        fragments: vec![fragment],
        text: text.to_owned(),
        map: map.into(),
    }
}

/// The expected document of one comment above the package clause.
#[cfg(feature = "go")]
fn package_comment<const N: usize>(
    fragment: Range<usize>,
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        role: None,
        ..comment(fragment, text, map)
    }
}

/// The expected document of adjacent comments that join.
#[cfg(feature = "go")]
fn comment_run<const F: usize, const N: usize>(
    fragments: [Range<usize>; F],
    text: &str,
    map: [(Range<usize>, Run); N],
) -> Doc {
    Doc {
        role: Some(DocumentRole::Primary),
        composition: Composition::CommentRun,
        fragments: fragments.into(),
        text: text.to_owned(),
        map: map.into(),
    }
}

/// The expected view of documents in index order, each owned by the resolved node at
/// its position, with no retained candidate and no pending assessment.
#[cfg(feature = "go")]
fn owned_in_order<const N: usize>(
    documents: [Doc; N],
    owners: [(&'static str, Range<usize>); N],
) -> View {
    View {
        documents: documents.into(),
        owners: owners
            .into_iter()
            .map(|(kind, owner)| (Resolution::Resolved, kind, owner))
            .collect(),
        ..View::default()
    }
}

#[cfg(feature = "go")]
fn owned(document: Doc, kind: &'static str, owner: Range<usize>) -> View {
    owned_in_order([document], [(kind, owner)])
}

/// The expected view of one document that the names of one field declaration own.
#[cfg(feature = "go")]
fn named(document: Doc, declaration: Range<usize>, names: &[Range<usize>]) -> View {
    View {
        slots: vec![(0, names.to_vec())],
        ..owned(document, "field_declaration", declaration)
    }
}

/// The byte range of a spelling that is one included piece, as every spelling here is.
#[cfg(feature = "go")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "go")]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

/// The byte range of a fragment. The same holds for every fragment here, so no expected
/// view states it: the spelling is a comment, and its node is the `comment` node that
/// starts with it. The node may end one byte later, where it holds the CR of a CRLF.
#[cfg(feature = "go")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    let carrier = fragment.node().unwrap().node();
    assert_eq!(
        (carrier.kind(), carrier.start_byte()),
        ("comment", range.start)
    );
    range
}

/// The node that alone owns the association, or the declaration that encloses the field
/// slots that own it.
#[cfg(feature = "go")]
fn owner_node<'s>(
    facts: &Facts<'s>,
    association: FactRef<'_, Association<'s>>,
) -> tree_sitter::Node<'s> {
    let owner = facts.associated_owner(association, 0).unwrap();
    let around = match owner.get() {
        Owner::Node(_) => {
            assert_eq!(association.get().owner_count(), 1);
            owner
        }
        Owner::Slot(_) => facts.slot_enclosing(owner).unwrap(),
        other => panic!("a node or a field slot, found {other:?}"),
    };
    let Owner::Node(declared) = around.get() else {
        panic!("a node around the slot, found {:?}", around.get())
    };
    declared.node()
}

/// The byte range of the name of each field slot that owns the association, in its
/// order; nothing where a node owns it. The same holds for every slot here, so no
/// expected view states it: the role is `Field`, the ordinal is the position among the
/// owners, the name is one included piece, every slot of the association has the same
/// enclosing owner, and no qualification record names a slot.
#[cfg(feature = "go")]
fn slot_names(
    index: &DocumentationIndex<'_>,
    association: FactRef<'_, Association<'_>>,
) -> Vec<Range<usize>> {
    let facts = index.facts();
    let first = facts.associated_owner(association, 0).unwrap();
    if !matches!(first.get(), Owner::Slot(_)) {
        return Vec::new();
    }
    let around = facts.slot_enclosing(first).unwrap();
    (0..association.get().owner_count())
        .map(|at| {
            let owner = facts.associated_owner(association, at).unwrap();
            let Owner::Slot(slot) = owner.get() else {
                panic!("a field slot, found {:?}", owner.get())
            };
            assert_eq!((slot.role, slot.ordinal), (SlotRole::Field, at));
            assert!(!slot.source.has_excluded_gap());
            assert_eq!(facts.slot_enclosing(owner).unwrap(), around);
            let query = index.owner_associations(owner).unwrap();
            assert_eq!(query.qualifications().len(), 0);
            span(&slot.source)
        })
        .collect()
}

#[cfg(feature = "go")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one flat reading of everything the index publishes, in the order of the view's fields.
    let facts = index.facts();
    let documents = index
        .documents()
        .map(|document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            let owner_kind = index
                .associations()
                .find(|association| facts.associated_document(*association).unwrap() == document)
                .map(|association| owner_node(facts, association).kind());
            Doc {
                // The role of a package document is undecided, so the view holds none for it.
                role: (owner_kind != Some("package_clause")).then(|| document.get().role()),
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
    let mut slots = Vec::new();
    let owners = index
        .associations()
        .enumerate()
        .map(|(position, association)| {
            let attachment = association.get();
            let declaration = owner_node(facts, association);
            let names = slot_names(index, association);
            if !names.is_empty() {
                slots.push((position, names));
            }
            // The same for every association here, so no expected view states them. Each
            // document has one association, at the document's own position.
            assert_eq!(attachment.convention.name(), "GoDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            assert_eq!(index.documents().nth(position), Some(document));
            // Both follow from the tree: the binding is the owner node, or the declaration
            // around the owning slots, and the lexical scope is the parent of the first
            // comment of the document.
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                Some(declaration)
            );
            let first = facts.document_fragment(document, 0).unwrap();
            let first_comment = first.get().node().unwrap().node();
            assert_eq!(
                Some(attachment.lexical_scope.node()),
                first_comment.parent()
            );
            (
                attachment.resolution,
                declaration.kind(),
                declaration.byte_range(),
            )
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    // The same for every source here: the Go adapter is partial, and a qualification
    // record belongs to a documentation command that names its declaration, which Go
    // does not have.
    assert_eq!(index.capability(), Capability::Partial);
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        owners,
        slots,
        evidence,
        assessments: index.assessments().len(),
    }
}

/// A parse in which no node below the root holds a syntax error, is an error node or is
/// missing. Whether the root holds an error is for the caller to assert.
#[cfg(feature = "go")]
fn parsed(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    let root = parser.ts_tree().root_node();
    let flagged: Vec<_> = parser
        .root_node()
        .preorder()
        .map(|found| found.as_tree_sitter())
        .filter(|found| {
            *found != root && (found.has_error() || found.is_error() || found.is_missing())
        })
        .collect();
    assert!(
        flagged.is_empty(),
        "{flagged:?} in {}",
        source.escape_ascii()
    );
    parser
}

/// A parse whose tree holds no syntax error.
#[cfg(feature = "go")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = parsed(source);
    assert!(!parser.root_node().has_error(), "{}", source.escape_ascii());
    parser
}

/// Both entry points over one parse: the typed parser's method, then `collect`.
#[cfg(feature = "go")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Go,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

/// The view that both entry points publish.
#[cfg(feature = "go")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2]) -> View {
    let [typed, raw] = indexes.each_ref().map(published);
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

/// The view of an error-free parse under the default options.
#[cfg(feature = "go")]
fn view(source: &[u8]) -> View {
    agreed(&indexes(&clean(source), DocumentationOptions::default()))
}

/// The view of an error-free parse that also keeps those candidates.
#[cfg(feature = "go")]
fn retained(source: &[u8], retain: Retain) -> View {
    let options = DocumentationOptions {
        retain_unbound: matches!(retain, Retain::Unbound | Retain::Both),
        retain_directives: matches!(retain, Retain::Directives | Retain::Both),
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&clean(source), options))
}

/// Asserts the view under the default options, and that the retention adds exactly that
/// candidate to it. For an empty default view, the candidate shows that the collector
/// saw the comment.
#[cfg(feature = "go")]
fn assert_default_then_retained(
    source: &[u8],
    mut expected: View,
    retain: Retain,
    candidate: (CandidateDisposition, Range<usize>),
) {
    assert_eq!(view(source), expected, "{}", source.escape_ascii());
    expected.evidence.push(candidate);
    assert_eq!(
        retained(source, retain),
        expected,
        "{}",
        source.escape_ascii()
    );
}

/// The one node of that kind and byte range.
#[cfg(feature = "go")]
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

#[cfg(feature = "go")]
fn named_in(parent: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    parent.named_children(&mut parent.walk()).collect()
}

#[cfg(feature = "go")]
fn shapes(nodes: &[tree_sitter::Node<'_>]) -> Vec<(&'static str, Range<usize>)> {
    nodes
        .iter()
        .map(|found| (found.kind(), found.byte_range()))
        .collect()
}

#[cfg(feature = "go")]
#[test]
fn go_declaration_kinds_own_the_comment_above_them() {
    assert_eq!(
        view(b"// audit\npackage p"),
        owned(
            package_comment(0..8, "audit", [(0..5, Copied(3..8))]),
            "package_clause",
            9..18
        )
    );
    let declarations: [(&[u8], Doc, &str, Range<usize>); 5] = [
        (
            b"package p\n/* audit */\nfunc F() {}",
            comment(10..21, "audit", [(0..5, Copied(13..18))]),
            "function_declaration",
            22..33,
        ),
        (
            b"package p\ntype T int\n// audit\nfunc (t T) M() {}",
            comment(21..29, "audit", [(0..5, Copied(24..29))]),
            "method_declaration",
            30..47,
        ),
        (
            b"package p\n// audit\nconst C = 1",
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "const_declaration",
            19..30,
        ),
        (
            b"package p\n// audit\ntype T struct{}\n",
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "type_declaration",
            19..34,
        ),
        (
            b"package p\n// audit\ntype A = int\n",
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "type_declaration",
            19..31,
        ),
    ];
    for (source, document, kind, owner) in declarations {
        assert_eq!(
            view(source),
            owned(document, kind, owner),
            "{}",
            source.escape_ascii()
        );
    }

    let parser = clean(b"package p\n// audit\nvar x, y int\n");
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        owned(
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "var_declaration",
            19..31
        )
    );
    let declaration = node_at(&parser, "var_declaration", 19..31);
    let spec = node_at(&parser, "var_spec", 23..31);
    for index in &both {
        for declared in [declaration, spec] {
            let projection = index.node_projection(declared).unwrap();
            assert!(!projection.pending());
            let owners: Vec<_> = projection.owners().collect();
            let [owner] = owners[..] else {
                panic!("one owner, found {owners:?}")
            };
            let Owner::Node(itself) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            assert_eq!(itself.node(), declared);
        }
        let query = index.node_associations(declaration).unwrap();
        assert_eq!(query.associations().len(), 1);
        assert!(!query.projection_pending());
        assert_eq!(query.assessments().count(), 0);
        // The spec is an owner as well, and the comment above the declaration is not
        // its document.
        let query = index.node_associations(spec).unwrap();
        assert_eq!(query.associations().len(), 0);
        assert!(!query.projection_pending());
        assert_eq!(query.assessments().count(), 0);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_group_and_its_specs_have_separate_documents() {
    assert_eq!(
        view(b"package p\n// group\nvar (\n// member\nX int\n)\n"),
        owned_in_order(
            [
                comment(10..18, "group", [(0..5, Copied(13..18))]),
                comment(25..34, "member", [(0..6, Copied(28..34))]),
            ],
            [("var_declaration", 19..42), ("var_spec", 35..40)]
        )
    );
    assert_eq!(
        view(b"package p\n// g\nconst (\n// m\nC, D = 1, 2\n)\n"),
        owned_in_order(
            [
                comment(10..14, "g", [(0..1, Copied(13..14))]),
                comment(23..27, "m", [(0..1, Copied(26..27))]),
            ],
            [("const_declaration", 15..41), ("const_spec", 28..39)]
        )
    );
    assert_eq!(
        view(b"package p\n// h\ntype (\n// n\nT int\n// o\nU = int\n)\n"),
        owned_in_order(
            [
                comment(10..14, "h", [(0..1, Copied(13..14))]),
                comment(22..26, "n", [(0..1, Copied(25..26))]),
                comment(33..37, "o", [(0..1, Copied(36..37))]),
            ],
            [
                ("type_declaration", 15..47),
                ("type_spec", 27..32),
                ("type_alias", 38..45),
            ]
        )
    );
}

#[cfg(feature = "go")]
#[test]
fn go_adjacent_comments_join_into_one_run() {
    let runs: [(&[u8], Doc, Range<usize>); 3] = [
        (
            b"package p\n// a\n// b\nfunc F() {}",
            comment_run(
                [10..14, 15..19],
                "a\nb",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Synthetic),
                    (2..3, Copied(18..19)),
                ],
            ),
            20..31,
        ),
        (
            b"package p\n// a\n/* b */\n// c\nfunc F() {}",
            comment_run(
                [10..14, 15..22, 23..27],
                "a\nb\nc",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Synthetic),
                    (2..3, Copied(18..19)),
                    (3..4, Synthetic),
                    (4..5, Copied(26..27)),
                ],
            ),
            28..39,
        ),
        (
            b"package p\n/* a */ /* b */\nfunc F() {}",
            comment_run(
                [10..17, 18..25],
                "a\nb",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Synthetic),
                    (2..3, Copied(21..22)),
                ],
            ),
            26..37,
        ),
    ];
    for (source, document, owner) in runs {
        assert_eq!(
            view(source),
            owned(document, "function_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_empty_comment_row_stays_inside_the_run() {
    assert_eq!(
        view(b"package p\n// a\n//\n// c\nfunc F() {}"),
        owned(
            comment_run(
                [10..14, 15..17, 18..22],
                "a\n\nc",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Synthetic),
                    (2..3, Synthetic),
                    (3..4, Copied(21..22)),
                ]
            ),
            "function_declaration",
            23..34
        )
    );
    // At the start and at the end of a run, an empty row is a fragment that adds no text.
    assert_eq!(
        view(b"package p\n//\n// a\n//\nfunc F() {}"),
        owned(
            comment_run([10..12, 13..17, 18..20], "a", [(0..1, Copied(16..17))]),
            "function_declaration",
            21..32
        )
    );
}

#[cfg(feature = "go")]
#[test]
fn go_blank_source_row_detaches_a_run() {
    // The comment at 10..14 is above a blank row and is in no document.
    assert_default_then_retained(
        b"package p\n// a\n\n// b\nfunc F() {}",
        owned(
            comment(16..20, "b", [(0..1, Copied(19..20))]),
            "function_declaration",
            21..32,
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 10..14),
    );
    assert_default_then_retained(
        b"package p\n// a\n\nfunc F() {}",
        View::default(),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 10..14),
    );
    // Keeping directives does not keep the detached comment.
    let detached: &[u8] = b"package p\n// a\n\n// b\nfunc F() {}";
    assert_eq!(retained(detached, Retain::Directives), view(detached));
}

#[cfg(feature = "go")]
#[test]
fn go_directive_rows_are_evidence_and_not_prose() {
    // The directive at 19..37 is in no document.
    assert_default_then_retained(
        b"package p\n// audit\n//go:generate tool\nfunc F() {}",
        owned(
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "function_declaration",
            38..49,
        ),
        Retain::Directives,
        (CandidateDisposition::Directive, 19..37),
    );
    // Keeping unbound candidates does not keep the directive of a selected run.
    let selected: &[u8] = b"package p\n// audit\n//go:generate tool\nfunc F() {}";
    assert_eq!(retained(selected, Retain::Unbound), view(selected));
    // A directive before, inside and after the prose, and a comment that only looks
    // like one: `// go:generate` has a space after the marker.
    let prose: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"package p\n//go:noinline\n// audit\nfunc F() {}",
            comment(24..32, "audit", [(0..5, Copied(27..32))]),
            33..44,
        ),
        (
            b"package p\n// a\n//go:generate tool\n// b\nfunc F() {}",
            comment_run(
                [10..14, 34..38],
                "a\nb",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Synthetic),
                    (2..3, Copied(37..38)),
                ],
            ),
            39..50,
        ),
        (
            b"package p\n// audit\n//export F\nfunc F() {}",
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            30..41,
        ),
        (
            b"package p\n// go:generate tool\nfunc F() {}",
            comment(10..29, "go:generate tool", [(0..16, Copied(13..29))]),
            30..41,
        ),
    ];
    for (source, document, owner) in prose {
        assert_eq!(
            view(source),
            owned(document, "function_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
    // A run that holds only directives documents nothing. That is the collector's answer
    // today, pinned as it is; whether such a run should be a document is not decided.
    // In the first source a blank row also separates the run from the package clause;
    // its control sets both options.
    let directives_alone: [(&[u8], Retain, Range<usize>); 3] = [
        (b"//go:build linux\n\npackage p", Retain::Both, 0..16),
        (b"//go:generate tool\npackage p", Retain::Directives, 0..18),
        (
            b"package p\n//go:noinline\nfunc F() {}",
            Retain::Directives,
            10..23,
        ),
    ];
    for (source, retain, directive) in directives_alone {
        assert_default_then_retained(
            source,
            View::default(),
            retain,
            (CandidateDisposition::Directive, directive),
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_trailing_comment_documents_nothing() {
    // The comment at 20..24 follows code on its row and is in no document.
    assert_default_then_retained(
        b"package p\nvar x = 1 // t\nfunc F() {}",
        View::default(),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 20..24),
    );
    assert_default_then_retained(
        b"package p\nvar x = 1 // t\n// d\nfunc F() {}",
        owned(
            comment(25..29, "d", [(0..1, Copied(28..29))]),
            "function_declaration",
            30..41,
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 20..24),
    );
}

#[cfg(feature = "go")]
#[test]
fn go_local_declaration_has_no_document_and_no_owner() {
    let first_in_body = b"package p\nfunc F() {\n// local\nvar x = 1\n}";
    let parser = clean(first_in_body);
    let both = indexes(&parser, DocumentationOptions::default());
    let mut expected = View::default();
    assert_eq!(agreed(&both), expected);
    let locals = [
        node_at(&parser, "var_declaration", 30..39),
        node_at(&parser, "var_spec", 34..39),
    ];
    for index in &both {
        for local in locals {
            assert_eq!(
                index.node_projection(local).err(),
                Some(OwnerError::UnsupportedOwner),
                "{}",
                local.kind()
            );
        }
    }
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 21..29));
    assert_eq!(retained(first_in_body, Retain::Unbound), expected);

    let after_a_statement: [(&[u8], Range<usize>); 2] = [
        (
            b"package p\nfunc F() {\nx := 1\n// local\nvar y = x\n_ = y\n}",
            28..36,
        ),
        (
            b"package p\nfunc F() {\n_ = 0\n// local\ntype T int\n}",
            27..35,
        ),
    ];
    for (source, unattached) in after_a_statement {
        assert_default_then_retained(
            source,
            View::default(),
            Retain::Unbound,
            (CandidateDisposition::Unattached, unattached),
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_import_has_no_document_and_no_owner() {
    let single = b"package p\n// not doc\nimport \"fmt\"";
    let parser = clean(single);
    let both = indexes(&parser, DocumentationOptions::default());
    let mut expected = View::default();
    assert_eq!(agreed(&both), expected);
    let imports = [
        node_at(&parser, "import_declaration", 21..33),
        node_at(&parser, "import_spec", 28..33),
    ];
    for index in &both {
        for import in imports {
            assert_eq!(
                index.node_projection(import).err(),
                Some(OwnerError::UnsupportedOwner),
                "{}",
                import.kind()
            );
        }
    }
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 10..20));
    assert_eq!(retained(single, Retain::Unbound), expected);

    assert_default_then_retained(
        b"package p\nimport (\n// c\n\"fmt\"\n)",
        View::default(),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 19..23),
    );
}

#[cfg(feature = "go")]
#[test]
fn go_documents_of_one_file_stay_separate_and_in_source_order() {
    assert_eq!(
        view(b"// pkg\npackage p\n// fn\nfunc F() {}"),
        owned_in_order(
            [
                package_comment(0..6, "pkg", [(0..3, Copied(3..6))]),
                comment(17..22, "fn", [(0..2, Copied(20..22))]),
            ],
            [("package_clause", 7..16), ("function_declaration", 23..34)]
        )
    );
    assert_eq!(
        view(b"package p\n// same\nfunc A() {}\n// same\nfunc B() {}"),
        owned_in_order(
            [
                comment(10..17, "same", [(0..4, Copied(13..17))]),
                comment(30..37, "same", [(0..4, Copied(33..37))]),
            ],
            [
                ("function_declaration", 18..29),
                ("function_declaration", 38..49),
            ]
        )
    );
}

#[cfg(feature = "go")]
#[test]
fn go_fixture_function_owns_its_comment() {
    let source = crate::test_support::documentation::source("primary.go");
    assert_eq!(source, b"package p\n// audit\nfunc F() {}");
    assert_eq!(
        view(&source),
        owned(
            comment(10..18, "audit", [(0..5, Copied(13..18))]),
            "function_declaration",
            19..30
        )
    );
}

#[cfg(feature = "go")]
#[test]
fn go_line_endings_and_the_end_of_the_file() {
    let documented: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"package p\r\n// a\r\n// b\r\nfunc F() {}\r\n",
            comment_run(
                [11..15, 17..21],
                "a\nb",
                [
                    (0..1, Copied(14..15)),
                    (1..2, Synthetic),
                    (2..3, Copied(20..21)),
                ],
            ),
            23..34,
        ),
        (
            b"package p\r\n/* a\r\n   b */\r\nfunc F() {}\r\n",
            comment(
                11..24,
                "a\n   b",
                [
                    (0..1, Copied(14..15)),
                    (1..2, Decoded(15..17)),
                    (2..6, Copied(17..21)),
                ],
            ),
            26..37,
        ),
        (
            b"package p\r\n// a\r\n//\r\n// c\r\nfunc F() {}",
            comment_run(
                [11..15, 17..19, 21..25],
                "a\n\nc",
                [
                    (0..1, Copied(14..15)),
                    (1..2, Synthetic),
                    (2..3, Synthetic),
                    (3..4, Copied(24..25)),
                ],
            ),
            27..38,
        ),
        (
            b"package p\r\n// audit\r\n//go:generate tool\r\nfunc F() {}",
            comment(11..19, "audit", [(0..5, Copied(14..19))]),
            41..52,
        ),
    ];
    for (source, document, owner) in documented {
        assert_eq!(
            view(source),
            owned(document, "function_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"// audit\npackage p\n"),
        owned(
            package_comment(0..8, "audit", [(0..5, Copied(3..8))]),
            "package_clause",
            9..18
        )
    );
    // A CRLF blank row detaches the comment, and a comment at the end of the file has
    // no declaration after it.
    let undocumented: [(&[u8], Range<usize>); 2] = [
        (b"package p\r\n// a\r\n\r\nfunc F() {}", 11..15),
        (b"package p\nfunc F() {}\n// tail", 22..29),
    ];
    for (source, unattached) in undocumented {
        assert_default_then_retained(
            source,
            View::default(),
            Retain::Unbound,
            (CandidateDisposition::Unattached, unattached),
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_text_cleanup_removes_only_the_markers_and_one_space() {
    let comments: [(&[u8], Doc, Range<usize>); 7] = [
        (
            b"package p\n//audit\nfunc F() {}",
            comment(10..17, "audit", [(0..5, Copied(12..17))]),
            18..29,
        ),
        (
            b"package p\n//  audit\nfunc F() {}",
            comment(10..19, " audit", [(0..6, Copied(13..19))]),
            20..31,
        ),
        (
            b"package p\n// audit  \nfunc F() {}",
            comment(10..20, "audit  ", [(0..7, Copied(13..20))]),
            21..32,
        ),
        (
            b"package p\n/*audit*/\nfunc F() {}",
            comment(10..19, "audit", [(0..5, Copied(12..17))]),
            20..31,
        ),
        (
            b"package p\n/*  audit  */\nfunc F() {}",
            comment(10..23, " audit ", [(0..7, Copied(13..20))]),
            24..35,
        ),
        (
            b"package p\n/* a\n * b */\nfunc F() {}",
            comment(
                10..22,
                "a\n * b",
                [
                    (0..1, Copied(13..14)),
                    (1..2, Copied(14..15)),
                    (2..6, Copied(15..19)),
                ],
            ),
            23..34,
        ),
        (
            b"package p\n/*\naudit\n*/\nfunc F() {}",
            comment(10..21, "audit", [(0..5, Copied(13..18))]),
            22..33,
        ),
    ];
    for (source, document, owner) in comments {
        assert_eq!(
            view(source),
            owned(document, "function_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_grammar_shapes_the_unit_relies_on() {
    // A comment above a declaration is an extra child of the file, and the declaration
    // is its next sibling.
    let parser = clean(b"package p\n// audit\nfunc F() {}");
    let top = named_in(parser.ts_tree().root_node());
    assert_eq!(
        shapes(&top),
        [
            ("package_clause", 0..9),
            ("comment", 10..18),
            ("function_declaration", 19..30),
        ]
    );
    assert!(top[1].is_extra());
    assert_eq!(top[1].next_sibling(), Some(top[2]));

    // The specs of a `var` group and their comments are in a list node.
    let parser = clean(b"package p\n// group\nvar (\n// member\nX int\n)\n");
    let group = named_in(node_at(&parser, "var_declaration", 19..42));
    assert_eq!(shapes(&group), [("var_spec_list", 23..42)]);
    assert_eq!(
        shapes(&named_in(group[0])),
        [("comment", 25..34), ("var_spec", 35..40)]
    );

    // The specs of a `const` group and of a `type` group are directly under the
    // declaration; there is no list node.
    let parser = clean(b"package p\n// g\nconst (\n// m\nC, D = 1, 2\n)\n");
    assert_eq!(
        shapes(&named_in(node_at(&parser, "const_declaration", 15..41))),
        [("comment", 23..27), ("const_spec", 28..39)]
    );
    let parser = clean(b"package p\n// h\ntype (\n// n\nT int\n// o\nU = int\n)\n");
    assert_eq!(
        shapes(&named_in(node_at(&parser, "type_declaration", 15..47))),
        [
            ("comment", 22..26),
            ("type_spec", 27..32),
            ("comment", 33..37),
            ("type_alias", 38..45),
        ]
    );

    // An ungrouped declaration holds its spec directly.
    let parser = clean(
        b"package p\n// t\ntype T struct{}\n// c\nconst C = 1\n// a\ntype A = int\n// v\nvar x, y int\n",
    );
    let ungrouped: [(&str, Range<usize>, &str, Range<usize>); 4] = [
        ("type_declaration", 15..30, "type_spec", 20..30),
        ("const_declaration", 36..47, "const_spec", 42..47),
        ("type_declaration", 53..65, "type_alias", 58..65),
        ("var_declaration", 71..83, "var_spec", 75..83),
    ];
    for (kind, declaration, spec_kind, spec) in ungrouped {
        assert_eq!(
            shapes(&named_in(node_at(&parser, kind, declaration))),
            [(spec_kind, spec)]
        );
    }

    // The fields of a struct and their comments are in one list. A field with two
    // names has two name nodes, and an embedded field has none.
    let parser = clean(b"package p\ntype S struct {\n// both\nX, Y int\n// e\nio.Reader\n}\n");
    let fields = named_in(node_at(&parser, "field_declaration_list", 24..59));
    assert_eq!(
        shapes(&fields),
        [
            ("comment", 26..33),
            ("field_declaration", 34..42),
            ("comment", 43..47),
            ("field_declaration", 48..57),
        ]
    );
    assert_eq!(
        shapes(&named_in(fields[1])),
        [
            ("field_identifier", 34..35),
            ("field_identifier", 37..38),
            ("type_identifier", 39..42),
        ]
    );
    assert_eq!(shapes(&named_in(fields[3])), [("qualified_type", 48..57)]);

    // The elements of an interface and their comments are directly under the interface
    // type. An embedded name and a `~` element are both type elements.
    let parser =
        clean(b"package p\ntype I interface {\n// m\nM()\n// e\nio.Reader\n// u\n~int\n}\n");
    let elements = named_in(node_at(&parser, "interface_type", 17..64));
    assert_eq!(
        shapes(&elements),
        [
            ("comment", 29..33),
            ("method_elem", 34..37),
            ("comment", 38..42),
            ("type_elem", 43..52),
            ("comment", 53..57),
            ("type_elem", 58..62),
        ]
    );
    assert_eq!(shapes(&named_in(elements[3])), [("qualified_type", 43..52)]);
    assert_eq!(shapes(&named_in(elements[5])), [("negated_type", 58..62)]);

    // The first comment of a body is outside the statement list; a later one is inside.
    let parser = clean(b"package p\nfunc F() {\n// first\nx := 1\n// local\nvar y = x\n_ = y\n}");
    let body = named_in(node_at(&parser, "block", 19..63));
    assert_eq!(
        shapes(&body),
        [("comment", 21..29), ("statement_list", 30..62)]
    );
    assert_eq!(
        shapes(&named_in(body[1])),
        [
            ("short_var_declaration", 30..36),
            ("comment", 37..45),
            ("var_declaration", 46..55),
            ("assignment_statement", 56..61),
        ]
    );

    // A trailing comment, an empty comment row and the comment above a method are all
    // children of the file, in source order.
    let parser = clean(b"package p\nvar x = 1 // t\n//\n// d\nfunc (t T) M() {}");
    assert_eq!(
        shapes(&named_in(parser.ts_tree().root_node())),
        [
            ("package_clause", 0..9),
            ("var_declaration", 10..19),
            ("comment", 20..24),
            ("comment", 25..27),
            ("comment", 28..32),
            ("method_declaration", 33..50),
        ]
    );

    // The specs of an import group and their comments are in a list node.
    let parser = clean(b"package p\nimport (\n// c\n\"fmt\"\n)");
    let imports = named_in(node_at(&parser, "import_declaration", 10..31));
    assert_eq!(shapes(&imports), [("import_spec_list", 17..31)]);
    assert_eq!(
        shapes(&named_in(imports[0])),
        [("comment", 19..23), ("import_spec", 24..29)]
    );

    // With CRLF a line comment node includes the CR, and a block comment node ends at
    // its `*/`.
    let crlf = b"package p\r\n// a\r\n/* b\r\n c */\r\nfunc F() {}\r\n";
    let parser = clean(crlf);
    let top = named_in(parser.ts_tree().root_node());
    assert_eq!(
        shapes(&top),
        [
            ("package_clause", 0..9),
            ("comment", 11..16),
            ("comment", 17..28),
            ("function_declaration", 30..41),
        ]
    );
    assert_eq!(&crlf[top[1].byte_range()], b"// a\r");
    assert_eq!(&crlf[top[1].end_byte()..top[2].start_byte()], b"\n");
    assert_eq!(&crlf[top[2].end_byte()..top[3].start_byte()], b"\r\n");

    // With LF a line comment node ends before the LF, and the root spans the final LF.
    let lf = b"package p\n// a\n/* b\n c */\nfunc F() {}\n";
    let parser = clean(lf);
    let root = parser.ts_tree().root_node();
    let top = named_in(root);
    assert_eq!(
        shapes(&top),
        [
            ("package_clause", 0..9),
            ("comment", 10..14),
            ("comment", 15..25),
            ("function_declaration", 26..37),
        ]
    );
    assert_eq!(&lf[top[1].end_byte()..top[2].start_byte()], b"\n");
    assert_eq!(&lf[top[2].end_byte()..top[3].start_byte()], b"\n");
    assert_eq!(root.byte_range(), 0..38);
    assert_eq!(&lf[top[3].end_byte()..root.end_byte()], b"\n");

    // A file that ends in a `type` declaration without a line ending has the same nodes
    // as the file with one, and its root alone holds an error.
    let ends: [(&[u8], bool); 2] = [
        (b"package p\ntype T int", true),
        (b"package p\ntype T int\n", false),
    ];
    for (source, root_error) in ends {
        let parser = parsed(source);
        let root = parser.ts_tree().root_node();
        assert_eq!(root.has_error(), root_error, "{}", source.escape_ascii());
        assert_eq!(
            shapes(&named_in(root)),
            [("package_clause", 0..9), ("type_declaration", 10..20)],
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "go")]
const AWAITS_PROJECTION: &str = "Selected local or wrapper form awaits owner projection";

#[cfg(feature = "go")]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";

#[cfg(feature = "go")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

/// Every document as its fragments and its text, for a parse the view does not read.
#[cfg(feature = "go")]
fn texts(index: &DocumentationIndex<'_>) -> Vec<(Vec<Range<usize>>, String)> {
    index
        .documents()
        .map(|document| {
            let fragments = (0..document.get().fragment_count())
                .map(|at| {
                    let fragment = index.facts().document_fragment(document, at).unwrap();
                    fragment_span(fragment.get())
                })
                .collect();
            let complete = index.complete_text(document).unwrap().unwrap();
            (fragments, complete.text().to_owned())
        })
        .collect()
}

/// One owner as text: a node as kind and byte range; a field slot as its ordinal, the
/// byte range of each piece of its name, and the node that encloses it; the file.
#[cfg(feature = "go")]
fn owner_text<'s>(index: &DocumentationIndex<'s>, owner: FactRef<'_, Owner<'s>>) -> String {
    match owner.get() {
        Owner::Node(declared) => {
            let (kind, range) = site(declared.node());
            format!("{kind} {}..{}", range.start, range.end)
        }
        Owner::Slot(slot) => {
            assert_eq!(slot.role, SlotRole::Field);
            assert!(!slot.source.has_excluded_gap());
            let pieces: Vec<_> = slot
                .source
                .pieces()
                .iter()
                .map(|piece| format!("{}..{}", piece.range().start, piece.range().end))
                .collect();
            let enclosing = index.facts().slot_enclosing(owner).unwrap();
            format!(
                "slot {} {} in {}",
                slot.ordinal,
                pieces.join(","),
                owner_text(index, enclosing)
            )
        }
        Owner::File(file) => {
            let (kind, range) = site(file.node());
            format!("file {kind} {}..{}", range.start, range.end)
        }
        other => panic!("a node, a field slot or the file, found {other:?}"),
    }
}

/// One association where the view does not reach: the position of its document, its
/// resolution, its owners, its binding and its lexical scope. It may have no owner.
#[cfg(feature = "go")]
type Edge = (
    usize,
    Resolution,
    Vec<String>,
    (&'static str, Range<usize>),
    (&'static str, Range<usize>),
);

#[cfg(feature = "go")]
fn edges(index: &DocumentationIndex<'_>) -> Vec<Edge> {
    let facts = index.facts();
    index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.convention.name(), "GoDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            let owners = (0..attachment.owner_count())
                .map(|at| owner_text(index, facts.associated_owner(association, at).unwrap()))
                .collect();
            (
                index
                    .documents()
                    .position(|known| known == document)
                    .unwrap(),
                attachment.resolution,
                owners,
                site(attachment.binding.unwrap().node()),
                site(attachment.lexical_scope.node()),
            )
        })
        .collect()
}

/// One pending assessment: its reason, its node and the position of its document.
#[cfg(feature = "go")]
type Pending = (&'static str, (&'static str, Range<usize>), Option<usize>);

/// The pending assessments in the order of their nodes; the contract states no order
/// for them. Each one here is on the owner axis.
#[cfg(feature = "go")]
fn pending<'i, 's: 'i>(
    index: &'i DocumentationIndex<'s>,
    found: impl Iterator<Item = FactRef<'i, Assessment<'s>>>,
) -> Vec<Pending> {
    let mut all: Vec<Pending> = found
        .map(|assessment| {
            assert_eq!(assessment.get().axis, AssessmentAxis::Owner);
            let document = index.assessed_document(assessment).unwrap();
            (
                assessment.get().reason,
                site(assessment.get().node.unwrap().node()),
                document.map(|document| {
                    index
                        .documents()
                        .position(|known| known == document)
                        .unwrap()
                }),
            )
        })
        .collect();
    all.sort_by_key(|(reason, (_, range), document)| (range.start, range.end, *reason, *document));
    all
}

/// Every retained candidate as disposition and byte range.
#[cfg(feature = "go")]
fn kept(index: &DocumentationIndex<'_>) -> Vec<(CandidateDisposition, Range<usize>)> {
    index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect()
}

#[cfg(feature = "go")]
#[test]
fn go_retained_candidates_stay_in_source_order() {
    // Two comments that document nothing: one follows code on its row, the other is
    // above a blank row.
    let unbound: &[u8] = b"package p\nvar x = 1 // t\n\n// a\n\nfunc F() {}\n";
    let mut expected = View::default();
    assert_eq!(view(unbound), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 20..24),
        (CandidateDisposition::Unattached, 26..30),
    ]);
    assert_eq!(retained(unbound, Retain::Unbound), expected);
    // A comment above a blank row and the directive of a selected run: one option keeps
    // each, and the list is in source order whatever the dispositions are.
    let mixed: &[u8] = b"package p\n// a\n\n// b\n//go:generate tool\nfunc F() {}\n";
    let mut expected = owned(
        comment(16..20, "b", [(0..1, Copied(19..20))]),
        "function_declaration",
        40..51,
    );
    assert_eq!(view(mixed), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 10..14),
        (CandidateDisposition::Directive, 21..39),
    ]);
    assert_eq!(retained(mixed, Retain::Both), expected);
}

/// Asserts that the source publishes nothing by default, that none of the nodes is an
/// owner, and that each comment is kept on request, in order. The kept comments show
/// that the collector saw them, so the empty view is an answer.
#[cfg(feature = "go")]
fn assert_owns_nothing(source: &[u8], nodes: &[(&str, Range<usize>)], comments: &[Range<usize>]) {
    let name = source.escape_ascii();
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    let mut expected = View::default();
    assert_eq!(agreed(&both), expected, "{name}");
    for index in &both {
        for (kind, range) in nodes {
            assert_eq!(
                index
                    .node_projection(node_at(&parser, kind, range.clone()))
                    .err(),
                Some(OwnerError::UnsupportedOwner),
                "{kind} {range:?} in {name}"
            );
        }
    }
    expected.evidence.extend(
        comments
            .iter()
            .map(|range| (CandidateDisposition::Unattached, range.clone())),
    );
    assert_eq!(retained(source, Retain::Unbound), expected, "{name}");
}

#[cfg(feature = "go")]
#[test]
fn go_members_declared_in_a_function_body_own_nothing() {
    let cases: [(&[u8], &[(&str, Range<usize>)], &[Range<usize>]); 2] = [
        (
            b"package p\nfunc F() {\ntype T struct {\n// a\nX int\n}\n}\n",
            &[("field_declaration", 42..47)],
            &[37..41],
        ),
        (
            b"package p\nfunc F() {\ntype I interface {\n// m\nM()\n// e\nio.Reader\n}\n}\n",
            &[("method_elem", 45..48), ("type_elem", 54..63)],
            &[40..44, 49..53],
        ),
    ];
    for (source, members, comments) in cases {
        assert_owns_nothing(source, members, comments);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_elements_without_a_leading_name_and_signature_parts_own_nothing() {
    // An interface element that does not open with a name, a type argument and a
    // parameter: Go gives none of them a lead comment, and none is an owner here.
    let cases: [(&[u8], &[(&str, Range<usize>)], &[Range<usize>]); 4] = [
        (
            b"package p\ntype I interface {\n// u\n~int\n}\n",
            &[("type_elem", 34..38)],
            &[29..33],
        ),
        (
            b"package p\ntype I interface {\n// a\n~int | string\n// b\n*T\n// c\n[]int\n}\n",
            &[
                ("type_elem", 34..47),
                ("type_elem", 53..55),
                ("type_elem", 61..66),
            ],
            &[29..33, 48..52, 56..60],
        ),
        (
            b"package p\nvar v Map[\n// c\nstring, int]\n",
            &[("type_elem", 26..32), ("type_elem", 34..37)],
            &[21..25],
        ),
        (
            b"package p\nfunc F(\n// p\nx int) {}\n",
            &[("parameter_declaration", 23..28)],
            &[18..22],
        ),
    ];
    for (source, parts, comments) in cases {
        assert_owns_nothing(source, parts, comments);
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_member_with_a_syntax_error_stays_a_pending_owner() {
    // Each member holds an error of its own: an `ERROR` node in the first four, a
    // missing element type in the last. The type that holds the member has the error
    // too, so the declaration and its spec are pending with it.
    struct Case<'a> {
        source: &'a [u8],
        member: (&'static str, Range<usize>),
        // The error inside the member: an `ERROR` node, or the zero-width missing node.
        error: (&'static str, Range<usize>),
        comment: Range<usize>,
        text: &'a str,
        scope: (&'static str, Range<usize>),
        declaration: Range<usize>,
        spec: Range<usize>,
        // The comment inside the broken method, kept on request.
        inside: &'a [Range<usize>],
    }
    let cases = [
        Case {
            source: b"package p\ntype S struct {\n// a\nX %% int\n}\n",
            member: ("field_declaration", 31..39),
            error: ("ERROR", 33..35),
            comment: 26..30,
            text: "a",
            scope: ("field_declaration_list", 24..41),
            declaration: 10..41,
            spec: 15..41,
            inside: &[],
        },
        Case {
            source: b"package p\ntype S struct {\n// e\nio.%%Reader\n}\n",
            member: ("field_declaration", 31..42),
            error: ("ERROR", 34..36),
            comment: 26..30,
            text: "e",
            scope: ("field_declaration_list", 24..44),
            declaration: 10..44,
            spec: 15..44,
            inside: &[],
        },
        Case {
            source: b"package p\ntype I interface {\n// m\nM(\n// e\nio.Reader\n}\n",
            member: ("method_elem", 34..51),
            error: (")", 51..51),
            comment: 29..33,
            text: "m",
            scope: ("interface_type", 17..53),
            declaration: 10..53,
            spec: 15..53,
            inside: &[37..41],
        },
        Case {
            source: b"package p\ntype I interface {\n// e\nio.%%Reader\n}\n",
            member: ("type_elem", 34..45),
            error: ("ERROR", 37..39),
            comment: 29..33,
            text: "e",
            scope: ("interface_type", 17..47),
            declaration: 10..47,
            spec: 15..47,
            inside: &[],
        },
        Case {
            source: b"package p\ntype S struct {\n// d\nX []\n}\n",
            member: ("field_declaration", 31..35),
            error: ("type_identifier", 35..35),
            comment: 26..30,
            text: "d",
            scope: ("field_declaration_list", 24..37),
            declaration: 10..37,
            spec: 15..37,
            inside: &[],
        },
    ];
    for case in cases {
        let name = case.source.escape_ascii();
        let parser = AnyParser::parse(LANG::Go, case.source.to_vec(), None, None).unwrap();
        let member = node_at(&parser, case.member.0, case.member.1.clone());
        let error = node_at(&parser, case.error.0, case.error.1.clone());
        assert!(member.has_error(), "{name}");
        assert!(
            error.is_error() || error.is_missing() || error.has_error(),
            "{name}"
        );
        assert!(
            member.start_byte() <= error.start_byte() && error.end_byte() <= member.end_byte(),
            "{name}"
        );
        let options = DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        };
        for index in &indexes(&parser, options) {
            assert_eq!(
                texts(index),
                [(vec![case.comment.clone()], case.text.to_owned())],
                "{name}"
            );
            // The error is the reason the stream gives, so the association exists; it
            // has no owner, because its target is pending.
            assert_eq!(
                edges(index),
                [(
                    0,
                    Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
                    vec![],
                    case.member.clone(),
                    case.scope.clone(),
                )],
                "{name}"
            );
            assert_eq!(
                pending(index, index.assessments()),
                [
                    (
                        NEEDS_PROJECTION,
                        ("type_declaration", case.declaration.clone()),
                        None
                    ),
                    (NEEDS_PROJECTION, ("type_spec", case.spec.clone()), None),
                    (NEEDS_PROJECTION, case.member.clone(), None),
                    (AWAITS_PROJECTION, case.member.clone(), Some(0)),
                ],
                "{name}"
            );
            let inside: Vec<_> = case
                .inside
                .iter()
                .map(|range| (CandidateDisposition::Unattached, range.clone()))
                .collect();
            assert_eq!(kept(index), inside, "{name}");
            assert!(index.node_projection(member).unwrap().pending(), "{name}");
            assert_eq!(
                index.node_owners(member).err(),
                Some(OwnerError::PendingOwner),
                "{name}"
            );
            // expected: the file and the package clause.
            assert_eq!(index.facts().owners().len(), 2, "{name}");
        }
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_member_grammar_shapes_the_unit_relies_on() {
    // The names of a field declaration are its children of kind `field_identifier`, in
    // order, and they are the children the grammar gives the field `name`. A field that
    // embeds a type has none, and `_` is a name like any other.
    let source: &[u8] = b"package p\ntype S struct {\nX, Y int `t`\n_ int\n*io.Reader\nN struct {\nA int\n}\n}\ntype I interface {\nM() int\nio.Reader\n}\n";
    let parser = clean(source);
    let members: [(&str, Range<usize>, &[Range<usize>]); 7] = [
        ("field_declaration", 26..38, &[26..27, 29..30]),
        ("field_declaration", 39..44, &[39..40]),
        ("field_declaration", 45..55, &[]),
        ("field_declaration", 56..74, &[56..57]),
        ("field_declaration", 67..72, &[67..68]),
        // The name of a method is a `field_identifier` as well, so the kind of the
        // member decides whether such a child names a field.
        ("method_elem", 96..103, &[96..97]),
        ("type_elem", 104..113, &[]),
    ];
    for (kind, range, names) in members {
        let member = node_at(&parser, kind, range.clone());
        let mut cursor = member.walk();
        let mut by_field = Vec::new();
        let mut by_kind = Vec::new();
        assert!(cursor.goto_first_child());
        loop {
            if cursor.field_name() == Some("name") {
                by_field.push(cursor.node().byte_range());
            }
            if cursor.node().kind() == "field_identifier" {
                by_kind.push(cursor.node().byte_range());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        assert_eq!(by_kind, names, "{kind} {range:?}");
        assert_eq!(by_field, names, "{kind} {range:?}");
    }
    assert_eq!(&source[39..40], b"_");
    // expected: the seven members above are every member of the source.
    assert_eq!(
        parser
            .root_node()
            .preorder()
            .filter(|found| matches!(
                found.kind(),
                "field_declaration" | "method_elem" | "type_elem"
            ))
            .count(),
        7
    );

    // A comment between the parts of a member is a child of the member. Only a block
    // comment that holds a line feed ends on a later row than it starts on; a carriage
    // return alone is no line end in Go, and the grammar counts no row for it.
    let inside: [(&[u8], Range<usize>, (&str, Range<usize>), bool); 6] = [
        (
            b"package p\ntype S struct {\n// d\nX /* a\n b */ int\n}\n",
            33..43,
            ("field_declaration", 31..47),
            true,
        ),
        (
            b"package p\ntype I interface {\n// m\nM() /* a\n b */ int\n}\n",
            38..48,
            ("method_elem", 34..52),
            true,
        ),
        (
            b"package p\ntype I interface {\n// e\nA | /* a\n b */ B\n}\n",
            38..48,
            ("type_elem", 34..50),
            true,
        ),
        (
            b"package p\ntype S struct {\n// d\nX, /* c */ Y int\n}\n",
            34..41,
            ("field_declaration", 31..47),
            false,
        ),
        (
            b"package p\ntype S struct {\n// a\nX,\n// b\nY int\n}\n",
            34..38,
            ("field_declaration", 31..44),
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nX /* a\r b */ int\n}\n",
            33..43,
            ("field_declaration", 31..47),
            false,
        ),
    ];
    for (source, comment, member, breaks) in inside {
        let parser = clean(source);
        let comment = node_at(&parser, "comment", comment);
        assert_eq!(
            comment.parent().map(site),
            Some(member),
            "{}",
            source.escape_ascii()
        );
        assert_eq!(
            comment.start_position().row != comment.end_position().row,
            breaks,
            "{}",
            source.escape_ascii()
        );
    }

    // A comment inside a function type is a child of that type and not of the member.
    let parser = clean(b"package p\ntype S struct {\n// d\nX func() /* a\n b */ int\n}\n");
    let comment = node_at(&parser, "comment", 40..50);
    assert_eq!(comment.parent().map(site), Some(("function_type", 33..54)));
    assert_eq!(
        comment.parent().and_then(|found| found.parent()).map(site),
        Some(("field_declaration", 31..54))
    );

    // A line feed after a name ends the member, as it does in Go: two declarations that
    // each embed a type, and no field named `X`.
    let parser = clean(b"package p\ntype S struct {\n// d\nX\nint\n}\n");
    let fields = named_in(node_at(&parser, "field_declaration_list", 24..38));
    assert_eq!(
        shapes(&fields),
        [
            ("comment", 26..30),
            ("field_declaration", 31..32),
            ("field_declaration", 33..36),
        ]
    );
    assert_eq!(shapes(&named_in(fields[1])), [("type_identifier", 31..32)]);
    assert_eq!(shapes(&named_in(fields[2])), [("type_identifier", 33..36)]);
}

/// The tokens below a node, in source order, each with whether the grammar names it: a
/// node without children, or a string literal as one token. Comments are left out.
#[cfg(feature = "go")]
fn tokens(node: tree_sitter::Node<'_>) -> Vec<(&'static str, bool)> {
    let mut found = Vec::new();
    let mut open = vec![node];
    while let Some(next) = open.pop() {
        let string = matches!(
            next.kind(),
            "raw_string_literal" | "interpreted_string_literal"
        );
        if next.kind() == "comment" {
            continue;
        }
        if next.child_count() == 0 || string {
            found.push((next.kind(), next.is_named()));
        } else {
            let children: Vec<_> = next.children(&mut next.walk()).collect();
            open.extend(children.into_iter().rev());
        }
    }
    found
}

/// The kind of every child of a node, named or not, in order.
#[cfg(feature = "go")]
fn child_kinds(node: tree_sitter::Node<'_>) -> Vec<&'static str> {
    node.children(&mut node.walk())
        .map(|child| child.kind())
        .collect()
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_token_shapes_the_line_end_rule_relies_on() {
    // Go ends a line after an identifier, a literal, `)`, `]` or `}`. In the pinned
    // grammar the identifiers and the literals are exactly the tokens it names: every
    // keyword, operator and bracket is a token without a name.
    let source: &[u8] = b"package p\ntype S struct {\nX [iota + len(nil)]int `a`\nY [true]map[string]*pkg.T \"b\"\nZ [1.5 + 2i + 'c' + 3]chan<- func(a ...int) (r bool)\n_ struct{}\nW interface{ ~int | G[int, string] }\nV [false]int\n}\n";
    let parser = clean(source);
    let list = node_at(&parser, "field_declaration_list", 24..198);
    let mut named = std::collections::BTreeSet::new();
    let mut unnamed = std::collections::BTreeSet::new();
    for (kind, is_named) in tokens(list) {
        if is_named {
            named.insert(kind);
        } else {
            unnamed.insert(kind);
        }
    }
    assert_eq!(
        named.into_iter().collect::<Vec<_>>(),
        [
            "false",
            "field_identifier",
            "float_literal",
            "identifier",
            "imaginary_literal",
            "int_literal",
            "interpreted_string_literal",
            "iota",
            "nil",
            "package_identifier",
            "raw_string_literal",
            "rune_literal",
            "true",
            "type_identifier",
        ]
    );
    assert_eq!(
        unnamed.into_iter().collect::<Vec<_>>(),
        [
            "(",
            ")",
            "*",
            "+",
            ",",
            ".",
            "...",
            "<-",
            "[",
            "]",
            "chan",
            "func",
            "interface",
            "map",
            "struct",
            "{",
            "|",
            "}",
            "~",
        ]
    );

    // A string literal is a node with children, also when it is empty (an empty raw
    // string has a content node without a byte), so it is no token of its own accord;
    // the rule takes it as one. A raw string may span rows.
    let source: &[u8] = b"package p\ntype S struct {\nX int `a\nb`\nY int \"\"\nZ int ``\n}\n";
    let parser = clean(source);
    let strings: [(&str, Range<usize>, &[&str]); 3] = [
        (
            "raw_string_literal",
            32..37,
            &["`", "raw_string_literal_content", "`"],
        ),
        ("interpreted_string_literal", 44..46, &["\"", "\""]),
        (
            "raw_string_literal",
            53..55,
            &["`", "raw_string_literal_content", "`"],
        ),
    ];
    for (kind, range, children) in strings {
        let string = node_at(&parser, kind, range);
        assert!(string.is_named());
        assert_eq!(child_kinds(string), children);
    }
    let spanning = node_at(&parser, "raw_string_literal", 32..37);
    assert_eq!(
        (spanning.start_position().row, spanning.end_position().row),
        (2, 3)
    );

    // The members of a nested struct or interface type and the statements of a function
    // literal stand between a `{` and a `}` that are children of one node; in an
    // interface type the keyword is a child of that node as well.
    let source: &[u8] = b"package p\ntype S struct {\nA struct {\nX int\n}\nI interface {\nM()\n}\nF [len(func() int {\nreturn 1\n}())]int\n}\n";
    let parser = clean(source);
    let bodies: [(&str, Range<usize>, &[&str]); 3] = [
        (
            "field_declaration_list",
            35..44,
            &["{", "field_declaration", "}"],
        ),
        (
            "interface_type",
            47..64,
            &["interface", "{", "method_elem", "}"],
        ),
        ("block", 83..95, &["{", "statement_list", "}"]),
    ];
    for (kind, range, children) in bodies {
        assert_eq!(child_kinds(node_at(&parser, kind, range)), children);
    }

    // After the `]` of an array length the grammar reads on past a line end, where Go
    // ends the line: one field declaration over two rows.
    let parser = clean(b"package p\ntype S struct {\n// d\nX [2]\nint\n}\n");
    let field = node_at(&parser, "field_declaration", 31..40);
    assert_eq!(
        shapes(&named_in(field)),
        [("field_identifier", 31..32), ("array_type", 33..40)]
    );
    let array = node_at(&parser, "array_type", 33..40);
    assert_eq!(
        child_kinds(array),
        ["[", "int_literal", "]", "type_identifier"]
    );
    let close = array.child(2).unwrap();
    let element = array.child(3).unwrap();
    assert_eq!(
        (close.end_position().row, element.start_position().row),
        (3, 4)
    );

    // With a name between the brackets the grammar reads an embedded generic type, which
    // ends at the line end as in Go: two field declarations.
    let parser = clean(b"package p\ntype S struct {\n// d\nX [N]\nT\n}\n");
    let list = node_at(&parser, "field_declaration_list", 24..40);
    assert_eq!(
        shapes(&named_in(list)),
        [
            ("comment", 26..30),
            ("field_declaration", 31..36),
            ("field_declaration", 37..38),
        ]
    );
    let embedded = node_at(&parser, "field_declaration", 31..36);
    assert_eq!(shapes(&named_in(embedded)), [("generic_type", 31..36)]);

    // A comment after the parameters of a function type is a child of that type, below
    // the member, and the result after it is read into the same type.
    let parser =
        clean(b"package p\ntype I interface {\n// d\nE | func() /* stop\n here */ B | C\n}\n");
    let inside = node_at(&parser, "comment", 45..61);
    let function = inside.parent().unwrap();
    assert_eq!(site(function), ("function_type", 38..63));
    assert_eq!(site(function.parent().unwrap()), ("type_elem", 34..67));
}

/// A tree parsed with those included ranges. Only `collect` takes such a tree.
#[cfg(feature = "go")]
fn restricted(source: &[u8], included: &[Range<usize>]) -> tree_sitter::Tree {
    // The row is the number of line feeds before the offset, and the column the number
    // of bytes after the last of them.
    let point = |offset: usize| {
        let mut rows = source[..offset].split(|byte| *byte == b'\n');
        let last = rows.next_back().unwrap();
        tree_sitter::Point::new(rows.count(), last.len())
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
        .set_language(&LANG::Go.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

/// The one node of that kind and byte range in a tree that no `AnyParser` holds.
#[cfg(feature = "go")]
fn node_in<'t>(
    tree: &'t tree_sitter::Tree,
    kind: &str,
    range: Range<usize>,
) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(current) = stack.pop() {
        if current.kind() == kind && current.byte_range() == range {
            found.push(current);
        }
        stack.extend(current.children(&mut current.walk()));
    }
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

/// The owners a node projects to, as text.
#[cfg(feature = "go")]
fn owners_of(index: &DocumentationIndex<'_>, node: tree_sitter::Node<'_>) -> Vec<String> {
    index
        .node_owners(node)
        .unwrap()
        .map(|owner| owner_text(index, owner))
        .collect()
}

#[cfg(feature = "go")]
#[test]
fn go_names_of_a_field_declaration_each_own_a_field_slot() {
    let fields: [(&[u8], Doc, Range<usize>, &[Range<usize>]); 6] = [
        (
            b"package p\ntype S struct {\n// both\nX, Y int\n}\n",
            comment(26..33, "both", [(0..4, Copied(29..33))]),
            34..42,
            &[34..35, 37..38],
        ),
        (
            b"package p\ntype S struct {\n// audit\nX int\n}\n",
            comment(26..34, "audit", [(0..5, Copied(29..34))]),
            35..40,
            &[35..36],
        ),
        // A tag is no name.
        (
            b"package p\ntype S struct {\n// d\nX, Y int `json:\"x\"`\n}\n",
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            31..50,
            &[31..32, 34..35],
        ),
        // A line feed after the comma does not end the declaration.
        (
            b"package p\ntype S struct {\n// d\nX,\nY int\n}\n",
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            31..39,
            &[31..32, 34..35],
        ),
        (
            b"package p\r\ntype S struct {\r\n// both\r\nX, Y int\r\n}\r\n",
            comment(28..35, "both", [(0..4, Copied(31..35))]),
            37..45,
            &[37..38, 40..41],
        ),
        // A block comment that ends on the row above the field documents it.
        (
            b"package p\ntype S struct {\n/* a\n b */\nX int\n}\n",
            comment(
                26..36,
                "a\n b",
                [
                    (0..1, Copied(29..30)),
                    (1..2, Copied(30..31)),
                    (2..4, Copied(31..33)),
                ],
            ),
            37..42,
            &[37..38],
        ),
    ];
    for (source, document, declaration, names) in fields {
        assert_eq!(
            view(source),
            named(document, declaration, names),
            "{}",
            source.escape_ascii()
        );
    }

    // Two documents in one struct, and `_` is a name like any other.
    assert_eq!(
        view(b"package p\ntype S struct {\n// pad\n_ int\n// two\n_, X int\n}\n"),
        View {
            slots: vec![(0, vec![33..34]), (1, vec![46..47, 49..50])],
            ..owned_in_order(
                [
                    comment(26..32, "pad", [(0..3, Copied(29..32))]),
                    comment(39..45, "two", [(0..3, Copied(42..45))]),
                ],
                [("field_declaration", 33..38), ("field_declaration", 46..54)]
            )
        }
    );

    // A directive above the document is no part of it, and is kept on request.
    assert_default_then_retained(
        b"package p\ntype S struct {\n//go:notinheap\n// audit\nX int\n}\n",
        named(
            comment(41..49, "audit", [(0..5, Copied(44..49))]),
            50..55,
            &[50..51],
        ),
        Retain::Directives,
        (CandidateDisposition::Directive, 26..40),
    );

    // The slots are spelled by the names and by nothing else of the declaration.
    let parser = clean(b"package p\ntype S struct {\n// both\nX, Y int\n}\n");
    for index in &indexes(&parser, DocumentationOptions::default()) {
        let association = index.associations().next().unwrap();
        let spelled: Vec<_> = (0..association.get().owner_count())
            .map(|at| {
                let owner = index.facts().associated_owner(association, at).unwrap();
                let Owner::Slot(slot) = owner.get() else {
                    panic!("a field slot, found {:?}", owner.get())
                };
                slot.source.pieces()[0].bytes().to_vec()
            })
            .collect();
        assert_eq!(spelled, [b"X".to_vec(), b"Y".to_vec()]);
        // expected: the file, the package clause, the type declaration and its spec,
        // the field declaration, and one slot for each of the two names.
        assert_eq!(index.facts().owners().len(), 7);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_inventory_row_gives_one_document_to_both_names() {
    // The source of the case inventory, which has no final line end. The pinned grammar
    // flags the root of such a file and no node below it.
    let parser = parsed(b"package p\ntype S struct {\n// both\nX, Y int\n}");
    assert!(parser.root_node().has_error());
    assert_eq!(
        agreed(&indexes(&parser, DocumentationOptions::default())),
        named(
            comment(26..33, "both", [(0..4, Copied(29..33))]),
            34..42,
            &[34..35, 37..38],
        )
    );
}

#[cfg(feature = "go")]
#[test]
fn go_field_queries_answer_per_slot_and_per_declaration() {
    let parser = clean(b"package p\ntype S struct {\n// audit\nX int\n}\n");
    let field = node_at(&parser, "field_declaration", 35..40);
    for index in &indexes(&parser, DocumentationOptions::default()) {
        let association = index.associations().next().unwrap();
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let projection = index.node_projection(field).unwrap();
        assert!(!projection.pending());
        assert!(projection.owners().eq([owner]));
        assert!(index.node_owners(field).unwrap().eq([owner]));
        let query = index.node_associations(field).unwrap();
        assert!(!query.projection_pending());
        assert!(query.associations().eq([association]));
        assert_eq!(query.assessments().count(), 0);
        let slot = index.owner_associations(owner).unwrap();
        assert!(slot.associations().eq([association]));
        // The declaration is an owner of its own; the document is not listed under it.
        let enclosing = index.facts().slot_enclosing(owner).unwrap();
        assert_eq!(owner_text(index, enclosing), "field_declaration 35..40");
        let declaration = index.owner_associations(enclosing).unwrap();
        assert_eq!(declaration.associations().len(), 0);
        assert_eq!(declaration.assessments().count(), 0);
        // The name and the type of the field are no owners.
        for (kind, range) in [("field_identifier", 35..36), ("type_identifier", 37..40)] {
            let part = node_at(&parser, kind, range);
            assert_eq!(
                index.node_projection(part).err(),
                Some(OwnerError::UnsupportedOwner),
                "{kind}"
            );
            assert_eq!(
                index.node_associations(part).err(),
                Some(OwnerError::UnsupportedOwner),
                "{kind}"
            );
            assert_eq!(
                index.node_owners(part).err(),
                Some(OwnerError::UnsupportedOwner),
                "{kind}"
            );
        }
    }

    let parser = clean(b"package p\ntype S struct {\n// both\nX, Y int\n}\n");
    let field = node_at(&parser, "field_declaration", 34..42);
    for index in &indexes(&parser, DocumentationOptions::default()) {
        let association = index.associations().next().unwrap();
        let owners = [
            index.facts().associated_owner(association, 0).unwrap(),
            index.facts().associated_owner(association, 1).unwrap(),
        ];
        assert_ne!(owners[0], owners[1]);
        // Two names: the declaration has no single owner to answer for.
        assert_eq!(
            index.node_associations(field).err(),
            Some(OwnerError::MultipleOwners)
        );
        assert!(index.node_owners(field).unwrap().eq(owners));
        let projection = index.node_projection(field).unwrap();
        assert!(!projection.pending());
        assert!(projection.owners().eq(owners));
        assert_eq!(projection.assessments().count(), 0);
        for owner in owners {
            let query = index.owner_associations(owner).unwrap();
            assert!(query.associations().eq([association]));
            assert_eq!(query.assessments().count(), 0);
        }
        for (kind, range) in [("field_identifier", 34..35), (",", 35..36)] {
            assert_eq!(
                index.node_projection(node_at(&parser, kind, range)).err(),
                Some(OwnerError::UnsupportedOwner),
                "{kind}"
            );
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_fields_of_a_nested_struct_own_slots_in_their_own_declaration() {
    let source: &[u8] =
        b"package p\ntype S struct {\n// same\nN struct {\n// same\nX, Y int\n}\n}\n";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    // The two documents have equal text, and each keeps its own lexical scope: the
    // field list its comment is in, which `published` reads from the tree.
    assert_eq!(
        agreed(&both),
        View {
            slots: vec![(0, vec![34..35]), (1, vec![53..54, 56..57])],
            ..owned_in_order(
                [
                    comment(26..33, "same", [(0..4, Copied(29..33))]),
                    comment(45..52, "same", [(0..4, Copied(48..52))]),
                ],
                [("field_declaration", 34..63), ("field_declaration", 53..61)]
            )
        }
    );
    for index in &both {
        let documents: Vec<_> = index.documents().collect();
        assert_ne!(documents[0], documents[1]);
        let associations: Vec<_> = index.associations().collect();
        // The slot of `N` answers with the outer document only.
        let outer = index.facts().associated_owner(associations[0], 0).unwrap();
        let query = index.owner_associations(outer).unwrap();
        assert!(query.associations().eq([associations[0]]));
        // The walk leaves the inner declaration first, so its owners come first.
        let owners: Vec<_> = index
            .facts()
            .owners()
            .map(|owner| owner_text(index, owner))
            .collect();
        assert_eq!(
            owners,
            [
                "file source_file 0..66",
                "package_clause 0..9",
                "type_declaration 10..65",
                "type_spec 15..65",
                "field_declaration 53..61",
                "slot 0 53..54 in field_declaration 53..61",
                "slot 1 56..57 in field_declaration 53..61",
                "field_declaration 34..63",
                "slot 0 34..35 in field_declaration 34..63",
            ]
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_embedded_field_owns_as_its_declaration() {
    assert_eq!(
        view(b"package p\ntype S struct {\n// audit\nio.Reader\n}\n"),
        owned(
            comment(26..34, "audit", [(0..5, Copied(29..34))]),
            "field_declaration",
            35..44
        )
    );
    // A type name, a pointer to one, a qualified one, an instantiated one, and one with
    // a tag: none has a name of its own, so none has a slot.
    let source: &[u8] =
        b"package p\ntype S struct {\n// a\nT\n// b\n*T\n// c\n*io.Reader\n// d\nG[int]\n// e\nT `json:\"t\"`\n}\n";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        owned_in_order(
            [
                comment(26..30, "a", [(0..1, Copied(29..30))]),
                comment(33..37, "b", [(0..1, Copied(36..37))]),
                comment(41..45, "c", [(0..1, Copied(44..45))]),
                comment(57..61, "d", [(0..1, Copied(60..61))]),
                comment(69..73, "e", [(0..1, Copied(72..73))]),
            ],
            [
                ("field_declaration", 31..32),
                ("field_declaration", 38..40),
                ("field_declaration", 46..56),
                ("field_declaration", 62..68),
                ("field_declaration", 74..86),
            ]
        )
    );
    for index in &both {
        // expected: the file, the package clause, the type declaration and its spec,
        // and the five field declarations; no slot.
        assert_eq!(index.facts().owners().len(), 9);
        let field = node_at(&parser, "field_declaration", 46..56);
        assert_eq!(owners_of(index, field), ["field_declaration 46..56"]);
        let query = index.node_associations(field).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 1);
        assert_eq!(query.assessments().count(), 0);
        // The type argument of the instantiated type is no owner.
        assert_eq!(
            index
                .node_projection(node_at(&parser, "type_elem", 64..67))
                .err(),
            Some(OwnerError::UnsupportedOwner)
        );
    }

    // A line feed after a lone name ends the declaration, in Go as in the grammar: `X`
    // and `int` are two embedded fields, and the document belongs to the first.
    let source: &[u8] = b"package p\ntype S struct {\n// d\nX\nint\n}\n";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        owned(
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            "field_declaration",
            31..32
        )
    );
    for index in &both {
        let second = node_at(&parser, "field_declaration", 33..36);
        assert_eq!(owners_of(index, second), ["field_declaration 33..36"]);
        let query = index.node_associations(second).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 0);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_interface_elements_own_as_their_nodes() {
    // A method and an embedded name own their comments. `~int` opens with no name:
    // its comment is in no document, and the element is no owner.
    let source: &[u8] =
        b"package p\ntype I interface {\n// m\nM()\n// e\nio.Reader\n// u\n~int\n}\n";
    assert_default_then_retained(
        source,
        owned_in_order(
            [
                comment(29..33, "m", [(0..1, Copied(32..33))]),
                comment(38..42, "e", [(0..1, Copied(41..42))]),
            ],
            [("method_elem", 34..37), ("type_elem", 43..52)],
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 53..57),
    );
    let parser = clean(source);
    for index in &indexes(&parser, DocumentationOptions::default()) {
        for (kind, range) in [("method_elem", 34..37), ("type_elem", 43..52)] {
            let element = node_at(&parser, kind, range.clone());
            assert_eq!(
                owners_of(index, element),
                [format!("{kind} {}..{}", range.start, range.end)]
            );
            let query = index.node_associations(element).unwrap();
            assert!(!query.projection_pending());
            assert_eq!(query.associations().len(), 1);
            assert_eq!(query.assessments().count(), 0);
        }
        assert_eq!(
            index
                .node_projection(node_at(&parser, "type_elem", 58..62))
                .err(),
            Some(OwnerError::UnsupportedOwner)
        );
        // The name of a method is no field slot.
        assert_eq!(
            index
                .node_projection(node_at(&parser, "field_identifier", 34..35))
                .err(),
            Some(OwnerError::UnsupportedOwner)
        );
        // expected: the file, the package clause, the type declaration and its spec,
        // the method and the embedded name.
        assert_eq!(index.facts().owners().len(), 6);
    }

    // A union, an instantiated type, a single name, a method with parameters and
    // results, and a type with two arguments.
    let source: &[u8] = b"package p\ntype I interface {\n// a\nint | string\n// c\nG[int]\n// d\nE\n// f\nM(x int) (int, error)\n// g\nMap[string, int]\n}\n";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        owned_in_order(
            [
                comment(29..33, "a", [(0..1, Copied(32..33))]),
                comment(47..51, "c", [(0..1, Copied(50..51))]),
                comment(59..63, "d", [(0..1, Copied(62..63))]),
                comment(66..70, "f", [(0..1, Copied(69..70))]),
                comment(93..97, "g", [(0..1, Copied(96..97))]),
            ],
            [
                ("type_elem", 34..46),
                ("type_elem", 52..58),
                ("type_elem", 64..65),
                ("method_elem", 71..92),
                ("type_elem", 98..114),
            ]
        )
    );
    for index in &both {
        // The arguments of an instantiated type are type elements of the grammar and
        // no owners.
        for range in [54..57, 102..108, 110..113] {
            assert_eq!(
                index
                    .node_projection(node_at(&parser, "type_elem", range.clone()))
                    .err(),
                Some(OwnerError::UnsupportedOwner),
                "{range:?}"
            );
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_members_own_wherever_their_type_is_written() {
    // A struct type of a variable, of a parameter, of a composite literal and of a
    // generic type, all outside a function body.
    let structs: [(&[u8], Range<usize>, usize, Range<usize>); 4] = [
        (
            b"package p\nvar v struct {\n// d\nX int\n}\n",
            25..29,
            28,
            30..35,
        ),
        (
            b"package p\nfunc F(s struct {\n// d\nX int\n}) {}\n",
            28..32,
            31,
            33..38,
        ),
        (
            b"package p\nvar v = struct {\n// d\nX int\n}{}\n",
            27..31,
            30,
            32..37,
        ),
        (
            b"package p\ntype S[T any] struct {\n// d\nX T\n}\n",
            33..37,
            36,
            38..41,
        ),
    ];
    for (source, fragment, text, field) in structs {
        let name = field.start..field.start + 1;
        assert_eq!(
            view(source),
            named(
                comment(fragment, "d", [(0..1, Copied(text..text + 1))]),
                field,
                &[name]
            ),
            "{}",
            source.escape_ascii()
        );
    }

    // An interface that is the type of a field: the field has a slot, and the method
    // inside owns as its node.
    assert_eq!(
        view(b"package p\ntype S struct {\n// f\nI interface {\n// m\nM()\n}\n}\n"),
        View {
            slots: vec![(0, vec![31..32])],
            ..owned_in_order(
                [
                    comment(26..30, "f", [(0..1, Copied(29..30))]),
                    comment(45..49, "m", [(0..1, Copied(48..49))]),
                ],
                [("field_declaration", 31..55), ("method_elem", 50..53)]
            )
        }
    );
    // An interface that constrains a type parameter.
    assert_eq!(
        view(b"package p\nfunc F[T interface {\n// c\nint\n}]() {}\n"),
        owned(
            comment(31..35, "c", [(0..1, Copied(34..35))]),
            "type_elem",
            36..39
        )
    );
    // A struct that is the type of a parameter of a method.
    assert_eq!(
        view(b"package p\ntype I interface {\n// m\nM(s struct {\n// d\nX int\n})\n}\n"),
        View {
            slots: vec![(1, vec![52..53])],
            ..owned_in_order(
                [
                    comment(29..33, "m", [(0..1, Copied(32..33))]),
                    comment(47..51, "d", [(0..1, Copied(50..51))]),
                ],
                [("method_elem", 34..60), ("field_declaration", 52..57)]
            )
        }
    );
}

#[cfg(feature = "go")]
#[test]
fn go_undocumented_members_have_owners_and_no_assessment() {
    let source: &[u8] =
        b"package p\ntype S struct {\nX int\nA, B string\nio.Reader\n}\ntype I interface {\nM()\nE\n}\n";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    // Nothing is published and nothing is pending.
    assert_eq!(agreed(&both), View::default());
    for index in &both {
        // A node owner is registered when the walk enters it, a member when the walk
        // leaves it.
        let owners: Vec<_> = index
            .facts()
            .owners()
            .map(|owner| owner_text(index, owner))
            .collect();
        assert_eq!(
            owners,
            [
                "file source_file 0..83",
                "package_clause 0..9",
                "type_declaration 10..55",
                "type_spec 15..55",
                "field_declaration 26..31",
                "slot 0 26..27 in field_declaration 26..31",
                "field_declaration 32..43",
                "slot 0 32..33 in field_declaration 32..43",
                "slot 1 35..36 in field_declaration 32..43",
                "field_declaration 44..53",
                "type_declaration 56..82",
                "type_spec 61..82",
                "method_elem 75..78",
                "type_elem 79..80",
            ]
        );
        for owner in index.facts().owners() {
            let query = index.owner_associations(owner).unwrap();
            assert_eq!(query.associations().len(), 0);
            assert_eq!(query.assessments().count(), 0);
        }
        let single: [(&str, Range<usize>); 4] = [
            ("field_declaration", 26..31),
            ("field_declaration", 44..53),
            ("method_elem", 75..78),
            ("type_elem", 79..80),
        ];
        for (kind, range) in single {
            let member = node_at(&parser, kind, range);
            let projection = index.node_projection(member).unwrap();
            assert!(!projection.pending(), "{kind}");
            assert_eq!(projection.assessments().count(), 0, "{kind}");
            let query = index.node_associations(member).unwrap();
            assert!(!query.projection_pending(), "{kind}");
            assert_eq!(query.associations().len(), 0, "{kind}");
            assert_eq!(query.assessments().count(), 0, "{kind}");
        }
        let two = node_at(&parser, "field_declaration", 32..43);
        assert!(!index.node_projection(two).unwrap().pending());
        assert_eq!(
            index.node_associations(two).err(),
            Some(OwnerError::MultipleOwners)
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_comments_that_document_no_member_leave_it_its_owners() {
    // `// t` follows the field `X` on its row. It documents nothing, and `X` has its
    // slot all the same.
    let trailing: &[u8] = b"package p\ntype S struct {\nX int // t\n// d\nY int\n}\n";
    assert_default_then_retained(
        trailing,
        named(
            comment(37..41, "d", [(0..1, Copied(40..41))]),
            42..47,
            &[42..43],
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 32..36),
    );
    let parser = clean(trailing);
    for index in &indexes(&parser, DocumentationOptions::default()) {
        let field = node_at(&parser, "field_declaration", 26..31);
        assert_eq!(
            owners_of(index, field),
            ["slot 0 26..27 in field_declaration 26..31"]
        );
        let query = index.node_associations(field).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 0);
        assert_eq!(query.assessments().count(), 0);
    }

    // A comment after the field on its row and a comment between two blank rows: two
    // candidates, no document, and both fields have their slots.
    let detached: &[u8] = b"package p\ntype S struct {\nX int // t\n\n// lost\n\nY int\n}\n";
    let mut expected = View::default();
    assert_eq!(view(detached), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 32..36),
        (CandidateDisposition::Unattached, 38..45),
    ]);
    assert_eq!(retained(detached, Retain::Unbound), expected);
    let parser = clean(detached);
    for index in &indexes(&parser, DocumentationOptions::default()) {
        for (field, name) in [(26..31, "26..27"), (47..52, "47..48")] {
            let text = format!(
                "slot 0 {name} in field_declaration {}..{}",
                field.start, field.end
            );
            assert_eq!(
                owners_of(index, node_at(&parser, "field_declaration", field)),
                [text]
            );
        }
    }

    // A block comment that ends on the row of the field is no lead comment.
    assert_default_then_retained(
        b"package p\ntype S struct {\n/* a\n b */ X int\n}\n",
        View::default(),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 26..36),
    );

    // The same in an interface: `// t` follows the method on its row.
    assert_default_then_retained(
        b"package p\ntype I interface {\nM() // t\n// e\nio.Reader\n}\n",
        owned(
            comment(38..42, "e", [(0..1, Copied(41..42))]),
            "type_elem",
            43..52,
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 33..37),
    );
}

#[cfg(feature = "go")]
#[test]
fn go_comment_inside_a_member_documents_nothing() {
    // A comment between the parts of a member is a child of the member. It has no
    // target, and the member has its owners.
    assert_default_then_retained(
        b"package p\ntype S struct {\n// d\nX, /* c */ Y int\n}\n",
        named(
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            31..47,
            &[31..32, 42..43],
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 34..41),
    );
    assert_default_then_retained(
        b"package p\ntype S struct {\n// a\nX,\n// b\nY int\n}\n",
        named(
            comment(26..30, "a", [(0..1, Copied(29..30))]),
            31..44,
            &[31..32, 39..40],
        ),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 34..38),
    );
    // Two comments inside one method, kept in source order.
    let method: &[u8] = b"package p\ntype I interface {\n// m\nM( /* in */ ) /* out */ int\n}\n";
    let mut expected = owned(
        comment(29..33, "m", [(0..1, Copied(32..33))]),
        "method_elem",
        34..61,
    );
    assert_eq!(view(method), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 37..45),
        (CandidateDisposition::Unattached, 48..57),
    ]);
    assert_eq!(retained(method, Retain::Unbound), expected);
}

#[cfg(feature = "go")]
#[test]
fn go_member_with_a_comment_that_ends_a_line_stays_a_pending_owner() {
    // Go ends a line at a comment that holds a line break. In the first source Go reads
    // two embedded fields, `X` and `int`, and in the second a method without a result
    // and an embedded `int`; the grammar reads one member each time. In the last three
    // Go reads what the grammar reads, and the member stays pending all the same: the
    // rule looks at the comment and not at the token before it.
    let members: [(&[u8], &str, Range<usize>, Doc, Range<usize>); 5] = [
        (
            b"package p\ntype S struct {\n// d\nX /* a\n b */ int\n}\n",
            "field_declaration",
            31..47,
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            33..43,
        ),
        (
            b"package p\ntype I interface {\n// m\nM() /* a\n b */ int\n}\n",
            "method_elem",
            34..52,
            comment(29..33, "m", [(0..1, Copied(32..33))]),
            38..48,
        ),
        (
            b"package p\ntype I interface {\n// e\nA | /* a\n b */ B\n}\n",
            "type_elem",
            34..50,
            comment(29..33, "e", [(0..1, Copied(32..33))]),
            38..48,
        ),
        (
            b"package p\ntype S struct {\n// d\nX, /* a\n b */ Y int\n}\n",
            "field_declaration",
            31..50,
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            34..44,
        ),
        (
            b"package p\ntype S struct {\n// d\n* /* a\n b */ T\n}\n",
            "field_declaration",
            31..45,
            comment(26..30, "d", [(0..1, Copied(29..30))]),
            33..43,
        ),
    ];
    for (source, kind, range, document, inside) in members {
        let name = source.escape_ascii();
        // The document is published with its text and has no association.
        let expected = View {
            documents: vec![document],
            assessments: 2,
            ..View::default()
        };
        assert_default_then_retained(
            source,
            expected,
            Retain::Unbound,
            (CandidateDisposition::Unattached, inside),
        );
        let parser = clean(source);
        let member = node_at(&parser, kind, range.clone());
        for index in &indexes(&parser, DocumentationOptions::default()) {
            assert_eq!(
                pending(index, index.assessments()),
                [
                    (NEEDS_PROJECTION, (kind, range.clone()), None),
                    (AWAITS_PROJECTION, (kind, range.clone()), Some(0)),
                ],
                "{name}"
            );
            assert!(index.node_projection(member).unwrap().pending(), "{name}");
            assert_eq!(
                index.node_owners(member).err(),
                Some(OwnerError::PendingOwner),
                "{name}"
            );
            let query = index.node_associations(member).unwrap();
            assert!(query.projection_pending(), "{name}");
            assert_eq!(query.associations().len(), 0, "{name}");
            // expected: the file, the package clause, the type declaration and its
            // spec; the member is no owner.
            assert_eq!(index.facts().owners().len(), 4, "{name}");
        }
    }
}

/// Asserts that the member is a pending owner in both indexes, as at the evidence base.
/// Its document is published with its text and has no association, the member carries
/// both `Owner` assessments, and the index has the four owners around it and no other.
/// `inside` are the comments inside the member: the retention keeps each as an
/// unattached candidate, in order.
#[cfg(feature = "go")]
fn assert_stays_pending(
    source: &[u8],
    kind: &'static str,
    range: Range<usize>,
    document: Doc,
    inside: &[Range<usize>],
) {
    let name = source.escape_ascii();
    let mut expected = View {
        documents: vec![document],
        assessments: 2,
        ..View::default()
    };
    assert_eq!(view(source), expected, "{name}");
    expected.evidence = inside
        .iter()
        .map(|comment| (CandidateDisposition::Unattached, comment.clone()))
        .collect();
    assert_eq!(retained(source, Retain::Unbound), expected, "{name}");
    let parser = clean(source);
    let member = node_at(&parser, kind, range.clone());
    for index in &indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, (kind, range.clone()), None),
                (AWAITS_PROJECTION, (kind, range.clone()), Some(0)),
            ],
            "{name}"
        );
        assert!(index.node_projection(member).unwrap().pending(), "{name}");
        assert_eq!(
            index.node_owners(member).err(),
            Some(OwnerError::PendingOwner),
            "{name}"
        );
        let query = index.node_associations(member).unwrap();
        assert!(query.projection_pending(), "{name}");
        assert_eq!(query.associations().len(), 0, "{name}");
        // expected: the file, the package clause, the type declaration and its spec;
        // the member is no owner.
        assert_eq!(index.facts().owners().len(), 4, "{name}");
    }
}

#[cfg(feature = "go")]
#[test]
fn go_line_end_below_a_child_keeps_the_member_pending() {
    // The comment holds a line break and stands below a child of the member, after the
    // `)` of a function type. Go ends the line there, so it ends the member before the
    // comment (`X func()`, `M() func()`, `E | func()`, `X []func()`, `M() <-chan func()`,
    // `X *func()`, `X map[string]func()`) and reads what follows as one more embedded
    // member. The grammar reads one member to the end of the row. That node is not the
    // member Go reads, so it stays a pending owner.
    let members: [(&[u8], &str, Range<usize>, Range<usize>); 8] = [
        (
            b"package p\ntype S struct {\n// d\nX func() /* a\n b */ int\n}\n",
            "field_declaration",
            31..54,
            40..50,
        ),
        (
            b"package p\ntype I interface {\n// d\nM() func() /* a\n b */ int\n}\n",
            "method_elem",
            34..59,
            45..55,
        ),
        (
            b"package p\ntype I interface {\n// d\nE | func() /* a\n b */ int\n}\n",
            "type_elem",
            34..59,
            45..55,
        ),
        (
            b"package p\ntype I interface {\n// d\nE | func() /* stop\n here */ B | C\n}\n",
            "type_elem",
            34..67,
            45..61,
        ),
        (
            b"package p\ntype S struct {\n// d\nX []func() /* a\n b */ int\n}\n",
            "field_declaration",
            31..56,
            42..52,
        ),
        (
            b"package p\ntype I interface {\n// d\nM() <-chan func() /* a\n b */ int\n}\n",
            "method_elem",
            34..66,
            52..62,
        ),
        (
            b"package p\ntype S struct {\n// d\nX *func() /* a\n b */ int\n}\n",
            "field_declaration",
            31..55,
            41..51,
        ),
        (
            b"package p\ntype S struct {\n// d\nX map[string]func() /* a\n b */ int\n}\n",
            "field_declaration",
            31..65,
            51..61,
        ),
    ];
    for (source, kind, range, inside) in members {
        // expected: the comment `// d` ends one byte before the member starts.
        let above = range.start - 5..range.start - 1;
        let document = comment(
            above.clone(),
            "d",
            [(0..1, Copied(above.end - 1..above.end))],
        );
        assert_stays_pending(source, kind, range, document, &[inside]);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_line_end_after_a_closing_bracket_keeps_the_member_pending() {
    // After a `]` the grammar reads on past a line end where an element type has to
    // follow, and Go ends the line. In the first three sources Go reads two embedded
    // fields, `X[2]` or `X[N]` and the name on the next row, and no field named `X`;
    // the third has its line end in a comment below the array type, the second in a
    // `//` comment on one row. Go rejects the last four files. In each the node is not
    // a member Go reads, so it stays a pending owner.
    let members: [(&[u8], &str, Range<usize>, &[Range<usize>]); 7] = [
        (
            b"package p\ntype S struct {\n// d\nX [2]\nint\n}\n",
            "field_declaration",
            31..40,
            &[],
        ),
        (
            b"package p\ntype S struct {\n// d\nX [2] // c\nint\n}\n",
            "field_declaration",
            31..45,
            &[37..41],
        ),
        (
            b"package p\ntype S struct {\n// d\nX [N] /* a\n b */ T\n}\n",
            "field_declaration",
            31..49,
            &[37..47],
        ),
        (
            b"package p\ntype S struct {\n// d\nX map[string]\nint\n}\n",
            "field_declaration",
            31..48,
            &[],
        ),
        (
            b"package p\ntype S struct {\n// d\nX []\nint\n}\n",
            "field_declaration",
            31..39,
            &[],
        ),
        (
            b"package p\ntype I interface {\n// d\nM() [2]\nint\n}\n",
            "method_elem",
            34..45,
            &[],
        ),
        (
            b"package p\ntype S struct {\n// d\nX, Y [2]\nint\n}\n",
            "field_declaration",
            31..43,
            &[],
        ),
    ];
    for (source, kind, range, inside) in members {
        // expected: the comment `// d` ends one byte before the member starts.
        let above = range.start - 5..range.start - 1;
        let document = comment(
            above.clone(),
            "d",
            [(0..1, Copied(above.end - 1..above.end))],
        );
        assert_stays_pending(source, kind, range, document, inside);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_line_end_after_a_name_a_literal_or_a_brace_keeps_the_member_pending() {
    // The other tokens after which Go ends a line, each before a line end that the
    // grammar reads past: the `}` of a struct type in a map key, a parameter name, a
    // string literal of each kind and a number. Go rejects every one of these files, so
    // there is no member of Go to follow, and the member stays a pending owner.
    let members: [(&[u8], &str, Range<usize>, &[Range<usize>]); 5] = [
        (
            b"package p\ntype S struct {\n// d\nX map[struct{}\n]int\n}\n",
            "field_declaration",
            31..50,
            &[],
        ),
        (
            b"package p\ntype I interface {\n// d\nM(a /* x\n y */ int)\n}\n",
            "method_elem",
            34..53,
            &[38..48],
        ),
        (
            b"package p\ntype S struct {\n// d\nX [len(\"s\" /* a\n b */ )]int\n}\n",
            "field_declaration",
            31..58,
            &[42..52],
        ),
        (
            b"package p\ntype S struct {\n// d\nX [len(`s` /* a\n b */ )]int\n}\n",
            "field_declaration",
            31..58,
            &[42..52],
        ),
        (
            b"package p\ntype S struct {\n// d\nX [2 /* a\n b */ ]int\n}\n",
            "field_declaration",
            31..51,
            &[36..46],
        ),
    ];
    for (source, kind, range, inside) in members {
        // expected: the comment `// d` ends one byte before the member starts.
        let above = range.start - 5..range.start - 1;
        let document = comment(
            above.clone(),
            "d",
            [(0..1, Copied(above.end - 1..above.end))],
        );
        assert_stays_pending(source, kind, range, document, inside);
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_members_over_several_rows_keep_their_owners() {
    // Controls: a line end inside a member where Go ends no line, because the token
    // before it is none of an identifier, a literal, `)`, `]` and `}`, or because the
    // line end lies inside one token. Go reads each of these members as the grammar
    // does, and each keeps its owners.
    let d = || comment(26..30, "d", [(0..1, Copied(29..30))]);
    let e = || comment(29..33, "d", [(0..1, Copied(32..33))]);
    let plain: [(&[u8], View); 18] = [
        // A parameter list over rows.
        (
            b"package p\ntype I interface {\n// d\nM(\n\ta int,\n\tb int,\n) error\n}\n",
            owned(e(), "method_elem", 34..60),
        ),
        // A parameter list and a result list over rows.
        (
            b"package p\ntype S struct {\n// d\nF func(\n\ta int,\n) (\n\tint,\n\terror,\n)\n}\n",
            named(d(), 31..66, &[31..32]),
        ),
        // A type argument list over rows.
        (
            b"package p\ntype S struct {\n// d\nX G[\n\tint,\n\tstring,\n]\n}\n",
            named(d(), 31..52, &[31..32]),
        ),
        // A union with `|` at the row ends.
        (
            b"package p\ntype I interface {\n// d\nA |\n\tB |\n\tC\n}\n",
            owned(e(), "type_elem", 34..45),
        ),
        // Names after a comma.
        (
            b"package p\ntype S struct {\n// d\nX,\n\tY int\n}\n",
            named(d(), 31..40, &[31..32, 35..36]),
        ),
        // A tag in back quotes over rows: the line end is inside one token.
        (
            b"package p\ntype S struct {\n// d\nX int `a\nb`\n}\n",
            named(d(), 31..42, &[31..32]),
        ),
        // `X [N]` before the closing brace: an embedded generic type, for Go as well.
        (
            b"package p\ntype S struct {\n// d\nX [N]\n}\n",
            owned(d(), "field_declaration", 31..36),
        ),
        // The same before a name on the next row: two embedded fields, for Go as well.
        (
            b"package p\ntype S struct {\n// d\nX [N]\nT\n}\n",
            owned(d(), "field_declaration", 31..36),
        ),
        // After `chan`, `*`, `.`, `struct`, `map` and `func`.
        (
            b"package p\ntype S struct {\n// d\nX chan\nint\n}\n",
            named(d(), 31..41, &[31..32]),
        ),
        (
            b"package p\ntype S struct {\n// d\nX *\nint\n}\n",
            named(d(), 31..38, &[31..32]),
        ),
        (
            b"package p\ntype S struct {\n// d\nio.\nReader\n}\n",
            owned(d(), "field_declaration", 31..41),
        ),
        (
            b"package p\ntype S struct {\n// d\nX struct\n{}\n}\n",
            named(d(), 31..42, &[31..32]),
        ),
        (
            b"package p\ntype S struct {\n// d\nX map\n[string]int\n}\n",
            named(d(), 31..48, &[31..32]),
        ),
        (
            b"package p\ntype S struct {\n// d\nX func\n()\n}\n",
            named(d(), 31..40, &[31..32]),
        ),
        // A raw string over rows in an array length: the line end is inside one token,
        // and the token ends on the row of the `)` after it.
        (
            b"package p\ntype S struct {\n// d\nX [len(`a\nb`)]int\n}\n",
            named(d(), 31..48, &[31..32]),
        ),
        // The statements of a function literal are no tokens of the member.
        (
            b"package p\ntype S struct {\n// d\nX [len(func() []int {\nreturn nil\n}())]int\n}\n",
            named(d(), 31..72, &[31..32]),
        ),
        // After an operator.
        (
            b"package p\ntype S struct {\n// d\nX [1 +\n2]int\n}\n",
            named(d(), 31..43, &[31..32]),
        ),
        // A nested struct with documented members on their own rows.
        (
            b"package p\ntype S struct {\n// d\nA struct {\n// x\nX int\n// y\nY int\n}\n}\n",
            View {
                documents: vec![
                    d(),
                    comment(42..46, "x", [(0..1, Copied(45..46))]),
                    comment(53..57, "y", [(0..1, Copied(56..57))]),
                ],
                owners: vec![
                    (Resolution::Resolved, "field_declaration", 31..65),
                    (Resolution::Resolved, "field_declaration", 47..52),
                    (Resolution::Resolved, "field_declaration", 58..63),
                ],
                slots: vec![(0, vec![31..32]), (1, vec![47..48]), (2, vec![58..59])],
                ..View::default()
            },
        ),
    ];
    for (source, expected) in plain {
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
    }
    // A nested interface with documented members on their own rows.
    assert_eq!(
        view(b"package p\ntype S struct {\n// d\nI interface {\n// m\nM()\n// n\nN()\n}\n}\n"),
        View {
            documents: vec![
                d(),
                comment(45..49, "m", [(0..1, Copied(48..49))]),
                comment(54..58, "n", [(0..1, Copied(57..58))]),
            ],
            owners: vec![
                (Resolution::Resolved, "field_declaration", 31..64),
                (Resolution::Resolved, "method_elem", 50..53),
                (Resolution::Resolved, "method_elem", 59..62),
            ],
            slots: vec![(0, vec![31..32])],
            ..View::default()
        }
    );
    // A comment at a line end after a comma, and a comment that holds a line break
    // after a comma or an opening bracket: Go ends no line there.
    assert_default_then_retained(
        b"package p\ntype I interface {\n// d\nM(a int, // first\n\tb int) error\n}\n",
        owned(e(), "method_elem", 34..65),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 43..51),
    );
    assert_default_then_retained(
        b"package p\ntype S struct {\n// d\nF func(a, /* a\n b */ b int)\n}\n",
        named(d(), 31..58, &[31..32]),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 41..51),
    );
    assert_default_then_retained(
        b"package p\ntype S struct {\n// d\nX map[ /* a\n b */ string]int\n}\n",
        named(d(), 31..59, &[31..32]),
        Retain::Unbound,
        (CandidateDisposition::Unattached, 38..48),
    );
    // The second embedded field of `X [N]\nT` has its owner too.
    let parser = clean(b"package p\ntype S struct {\n// d\nX [N]\nT\n}\n");
    for index in &indexes(&parser, DocumentationOptions::default()) {
        assert_eq!(
            owners_of(index, node_at(&parser, "field_declaration", 37..38)),
            ["field_declaration 37..38"]
        );
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_nested_members_are_read_on_their_own() {
    // The members between the braces of a nested struct or interface type are no tokens
    // of the member around them. Each is read on its own: one may stay pending while
    // the other has its owners.
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };

    // The inner field ends early for Go (`X[2]` and `int`, two embedded fields); the
    // outer field is the member Go reads.
    let source: &[u8] = b"package p\ntype S struct {\n// d\nA struct {\n// i\nX [2]\nint\n}\n}\n";
    let parser = clean(source);
    for index in &indexes(&parser, options) {
        assert_eq!(
            texts(index),
            [
                (vec![26..30], "d".to_owned()),
                (vec![42..46], "i".to_owned())
            ]
        );
        assert_eq!(
            edges(index),
            [(
                0,
                Resolution::Resolved,
                vec!["slot 0 31..32 in field_declaration 31..58".to_owned()],
                ("field_declaration", 31..58),
                ("field_declaration_list", 24..60),
            )]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("field_declaration", 47..56), None),
                (AWAITS_PROJECTION, ("field_declaration", 47..56), Some(1)),
            ]
        );
        assert_eq!(kept(index), []);
        assert_eq!(
            index
                .node_owners(node_at(&parser, "field_declaration", 47..56))
                .err(),
            Some(OwnerError::PendingOwner)
        );
    }

    // The outer method ends early for Go, which reads `M() func()` and then the
    // interface as an embedded element; the method `N` inside is the member Go reads.
    let source: &[u8] = b"package p\ntype I interface {\n// d\nM() func() /* a\n b */ interface {\n// i\nN()\n}\n}\n";
    let parser = clean(source);
    for index in &indexes(&parser, options) {
        assert_eq!(
            texts(index),
            [
                (vec![29..33], "d".to_owned()),
                (vec![68..72], "i".to_owned())
            ]
        );
        assert_eq!(
            edges(index),
            [(
                1,
                Resolution::Resolved,
                vec!["method_elem 73..76".to_owned()],
                ("method_elem", 73..76),
                ("interface_type", 56..78),
            )]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("method_elem", 34..78), None),
                (AWAITS_PROJECTION, ("method_elem", 34..78), Some(0)),
            ]
        );
        assert_eq!(kept(index), [(CandidateDisposition::Unattached, 45..55)]);
    }

    // Go rejects the next two files, so these two outcomes are what the grammar reads
    // and no reading of Go. The outer field ends early and the inner field has its slot.
    let source: &[u8] = b"package p\ntype S struct {\n// d\nX [2]\nstruct {\n// i\nA int\n}\n}\n";
    let parser = clean(source);
    for index in &indexes(&parser, options) {
        assert_eq!(
            edges(index),
            [(
                1,
                Resolution::Resolved,
                vec!["slot 0 51..52 in field_declaration 51..56".to_owned()],
                ("field_declaration", 51..56),
                ("field_declaration_list", 44..58),
            )]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("field_declaration", 31..58), None),
                (AWAITS_PROJECTION, ("field_declaration", 31..58), Some(0)),
            ]
        );
    }
    // The inner method has a comment with a line break among its children and stays
    // pending; the outer field has its slot.
    let source: &[u8] =
        b"package p\ntype S struct {\n// d\nX interface {\n// i\nM /* a\n b */ ()\n}\n}\n";
    let parser = clean(source);
    for index in &indexes(&parser, options) {
        assert_eq!(
            edges(index),
            [(
                0,
                Resolution::Resolved,
                vec!["slot 0 31..32 in field_declaration 31..67".to_owned()],
                ("field_declaration", 31..67),
                ("field_declaration_list", 24..69),
            )]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("method_elem", 50..65), None),
                (AWAITS_PROJECTION, ("method_elem", 50..65), Some(1)),
            ]
        );
        assert_eq!(kept(index), [(CandidateDisposition::Unattached, 52..62)]);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_line_end_and_excluded_ranges() {
    let context = DocumentationContext::default();
    let unbound = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let source: &[u8] = b"package p\ntype S struct {\n// d\nX [2]\nint\n}\n";

    // No excluded byte: two adjacent included ranges meet at the line feed after `]`.
    // The field is whole, the rows of its tokens are those of the bytes, and Go ends it
    // at that line end: it stays pending, and its document has no association.
    let tree = restricted(source, &[0..36, 36..43]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, source, &context, &unbound).unwrap();
    let field = node_in(&tree, "field_declaration", 31..40);
    assert_eq!(texts(&index), [(vec![26..30], "d".to_owned())]);
    assert_eq!(edges(&index), []);
    assert_eq!(kept(&index), []);
    assert_eq!(
        pending(&index, index.assessments()),
        [
            (NEEDS_PROJECTION, ("field_declaration", 31..40), None),
            (AWAITS_PROJECTION, ("field_declaration", 31..40), Some(0)),
        ]
    );
    assert_eq!(
        index.node_owners(field).err(),
        Some(OwnerError::PendingOwner)
    );

    // The line feed excluded: the field holds an excluded byte, so the existing guard
    // keeps it pending before any token is read, as it keeps every node that holds the
    // byte, and the comment reaches no target.
    let tree = restricted(source, &[0..36, 37..43]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, source, &context, &unbound).unwrap();
    assert_eq!(texts(&index), []);
    assert_eq!(kept(&index), [(CandidateDisposition::Unattached, 26..30)]);
    assert_eq!(
        pending(&index, index.assessments()),
        [
            (NEEDS_PROJECTION, ("type_declaration", 10..42), None),
            (NEEDS_PROJECTION, ("type_spec", 15..42), None),
            (NEEDS_PROJECTION, ("struct_type", 17..42), None),
            (NEEDS_PROJECTION, ("field_declaration_list", 24..42), None),
            (NEEDS_PROJECTION, ("field_declaration", 31..40), None),
            (NEEDS_PROJECTION, ("array_type", 33..40), None),
        ]
    );

    // The control: two adjacent ranges meet at a line feed after `chan`, where Go ends
    // no line. The field has its slot and its document.
    let source: &[u8] = b"package p\ntype S struct {\n// d\nX chan\nint\n}\n";
    let tree = restricted(source, &[0..37, 37..44]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, source, &context, &unbound).unwrap();
    assert_eq!(
        edges(&index),
        [(
            0,
            Resolution::Resolved,
            vec!["slot 0 31..32 in field_declaration 31..41".to_owned()],
            ("field_declaration", 31..41),
            ("field_declaration_list", 24..43),
        )]
    );
    assert_eq!(pending(&index, index.assessments()), []);
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_member_beside_recovery_keeps_its_owners() {
    let unbound = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let parse = |source: &[u8]| AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    let boundary = Resolution::Unresolved(UnresolvedReason::GrammarBoundary);

    // An unclosed struct: the grammar gives an `ERROR` that holds the field. The field
    // has no error of its own and gets its slot. The `ERROR` above it makes the
    // association unresolved, and the slot is its candidate.
    let parser = parse(b"package p\ntype S struct {\n// d\nX int\n");
    let field = node_at(&parser, "field_declaration", 31..36);
    assert!(!field.has_error());
    assert_eq!(field.parent().map(site), Some(("ERROR", 10..37)));
    for index in &indexes(&parser, unbound) {
        assert_eq!(texts(index), [(vec![26..30], "d".to_owned())]);
        assert_eq!(
            edges(index),
            [(
                0,
                boundary,
                vec!["slot 0 31..32 in field_declaration 31..36".to_owned()],
                ("field_declaration", 31..36),
                ("ERROR", 10..37),
            )]
        );
        assert_eq!(pending(index, index.assessments()), []);
        assert_eq!(kept(index), []);
    }

    // The same for a method of an unclosed interface.
    let parser = parse(b"package p\ntype I interface {\n// m\nM()\n");
    let method = node_at(&parser, "method_elem", 34..37);
    assert!(!method.has_error());
    assert_eq!(method.parent().map(site), Some(("ERROR", 10..38)));
    for index in &indexes(&parser, unbound) {
        assert_eq!(texts(index), [(vec![29..33], "m".to_owned())]);
        assert_eq!(
            edges(index),
            [(
                0,
                boundary,
                vec!["method_elem 34..37".to_owned()],
                ("method_elem", 34..37),
                ("ERROR", 10..38),
            )]
        );
        assert_eq!(pending(index, index.assessments()), []);
    }

    // An error in one field leaves the fields beside it their slots. The broken field
    // stays pending, and so do the type declaration and its spec, which hold the error.
    let parser =
        parse(b"package p\ntype S struct {\n// a\nX int\n// b\nY %% int\n// c\nZ int\n}\n");
    let list = ("field_declaration_list", 24..63);
    let broken = node_at(&parser, "field_declaration", 42..50);
    assert!(broken.has_error());
    for index in &indexes(&parser, unbound) {
        assert_eq!(
            texts(index),
            [
                (vec![26..30], "a".to_owned()),
                (vec![37..41], "b".to_owned()),
                (vec![51..55], "c".to_owned()),
            ]
        );
        assert_eq!(
            edges(index),
            [
                (
                    0,
                    Resolution::Resolved,
                    vec!["slot 0 31..32 in field_declaration 31..36".to_owned()],
                    ("field_declaration", 31..36),
                    list.clone(),
                ),
                (
                    1,
                    boundary,
                    vec![],
                    ("field_declaration", 42..50),
                    list.clone()
                ),
                (
                    2,
                    Resolution::Resolved,
                    vec!["slot 0 56..57 in field_declaration 56..61".to_owned()],
                    ("field_declaration", 56..61),
                    list.clone(),
                ),
            ]
        );
        let above = [
            (NEEDS_PROJECTION, ("type_declaration", 10..63), None),
            (NEEDS_PROJECTION, ("type_spec", 15..63), None),
        ];
        let mut all = above.to_vec();
        all.extend([
            (NEEDS_PROJECTION, ("field_declaration", 42..50), None),
            (AWAITS_PROJECTION, ("field_declaration", 42..50), Some(1)),
        ]);
        assert_eq!(pending(index, index.assessments()), all);
        assert_eq!(
            index.node_owners(broken).err(),
            Some(OwnerError::PendingOwner)
        );
        // The query of a clean field reaches the pending declarations above it.
        let clean = node_at(&parser, "field_declaration", 31..36);
        let query = index.node_associations(clean).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 1);
        assert_eq!(pending(index, query.assessments()), above);
    }

    // An `ERROR` after a method on its row is a sibling of the method. Both elements
    // have no error of their own and own their comments.
    let parser = parse(b"package p\ntype I interface {\n// m\nM() %%\n// e\nio.Reader\n}\n");
    assert!(node_at(&parser, "ERROR", 38..40).is_error());
    for index in &indexes(&parser, unbound) {
        assert_eq!(
            edges(index),
            [
                (
                    0,
                    Resolution::Resolved,
                    vec!["method_elem 34..37".to_owned()],
                    ("method_elem", 34..37),
                    ("interface_type", 17..57),
                ),
                (
                    1,
                    Resolution::Resolved,
                    vec!["type_elem 46..55".to_owned()],
                    ("type_elem", 46..55),
                    ("interface_type", 17..57),
                ),
            ]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("type_declaration", 10..57), None),
                (NEEDS_PROJECTION, ("type_spec", 15..57), None),
            ]
        );
    }

    // What the rule gives for two recovered shapes, pinned as it is and not as a
    // design. An unclosed method `M(` is read as a type element `M` beside an `ERROR`,
    // and that element owns the comment.
    let parser = parse(b"package p\ntype I interface {\n// m\nM(\n}\n");
    assert!(node_at(&parser, "ERROR", 35..36).is_error());
    assert!(!node_at(&parser, "type_elem", 34..35).has_error());
    for index in &indexes(&parser, unbound) {
        assert_eq!(texts(index), [(vec![29..33], "m".to_owned())]);
        assert_eq!(
            edges(index),
            [(
                0,
                Resolution::Resolved,
                vec!["type_elem 34..35".to_owned()],
                ("type_elem", 34..35),
                ("interface_type", 17..38),
            )]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("type_declaration", 10..38), None),
                (NEEDS_PROJECTION, ("type_spec", 15..38), None),
            ]
        );
    }
    // A function after an unclosed struct: `func` is an `ERROR`, and `F` is read as a
    // field that embeds a type. It is an owner without a document; the comment above
    // `func` reaches no member.
    let parser = parse(b"package p\ntype S struct {\nX int\n// d\nfunc F() {}\n");
    assert!(node_at(&parser, "ERROR", 37..41).is_error());
    for index in &indexes(&parser, unbound) {
        assert_eq!(texts(index), []);
        assert_eq!(edges(index), []);
        assert_eq!(kept(index), [(CandidateDisposition::Unattached, 32..36)]);
        assert_eq!(
            owners_of(index, node_at(&parser, "field_declaration", 26..31)),
            ["slot 0 26..27 in field_declaration 26..31"]
        );
        assert_eq!(
            owners_of(index, node_at(&parser, "field_declaration", 42..43)),
            ["field_declaration 42..43"]
        );
        assert_eq!(
            pending(index, index.assessments()),
            [
                (NEEDS_PROJECTION, ("type_declaration", 10..48), None),
                (NEEDS_PROJECTION, ("type_spec", 15..48), None),
            ]
        );
    }
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_member_and_excluded_ranges() {
    let context = DocumentationContext::default();
    let unbound = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let field_source: &[u8] = b"package p\ntype S struct {\n// both\n\tX, Y int\n}\n";
    let above_field = [
        (NEEDS_PROJECTION, ("type_declaration", 10..45), None),
        (NEEDS_PROJECTION, ("type_spec", 15..45), None),
        (NEEDS_PROJECTION, ("struct_type", 17..45), None),
        (NEEDS_PROJECTION, ("field_declaration_list", 24..45), None),
    ];

    // An excluded byte inside the declaration, the space after the comma: the field is
    // pending, as every node that holds the byte is, and the comment reaches no target.
    let tree = restricted(field_source, &[0..37, 38..46]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, field_source, &context, &unbound).unwrap();
    let field = node_in(&tree, "field_declaration", 35..43);
    assert_eq!(texts(&index), []);
    assert_eq!(edges(&index), []);
    assert_eq!(kept(&index), [(CandidateDisposition::Unattached, 26..33)]);
    assert!(index.node_projection(field).unwrap().pending());
    assert_eq!(
        index.node_owners(field).err(),
        Some(OwnerError::PendingOwner)
    );
    // expected: the file and the package clause.
    assert_eq!(index.facts().owners().len(), 2);
    let mut all = above_field.to_vec();
    all.push((NEEDS_PROJECTION, ("field_declaration", 35..43), None));
    assert_eq!(pending(&index, index.assessments()), all);

    // An excluded byte between the comment and the declaration, the tab: the field is
    // whole and gets its slots, and the comment is not attached across the gap.
    let tree = restricted(field_source, &[0..34, 35..46]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, field_source, &context, &unbound).unwrap();
    let field = node_in(&tree, "field_declaration", 35..43);
    assert_eq!(texts(&index), []);
    assert_eq!(edges(&index), []);
    assert_eq!(kept(&index), [(CandidateDisposition::Unattached, 26..33)]);
    assert_eq!(
        owners_of(&index, field),
        [
            "slot 0 35..36 in field_declaration 35..43",
            "slot 1 38..39 in field_declaration 35..43",
        ]
    );
    for owner in index.node_owners(field).unwrap() {
        let query = index.owner_associations(owner).unwrap();
        assert_eq!(query.associations().len(), 0);
    }
    assert_eq!(pending(&index, index.assessments()), above_field);

    // No excluded byte: two adjacent included ranges meet inside the name, after `al`.
    let source: &[u8] = b"package p\ntype S struct {\n// d\nalpha int\n}\n";
    let tree = restricted(source, &[0..33, 33..43]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, source, &context, &unbound).unwrap();
    assert_eq!(texts(&index), [(vec![26..30], "d".to_owned())]);
    assert_eq!(
        edges(&index),
        [(
            0,
            Resolution::Resolved,
            vec!["slot 0 31..33,33..36 in field_declaration 31..40".to_owned()],
            ("field_declaration", 31..40),
            ("field_declaration_list", 24..42),
        )]
    );
    assert_eq!(pending(&index, index.assessments()), []);
    assert_eq!(kept(&index), []);
    let association = index.associations().next().unwrap();
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Slot(slot) = owner.get() else {
        panic!("a field slot, found {:?}", owner.get())
    };
    let spelled: Vec<_> = slot
        .source
        .pieces()
        .iter()
        .map(SourcePiece::bytes)
        .collect();
    assert_eq!(spelled, [&b"al"[..], &b"pha"[..]]);

    // The same two gaps for an interface element.
    let method_source: &[u8] = b"package p\ntype I interface {\n// m\n\tM(a, b int)\n}\n";
    let above_method = [
        (NEEDS_PROJECTION, ("type_declaration", 10..48), None),
        (NEEDS_PROJECTION, ("type_spec", 15..48), None),
        (NEEDS_PROJECTION, ("interface_type", 17..48), None),
    ];
    let tree = restricted(method_source, &[0..39, 40..49]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, method_source, &context, &unbound).unwrap();
    let method = node_in(&tree, "method_elem", 35..46);
    assert_eq!(texts(&index), []);
    assert_eq!(kept(&index), [(CandidateDisposition::Unattached, 29..33)]);
    assert!(index.node_projection(method).unwrap().pending());
    let mut all = above_method.to_vec();
    all.extend([
        (NEEDS_PROJECTION, ("method_elem", 35..46), None),
        (NEEDS_PROJECTION, ("parameter_list", 36..46), None),
        (NEEDS_PROJECTION, ("parameter_declaration", 37..45), None),
    ]);
    assert_eq!(pending(&index, index.assessments()), all);

    let tree = restricted(method_source, &[0..34, 35..49]);
    assert!(!tree.root_node().has_error());
    let index = collect(LANG::Go, &tree, method_source, &context, &unbound).unwrap();
    let method = node_in(&tree, "method_elem", 35..46);
    assert_eq!(texts(&index), []);
    assert_eq!(kept(&index), [(CandidateDisposition::Unattached, 29..33)]);
    assert_eq!(owners_of(&index, method), ["method_elem 35..46"]);
    assert_eq!(pending(&index, index.assessments()), above_method);
}

#[cfg(feature = "go")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "go")]
impl WorkBudget for Counting {
    type Error = SourceError;
    #[cfg(feature = "go")]
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
    #[cfg(feature = "go")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        Ok(())
    }
}

/// A struct whose one documented field declaration has that many names.
#[cfg(feature = "go")]
fn wide_names(width: usize) -> String {
    let names: Vec<_> = (0..width).map(|at| format!("v{at}")).collect();
    format!(
        "package p\ntype S struct {{\n// audit\n{} int\n}}\n",
        names.join(", ")
    )
}

/// A struct with that many documented fields of one name each.
#[cfg(feature = "go")]
fn many_fields(count: usize) -> String {
    let fields: Vec<_> = (0..count)
        .map(|at| format!("// d{at}\nv{at} int\n"))
        .collect();
    format!("package p\ntype S struct {{\n{}}}\n", fields.concat())
}

/// An interface with that many documented methods.
#[cfg(feature = "go")]
fn many_methods(count: usize) -> String {
    let methods: Vec<_> = (0..count)
        .map(|at| format!("// d{at}\nM{at}()\n"))
        .collect();
    format!("package p\ntype I interface {{\n{}}}\n", methods.concat())
}

/// What one call of the member scan charged and returned: steps and bytes, and the byte
/// range of each name.
#[cfg(feature = "go")]
type Scan = (
    (usize, usize),
    Result<Option<Vec<Range<usize>>>, SourceError>,
);

/// The member scan on the one node of that kind and range, once per budget: `None` is a
/// budget that never fails, `Some(k)` one that fails at step k.
#[cfg(feature = "go")]
fn scanned(source: &str, kind: &str, range: Range<usize>, budgets: &[Option<usize>]) -> Vec<Scan> {
    let parser = AnyParser::parse(LANG::Go, source.as_bytes().to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let id = index
        .canonical
        .lookup(node_at(&parser, kind, range))
        .unwrap();
    budgets
        .iter()
        .map(|fail_at| {
            let budget = Counting {
                fail_at: *fail_at,
                ..Counting::default()
            };
            let found = super::projections::go_member_names(&index.canonical, id, &budget);
            let names = found.map(|found| {
                found.map(|names| {
                    names
                        .iter()
                        .map(|name| index.canonical.nodes[name.0].evidence.node().byte_range())
                        .collect()
                })
            });
            ((budget.steps.get(), budget.bytes.get()), names)
        })
        .collect()
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_member_scan_charges_one_step_per_child() {
    let cases: [(&str, &str, Range<usize>, usize, Option<Vec<Range<usize>>>); 11] = [
        (
            "package p\ntype S struct {\n// audit\nX int\n}\n",
            "field_declaration",
            35..40,
            // expected: the name and the type.
            2,
            Some(vec![35..36]),
        ),
        (
            "package p\ntype S struct {\n// both\nX, Y int\n}\n",
            "field_declaration",
            34..42,
            // expected: name, `,`, name, type.
            4,
            Some(vec![34..35, 37..38]),
        ),
        (
            "package p\ntype S struct {\n// d\nX, Y int `json:\"x\"`\n}\n",
            "field_declaration",
            31..50,
            // expected: name, `,`, name, type, tag.
            5,
            Some(vec![31..32, 34..35]),
        ),
        (
            "package p\ntype S struct {\n// d\nX, /* c */ Y int\n}\n",
            "field_declaration",
            31..47,
            // expected: name, `,`, a comment on one row, name, type.
            5,
            Some(vec![31..32, 42..43]),
        ),
        (
            "package p\ntype S struct {\n// audit\nio.Reader\n}\n",
            "field_declaration",
            35..44,
            // expected: the qualified type; no name.
            1,
            Some(vec![]),
        ),
        (
            "package p\ntype I interface {\n// m\nM()\n// e\nio.Reader\n// u\n~int\n}\n",
            "method_elem",
            34..37,
            // expected: the name and the parameters; the name of a method is no
            // field name.
            2,
            Some(vec![]),
        ),
        (
            "package p\ntype I interface {\n// m\nM()\n// e\nio.Reader\n// u\n~int\n}\n",
            "type_elem",
            43..52,
            // expected: the qualified type.
            1,
            Some(vec![]),
        ),
        (
            "package p\ntype I interface {\n// m\nM( /* in */ ) /* out */ int\n}\n",
            "method_elem",
            34..61,
            // expected: name, parameters, a comment on one row, result.
            4,
            Some(vec![]),
        ),
        (
            "package p\ntype S struct {\n// d\nX /* a\n b */ int\n}\n",
            "field_declaration",
            31..47,
            // expected: the name and the comment that ends the scan.
            2,
            None,
        ),
        (
            "package p\ntype I interface {\n// m\nM() /* a\n b */ int\n}\n",
            "method_elem",
            34..52,
            // expected: name, parameters and the comment that ends the scan.
            3,
            None,
        ),
        (
            "package p\ntype S struct {\n// d\nX, /* a\n b */ Y int\n}\n",
            "field_declaration",
            31..50,
            // expected: name, `,` and the comment that ends the scan.
            3,
            None,
        ),
    ];
    for (source, kind, range, steps, names) in cases {
        let [(counted, found)] = &scanned(source, kind, range.clone(), &[None])[..] else {
            panic!("one scan")
        };
        assert_eq!(*counted, (steps, 0), "{kind} {range:?}");
        assert_eq!(*found, Ok(names), "{kind} {range:?}");
    }

    // A wide declaration, and a budget that fails at each step in turn.
    let budgets: Vec<_> = std::iter::once(None).chain((1..=601).map(Some)).collect();
    let scans = scanned(&wide_names(300), "field_declaration", 35..1727, &budgets);
    let (counted, found) = &scans[0];
    // expected: 300 names, 299 `,` and the type.
    assert_eq!(*counted, (600, 0));
    let found = found.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(found.len(), 300);
    // expected: `v0` follows the 35 bytes before the names; the 300 names and their 299
    // separators are 1688 bytes, and the last name has four.
    assert_eq!(found[0], 35..37);
    assert_eq!(found[299], 1719..1723);
    // expected: a budget that fails at step k stops the scan after k steps, for each of
    // the 600 steps; one that would fail at step 601 is never reached.
    for (fail_at, (counted, found)) in scans.iter().enumerate().skip(1).take(600) {
        assert_eq!(*found, Err(SourceError::Cancelled), "{fail_at}");
        assert_eq!(*counted, (fail_at, 0), "{fail_at}");
    }
    assert_eq!(scans[601].0, (600, 0));
    assert!(matches!(scans[601].1, Ok(Some(_))));
}

#[cfg(feature = "go")]
#[test]
fn go_field_name_width_costs_linear_work() {
    for width in [64, 512, 2048] {
        let source = wide_names(width);
        let parser = AnyParser::parse(LANG::Go, source.into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let context = DocumentationContext::default();
        let options = DocumentationOptions::default();
        let before = observations();
        let index = parser.documentation(&context, &options).unwrap();
        let after = observations();
        // expected: 15 nodes beside the names (`source_file`, `package_clause`,
        // `package`, `package_identifier`, `type_declaration`, `type`, `type_spec`,
        // `type_identifier`, `struct_type`, `struct`, `field_declaration_list`, `{`,
        // `comment`, `field_declaration`, `}`) and the type of the field, a node per
        // name, and one comma less than names.
        assert_eq!(parser.root_node().preorder().count(), 2 * width + 15);
        // expected: one visit and one child batch per node.
        assert_eq!(after.visits - before.visits, 2 * width + 15);
        assert_eq!(after.child_batches - before.child_batches, 2 * width + 15);
        // expected: every node but the root, which returns first.
        assert_eq!(
            after.target_classifications - before.target_classifications,
            2 * width + 14
        );
        // expected: the file, the package clause, the type declaration, its spec and
        // the field declaration; a slot is pushed without an owner projection.
        assert_eq!(after.owner_projections - before.owner_projections, 5);
        // expected: one search for the comment fragment and one per slot; each is one
        // comparison with one included range.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            width + 1
        );
        // expected: the profile requests no field name, and nothing looks one up.
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        // expected: one document.
        assert_eq!(after.renders - before.renders, 1);
        // expected: the five owners above and one slot per name.
        assert_eq!(index.facts().owners().len(), width + 5);
        // expected: one document, with one association that lists a slot per name.
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().owner_count(), width);
        // expected: the first and the last slot have the ordinals 0 and n - 1 and are
        // spelled by the first and the last name.
        for position in [0, width - 1] {
            let owner = index
                .facts()
                .associated_owner(association, position)
                .unwrap();
            let Owner::Slot(found) = owner.get() else {
                panic!("a field slot, found {:?}", owner.get())
            };
            assert_eq!(found.ordinal, position);
            assert_eq!(
                found.source.pieces()[0].bytes(),
                format!("v{position}").as_bytes()
            );
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_member_count_costs_linear_work() {
    // Per member: the nodes it adds to the tree, and whether it has a slot.
    let shapes: [(fn(usize) -> String, usize, usize, usize); 2] = [
        // expected: a field adds its comment, its declaration, its name and its type;
        // beside the fields the tree has the 13 nodes of the struct around them.
        (many_fields, 4, 13, 1),
        // expected: a method adds its comment, its element, its name, its parameter
        // list and the two parentheses; beside the methods the tree has 12 nodes.
        (many_methods, 6, 12, 0),
    ];
    for (source, per_member, around, slots) in shapes {
        for count in [64, 512, 2048] {
            let parser =
                AnyParser::parse(LANG::Go, source(count).into_bytes(), None, None).unwrap();
            assert!(!parser.root_node().has_error());
            let context = DocumentationContext::default();
            let options = DocumentationOptions::default();
            let before = observations();
            let index = parser.documentation(&context, &options).unwrap();
            let after = observations();
            // expected: the nodes each member adds and the nodes around them.
            let nodes = per_member * count + around;
            assert_eq!(parser.root_node().preorder().count(), nodes);
            // expected: one visit and one child batch per node, and one classification
            // for every node but the root.
            assert_eq!(after.visits - before.visits, nodes);
            assert_eq!(after.child_batches - before.child_batches, nodes);
            assert_eq!(
                after.target_classifications - before.target_classifications,
                nodes - 1
            );
            // expected: the file, the package clause, the type declaration, its spec,
            // and one projection per member.
            assert_eq!(
                after.owner_projections - before.owner_projections,
                count + 4
            );
            // expected: one search per comment fragment and one per slot.
            assert_eq!(
                after.intersection_searches - before.intersection_searches,
                count + slots * count
            );
            // expected: the profile requests no field name, and nothing looks one up.
            assert_eq!(after.field_requests - before.field_requests, 0);
            assert_eq!(after.field_lookups - before.field_lookups, 0);
            // expected: one document per member, each with one association of one owner.
            assert_eq!(after.renders - before.renders, count);
            assert_eq!(index.documents().len(), count);
            assert_eq!(index.associations().len(), count);
            assert!(
                index
                    .associations()
                    .all(|association| association.get().owner_count() == 1)
            );
            // expected: the four owners above, each member, and each slot.
            assert_eq!(index.facts().owners().len(), 4 + count + slots * count);
            // expected: every member has its owners, so nothing is pending.
            assert_eq!(index.assessments().len(), 0);
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_field_name_width_collection_observes_cancellation() {
    let width = 2048;
    let parser = AnyParser::parse(LANG::Go, wide_names(width).into_bytes(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut inside = 0;
    let mut scanning = 0;
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
        let owners = after.owner_projections - before.owner_projections;
        let searches = after.intersection_searches - before.intersection_searches;
        // The field declaration has its owner and the last slot has not been cut: the
        // collection is in the slot loop.
        if owners == 5 && searches < width + 1 {
            inside += 1;
        }
        // Every node but the closing brace of the field list is entered and the field
        // declaration has no owner yet: the walk is between the last child of the
        // declaration and its slots, where only the two scans of its children are long.
        if owners == 4 && after.visits - before.visits == 2 * width + 14 {
            scanning += 1;
        }
    }
    // expected: 6 checkpoints fall in the 6144 step charges of the slot loop (3 per
    // slot, one checkpoint per 1024); the last may follow the last search.
    assert!(inside >= 5, "{inside}");
    // expected: no more than those 6; a loop that charges more steps for a later slot
    // holds more.
    assert!(inside <= 6, "{inside}");
    // expected: the declaration has 4096 children (2048 names, 2047 commas and the
    // type). The scan for the names and the scan for a line end each charge one step
    // per child: 8192 steps, which hold 8 checkpoints, or 9 with the few steps before
    // them. Without the charges of one scan 4096 steps lie there, which hold at most
    // 5; a scan that charges a child twice brings them to 12 or more.
    assert!((8..=9).contains(&scanning), "{scanning}");
    let index = crate::documentation::budget::cancel_after(polls + 1, || {
        parser.documentation(&context, &options)
    })
    .unwrap();
    assert_eq!(
        index.associations().next().unwrap().get().owner_count(),
        width
    );
}

/// An interface whose one documented method has that many parameters.
#[cfg(feature = "go")]
fn wide_parameters(width: usize) -> String {
    let parameters: Vec<_> = (0..width).map(|at| format!("a{at} int")).collect();
    format!(
        "package p\ntype I interface {{\n// d\nM({})\n}}\n",
        parameters.join(", ")
    )
}

/// A struct whose one field holds that many struct types, each inside the one before,
/// on one row: `N0 struct { N1 struct { X int } }` for a depth of two.
#[cfg(feature = "go")]
fn deep_structs(depth: usize) -> String {
    let open: Vec<_> = (0..depth).map(|at| format!("N{at} struct {{ ")).collect();
    format!(
        "package p\ntype S struct {{\n{}X int{}\n}}\n",
        open.concat(),
        " }".repeat(depth)
    )
}

/// What one call of the line-end scan charged, read and returned: steps and bytes, the
/// nodes it read, and whether Go ends the member early.
#[cfg(feature = "go")]
type LineEnds = ((usize, usize), usize, Result<bool, SourceError>);

/// The line-end scan on the one node of that kind and range, once per budget: `None` is
/// a budget that never fails, `Some(k)` one that fails at step k.
#[cfg(feature = "go")]
fn line_ends(
    source: &[u8],
    kind: &str,
    range: Range<usize>,
    budgets: &[Option<usize>],
) -> Vec<LineEnds> {
    let parser = AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let id = index
        .canonical
        .lookup(node_at(&parser, kind, range))
        .unwrap();
    budgets
        .iter()
        .map(|fail_at| {
            let budget = Counting {
                fail_at: *fail_at,
                ..Counting::default()
            };
            let before = super::projections::member_reads::observed();
            let found = super::projections::go_ends_member_early(&index.canonical, id, &budget);
            let reads = super::projections::member_reads::observed() - before;
            ((budget.steps.get(), budget.bytes.get()), reads, found)
        })
        .collect()
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_line_end_scan_reads_each_token_once() {
    let cases: [(&[u8], &str, Range<usize>, usize, bool); 12] = [
        (
            b"package p\ntype S struct {\n// audit\nX int\n}\n",
            "field_declaration",
            35..40,
            // expected: the name and the type.
            2,
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nX, Y int `json:\"x\"`\n}\n",
            "field_declaration",
            31..50,
            // expected: two names, the comma, the type, and the tag as one token; the
            // three nodes inside the tag are not read.
            5,
            false,
        ),
        (
            b"package p\ntype I interface {\n// d\nM(\n\ta int,\n\tb int,\n) error\n}\n",
            "method_elem",
            34..60,
            // expected: the name, the parameter list and the result; in the list two
            // parentheses, two commas and two declarations; in each declaration a name
            // and a type.
            13,
            false,
        ),
        (
            b"package p\ntype I interface {\n// d\nM(a int, // first\n\tb int) error\n}\n",
            "method_elem",
            34..65,
            // expected: the name, the parameter list and the result; in the list two
            // parentheses, one comma, the comment and two declarations of two tokens.
            13,
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nA struct {\n// x\nX int\n// y\nY int\n}\n}\n",
            "field_declaration",
            31..65,
            // expected: the name, the struct type, `struct`, the field list and its two
            // braces; the two comments and the two fields between them are not read.
            6,
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nI interface {\n// m\nM()\n// n\nN()\n}\n}\n",
            "field_declaration",
            31..64,
            // expected: the name, the interface type, `interface` and the two braces.
            5,
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nX [len(func() []int {\nreturn nil\n}())]int\n}\n",
            "field_declaration",
            31..72,
            // expected: the name and the array type (2); `[`, the outer call, `len` and
            // its argument list (4); `(`, the inner call and the function literal (3);
            // `func`, the parameter list and its two parentheses (4); the slice type
            // with `[`, `]` and `int` (4); the block and its two braces (3); the inner
            // argument list with two parentheses (3); `)`, `]` and `int` (3).
            26,
            false,
        ),
        (
            b"package p\ntype S struct {\n// d\nX [2]\nint\n}\n",
            "field_declaration",
            31..40,
            // expected: the name, the array type, `[`, `2`, `]`, and the `int` on the
            // next row, at which the scan returns.
            6,
            true,
        ),
        (
            b"package p\ntype S struct {\n// d\nX func() /* a\n b */ int\n}\n",
            "field_declaration",
            31..54,
            // expected: the name, the function type, `func`, the parameter list, two
            // parentheses, the comment, and the `int` at which the scan returns.
            8,
            true,
        ),
        (
            b"package p\ntype I interface {\n// d\nE | func() /* stop\n here */ B | C\n}\n",
            "type_elem",
            34..67,
            // expected: `E`, `|`, the function type, `func`, the parameter list, two
            // parentheses, the comment, and the `B` at which the scan returns; the
            // second `|` and `C` are not read.
            9,
            true,
        ),
        (
            b"package p\ntype S struct {\n// d\nX [len(\"s\" /* a\n b */ )]int\n}\n",
            "field_declaration",
            31..58,
            // expected: the name, the array type, `[`, the call, `len`, the argument
            // list, `(`, the string as one token, the comment, and the `)` at which the
            // scan returns.
            10,
            true,
        ),
        (
            b"package p\ntype S struct {\n// d\nX map[struct{}\n]int\n}\n",
            "field_declaration",
            31..50,
            // expected: the name, the map type, `map`, `[`, the struct type, `struct`,
            // the field list, its two braces, and the `]` at which the scan returns.
            10,
            true,
        ),
    ];
    for (source, kind, range, steps, early) in cases {
        // expected: one step and one counted read per node read, and no byte charge.
        assert_eq!(
            line_ends(source, kind, range, &[None]),
            [((steps, 0), steps, Ok(early))],
            "{}",
            source.escape_ascii()
        );
    }
    // expected: 4 * 300 + 3 nodes (the name, the parameter list and its two
    // parentheses; per parameter the declaration, its name and its type; 299 commas).
    // A budget that fails at step k stops the scan after k steps, before the k-th read.
    let wide = wide_parameters(300);
    let member = ("method_elem", 34..wide.len() - 3);
    let budgets: Vec<_> = std::iter::once(None).chain((1..=1204).map(Some)).collect();
    let found = line_ends(wide.as_bytes(), member.0, member.1, &budgets);
    assert_eq!(found[0], ((1203, 0), 1203, Ok(false)));
    for (fail_at, scan) in (1..=1203).zip(&found[1..]) {
        assert_eq!(
            *scan,
            ((fail_at, 0), fail_at - 1, Err(SourceError::Cancelled)),
            "{fail_at}"
        );
    }
    assert_eq!(found[1204], ((1203, 0), 1203, Ok(false)));
}

/// The nodes the line-end scan read during one typed collection of a Go source, and
/// the index.
#[cfg(feature = "go")]
fn member_reads_of(parser: &AnyParser) -> (usize, DocumentationIndex<'_>) {
    let before = super::projections::member_reads::observed();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    (super::projections::member_reads::observed() - before, index)
}

#[cfg(feature = "go")]
#[test]
#[allow(clippy::too_many_lines)]
fn go_line_end_scan_costs_linear_work() {
    // A wide parameter list: every token of the method is read once.
    for width in [64, 512, 2048] {
        let parser =
            AnyParser::parse(LANG::Go, wide_parameters(width).into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let before = observations();
        let (reads, index) = member_reads_of(&parser);
        let after = observations();
        // expected: the name, the parameter list and its two parentheses; per parameter
        // the declaration, its name and its type; one comma less than parameters.
        assert_eq!(reads, 4 * width + 3);
        // expected: those nodes, the method itself, and 13 around it (`source_file`,
        // `package_clause`, `package`, `package_identifier`, `type_declaration`, `type`,
        // `type_spec`, `type_identifier`, `interface_type`, `interface`, `{`, `comment`,
        // `}`).
        let nodes = 4 * width + 17;
        assert_eq!(parser.root_node().preorder().count(), nodes);
        // expected: the scan moves no other counter. One visit and one child batch per
        // node, one classification for every node but the root, one search for the
        // comment fragment, no field name requested or looked up.
        assert_eq!(after.visits - before.visits, nodes);
        assert_eq!(after.child_batches - before.child_batches, nodes);
        assert_eq!(
            after.target_classifications - before.target_classifications,
            nodes - 1
        );
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            1
        );
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        // expected: the method has its owner and its document.
        assert_eq!(index.facts().owners().len(), 5);
        assert_eq!(index.associations().len(), 1);
        assert_eq!(index.assessments().len(), 0);
    }

    // Many members: the reads grow with the number of members.
    for count in [64, 512, 2048] {
        let parser =
            AnyParser::parse(LANG::Go, many_fields(count).into_bytes(), None, None).unwrap();
        // expected: per field its name and its type.
        assert_eq!(member_reads_of(&parser).0, 2 * count);
        let parser =
            AnyParser::parse(LANG::Go, many_methods(count).into_bytes(), None, None).unwrap();
        // expected: per method its name, its parameter list and the two parentheses.
        assert_eq!(member_reads_of(&parser).0, 4 * count);
    }

    // Structs nested many levels deep, on one row: no line end is inside, so every
    // field has its owners whether or not a nested body is read again, and only the
    // count tells. Each node is read for one member.
    for depth in [16, 64, 256] {
        let parser =
            AnyParser::parse(LANG::Go, deep_structs(depth).into_bytes(), None, None).unwrap();
        assert!(!parser.root_node().has_error());
        let (reads, index) = member_reads_of(&parser);
        // expected: per nesting field its name, the struct type, `struct`, the field
        // list and its two braces; for the innermost field its name and its type.
        assert_eq!(reads, 6 * depth + 2);
        // expected: per nesting field 7 nodes (those six and the declaration), 3 for
        // the innermost field, and 13 around them.
        assert_eq!(parser.root_node().preorder().count(), 7 * depth + 16);
        // expected: the file, the package clause, the type declaration and its spec;
        // per field its declaration and one slot.
        assert_eq!(index.facts().owners().len(), 4 + 2 * (depth + 1));
        assert_eq!(index.assessments().len(), 0);
    }

    // A Go file without a member: the same wide parameter list on a function. The scan
    // reads nothing; the collections above are the control that the counter counts.
    let parameters: Vec<_> = (0..2048).map(|at| format!("a{at} int")).collect();
    let source = format!("package p\n// d\nfunc F({}) {{}}\n", parameters.join(", "));
    let parser = AnyParser::parse(LANG::Go, source.into_bytes(), None, None).unwrap();
    assert!(!parser.root_node().has_error());
    let (reads, index) = member_reads_of(&parser);
    assert_eq!(reads, 0);
    // expected: the file, the package clause and the function; one document.
    assert_eq!(index.facts().owners().len(), 3);
    assert_eq!(index.documents().len(), 1);
}

#[cfg(all(feature = "go", feature = "java"))]
#[test]
fn go_line_end_scan_reads_nothing_for_another_profile() {
    // The control: a Go field with two names moves the counter by its four tokens.
    let parser = clean(b"package p\ntype S struct {\n// both\nX, Y int\n}\n");
    assert_eq!(member_reads_of(&parser).0, 4);
    // A Java field declaration with two names has the same owners, a declaration and
    // two field slots, and the scan reads nothing for it.
    let source = b"class C {\n/** d */\nint x, y;\n}\n";
    let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
    assert!(!parser.root_node().has_error());
    let (reads, index) = member_reads_of(&parser);
    assert_eq!(reads, 0);
    assert_eq!(index.associations().len(), 1);
    assert_eq!(index.associations().next().unwrap().get().owner_count(), 2);
}

#[cfg(feature = "go")]
#[test]
fn go_wide_parameter_list_collection_observes_cancellation() {
    let width = 2048;
    let parser =
        AnyParser::parse(LANG::Go, wide_parameters(width).into_bytes(), None, None).unwrap();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut scanning = 0;
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
        // Every node but the closing brace of the interface is entered and the method
        // has no owner yet: the walk is between the last token of the method and its
        // owner, where only the line-end scan is long.
        if after.owner_projections - before.owner_projections == 4
            && after.visits - before.visits == 4 * width + 16
        {
            scanning += 1;
        }
    }
    // expected: the scan charges one step for each of the 4 * 2048 + 3 = 8195 nodes it
    // reads, which hold 8 checkpoints, or 9 with the few steps before it. Without those
    // charges fewer than 1024 steps lie there, which hold at most one.
    assert!(scanning >= 8, "{scanning}");
    // expected: no more than 9; a scan that charges a node twice holds 16.
    assert!(scanning <= 9, "{scanning}");
    let index = crate::documentation::budget::cancel_after(polls + 1, || {
        parser.documentation(&context, &options)
    })
    .unwrap();
    assert_eq!(index.associations().len(), 1);
}
