//! Lua LDoc source-view tests compare documents, associations, retained candidates and
//! pending assessments through both entry points. The grammar test collects nothing and
//! pins the parse-tree shapes on which the source-view tests rely. Node queries are not
//! part of the compared view.

#[cfg(feature = "lua")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "lua")]
use crate::documentation::*;
#[cfg(feature = "lua")]
use crate::{AnyParser, LANG};
#[cfg(feature = "lua")]
use std::cell::Cell;
#[cfg(feature = "lua")]
use std::ops::Range;

#[cfg(feature = "lua")]
use Run::{Copied, Decoded, Synthetic};

#[cfg(feature = "lua")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "lua")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "lua")]
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    Node(&'static str, Range<usize>),
    File(Range<usize>),
    Slot(Range<usize>, Range<usize>),
}

#[cfg(feature = "lua")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<(&'static str, Range<usize>)>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and pending assessments published by
/// one collection. Assessments have no defined order. Node queries are not part of this
/// view; values shared by every case are asserted by `published`.
#[cfg(feature = "lua")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<(Resolution, Holder)>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "lua")]
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

#[cfg(feature = "lua")]
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

#[cfg(feature = "lua")]
fn file(document: Doc) -> Doc {
    Doc {
        role: DocumentRole::FilePackage,
        ..document
    }
}

#[cfg(feature = "lua")]
fn owned(document: Doc, holder: Holder) -> View {
    View {
        documents: vec![document],
        associations: vec![(Resolution::Resolved, holder)],
        ..View::default()
    }
}

#[cfg(feature = "lua")]
fn function(document: Doc, range: Range<usize>) -> View {
    owned(document, Holder::Node("function_declaration", range))
}

#[cfg(feature = "lua")]
fn unordered(assessments: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = assessments.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "lua")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "lua")]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "lua")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "lua")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("comment", range.clone()))
    );
    range
}

