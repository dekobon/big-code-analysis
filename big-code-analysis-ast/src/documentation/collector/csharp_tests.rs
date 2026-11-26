//! C# XML documentation in the source view: which declaration owns a comment, the text
//! of the document and where each part of that text comes from. Every source is collected
//! through both entry points, except in the last test, which pins the parse-tree shapes
//! the other tests rely on.

#[cfg(feature = "csharp")]
use crate::documentation::*;
#[cfg(feature = "csharp")]
use crate::test_support::for_each_node_with_chain;
#[cfg(feature = "csharp")]
use crate::{Ancestors, AnyParser, Checker, CsharpCode, LANG};
#[cfg(feature = "csharp")]
use std::ops::Range;

#[cfg(feature = "csharp")]
use Run::{Copied, Decoded, Synthetic};

/// The origin of one run of a document's text, with the source byte range it reads.
#[cfg(feature = "csharp")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

/// One document: its role, its fragments as source byte ranges, its text, and each run
/// of the text as an output range with its origin.
#[cfg(feature = "csharp")]
#[derive(Debug, PartialEq, Eq)]
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
    let owners = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.owner_count(), 1);
            let owner = facts.associated_owner(association, 0).unwrap();
            let Owner::Node(declared) = owner.get() else {
                panic!("a node owner, found {:?}", owner.get())
            };
            let declaration = declared.node();
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
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    View {
        documents,
        owners,
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
    let source = b"class C {\n///first\nint P { get; set; }\n///second\nevent System.Action E { add {} remove {} }\n}";
    let parser = clean(source);
    let both = indexes(&parser, DocumentationOptions::default());
    let seen = agreed(&both);
    assert_eq!(seen.documents.len(), 2);
    assert_eq!(seen.documents[0].text, "first");
    assert_eq!(seen.documents[1].text, "second");
    assert_eq!(seen.owners[0].1, "property_declaration");
    assert_eq!(seen.owners[1].1, "event_declaration");
    assert_eq!(seen.documents[0].fragments, vec![10..18]);
    assert_eq!(seen.documents[1].fragments, vec![39..48]);
    assert_eq!(seen.assessments, 0);
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
