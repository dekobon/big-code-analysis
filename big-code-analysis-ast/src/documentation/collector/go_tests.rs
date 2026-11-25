//! Go documentation in the source view: which declaration owns a comment, the text of
//! the document and where each part of that text comes from. Every source is collected
//! through both entry points, except in the grammar tests, which collect nothing and pin
//! the parse-tree shapes the other tests rely on.

#![allow(clippy::single_range_in_vec_init, clippy::type_complexity)]

#[cfg(feature = "go")]
use crate::documentation::*;
#[cfg(feature = "go")]
use crate::{AnyParser, LANG};
#[cfg(feature = "go")]
use std::ops::Range;

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
/// range, and the number of pending assessments. The default value is an index that
/// holds none of them. What is the same for every source here is asserted by
/// `published` and is in no expected value. Node queries are not part of it.
#[cfg(feature = "go")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    owners: Vec<(Resolution, &'static str, Range<usize>)>,
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

/// The node that alone owns the association, as every owner here is.
#[cfg(feature = "go")]
fn owner_node<'s>(
    facts: &Facts<'s>,
    association: FactRef<'_, Association<'s>>,
) -> tree_sitter::Node<'s> {
    assert_eq!(association.get().owner_count(), 1);
    let owner = facts.associated_owner(association, 0).unwrap();
    let Owner::Node(declared) = owner.get() else {
        panic!("a node owner, found {:?}", owner.get())
    };
    declared.node()
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
    let owners = index
        .associations()
        .enumerate()
        .map(|(position, association)| {
            let attachment = association.get();
            let declaration = owner_node(facts, association);
            // The same for every association here, so no expected view states them. Each
            // document has one association, at the document's own position.
            assert_eq!(attachment.convention.name(), "GoDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            assert_eq!(index.documents().nth(position), Some(document));
            // Both follow from the tree: the binding is the owner itself, and the lexical
            // scope is the parent of the first comment of the document.
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