#[cfg(feature = "lua")]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the four published lists and the rules they share.
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
            assert_eq!(attachment.convention.name(), "LDoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            assert_eq!(ordered.get(at), Some(&document));
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owner = facts.associated_owner(association, 0).unwrap();
            let holder = match owner.get() {
                Owner::Node(declared) => {
                    assert_eq!(
                        attachment
                            .binding
                            .map(NodeEvidence::node)
                            .map(|node| node.byte_range()),
                        Some(declared.node().byte_range())
                    );
                    let (kind, range) = site(declared.node());
                    Holder::Node(kind, range)
                }
                Owner::File(file) => {
                    assert_eq!(file.node(), root);
                    assert!(attachment.binding.is_none());
                    Holder::File(file.node().byte_range())
                }
                Owner::Slot(slot) => {
                    assert_eq!(slot.role, SlotRole::Field);
                    assert_eq!(slot.ordinal, 0);
                    let enclosing = index.facts().slot_enclosing(owner).unwrap();
                    let Owner::Node(parent) = enclosing.get() else {
                        panic!("field slot must be enclosed by its field")
                    };
                    assert_eq!(parent.node().kind(), "field");
                    assert_eq!(
                        attachment.binding.map(NodeEvidence::node),
                        Some(parent.node())
                    );
                    Holder::Slot(span(&slot.source), parent.node().byte_range())
                }
                other => panic!("a node, file or field slot, found {other:?}"),
            };
            (attachment.resolution, holder)
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
    // The Lua LDoc adapter is partial for every source here.
    assert_eq!(index.capability(), Capability::Partial);
    // A qualification record belongs to a documentation command that names its declaration
    // (the Doxygen named-reference path), which LDoc does not have here.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "lua")]
fn clean(source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(LANG::Lua, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        source.escape_ascii()
    );
    parser
}

#[cfg(feature = "lua")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Lua,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "lua")]
fn agreed(indexes: &[DocumentationIndex<'_>; 2], root: tree_sitter::Node<'_>) -> View {
    let [typed, raw] = indexes.each_ref().map(|index| published(index, root));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "lua")]
fn view(source: &[u8]) -> View {
    let parser = clean(source);
    agreed(
        &indexes(&parser, DocumentationOptions::default()),
        parser.ts_tree().root_node(),
    )
}

#[cfg(feature = "lua")]
fn retained(source: &[u8]) -> View {
    let parser = clean(source);
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    agreed(&indexes(&parser, options), parser.ts_tree().root_node())
}

#[cfg(feature = "lua")]
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

#[cfg(feature = "lua")]
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

#[cfg(feature = "lua")]
#[test]
fn lua_marked_block_above_a_function_documents_the_function() {
    let sources: [(&[u8], Range<usize>); 4] = [
        (b"module(\"m\")\n---audit\nfunction f() end\n", 21..37),
        (b"module(\"m\")\n---audit\nlocal function f() end\n", 21..43),
        (b"module(\"m\")\n---audit\nfunction M.f() end\n", 21..39),
        (b"module(\"m\")\n---audit\nfunction M:m() end\n", 21..39),
    ];
    for (source, owner) in sources {
        assert_eq!(
            view(source),
            function(comment(12..20, "audit", [(0..5, Copied(15..20))]), owner),
            "{}",
            source.escape_ascii()
        );
    }
}

/// Each one-name assignment has its own node owner.
#[cfg(feature = "lua")]
#[test]
fn lua_marked_block_above_an_assignment_has_its_owner() {
    let sources: [(&[u8], Range<usize>); 4] = [
        (b"module(\"m\")\n---audit\nf = function() end\n", 21..39),
        (b"module(\"m\")\n---audit\nM.f = function() end\n", 21..41),
        (b"module(\"m\")\n---audit\nt = {}\n", 21..27),
        (b"module(\"m\")\n---audit\nM.t = {}\n", 21..29),
    ];
    for (source, owner) in sources {
        assert_eq!(
            view(source),
            owned(
                comment(12..20, "audit", [(0..5, Copied(15..20))]),
                Holder::Node("assignment_statement", owner),
            ),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_long_bracket_block_is_one_fragment_and_ends_its_run() {
    let sources: [(&[u8], Doc, Range<usize>); 3] = [
        (
            b"module(\"m\")\n--[[-- audit ]]\nfunction f() end\n",
            comment(12..27, " audit ", [(0..7, Copied(18..25))]),
            28..44,
        ),
        (
            b"module(\"m\")\n--[==[-- audit ]==]\nfunction f() end\n",
            comment(12..31, " audit ", [(0..7, Copied(20..27))]),
            32..48,
        ),
        (
            b"module(\"m\")\n--[[--\naudit\nsecond\n]]\nfunction f() end\n",
            comment(
                12..34,
                "audit\nsecond",
                [
                    (0..5, Copied(19..24)),
                    (5..6, Copied(24..25)),
                    (6..12, Copied(25..31)),
                ],
            ),
            35..51,
        ),
    ];
    for (source, document, owner) in sources {
        assert_eq!(
            view(source),
            function(document, owner),
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"--[[-- audit ]]\n-- note\nfunction f() end\n"),
        owned(
            file(comment(0..15, " audit ", [(0..7, Copied(6..13))])),
            Holder::File(0..41)
        )
    );
    let source = b"module(\"m\")\n--[[-- audit ]]\n-- note\nfunction f() end\n";
    let mut expected = View::default();
    assert_eq!(view(source), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 12..27),
        (CandidateDisposition::Unattached, 28..35),
    ]);
    assert_eq!(retained(source), expected);
}

#[cfg(feature = "lua")]
#[test]
fn lua_plain_comments_give_no_document() {
    #[allow(clippy::single_range_in_vec_init)]
    let sources: [(&[u8], Vec<Range<usize>>); 5] = [
        (b"module(\"m\")\n-- audit\nfunction f() end\n", vec![12..20]),
        (
            b"module(\"m\")\n-- a\n-- b\nfunction f() end\n",
            vec![12..16, 17..21],
        ),
        (
            b"module(\"m\")\n--[[ audit ]]\nfunction f() end\n",
            vec![12..25],
        ),
        (b"-- audit\nfunction f() end\n", vec![0..8]),
        (
            b"module(\"m\")\n-- plain\n---audit\nfunction f() end\n",
            vec![12..20, 21..29],
        ),
    ];
    for (source, candidates) in sources {
        let mut expected = View::default();
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        expected.evidence.extend(
            candidates
                .into_iter()
                .map(|range| (CandidateDisposition::Unattached, range)),
        );
        assert_eq!(retained(source), expected, "{}", source.escape_ascii());
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_first_block_of_a_file_documents_the_file() {
    #[allow(clippy::type_complexity)]
    let sources: [(&[u8], Range<usize>, Range<usize>, Range<usize>); 7] = [
        (b"---audit\nfunction f() end\n", 0..8, 3..8, 0..26),
        (b"---audit\n\nfunction f() end\n", 0..8, 3..8, 0..27),
        (b"---audit\n", 0..8, 3..8, 0..9),
        (
            b"#!/usr/bin/env lua\n---audit\nfunction f() end\n",
            19..27,
            22..27,
            0..45,
        ),
        (
            b"-- plain\n\n---audit\nfunction f() end\n",
            10..18,
            13..18,
            0..36,
        ),
        (
            b"--[[ plain ]]\n---audit\nfunction f() end\n",
            14..22,
            17..22,
            0..40,
        ),
        (
            b"---audit\nmodule(\"m\")\nfunction f() end\n",
            0..8,
            3..8,
            0..38,
        ),
    ];
    for (source, fragment, text, root) in sources {
        assert_eq!(
            view(source),
            owned(
                file(comment(fragment, "audit", [(0..5, Copied(text))])),
                Holder::File(root)
            ),
            "{}",
            source.escape_ascii()
        );
    }
    assert_eq!(
        view(b"---audit\nfunction f() end\n---second\nfunction g() end\n"),
        View {
            documents: vec![
                file(comment(0..8, "audit", [(0..5, Copied(3..8))])),
                comment(26..35, "second", [(0..6, Copied(29..35))])
            ],
            associations: vec![
                (Resolution::Resolved, Holder::File(0..53)),
                (
                    Resolution::Resolved,
                    Holder::Node("function_declaration", 36..52)
                )
            ],
            ..View::default()
        }
    );
    assert_eq!(
        view(b"--- Module.\n-- @module m\nfunction f() end\n"),
        owned(
            file(run(
                [0..11, 12..24],
                "Module.\n@module m",
                [
                    (0..7, Copied(4..11)),
                    (7..8, Synthetic),
                    (8..17, Copied(15..24))
                ]
            )),
            Holder::File(0..42)
        )
    );
    let source = b"-- plain\n---audit\nfunction f() end\n";
    let mut expected = View::default();
    assert_eq!(view(source), expected);
    expected.evidence.extend([
        (CandidateDisposition::Unattached, 0..8),
        (CandidateDisposition::Unattached, 9..17),
    ]);
    assert_eq!(retained(source), expected);
}

#[cfg(feature = "lua")]
#[test]
fn lua_blank_row_separates_two_blocks() {
    let source = b"module(\"m\")\n---first\n\n---audit\nfunction f() end\n";
    let mut expected = function(comment(22..30, "audit", [(0..5, Copied(25..30))]), 31..47);
    assert_eq!(view(source), expected);
    expected
        .evidence
        .push((CandidateDisposition::Unattached, 12..20));
    assert_eq!(retained(source), expected);
}

#[cfg(feature = "lua")]
#[test]
fn lua_rows_of_a_run_compose_one_document() {
    let sources: [(&[u8], Doc, Range<usize>); 4] = [
        (
            b"module(\"m\")\n--- first\n-- second\nfunction f() end\n",
            run(
                [12..21, 22..31],
                "first\nsecond",
                [
                    (0..5, Copied(16..21)),
                    (5..6, Synthetic),
                    (6..12, Copied(25..31)),
                ],
            ),
            32..48,
        ),
        (
            b"module(\"m\")\n--- a\n--\n-- b\nfunction f() end\n",
            run(
                [12..17, 18..20, 21..25],
                "a\n\nb",
                [
                    (0..1, Copied(16..17)),
                    (1..2, Synthetic),
                    (2..3, Synthetic),
                    (3..4, Copied(24..25)),
                ],
            ),
            26..42,
        ),
        (
            b"module(\"m\")\n---\n-- audit\nfunction f() end\n",
            run([12..15, 16..24], "audit", [(0..5, Copied(19..24))]),
            25..41,
        ),
        (
            b"module(\"m\")\n---a\n---b\nfunction f() end\n",
            run(
                [12..16, 17..21],
                "a\nb",
                [
                    (0..1, Copied(15..16)),
                    (1..2, Synthetic),
                    (2..3, Copied(20..21)),
                ],
            ),
            22..38,
        ),
    ];
    for (source, document, owner) in sources {
        assert_eq!(
            view(source),
            function(document, owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_text_cleanup_removes_the_marker_and_at_most_one_space() {
    let sources: [(&[u8], Doc, Range<usize>); 7] = [
        (
            b"module(\"m\")\n--- audit\nfunction f() end\n",
            comment(12..21, "audit", [(0..5, Copied(16..21))]),
            22..38,
        ),
        (
            b"module(\"m\")\n---  audit\nfunction f() end\n",
            comment(12..22, " audit", [(0..6, Copied(16..22))]),
            23..39,
        ),
        (
            b"module(\"m\")\n--- a  \nfunction f() end\n",
            comment(12..19, "a  ", [(0..3, Copied(16..19))]),
            20..36,
        ),
        (
            b"module(\"m\")\n--- a\n--b\n--  c\nfunction f() end\n",
            run(
                [12..17, 18..21, 22..27],
                "a\nb\n c",
                [
                    (0..1, Copied(16..17)),
                    (1..2, Synthetic),
                    (2..3, Copied(20..21)),
                    (3..4, Synthetic),
                    (4..6, Copied(25..27)),
                ],
            ),
            28..44,
        ),
        (
            b"module(\"m\")\n--[[-audit]]\nfunction f() end\n",
            comment(12..24, "audit", [(0..5, Copied(17..22))]),
            25..41,
        ),
        (
            b"module(\"m\")\n--[=[-audit]=]\nfunction f() end\n",
            comment(12..26, "audit", [(0..5, Copied(18..23))]),
            27..43,
        ),
        (
            b"module(\"m\")\n--[==[-audit]==]\nfunction f() end\n",
            comment(12..28, "audit", [(0..5, Copied(19..24))]),
            29..45,
        ),
    ];
    for (source, document, owner) in sources {
        assert_eq!(
            view(source),
            function(document, owner),
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_crlf_rows_keep_owners_fragments_and_text() {
    assert_eq!(
        view(b"module(\"m\")\r\n---audit\r\nfunction f() end\r\n"),
        function(comment(13..21, "audit", [(0..5, Copied(16..21))]), 23..39)
    );
    assert_eq!(
        view(b"module(\"m\")\r\n--- a\r\n-- b\r\nfunction f() end\r\n"),
        function(
            run(
                [13..18, 20..24],
                "a\nb",
                [
                    (0..1, Copied(17..18)),
                    (1..2, Synthetic),
                    (2..3, Copied(23..24))
                ]
            ),
            26..42
        )
    );
    assert_eq!(
        view(b"module(\"m\")\r\n--[[--\r\naudit\r\nsecond\r\n]]\r\nfunction f() end\r\n"),
        function(
            comment(
                13..38,
                "audit\nsecond",
                [
                    (0..5, Copied(21..26)),
                    (5..6, Decoded(26..28)),
                    (6..12, Copied(28..34))
                ]
            ),
            40..56
        )
    );
    assert_eq!(
        view(b"---audit\r\nfunction f() end\r\n"),
        owned(
            file(comment(0..8, "audit", [(0..5, Copied(3..8))])),
            Holder::File(0..28)
        )
    );
}

#[cfg(feature = "lua")]
#[test]
#[allow(clippy::too_many_lines)]
fn lua_grammar_shapes_the_unit_relies_on() {
    let lf = b"module(\"m\")\n---audit\nfunction f() end\n";
    let parser = clean(lf);
    let root = parser.ts_tree().root_node();
    let line = node_at(&parser, "comment", 12..20);
    let function = line.next_sibling().unwrap();
    assert_eq!(site(root), ("chunk", 0..38));
    assert_eq!(line.parent(), Some(root));
    assert!(line.is_extra());
    assert_eq!(site(function), ("function_declaration", 21..37));
    assert_eq!(
        parts(line),
        [
            (Some("start"), "--", 12..14),
            (Some("content"), "comment_content", 14..20)
        ]
    );
    assert!(!line.child(0).unwrap().is_named());
    assert_eq!(&lf[line.end_byte()..function.start_byte()], b"\n");
    assert_eq!(&lf[function.end_byte()..root.end_byte()], b"\n");

    let parser = clean(
        b"function f() end\nlocal function g() end\nfunction M.f() end\nfunction M:m() end\n",
    );
    let root = parser.ts_tree().root_node();
    assert_eq!(
        parts(root),
        [
            (None, "function_declaration", 0..16),
            (Some("local_declaration"), "function_declaration", 17..39),
            (None, "function_declaration", 40..58),
            (None, "function_declaration", 59..77),
        ]
    );
    for (range, kind, name) in [
        (0..16, "identifier", 9..10),
        (17..39, "identifier", 32..33),
        (40..58, "dot_index_expression", 49..52),
        (59..77, "method_index_expression", 68..71),
    ] {
        let function = node_at(&parser, "function_declaration", range);
        assert_eq!(
            function.child_by_field_name("name").map(site),
            Some((kind, name))
        );
        assert!(parts(function).iter().all(|(_, kind, _)| *kind != "block"));
    }
    let local = node_at(&parser, "function_declaration", 17..39)
        .child(0)
        .unwrap();
    assert_eq!(site(local), ("local", 17..22));
    assert!(!local.is_named());

    let parser = clean(b"f = function() end\nM.f = function() end\nt = {}\nM.t = {}\n");
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "assignment_statement", 0..18),
            (None, "assignment_statement", 19..39),
            (None, "assignment_statement", 40..46),
            (None, "assignment_statement", 47..55),
        ]
    );
    for (range, variable, expression, value_kind, value, target) in [
        (0..18, 0..1, 4..18, "function_definition", 4..18, None),
        (
            19..39,
            19..22,
            25..39,
            "function_definition",
            25..39,
            Some(19..22),
        ),
        (40..46, 40..41, 44..46, "table_constructor", 44..46, None),
        (
            47..55,
            47..50,
            53..55,
            "table_constructor",
            53..55,
            Some(47..50),
        ),
    ] {
        let assignment = node_at(&parser, "assignment_statement", range);
        assert_eq!(
            parts(assignment)
                .iter()
                .filter(|(_, kind, _)| *kind != "=")
                .cloned()
                .collect::<Vec<_>>(),
            [
                (None, "variable_list", variable),
                (None, "expression_list", expression),
            ]
        );
        let expression = assignment.named_child(1).unwrap();
        assert_eq!(
            expression.named_child(0).map(site),
            Some((value_kind, value))
        );
        if let Some(target) = target {
            assert_eq!(
                assignment.named_child(0).unwrap().named_child(0).map(site),
                Some(("dot_index_expression", target))
            );
        }
    }

    let parser =
        clean(b"--[[-- audit ]]\n--[==[-audit]==]\n--[[--\naudit\nsecond\n]]\nfunction f() end\n");
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, "comment", 0..15),
            (None, "comment", 16..32),
            (None, "comment", 33..55),
            (None, "function_declaration", 56..72),
        ]
    );
    for (comment, opener, content, closer) in [
        (0..15, 0..4, 4..13, 13..15),
        (16..32, 16..22, 22..28, 28..32),
        (33..55, 33..37, 37..53, 53..55),
    ] {
        assert_eq!(
            parts(node_at(&parser, "comment", comment)),
            [
                (Some("start"), "[[", opener),
                (Some("content"), "comment_content", content),
                (Some("end"), "]]", closer),
            ]
        );
    }
    let rows = node_at(&parser, "comment", 33..55);
    assert_eq!(rows.end_position().row, rows.start_position().row + 3);

    let parser = clean(b"#!/usr/bin/env lua\n---audit\nfunction f() end\n");
    assert_eq!(
        parts(parser.ts_tree().root_node())[0..2],
        [(None, "hash_bang_line", 0..18), (None, "comment", 19..27)]
    );
    assert!(node_at(&parser, "hash_bang_line", 0..18).is_named());

    let crlf = b"module(\"m\")\r\n---audit\r\nfunction f() end\r\n";
    let parser = clean(crlf);
    let root = parser.ts_tree().root_node();
    let comment = node_at(&parser, "comment", 13..21);
    let function = node_at(&parser, "function_declaration", 23..39);
    assert_eq!(site(root), ("chunk", 0..41));
    assert_eq!(&crlf[comment.end_byte()..function.start_byte()], b"\r\n");
    assert_eq!(&crlf[function.end_byte()..root.end_byte()], b"\r\n");

    let crlf = b"module(\"m\")\r\n--[[--\r\naudit\r\nsecond\r\n]]\r\nfunction f() end\r\n";
    let parser = clean(crlf);
    let comment = node_at(&parser, "comment", 13..38);
    assert_eq!(
        comment.child_by_field_name("content").map(site),
        Some(("comment_content", 17..36))
    );
    assert!(crlf[comment.byte_range()].contains(&b'\r'));
    let function = node_at(&parser, "function_declaration", 40..56);
    assert_eq!(&crlf[comment.end_byte()..function.start_byte()], b"\r\n");

    let source = b"---audit\n\nfunction f() end\n";
    let parser = clean(source);
    let comment = node_at(&parser, "comment", 0..8);
    let function = comment.next_sibling().unwrap();
    assert_eq!(site(function), ("function_declaration", 10..26));
    assert_eq!(comment.parent(), function.parent());
    assert_eq!(
        function.start_position().row,
        comment.end_position().row + 2
    );
    assert_eq!(&source[comment.end_byte()..function.start_byte()], b"\n\n");

    let parser = clean(b"---audit\n");
    assert_eq!(site(parser.ts_tree().root_node()), ("chunk", 0..9));
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [(None, "comment", 0..8)]
    );

    let parser = clean(b"module(\"m\")\n--- a\n--\n-- b\nfunction f() end\n");
    let root = parser.ts_tree().root_node();
    assert_eq!(
        parts(root)[1..4],
        [
            (None, "comment", 12..17),
            (None, "comment", 18..20),
            (None, "comment", 21..25)
        ]
    );
    let empty = node_at(&parser, "comment_content", 20..20);
    assert!(!empty.is_missing());
    assert_eq!(empty.parent().map(site), Some(("comment", 18..20)));

    let parser = clean(b"local t = {}\nreturn {}\n");
    let local = node_at(&parser, "variable_declaration", 0..12);
    assert_eq!(
        local.named_child(0).map(site),
        Some(("assignment_statement", 6..12))
    );
    let returning = node_at(&parser, "return_statement", 13..22);
    let expressions = node_at(&parser, "expression_list", 20..22);
    assert_eq!(expressions.parent(), Some(returning));
    assert_eq!(
        expressions.named_child(0).map(site),
        Some(("table_constructor", 20..22))
    );

    let parser = clean(b"module(\"m\")\n---audit\nfunction f(\nx -- input\n) end\n");
    let function = node_at(&parser, "function_declaration", 21..49);
    let parameters = node_at(&parser, "parameters", 31..45);
    assert_eq!(parameters.parent(), Some(function));
    let comment = node_at(&parser, "comment", 35..43);
    assert_eq!(comment.parent(), Some(parameters));
    assert_eq!(
        comment.prev_sibling().map(site),
        Some(("identifier", 33..34))
    );

    let parser = clean(
        b"module(\"m\")\n--- settings\n-- @table options\noptions = {\n\
          alpha = 1, -- correction\n}\n",
    );
    let table = node_at(&parser, "table_constructor", 53..81);
    let comment = node_at(&parser, "comment", 66..79);
    assert_eq!(comment.parent(), Some(table));
    let comma = comment.prev_sibling().unwrap();
    assert_eq!(site(comma), (",", 64..65));
    assert!(!comma.is_named());
    assert_eq!(comma.prev_sibling().map(site), Some(("field", 55..64)));
}

#[cfg(feature = "lua")]
#[test]
#[allow(clippy::too_many_lines)]
fn lua_unit2_tagged_run_keeps_following_item() {
    for (source, body, comment, owner) in [
        (
            b"module(\"m\")\n--- audit\n-- @param x thing\nfunction f(x) end\n".as_slice(),
            "@param x thing",
            22..39,
            40..57,
        ),
        (
            b"module(\"m\")\n--- audit\n-- @return y thing\nfunction g() end\n",
            "@return y thing",
            22..40,
            41..57,
        ),
    ] {
        let end = 25 + body.len();
        assert_eq!(
            view(source),
            function(
                run(
                    [12..21, comment],
                    &format!("audit\n{body}"),
                    [
                        (0..5, Copied(16..21)),
                        (5..6, Synthetic),
                        (6..6 + body.len(), Copied(25..end)),
                    ]
                ),
                owner
            )
        );
    }
    for (source, marker) in [
        (
            b"module(\"m\")\n--- audit\n-- @function ghost\nfunction f() end\n".as_slice(),
            "@function ghost",
        ),
        (
            b"module(\"m\")\n--- audit\n-- @field ghost\nfunction f() end\n",
            "@field ghost",
        ),
    ] {
        let found = view(source);
        assert_eq!(found.associations, []);
        assert_eq!(found.documents.len(), 1);
        assert!(found.documents[0].text.contains(marker));
        assert_eq!(found.assessments.len(), 1);
    }
    for tag in [
        "lfunction",
        "factory",
        "annotation",
        "example",
        "topic",
        "file",
        "name",
        "table",
        "type",
        "section",
        "module",
        "script",
        "classmod",
        "submodule",
    ] {
        let source = format!("module(\"m\")\n--- audit\n-- @{tag} ghost\nfunction f() end\n");
        let found = view(source.as_bytes());
        assert_eq!(found.documents.len(), 1);
        assert_eq!(found.associations, []);
    }
    let plain = b"module(\"m\")\n--- audit\nfunction f() end\n";
    assert_eq!(
        view(plain).associations,
        [(
            Resolution::Resolved,
            Holder::Node("function_declaration", 22..38)
        )]
    );
    let first_file = b"--- audit\n-- @function ghost\nfunction f() end\n";
    assert_eq!(
        view(first_file).associations,
        [(Resolution::Resolved, Holder::File(0..46))]
    );
    let changed = b"module(\"m\")\n--- audit\n-- @param x thing\nfunction f(x) end\n--- second\n-- @return y thing\nfunction g() end\n";
    let found = view(changed);
    assert_eq!(found.documents.len(), 2);
    assert_eq!(
        found.associations,
        [
            (
                Resolution::Resolved,
                Holder::Node("function_declaration", 40..57)
            ),
            (
                Resolution::Resolved,
                Holder::Node("function_declaration", 88..104)
            ),
        ]
    );
    let other = b"module(\"m\")\n--- audit\n-- @param x ghost\nx = 1\n";
    assert_eq!(
        view(other).associations,
        [(
            Resolution::Resolved,
            Holder::Node("assignment_statement", 40..45)
        )]
    );
    let long_virtual =
        b"module(\"m\")\n--[[-- @function ghost\n@param x thing\n]]\nfunction f(x) end\n";
    let found = view(long_virtual);
    assert_eq!(found.documents.len(), 1);
    assert_eq!(found.associations, []);
    assert_eq!(found.assessments.len(), 1);
    let long_annotated = b"module(\"m\")\n--[[-- audit\n@param x thing\n]]\nfunction f(x) end\n";
    let found = view(long_annotated);
    assert_eq!(found.documents.len(), 1);
    assert_eq!(
        found.associations,
        [(
            Resolution::Resolved,
            Holder::Node("function_declaration", 43..60)
        )]
    );
    let virtual_only = b"module(\"m\")\n---audit\n-- @function ghost\n";
    assert_eq!(view(virtual_only).associations, []);
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_field_has_one_named_slot_and_other_forms_remain_pending() {
    let source = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = 1,\n}\n";
    let actual = view(source);
    assert_eq!(
        actual.associations,
        [(Resolution::Resolved, Holder::Slot(35..40, 35..44))]
    );
    assert_eq!(
        actual.documents,
        [comment(24..34, "detail", [(0..6, Copied(28..34))])]
    );
    assert_eq!(actual.assessments.len(), 1);
    let inside = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = -- inline\n1,\n}\n";
    assert_eq!(
        view(inside).associations,
        [(Resolution::Resolved, Holder::Slot(35..40, 35..54))]
    );
    let parser = clean(source);
    let index = &indexes(&parser, DocumentationOptions::default())[0];
    let field = node_at(&parser, "field", 35..44);
    let projection = index.node_projection(field).unwrap();
    assert!(!projection.pending());
    assert_eq!(projection.owners().len(), 1);
    let query = index.node_associations(field).unwrap();
    assert!(!query.projection_pending());
    assert_eq!(query.associations().len(), 1);
    let trailing = b"module(\"m\")\nlocal t = {\nalpha = 1, --- detail\n}\n";
    assert_eq!(view(trailing).documents, []);
    assert_eq!(
        retained(trailing),
        View {
            evidence: vec![(CandidateDisposition::Unattached, 35..45)],
            assessments: unordered([Pending {
                axis: AssessmentAxis::Selection,
                reason: "Native LDoc parameter/table comment awaits adapter context",
                node: Some(("comment", 35..45)),
                document: None,
            }]),
            ..View::default()
        }
    );
    let computed = b"module(\"m\")\nlocal t = {\n--- detail\n[k] = 1,\n}\n";
    assert_eq!(view(computed).documents, []);
    assert_eq!(view(computed).associations, []);
    assert_eq!(
        retained(computed),
        View {
            evidence: vec![(CandidateDisposition::Unattached, 24..34)],
            assessments: unordered([Pending {
                axis: AssessmentAxis::Selection,
                reason: "Native LDoc parameter/table comment awaits adapter context",
                node: Some(("comment", 24..34)),
                document: None,
            }]),
            ..View::default()
        }
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_local_and_return_owners_and_unrecognized_controls() {
    for (source, owner) in [
        (
            b"module(\"m\")\n--- audit\nlocal a = 1\n".as_slice(),
            Holder::Node("variable_declaration", 22..33),
        ),
        (
            b"module(\"m\")\n--- audit\nreturn {}\n",
            Holder::Node("return_statement", 22..31),
        ),
        (
            b"module(\"m\")\n--- audit\nM.x = 1\n",
            Holder::Node("assignment_statement", 22..29),
        ),
        (
            b"module(\"m\")\n--- audit\nlocal -- note\na = 1\n",
            Holder::Node("variable_declaration", 22..41),
        ),
        (
            b"module(\"m\")\n--- audit\na -- note\n= 1\n",
            Holder::Node("assignment_statement", 22..35),
        ),
    ] {
        assert_eq!(view(source).associations, [(Resolution::Resolved, owner)]);
    }
    for source in [
        b"module(\"m\")\n--- audit\nlocal a\n".as_slice(),
        b"module(\"m\")\n--- audit\nreturn M\n",
        b"module(\"m\")\n--- audit\nlocal a, b = 1, 2\n",
        b"module(\"m\")\n--- audit\na, b = 1, 2\n",
        b"module(\"m\")\n--- audit\nlocal x <const> = 1\n",
        b"module(\"m\")\n--- audit\nM[1] = 1\n",
    ] {
        assert_eq!(view(source), View::default());
        assert_eq!(
            retained(source),
            View {
                evidence: vec![(CandidateDisposition::Unattached, 12..21)],
                ..View::default()
            }
        );
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_two_documents_keep_distinct_field_slots() {
    let source = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = 1,\n--- beta\nbeta = 2,\n}\n";
    let found = view(source);
    assert_eq!(
        found.documents,
        [
            comment(24..34, "detail", [(0..6, Copied(28..34))]),
            comment(46..54, "beta", [(0..4, Copied(50..54))]),
        ]
    );
    assert_eq!(
        found.associations,
        [
            (Resolution::Resolved, Holder::Slot(35..40, 35..44)),
            (Resolution::Resolved, Holder::Slot(55..59, 55..63)),
        ]
    );
    assert_eq!(found.assessments.len(), 2);
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    let first = indexes[0].associations().next().unwrap();
    let second = indexes[0].associations().nth(1).unwrap();
    assert_ne!(
        indexes[0].facts().associated_owner(first, 0).unwrap(),
        indexes[0].facts().associated_owner(second, 0).unwrap()
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_nonroot_wrapper_binds_the_block_and_owns_the_first_statement() {
    let source = b"module(\"m\")\ndo\n--- audit\nx = 1\nend\n";
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    let found = agreed(&indexes, parser.ts_tree().root_node());
    assert_eq!(
        found.documents,
        [comment(15..24, "audit", [(0..5, Copied(19..24))])]
    );
    assert_eq!(
        found.associations,
        [(
            Resolution::Resolved,
            Holder::Node("assignment_statement", 25..30)
        ),]
    );
    let association = indexes[0].associations().next().unwrap();
    assert_eq!(
        association.get().binding.map(NodeEvidence::node).map(site),
        Some(("block", 25..30))
    );
    assert_eq!(
        site(association.get().lexical_scope.node()),
        ("do_statement", 12..34)
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_recovery_keeps_broken_forms_pending_and_clean_neighbor_resolved() {
    let broken = [
        (
            b"module(\"m\")\n--- audit\nx =\n".as_slice(),
            "assignment_statement",
        ),
        (
            b"module(\"m\")\n--- audit\nlocal a = {k = }\n",
            "variable_declaration",
        ),
        (
            b"module(\"m\")\n--- audit\nreturn {k = }\n",
            "return_statement",
        ),
    ];
    for (source, kind) in broken {
        let parser = AnyParser::parse(LANG::Lua, source.to_vec(), None, None).unwrap();
        assert!(parser.ts_tree().root_node().has_error());
        let both = indexes(&parser, DocumentationOptions::default());
        let node = parser.ts_tree().root_node().named_child(2).unwrap();
        assert_eq!(node.kind(), kind);
        assert!(node.has_error());
        for index in &both {
            assert!(index.node_projection(node).unwrap().pending());
            assert_eq!(index.documents().len(), 1);
            let association = index.associations().next().unwrap();
            assert_eq!(
                association.get().resolution,
                Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
            );
            assert_eq!(association.get().owner_count(), 0);
        }
    }
    let broken_field = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = (1\n}\n";
    let parser = AnyParser::parse(LANG::Lua, broken_field.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let both = indexes(&parser, DocumentationOptions::default());
    let field_node = parser
        .root_node()
        .preorder()
        .find(|node| node.kind() == "field")
        .unwrap();
    assert!(field_node.as_tree_sitter().has_error());
    for index in &both {
        assert!(
            index
                .node_projection(field_node.as_tree_sitter())
                .unwrap()
                .pending()
        );
        assert_eq!(index.documents().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().resolution,
            Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
        );
        assert_eq!(association.get().owner_count(), 0);
    }
    let field = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = 1,\n%%\n}\n";
    let parser = AnyParser::parse(LANG::Lua, field.to_vec(), None, None).unwrap();
    assert!(parser.ts_tree().root_node().has_error());
    let both = indexes(&parser, DocumentationOptions::default());
    let clean_field = node_at(&parser, "field", 35..44);
    assert!(!clean_field.has_error());
    for index in &both {
        let projection = index.node_projection(clean_field).unwrap();
        assert!(!projection.pending());
        assert_eq!(projection.owners().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().owner_count(), 1);
        assert!(
            matches!(index.facts().associated_owner(association, 0).unwrap().get(),
            Owner::Slot(slot) if slot.role == SlotRole::Field && span(&slot.source) == (35..40))
        );
    }
}

#[cfg(feature = "lua")]
#[derive(Default)]
struct LuaCounting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    fail_at: Option<usize>,
    fail_bytes_at: Option<usize>,
}

#[cfg(feature = "lua")]
impl WorkBudget for LuaCounting {
    type Error = SourceError;
    #[cfg(feature = "lua")]
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
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        if self
            .fail_bytes_at
            .is_some_and(|limit| self.bytes.get() >= limit)
        {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_name_scan_charges_and_can_cancel_at_each_child() {
    let source = b"module(\"m\")\n--- audit\nlocal a = 1\n";
    let parser = clean(source);
    let index = &indexes(&parser, DocumentationOptions::default())[0];
    let decl = node_at(&parser, "variable_declaration", 22..33);
    let id = index.canonical.lookup(decl).unwrap();
    let budget = LuaCounting::default();
    let list = super::projections::lua_declared_list(&index.canonical, id, &budget)
        .unwrap()
        .unwrap();
    assert_eq!(
        site(index.canonical.nodes[list.0].evidence.node()),
        ("variable_list", 28..29)
    );
    // expected: `local`, the assignment and its `variable_list` child.
    assert_eq!(budget.steps.get(), 3);
    let name = super::projections::lua_list_single_name(&index.canonical, list, &budget)
        .unwrap()
        .unwrap();
    assert_eq!(
        site(index.canonical.nodes[name.0].evidence.node()),
        ("identifier", 28..29)
    );
    // expected: the list's identifier adds one read to the preceding three.
    assert_eq!(budget.steps.get(), 4);
    for at in 1..=4 {
        let budget = LuaCounting {
            fail_at: Some(at),
            ..LuaCounting::default()
        };
        assert!(matches!(
            super::projections::lua_single_name(&index.canonical, id, &budget),
            Err(SourceError::Cancelled)
        ));
        assert_eq!(budget.steps.get(), at);
    }
    let budget = LuaCounting {
        fail_at: Some(5),
        ..LuaCounting::default()
    };
    assert!(
        super::projections::lua_single_name(&index.canonical, id, &budget)
            .unwrap()
            .is_some()
    );
    assert_eq!(budget.steps.get(), 4);
    let source = b"module(\"m\")\n--- audit\nlocal -- note\na = 1\n";
    let parser = clean(source);
    let index = &indexes(&parser, DocumentationOptions::default())[0];
    let decl = node_at(&parser, "variable_declaration", 22..41);
    let id = index.canonical.lookup(decl).unwrap();
    let budget = LuaCounting::default();
    let name = super::projections::lua_single_name(&index.canonical, id, &budget)
        .unwrap()
        .unwrap();
    assert_eq!(
        site(index.canonical.nodes[name.0].evidence.node()),
        ("identifier", 36..37)
    );
    // expected: local, comment, assignment, variable list and name are five children read.
    assert_eq!(budget.steps.get(), 5);
}

#[cfg(feature = "lua")]
#[test]
#[allow(clippy::naive_bytecount, clippy::single_range_in_vec_init)]
fn lua_unit2_excluded_name_and_comment_gap_do_not_attach() {
    let source = b"module(\"m\")\n--- audit\nx = 1\n";
    let restricted = |parts: &[Range<usize>]| {
        let point = |byte: usize| {
            let prefix = &source[..byte];
            tree_sitter::Point::new(
                prefix.iter().filter(|&&b| b == b'\n').count(),
                prefix.iter().rev().take_while(|&&b| b != b'\n').count(),
            )
        };
        let ranges = parts
            .iter()
            .map(|range| tree_sitter::Range {
                start_byte: range.start,
                end_byte: range.end,
                start_point: point(range.start),
                end_point: point(range.end),
            })
            .collect::<Vec<_>>();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANG::Lua.tree_sitter_language().unwrap())
            .unwrap();
        parser.set_included_ranges(&ranges).unwrap();
        parser.parse(source, None).unwrap()
    };
    let context = DocumentationContext::default();
    let options = DocumentationOptions {
        retain_unbound: true,
        ..DocumentationOptions::default()
    };
    let excluded_name = restricted(&[0..23, 24..source.len()]);
    assert!(!excluded_name.root_node().has_error());
    let index = collect(LANG::Lua, &excluded_name, source, &context, &options).unwrap();
    let assignment = excluded_name.root_node().named_child(2).unwrap();
    assert_eq!(site(assignment), ("assignment_statement", 22..27));
    assert!(index.node_projection(assignment).unwrap().pending());
    assert_eq!(index.associations().len(), 0);
    let gap = restricted(&[0..21, 22..source.len()]);
    let index = collect(LANG::Lua, &gap, source, &context, &options).unwrap();
    assert_eq!(index.associations().len(), 0);
    let control = restricted(&[0..source.len()]);
    let index = collect(LANG::Lua, &control, source, &context, &options).unwrap();
    assert_eq!(index.associations().len(), 1);
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_two_unbound_candidates_need_explicit_retention() {
    let source = b"module(\"m\")\n-- plain\nx = 1\n\n-- other\ny = 2\n--- audit\nz = 3\n";
    assert_eq!(view(source).evidence, []);
    let kept = retained(source);
    assert_eq!(
        kept.evidence,
        [
            (CandidateDisposition::Unattached, 12..20),
            (CandidateDisposition::Unattached, 28..36),
        ]
    );
    assert_eq!(
        kept.associations,
        [(
            Resolution::Resolved,
            Holder::Node("assignment_statement", 53..58)
        ),]
    );
}

// test-lang-gates: hand-written(lua) — this source-only classifier belongs to the Lua adapter.
#[cfg(feature = "lua")]
#[test]
fn lua_unit2_tag_classifier_charges_lines_comparisons_and_cancels() {
    let untagged = b"--- audit\n-- @function ghost";
    let budget = LuaCounting::default();
    assert!(crate::checker::lua::standalone_tag_with_budget(untagged, &budget).unwrap());
    // expected: one loop step per of the two physical lines.
    assert_eq!(budget.steps.get(), 2);
    let charged = budget.bytes.get();
    // expected: line separators, body trimming, tag scan and all fixed-name comparisons.
    assert!(charged > untagged.len());
    assert!(charged < untagged.len() + 16 * 16);
    // expected: 28 scanner bytes, 13 body bytes, 9 tag-name bytes and
    // 65 candidate-comparison bytes give exactly 115 charged bytes.
    assert_eq!(charged, 28 + 13 + 9 + 65);
    // expected: the fixed tag-name comparisons account for some charged bytes.
    assert!(charged - untagged.len() >= b"function".len());
    for at in 1..=2 {
        let b = LuaCounting {
            fail_at: Some(at),
            ..LuaCounting::default()
        };
        assert!(matches!(
            crate::checker::lua::standalone_tag_with_budget(untagged, &b),
            Err(SourceError::Cancelled)
        ));
        assert_eq!(b.steps.get(), at);
    }
    let without_comparisons = charged - b"function".len();
    assert!(without_comparisons < charged);
    let b = LuaCounting {
        fail_bytes_at: Some(charged),
        ..LuaCounting::default()
    };
    assert!(matches!(
        crate::checker::lua::standalone_tag_with_budget(untagged, &b),
        Err(SourceError::Cancelled)
    ));
    assert_eq!(b.bytes.get(), charged);
    let b = LuaCounting {
        fail_bytes_at: Some(charged + 1),
        ..LuaCounting::default()
    };
    assert!(crate::checker::lua::standalone_tag_with_budget(untagged, &b).unwrap());
    assert_eq!(b.bytes.get(), charged);
    let control = LuaCounting::default();
    assert!(!crate::checker::lua::standalone_tag_with_budget(b"---audits", &control).unwrap());
    assert_eq!(control.steps.get(), 1);
    assert_eq!(control.bytes.get(), 15);
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_field_slot_loop_observes_collection_budget_cancellation() {
    use std::fmt::Write;
    let options = DocumentationOptions::default();
    let context = DocumentationContext::default();
    let mut fields = String::from("module(\"m\")\nlocal t = {\n");
    for at in 0..1_024 {
        write!(fields, "--- doc {at}\nkey{at} = 1,\n").unwrap();
    }
    fields.push_str("}\n");
    let locals = format!("module(\"m\")\n{}", "--- doc\nlocal a = 1\n".repeat(260));
    let mut tagged = String::from("module(\"m\")\n--[[-- audit\n");
    for _ in 0..1_024 {
        tagged.push_str("-- @param x\n");
    }
    tagged.push_str("]]\nfunction f() end\n");
    for (source, documents, associations, lower, upper) in [
        (fields.as_bytes(), 1_024, 1_024, 4_303, 4_303),
        (locals.as_bytes(), 260, 260, 1_113, 1_113),
        (tagged.as_bytes(), 1, 1, 4_118, 4_118),
    ] {
        let parser = clean(source);
        let start = observations().polls;
        let index = parser.documentation(&context, &options).unwrap();
        let checks = observations().polls - start;
        assert_eq!(index.documents().len(), documents);
        assert_eq!(index.associations().len(), associations);
        // expected: the three source shapes have the measured total checkpoints
        // recorded here; both sides fail if a handoff drops a checkpoint.
        assert!((lower..=upper).contains(&checks), "polls={checks}");
        for checkpoint in [1, checks] {
            let result = crate::documentation::budget::cancel_after(checkpoint, || {
                parser.documentation(&context, &options)
            });
            assert!(matches!(result, Err(CollectionError::Cancelled)));
        }
        let final_index = crate::documentation::budget::cancel_after(checks + 1, || {
            parser.documentation(&context, &options)
        })
        .unwrap();
        assert_eq!(final_index.associations().len(), associations);
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_unit2_field_name_scan_charges_and_skips_comment() {
    let source = b"module(\"m\")\nlocal t = {\n--- detail\nalpha = -- inline\n1,\n}\n";
    let parser = clean(source);
    let indexes = indexes(&parser, DocumentationOptions::default());
    let field = node_at(&parser, "field", 35..54);
    let id = indexes[0].canonical.lookup(field).unwrap();
    let budget = LuaCounting::default();
    let name = super::projections::lua_field_name(&indexes[0].canonical, id, &budget)
        .unwrap()
        .unwrap();
    assert_eq!(
        site(indexes[0].canonical.nodes[name.0].evidence.node()),
        ("identifier", 35..40)
    );
    // expected: the first field child is the name, so no comment after it is read.
    assert_eq!(budget.steps.get(), 1);
    let cancelled = LuaCounting {
        fail_at: Some(1),
        ..LuaCounting::default()
    };
    assert!(matches!(
        super::projections::lua_field_name(&indexes[0].canonical, id, &cancelled),
        Err(SourceError::Cancelled)
    ));
    assert_eq!(cancelled.steps.get(), 1);
}
