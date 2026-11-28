//! Doxygen documentation of C, C++, Mozcpp and Objective-C in the source view: what owns a
//! comment, the text of the document and where each part of that text comes from. The
//! source-view tests collect every source through both entry points and compare the
//! documents, associations, retained candidates and pending assessments the index
//! publishes. The grammar tests collect nothing: they pin the parse-tree shapes the
//! source-view tests rely on.

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
use crate::documentation::*;
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
use crate::{AnyParser, LANG};
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
use std::ops::Range;

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
use Run::{Copied, Synthetic};

/// The origin of one run of a document's text, with the source byte range it reads.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

/// One document: its fragments as source byte ranges, and each run of its text as an
/// output range with its origin.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

/// A node of the parse tree as its kind and byte range.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
type Site = (&'static str, Range<usize>);

/// One owner of an association.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    Node(Site),
    /// The byte range of the slot's name, and the node owner that encloses the slot.
    Slot {
        role: SlotRole,
        ordinal: usize,
        source: Range<usize>,
        enclosing: Site,
    },
    /// The byte range of the root.
    File(Range<usize>),
}

/// One association: the position of its document in the published order, its owners in
/// order, and the node it is bound to.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    resolution: Resolution,
    owners: Vec<Holder>,
    binding: Option<Site>,
}

/// One pending assessment, with the position of its linked document in the published
/// order.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<Site>,
    document: Option<usize>,
}

/// What one collection published. Documents, associations and retained candidates are in
/// published order, and a retained candidate is its disposition and byte range. Pending
/// assessments are in the order of `unordered`. The default value is an index that
/// published nothing and holds nothing pending. What is the same for every source here is
/// asserted by `published` and is in no expected view. Node queries are not part of it.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Link>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
const AWAITS_OWNER: &str = "Selected local or wrapper form awaits owner projection";

/// The expected primary document of one comment.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
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

/// The expected `Field` slot of the declarator whose name is at `source`.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn field_slot(ordinal: usize, source: Range<usize>, enclosing: &Site) -> Holder {
    Holder::Slot {
        role: SlotRole::Field,
        ordinal,
        source,
        enclosing: enclosing.clone(),
    }
}

/// The expected association of a document whose binding follows from its owners: the
/// node owner itself, or the node that encloses its slot owners.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn bound(document: usize, resolution: Resolution, owners: Vec<Holder>) -> Link {
    let binding = match owners.first() {
        Some(Holder::Node(declared)) => declared.clone(),
        Some(Holder::Slot { enclosing, .. }) => enclosing.clone(),
        other => panic!("a node or slot owner, found {other:?}"),
    };
    Link {
        document,
        resolution,
        owners,
        binding: Some(binding),
    }
}

/// The expected view of one document that `owners` own through one resolved association
/// bound as in `bound`, with no retained candidate and no pending assessment.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn owned(document: Doc, owners: Vec<Holder>) -> View {
    View {
        documents: vec![document],
        associations: vec![bound(0, Resolution::Resolved, owners)],
        ..View::default()
    }
}

/// The expected pending assessment on the owner axis, the only axis these cases hold.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn pending(reason: &'static str, node: Option<Site>, document: Option<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason,
        node,
        document,
    }
}

/// The pending assessments of a view, published or expected. The contract states no
/// order for assessments, so they are sorted by their debug text, and a comparison does
/// not depend on the order in which the index publishes them.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unordered(assessments: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = assessments.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

/// The byte range of a spelling that is one included piece, as every spelling here is.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn site(found: tree_sitter::Node<'_>) -> Site {
    (found.kind(), found.byte_range())
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn holder<'s>(
    facts: &Facts<'s>,
    owner: FactRef<'_, Owner<'s>>,
    root: tree_sitter::Node<'_>,
) -> Holder {
    match owner.get() {
        Owner::Node(declared) => Holder::Node(site(declared.node())),
        Owner::Slot(slot) => {
            let enclosing = facts.slot_enclosing(owner).unwrap();
            let Owner::Node(declared) = enclosing.get() else {
                panic!("a slot inside a node owner, found {:?}", enclosing.get())
            };
            Holder::Slot {
                role: slot.role,
                ordinal: slot.ordinal,
                source: span(&slot.source),
                enclosing: site(declared.node()),
            }
        }
        Owner::File(file) => {
            assert_eq!(file.node(), root);
            Holder::File(file.node().byte_range())
        }
        other => panic!("a node, slot or file owner, found {other:?}"),
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
    let position = |document: FactRef<'_, Document<'_>>| {
        ordered.iter().position(|known| *known == document).unwrap()
    };
    let documents = ordered
        .iter()
        .map(|&document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| {
                        let fragment = facts.document_fragment(document, at).unwrap().get();
                        let range = span(fragment.source());
                        // The same for every fragment here, so no expected view states it:
                        // the spelling is one whole comment node.
                        assert_eq!(fragment.form(), FragmentForm::Comment);
                        assert_eq!(
                            fragment.node().map(|carrier| site(carrier.node())),
                            Some(("comment", range.clone()))
                        );
                        range
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
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            // The same for every association here, so no expected view states them.
            assert_eq!(attachment.convention.name(), "Doxygen");
            assert_eq!(attachment.publication, Publication::Unknown);
            // The lexical scope follows from the tree: it is the parent of the first
            // comment of the document.
            let document = facts.associated_document(association).unwrap();
            let first = facts.document_fragment(document, 0).unwrap();
            let first_comment = first.get().node().unwrap().node();
            assert_eq!(
                Some(attachment.lexical_scope.node()),
                first_comment.parent()
            );
            Link {
                document: position(document),
                resolution: attachment.resolution,
                owners: (0..attachment.owner_count())
                    .map(|at| {
                        let owner = facts.associated_owner(association, at).unwrap();
                        holder(facts, owner, root)
                    })
                    .collect(),
                binding: attachment.binding.map(|wrapper| site(wrapper.node())),
            }
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            let range = span(fragment.get().source());
            assert_eq!(fragment.get().form(), FragmentForm::Comment);
            assert_eq!(
                fragment.get().node().map(|carrier| site(carrier.node())),
                Some(("comment", range.clone()))
            );
            (candidate.get().disposition, range)
        })
        .collect();
    let assessments = unordered(index.assessments().map(|assessment| Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: assessment.get().node.map(|about| site(about.node())),
        document: index.assessed_document(assessment).unwrap().map(position),
    }));
    // The same for every source here: the Doxygen adapter is partial. A qualification
    // record states that a command which names its declaration, or the declaration it
    // names, lies inside a conditional or a recovery region; no source has that.
    assert_eq!(index.capability(), Capability::Partial);
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

/// A parse whose tree holds no syntax error.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn clean(language: LANG, source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.root_node().has_error(),
        "{language:?}: {}",
        source.escape_ascii()
    );
    parser
}

