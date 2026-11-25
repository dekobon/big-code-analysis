//! JavaScript and Mozjs JSDoc source-view tests compare documents, associations, retained
//! candidates and pending assessments through both entry points. Node queries are asserted
//! separately where a case names them. The grammar tests collect nothing and pin tree shapes.

#[cfg(any(feature = "javascript", feature = "mozjs"))]
use crate::documentation::*;
#[cfg(any(feature = "javascript", feature = "mozjs"))]
use crate::{AnyParser, LANG};
#[cfg(any(feature = "javascript", feature = "mozjs"))]
use std::ops::Range;

#[cfg(any(feature = "javascript", feature = "mozjs"))]
use Run::{Copied, Decoded};

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    Node(&'static str, Range<usize>),
    File(Range<usize>),
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// Documents, ordered associations, retained candidates and unordered pending assessments.
/// Shared values are asserted by `published`. Node queries are asserted separately.
#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Holder>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
const AWAITS_OWNER: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(any(feature = "javascript", feature = "mozjs"))]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";

#[cfg(any(feature = "javascript", feature = "mozjs"))]
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

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn file(document: Doc) -> Doc {
    Doc {
        role: DocumentRole::FilePackage,
        ..document
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn owned(document: Doc, owner: Holder) -> View {
    View {
        documents: vec![document],
        associations: vec![owner],
        ..View::default()
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn node_owner(document: Doc, kind: &'static str, range: Range<usize>) -> View {
    owned(document, Holder::Node(kind, range))
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn pending(
    reason: &'static str,
    kind: &'static str,
    range: Range<usize>,
    doc: Option<usize>,
) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason,
        node: Some((kind, range)),
        document: doc,
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn unordered(entries: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = entries.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("comment", range.clone()))
    );
    range
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the four published lists and their shared rules.
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
        .enumerate()
        .map(|(at, association)| {
            let attachment = association.get();
            assert_eq!(attachment.owner_count(), 1);
            assert_eq!(attachment.convention.name(), "JSDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            assert_eq!(attachment.resolution, Resolution::Resolved);
            let document = facts.associated_document(association).unwrap();
            assert_eq!(ordered.get(at), Some(&document));
            let first = facts.document_fragment(document, 0).unwrap();
            let comment_node = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment_node.parent());
            let owner = facts.associated_owner(association, 0).unwrap();
            match owner.get() {
                Owner::Node(declared) => {
                    let node = declared.node();
                    let binding = if node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "export_statement")
                    {
                        node.parent()
                    } else {
                        Some(node)
                    };
                    assert_eq!(attachment.binding.map(NodeEvidence::node), binding);
                    let (kind, range) = site(node);
                    Holder::Node(kind, range)
                }
                Owner::File(file) => {
                    assert_eq!(file.node(), root);
                    assert!(file.node().parent().is_none());
                    assert!(attachment.binding.is_none());
                    Holder::File(file.node().byte_range())
                }
                other => panic!("a node or file owner, found {other:?}"),
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
    // This adapter is partial in every case; none has a named-reference command.
    assert_eq!(index.capability(), Capability::Partial);
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn clean(language: LANG, source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        source.escape_ascii()
    );
    parser
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            parser.language(),
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn agreed(indexes: &[DocumentationIndex<'_>; 2], root: tree_sitter::Node<'_>) -> View {
    let [typed, raw] = indexes.each_ref().map(|index| published(index, root));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn view(language: LANG, source: &[u8]) -> View {
    let parser = clean(language, source);
    agreed(
        &indexes(&parser, DocumentationOptions::default()),
        parser.ts_tree().root_node(),
    )
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn retained(language: LANG, source: &[u8]) -> View {
    let parser = clean(language, source);
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&parser, options), parser.ts_tree().root_node())
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
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

#[cfg(any(feature = "javascript", feature = "mozjs"))]
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

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn pending_query(indexes: &[DocumentationIndex<'_>; 2], node: tree_sitter::Node<'_>) {
    for index in indexes {
        let projection = index.node_projection(node).unwrap();
        assert!(projection.pending());
        let owners: Vec<_> = projection.owners().collect();
        assert_eq!(owners.len(), 0, "{owners:?}");
        let query = index.node_associations(node).unwrap();
        assert_eq!(query.associations().len(), 0);
        assert!(query.projection_pending());
    }
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn leading_block_documents_the_declaration_after_it(language: LANG) {
    let rows: [(&[u8], Doc, &str, Range<usize>); 3] = [
        (
            b"/**audit*/\nfunction f() {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            "function_declaration",
            11..26,
        ),
        (
            b"/** d */\nfunction* g() {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "generator_function_declaration",
            9..25,
        ),
        (
            b"/** d */\nclass C {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..19,
        ),
    ];
    for (source, document, kind, range) in rows {
        assert_eq!(
            view(language, source),
            node_owner(document, kind, range),
            "{}",
            source.escape_ascii()
        );
    }
    let source = b"/** a */\nfunction f() {}\n/** b */\nfunction g() {}\n";
    assert_eq!(
        view(language, source),
        View {
            documents: vec![
                comment(0..8, "a", [(0..1, Copied(4..5))]),
                comment(25..33, "b", [(0..1, Copied(29..30))]),
            ],
            associations: vec![
                Holder::Node("function_declaration", 9..24),
                Holder::Node("function_declaration", 34..49),
            ],
            ..View::default()
        }
    );
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[allow(clippy::type_complexity)]
fn class_and_object_members_own_their_leading_block(language: LANG) {
    let rows: [(&[u8], Range<usize>, &str, Range<usize>, &str); 5] = [
        (
            b"class C {\n  /** m */\n  m() {}\n}\n",
            12..20,
            "method_definition",
            23..29,
            "m",
        ),
        (
            b"class C {\n  /** x */\n  x = 1;\n}\n",
            12..20,
            "field_definition",
            23..28,
            "x",
        ),
        (
            b"use({\n  /** p */\n  p: 1,\n});\n",
            8..16,
            "pair",
            19..23,
            "p",
        ),
        (
            b"use({\n  /** q */\n  q,\n});\n",
            8..16,
            "shorthand_property_identifier",
            19..20,
            "q",
        ),
        (
            b"use({\n  /** m */\n  m() {},\n});\n",
            8..16,
            "method_definition",
            19..25,
            "m",
        ),
    ];
    for (source, fragment, kind, owner, text) in rows {
        let offset = fragment.start + 4;
        assert_eq!(
            view(language, source),
            node_owner(
                comment(fragment, text, [(0..1, Copied(offset..offset + 1))]),
                kind,
                owner
            ),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_class_and_object_members_own_their_leading_block() {
    class_and_object_members_own_their_leading_block(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_class_and_object_members_own_their_leading_block() {
    class_and_object_members_own_their_leading_block(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[allow(clippy::type_complexity)]
fn exported_declaration_owns_the_block_and_the_export_is_the_binding(language: LANG) {
    let rows: [(
        &[u8],
        Range<usize>,
        Range<usize>,
        Range<usize>,
        &str,
        Range<usize>,
        &str,
    ); 5] = [
        (
            b"/**audit*/\nexport function f() {}\n",
            0..10,
            3..8,
            11..33,
            "function_declaration",
            18..33,
            "audit",
        ),
        (
            b"/** d */\nexport function* g() {}\n",
            0..8,
            4..5,
            9..32,
            "generator_function_declaration",
            16..32,
            "d",
        ),
        (
            b"/** d */\nexport class C {}\n",
            0..8,
            4..5,
            9..26,
            "class_declaration",
            16..26,
            "d",
        ),
        (
            b"/** d */\nexport default function f() {}\n",
            0..8,
            4..5,
            9..39,
            "function_declaration",
            24..39,
            "d",
        ),
        (
            b"export class C {\n  /** m */\n  m() {}\n}\n",
            19..27,
            23..24,
            0..38,
            "method_definition",
            30..36,
            "m",
        ),
    ];
    for (source, fragment, copied, export, kind, owner, text) in rows {
        let parser = clean(language, source);
        let both = indexes(&parser, DocumentationOptions::default());
        assert_eq!(
            agreed(&both, parser.ts_tree().root_node()),
            node_owner(
                comment(
                    fragment.clone(),
                    text,
                    [(0..copied.len(), Copied(copied.clone()))]
                ),
                kind,
                owner.clone()
            ),
            "{}",
            source.escape_ascii()
        );
        if fragment.start == 0 {
            let wrapper = node_at(&parser, "export_statement", export);
            for index in &both {
                let projection = index.node_projection(wrapper).unwrap();
                assert!(!projection.pending());
                let owners: Vec<_> = projection.owners().collect();
                let [found] = owners[..] else {
                    panic!("one projected owner, found {owners:?}")
                };
                let Owner::Node(declared) = found.get() else {
                    panic!("node owner: {found:?}")
                };
                assert_eq!(site(declared.node()), (kind, owner.clone()));
                let query = index.node_associations(wrapper).unwrap();
                assert_eq!(query.associations().len(), 1);
                assert!(!query.projection_pending());
            }
        }
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_exported_declaration_owns_the_block_and_the_export_is_the_binding() {
    exported_declaration_owns_the_block_and_the_export_is_the_binding(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_exported_declaration_owns_the_block_and_the_export_is_the_binding() {
    exported_declaration_owns_the_block_and_the_export_is_the_binding(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn nearest_block_comment_before_a_declaration_decides(language: LANG) {
    assert_eq!(
        view(language, b"/** d */\n// plain\nfunction f() {}\n"),
        node_owner(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "function_declaration",
            18..33
        )
    );
    let superseded = b"function z() {}\n/**old*/\n/**new*/\nfunction f() {}\n";
    let mut expected = node_owner(
        comment(25..33, "new", [(0..3, Copied(28..31))]),
        "function_declaration",
        34..49,
    );
    assert_eq!(view(language, superseded), expected);
    expected
        .evidence
        .push((CandidateDisposition::Superseded, 16..24));
    assert_eq!(retained(language, superseded), expected);
    let detached = b"function z() {}\n/** d */\n/* plain */\nfunction f() {}\n";
    let mut expected = View::default();
    assert_eq!(view(language, detached), expected);
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 16..24));
    assert_eq!(retained(language, detached), expected);
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_nearest_block_comment_before_a_declaration_decides() {
    nearest_block_comment_before_a_declaration_decides(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_nearest_block_comment_before_a_declaration_decides() {
    nearest_block_comment_before_a_declaration_decides(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[allow(clippy::type_complexity)]
fn file_tag_documents_the_file(language: LANG) {
    let rows: [(&[u8], Range<usize>, &str, Range<usize>, Range<usize>); 5] = [
        (
            b"/**@file audit*/\nfunction f() {}\n",
            0..16,
            "@file audit",
            3..14,
            0..33,
        ),
        (
            b"/** @fileoverview audit */\nfunction f() {}\n",
            0..26,
            "@fileoverview audit",
            4..23,
            0..43,
        ),
        (
            b"/** @overview audit */\nfunction f() {}\n",
            0..22,
            "@overview audit",
            4..19,
            0..39,
        ),
        (
            b"/** @module m */\nfunction f() {}\n",
            0..16,
            "@module m",
            4..13,
            0..33,
        ),
        (
            b"/** @file audit */\n\"use strict\";\n",
            0..18,
            "@file audit",
            4..15,
            0..33,
        ),
    ];
    for (source, fragment, text, copied, root) in rows {
        assert_eq!(
            view(language, source),
            owned(
                file(comment(fragment, text, [(0..text.len(), Copied(copied))])),
                Holder::File(root)
            ),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_file_tag_documents_the_file() {
    file_tag_documents_the_file(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_file_tag_documents_the_file() {
    file_tag_documents_the_file(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn block_in_a_body_documents_a_nested_declaration_and_no_statement(language: LANG) {
    assert_eq!(
        view(
            language,
            b"function outer() { /**audit*/ function inner() {} }\n"
        ),
        node_owner(
            comment(19..29, "audit", [(0..5, Copied(22..27))]),
            "function_declaration",
            30..49
        )
    );
    assert_eq!(
        view(
            language,
            b"class C {\n  m() {\n    /** d */\n    function inner() {}\n  }\n}\n"
        ),
        node_owner(
            comment(22..30, "d", [(0..1, Copied(26..27))]),
            "function_declaration",
            35..54
        )
    );
    let without_owner = [
        (
            &b"function f() {\n    /** note */\n    return 1;\n}\n"[..],
            19..30,
        ),
        (&b"function f() {\n  /** note */\n  g();\n}\n"[..], 17..28),
    ];
    for (source, fragment) in without_owner {
        let mut expected = View::default();
        assert_eq!(view(language, source), expected);
        expected
            .evidence
            .push((CandidateDisposition::Unattached, fragment));
        assert_eq!(retained(language, source), expected);
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_block_in_a_body_documents_a_nested_declaration_and_no_statement() {
    block_in_a_body_documents_a_nested_declaration_and_no_statement(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_block_in_a_body_documents_a_nested_declaration_and_no_statement() {
    block_in_a_body_documents_a_nested_declaration_and_no_statement(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn comment_that_is_no_jsdoc_block_gives_no_document(language: LANG) {
    let sources: [&[u8]; 3] = [
        b"/* plain */\nfunction f() {}\n",
        b"// plain\nfunction f() {}\n",
        b"/*** banner */\nfunction f() {}\n",
    ];
    for source in sources {
        assert_eq!(
            view(language, source),
            View::default(),
            "{}",
            source.escape_ascii()
        );
        assert_eq!(
            retained(language, source),
            View::default(),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_comment_that_is_no_jsdoc_block_gives_no_document() {
    comment_that_is_no_jsdoc_block_gives_no_document(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_comment_that_is_no_jsdoc_block_gives_no_document() {
    comment_that_is_no_jsdoc_block_gives_no_document(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn block_text_loses_its_delimiters_and_one_star_per_line(language: LANG) {
    let rows: [(&[u8], Doc, Range<usize>); 5] = [
        (
            b"/**\n * a\n * b\n */\nfunction f() {}\n",
            comment(
                0..17,
                "a\nb",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..3, Copied(12..13)),
                ],
            ),
            18..33,
        ),
        (
            b"/**\n * a  \n *   b\n */\nfunction f() {}\n",
            comment(
                0..21,
                "a  \n  b",
                [
                    (0..3, Copied(7..10)),
                    (3..4, Copied(10..11)),
                    (4..7, Copied(14..17)),
                ],
            ),
            22..37,
        ),
        (
            b"/**\n * a\n *\n * b\n */\nfunction f() {}\n",
            comment(
                0..20,
                "a\n\nb",
                [
                    (0..1, Copied(7..8)),
                    (1..2, Copied(8..9)),
                    (2..3, Copied(11..12)),
                    (3..4, Copied(15..16)),
                ],
            ),
            21..36,
        ),
        (
            b"/** a\n * b */\nfunction f() {}\n",
            comment(
                0..13,
                "a\nb",
                [
                    (0..1, Copied(4..5)),
                    (1..2, Copied(5..6)),
                    (2..3, Copied(9..10)),
                ],
            ),
            14..29,
        ),
        (
            b"/**\n * @param {number} x the x\n * @returns {number}\n */\n\
              function f(x) { return x; }\n",
            comment(
                0..55,
                "@param {number} x the x\n@returns {number}",
                [
                    (0..23, Copied(7..30)),
                    (23..24, Copied(30..31)),
                    (24..41, Copied(34..51)),
                ],
            ),
            56..83,
        ),
    ];
    for (source, document, owner) in rows {
        assert_eq!(
            view(language, source),
            node_owner(document, "function_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_block_text_loses_its_delimiters_and_one_star_per_line() {
    block_text_loses_its_delimiters_and_one_star_per_line(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_block_text_loses_its_delimiters_and_one_star_per_line() {
    block_text_loses_its_delimiters_and_one_star_per_line(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn crlf_line_endings_keep_byte_ranges_and_decode_to_lf(language: LANG) {
    assert_eq!(
        view(language, b"/** d */\r\nfunction f() {}\r\n"),
        node_owner(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            "function_declaration",
            10..25
        )
    );
    assert_eq!(
        view(
            language,
            b"/**\r\n * a\r\n * b\r\n */\r\nfunction f() {}\r\n"
        ),
        node_owner(
            comment(
                0..20,
                "a\nb",
                [
                    (0..1, Copied(8..9)),
                    (1..2, Decoded(9..11)),
                    (2..3, Copied(14..15)),
                ]
            ),
            "function_declaration",
            22..37
        )
    );
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_crlf_line_endings_keep_byte_ranges_and_decode_to_lf() {
    crlf_line_endings_keep_byte_ranges_and_decode_to_lf(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_crlf_line_endings_keep_byte_ranges_and_decode_to_lf() {
    crlf_line_endings_keep_byte_ranges_and_decode_to_lf(LANG::Mozjs);
}

// The ownerless documents and pending assessments are today's outcome, not the later design.
#[cfg(any(feature = "javascript", feature = "mozjs"))]
fn declarations_awaiting_an_owner_keep_their_pending_outcome(language: LANG) {
    let ordinary: [(&[u8], &str, Range<usize>); 4] = [
        (
            b"/**audit*/\nconst f = () => 1;\n",
            "lexical_declaration",
            11..29,
        ),
        (
            b"/**audit*/\nconst a = 1, b = 2;\n",
            "lexical_declaration",
            11..30,
        ),
        (b"/**audit*/\nlet u;\n", "lexical_declaration", 11..17),
        (
            b"/**audit*/\nvar v = function () {};\n",
            "variable_declaration",
            11..34,
        ),
    ];
    for (source, kind, range) in ordinary {
        let parser = clean(language, source);
        let both = indexes(&parser, DocumentationOptions::default());
        assert_eq!(
            agreed(&both, parser.ts_tree().root_node()),
            View {
                documents: vec![comment(0..10, "audit", [(0..5, Copied(3..8))])],
                assessments: unordered([
                    pending(AWAITS_OWNER, kind, range.clone(), Some(0)),
                    pending(NEEDS_PROJECTION, kind, range.clone(), None),
                ]),
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
        pending_query(&both, node_at(&parser, kind, range));
    }
    let source = b"/**audit*/\nexport const e = 3;\n";
    let parser = clean(language, source);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both, parser.ts_tree().root_node()),
        View {
            documents: vec![comment(0..10, "audit", [(0..5, Copied(3..8))])],
            assessments: unordered([
                pending(AWAITS_OWNER, "export_statement", 11..30, Some(0)),
                pending(NEEDS_PROJECTION, "export_statement", 11..30, None),
                pending(NEEDS_PROJECTION, "lexical_declaration", 18..30, None),
            ]),
            ..View::default()
        }
    );
    for (kind, range) in [
        ("export_statement", 11..30),
        ("lexical_declaration", 18..30),
    ] {
        pending_query(&both, node_at(&parser, kind, range));
    }
    let anonymous: [(&[u8], Range<usize>); 2] = [
        (b"/**audit*/\nexport default function () {}\n", 11..40),
        (b"/**audit*/\nexport default class {}\n", 11..34),
    ];
    for (source, range) in anonymous {
        let parser = clean(language, source);
        let both = indexes(&parser, DocumentationOptions::default());
        assert_eq!(
            agreed(&both, parser.ts_tree().root_node()),
            View {
                documents: vec![comment(0..10, "audit", [(0..5, Copied(3..8))])],
                assessments: unordered([pending(
                    AWAITS_OWNER,
                    "export_statement",
                    range.clone(),
                    Some(0)
                )]),
                ..View::default()
            }
        );
        for index in &both {
            assert_eq!(
                index
                    .node_projection(node_at(&parser, "export_statement", range.clone()))
                    .err(),
                Some(OwnerError::UnsupportedOwner)
            );
        }
    }
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_declarations_awaiting_an_owner_keep_their_pending_outcome() {
    declarations_awaiting_an_owner_keep_their_pending_outcome(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_declarations_awaiting_an_owner_keep_their_pending_outcome() {
    declarations_awaiting_an_owner_keep_their_pending_outcome(LANG::Mozjs);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
#[allow(clippy::too_many_lines)]
fn grammar_shapes_the_unit_relies_on(language: LANG) {
    let source = b"function f() {}\nfunction* g() {}\nclass C {}\n";
    let parser = clean(language, source);
    let root = parser.ts_tree().root_node();
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        parts(root),
        [
            (None, "function_declaration", 0..15),
            (None, "generator_function_declaration", 16..32),
            (None, "class_declaration", 33..43),
        ]
    );

    let parser = clean(
        language,
        b"class C {\n  /** x */\n  x = 1;\n  /** m */\n  m() {}\n}\n",
    );
    let body = node_at(&parser, "class_body", 8..51);
    assert_eq!(
        parts(body),
        [
            (None, "{", 8..9),
            (None, "comment", 12..20),
            (Some("member"), "field_definition", 23..28),
            (None, ";", 28..29),
            (None, "comment", 32..40),
            (Some("member"), "method_definition", 43..49),
            (None, "}", 50..51),
        ]
    );
    assert!(!body.child(3).unwrap().is_named());
    for range in [12..20, 32..40] {
        assert!(node_at(&parser, "comment", range).is_extra());
    }

    let parser = clean(
        language,
        b"use({\n  /** p */\n  p: 1,\n  /** q */\n  q,\n  /** m */\n  m() {},\n});\n",
    );
    let statement = node_at(&parser, "expression_statement", 0..65);
    let call = node_at(&parser, "call_expression", 0..64);
    let arguments = node_at(&parser, "arguments", 3..64);
    let object = node_at(&parser, "object", 4..63);
    assert_eq!(call.parent(), Some(statement));
    assert_eq!(arguments.parent(), Some(call));
    assert_eq!(object.parent(), Some(arguments));
    assert_eq!(
        parts(object),
        [
            (None, "{", 4..5),
            (None, "comment", 8..16),
            (None, "pair", 19..23),
            (None, ",", 23..24),
            (None, "comment", 27..35),
            (None, "shorthand_property_identifier", 38..39),
            (None, ",", 39..40),
            (None, "comment", 43..51),
            (None, "method_definition", 54..60),
            (None, ",", 60..61),
            (None, "}", 62..63),
        ]
    );
    for at in [3, 6, 9] {
        assert!(!object.child(at).unwrap().is_named());
    }

    let parser = clean(language, b"/**audit*/\nexport function f() {}\n");
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [(None, "comment", 0..10), (None, "export_statement", 11..33),]
    );
    let wrapper = node_at(&parser, "export_statement", 11..33);
    assert_eq!(
        parts(wrapper),
        [
            (None, "export", 11..17),
            (Some("declaration"), "function_declaration", 18..33),
        ]
    );
    assert!(!wrapper.child(0).unwrap().is_named());
    assert_eq!(
        wrapper.child_by_field_name("declaration").map(site),
        Some(("function_declaration", 18..33))
    );

    let parser = clean(language, b"export default function f() {}\n");
    let wrapper = node_at(&parser, "export_statement", 0..30);
    assert_eq!(
        parts(wrapper),
        [
            (None, "export", 0..6),
            (None, "default", 7..14),
            (Some("declaration"), "function_declaration", 15..30),
        ]
    );
    for at in [0, 1] {
        assert!(!wrapper.child(at).unwrap().is_named());
    }

    let parser = clean(language, b"export default function () {}\n");
    let wrapper = node_at(&parser, "export_statement", 0..29);
    assert_eq!(
        wrapper.child_by_field_name("value").map(site),
        Some(("function_expression", 15..29))
    );
    let parser = clean(language, b"export default class {}\n");
    let wrapper = node_at(&parser, "export_statement", 0..23);
    assert_eq!(
        wrapper.child_by_field_name("value").map(site),
        Some(("class", 15..23))
    );
    assert!(wrapper.child_by_field_name("value").unwrap().is_named());

    let parser = clean(language, b"export const e = 3;\n");
    let wrapper = node_at(&parser, "export_statement", 0..19);
    assert_eq!(
        wrapper.child_by_field_name("declaration").map(site),
        Some(("lexical_declaration", 7..19))
    );
    assert_eq!(
        node_at(&parser, "variable_declarator", 13..18)
            .child_by_field_name("name")
            .map(site),
        Some(("identifier", 13..14))
    );

    let parser = clean(
        language,
        b"const f = () => 1;\nlet u;\nvar v = function () {};\n",
    );
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "lexical_declaration", 0..18),
            (None, "lexical_declaration", 19..25),
            (None, "variable_declaration", 26..49),
        ]
    );
    for (kind, range, declarator, semicolon) in [
        ("lexical_declaration", 0..18, 6..17, 17..18),
        ("lexical_declaration", 19..25, 23..24, 24..25),
        ("variable_declaration", 26..49, 30..48, 48..49),
    ] {
        let declaration = node_at(&parser, kind, range);
        assert!(parts(declaration).contains(&(None, "variable_declarator", declarator)));
        let last = declaration
            .child((declaration.child_count() - 1).try_into().unwrap())
            .unwrap();
        assert_eq!(site(last), (";", semicolon));
        assert!(!last.is_named());
    }

    let parser = clean(
        language,
        b"function outer() { /**audit*/ function inner() {} }\n",
    );
    let outer = node_at(&parser, "function_declaration", 0..51);
    let body = outer.child_by_field_name("body").unwrap();
    assert_eq!(site(body), ("statement_block", 17..51));
    let comment = node_at(&parser, "comment", 19..29);
    assert_eq!(comment.parent(), Some(body));
    assert_eq!(
        comment.next_sibling().map(site),
        Some(("function_declaration", 30..49))
    );

    let parser = clean(
        language,
        b"function f() {\n    /** note */\n    return 1;\n  g();\n}\n",
    );
    let body = node_at(&parser, "function_declaration", 0..53)
        .child_by_field_name("body")
        .unwrap();
    assert_eq!(site(body), ("statement_block", 13..53));
    assert_eq!(
        parts(body),
        [
            (None, "{", 13..14),
            (None, "comment", 19..30),
            (None, "return_statement", 35..44),
            (None, "expression_statement", 47..51),
            (None, "}", 52..53),
        ]
    );
    assert_eq!(
        node_at(&parser, "expression_statement", 47..51)
            .child(0)
            .map(site),
        Some(("call_expression", 47..50))
    );

    let source = b"/* plain */\n// plain\n/*** banner */\nfunction f() {}\n";
    let parser = clean(language, source);
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "comment", 0..11),
            (None, "comment", 12..20),
            (None, "comment", 21..35),
            (None, "function_declaration", 36..51),
        ]
    );
    for range in [0..11, 12..20, 21..35] {
        assert!(node_at(&parser, "comment", range).is_extra());
    }
    assert_eq!(
        source[node_at(&parser, "comment", 12..20).end_byte()],
        b'\n'
    );

    let source = b"/**\r\n * a\r\n * b\r\n */\r\nfunction f() {}\r\n";
    let parser = clean(language, source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("program", 0..39));
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        parts(root),
        [
            (None, "comment", 0..20),
            (None, "function_declaration", 22..37),
        ]
    );
    let comment = node_at(&parser, "comment", 0..20);
    assert_eq!(comment.end_position().row - comment.start_position().row, 3);
    assert_eq!(
        source[comment.byte_range()]
            .windows(2)
            .filter(|pair| *pair == b"\r\n")
            .count(),
        3
    );

    let source = b"function z() {}\n/**old*/\n/**new*/\nfunction f() {}\n";
    let parser = clean(language, source);
    let root = parser.ts_tree().root_node();
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        parts(root),
        [
            (None, "function_declaration", 0..15),
            (None, "comment", 16..24),
            (None, "comment", 25..33),
            (None, "function_declaration", 34..49),
        ]
    );

    let source = b"/** @file audit */\n\"use strict\";\n";
    let parser = clean(language, source);
    let root = parser.ts_tree().root_node();
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        parts(root),
        [
            (None, "comment", 0..18),
            (None, "expression_statement", 19..32),
        ]
    );
    assert_eq!(
        node_at(&parser, "expression_statement", 19..32)
            .child(0)
            .map(site),
        Some(("string", 19..31))
    );
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Javascript);
}
#[cfg(feature = "mozjs")]
#[test]
fn mozjs_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Mozjs);
}
