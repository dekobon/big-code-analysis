//! C# XML documentation in the source view: which declaration owns a comment, the text
//! of the document and where each part of that text comes from. Every source is collected
//! through both entry points, except in the last test, which pins the parse-tree shapes
//! the other tests rely on.

#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "csharp")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "csharp")]
use crate::documentation::*;
#[cfg(feature = "csharp")]
use crate::test_support::for_each_node_with_chain;
#[cfg(feature = "csharp")]
use crate::{Ancestors, AnyParser, Checker, CsharpCode, LANG};
#[cfg(feature = "csharp")]
use std::cell::Cell;
#[cfg(feature = "csharp")]
use std::ops::Range;

#[cfg(feature = "csharp")]
use Run::{Copied, Decoded, Synthetic};

/// The origin of one run of a document's text, with the source byte range it reads.
#[cfg(feature = "csharp")]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

/// One document: its role, its fragments as source byte ranges, its text, and each run
/// of the text as an output range with its origin.
#[cfg(feature = "csharp")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

/// What one collection published: the documents, the owner of each association as
/// resolution, node kind and byte range, each retained candidate as disposition and byte
/// range, and the number of pending assessments. The default value is an index that
/// holds none of them.
#[cfg(feature = "csharp")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    owners: Vec<(Resolution, &'static str, Range<usize>)>,
    slots: Vec<Vec<(SlotRole, usize, Range<usize>)>>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: usize,
}

/// The expected document of one comment.
#[cfg(feature = "csharp")]
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

/// The expected document of adjacent comments that join.
#[cfg(feature = "csharp")]
fn comment_run<const F: usize, const N: usize>(
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

/// The expected view of one document that one resolved node owns, with no retained
/// candidate and no pending assessment.
#[cfg(feature = "csharp")]
fn owned(document: Doc, kind: &'static str, owner: Range<usize>) -> View {
    View {
        documents: vec![document],
        owners: vec![(Resolution::Resolved, kind, owner)],
        slots: vec![Vec::new()],
        evidence: Vec::new(),
        assessments: 0,
    }
}

#[cfg(feature = "csharp")]
fn field_document(
    fragment: Range<usize>,
    text: &str,
    content: Range<usize>,
    kind: &'static str,
    declaration: Range<usize>,
    names: &[Range<usize>],
    resolution: Resolution,
) -> View {
    // bca: suppress(nargs) -- each argument independently pins document bytes or one field-owner property.
    View {
        documents: vec![comment(fragment, text, [(0..text.len(), Copied(content))])],
        owners: vec![(resolution, kind, declaration)],
        slots: vec![
            names
                .iter()
                .enumerate()
                .map(|(ordinal, name)| (SlotRole::Field, ordinal, name.clone()))
                .collect(),
        ],
        evidence: Vec::new(),
        assessments: 0,
    }
}

/// The byte range of a spelling that is one included piece, as every spelling here is.
#[cfg(feature = "csharp")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "csharp")]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "csharp")]
fn fragment_span(item: &Fragment<'_>) -> Range<usize> {
    assert_eq!(item.form(), FragmentForm::Comment);
    let range = span(item.source());
    let node = item.node().unwrap().node();
    assert_eq!(node.kind(), "comment");
    assert_eq!(node.byte_range(), range);
    range
}

#[cfg(feature = "csharp")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the published documents, associations, and retained evidence.
    let facts = index.facts();
    let documents = index
        .documents()
        .map(|document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| {
                        let fragment = facts.document_fragment(document, at).unwrap();
                        fragment_span(fragment.get())
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
    let associations: Vec<_> = index
        .associations()
        .enumerate()
        .map(|(position, association)| {
            // bca: suppress(halstead) -- one association's node or ordered slots share binding and scope checks.
            let attachment = association.get();
            let listed: Vec<_> = (0..attachment.owner_count())
                .map(|at| facts.associated_owner(association, at).unwrap())
                .collect();
            assert_ne!(attachment.owner_count(), 0);
            let (declaration, slots) = match listed[0].get() {
                Owner::Node(declared) => {
                    assert_eq!(listed.len(), 1);
                    (declared.node(), Vec::new())
                }
                Owner::Slot(_) => {
                    let mut found = Vec::new();
                    let mut enclosing = None;
                    for (ordinal, owner) in listed.iter().enumerate() {
                        let Owner::Slot(slot) = owner.get() else {
                            panic!("a field slot, found {:?}", owner.get())
                        };
                        let parent = facts.slot_enclosing(*owner).unwrap();
                        let Owner::Node(node) = parent.get() else {
                            panic!("a declaration, found {:?}", parent.get())
                        };
                        assert!(enclosing.is_none_or(|previous| previous == node.node()));
                        enclosing = Some(node.node());
                        assert_eq!(slot.ordinal, ordinal);
                        found.push((slot.role, slot.ordinal, span(&slot.source)));
                    }
                    (enclosing.unwrap(), found)
                }
                other => panic!("a declaration or field slot, found {other:?}"),
            };
            // Both follow from the tree: the binding is the owner itself, and the lexical
            // scope is the parent of the first comment of the document.
            assert_eq!(
                attachment.binding.map(NodeEvidence::node),
                Some(declaration)
            );
            let document = facts.associated_document(association).unwrap();
            assert_eq!(document, index.documents().nth(position).unwrap());
            assert_eq!(attachment.convention.name(), "CsharpXml");
            assert_eq!(attachment.publication, Publication::Unknown);
            let first = facts.document_fragment(document, 0).unwrap();
            let first_comment = first.get().node().unwrap().node();
            assert_eq!(
                Some(attachment.lexical_scope.node()),
                first_comment.parent()
            );
            (
                (
                    attachment.resolution,
                    declaration.kind(),
                    declaration.byte_range(),
                ),
                slots,
            )
        })
        .collect();
    let (owners, slots) = associations.into_iter().unzip();
    assert_eq!(index.capability(), Capability::Partial);
    assert_eq!(index.qualifications().len(), 0);
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    View {
        documents,
        owners,
        slots,
        evidence,
        assessments: index.assessments().len(),
    }
}

/// A parse whose tree holds no syntax error.
#[cfg(feature = "csharp")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Csharp, source.to_vec(), None, None).unwrap();
    assert!(!parser.root_node().has_error(), "{}", source.escape_ascii());
    parser
}

/// Both entry points over one parse: the typed parser's method, then `collect`.
#[cfg(feature = "csharp")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Csharp,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

/// The view that both entry points publish.
#[cfg(feature = "csharp")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2]) -> View {
    let [typed, raw] = indexes.each_ref().map(published);
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

/// The view of an error-free parse under the default options.
#[cfg(feature = "csharp")]
fn view(source: &[u8]) -> View {
    agreed(&indexes(&clean(source), DocumentationOptions::default()))
}

/// The view of an error-free parse that also retains the candidates no document uses.
#[cfg(feature = "csharp")]
fn retained(source: &[u8]) -> View {
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&clean(source), options))
}