/// The view that both entry points publish for an error-free parse under the default
/// options: the typed parser's method, then `collect`.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn view(language: LANG, source: &[u8]) -> View {
    let parser = clean(language, source);
    view_from_parser(language, &parser)
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn view_from_parser(language: LANG, parser: &AnyParser) -> View {
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let root = parser.ts_tree().root_node();
    let typed = published(&parser.documentation(&context, &options).unwrap(), root);
    let raw = published(
        &collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
        root,
    );
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

/// The one node of that kind and byte range.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
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

/// Every child of `parent` in order, named or not: its field name, kind and byte range.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn parts(parent: tree_sitter::Node<'_>) -> Vec<(Option<&'static str>, &'static str, Range<usize>)> {
    let mut cursor = parent.walk();
    let mut found = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            found.push((cursor.field_name(), child.kind(), child.byte_range()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    found
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn named_parts(parent: tree_sitter::Node<'_>) -> Vec<Site> {
    parent
        .named_children(&mut parent.walk())
        .map(site)
        .collect()
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn leading_block_documents_the_declaration_after_it(language: LANG) {
    let rows: [(&[u8], Site); 7] = [
        (
            b"/** d */\nint f(void) { return 0; }\n",
            ("function_definition", 9..34),
        ),
        (b"/** d */\nint f(void);\n", ("declaration", 9..21)),
        (b"/** d */\ntypedef int T;\n", ("type_definition", 9..23)),
        (
            b"/** d */\nstruct S { int a; };\n",
            ("struct_specifier", 9..28),
        ),
        (
            b"/** d */\nunion U { int a; };\n",
            ("union_specifier", 9..27),
        ),
        (b"/** d */\nenum E { A };\n", ("enum_specifier", 9..21)),
        (b"/** d */\n#define M 1\n", ("preproc_def", 9..21)),
    ];
    for (source, owner) in rows {
        assert_eq!(
            view(language, source),
            owned(
                comment(0..8, "d", [(0..1, Copied(4..5))]),
                vec![Holder::Node(owner)]
            ),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "c")]
#[test]
fn c_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn each_variable_declarator_owns_one_slot(language: LANG) {
    let one = ("declaration", 9..15);
    assert_eq!(
        view(language, b"/** d */\nint x;\n"),
        owned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            vec![field_slot(0, 13..14, &one)]
        )
    );
    let two = ("declaration", 11..20);
    assert_eq!(
        view(language, b"/**audit*/\nint a, b;\n"),
        owned(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            vec![field_slot(0, 15..16, &two), field_slot(1, 18..19, &two)]
        )
    );
}

#[cfg(feature = "c")]
#[test]
fn c_each_variable_declarator_owns_one_slot() {
    each_variable_declarator_owns_one_slot(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_each_variable_declarator_owns_one_slot() {
    each_variable_declarator_owns_one_slot(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_each_variable_declarator_owns_one_slot() {
    each_variable_declarator_owns_one_slot(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_each_variable_declarator_owns_one_slot() {
    each_variable_declarator_owns_one_slot(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn enumerator_and_field_own_their_leading_block(language: LANG) {
    assert_eq!(
        view(language, b"enum E { /** la */ A, /** lb */ B };\n"),
        View {
            documents: vec![
                comment(9..18, "la", [(0..2, Copied(13..15))]),
                comment(22..31, "lb", [(0..2, Copied(26..28))]),
            ],
            associations: vec![
                bound(
                    0,
                    Resolution::Resolved,
                    vec![Holder::Node(("enumerator", 19..20))]
                ),
                bound(
                    1,
                    Resolution::Resolved,
                    vec![Holder::Node(("enumerator", 32..33))]
                ),
            ],
            ..View::default()
        }
    );
    let field = ("field_declaration", 24..30);
    assert_eq!(
        view(language, b"struct S {\n  /** d */\n  int x;\n};\n"),
        owned(
            comment(13..21, "d", [(0..1, Copied(17..18))]),
            vec![field_slot(0, 28..29, &field)]
        )
    );
}

#[cfg(feature = "c")]
#[test]
fn c_enumerator_and_field_own_their_leading_block() {
    enumerator_and_field_own_their_leading_block(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_enumerator_and_field_own_their_leading_block() {
    enumerator_and_field_own_their_leading_block(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_enumerator_and_field_own_their_leading_block() {
    enumerator_and_field_own_their_leading_block(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_enumerator_and_field_own_their_leading_block() {
    enumerator_and_field_own_their_leading_block(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn trailing_block_documents_the_member_before_it(language: LANG) {
    let field = ("field_declaration", 11..17);
    assert_eq!(
        view(language, b"struct S { int x; ///<audit\n};\n"),
        owned(
            comment(18..27, "audit", [(0..5, Copied(22..27))]),
            vec![field_slot(0, 15..16, &field)]
        )
    );
    assert_eq!(
        view(language, b"struct S { int x; /**< t */ };\n"),
        owned(
            comment(18..27, "t", [(0..1, Copied(23..24))]),
            vec![field_slot(0, 15..16, &field)]
        )
    );
    assert_eq!(
        view(language, b"enum E {\n  A,\n  B ///< tb\n};\n"),
        owned(
            comment(18..25, "tb", [(0..2, Copied(23..25))]),
            vec![Holder::Node(("enumerator", 16..17))]
        )
    );
    assert_eq!(
        view(language, b"enum E {\n  A,\n  B /**< tb */\n};\n"),
        owned(
            comment(18..28, "tb", [(0..2, Copied(23..25))]),
            vec![Holder::Node(("enumerator", 16..17))]
        )
    );
}

#[cfg(feature = "c")]
#[test]
fn c_trailing_block_documents_the_member_before_it() {
    trailing_block_documents_the_member_before_it(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_trailing_block_documents_the_member_before_it() {
    trailing_block_documents_the_member_before_it(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_trailing_block_documents_the_member_before_it() {
    trailing_block_documents_the_member_before_it(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_trailing_block_documents_the_member_before_it() {
    trailing_block_documents_the_member_before_it(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn file_command_documents_the_file(language: LANG) {
    let rows: [(&[u8], Range<usize>, Vec<Pending>); 2] = [
        (b"/** @file */\nint x;\n", 0..20, vec![]),
        (
            b"/** @file */\n#ifndef G_H\n#define G_H\nint x;\n#endif\n",
            0..51,
            // The assessment on the include guard is pending work, pinned as it is today.
            vec![pending(
                NEEDS_PROJECTION,
                Some(("preproc_ifdef", 13..50)),
                None,
            )],
        ),
    ];
    for (source, file, assessments) in rows {
        assert_eq!(
            view(language, source),
            View {
                documents: vec![Doc {
                    role: DocumentRole::FilePackage,
                    ..comment(0..12, "@file", [(0..5, Copied(4..9))])
                }],
                // The file owns the document, and no node binds it.
                associations: vec![Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owners: vec![Holder::File(file)],
                    binding: None,
                }],
                assessments: unordered(assessments),
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "c")]
#[test]
fn c_file_command_documents_the_file() {
    file_command_documents_the_file(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_file_command_documents_the_file() {
    file_command_documents_the_file(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_file_command_documents_the_file() {
    file_command_documents_the_file(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_file_command_documents_the_file() {
    file_command_documents_the_file(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn fn_command_resolves_by_signature(language: LANG) {
    // The prototype that the signature names owns the document, and no node binds it.
    assert_eq!(
        view(
            language,
            b"/**@fn void f(void)\n * audit*/\nvoid f(void);\n"
        ),
        View {
            documents: vec![comment(
                0..30,
                "@fn void f(void)\naudit",
                [
                    (0..16, Copied(3..19)),
                    (16..17, Copied(19..20)),
                    (17..22, Copied(23..28)),
                ]
            )],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::Node(("declaration", 31..44))],
                binding: None,
            }],
            ..View::default()
        }
    );
    // No declaration has the name, so the association lists no owner.
    assert_eq!(
        view(
            language,
            b"/**@fn void missing(void)\n * audit*/\nvoid f(void);\n"
        ),
        View {
            documents: vec![comment(
                0..36,
                "@fn void missing(void)\naudit",
                [
                    (0..22, Copied(3..25)),
                    (22..23, Copied(25..26)),
                    (23..28, Copied(29..34)),
                ]
            )],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Unresolved(UnresolvedReason::MissingName),
                owners: vec![],
                binding: None,
            }],
            ..View::default()
        }
    );
}

#[cfg(feature = "c")]
#[test]
fn c_fn_command_resolves_by_signature() {
    fn_command_resolves_by_signature(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_fn_command_resolves_by_signature() {
    fn_command_resolves_by_signature(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_fn_command_resolves_by_signature() {
    fn_command_resolves_by_signature(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_fn_command_resolves_by_signature() {
    fn_command_resolves_by_signature(LANG::Objc);
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn class_member_function_namespace_and_alias_own_their_block(language: LANG) {
    let rows: [(&[u8], Doc, Site); 5] = [
        (
            b"/** c */\nclass C { };\n",
            comment(0..8, "c", [(0..1, Copied(4..5))]),
            ("class_specifier", 9..20),
        ),
        (
            b"class C {\n  /** m */\n  void m();\n};\n",
            comment(12..20, "m", [(0..1, Copied(16..17))]),
            ("field_declaration", 23..32),
        ),
        (
            b"class C {\n  /** m */\n  void m() { }\n};\n",
            comment(12..20, "m", [(0..1, Copied(16..17))]),
            ("function_definition", 23..35),
        ),
        (
            b"/** n */\nnamespace N { }\n",
            comment(0..8, "n", [(0..1, Copied(4..5))]),
            ("namespace_definition", 9..24),
        ),
        (
            b"/** a */\nusing T = int;\n",
            comment(0..8, "a", [(0..1, Copied(4..5))]),
            ("alias_declaration", 9..23),
        ),
    ];
    for (source, document, owner) in rows {
        assert_eq!(
            view(language, source),
            owned(document, vec![Holder::Node(owner)]),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "cpp")]
#[test]
fn cpp_class_member_function_namespace_and_alias_own_their_block() {
    class_member_function_namespace_and_alias_own_their_block(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_class_member_function_namespace_and_alias_own_their_block() {
    class_member_function_namespace_and_alias_own_their_block(LANG::Mozcpp);
}

#[cfg(feature = "objc")]
#[test]
fn objc_interface_property_method_and_instance_variable_own_their_block() {
    let rows: [(&[u8], Doc, Site); 4] = [
        (
            b"/** i */\n@interface C\n@end\n",
            comment(0..8, "i", [(0..1, Copied(4..5))]),
            ("class_interface", 9..26),
        ),
        (
            b"@interface C\n/** p */\n@property int p;\n@end\n",
            comment(13..21, "p", [(0..1, Copied(17..18))]),
            ("property_declaration", 22..38),
        ),
        (
            b"@interface C\n/**audit*/\n- (void)f;\n@end\n",
            comment(13..23, "audit", [(0..5, Copied(16..21))]),
            ("method_declaration", 24..34),
        ),
        (
            b"@interface C {\n  /** v */\n  int v;\n}\n@end\n",
            comment(17..25, "v", [(0..1, Copied(21..22))]),
            ("instance_variable", 28..34),
        ),
    ];
    for (source, document, owner) in rows {
        assert_eq!(
            view(LANG::Objc, source),
            owned(document, vec![Holder::Node(owner)]),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn comment_forms_are_cleaned_to_their_text(language: LANG) {
    let rows: [(&[u8], Doc, Site); 5] = [
        (
            b"/*! d */\nint f(void);\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("declaration", 9..21),
        ),
        (
            b"/// d\nint f(void);\n",
            comment(0..5, "d", [(0..1, Copied(4..5))]),
            ("declaration", 6..18),
        ),
        (
            b"//! d\nint f(void);\n",
            comment(0..5, "d", [(0..1, Copied(4..5))]),
            ("declaration", 6..18),
        ),
        (
            b"/// a\n/// b\nint f(void);\n",
            Doc {
                role: DocumentRole::Primary,
                composition: Composition::CommentRun,
                fragments: vec![0..5, 6..11],
                text: "a\nb".to_owned(),
                map: vec![
                    (0..1, Copied(4..5)),
                    (1..2, Synthetic),
                    (2..3, Copied(10..11)),
                ],
            },
            ("declaration", 12..24),
        ),
        (
            b"/**\n * a\n * b\n */\nint f(void);\n",
            comment(
                0..17,
                "a\nb",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..3, Copied(12..13)),
                ],
            ),
            ("declaration", 18..30),
        ),
    ];
    for (source, document, owner) in rows {
        assert_eq!(
            view(language, source),
            owned(document, vec![Holder::Node(owner)]),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "c")]
#[test]
fn c_comment_forms_are_cleaned_to_their_text() {
    comment_forms_are_cleaned_to_their_text(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_comment_forms_are_cleaned_to_their_text() {
    comment_forms_are_cleaned_to_their_text(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_comment_forms_are_cleaned_to_their_text() {
    comment_forms_are_cleaned_to_their_text(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_comment_forms_are_cleaned_to_their_text() {
    comment_forms_are_cleaned_to_their_text(LANG::Objc);
}

// These outcomes are pinned as they are today. Later units of issue 1671 change them on
// purpose, so this test is expected to move with them.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn positions_without_a_resolved_owner_keep_their_outcome(language: LANG) {
    let conditional = ("preproc_if", 9..55);
    assert_eq!(
        view(
            language,
            b"/** d */\n#if A\nvoid f(void);\n#else\nvoid g(void);\n#endif\n"
        ),
        View {
            documents: vec![comment(0..8, "d", [(0..1, Copied(4..5))])],
            // Today no branch declaration is listed as an owner, and the conditional is the
            // binding.
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Unresolved(UnresolvedReason::Conditional),
                owners: vec![],
                binding: Some(conditional.clone()),
            }],
            assessments: unordered([
                pending(AWAITS_OWNER, Some(conditional.clone()), Some(0)),
                pending(NEEDS_PROJECTION, Some(conditional), None),
                pending(NEEDS_PROJECTION, Some(("preproc_else", 29..48)), None),
            ]),
            ..View::default()
        }
    );
    assert_eq!(
        view(language, b"int f(void) {\n  /** note */\n  return 0;\n}\n"),
        View {
            documents: vec![comment(16..27, "note", [(0..4, Copied(20..24))])],
            assessments: unordered([pending(
                "Exact named matching awaits supported signature grammar",
                None,
                Some(0)
            )]),
            ..View::default()
        }
    );
    assert_eq!(
        view(language, b"int a, /**< ta */ b;\n"),
        View {
            documents: vec![comment(7..17, "ta", [(0..2, Copied(12..14))])],
            assessments: unordered([pending(AWAITS_OWNER, Some((",", 5..6)), Some(0))]),
            ..View::default()
        }
    );
    // Today the candidate inside an include guard is listed, as an uncertain one.
    assert_eq!(
        view(
            language,
            b"#ifndef G_H\n#define G_H\n/** d */\nint f(void);\n#endif\n"
        ),
        View {
            documents: vec![comment(24..32, "d", [(0..1, Copied(28..29))])],
            associations: vec![bound(
                0,
                Resolution::Unresolved(UnresolvedReason::Conditional),
                vec![Holder::Node(("declaration", 33..45))]
            )],
            assessments: unordered([pending(
                NEEDS_PROJECTION,
                Some(("preproc_ifdef", 0..52)),
                None
            )]),
            ..View::default()
        }
    );
    let variable = ("declaration", 33..39);
    assert_eq!(
        view(
            language,
            b"#ifndef G_H\n#define G_H\n/** d */\nint x;\n#endif\n"
        ),
        View {
            documents: vec![comment(24..32, "d", [(0..1, Copied(28..29))])],
            associations: vec![bound(
                0,
                Resolution::Unresolved(UnresolvedReason::Conditional),
                vec![field_slot(0, 37..38, &variable)]
            )],
            assessments: unordered([pending(
                NEEDS_PROJECTION,
                Some(("preproc_ifdef", 0..46)),
                None
            )]),
            ..View::default()
        }
    );
}

#[cfg(feature = "c")]
#[test]
fn c_positions_without_a_resolved_owner_keep_their_outcome() {
    positions_without_a_resolved_owner_keep_their_outcome(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_positions_without_a_resolved_owner_keep_their_outcome() {
    positions_without_a_resolved_owner_keep_their_outcome(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_positions_without_a_resolved_owner_keep_their_outcome() {
    positions_without_a_resolved_owner_keep_their_outcome(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_positions_without_a_resolved_owner_keep_their_outcome() {
    positions_without_a_resolved_owner_keep_their_outcome(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[allow(clippy::too_many_lines)]
fn grammar_shapes_the_unit_relies_on(language: LANG) {
    // Each declarator name is a `declarator` child of the declaration, and the comma
    // between two names is an unnamed child.
    let parser = clean(language, b"int a, b;\n");
    let declaration = node_at(&parser, "declaration", 0..9);
    assert_eq!(
        parts(declaration),
        [
            (Some("type"), "primitive_type", 0..3),
            (Some("declarator"), "identifier", 4..5),
            (None, ",", 5..6),
            (Some("declarator"), "identifier", 7..8),
            (None, ";", 8..9),
        ]
    );
    let comma = node_at(&parser, "identifier", 4..5).next_sibling().unwrap();
    assert_eq!(site(comma), (",", 5..6));
    assert!(!comma.is_named());

    // A prototype is a declaration whose declarator is a function declarator.
    let parser = clean(language, b"int f(void);\n");
    let declaration = node_at(&parser, "declaration", 0..12);
    let function = declaration.child_by_field_name("declarator").unwrap();
    assert_eq!(site(function), ("function_declarator", 4..11));
    assert_eq!(
        function.child_by_field_name("declarator").map(site),
        Some(("identifier", 4..5))
    );

    // A struct ends before its `;`. A field holds its `;`, and a comment after the field
    // is its next sibling and ends before the line feed.
    let source = b"struct S { int x; ///<audit\n};\n";
    let parser = clean(language, source);
    let terminator = node_at(&parser, "struct_specifier", 0..29)
        .next_sibling()
        .unwrap();
    assert_eq!(site(terminator), (";", 29..30));
    assert!(!terminator.is_named());
    let field = node_at(&parser, "field_declaration", 11..17);
    assert_eq!(
        parts(field),
        [
            (Some("type"), "primitive_type", 11..14),
            (Some("declarator"), "field_identifier", 15..16),
            (None, ";", 16..17),
        ]
    );
    let trailing = field.next_sibling().unwrap();
    assert_eq!(site(trailing), ("comment", 18..27));
    assert!(trailing.is_extra());
    assert_eq!(source[trailing.end_byte()], b'\n');

    // Comments, enumerators and the comma between them are children of the list.
    let parser = clean(language, b"enum E { /** la */ A, /** lb */ B };\n");
    assert_eq!(
        parts(node_at(&parser, "enumerator_list", 7..35)),
        [
            (None, "{", 7..8),
            (None, "comment", 9..18),
            (None, "enumerator", 19..20),
            (None, ",", 20..21),
            (None, "comment", 22..31),
            (None, "enumerator", 32..33),
            (None, "}", 34..35),
        ]
    );
    let comma = node_at(&parser, "enumerator", 19..20)
        .next_sibling()
        .unwrap();
    assert_eq!(site(comma), (",", 20..21));
    assert!(!comma.is_named());

    // A `#define` holds its line feed.
    let source = b"/** d */\n#define M 1\n";
    let parser = clean(language, source);
    let define = node_at(&parser, "preproc_def", 9..21);
    assert_eq!(define.end_byte(), source.len());
    assert_eq!(source[define.end_byte() - 1], b'\n');

    // A conditional holds its first branch, its `#else` branch as `alternative` and its
    // `#endif`, and ends before the final line feed.
    let source = b"/** d */\n#if A\nvoid f(void);\n#else\nvoid g(void);\n#endif\n";
    let parser = clean(language, source);
    let conditional = node_at(&parser, "preproc_if", 9..55);
    assert_eq!(
        parts(conditional),
        [
            (None, "#if", 9..12),
            (Some("condition"), "identifier", 13..14),
            (None, "\n", 14..15),
            (None, "declaration", 15..28),
            (Some("alternative"), "preproc_else", 29..48),
            (None, "#endif", 49..55),
        ]
    );
    assert_eq!(&source[conditional.end_byte()..], b"\n");

    // An include guard holds its `#define`, the comment and the declaration.
    let parser = clean(
        language,
        b"#ifndef G_H\n#define G_H\n/** d */\nint f(void);\n#endif\n",
    );
    assert_eq!(
        parts(node_at(&parser, "preproc_ifdef", 0..52)),
        [
            (None, "#ifndef", 0..7),
            (Some("name"), "identifier", 8..11),
            (None, "preproc_def", 12..24),
            (None, "comment", 24..32),
            (None, "declaration", 33..45),
            (None, "#endif", 46..52),
        ]
    );

    // A comment after a declarator's comma is a child of the declaration.
    let parser = clean(language, b"int a, /**< ta */ b;\n");
    let trailing = node_at(&parser, "comment", 7..17);
    assert_eq!(trailing.parent().map(site), Some(("declaration", 0..20)));
    let comma = trailing.prev_sibling().unwrap();
    assert_eq!(site(comma), (",", 5..6));
    assert!(!comma.is_named());

    // A comment in a function body is a child of the body.
    let parser = clean(language, b"int f(void) {\n  /** note */\n  return 0;\n}\n");
    let function = node_at(&parser, "function_definition", 0..41);
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(site(body), ("compound_statement", 12..41));
    assert_eq!(node_at(&parser, "comment", 16..27).parent(), Some(body));

    // A line comment ends before its line feed.
    let source = b"/// d\nint f(void);\n";
    let parser = clean(language, source);
    let line = node_at(&parser, "comment", 0..5);
    let declaration = line.next_sibling().unwrap();
    assert_eq!(site(declaration), ("declaration", 6..18));
    assert_eq!(&source[line.end_byte()..declaration.start_byte()], b"\n");

    // The kinds of a typedef, a union, an enum and a function definition at file scope.
    let parser = clean(
        language,
        b"typedef int T;\nunion U { int a; };\nenum E { A };\nint f(void) { return 0; }\n",
    );
    assert_eq!(
        named_parts(parser.ts_tree().root_node()),
        [
            ("type_definition", 0..14),
            ("union_specifier", 15..33),
            ("enum_specifier", 35..47),
            ("function_definition", 49..74),
        ]
    );
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn cpp_grammar_shapes(language: LANG) {
    // A class ends before its `;`. A member prototype is a field declaration whose
    // function declarator names a field identifier; an inline definition is a function
    // definition.
    let parser = clean(language, b"class C {\n  void m();\n  void n() { }\n};\n");
    let class = node_at(&parser, "class_specifier", 0..38);
    let terminator = class.next_sibling().unwrap();
    assert_eq!(site(terminator), (";", 38..39));
    assert!(!terminator.is_named());
    assert_eq!(
        named_parts(class.child_by_field_name("body").unwrap()),
        [
            ("field_declaration", 12..21),
            ("function_definition", 24..36),
        ]
    );
    let function = node_at(&parser, "field_declaration", 12..21)
        .child_by_field_name("declarator")
        .unwrap();
    assert_eq!(site(function), ("function_declarator", 17..20));
    assert_eq!(
        function.child_by_field_name("declarator").map(site),
        Some(("field_identifier", 17..18))
    );

    // A namespace and an alias are children of the root.
    let parser = clean(language, b"namespace N { }\nusing T = int;\n");
    assert_eq!(
        named_parts(parser.ts_tree().root_node()),
        [
            ("namespace_definition", 0..15),
            ("alias_declaration", 16..30),
        ]
    );
}

#[cfg(feature = "objc")]
fn objc_grammar_shapes() {
    // An interface holds its instance variables, properties and methods. An instance
    // variable and a property each wrap a struct declaration, not a field declaration.
    let parser = clean(
        LANG::Objc,
        b"@interface C {\n  int v;\n}\n@property int p;\n- (void)f;\n@end\n",
    );
    assert_eq!(
        named_parts(node_at(&parser, "class_interface", 0..58)),
        [
            ("identifier", 11..12),
            ("instance_variables", 13..25),
            ("property_declaration", 26..42),
            ("method_declaration", 43..53),
        ]
    );
    assert_eq!(
        named_parts(node_at(&parser, "instance_variable", 17..23)),
        [("struct_declaration", 17..23)]
    );
    assert_eq!(
        named_parts(node_at(&parser, "property_declaration", 26..42)),
        [("struct_declaration", 36..42)]
    );
}

#[cfg(feature = "c")]
#[test]
fn c_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Cpp);
    cpp_grammar_shapes(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Mozcpp);
    cpp_grammar_shapes(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Objc);
    objc_grammar_shapes();
}

// expected: forms.json has one three-fragment file document and one two-fragment
// function document; quoted and CDATA commands alone have no document.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_quoted_fixture(language: LANG) {
    let source = crate::test_support::documentation::source("doxygen-quoted.h");
    let source = source.as_slice();
    assert_eq!(clean(language, source).root_node().end_byte(), source.len());
    let expected = View {
        documents: vec![
            Doc {
                role: DocumentRole::FilePackage,
                composition: Composition::CommentRun,
                fragments: vec![0..14, 30..56, 72..88],
                text: "\"@file\"\n<![CDATA[ @file ]]>\n\"x\" @file".to_owned(),
                map: vec![
                    (0..7, Copied(4..11)),
                    (7..8, Synthetic),
                    (8..27, Copied(34..53)),
                    (27..28, Synthetic),
                    (28..37, Copied(76..85)),
                ],
            },
            Doc {
                role: DocumentRole::Primary,
                composition: Composition::CommentRun,
                fragments: vec![104..116, 117..130],
                text: "first\nsecond".to_owned(),
                map: vec![
                    (0..5, Copied(108..113)),
                    (5..6, Synthetic),
                    (6..12, Copied(121..127)),
                ],
            },
        ],
        associations: vec![
            Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::File(0..157)],
                binding: None,
            },
            bound(
                1,
                Resolution::Resolved,
                vec![Holder::Node(("function_definition", 131..156))],
            ),
        ],
        ..View::default()
    };
    assert_eq!(view(language, source), expected);
    // The sibling grammar must preserve the complete public view on the shared fixture.
    if language == LANG::Mozcpp {
        assert_eq!(view(LANG::Mozcpp, source), view(LANG::Cpp, source));
    }
    assert_eq!(
        view(
            language,
            &crate::test_support::documentation::source("doxygen-quote.h")
        ),
        View::default()
    );
    assert_eq!(
        view(
            language,
            &crate::test_support::documentation::source("doxygen-cdata.h")
        ),
        View::default()
    );
    // expected: both quoted structural lookalikes are detached. Retention
    // publishes their evidence in source order, not sorted by disposition.
    let source = b"/** \"@file\" */\n\n/** <![CDATA[ @file ]]> */\n";
    let parser = clean(language, source);
    let context = DocumentationContext::default();
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let root = parser.ts_tree().root_node();
    let typed = published(&parser.documentation(&context, &options).unwrap(), root);
    let raw = published(
        &collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
        root,
    );
    assert_eq!(typed, raw);
    assert_eq!(
        typed,
        View {
            evidence: vec![
                (CandidateDisposition::Unattached, 0..14),
                (CandidateDisposition::Unattached, 16..42),
            ],
            ..View::default()
        }
    );
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_gather_tail(language: LANG) {
    // expected: the first two comments gather as a file document; only the
    // last comment is bound forward to the prototype after the gatherer.
    assert_eq!(
        view(
            language,
            b"/** head */\n/** @file */\n/** tail */\nint f(void);\n"
        ),
        View {
            documents: vec![
                Doc {
                    role: DocumentRole::FilePackage,
                    composition: Composition::CommentRun,
                    fragments: vec![0..11, 12..24],
                    text: "head\n@file".to_owned(),
                    map: vec![
                        (0..4, Copied(4..8)),
                        (4..5, Synthetic),
                        (5..10, Copied(16..21))
                    ],
                },
                comment(25..36, "tail", [(0..4, Copied(29..33))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owners: vec![Holder::File(0..50)],
                    binding: None
                },
                bound(
                    1,
                    Resolution::Resolved,
                    vec![Holder::Node(("declaration", 37..49))]
                ),
            ],
            ..View::default()
        }
    );
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
#[allow(clippy::too_many_lines)]
fn unit2_comma(language: LANG) {
    // expected: the comma is an unnamed child of enumerator_list, and A is its
    // previous non-comment named sibling; the analogous variable remains pending.
    let parser = clean(language, b"enum E { A, /**< ta */ B };\n");
    let comma = node_at(&parser, ",", 10..11);
    assert!(!comma.is_named());
    assert_eq!(comma.parent().map(site), Some(("enumerator_list", 7..26)));
    assert_eq!(comma.prev_sibling().map(site), Some(("enumerator", 9..10)));
    assert_eq!(
        view(language, b"enum E { A, /**< ta */ B };\n"),
        owned(
            comment(12..22, "ta", [(0..2, Copied(17..19))]),
            vec![Holder::Node(("enumerator", 9..10))]
        )
    );
    let spaced = clean(language, b"enum E { A /* gap */, /**< ta */ B };\n");
    let gap_comment = node_at(&spaced, "comment", 11..20);
    assert_eq!(gap_comment.next_sibling().map(site), Some((",", 20..21)));
    assert_eq!(
        gap_comment.prev_sibling().map(site),
        Some(("enumerator", 9..10))
    );
    assert_eq!(
        view(language, b"enum E { A /* gap */, /**< ta */ B };\n"),
        owned(
            comment(22..32, "ta", [(0..2, Copied(27..29))]),
            vec![Holder::Node(("enumerator", 9..10))]
        )
    );
    // The excluded bytes hide the actual enumerator B between A and the comma.
    let source = b"enum E { A, B, /**< ta */ C };\n";
    let point = |at: usize| tree_sitter::Point::new(0, at);
    let ranges = [
        tree_sitter::Range {
            start_byte: 0,
            end_byte: 10,
            start_point: point(0),
            end_point: point(10),
        },
        tree_sitter::Range {
            start_byte: 13,
            end_byte: source.len(),
            start_point: point(13),
            end_point: point(source.len()),
        },
    ];
    let mut raw = tree_sitter::Parser::new();
    raw.set_language(&language.tree_sitter_language().unwrap())
        .unwrap();
    raw.set_included_ranges(&ranges).unwrap();
    let tree = raw.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let index = collect(
        language,
        &tree,
        source,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    let docs: Vec<_> = index.documents().collect();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get().role(), DocumentRole::Primary);
    assert_eq!(docs[0].get().composition(), Composition::Single);
    let fragment = index.facts().document_fragment(docs[0], 0).unwrap();
    assert_eq!(span(fragment.get().source()), 15..25);
    assert_eq!(fragment.get().form(), FragmentForm::Comment);
    assert_eq!(
        fragment.get().node().map(|node| site(node.node())),
        Some(("comment", 15..25))
    );
    assert_eq!(
        index
            .complete_text(docs[0])
            .unwrap()
            .map(|text| text.text().to_owned()),
        None
    );
    assert!(matches!(
        docs[0].get().content(),
        Content::Unavailable(UnavailableReason::ExcludedGap)
    ));
    let associations: Vec<_> = index.associations().collect();
    assert_eq!(associations.len(), 1);
    assert_eq!(
        index.facts().associated_document(associations[0]),
        Ok(docs[0])
    );
    assert_eq!(
        associations[0].get().resolution,
        Resolution::Unresolved(UnresolvedReason::ExcludedGap)
    );
    assert_eq!(associations[0].get().owner_count(), 0);
    assert!(associations[0].get().binding.is_none());
    assert_eq!(index.assessments().len(), 3);
    assert_eq!(index.evidence().len(), 0);
    if language == LANG::Mozcpp {
        assert_eq!(
            view(LANG::Mozcpp, b"enum E { A, /**< ta */ B };\n"),
            view(LANG::Cpp, b"enum E { A, /**< ta */ B };\n")
        );
    }
    // expected: this comma belongs to the call's argument_list, not the
    // enumerator_list. All initializer commas keep a pending document, not A.
    for (source, comma_at, comment_at, text_at, text) in [
        (
            b"enum E { A = foo(1, /**< bad */ 2), B };\n".as_slice(),
            18..19,
            20..31,
            25..28,
            "bad",
        ),
        (
            b"enum E { A = foo(1, /*!< bang */ 2), B };\n".as_slice(),
            18..19,
            20..32,
            25..29,
            "bang",
        ),
        (
            b"enum E { A = foo(bar(1, /**< nested */ 2)), B };\n".as_slice(),
            22..23,
            24..38,
            29..35,
            "nested",
        ),
    ] {
        let parser = clean(language, source);
        let comma = node_at(&parser, ",", comma_at.clone());
        assert_eq!(
            comma.parent().map(|parent| parent.kind()),
            Some("argument_list")
        );
        let expected = View {
            documents: vec![comment(
                comment_at,
                text,
                [(0..text.len(), Copied(text_at))],
            )],
            assessments: unordered([pending(AWAITS_OWNER, Some((",", comma_at)), Some(0))]),
            ..View::default()
        };
        assert_eq!(view(language, source), expected);
        let context = DocumentationContext::default();
        let options = DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        };
        let root = parser.ts_tree().root_node();
        let typed = published(&parser.documentation(&context, &options).unwrap(), root);
        let raw = published(
            &collect(
                language,
                parser.ts_tree(),
                parser.code(),
                &context,
                &options,
            )
            .unwrap(),
            root,
        );
        assert_eq!(typed, raw);
        assert_eq!(typed, expected);
    }
    // Recovery puts `#ifdef` in an ERROR before X in C, and Objc keeps
    // the comma within the preprocessor node. Neither may resolve X.
    let source = b"enum E {\n  A\n#ifdef X\n  , /**< ta */ B\n#endif\n};\n";
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 0);
    let assessments: Vec<_> = index.assessments().collect();
    assert!(assessments.iter().any(|assessment| {
        assessment.get().axis == AssessmentAxis::Owner
            && assessment.get().reason == AWAITS_OWNER
            && index.assessed_document(*assessment).unwrap().is_some()
    }));
    if language == LANG::C {
        assert_eq!(
            assessments[0].get().node.map(|node| site(node.node())),
            Some((",", 24..25))
        );
    }
    assert_eq!(
        view(language, b"int a, /**< ta */ b;\n"),
        View {
            documents: vec![comment(7..17, "ta", [(0..2, Copied(12..14))])],
            assessments: unordered([pending(AWAITS_OWNER, Some((",", 5..6)), Some(0))]),
            ..View::default()
        }
    );
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_linkage(language: LANG) {
    let one = b"/** e */\nextern \"C\" void f(void);\n";
    assert_eq!(
        view(language, one),
        View {
            documents: vec![comment(0..8, "e", [(0..1, Copied(4..5))])],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::Node(("declaration", 20..33))],
                binding: Some(("linkage_specification", 9..33)),
            }],
            ..View::default()
        }
    );
    if matches!(language, LANG::Cpp | LANG::Mozcpp) {
        // expected: the inner variable declaration has one Field slot;
        // the outer linkage specification remains the binding.
        assert_eq!(
            view(language, b"/** e */\nextern \"C++\" int x;\n"),
            View {
                documents: vec![comment(0..8, "e", [(0..1, Copied(4..5))])],
                associations: vec![Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owners: vec![field_slot(0, 26..27, &("declaration", 22..28))],
                    binding: Some(("linkage_specification", 9..28)),
                }],
                ..View::default()
            }
        );
    }
    // A comment inside the header is skipped, not projected as an owner.
    assert_eq!(
        view(
            language,
            b"/** e */\nextern /* between */ \"C\" /* between */ void f(void);\n"
        ),
        View {
            documents: vec![comment(0..8, "e", [(0..1, Copied(4..5))])],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::Node(("declaration", 48..61))],
                binding: Some(("linkage_specification", 9..61)),
            }],
            ..View::default()
        }
    );
    // A declaration_list cannot project to one declaration, even when a tool picks first.
    assert_eq!(
        view(
            language,
            b"/** e */\nextern \"C\" { void f(void); void g(void); }\n"
        ),
        View {
            documents: vec![comment(0..8, "e", [(0..1, Copied(4..5))])],
            assessments: unordered([
                pending(
                    AWAITS_OWNER,
                    Some(("linkage_specification", 9..51)),
                    Some(0)
                ),
                pending(
                    NEEDS_PROJECTION,
                    Some(("linkage_specification", 9..51)),
                    None
                ),
            ]),
            ..View::default()
        }
    );
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn unit2_template(language: LANG) {
    // expected: the outer template remains the binding for every projected inner owner.
    for (source, owner, binding) in [
        (
            b"/** t */\ntemplate <typename T> void f(T x);\n".as_slice(),
            ("declaration", 31..43),
            ("template_declaration", 9..43),
        ),
        (
            b"/** t */\ntemplate <typename T> void f(T x) {}\n".as_slice(),
            ("function_definition", 31..45),
            ("template_declaration", 9..45),
        ),
        (
            b"/** t */\ntemplate <typename T> class C {};\n".as_slice(),
            ("class_specifier", 31..41),
            ("template_declaration", 9..42),
        ),
        (
            b"/** t */\ntemplate <typename T> template <typename U> class C {};\n".as_slice(),
            ("class_specifier", 53..63),
            ("template_declaration", 9..64),
        ),
        (
            b"/** t */\ntemplate <typename T> /* between */ void f(T x);\n".as_slice(),
            ("declaration", 45..57),
            ("template_declaration", 9..57),
        ),
        (
            b"/** b */\ntemplate <typename T> requires true void g(T);\n".as_slice(),
            ("declaration", 45..55),
            ("template_declaration", 9..55),
        ),
        (
            b"/** p */\ntemplate <typename T> struct S<T*> {};\n".as_slice(),
            ("struct_specifier", 31..46),
            ("template_declaration", 9..47),
        ),
    ] {
        let text = match source.get(4).copied() {
            Some(b'b') => "b",
            Some(b'p') => "p",
            _ => "t",
        };
        assert_eq!(
            view(language, source),
            View {
                documents: vec![comment(0..8, text, [(0..1, Copied(4..5))])],
                associations: vec![Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owners: vec![Holder::Node(owner)],
                    binding: Some(binding),
                }],
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
    // Characterization: forward declarations and concepts have no supported
    // inner owner yet. The wrapper keeps a pending assessment, not a resolved
    // association and not an assertion of semantic absence.
    for (source, wrapper) in [
        (
            b"/** f */\ntemplate <typename T> struct S;\n".as_slice(),
            9..40,
        ),
        (
            b"/** f */\ntemplate <typename T> class C;\n".as_slice(),
            9..39,
        ),
        (
            b"/** f */\ntemplate <typename T> concept C = true;\n".as_slice(),
            9..48,
        ),
    ] {
        assert_eq!(
            view(language, source),
            View {
                documents: vec![comment(0..8, "f", [(0..1, Copied(4..5))])],
                assessments: unordered([
                    pending(
                        AWAITS_OWNER,
                        Some(("template_declaration", wrapper.clone())),
                        Some(0)
                    ),
                    pending(
                        NEEDS_PROJECTION,
                        Some(("template_declaration", wrapper)),
                        None
                    ),
                ]),
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_recovered_enumerator(language: LANG) {
    // expected: the error after A does not damage A itself or the preceding comma.
    let source = b"enum E { A, /**< ta */ %% B };\n";
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    assert_eq!(
        view_from_parser(language, &parser),
        View {
            documents: vec![comment(12..22, "ta", [(0..2, Copied(17..19))])],
            associations: vec![bound(
                0,
                Resolution::Resolved,
                vec![Holder::Node(("enumerator", 9..10))]
            )],
            assessments: unordered([pending(
                NEEDS_PROJECTION,
                Some(("enum_specifier", 0..29)),
                None
            ),]),
            ..View::default()
        }
    );
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn unit2_recovered_wrappers(language: LANG) {
    for (source, wrapper, declaration, label) in [
        (
            b"/** t */\ntemplate <typename T> void f(%%);\n".as_slice(),
            "template_declaration",
            31..42,
            "t",
        ),
        (
            b"/** e */\nextern \"C\" void f(%%);\n".as_slice(),
            "linkage_specification",
            20..31,
            "e",
        ),
    ] {
        let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
        assert!(parser.ts_tree().root_node().has_error());
        let wrapped = (wrapper, 9..declaration.end);
        let declaration = ("declaration", declaration);
        assert_eq!(
            view_from_parser(language, &parser),
            View {
                documents: vec![comment(0..8, label, [(0..1, Copied(4..5))])],
                assessments: unordered([
                    pending(
                        "Complex signature awaits exact named matching",
                        Some(declaration.clone()),
                        None
                    ),
                    pending(AWAITS_OWNER, Some(wrapped.clone()), Some(0)),
                    pending(NEEDS_PROJECTION, Some(wrapped), None),
                    pending(NEEDS_PROJECTION, Some(declaration), None),
                ]),
                ..View::default()
            }
        );
    }
}

// Characterization only: an unescaped physical LF inside an ordinary quoted
// string is invalid C/Objective-C. This pins today's grammar recovery, not
// conformant source documentation or an intended @file attachment.
#[cfg(any(feature = "c", feature = "objc"))]
fn unit2_invalid_multiline_quote(language: LANG) {
    let (source, fragment, text_start, scope, declaration, root_len) = match language {
        LANG::C => (
            b"const char *s = \"\n/** @file */\nint f(void);\n\";\n".as_slice(),
            18..30,
            22..27,
            ("string_literal", 16..30),
            ("declaration", 0..43),
            47,
        ),
        LANG::Objc => (
            b"id s = @\"\n/** @file */\nint f(void);\n\";\n".as_slice(),
            10..22,
            14..19,
            ("string_literal", 7..22),
            ("declaration", 0..35),
            39,
        ),
        other => panic!("not a characterization variant: {other:?}"),
    };
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    assert_eq!(
        node_at(&parser, "comment", fragment.clone())
            .parent()
            .map(site),
        Some(scope)
    );
    assert_eq!(
        view_from_parser(language, &parser),
        View {
            documents: vec![Doc {
                role: DocumentRole::FilePackage,
                ..comment(fragment, "@file", [(0..5, Copied(text_start))])
            }],
            associations: vec![Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::File(0..root_len)],
                binding: None
            }],
            assessments: unordered([pending(NEEDS_PROJECTION, Some(declaration), None)]),
            ..View::default()
        }
    );
}

#[cfg(feature = "c")]
#[test]
fn c_unit2_invalid_multiline_quote() {
    unit2_invalid_multiline_quote(LANG::C);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_invalid_multiline_quote() {
    unit2_invalid_multiline_quote(LANG::Objc);
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_literal_boundaries(language: LANG) {
    let ordinary = b"const char *s = \"\\n/** @file */\\nint f(void);\\n\";\nint f(void);\n";
    let line = b"// /** @file */ int f(void);\nint f(void);\n";
    let block = b"/*\n/** @file * /\nint f(void);\n*/\nint f(void);\n";
    for source in [ordinary.as_slice(), line.as_slice(), block.as_slice()] {
        let parser = clean(language, source);
        assert_eq!(parser.root_node().end_byte(), source.len());
        assert_eq!(view(language, source), View::default());
    }
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn unit2_cpp_raw_literal(language: LANG) {
    let source = b"const char *s = R\"(\n/** @file */\nint f(void);\n)\";\nint f(void);\n";
    assert_eq!(view(language, source), View::default());
}

#[cfg(feature = "objc")]
fn unit2_objc_literal() {
    let source = b"id s = @\"\\n/** @file */\\nint f(void);\\n\";\nint f(void);\n";
    assert_eq!(view(LANG::Objc, source), View::default());
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn unit2_line_gatherer_keeps_tail(language: LANG) {
    // expected: Doxygen's adjacent /// row below @file belongs to the file, too;
    // the block after those rows is a separate declaration document.
    let source = b"/// head\n/// @file\n/// after\n/** tail */\nint f(void);\n";
    let expected = View {
        documents: vec![
            Doc {
                role: DocumentRole::FilePackage,
                composition: Composition::CommentRun,
                fragments: vec![0..8, 9..18, 19..28],
                text: "head\n@file\nafter".to_owned(),
                map: vec![
                    (0..4, Copied(4..8)),
                    (4..5, Synthetic),
                    (5..10, Copied(13..18)),
                    (10..11, Synthetic),
                    (11..16, Copied(23..28)),
                ],
            },
            comment(29..40, "tail", [(0..4, Copied(33..37))]),
        ],
        associations: vec![
            Link {
                document: 0,
                resolution: Resolution::Resolved,
                owners: vec![Holder::File(0..54)],
                binding: None,
            },
            bound(
                1,
                Resolution::Resolved,
                vec![Holder::Node(("declaration", 41..53))],
            ),
        ],
        ..View::default()
    };
    assert_eq!(view(language, source), expected);
    // A retained candidate cannot mask a duplicate or a superseded tail.
    let parser = clean(language, source);
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let index = parser
        .documentation(&DocumentationContext::default(), &options)
        .unwrap();
    assert_eq!(published(&index, parser.ts_tree().root_node()), expected);
    assert_eq!(index.evidence().len(), 0);

    // A blank row ends the line gatherer's continuation, not the file document.
    assert_eq!(
        view(
            language,
            b"/// @file\n/// file second line\n\n/// func doc\nint f(void);\n"
        ),
        View {
            documents: vec![
                Doc {
                    role: DocumentRole::FilePackage,
                    composition: Composition::CommentRun,
                    fragments: vec![0..9, 10..30],
                    text: "@file\nfile second line".to_owned(),
                    map: vec![
                        (0..5, Copied(4..9)),
                        (5..6, Synthetic),
                        (6..22, Copied(14..30))
                    ]
                },
                comment(32..44, "func doc", [(0..8, Copied(36..44))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    resolution: Resolution::Resolved,
                    owners: vec![Holder::File(0..58)],
                    binding: None
                },
                bound(
                    1,
                    Resolution::Resolved,
                    vec![Holder::Node(("declaration", 45..57))]
                ),
            ],
            ..View::default()
        }
    );
}

#[cfg(feature = "c")]
#[derive(Default)]
struct Unit2Counting {
    steps: std::cell::Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "c")]
impl crate::documentation::budget::WorkBudget for Unit2Counting {
    type Error = SourceError;
    // test-lang-gates: hand-written(c) — method belongs to the c-gated budget implementation.
    #[cfg(feature = "c")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }
    // test-lang-gates: hand-written(c) — method belongs to the c-gated budget implementation.
    #[cfg(feature = "c")]
    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        if self.fail_at == Some(self.steps.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
    // test-lang-gates: hand-written(c) — method belongs to the c-gated budget implementation.
    #[cfg(feature = "c")]
    fn bytes(&self, _: usize) -> Result<(), SourceError> {
        Ok(())
    }
}

#[cfg(feature = "c")]
#[test]
fn c_unit2_comma_budget_has_two_sides_and_cancels_each_step() {
    let source = b"enum E { A /* gap */, /**< ta */ B };\n";
    let parser = clean(LANG::C, source);
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let comma = node_at(&parser, ",", 20..21);
    let budget = Unit2Counting::default();
    let (owner, gap) = super::enum_comma_predecessor(&context, comma, &budget).unwrap();
    assert_eq!(owner.map(site), Some(("enumerator", 9..10)));
    assert!(!gap);
    let steps = budget.steps.get();
    // expected: comment and enumerator, one gap lookup, one earlier-sibling check.
    assert_eq!(steps, 4);
    for fail_at in 1..=steps {
        let budget = Unit2Counting {
            fail_at: Some(fail_at),
            ..Unit2Counting::default()
        };
        assert!(matches!(
            super::enum_comma_predecessor(&context, comma, &budget),
            Err(SourceError::Cancelled)
        ));
        assert_eq!(budget.steps.get(), fail_at);
    }
}

#[cfg(feature = "c")]
#[test]
fn c_unit2_quoted_fixture() {
    unit2_quoted_fixture(LANG::C);
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_gather_tail() {
    unit2_gather_tail(LANG::C);
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_comma() {
    unit2_comma(LANG::C);
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_linkage() {
    unit2_linkage(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_quoted_fixture() {
    unit2_quoted_fixture(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_gather_tail() {
    unit2_gather_tail(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_comma() {
    unit2_comma(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_linkage() {
    unit2_linkage(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_template() {
    unit2_template(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_quoted_fixture() {
    unit2_quoted_fixture(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_gather_tail() {
    unit2_gather_tail(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_comma() {
    unit2_comma(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_linkage() {
    unit2_linkage(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_template() {
    unit2_template(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_quoted_fixture() {
    unit2_quoted_fixture(LANG::Objc);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_gather_tail() {
    unit2_gather_tail(LANG::Objc);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_comma() {
    unit2_comma(LANG::Objc);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_linkage() {
    unit2_linkage(LANG::Objc);
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_recovered_enumerator() {
    unit2_recovered_enumerator(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_recovered_enumerator() {
    unit2_recovered_enumerator(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_recovered_wrappers() {
    unit2_recovered_wrappers(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_recovered_enumerator() {
    unit2_recovered_enumerator(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_recovered_wrappers() {
    unit2_recovered_wrappers(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_recovered_enumerator() {
    unit2_recovered_enumerator(LANG::Objc);
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::Cpp);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_cpp_raw_literal() {
    unit2_cpp_raw_literal(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::Mozcpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_cpp_raw_literal() {
    unit2_cpp_raw_literal(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::Objc);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_objc_literal() {
    unit2_objc_literal();
}
#[cfg(feature = "c")]
#[test]
fn c_unit2_line_gatherer_keeps_tail() {
    unit2_line_gatherer_keeps_tail(LANG::C);
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_unit2_line_gatherer_keeps_tail() {
    unit2_line_gatherer_keeps_tail(LANG::Cpp);
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_unit2_line_gatherer_keeps_tail() {
    unit2_line_gatherer_keeps_tail(LANG::Mozcpp);
}
#[cfg(feature = "objc")]
#[test]
fn objc_unit2_line_gatherer_keeps_tail() {
    unit2_line_gatherer_keeps_tail(LANG::Objc);
}

// Count each new source-selection budget route apart from collection's other work.
#[cfg(feature = "c")]
#[derive(Default)]
struct Unit2SelectionCounting {
    steps: std::cell::Cell<usize>,
    byte_charges: std::cell::Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "c")]
impl crate::documentation::budget::WorkBudget for Unit2SelectionCounting {
    type Error = SourceError;
    // test-lang-gates: hand-written(c) — this method belongs to the c-only budget.
    #[cfg(feature = "c")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }
    // test-lang-gates: hand-written(c) — this method belongs to the c-only budget.
    #[cfg(feature = "c")]
    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        if self.fail_at == Some(self.steps.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
    // test-lang-gates: hand-written(c) — this method belongs to the c-only budget.
    #[cfg(feature = "c")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.byte_charges.set(self.byte_charges.get() + count);
        Ok(())
    }
}

#[cfg(feature = "c")]
fn unit2_selection_with_budget<'s>(
    parser: &'s AnyParser,
    context: &SourceContext<'s>,
    budget: &Unit2SelectionCounting,
) -> Result<Vec<crate::documentation::streaming::markers::MarkerRun<'s>>, SourceError> {
    let root = parser.root_node();
    let comment = root
        .children()
        .filter(|n| n.kind() == "comment")
        .last()
        .unwrap();
    let chain = [root];
    let ancestors = crate::Ancestors::known(&chain);
    let mut output = Vec::new();
    crate::documentation::streaming::markers::source_comment_with_budget::<
        crate::documentation::recognition::NativeRules<crate::CCode>,
        _,
    >(
        &comment,
        ancestors,
        &mut crate::documentation::streaming::markers::SourceRequest {
            context,
            retain_unbound: false,
            sink: &mut |run| output.push(run),
        },
        budget,
    )?;
    Ok(output)
}

#[cfg(feature = "c")]
#[test]
fn c_unit2_selection_budget_has_two_sides_and_cancels_each_step() {
    for source in [
        b"/** head */\n/** @file */\n/** tail */\nint f(void);\n".as_slice(),
        b"/// head\n/// @file\n/// after\n/** tail */\nint f(void);\n".as_slice(),
    ] {
        let parser = AnyParser::parse(LANG::C, source.to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error());
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let budget = Unit2SelectionCounting::default();
        let result = unit2_selection_with_budget(&parser, &context, &budget).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target.unwrap().node().kind(), "declaration");
        let steps = budget.steps.get();
        // expected: block form charges 41 steps, line continuation charges 70:
        // The block's 41 = target check 2 + backward stretch 6 + reverse 1
        // + gather-call 1 + gather probes 2 + gathered 2 + offset search 2
        // + bound-selection 1 + member/coverage/classification 24;
        // the contiguous line form's 70 adds 29 for one more gathered
        // member, its line adjacency check and the longer source scan.
        let expected = if source.starts_with(b"///") { 70 } else { 41 };
        assert!(steps >= expected, "{steps}");
        assert!(steps <= expected, "{steps}");
        assert!(budget.byte_charges.get() > 0);
        for fail_at in 1..=steps {
            let probe = Unit2SelectionCounting {
                fail_at: Some(fail_at),
                ..Unit2SelectionCounting::default()
            };
            assert!(
                matches!(
                    unit2_selection_with_budget(&parser, &context, &probe),
                    Err(SourceError::Cancelled)
                ),
                "{fail_at}"
            );
            assert_eq!(probe.steps.get(), fail_at);
        }
    }
}
