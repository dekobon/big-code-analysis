//! Go documentation in the source view: which declaration owns a comment, the text of
//! the document and where each part of that text comes from. Every source is collected
//! through both entry points, except in the last test, which collects nothing and pins
//! the parse-tree shapes the other tests rely on.

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
/// holds none of them.
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
                    .map(|at| {
                        let fragment = facts.document_fragment(document, at).unwrap();
                        span(fragment.get().source())
                    })
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
            let declaration = owner_node(facts, association);
            // Both follow from the tree: the binding is the owner itself, and the lexical
            // scope is the parent of the first comment of the document.
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                Some(declaration)
            );
            let document = facts.associated_document(association).unwrap();
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
            (candidate.get().disposition, span(fragment.get().source()))
        })
        .collect();
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