/// The one node of that kind and byte range.
#[cfg(feature = "csharp")]
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

#[cfg(feature = "csharp")]
fn named_children(parent: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    parent.named_children(&mut parent.walk()).collect()
}

#[cfg(feature = "csharp")]
fn shapes(nodes: &[tree_sitter::Node<'_>]) -> Vec<(&'static str, Range<usize>)> {
    nodes
        .iter()
        .map(|found| (found.kind(), found.byte_range()))
        .collect()
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_declaration_kinds_own_their_leading_xml_documentation() {
    let types: [(&[u8], &str, Range<usize>); 5] = [
        (b"///audit\nstruct S {}", "struct_declaration", 9..20),
        (b"///audit\nrecord R(int A);", "record_declaration", 9..25),
        (b"///audit\ninterface I {}", "interface_declaration", 9..23),
        (b"///audit\nenum E { A }", "enum_declaration", 9..21),
        (
            b"///audit\ndelegate void D();",
            "delegate_declaration",
            9..27,
        ),
    ];
    for (source, kind, owner) in types {
        assert_eq!(
            view(source),
            owned(comment(0..8, "audit", [(0..5, Copied(3..8))]), kind, owner),
            "{kind}"
        );
    }
    let members: [(&[u8], &str, Range<usize>); 8] = [
        (
            b"class C {\n///audit\nvoid M() {}\n}",
            "method_declaration",
            19..30,
        ),
        (
            b"class C {\n///audit\nC() {}\n}",
            "constructor_declaration",
            19..25,
        ),
        (
            b"class C {\n///audit\n~C() {}\n}",
            "destructor_declaration",
            19..26,
        ),
        (
            b"class C {\n///audit\nint P { get; set; }\n}",
            "property_declaration",
            19..38,
        ),
        (
            b"class C {\n///audit\nint this[int i] { get => i; }\n}",
            "indexer_declaration",
            19..48,
        ),
        (
            b"class C {\n///audit\nevent System.Action F { add { } remove { } }\n}",
            "event_declaration",
            19..63,
        ),
        (
            b"class C {\n///audit\npublic static C operator +(C a, C b) => a;\n}",
            "operator_declaration",
            19..61,
        ),
        (
            b"class C {\n///audit\npublic static explicit operator int(C a) => 1;\n}",
            "conversion_operator_declaration",
            19..65,
        ),
    ];
    for (source, kind, owner) in members {
        assert_eq!(
            view(source),
            owned(
                comment(10..18, "audit", [(0..5, Copied(13..18))]),
                kind,
                owner
            ),
            "{kind}"
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_expression_bodied_property_and_indexer_own_their_documentation() {
    let members: [(&[u8], &str, Range<usize>); 2] = [
        (
            b"class C {\n///<summary>audit</summary>\nint P => 1;\n}",
            "property_declaration",
            38..49,
        ),
        (
            b"class C {\n///<summary>audit</summary>\nint this[int x] => x;\n}",
            "indexer_declaration",
            38..59,
        ),
    ];
    for (source, kind, owner) in members {
        let parser = clean(source);
        let searches = crate::checker::csharp_accessor_searches::observed();
        let both = indexes(&parser, DocumentationOptions::default());
        // expected: 0, because the accessor search belongs to the callable test of a
        // property or an indexer, and neither collection asks that test.
        assert_eq!(
            crate::checker::csharp_accessor_searches::observed() - searches,
            0,
            "{kind}"
        );
        // The control. expected: one accessor search per callable test of a property or
        // an indexer.
        let mut asked = 0;
        for_each_node_with_chain::<CsharpCode>(source, |member, chain| {
            if member.kind() != kind || (member.start_byte()..member.end_byte()) != owner {
                return;
            }
            let searches = crate::checker::csharp_accessor_searches::observed();
            assert!(CsharpCode::is_func_with_code(
                member,
                source,
                Ancestors::known(chain)
            ));
            assert_eq!(
                crate::checker::csharp_accessor_searches::observed() - searches,
                1,
                "{kind}"
            );
            asked += 1;
        });
        assert_eq!(asked, 1, "{kind}");
        assert_eq!(
            agreed(&both),
            owned(
                comment(
                    10..37,
                    "<summary>audit</summary>",
                    [(0..24, Copied(13..37))]
                ),
                kind,
                owner
            ),
            "{kind}"
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_enum_member_owns_its_documentation() {
    assert_eq!(
        view(b"enum E {\n///audit\nA\n}"),
        owned(
            comment(9..17, "audit", [(0..5, Copied(12..17))]),
            "enum_member_declaration",
            18..19
        )
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_adjacent_documentation_lines_join_into_one_run() {
    assert_eq!(
        view(b"///a\n///b\nclass C {}"),
        owned(
            comment_run(
                [0..4, 5..9],
                "a\nb",
                [
                    (0..1, Copied(3..4)),
                    (1..2, Synthetic),
                    (2..3, Copied(8..9))
                ]
            ),
            "class_declaration",
            10..20
        )
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_plain_comment_ends_the_run_and_older_lines_are_superseded() {
    let source = b"///old\n// plain\n///a\n///b\nclass C {}";
    let mut expected = owned(
        comment_run(
            [16..20, 21..25],
            "a\nb",
            [
                (0..1, Copied(19..20)),
                (1..2, Synthetic),
                (2..3, Copied(24..25)),
            ],
        ),
        "class_declaration",
        26..36,
    );
    // The line at 0..6 is in no document. It is kept as superseded only on request.
    assert_eq!(view(source), expected);
    expected
        .evidence
        .push((CandidateDisposition::Superseded, 0..6));
    assert_eq!(retained(source), expected);
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_plain_comments_and_directives_do_not_detach_documentation() {
    let rows: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"/// a\n// p\nclass C {}",
            comment(0..5, "a", [(0..1, Copied(4..5))]),
            11..21,
        ),
        (
            b"///d\n#nullable enable\nclass C {}",
            comment(0..4, "d", [(0..1, Copied(3..4))]),
            22..32,
        ),
        (
            b"///a\n#region R\n///b\nclass C {}\n#endregion\n",
            comment(15..19, "b", [(0..1, Copied(18..19))]),
            20..30,
        ),
        (
            b"///r\n#region R\nclass C {}\n#endregion\n",
            comment(0..4, "r", [(0..1, Copied(3..4))]),
            15..25,
        ),
    ];
    for (source, document, owner) in rows {
        assert_eq!(
            view(source),
            owned(document, "class_declaration", owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_documentation_attaches_before_an_attribute_and_not_after_it() {
    assert_eq!(
        view(b"///<summary>audit</summary>\n[System.Obsolete] class C {}"),
        owned(
            comment(0..27, "<summary>audit</summary>", [(0..24, Copied(3..27))]),
            "class_declaration",
            28..56
        )
    );
    let after = b"[System.Obsolete]\n///audit\nclass C {}";
    assert_eq!(view(after), View::default());
    assert_eq!(
        retained(after),
        View {
            evidence: vec![(CandidateDisposition::Unattached, 18..26)],
            ..View::default()
        }
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_block_documentation_is_cleaned_as_a_block_comment() {
    assert_eq!(
        view(b"/** audit */\nclass C {}"),
        owned(
            comment(0..12, "audit", [(0..5, Copied(4..9))]),
            "class_declaration",
            13..23
        )
    );
    assert_eq!(
        view(b"/**\n * <summary>\n *   two\n * </summary>\n */\nclass C {}"),
        owned(
            comment(
                0..43,
                "<summary>\n  two\n</summary>",
                [
                    (0..9, Copied(7..16)),
                    (9..10, Copied(16..17)),
                    (10..15, Copied(20..25)),
                    (15..16, Copied(25..26)),
                    (16..26, Copied(29..39)),
                ]
            ),
            "class_declaration",
            44..54
        )
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_xml_markup_stays_verbatim() {
    assert_eq!(
        view(b"/// <summary>\n/// a &lt; b\n/// </summary>\nclass C {}"),
        owned(
            comment_run(
                [0..13, 14..26, 27..41],
                "<summary>\na &lt; b\n</summary>",
                [
                    (0..9, Copied(4..13)),
                    (9..10, Synthetic),
                    (10..18, Copied(18..26)),
                    (18..19, Synthetic),
                    (19..29, Copied(31..41)),
                ]
            ),
            "class_declaration",
            42..52
        )
    );
    assert_eq!(
        view(b"class C {\n/// <inheritdoc/>\nvoid F() {}\n}"),
        owned(
            comment(10..27, "<inheritdoc/>", [(0..13, Copied(14..27))]),
            "method_declaration",
            28..39
        )
    );
    assert_eq!(
        view(b"/// <summary></summary>\nclass C {}"),
        owned(
            comment(0..23, "<summary></summary>", [(0..19, Copied(4..23))]),
            "class_declaration",
            24..34
        )
    );
    assert_eq!(
        view(b"/// <include file=\"d.xml\" path=\"x\"/>\nclass C {}"),
        owned(
            comment(
                0..36,
                "<include file=\"d.xml\" path=\"x\"/>",
                [(0..32, Copied(4..36))]
            ),
            "class_declaration",
            37..47
        )
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_bare_documentation_line_is_a_present_empty_document() {
    assert_eq!(
        view(b"///\nclass C {}"),
        owned(comment(0..3, "", []), "class_declaration", 4..14)
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_type_with_primary_constructor_owns_its_documentation() {
    let source = crate::test_support::documentation::source("csharp-primary.cs");
    assert_eq!(
        source,
        b"/// <summary>type</summary>\nclass C(int x) { int F() { return x + 1; } }\n"
    );
    let parser = clean(&source);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        owned(
            comment(0..27, "<summary>type</summary>", [(0..23, Copied(4..27))]),
            "class_declaration",
            28..72
        )
    );
    let class = node_at(&parser, "class_declaration", 28..72);
    let parameters = node_at(&parser, "parameter_list", 35..42);
    for index in &both {
        let query = index.node_associations(class).unwrap();
        assert_eq!(query.associations().len(), 1);
        assert!(!query.projection_pending());
        assert_eq!(query.assessments().count(), 0);
        assert_eq!(
            index.node_projection(parameters).err(),
            Some(OwnerError::UnsupportedOwner)
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_excluded_placements_give_no_document() {
    let kept_on_request: [(&[u8], Range<usize>); 8] = [
        (b"class C { int P { /**audit*/ get; } }", 18..28),
        (b"class C(/**audit*/ int x) {}", 8..18),
        (b"class C { void F() { ///audit\nint x = 1; } }", 21..29),
        (b"///audit\nnamespace N { }", 0..8),
        (b"///audit\nnamespace N;", 0..8),
        (b"///audit\nusing System;", 0..8),
        (b"class C {\n///audit\n}", 10..18),
        (b"class C {}\n///audit", 11..19),
    ];
    for (source, unattached) in kept_on_request {
        assert_eq!(view(source), View::default(), "{}", source.escape_ascii());
        assert_eq!(
            retained(source),
            View {
                evidence: vec![(CandidateDisposition::Unattached, unattached)],
                ..View::default()
            },
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_documentation_on_a_code_row_attaches_to_the_next_declaration() {
    assert_eq!(
        view(b"/** t */ class D { }"),
        owned(
            comment(0..8, "t", [(0..1, Copied(4..5))]),
            "class_declaration",
            9..20
        )
    );
    assert_eq!(
        view(b"class C { void A() {} /// d\nvoid B() {} }"),
        owned(
            comment(22..27, "d", [(0..1, Copied(26..27))]),
            "method_declaration",
            28..39
        )
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_line_endings_and_trailing_documentation() {
    assert_eq!(
        view(b"///audit\r\nclass C {}\r\n"),
        owned(
            comment(0..8, "audit", [(0..5, Copied(3..8))]),
            "class_declaration",
            10..20
        )
    );
    assert_eq!(
        view(b"/**a\r\n * b*/\r\nclass C {}"),
        owned(
            comment(
                0..12,
                "a\nb",
                [
                    (0..1, Copied(3..4)),
                    (1..2, Decoded(4..6)),
                    (2..3, Copied(9..10)),
                ]
            ),
            "class_declaration",
            14..24
        )
    );
    let tail = b"///audit\nclass C {}\n/// tail";
    let mut expected = owned(
        comment(0..8, "audit", [(0..5, Copied(3..8))]),
        "class_declaration",
        9..19,
    );
    // The line at 20..28 has no declaration after it and is in no document.
    assert_eq!(view(tail), expected);
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 20..28));
    assert_eq!(retained(tail), expected);
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_grammar_shapes_the_unit_relies_on() {
    // Attributes and modifiers are inside the declaration, which starts at the attribute.
    let parser = clean(b"[System.Obsolete] public class C {}");
    let top = named_children(parser.ts_tree().root_node());
    assert_eq!(shapes(&top), [("class_declaration", 0..35)]);
    assert_eq!(
        shapes(&named_children(top[0])),
        [
            ("attribute_list", 0..17),
            ("modifier", 18..24),
            ("identifier", 31..32),
            ("declaration_list", 33..35),
        ]
    );

    // A directive that wraps no code is an extra sibling of the declarations.
    let parser = clean(
        b"#define X\n#undef X\n#region R\n#endregion\n#line 1\n#pragma warning disable\n\
          #nullable enable\n#error e\n#warning w\nclass C {}",
    );
    let top = named_children(parser.ts_tree().root_node());
    assert_eq!(
        shapes(&top),
        [
            ("preproc_define", 0..10),
            ("preproc_undef", 10..19),
            ("preproc_region", 19..29),
            ("preproc_endregion", 29..40),
            ("preproc_line", 40..48),
            ("preproc_pragma", 48..72),
            ("preproc_nullable", 72..89),
            ("preproc_error", 89..98),
            ("preproc_warning", 98..109),
            ("class_declaration", 109..119),
        ]
    );
    assert!(top[..9].iter().all(tree_sitter::Node::is_extra));
    assert!(!top[9].is_extra());

    // `#if` around members is a node of the body that holds them.
    let parser = clean(b"class C {\n#if A\nvoid F() {}\n#endif\n}");
    let top = named_children(parser.ts_tree().root_node());
    assert_eq!(shapes(&top), [("class_declaration", 0..36)]);
    let class = named_children(top[0]);
    assert_eq!(
        shapes(&class),
        [("identifier", 6..7), ("declaration_list", 8..36)]
    );
    let body = named_children(class[1]);
    assert_eq!(shapes(&body), [("preproc_if", 10..34)]);
    assert_eq!(
        shapes(&named_children(body[0])),
        [("identifier", 14..15), ("method_declaration", 16..27)]
    );

    // `#if` around an attribute is inside the declaration, which starts at the `#if`.
    let parser = clean(b"#if A\n[System.Obsolete]\n#endif\nclass C {}");
    let top = named_children(parser.ts_tree().root_node());
    assert_eq!(shapes(&top), [("class_declaration", 0..41)]);
    assert_eq!(
        shapes(&named_children(top[0])),
        [
            ("preproc_if_in_attribute_list", 0..30),
            ("identifier", 37..38),
            ("declaration_list", 39..41),
        ]
    );

    // A primary constructor is a parameter list directly under the type.
    let parser = clean(b"class C(int x) {}");
    let top = named_children(parser.ts_tree().root_node());
    assert_eq!(shapes(&top), [("class_declaration", 0..17)]);
    assert_eq!(
        shapes(&named_children(top[0])),
        [
            ("identifier", 6..7),
            ("parameter_list", 7..14),
            ("declaration_list", 15..17),
        ]
    );

    // A line comment ends before its line ending, whichever it is.
    let endings: [(&[u8], &[u8], Range<usize>); 2] = [
        (b"///audit\r\nclass C {}\r\n", b"\r\n", 10..20),
        (b"///audit\nclass C {}", b"\n", 9..19),
    ];
    for (source, ending, class) in endings {
        let parser = clean(source);
        let top = named_children(parser.ts_tree().root_node());
        assert_eq!(
            shapes(&top),
            [("comment", 0..8), ("class_declaration", class)]
        );
        assert!(top[0].is_extra());
        assert_eq!(&source[top[0].end_byte()..top[1].start_byte()], ending);
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_two_declarations_keep_document_and_association_order() {
    // Both independent comments precede different already-supported declarations.
    let source = concat!(
        "class C {\n///first\nint P { get; set; }\n",
        "///second\nevent System.Action E { add {} remove {} }\n}"
    )
    .as_bytes();
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    let seen = agreed(&both);
    let first = owned(
        comment(10..18, "first", [(0..5, Copied(13..18))]),
        "property_declaration",
        19..38,
    );
    let second = owned(
        comment(39..48, "second", [(0..6, Copied(42..48))]),
        "event_declaration",
        49..91,
    );
    assert_eq!(
        seen,
        View {
            documents: [first.documents[0].clone(), second.documents[0].clone()].into(),
            owners: [first.owners[0].clone(), second.owners[0].clone()].into(),
            slots: [first.slots[0].clone(), second.slots[0].clone()].into(),
            ..View::default()
        }
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_local_and_top_level_fields_do_not_publish_documents() {
    let local = b"class C { void M() { ///local\nint x = 1; } }";
    let top = b"///top\nint x = 1;";
    for (source, range) in [(local.as_slice(), 21..29), (top, 0..6)] {
        assert_eq!(view(source), View::default());
        assert_eq!(
            retained(source),
            View {
                evidence: vec![(CandidateDisposition::Unattached, range)],
                ..View::default()
            }
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
#[allow(clippy::type_complexity, clippy::single_range_in_vec_init)]
fn csharp_field_const_and_event_names_each_own_one_field_slot() {
    let cases: [(&[u8], Range<usize>, Range<usize>, &str, &[Range<usize>]); 3] = [
        (
            b"class C {\n///both\nint a, b;\n}",
            10..17,
            18..27,
            "field_declaration",
            &[22..23, 25..26],
        ),
        (
            b"class C {\n///one\nconst int K = 1;\n}",
            10..16,
            17..33,
            "field_declaration",
            &[27..28],
        ),
        (
            b"class C {\n///both\nevent System.Action E1, E2;\n}",
            10..17,
            18..45,
            "event_field_declaration",
            &[38..40, 42..44],
        ),
    ];
    for (source, fragment, declaration, kind, names) in cases {
        let text = if kind == "field_declaration" && names.len() == 1 {
            "one"
        } else {
            "both"
        };
        let content = fragment.start + 3..fragment.end;
        assert_eq!(
            view(source),
            field_document(
                fragment,
                text,
                content,
                kind,
                declaration,
                names,
                Resolution::Resolved,
            ),
            "{}",
            source.escape_ascii(),
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_field_modifiers_fixed_buffers_comments_and_unicode_keep_name_slots() {
    for (source, fragment, text, owner, names) in [
        (
            b"class C {\n///both\n[System.Obsolete] private static readonly int a = 1, b;\n}"
                .as_slice(),
            10..17,
            "both",
            18..73,
            vec![64..65, 71..72],
        ),
        (
            b"unsafe class C {\n///buf\nfixed int a[4];\n}".as_slice(),
            17..23,
            "buf",
            24..39,
            vec![34..35],
        ),
        (
            b"class C {\n///both\nint /* c */ a, /* c */ b;\n}".as_slice(),
            10..17,
            "both",
            18..43,
            vec![30..31, 41..42],
        ),
        (
            "class C {\n///unicode\nint Δ;\n}".as_bytes(),
            10..20,
            "unicode",
            21..28,
            vec![25..27],
        ),
        (
            b"class C {\n///unnamed\nint _;\n}".as_slice(),
            10..20,
            "unnamed",
            21..27,
            vec![25..26],
        ),
    ] {
        assert_eq!(
            view(source),
            field_document(
                fragment.clone(),
                text,
                fragment.start + 3..fragment.end,
                "field_declaration",
                owner,
                &names,
                Resolution::Resolved,
            ),
            "{}",
            source.escape_ascii(),
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_later_documentation_inside_a_field_is_only_retained_evidence() {
    let source = b"class C {\n///both\nint a, ///later\nb;\n}";
    let expected = field_document(
        10..17,
        "both",
        13..17,
        "field_declaration",
        18..36,
        &[22..23, 34..35],
        Resolution::Resolved,
    );
    assert_eq!(view(source), expected);
    let mut retained_view = expected;
    retained_view
        .evidence
        .push((CandidateDisposition::Unattached, 25..33));
    assert_eq!(retained(source), retained_view);
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_two_unattached_fragments_keep_their_source_order() {
    let source = b"class C {\n///first\nint a, ///left\nb;\n// plain\n///right\n}";
    let mut expected = field_document(
        10..18,
        "first",
        13..18,
        "field_declaration",
        19..36,
        &[23..24, 34..35],
        Resolution::Resolved,
    );
    assert_eq!(view(source), expected);
    expected.evidence = vec![
        (CandidateDisposition::Unattached, 26..33),
        (CandidateDisposition::Unattached, 46..54),
    ];
    assert_eq!(retained(source), expected);
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_field_and_event_documents_remain_distinct_in_one_class() {
    let source = b"class C {\n///first\nint a;\n///second\nevent System.Action B;\n}";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    let actual = agreed(&both);
    let first = field_document(
        10..18,
        "first",
        13..18,
        "field_declaration",
        19..25,
        &[23..24],
        Resolution::Resolved,
    );
    let second = field_document(
        26..35,
        "second",
        29..35,
        "event_field_declaration",
        36..58,
        &[56..57],
        Resolution::Resolved,
    );
    assert_eq!(
        actual,
        View {
            documents: [first.documents[0].clone(), second.documents[0].clone()].into(),
            owners: [first.owners[0].clone(), second.owners[0].clone()].into(),
            slots: [first.slots[0].clone(), second.slots[0].clone()].into(),
            evidence: Vec::new(),
            assessments: 0,
        }
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_error_field_constant_and_event_declarations_stay_pending() {
    for (source, kind, range) in [
        (
            b"class C {\n///broken\nint a = ;\n}".as_slice(),
            "field_declaration",
            20..29,
        ),
        (
            b"class C {\n///broken\nconst int K = ;\n}".as_slice(),
            "field_declaration",
            20..35,
        ),
        (
            b"class C {\n///broken\nevent System.Action E = ;\n}".as_slice(),
            "event_field_declaration",
            20..45,
        ),
    ] {
        let parser = AnyParser::parse(LANG::Csharp, source.to_vec(), None, None).unwrap();
        let field = node_at(&parser, kind, range);
        assert!(field.has_error());
        let both = indexes(&parser, DocumentationOptions::default());
        for index in &both {
            assert_eq!(index.documents().len(), 1);
            assert_eq!(index.associations().len(), 0);
            assert_eq!(index.assessments().len(), 3);
            assert!(index.node_projection(field).unwrap().pending());
            assert!(index.node_associations(field).unwrap().projection_pending());
        }
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_broken_field_does_not_erase_the_next_documented_field() {
    let source = b"class C {\n///a\nint x = ;\n///b\nint y;\n}";
    let parser = AnyParser::parse(LANG::Csharp, source.to_vec(), None, None).unwrap();
    let both = indexes(&parser, DocumentationOptions::default());
    for index in &both {
        assert_eq!(index.documents().len(), 2);
        assert_eq!(index.associations().len(), 1);
        assert_eq!(index.assessments().len(), 3);
        assert!(
            index
                .node_projection(node_at(&parser, "field_declaration", 15..24))
                .unwrap()
                .pending()
        );
        let good = node_at(&parser, "field_declaration", 30..36);
        let projection = index.node_projection(good).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 1);
        let association = index.associations().next().unwrap();
        let second = index.documents().nth(1).unwrap();
        assert_eq!(
            index.facts().associated_document(association).unwrap(),
            second
        );
        assert_eq!(index.complete_text(second).unwrap().unwrap().text(), "b");
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_field_inside_conditional_keeps_conditional_uncertainty() {
    let source = b"class C {\n#if X\n///branch\nint a;\n#endif\n}";
    let mut expected = field_document(
        16..25,
        "branch",
        19..25,
        "field_declaration",
        26..32,
        &[30..31],
        Resolution::Unresolved(UnresolvedReason::Conditional),
    );
    expected.assessments = 1; // The preproc_if itself is still pending.
    assert_eq!(view(source), expected);
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_document_before_a_conditional_wrapper_stays_pending_while_field_has_slots() {
    let source = b"class C {\n///wrapper\n#if X\nint a;\n#endif\n}";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    for index in &both {
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 0);
        assert_eq!(index.assessments().len(), 3);
        let field = node_at(&parser, "field_declaration", 27..33);
        let projection = index.node_projection(field).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 1);
        assert!(!index.node_associations(field).unwrap().projection_pending());
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_skipped_nested_class_field_keeps_the_existing_conditional_association() {
    // Characterization, not conditional-compilation policy: a compiler skips this class.
    let source =
        b"#if false\nclass C {\n///LIT\nint Y;\n}\n#endif\nclass D {\n///REAL\nint Z;\n}\n";
    let parser = clean(source);
    let first = field_document(
        20..26,
        "LIT",
        23..26,
        "field_declaration",
        27..33,
        &[31..32],
        Resolution::Unresolved(UnresolvedReason::Conditional),
    );
    let second = field_document(
        53..60,
        "REAL",
        56..60,
        "field_declaration",
        61..67,
        &[65..66],
        Resolution::Resolved,
    );
    assert_eq!(
        agreed(&indexes(&parser, DocumentationOptions::default())),
        View {
            documents: [first.documents[0].clone(), second.documents[0].clone()].into(),
            owners: [first.owners[0].clone(), second.owners[0].clone()].into(),
            slots: [first.slots[0].clone(), second.slots[0].clone()].into(),
            assessments: 1,
            ..View::default()
        }
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_verbatim_backslash_does_not_swallow_the_following_field() {
    // Characterization: C# verbatim strings do not escape a quote with backslash.
    let source = concat!(
        "class C {\n    string s = @\"a\\\";\n",
        "    /// <summary>LIT</summary>\n    public int Y;\n",
        "    string t = @\"\";\n    /// <summary>REAL</summary>\n",
        "    public int Z;\n}\n"
    )
    .as_bytes();
    let parser = clean(source);
    let first = field_document(
        36..62,
        "<summary>LIT</summary>",
        40..62,
        "field_declaration",
        67..80,
        &[78..79],
        Resolution::Resolved,
    );
    let second = field_document(
        105..132,
        "<summary>REAL</summary>",
        109..132,
        "field_declaration",
        137..150,
        &[148..149],
        Resolution::Resolved,
    );
    assert_eq!(
        agreed(&indexes(&parser, DocumentationOptions::default())),
        View {
            documents: [first.documents[0].clone(), second.documents[0].clone()].into(),
            owners: [first.owners[0].clone(), second.owners[0].clone()].into(),
            slots: [first.slots[0].clone(), second.slots[0].clone()].into(),
            ..View::default()
        }
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_field_slot_and_declaration_queries_keep_distinct_owner_identities() {
    let source = b"class C {\n///both\nint a, b;\n}";
    let parser = clean(source);
    let declaration = node_at(&parser, "field_declaration", 18..27);
    for index in indexes(&parser, DocumentationOptions::default()) {
        let association = index.associations().next().unwrap();
        let names: Vec<_> = (0..2)
            .map(|at| index.facts().associated_owner(association, at).unwrap())
            .collect();
        assert_ne!(names[0], names[1]);
        let projection = index.node_projection(declaration).unwrap();
        assert!(!projection.pending());
        assert!(projection.owners().eq(names.iter().copied()));
        assert!(
            index
                .node_owners(declaration)
                .unwrap()
                .eq(names.iter().copied())
        );
        assert!(matches!(
            index.node_associations(declaration),
            Err(OwnerError::MultipleOwners)
        ));
        for slot in names {
            assert!(
                index
                    .owner_associations(slot)
                    .unwrap()
                    .associations()
                    .eq([association])
            );
            let enclosing = index.facts().slot_enclosing(slot).unwrap();
            assert!(matches!(enclosing.get(), Owner::Node(_)));
            assert_eq!(
                index
                    .owner_associations(enclosing)
                    .unwrap()
                    .associations()
                    .len(),
                0
            );
        }
        for (kind, range) in [
            ("variable_declarator", 22..23),
            ("identifier", 22..23),
            ("variable_declarator", 25..26),
        ] {
            let node = node_at(&parser, kind, range);
            assert!(matches!(
                index.node_projection(node),
                Err(OwnerError::UnsupportedOwner)
            ));
        }
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_undocumented_field_still_has_slots_without_a_document() {
    let source = b"class C { int a, b; }";
    let parser = clean(source);
    let field = node_at(&parser, "field_declaration", 10..19);
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(agreed(&both), View::default());
    for index in &both {
        let projection = index.node_projection(field).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 2);
        assert_eq!(index.node_owners(field).unwrap().count(), 2);
        assert_eq!(index.facts().owners().len(), 5);
        assert!(matches!(
            index.node_associations(field),
            Err(OwnerError::MultipleOwners)
        ));
        for owner in projection.owners() {
            let query = index.owner_associations(owner).unwrap();
            assert_eq!(query.associations().len(), 0);
            assert_eq!(query.assessments().count(), 0);
        }
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_slot_projection_requires_the_whole_declaration_included() {
    let source = b"class C {\n///both\nint a, b;\n}";
    let included = [0..24, 25..source.len()]; // Space between comma and b is excluded.
    let ranges: Vec<_> = included
        .iter()
        .map(|range| {
            let point = |at: usize| {
                let before = &source[..at];
                tree_sitter::Point::new(
                    before.iter().filter(|byte| **byte == b'\n').count(),
                    before
                        .iter()
                        .rev()
                        .take_while(|byte| **byte != b'\n')
                        .count(),
                )
            };
            tree_sitter::Range {
                start_byte: range.start,
                end_byte: range.end,
                start_point: point(range.start),
                end_point: point(range.end),
            }
        })
        .collect();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Csharp.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let index = collect(
        LANG::Csharp,
        &tree,
        source,
        &DocumentationContext::default(),
        &options,
    )
    .unwrap();
    assert_eq!(index.documents().len(), 0);
    assert_eq!(index.associations().len(), 0);
    assert_eq!(index.evidence().len(), 1);
    let field = tree
        .root_node()
        .named_descendant_for_byte_range(18, 27)
        .unwrap();
    assert_eq!(field.kind(), "field_declaration");
    assert!(index.node_projection(field).unwrap().pending());
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_error_beside_an_intact_field_does_not_erase_its_slot() {
    let source = b"class C {\n///d\nint a;\n%\n}";
    let parser = AnyParser::parse(LANG::Csharp, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().has_error());
    let field = node_at(&parser, "field_declaration", 15..21);
    assert!(!field.has_error());
    let mut expected = field_document(
        10..14,
        "d",
        13..14,
        "field_declaration",
        15..21,
        &[19..20],
        Resolution::Resolved,
    );
    expected.assessments = 1; // Only the containing class remains pending.
    assert_eq!(
        agreed(&indexes(&parser, DocumentationOptions::default())),
        expected
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_tuple_pattern_field_declarator_stays_pending_without_slots() {
    let source = b"class C {\n/// doc\nint (a, b) = (1, 2);\n}\n";
    let parser = clean(source);
    let declaration = node_at(&parser, "field_declaration", 18..38);
    let variables = node_at(&parser, "variable_declarator", 22..37);
    assert_eq!(variables.child(0).unwrap().kind(), "tuple_pattern");
    let both = indexes(&parser, DocumentationOptions::default());
    assert_eq!(
        agreed(&both),
        View {
            documents: vec![comment(10..17, "doc", [(0..3, Copied(14..17))])],
            assessments: 2,
            ..View::default()
        }
    );
    for index in &both {
        assert!(index.node_projection(declaration).unwrap().pending());
        assert!(
            index
                .node_associations(declaration)
                .unwrap()
                .projection_pending()
        );
        assert!(
            !index
                .facts()
                .owners()
                .any(|owner| matches!(owner.get(), Owner::Slot(_)))
        );
    }
}

#[cfg(feature = "csharp")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "csharp")]
impl WorkBudget for Counting {
    type Error = SourceError;

    #[cfg(feature = "csharp")]
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

    #[cfg(feature = "csharp")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        Ok(())
    }
}

#[cfg(feature = "csharp")]
fn wide_fields(width: usize) -> String {
    let names: Vec<_> = (0..width).map(|at| format!("v{at}")).collect();
    format!("class C {{\n///audit\nint {};\n}}", names.join(", "))
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_declarator_scan_charges_every_structural_child() {
    for (source, declaration, steps, names) in [
        (
            b"class C {\n///both\nint a, b;\n}".as_slice(),
            18..27,
            5,
            vec![22..23, 25..26],
        ),
        (
            b"class C {\n///both\nint /* c */ a, /* c */ b;\n}".as_slice(),
            18..43,
            7,
            vec![30..31, 41..42],
        ),
        (
            b"class C {\n///both\n[System.Obsolete] private static readonly int a = 1, b;\n}"
                .as_slice(),
            18..73,
            9,
            vec![64..65, 71..72],
        ),
    ] {
        let parser = clean(source);
        let index = indexes(&parser, DocumentationOptions::default());
        let id = index[0]
            .canonical
            .lookup(node_at(&parser, "field_declaration", declaration))
            .unwrap();
        let count = Counting::default();
        let found = super::projections::csharp_declarator_names(&index[0].canonical, id, &count)
            .unwrap()
            .unwrap();
        assert_eq!(count.steps.get(), steps);
        assert_eq!(count.bytes.get(), 0);
        assert_eq!(
            found
                .iter()
                .map(|name| index[0].canonical.nodes[name.0]
                    .evidence
                    .node()
                    .byte_range())
                .collect::<Vec<_>>(),
            names,
        );
        for fail_at in 1..=steps {
            let budget = Counting {
                fail_at: Some(fail_at),
                ..Counting::default()
            };
            assert!(matches!(
                super::projections::csharp_declarator_names(&index[0].canonical, id, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), fail_at);
        }
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_field_width_has_linear_owner_and_name_work() {
    for width in [8, 64, 256] {
        let source = wide_fields(width);
        let parser = clean(source.as_bytes());
        let before = observations();
        let indexes = indexes(&parser, DocumentationOptions::default());
        let after = observations();
        let index = &indexes[0];
        let declaration = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "field_declaration")
            .unwrap();
        let id = index
            .canonical
            .lookup(declaration.as_tree_sitter())
            .unwrap();
        let budget = Counting::default();
        let found = super::projections::csharp_declarator_names(&index.canonical, id, &budget)
            .unwrap()
            .unwrap();
        // expected: the first variable_declaration child, then its type, width
        // declarators and width - 1 commas: 1 + (2 * width).
        assert_eq!(budget.steps.get(), 2 * width + 1);
        assert_eq!(found.len(), width);
        // expected: file, class, declaration, and one field slot per name.
        assert_eq!(index.facts().owners().len(), width + 3);
        assert_eq!(
            index.associations().next().unwrap().get().owner_count(),
            width
        );
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        assert_eq!(after.renders - before.renders, 2);
        assert_eq!(
            after.visits - before.visits,
            2 * parser.root_node().preorder().count()
        );
    }
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_wide_fields_cancel_in_both_the_name_scan_and_slot_loop() {
    let width = 2048;
    let source = wide_fields(width);
    let parser = clean(source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let before = observations();
    parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let mut slots = 0;
    let mut scan = 0;
    for checkpoint in 1..=polls {
        let before = observations();
        let result = crate::documentation::budget::cancel_after(checkpoint, || {
            parser.documentation(&context, &options)
        });
        let after = observations();
        assert!(matches!(result, Err(CollectionError::Cancelled)));
        let owners = after.owner_projections - before.owner_projections;
        let searches = after.intersection_searches - before.intersection_searches;
        if owners == 3 && searches < width + 1 {
            slots += 1;
        }
        if owners == 2 && after.visits - before.visits == parser.root_node().preorder().count() - 1
        {
            scan += 1;
        }
    }
    // expected: each of 2048 slots charges three steps, yielding 5 or 6 polls.
    assert!((5..=6).contains(&slots), "slots={slots}");
    // expected: 4097 scanned children charge at least 4 and at most 5 polls.
    assert!((4..=5).contains(&scan), "scan={scan}");
    assert_eq!(
        crate::documentation::budget::cancel_after(polls + 1, || {
            parser.documentation(&context, &options)
        })
        .unwrap()
        .associations()
        .next()
        .unwrap()
        .get()
        .owner_count(),
        width,
    );
}
