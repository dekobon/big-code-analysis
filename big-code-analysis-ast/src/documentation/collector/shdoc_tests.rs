//! Bash shdoc source-view tests. Each fixture is collected through both entry points
//! with every combination of the two retention options, and the published documents,
//! associations, retained candidates and pending assessments are compared with a view
//! written from the source bytes. The checker also asks the node queries of every
//! function definition. The cost tests call the shdoc rules and the scan with a
//! counting budget, and the scan table asks the scan without a tree. The first test
//! collects nothing and pins the parse shapes the rules rely on.
//!
//! Every item that parses Bash carries the gate of the Bash grammar. The four tests
//! that ask the scan without a tree, and the counting budget they share, carry none:
//! the scan is compiled in every build.

#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

use crate::documentation::budget::WorkBudget;
use crate::documentation::*;
#[cfg(feature = "bash")]
use crate::{AnyParser, LANG};
use std::cell::Cell;
use std::ops::Range;

#[cfg(feature = "bash")]
use CandidateDisposition::{Directive, Unattached};
#[cfg(feature = "bash")]
use Run::{Copied, Synthetic};

#[cfg(feature = "bash")]
const SELECTED: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "bash")]
const RECOGNIZED: &str = "Recognized declaration requires a semantic projection";
#[cfg(feature = "bash")]
const WITHHELD: &str = "Rows without a confirmed reading may be literal text";

#[cfg(feature = "bash")]
type Site = (&'static str, Range<usize>);
#[cfg(feature = "bash")]
type Kept = Vec<(CandidateDisposition, Range<usize>)>;

#[cfg(feature = "bash")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Synthetic,
}

#[cfg(feature = "bash")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Doc {
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "bash")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edge {
    document: usize,
    resolution: Resolution,
    owners: Vec<Site>,
    binding: Option<Site>,
    scope: Site,
}

#[cfg(feature = "bash")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<Site>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and pending assessments published by
/// one collection. Assessments have no defined order. The role of a document, the form
/// and node of a fragment, the convention, the publication, the capability and the
/// qualification records are the same for every case and are asserted by `published`.
#[cfg(feature = "bash")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Edge>,
    evidence: Kept,
    assessments: Vec<Pending>,
}

/// One fixture: its bytes, the view under default options, the candidates each
/// retention setting adds, and what the node queries answer for each function.
#[cfg(feature = "bash")]
#[derive(Default)]
struct Case {
    name: &'static str,
    source: &'static [u8],
    view: View,
    unbound: Kept,
    directives: Kept,
    both: Kept,
    owners: Vec<Range<usize>>,
    pending: Vec<Range<usize>>,
    strangers: Vec<Range<usize>>,
}

/// A document of one row: the text is the row without `#` and one space.
#[cfg(feature = "bash")]
fn row(fragment: Range<usize>, text: &str) -> Doc {
    assert_eq!(text.len() + 2, fragment.len());
    Doc {
        composition: Composition::Single,
        map: vec![(0..text.len(), Copied(fragment.start + 2..fragment.end))],
        fragments: vec![fragment],
        text: text.to_owned(),
    }
}

/// A document with its whole map: several rows, or one row that keeps a blank.
#[cfg(feature = "bash")]
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
        text: text.to_owned(),
        map: map.into(),
    }
}

#[cfg(feature = "bash")]
fn function(range: Range<usize>) -> Site {
    ("function_definition", range)
}

#[cfg(feature = "bash")]
fn top(range: Range<usize>) -> Site {
    ("program", range)
}

#[cfg(feature = "bash")]
fn body(range: Range<usize>) -> Site {
    ("compound_statement", range)
}

#[cfg(feature = "bash")]
fn owned(document: usize, owner: Range<usize>, scope: Site) -> Edge {
    Edge {
        document,
        resolution: Resolution::Resolved,
        owners: vec![function(owner.clone())],
        binding: Some(function(owner)),
        scope,
    }
}

/// An association that names its function as a candidate behind a grammar boundary.
#[cfg(feature = "bash")]
fn candidate(document: usize, owner: Range<usize>, scope: Site) -> Edge {
    Edge {
        resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
        ..owned(document, owner, scope)
    }
}

/// An association without an owner, behind a grammar boundary.
#[cfg(feature = "bash")]
fn ownerless(document: usize, binding: Option<Range<usize>>, scope: Site) -> Edge {
    Edge {
        document,
        resolution: Resolution::Unresolved(UnresolvedReason::GrammarBoundary),
        owners: Vec::new(),
        binding: binding.map(function),
        scope,
    }
}

#[cfg(feature = "bash")]
fn selected(node: Option<Site>, document: usize) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason: SELECTED,
        node,
        document: Some(document),
    }
}

#[cfg(feature = "bash")]
fn recognized(node: Site) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason: RECOGNIZED,
        node: Some(node),
        document: None,
    }
}

/// The assessment of a withheld run, on its first row.
#[cfg(feature = "bash")]
fn withheld(row: Range<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Selection,
        reason: WITHHELD,
        node: Some(("comment", row)),
        document: None,
    }
}

#[cfg(feature = "bash")]
fn unordered(assessments: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = assessments.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "bash")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "bash")]
fn site(node: tree_sitter::Node<'_>) -> Site {
    (node.kind(), node.byte_range())
}

/// The range of a fragment, for documents and retained candidates alike: a comment
/// form whose node is the comment that starts where the fragment starts. The node also
/// holds the CR of a CRLF line end, which the fragment leaves out.
#[cfg(feature = "bash")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    let node = fragment.node().unwrap().node();
    assert_eq!((node.kind(), node.start_byte()), ("comment", range.start));
    range
}

#[cfg(feature = "bash")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the four published lists and the rules they share.
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
    let documents = ordered
        .iter()
        .map(|&document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            Doc {
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| fragment_span(facts.document_fragment(document, at).unwrap().get()))
                    .collect(),
                text: complete.text().to_owned(),
                map: complete
                    .mappings()
                    .iter()
                    .map(|entry| match &entry.origin {
                        Origin::Copied(pieces) => (entry.output.range(), Copied(span(pieces))),
                        Origin::Synthetic => (entry.output.range(), Synthetic),
                        other => panic!("a copied or synthetic origin, found {other:?}"),
                    })
                    .collect(),
            }
        })
        .collect();
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.convention.name(), "shdoc");
            assert_eq!(attachment.publication, Publication::Unknown);
            let document = facts.associated_document(association).unwrap();
            // The role of a document without an association is left to the unit that
            // decides what a file document is.
            assert_eq!(document.get().role(), DocumentRole::Primary);
            let first = facts.document_fragment(document, 0).unwrap();
            let comment = first.get().node().unwrap().node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let owners = (0..attachment.owner_count())
                .map(
                    |at| match facts.associated_owner(association, at).unwrap().get() {
                        Owner::Node(declared) => site(declared.node()),
                        other => panic!("a node owner, found {other:?}"),
                    },
                )
                .collect();
            Edge {
                document: ordered.iter().position(|known| *known == document).unwrap(),
                resolution: attachment.resolution,
                owners,
                binding: attachment.binding.map(|node| site(node.node())),
                scope: site(attachment.lexical_scope.node()),
            }
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|kept| {
            let fragment = index.evidence_fragment(kept).unwrap();
            (kept.get().disposition, fragment_span(fragment.get()))
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
    // The shdoc adapter is partial for every source here.
    assert_eq!(index.capability(), Capability::Partial);
    // A qualification record belongs to a documentation command that names its
    // declaration (the Doxygen named-reference path), which shdoc does not have here.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "bash")]
fn options(retain_unbound: bool, retain_directives: bool) -> DocumentationOptions {
    DocumentationOptions {
        retain_unbound,
        retain_directives,
        ..DocumentationOptions::default()
    }
}

#[cfg(feature = "bash")]
fn indexes(parser: &AnyParser, options: DocumentationOptions) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Bash,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

/// What both entry points publish for `parser` under `options`.
#[cfg(feature = "bash")]
fn agreed(parser: &AnyParser, options: DocumentationOptions) -> View {
    let [typed, raw] = indexes(parser, options).map(|index| published(&index));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "bash")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Bash, source.to_vec(), None, None).unwrap()
}

#[cfg(feature = "bash")]
fn function_nodes(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            found.push(node);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    found.sort_by_key(tree_sitter::Node::start_byte);
    found
}

/// Checks one fixture under the four retention settings, and the node queries of every
/// function definition of its tree.
#[cfg(feature = "bash")]
fn check(case: &Case) {
    let name = case.name;
    let parser = parsed(case.source);
    let settings = [
        (false, false, &Kept::new()),
        (true, false, &case.unbound),
        (false, true, &case.directives),
        (true, true, &case.both),
    ];
    for (unbound, directives, kept) in settings {
        let expected = View {
            evidence: kept.clone(),
            ..case.view.clone()
        };
        assert_eq!(
            agreed(&parser, options(unbound, directives)),
            expected,
            "{name} retain_unbound={unbound} retain_directives={directives}"
        );
    }
    let [index, _] = indexes(&parser, DocumentationOptions::default());
    let mut listed = Vec::new();
    for range in &case.owners {
        let node = node_at(&parser, "function_definition", range.clone());
        let projection = index.node_projection(node).unwrap();
        assert!(!projection.pending(), "{name} {range:?}");
        let owners: Vec<_> = index.node_owners(node).unwrap().collect();
        let [owner] = owners[..] else {
            panic!("{name} {range:?}: one owner, found {owners:?}")
        };
        assert!(
            matches!(owner.get(), Owner::Node(declared) if declared.node() == node),
            "{name} {range:?}"
        );
        let query = index.node_associations(node).unwrap();
        assert!(!query.projection_pending(), "{name} {range:?}");
        let expected = case
            .view
            .associations
            .iter()
            .filter(|edge| edge.owners.contains(&function(range.clone())))
            .count();
        assert_eq!(query.associations().len(), expected, "{name} {range:?}");
        listed.push(range.clone());
    }
    for range in &case.pending {
        let node = node_at(&parser, "function_definition", range.clone());
        assert!(index.node_projection(node).unwrap().pending(), "{name}");
        assert!(matches!(
            index.node_owners(node).map(Iterator::count),
            Err(OwnerError::PendingOwner)
        ));
        assert!(index.node_associations(node).unwrap().projection_pending());
        listed.push(range.clone());
    }
    for range in &case.strangers {
        let node = node_at(&parser, "function_definition", range.clone());
        assert!(matches!(
            index
                .node_projection(node)
                .map(|projection| projection.pending()),
            Err(OwnerError::UnsupportedOwner)
        ));
        listed.push(range.clone());
    }
    listed.sort_by_key(|range| range.start);
    let present: Vec<_> = function_nodes(parser.ts_tree().root_node())
        .iter()
        .map(tree_sitter::Node::byte_range)
        .collect();
    assert_eq!(
        listed, present,
        "{name}: every function definition is listed"
    );
}

#[cfg(feature = "bash")]
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

/// Every node of the tree, one line each: indentation by depth, the field name, the
/// kind, the byte range, and what the grammar says of the node.
#[cfg(feature = "bash")]
fn tree_lines(root: tree_sitter::Node<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cursor = root.walk();
    let mut depth = 0;
    loop {
        let node = cursor.node();
        let field = cursor
            .field_name()
            .map(|name| format!("{name}: "))
            .unwrap_or_default();
        let mut line = format!(
            "{}{field}{} {}..{}",
            "  ".repeat(depth),
            node.kind(),
            node.start_byte(),
            node.end_byte()
        );
        for (set, flag) in [
            (!node.is_named(), " unnamed"),
            (node.is_extra(), " extra"),
            (node.has_error(), " has_error"),
            (node.is_error(), " is_error"),
            (node.is_missing(), " missing"),
        ] {
            if set {
                line += flag;
            }
        }
        lines.push(line);
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return lines;
            }
            depth -= 1;
        }
    }
}

/// The parse shapes the shdoc rules rely on, as the pinned grammar gives them. In
/// order: a comment takes a trailing backslash and ends at the line feed; with CRLF it
/// holds the CR; a backslash before a line end, and one before a blank, belong to no
/// node, also at the start of the source; a blank row has no node; a comment after
/// code is a sibling on its row; `;` is a sibling token; `function f` and a file
/// redirect are inside the function node; a here-document redirect wraps it; the four
/// children of a here-document redirect, with `<<` and with `<<-`; a row-ending pipe
/// puts the rows below into the redirect; a blank before the delimiter still ends the
/// body; a second operator is an error inside the first redirect, and its text is
/// parsed as code; `&` after the word leaves the operator in an error; a shift is an
/// operator of a `binary_expression`; a `for` body gives no function node; the body
/// of a here-document starts after a blank first row; a comment in the tail that ends
/// with a backslash makes the rows below it comment nodes of the redirect; a
/// substitution in the text holds comment and function nodes; a here-document inside
/// such a substitution is a redirect of its own; `;` after the word is an error
/// inside the redirect; `<>` in a subshell is an error in valid Bash; a comment in a
/// subscript, in an arithmetic expansion and in an arithmetic command is an extra
/// node of the expression. Then the readings of design review round 1: a body row
/// with a backslash at its end does not join the delimiter row; of two operators
/// across `|` the second text is read first; `$'\\''` and a back-quoted substitution
/// with a quote in it end one byte late; a subscript ends at the first `]`; a joined
/// row is no delimiter; `<<=` behind `let` is an error; a comment in back quotes
/// takes the closing back quote with it.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_parse_shapes_the_shdoc_rules_rely_on() {
    let shapes: [(&[u8], &[&str]); 36] = [
        (
            b"# @description a\\\n# b\nf() { :; }",
            &[
                "program 0..32",
                "  comment 0..17 extra",
                "  comment 18..21 extra",
                "  function_definition 22..32",
                "    name: word 22..23",
                "    ( 23..24 unnamed",
                "    ) 24..25 unnamed",
                "    body: compound_statement 26..32",
                "      { 26..27 unnamed",
                "      command 28..29",
                "        name: command_name 28..29",
                "          word 28..29",
                "      ; 29..30 unnamed",
                "      } 31..32 unnamed",
            ],
        ),
        (
            b"# @description a\r\nf() { :; }\r\n",
            &[
                "program 0..30",
                "  comment 0..17 extra",
                "  function_definition 18..28",
                "    name: word 18..19",
                "    ( 19..20 unnamed",
                "    ) 20..21 unnamed",
                "    body: compound_statement 22..28",
                "      { 22..23 unnamed",
                "      command 24..25",
                "        name: command_name 24..25",
                "          word 24..25",
                "      ; 25..26 unnamed",
                "      } 27..28 unnamed",
            ],
        ),
        (
            b"echo a\\\n# @description c\nf() { :; }\n",
            &[
                "program 0..36",
                "  command 0..6",
                "    name: command_name 0..4",
                "      word 0..4",
                "    argument: word 5..6",
                "  comment 8..24 extra",
                "  function_definition 25..35",
                "    name: word 25..26",
                "    ( 26..27 unnamed",
                "    ) 27..28 unnamed",
                "    body: compound_statement 29..35",
                "      { 29..30 unnamed",
                "      command 31..32",
                "        name: command_name 31..32",
                "          word 31..32",
                "      ; 32..33 unnamed",
                "      } 34..35 unnamed",
            ],
        ),
        (
            b"# @description a\n\\ # b\nf() { :; }\n",
            &[
                "program 0..34",
                "  comment 0..16 extra",
                "  comment 19..22 extra",
                "  function_definition 23..33",
                "    name: word 23..24",
                "    ( 24..25 unnamed",
                "    ) 25..26 unnamed",
                "    body: compound_statement 27..33",
                "      { 27..28 unnamed",
                "      command 29..30",
                "        name: command_name 29..30",
                "          word 29..30",
                "      ; 30..31 unnamed",
                "      } 32..33 unnamed",
            ],
        ),
        (
            b"\\\n# @description c\nf() { :; }\n",
            &[
                "program 2..30",
                "  comment 2..18 extra",
                "  function_definition 19..29",
                "    name: word 19..20",
                "    ( 20..21 unnamed",
                "    ) 21..22 unnamed",
                "    body: compound_statement 23..29",
                "      { 23..24 unnamed",
                "      command 25..26",
                "        name: command_name 25..26",
                "          word 25..26",
                "      ; 26..27 unnamed",
                "      } 28..29 unnamed",
            ],
        ),
        (
            b"# @description audit\n\nf() { :; }\n",
            &[
                "program 0..33",
                "  comment 0..20 extra",
                "  function_definition 22..32",
                "    name: word 22..23",
                "    ( 23..24 unnamed",
                "    ) 24..25 unnamed",
                "    body: compound_statement 26..32",
                "      { 26..27 unnamed",
                "      command 28..29",
                "        name: command_name 28..29",
                "          word 28..29",
                "      ; 29..30 unnamed",
                "      } 31..32 unnamed",
            ],
        ),
        (
            b"x=1 # @description t\n# plain\nf() { :; }\n",
            &[
                "program 0..40",
                "  variable_assignment 0..3",
                "    name: variable_name 0..1",
                "    = 1..2 unnamed",
                "    value: number 2..3",
                "  comment 4..20 extra",
                "  comment 21..28 extra",
                "  function_definition 29..39",
                "    name: word 29..30",
                "    ( 30..31 unnamed",
                "    ) 31..32 unnamed",
                "    body: compound_statement 33..39",
                "      { 33..34 unnamed",
                "      command 35..36",
                "        name: command_name 35..36",
                "          word 35..36",
                "      ; 36..37 unnamed",
                "      } 38..39 unnamed",
            ],
        ),
        (
            b"# @description a\nf() { :; }; g() { :; }\n",
            &[
                "program 0..40",
                "  comment 0..16 extra",
                "  function_definition 17..27",
                "    name: word 17..18",
                "    ( 18..19 unnamed",
                "    ) 19..20 unnamed",
                "    body: compound_statement 21..27",
                "      { 21..22 unnamed",
                "      command 23..24",
                "        name: command_name 23..24",
                "          word 23..24",
                "      ; 24..25 unnamed",
                "      } 26..27 unnamed",
                "  ; 27..28 unnamed",
                "  function_definition 29..39",
                "    name: word 29..30",
                "    ( 30..31 unnamed",
                "    ) 31..32 unnamed",
                "    body: compound_statement 33..39",
                "      { 33..34 unnamed",
                "      command 35..36",
                "        name: command_name 35..36",
                "          word 35..36",
                "      ; 36..37 unnamed",
                "      } 38..39 unnamed",
            ],
        ),
        (
            b"# @description audit\nfunction f { :; }",
            &[
                "program 0..38",
                "  comment 0..20 extra",
                "  function_definition 21..38",
                "    function 21..29 unnamed",
                "    name: word 30..31",
                "    body: compound_statement 32..38",
                "      { 32..33 unnamed",
                "      command 34..35",
                "        name: command_name 34..35",
                "          word 34..35",
                "      ; 35..36 unnamed",
                "      } 37..38 unnamed",
            ],
        ),
        (
            b"# @description audit\nf() { :; } >/dev/null\n",
            &[
                "program 0..43",
                "  comment 0..20 extra",
                "  function_definition 21..42",
                "    name: word 21..22",
                "    ( 22..23 unnamed",
                "    ) 23..24 unnamed",
                "    body: compound_statement 25..31",
                "      { 25..26 unnamed",
                "      command 27..28",
                "        name: command_name 27..28",
                "          word 27..28",
                "      ; 28..29 unnamed",
                "      } 30..31 unnamed",
                "    redirect: file_redirect 32..42",
                "      > 32..33 unnamed",
                "      destination: word 33..42",
            ],
        ),
        (
            b"# @description c\nh() { :; } <<EOF\nbody\nEOF\n",
            &[
                "program 0..43",
                "  comment 0..16 extra",
                "  redirected_statement 17..42",
                "    body: function_definition 17..27",
                "      name: word 17..18",
                "      ( 18..19 unnamed",
                "      ) 19..20 unnamed",
                "      body: compound_statement 21..27",
                "        { 21..22 unnamed",
                "        command 23..24",
                "          name: command_name 23..24",
                "            word 23..24",
                "        ; 24..25 unnamed",
                "        } 26..27 unnamed",
                "    redirect: heredoc_redirect 28..42",
                "      << 28..30 unnamed",
                "      heredoc_start 30..33",
                "      heredoc_body 34..39",
                "      heredoc_end 39..42",
            ],
        ),
        (
            b"cat <<EOF\nbody\nEOF\n# @description a\nf() { :; }\n",
            &[
                "program 0..47",
                "  redirected_statement 0..18",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..18",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..15",
                "      heredoc_end 15..18",
                "  comment 19..35 extra",
                "  function_definition 36..46",
                "    name: word 36..37",
                "    ( 37..38 unnamed",
                "    ) 38..39 unnamed",
                "    body: compound_statement 40..46",
                "      { 40..41 unnamed",
                "      command 42..43",
                "        name: command_name 42..43",
                "          word 42..43",
                "      ; 43..44 unnamed",
                "      } 45..46 unnamed",
            ],
        ),
        (
            b"cat <<-EOF\n\tbody\n\tEOF\n# @description a\nf() { :; }\n",
            &[
                "program 0..50",
                "  redirected_statement 0..21",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..21",
                "      <<- 4..7 unnamed",
                "      heredoc_start 7..10",
                "      heredoc_body 12..18",
                "      heredoc_end 18..21",
                "  comment 22..38 extra",
                "  function_definition 39..49",
                "    name: word 39..40",
                "    ( 40..41 unnamed",
                "    ) 41..42 unnamed",
                "    body: compound_statement 43..49",
                "      { 43..44 unnamed",
                "      command 45..46",
                "        name: command_name 45..46",
                "          word 45..46",
                "      ; 46..47 unnamed",
                "      } 48..49 unnamed",
            ],
        ),
        (
            b"cat <<EOT |\n# @description x\nf() { :; }\nEOT\ngrep x\n",
            &[
                "program 0..51",
                "  redirected_statement 0..43",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..43",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      pipeline 10..39",
                "        | 10..11 unnamed",
                "        comment 12..28 extra",
                "        function_definition 29..39",
                "          name: word 29..30",
                "          ( 30..31 unnamed",
                "          ) 31..32 unnamed",
                "          body: compound_statement 33..39",
                "            { 33..34 unnamed",
                "            command 35..36",
                "              name: command_name 35..36",
                "                word 35..36",
                "            ; 36..37 unnamed",
                "            } 38..39 unnamed",
                "      heredoc_body 40..40",
                "      heredoc_end 40..43",
                "  command 44..50",
                "    name: command_name 44..48",
                "      word 44..48",
                "    argument: word 49..50",
            ],
        ),
        (
            b"cat <<EOF\nEOF \n# @description a\nf() { :; }\nEOF\n",
            &[
                "program 0..47",
                "  redirected_statement 0..13",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..13",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..10",
                "      heredoc_end 10..13",
                "  comment 15..31 extra",
                "  function_definition 32..42",
                "    name: word 32..33",
                "    ( 33..34 unnamed",
                "    ) 34..35 unnamed",
                "    body: compound_statement 36..42",
                "      { 36..37 unnamed",
                "      command 38..39",
                "        name: command_name 38..39",
                "          word 38..39",
                "      ; 39..40 unnamed",
                "      } 41..42 unnamed",
                "  command 43..46",
                "    name: command_name 43..46",
                "      word 43..46",
            ],
        ),
        (
            b"cat <<A <<B\nx\nA\n# @description a\nf() { :; }\nB\n",
            &[
                "program 0..46 has_error",
                "  redirected_statement 0..15 has_error",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..15 has_error",
                "      << 4..6 unnamed",
                "      heredoc_start 6..7",
                "      ERROR 8..9 extra has_error is_error",
                "        < 8..9 unnamed",
                "      redirect: file_redirect 9..11",
                "        < 9..10 unnamed",
                "        destination: word 10..11",
                "      heredoc_body 12..14",
                "      heredoc_end 14..15",
                "  comment 16..32 extra",
                "  function_definition 33..43",
                "    name: word 33..34",
                "    ( 34..35 unnamed",
                "    ) 35..36 unnamed",
                "    body: compound_statement 37..43",
                "      { 37..38 unnamed",
                "      command 39..40",
                "        name: command_name 39..40",
                "          word 39..40",
                "      ; 40..41 unnamed",
                "      } 42..43 unnamed",
                "  command 44..45",
                "    name: command_name 44..45",
                "      word 44..45",
            ],
        ),
        (
            b"cat <<EOF &\n# @description a\nf() { :; }\nEOF\n",
            &[
                "program 0..44 has_error",
                "  command 0..3",
                "    name: command_name 0..3",
                "      word 0..3",
                "  ERROR 4..9 extra has_error is_error",
                "    << 4..6 unnamed",
                "    heredoc_start 6..9",
                "  & 10..11 unnamed",
                "  comment 12..28 extra",
                "  function_definition 29..39",
                "    name: word 29..30",
                "    ( 30..31 unnamed",
                "    ) 31..32 unnamed",
                "    body: compound_statement 33..39",
                "      { 33..34 unnamed",
                "      command 35..36",
                "        name: command_name 35..36",
                "          word 35..36",
                "      ; 36..37 unnamed",
                "      } 38..39 unnamed",
                "  command 40..43",
                "    name: command_name 40..43",
                "      word 40..43",
            ],
        ),
        (
            b"x=$(( 1 << 2 ))\n# @description a\nf() { :; }\n",
            &[
                "program 0..44",
                "  variable_assignment 0..15",
                "    name: variable_name 0..1",
                "    = 1..2 unnamed",
                "    value: arithmetic_expansion 2..15",
                "      $(( 2..5 unnamed",
                "      binary_expression 6..12",
                "        left: number 6..7",
                "        operator: << 8..10 unnamed",
                "        right: number 11..12",
                "      )) 13..15 unnamed",
                "  comment 16..32 extra",
                "  function_definition 33..43",
                "    name: word 33..34",
                "    ( 34..35 unnamed",
                "    ) 35..36 unnamed",
                "    body: compound_statement 37..43",
                "      { 37..38 unnamed",
                "      command 39..40",
                "        name: command_name 39..40",
                "          word 39..40",
                "      ; 40..41 unnamed",
                "      } 42..43 unnamed",
            ],
        ),
        (
            b"# @description a\ng1() for i in 1; do :; done\n",
            &[
                "program 0..45 has_error",
                "  comment 0..16 extra",
                "  command 17..32 has_error",
                "    name: command_name 17..19",
                "      word 17..19",
                "    ERROR 19..21 extra has_error is_error",
                "      ( 19..20 unnamed",
                "      ) 20..21 unnamed",
                "    argument: word 22..25",
                "    argument: word 26..27",
                "    argument: word 28..30",
                "    argument: number 31..32",
                "  ; 32..33 unnamed",
                "  command 34..38",
                "    name: command_name 34..36",
                "      word 34..36",
                "    argument: word 37..38",
                "  ; 38..39 unnamed",
                "  command 40..44",
                "    name: command_name 40..44",
                "      word 40..44",
            ],
        ),
        (
            b"cat <<EOF\n\nbody\nEOF\n# @description a\nf() { :; }\n",
            &[
                "program 0..48",
                "  redirected_statement 0..19",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..19",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 11..16",
                "      heredoc_end 16..19",
                "  comment 20..36 extra",
                "  function_definition 37..47",
                "    name: word 37..38",
                "    ( 38..39 unnamed",
                "    ) 39..40 unnamed",
                "    body: compound_statement 41..47",
                "      { 41..42 unnamed",
                "      command 43..44",
                "        name: command_name 43..44",
                "          word 43..44",
                "      ; 44..45 unnamed",
                "      } 46..47 unnamed",
            ],
        ),
        (
            b"cat <<EOF # c \\\n\\\n# @description x\nf() { :; }\nEOF\ng() { :; }\n",
            &[
                "program 0..61",
                "  redirected_statement 0..49",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..49",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      comment 10..15 extra",
                "      comment 18..34 extra",
                "      heredoc_body 35..46",
                "      heredoc_end 46..49",
                "  function_definition 50..60",
                "    name: word 50..51",
                "    ( 51..52 unnamed",
                "    ) 52..53 unnamed",
                "    body: compound_statement 54..60",
                "      { 54..55 unnamed",
                "      command 56..57",
                "        name: command_name 56..57",
                "          word 56..57",
                "      ; 57..58 unnamed",
                "      } 59..60 unnamed",
            ],
        ),
        (
            b"cat <<EOF\n$(\n# @description x\nf() { :; }\n)\nEOF\ng() { :; }\n",
            &[
                "program 0..58",
                "  redirected_statement 0..46",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..46",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..43",
                "        command_substitution 10..42",
                "          $( 10..12 unnamed",
                "          comment 13..29 extra",
                "          function_definition 30..40",
                "            name: word 30..31",
                "            ( 31..32 unnamed",
                "            ) 32..33 unnamed",
                "            body: compound_statement 34..40",
                "              { 34..35 unnamed",
                "              command 36..37",
                "                name: command_name 36..37",
                "                  word 36..37",
                "              ; 37..38 unnamed",
                "              } 39..40 unnamed",
                "          ) 41..42 unnamed",
                "        heredoc_content 43..43",
                "      heredoc_end 43..46",
                "  function_definition 47..57",
                "    name: word 47..48",
                "    ( 48..49 unnamed",
                "    ) 49..50 unnamed",
                "    body: compound_statement 51..57",
                "      { 51..52 unnamed",
                "      command 53..54",
                "        name: command_name 53..54",
                "          word 53..54",
                "      ; 54..55 unnamed",
                "      } 56..57 unnamed",
            ],
        ),
        (
            b"cat <<EOF\n$(cat <<X\n X\nX\n)\nEOF\n# @description a\nf() { :; }\n",
            &[
                "program 0..59",
                "  redirected_statement 0..30",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..30",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..27",
                "        command_substitution 10..26",
                "          $( 10..12 unnamed",
                "          redirected_statement 12..22",
                "            body: command 12..15",
                "              name: command_name 12..15",
                "                word 12..15",
                "            redirect: heredoc_redirect 16..22",
                "              << 16..18 unnamed",
                "              heredoc_start 18..19",
                "              heredoc_body 21..21",
                "              heredoc_end 21..22",
                "          command 23..24",
                "            name: command_name 23..24",
                "              word 23..24",
                "          ) 25..26 unnamed",
                "        heredoc_content 27..27",
                "      heredoc_end 27..30",
                "  comment 31..47 extra",
                "  function_definition 48..58",
                "    name: word 48..49",
                "    ( 49..50 unnamed",
                "    ) 50..51 unnamed",
                "    body: compound_statement 52..58",
                "      { 52..53 unnamed",
                "      command 54..55",
                "        name: command_name 54..55",
                "          word 54..55",
                "      ; 55..56 unnamed",
                "      } 57..58 unnamed",
            ],
        ),
        (
            b"cat <<EOF ;\nbody\nEOF\n# @description a\nf() { :; }\n",
            &[
                "program 0..49 has_error",
                "  redirected_statement 0..20 has_error",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..20 has_error",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      ERROR 10..11 extra has_error is_error",
                "        ; 10..11 unnamed",
                "      heredoc_body 12..17",
                "      heredoc_end 17..20",
                "  comment 21..37 extra",
                "  function_definition 38..48",
                "    name: word 38..39",
                "    ( 39..40 unnamed",
                "    ) 40..41 unnamed",
                "    body: compound_statement 42..48",
                "      { 42..43 unnamed",
                "      command 44..45",
                "        name: command_name 44..45",
                "          word 44..45",
                "      ; 45..46 unnamed",
                "      } 47..48 unnamed",
            ],
        ),
        (
            b"! ( : <>/dev/tty ) 2>/dev/null\n# @description a\nf() { :; }\n",
            &[
                "program 0..59 has_error",
                "  redirected_statement 0..30 has_error",
                "    body: negated_command 0..18 has_error",
                "      ! 0..1 unnamed",
                "      subshell 2..18 has_error",
                "        ( 2..3 unnamed",
                "        redirected_statement 4..16 has_error",
                "          body: command 4..5",
                "            name: command_name 4..5",
                "              word 4..5",
                "          ERROR 6..7 extra has_error is_error",
                "            < 6..7 unnamed",
                "          redirect: file_redirect 7..16",
                "            > 7..8 unnamed",
                "            destination: word 8..16",
                "        ) 17..18 unnamed",
                "    redirect: file_redirect 19..30",
                "      descriptor: file_descriptor 19..20",
                "      > 20..21 unnamed",
                "      destination: word 21..30",
                "  comment 31..47 extra",
                "  function_definition 48..58",
                "    name: word 48..49",
                "    ( 49..50 unnamed",
                "    ) 50..51 unnamed",
                "    body: compound_statement 52..58",
                "      { 52..53 unnamed",
                "      command 54..55",
                "        name: command_name 54..55",
                "          word 54..55",
                "      ; 55..56 unnamed",
                "      } 57..58 unnamed",
            ],
        ),
        (
            b"a[\n# note\n# @description LIT\n1]=x\n",
            &[
                "program 0..34",
                "  variable_assignment 0..33",
                "    name: subscript 0..31",
                "      name: variable_name 0..1",
                "      [ 1..2 unnamed",
                "      comment 3..9 extra",
                "      comment 10..28 extra",
                "      index: number 29..30",
                "      ] 30..31 unnamed",
                "    = 31..32 unnamed",
                "    value: word 32..33",
            ],
        ),
        (
            b"echo $(( 1 +\n# note\n# @description LIT\n2 ))\n",
            &[
                "program 0..44",
                "  command 0..43",
                "    name: command_name 0..4",
                "      word 0..4",
                "    argument: arithmetic_expansion 5..43",
                "      $(( 5..8 unnamed",
                "      binary_expression 9..40",
                "        left: number 9..10",
                "        operator: + 11..12 unnamed",
                "        comment 13..19 extra",
                "        comment 20..38 extra",
                "        right: number 39..40",
                "      )) 41..43 unnamed",
            ],
        ),
        (
            b"(( x = 1 +\n# note\n# @description LIT\n2 ))\n",
            &[
                "program 0..42",
                "  compound_statement 0..41",
                "    (( 0..2 unnamed",
                "    binary_expression 3..38",
                "      left: variable_name 3..4",
                "      operator: = 5..6 unnamed",
                "      right: binary_expression 7..38",
                "        left: number 7..8",
                "        operator: + 9..10 unnamed",
                "        comment 11..17 extra",
                "        comment 18..36 extra",
                "        right: number 37..38",
                "    )) 39..41 unnamed",
            ],
        ),
        (
            b"cat <<EOF\nhello\\\nEOF\n# @description LIT\nf() { :; }\nEOF\n# @description REAL\n\
                g() { :; }\n",
            &[
                "program 0..86",
                "  redirected_statement 0..20",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..20",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..17",
                "      heredoc_end 17..20",
                "  comment 21..39 extra",
                "  function_definition 40..50",
                "    name: word 40..41",
                "    ( 41..42 unnamed",
                "    ) 42..43 unnamed",
                "    body: compound_statement 44..50",
                "      { 44..45 unnamed",
                "      command 46..47",
                "        name: command_name 46..47",
                "          word 46..47",
                "      ; 47..48 unnamed",
                "      } 49..50 unnamed",
                "  command 51..54",
                "    name: command_name 51..54",
                "      word 51..54",
                "  comment 55..74 extra",
                "  function_definition 75..85",
                "    name: word 75..76",
                "    ( 76..77 unnamed",
                "    ) 77..78 unnamed",
                "    body: compound_statement 79..85",
                "      { 79..80 unnamed",
                "      command 81..82",
                "        name: command_name 81..82",
                "          word 81..82",
                "      ; 82..83 unnamed",
                "      } 84..85 unnamed",
            ],
        ),
        (
            b"cat <<A | cat <<B\nB\nA\n# @description LIT\nf() { :; }\nB\ng() { :; }\n",
            &[
                "program 0..65",
                "  redirected_statement 0..21",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..21",
                "      << 4..6 unnamed",
                "      heredoc_start 6..7",
                "      pipeline 8..19",
                "        | 8..9 unnamed",
                "        redirected_statement 10..19",
                "          body: command 10..13",
                "            name: command_name 10..13",
                "              word 10..13",
                "          redirect: heredoc_redirect 14..19",
                "            << 14..16 unnamed",
                "            heredoc_start 16..17",
                "            heredoc_body 18..18",
                "            heredoc_end 18..19",
                "      heredoc_body 20..20",
                "      heredoc_end 20..21",
                "  comment 22..40 extra",
                "  function_definition 41..51",
                "    name: word 41..42",
                "    ( 42..43 unnamed",
                "    ) 43..44 unnamed",
                "    body: compound_statement 45..51",
                "      { 45..46 unnamed",
                "      command 47..48",
                "        name: command_name 47..48",
                "          word 47..48",
                "      ; 48..49 unnamed",
                "      } 50..51 unnamed",
                "  command 52..53",
                "    name: command_name 52..53",
                "      word 52..53",
                "  function_definition 54..64",
                "    name: word 54..55",
                "    ( 55..56 unnamed",
                "    ) 56..57 unnamed",
                "    body: compound_statement 58..64",
                "      { 58..59 unnamed",
                "      command 60..61",
                "        name: command_name 60..61",
                "          word 60..61",
                "      ; 61..62 unnamed",
                "      } 63..64 unnamed",
            ],
        ),
        (
            b": $'\\\\''\n# @description LIT\nf() { :; }\n' #'\ng() { :; }\n",
            &[
                "program 0..55",
                "  command 0..8",
                "    name: command_name 0..1",
                "      word 0..1",
                "    argument: ansi_c_string 2..8",
                "  comment 9..27 extra",
                "  function_definition 28..38",
                "    name: word 28..29",
                "    ( 29..30 unnamed",
                "    ) 30..31 unnamed",
                "    body: compound_statement 32..38",
                "      { 32..33 unnamed",
                "      command 34..35",
                "        name: command_name 34..35",
                "          word 34..35",
                "      ; 35..36 unnamed",
                "      } 37..38 unnamed",
                "  command 39..43",
                "    name: command_name 39..43",
                "      raw_string 39..43",
                "  function_definition 44..54",
                "    name: word 44..45",
                "    ( 45..46 unnamed",
                "    ) 46..47 unnamed",
                "    body: compound_statement 48..54",
                "      { 48..49 unnamed",
                "      command 50..51",
                "        name: command_name 50..51",
                "          word 50..51",
                "      ; 51..52 unnamed",
                "      } 53..54 unnamed",
            ],
        ),
        (
            b": `'`'`\n# @description LIT\nf() { :; }\n' #'\ng() { :; }\n",
            &[
                "program 0..54",
                "  command 0..7",
                "    name: command_name 0..1",
                "      word 0..1",
                "    argument: command_substitution 2..7",
                "      ` 2..3 unnamed",
                "      command 3..6",
                "        name: command_name 3..6",
                "          raw_string 3..6",
                "      ` 6..7 unnamed",
                "  comment 8..26 extra",
                "  function_definition 27..37",
                "    name: word 27..28",
                "    ( 28..29 unnamed",
                "    ) 29..30 unnamed",
                "    body: compound_statement 31..37",
                "      { 31..32 unnamed",
                "      command 33..34",
                "        name: command_name 33..34",
                "          word 33..34",
                "      ; 34..35 unnamed",
                "      } 36..37 unnamed",
                "  command 38..42",
                "    name: command_name 38..42",
                "      raw_string 38..42",
                "  function_definition 43..53",
                "    name: word 43..44",
                "    ( 44..45 unnamed",
                "    ) 45..46 unnamed",
                "    body: compound_statement 47..53",
                "      { 47..48 unnamed",
                "      command 49..50",
                "        name: command_name 49..50",
                "          word 49..50",
                "      ; 50..51 unnamed",
                "      } 52..53 unnamed",
            ],
        ),
        (
            b"a[b[0]=1\n# @description LIT\nf() { :; }\n]=1\ng() { :; }\n",
            &[
                "program 0..54",
                "  variable_assignment 0..8",
                "    name: subscript 0..6",
                "      name: variable_name 0..1",
                "      [ 1..2 unnamed",
                "      index: concatenation 2..5",
                "        word 2..3",
                "        word 3..4",
                "        number 4..5",
                "      ] 5..6 unnamed",
                "    = 6..7 unnamed",
                "    value: number 7..8",
                "  comment 9..27 extra",
                "  function_definition 28..38",
                "    name: word 28..29",
                "    ( 29..30 unnamed",
                "    ) 30..31 unnamed",
                "    body: compound_statement 32..38",
                "      { 32..33 unnamed",
                "      command 34..35",
                "        name: command_name 34..35",
                "          word 34..35",
                "      ; 35..36 unnamed",
                "      } 37..38 unnamed",
                "  command 39..42",
                "    name: command_name 39..42",
                "      concatenation 39..42",
                "        word 39..40",
                "        word 40..42",
                "  function_definition 43..53",
                "    name: word 43..44",
                "    ( 44..45 unnamed",
                "    ) 45..46 unnamed",
                "    body: compound_statement 47..53",
                "      { 47..48 unnamed",
                "      command 49..50",
                "        name: command_name 49..50",
                "          word 49..50",
                "      ; 50..51 unnamed",
                "      } 52..53 unnamed",
            ],
        ),
        (
            b"cat <<EOF\nE\\\nOF\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            &[
                "program 0..61",
                "  redirected_statement 0..49",
                "    body: command 0..3",
                "      name: command_name 0..3",
                "        word 0..3",
                "    redirect: heredoc_redirect 4..49",
                "      << 4..6 unnamed",
                "      heredoc_start 6..9",
                "      heredoc_body 10..46",
                "      heredoc_end 46..49",
                "  function_definition 50..60",
                "    name: word 50..51",
                "    ( 51..52 unnamed",
                "    ) 52..53 unnamed",
                "    body: compound_statement 54..60",
                "      { 54..55 unnamed",
                "      command 56..57",
                "        name: command_name 56..57",
                "          word 56..57",
                "      ; 57..58 unnamed",
                "      } 59..60 unnamed",
            ],
        ),
        (
            b"let x<<=2\n# @description REAL\ng() { :; }\n",
            &[
                "program 0..41 has_error",
                "  ERROR 0..7 extra has_error is_error",
                "    command 0..5",
                "      name: command_name 0..3",
                "        word 0..3",
                "      argument: word 4..5",
                "    << 5..7 unnamed",
                "  command 7..9",
                "    name: command_name 7..9",
                "      word 7..9",
                "  comment 10..29 extra",
                "  function_definition 30..40",
                "    name: word 30..31",
                "    ( 31..32 unnamed",
                "    ) 32..33 unnamed",
                "    body: compound_statement 34..40",
                "      { 34..35 unnamed",
                "      command 36..37",
                "        name: command_name 36..37",
                "          word 36..37",
                "      ; 37..38 unnamed",
                "      } 39..40 unnamed",
            ],
        ),
        (
            b"x=`echo a # @description x`\n# @description a\nf() { :; }\n",
            &[
                "program 0..56 has_error",
                "  variable_assignment 0..56 has_error",
                "    name: variable_name 0..1",
                "    = 1..2 unnamed",
                "    value: command_substitution 2..56 has_error",
                "      ` 2..3 unnamed",
                "      command 3..9",
                "        name: command_name 3..7",
                "          word 3..7",
                "        argument: word 8..9",
                "      comment 10..27 extra",
                "      comment 28..44 extra",
                "      function_definition 45..55",
                "        name: word 45..46",
                "        ( 46..47 unnamed",
                "        ) 47..48 unnamed",
                "        body: compound_statement 49..55",
                "          { 49..50 unnamed",
                "          command 51..52",
                "            name: command_name 51..52",
                "              word 51..52",
                "          ; 52..53 unnamed",
                "          } 54..55 unnamed",
                "      ` 56..56 unnamed has_error missing",
            ],
        ),
    ];
    for (source, expected) in shapes {
        let parser = parsed(source);
        assert_eq!(
            tree_lines(parser.ts_tree().root_node()),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_tagged_run_documents_the_function_below() {
    // The first source is the primary fixture of the contract.
    assert_eq!(
        crate::test_support::documentation::source("primary.bash"),
        b"# @description audit\nf() { :; }"
    );
    for case in [
        Case {
            name: "a01_primary",
            source: b"# @description audit\nf() { :; }",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..31, top(0..31))],
                ..View::default()
            },
            owners: vec![21..31],
            ..Case::default()
        },
        Case {
            name: "a02_function_keyword",
            source: b"# @description audit\nfunction f { :; }",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..38, top(0..38))],
                ..View::default()
            },
            owners: vec![21..38],
            ..Case::default()
        },
        Case {
            name: "a03_function_keyword_parens",
            source: b"# @description audit\nfunction f() { :; }\n",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..40, top(0..41))],
                ..View::default()
            },
            owners: vec![21..40],
            ..Case::default()
        },
        Case {
            name: "a04_brace_on_next_row",
            source: b"# @description audit\nf()\n{\n  :\n}\n",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..32, top(0..33))],
                ..View::default()
            },
            owners: vec![21..32],
            ..Case::default()
        },
        Case {
            name: "a05_file_redirect",
            source: b"# @description audit\nf() { :; } >/dev/null\n",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..42, top(0..43))],
                ..View::default()
            },
            owners: vec![21..42],
            ..Case::default()
        },
        Case {
            name: "a06_background",
            source: b"# @description audit\nf() { :; } &\n",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 21..31, top(0..34))],
                ..View::default()
            },
            owners: vec![21..31],
            ..Case::default()
        },
        Case {
            name: "a12_indent_and_trailing_blanks",
            source: b"  # @description audit  \n  f() { :; }\n",
            view: View {
                documents: vec![row(2..24, "@description audit  ")],
                associations: vec![owned(0, 27..37, top(2..38))],
                ..View::default()
            },
            owners: vec![27..37],
            ..Case::default()
        },
        Case {
            name: "a13_two_on_a_row",
            source: b"# @description a\nf() { :; }; g() { :; }\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![owned(0, 17..27, top(0..40))],
                ..View::default()
            },
            owners: vec![17..27, 29..39],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_rows_of_a_run_form_one_text() {
    for case in [
        Case {
            name: "a07_rows",
            source: b"# @description First line\n# second line\n#\n# @arg $1 string Name\n\
                f() { :; }",
            view: View {
                documents: vec![rows(
                    [0..25, 26..39, 40..41, 42..63],
                    "@description First line\nsecond line\n\n@arg $1 string Name",
                    [
                        (0..23, Copied(2..25)),
                        (23..24, Synthetic),
                        (24..35, Copied(28..39)),
                        (35..36, Synthetic),
                        (36..37, Synthetic),
                        (37..56, Copied(44..63)),
                    ],
                )],
                associations: vec![owned(0, 64..74, top(0..74))],
                ..View::default()
            },
            owners: vec![64..74],
            ..Case::default()
        },
        Case {
            name: "a11_backslash_ends_a_row",
            source: b"# @description a\\\n# b\nf() { :; }",
            view: View {
                documents: vec![rows(
                    [0..17, 18..21],
                    "@description a\\\nb",
                    [
                        (0..15, Copied(2..17)),
                        (15..16, Synthetic),
                        (16..17, Copied(20..21)),
                    ],
                )],
                associations: vec![owned(0, 22..32, top(0..32))],
                ..View::default()
            },
            owners: vec![22..32],
            ..Case::default()
        },
        Case {
            name: "s04_mention",
            source: b"# @description audit\n# note: shellcheck is noisy\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..20, 21..48],
                    "@description audit\nnote: shellcheck is noisy",
                    [
                        (0..18, Copied(2..20)),
                        (18..19, Synthetic),
                        (19..44, Copied(23..48)),
                    ],
                )],
                associations: vec![owned(0, 49..59, top(0..60))],
                ..View::default()
            },
            owners: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "s08_broken_directives",
            source: b"# @description audit\n# shellchecker x\n# shellcheck: y\n# shellcheck\n\
                # ShellCheck z\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..20, 67..81],
                    "@description audit\nShellCheck z",
                    [
                        (0..18, Copied(2..20)),
                        (18..19, Synthetic),
                        (19..31, Copied(69..81)),
                    ],
                )],
                associations: vec![owned(0, 82..92, top(0..93))],
                ..View::default()
            },
            directives: vec![
                (Directive, 21..37),
                (Directive, 38..53),
                (Directive, 54..66),
            ],
            both: vec![
                (Directive, 21..37),
                (Directive, 38..53),
                (Directive, 54..66),
            ],
            owners: vec![82..92],
            ..Case::default()
        },
        Case {
            name: "a14_section_row",
            source: b"# @section Utils\n# @description d\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..16, 17..33],
                    "@section Utils\n@description d",
                    [
                        (0..14, Copied(2..16)),
                        (14..15, Synthetic),
                        (15..29, Copied(19..33)),
                    ],
                )],
                associations: vec![owned(0, 34..44, top(0..45))],
                ..View::default()
            },
            owners: vec![34..44],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_documents_follow_source_order_and_keep_their_scope() {
    for case in [
        Case {
            name: "a08_two_functions",
            source: b"# @description a\nf() { :; }\n# @description b\nfunction g { :; }\n",
            view: View {
                documents: vec![row(0..16, "@description a"), row(28..44, "@description b")],
                associations: vec![owned(0, 17..27, top(0..63)), owned(1, 45..62, top(0..63))],
                ..View::default()
            },
            owners: vec![17..27, 45..62],
            ..Case::default()
        },
        Case {
            name: "a09_nested",
            source: b"# @description outer\n\
                outer() {\n  # @description inner\n  inner() { :; }\n}\n",
            view: View {
                documents: vec![
                    row(0..20, "@description outer"),
                    row(33..53, "@description inner"),
                ],
                associations: vec![owned(0, 21..72, top(0..73)), owned(1, 56..70, body(29..72))],
                ..View::default()
            },
            owners: vec![21..72, 56..70],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_functions_are_owners_without_documentation() {
    for case in [Case {
        name: "a10_undocumented",
        source: b"f() { :; }\ng() { :; }\n",
        owners: vec![0..10, 11..21],
        ..Case::default()
    }] {
        check(&case);
        let parser = parsed(case.source);
        let [index, _] = indexes(&parser, DocumentationOptions::default());
        // expected: the file and the two functions.
        assert_eq!(index.facts().owners().len(), 3);
        for range in &case.owners {
            let node = node_at(&parser, "function_definition", range.clone());
            let query = index.node_associations(node).unwrap();
            assert_eq!(query.associations().len(), 0);
            assert_eq!(query.assessments().count(), 0);
        }
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_tag_lines_open_a_run() {
    for case in [
        Case {
            name: "t01_tag",
            source: b"# @description x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..16, "@description x")],
                associations: vec![owned(0, 17..27, top(0..28))],
                ..View::default()
            },
            owners: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "t02_tag",
            source: b"# @example\nf() { :; }\n",
            view: View {
                documents: vec![row(0..10, "@example")],
                associations: vec![owned(0, 11..21, top(0..22))],
                ..View::default()
            },
            owners: vec![11..21],
            ..Case::default()
        },
        Case {
            name: "t03_tag",
            source: b"# @option -h | --help Show help\nf() { :; }\n",
            view: View {
                documents: vec![row(0..31, "@option -h | --help Show help")],
                associations: vec![owned(0, 32..42, top(0..43))],
                ..View::default()
            },
            owners: vec![32..42],
            ..Case::default()
        },
        Case {
            name: "t04_tag",
            source: b"# @arg $1 string Name\nf() { :; }\n",
            view: View {
                documents: vec![row(0..21, "@arg $1 string Name")],
                associations: vec![owned(0, 22..32, top(0..33))],
                ..View::default()
            },
            owners: vec![22..32],
            ..Case::default()
        },
        Case {
            name: "t05_tag",
            source: b"# @noargs\nf() { :; }\n",
            view: View {
                documents: vec![row(0..9, "@noargs")],
                associations: vec![owned(0, 10..20, top(0..21))],
                ..View::default()
            },
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t06_tag",
            source: b"# @set VAR string Value\nf() { :; }\n",
            view: View {
                documents: vec![row(0..23, "@set VAR string Value")],
                associations: vec![owned(0, 24..34, top(0..35))],
                ..View::default()
            },
            owners: vec![24..34],
            ..Case::default()
        },
        Case {
            name: "t07_tag",
            source: b"# @exitcode 0 If ok\nf() { :; }\n",
            view: View {
                documents: vec![row(0..19, "@exitcode 0 If ok")],
                associations: vec![owned(0, 20..30, top(0..31))],
                ..View::default()
            },
            owners: vec![20..30],
            ..Case::default()
        },
        Case {
            name: "t08_tag",
            source: b"# @see other\nf() { :; }\n",
            view: View {
                documents: vec![row(0..12, "@see other")],
                associations: vec![owned(0, 13..23, top(0..24))],
                ..View::default()
            },
            owners: vec![13..23],
            ..Case::default()
        },
        Case {
            name: "t09_tag",
            source: b"# @deprecated\nf() { :; }\n",
            view: View {
                documents: vec![row(0..13, "@deprecated")],
                associations: vec![owned(0, 14..24, top(0..25))],
                ..View::default()
            },
            owners: vec![14..24],
            ..Case::default()
        },
        Case {
            name: "t10_tag",
            source: b"# @stdin Data\nf() { :; }\n",
            view: View {
                documents: vec![row(0..13, "@stdin Data")],
                associations: vec![owned(0, 14..24, top(0..25))],
                ..View::default()
            },
            owners: vec![14..24],
            ..Case::default()
        },
        Case {
            name: "t11_tag",
            source: b"# @stdout Data\nf() { :; }\n",
            view: View {
                documents: vec![row(0..14, "@stdout Data")],
                associations: vec![owned(0, 15..25, top(0..26))],
                ..View::default()
            },
            owners: vec![15..25],
            ..Case::default()
        },
        Case {
            name: "t12_tag",
            source: b"# @stderr Data\nf() { :; }\n",
            view: View {
                documents: vec![row(0..14, "@stderr Data")],
                associations: vec![owned(0, 15..25, top(0..26))],
                ..View::default()
            },
            owners: vec![15..25],
            ..Case::default()
        },
        Case {
            name: "t13_tag",
            source: b"# @descriptionx\nf() { :; }\n",
            view: View {
                documents: vec![row(0..15, "@descriptionx")],
                associations: vec![owned(0, 16..26, top(0..27))],
                ..View::default()
            },
            owners: vec![16..26],
            ..Case::default()
        },
        Case {
            name: "t14_tag",
            source: b"# @seealso x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..12, "@seealso x")],
                associations: vec![owned(0, 13..23, top(0..24))],
                ..View::default()
            },
            owners: vec![13..23],
            ..Case::default()
        },
        Case {
            name: "t15_tag",
            source: b"#  @arg $1 x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..12, " @arg $1 x")],
                associations: vec![owned(0, 13..23, top(0..24))],
                ..View::default()
            },
            owners: vec![13..23],
            ..Case::default()
        },
        Case {
            name: "t16_tag",
            source: b"#\t@option -h x\nf() { :; }\n",
            view: View {
                documents: vec![rows([0..14], "\t@option -h x", [(0..13, Copied(1..14))])],
                associations: vec![owned(0, 15..25, top(0..26))],
                ..View::default()
            },
            owners: vec![15..25],
            ..Case::default()
        },
        Case {
            name: "t17_tag",
            source: b"#  @noargs  \nf() { :; }\n",
            view: View {
                documents: vec![row(0..12, " @noargs  ")],
                associations: vec![owned(0, 13..23, top(0..24))],
                ..View::default()
            },
            owners: vec![13..23],
            ..Case::default()
        },
        Case {
            name: "t18_tag",
            source: b"# @section x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..12, "@section x")],
                associations: vec![owned(0, 13..23, top(0..24))],
                ..View::default()
            },
            owners: vec![13..23],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_rows_that_shdoc_reads_as_text_open_no_run() {
    for case in [
        Case {
            name: "t21_text",
            source: b"#@description audit\nf() { :; }\n",
            unbound: vec![(Unattached, 0..19)],
            both: vec![(Unattached, 0..19)],
            owners: vec![20..30],
            ..Case::default()
        },
        Case {
            name: "t22_text",
            source: b"## @description audit\nf() { :; }\n",
            unbound: vec![(Unattached, 0..21)],
            both: vec![(Unattached, 0..21)],
            owners: vec![22..32],
            ..Case::default()
        },
        Case {
            name: "t23_text",
            source: b"#  @description x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..17)],
            both: vec![(Unattached, 0..17)],
            owners: vec![18..28],
            ..Case::default()
        },
        Case {
            name: "t24_text",
            source: b"#\t@description x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            owners: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "t25_text",
            source: b"# @Description x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            owners: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "t26_text",
            source: b"# @arg\nf() { :; }\n",
            unbound: vec![(Unattached, 0..6)],
            both: vec![(Unattached, 0..6)],
            owners: vec![7..17],
            ..Case::default()
        },
        Case {
            name: "t27_text",
            source: b"# @arg  \nf() { :; }\n",
            unbound: vec![(Unattached, 0..8)],
            both: vec![(Unattached, 0..8)],
            owners: vec![9..19],
            ..Case::default()
        },
        Case {
            name: "t28_text",
            source: b"# @noargs x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..11)],
            both: vec![(Unattached, 0..11)],
            owners: vec![12..22],
            ..Case::default()
        },
        Case {
            name: "t29_text",
            source: b"# @stdout\nf() { :; }\n",
            unbound: vec![(Unattached, 0..9)],
            both: vec![(Unattached, 0..9)],
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t30_text",
            source: b"# @todo x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..9)],
            both: vec![(Unattached, 0..9)],
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t31_text",
            source: b"#\t@name x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..9)],
            both: vec![(Unattached, 0..9)],
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t32_text",
            source: b"# @ description x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..17)],
            both: vec![(Unattached, 0..17)],
            owners: vec![18..28],
            ..Case::default()
        },
        Case {
            name: "u01_unmarked",
            source: b"# audit\nf() { :; }",
            unbound: vec![(Unattached, 0..7)],
            both: vec![(Unattached, 0..7)],
            owners: vec![8..18],
            ..Case::default()
        },
        Case {
            name: "u02_address",
            source: b"# mail dev@example.com\nf() { :; }\n",
            unbound: vec![(Unattached, 0..22)],
            both: vec![(Unattached, 0..22)],
            owners: vec![23..33],
            ..Case::default()
        },
    ] {
        assert_eq!(case.view, View::default());
        assert_ne!(case.unbound, Kept::new());
        check(&case);
    }
}

// The four file-tag rows pin a pending outcome: the unit that decides what a file
// document is changes it on purpose.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_file_tags_and_plain_first_rows_keep_a_pending_document() {
    for case in [
        Case {
            name: "t41_file_tag",
            source: b"# @name x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..9, "@name x")],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t42_file_tag",
            source: b"# @file x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..9, "@file x")],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![10..20],
            ..Case::default()
        },
        Case {
            name: "t43_file_tag",
            source: b"# @brief x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..10, "@brief x")],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![11..21],
            ..Case::default()
        },
        Case {
            name: "t44_file_tag",
            source: b"# @files x\nf() { :; }\n",
            view: View {
                documents: vec![row(0..10, "@files x")],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![11..21],
            ..Case::default()
        },
        Case {
            name: "s06_shebang_not_first",
            source: b"\n#!/bin/sh\n# @description audit\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [1..10, 11..31],
                    "!/bin/sh\n@description audit",
                    [
                        (0..8, Copied(2..10)),
                        (8..9, Synthetic),
                        (9..27, Copied(13..31)),
                    ],
                )],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![32..42],
            ..Case::default()
        },
        Case {
            name: "p01_file",
            source: b"# @file sample\n# @description audit\necho ok",
            view: View {
                documents: vec![rows(
                    [0..14, 15..35],
                    "@file sample\n@description audit",
                    [
                        (0..12, Copied(2..14)),
                        (12..13, Synthetic),
                        (13..31, Copied(17..35)),
                    ],
                )],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "p02_file_then_function",
            source: b"# @file sample\n# @description file text\n\n# @description fn text\n\
                f() { :; }\n",
            view: View {
                documents: vec![
                    rows(
                        [0..14, 15..39],
                        "@file sample\n@description file text",
                        [
                            (0..12, Copied(2..14)),
                            (12..13, Synthetic),
                            (13..35, Copied(17..39)),
                        ],
                    ),
                    row(41..63, "@description fn text"),
                ],
                associations: vec![owned(1, 64..74, top(0..75))],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![64..74],
            ..Case::default()
        },
        Case {
            name: "p03_name_above_function",
            source: b"# @name lib\n# @description d\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..11, 12..28],
                    "@name lib\n@description d",
                    [
                        (0..9, Copied(2..11)),
                        (9..10, Synthetic),
                        (10..24, Copied(14..28)),
                    ],
                )],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "p04_plain_first",
            source: b"# plain\n# @description audit\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..7, 8..28],
                    "plain\n@description audit",
                    [
                        (0..5, Copied(2..7)),
                        (5..6, Synthetic),
                        (6..24, Copied(10..28)),
                    ],
                )],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "p07_brief",
            source: b"# @description d\n# @brief short\nf() { :; }\n",
            view: View {
                documents: vec![rows(
                    [0..16, 17..31],
                    "@description d\n@brief short",
                    [
                        (0..14, Copied(2..16)),
                        (14..15, Synthetic),
                        (15..27, Copied(19..31)),
                    ],
                )],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            owners: vec![32..42],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_detached_runs_are_not_selected() {
    for case in [
        Case {
            name: "d01_blank_row",
            source: b"# @description audit\n\nf() { :; }\n",
            unbound: vec![(Unattached, 0..20)],
            both: vec![(Unattached, 0..20)],
            owners: vec![22..32],
            ..Case::default()
        },
        Case {
            name: "d02_command_between",
            source: b"# @description audit\necho x\nf() { :; }\n",
            unbound: vec![(Unattached, 0..20)],
            both: vec![(Unattached, 0..20)],
            owners: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "d03_command_on_the_row",
            source: b"# @description a\necho x; f() { :; }\n",
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            owners: vec![25..35],
            ..Case::default()
        },
        Case {
            name: "d04_end_of_file",
            source: b"# @description a\n",
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            ..Case::default()
        },
        Case {
            name: "d05_above_a_command",
            source: b"# @description audit\necho ok\n",
            unbound: vec![(Unattached, 0..20)],
            both: vec![(Unattached, 0..20)],
            ..Case::default()
        },
    ] {
        assert_eq!(case.view, View::default());
        assert_ne!(case.unbound, Kept::new());
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_comments_after_code_open_no_run() {
    for case in [
        Case {
            name: "r01_trailing_then_run",
            source: b"g() { :; } # after g\n# @description h\nh() { :; }\n",
            view: View {
                documents: vec![row(21..37, "@description h")],
                associations: vec![owned(0, 38..48, top(0..49))],
                ..View::default()
            },
            unbound: vec![(Unattached, 11..20)],
            both: vec![(Unattached, 11..20)],
            owners: vec![0..10, 38..48],
            ..Case::default()
        },
        Case {
            name: "r02_trailing_tag_then_plain",
            source: b"x=1 # @description t\n# plain\nf() { :; }\n",
            unbound: vec![(Unattached, 4..20), (Unattached, 21..28)],
            both: vec![(Unattached, 4..20), (Unattached, 21..28)],
            owners: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "r03_in_a_body",
            source: b"f() {\n  # @description inner note\n  echo 1\n}\n",
            unbound: vec![(Unattached, 8..33)],
            both: vec![(Unattached, 8..33)],
            owners: vec![0..44],
            ..Case::default()
        },
        Case {
            name: "r04_trailing_tag_then_tag",
            source: b"x=1 # @description t\n# @description u\nf() { :; }\n",
            view: View {
                documents: vec![row(21..37, "@description u")],
                associations: vec![owned(0, 38..48, top(0..49))],
                ..View::default()
            },
            unbound: vec![(Unattached, 4..20)],
            both: vec![(Unattached, 4..20)],
            owners: vec![38..48],
            ..Case::default()
        },
    ] {
        assert_ne!(case.unbound, Kept::new());
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_directive_rows_are_candidates_and_not_text() {
    for case in [
        Case {
            name: "s01_shebang",
            source: b"#!/bin/bash\n# @description audit\nf() { :; }\n",
            view: View {
                documents: vec![row(12..32, "@description audit")],
                associations: vec![owned(0, 33..43, top(0..44))],
                ..View::default()
            },
            directives: vec![(Directive, 0..11)],
            both: vec![(Directive, 0..11)],
            owners: vec![33..43],
            ..Case::default()
        },
        Case {
            name: "s02_shellcheck_below",
            source: b"# @description audit\n# shellcheck disable=SC2034\nf() { :; }\n",
            view: View {
                documents: vec![row(0..20, "@description audit")],
                associations: vec![owned(0, 49..59, top(0..60))],
                ..View::default()
            },
            directives: vec![(Directive, 21..48)],
            both: vec![(Directive, 21..48)],
            owners: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "s03_shellcheck_above",
            source: b"# shellcheck disable=SC2034\n# @description audit\nf() { :; }\n",
            view: View {
                documents: vec![row(28..48, "@description audit")],
                associations: vec![owned(0, 49..59, top(0..60))],
                ..View::default()
            },
            directives: vec![(Directive, 0..27)],
            both: vec![(Directive, 0..27)],
            owners: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "s05_only_directives",
            source: b"#!/bin/bash\n# shellcheck shell=bash\nf() { :; }\n",
            both: vec![(Directive, 0..11), (Directive, 12..35)],
            owners: vec![36..46],
            ..Case::default()
        },
        Case {
            name: "s07_shellcheck_spellings",
            source: b"#shellcheck disable=SC2034\n#\tshellcheck disable=SC2034\n\
                # @description audit\nf() { :; }\n",
            view: View {
                documents: vec![row(55..75, "@description audit")],
                associations: vec![owned(0, 76..86, top(0..87))],
                ..View::default()
            },
            directives: vec![(Directive, 0..26), (Directive, 27..54)],
            both: vec![(Directive, 0..26), (Directive, 27..54)],
            owners: vec![76..86],
            ..Case::default()
        },
    ] {
        assert_ne!(case.both, Kept::new());
        check(&case);
    }
}

// A document without an owner is a pending outcome: a later unit decides whether the
// grammar's error node is a function.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_run_above_a_syntax_error_keeps_its_document_unresolved() {
    for case in [
        Case {
            name: "p05_for_body",
            source: b"# @description a\ng1() for i in 1; do :; done\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![ownerless(0, None, top(0..45))],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "p06_keyword_without_body",
            source: b"# @description a\nfunction f\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![ownerless(0, None, top(0..28))],
                assessments: unordered([selected(None, 0)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "e01_error_in_the_function",
            source: b"# @description a\nf() { :; }\n# @description b\ng() { echo ) ; }\n",
            view: View {
                documents: vec![row(0..16, "@description a"), row(28..44, "@description b")],
                associations: vec![
                    owned(0, 17..27, top(0..62)),
                    ownerless(1, Some(45..61), top(0..62)),
                ],
                assessments: unordered([
                    recognized(function(45..61)),
                    selected(Some(function(45..61)), 1),
                ]),
                ..View::default()
            },
            owners: vec![17..27],
            pending: vec![45..61],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

// The pending functions pin today's outcome; the unit that decides the other positions
// changes it on purpose. In `n04` the position is the grammar's alone: it reads a
// comment in back quotes to the end of the row and never ends the substitution, so
// the function below, which Bash defines, is a node of the substitution.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_functions_outside_the_two_owner_positions_stay_pending() {
    for case in [
        Case {
            name: "o01_subshell",
            source: b"(\n  # @description s\n  f() { :; }\n)\n",
            view: View {
                documents: vec![row(4..20, "@description s")],
                assessments: unordered([
                    recognized(function(23..33)),
                    selected(Some(function(23..33)), 0),
                ]),
                ..View::default()
            },
            pending: vec![23..33],
            ..Case::default()
        },
        Case {
            name: "o02_group",
            source: b"{\n  # @description g\n  f() { :; }\n}\n",
            view: View {
                documents: vec![row(4..20, "@description g")],
                assessments: unordered([
                    recognized(function(23..33)),
                    selected(Some(function(23..33)), 0),
                ]),
                ..View::default()
            },
            pending: vec![23..33],
            ..Case::default()
        },
        Case {
            name: "o03_if",
            source: b"if true; then\n  # @description i\n  f() { :; }\nfi\n",
            view: View {
                documents: vec![row(16..32, "@description i")],
                assessments: unordered([
                    recognized(function(35..45)),
                    selected(Some(function(35..45)), 0),
                ]),
                ..View::default()
            },
            pending: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "o04_loop",
            source: b"for i in 1; do\n  # @description l\n  f() { :; }\ndone\n",
            view: View {
                documents: vec![row(17..33, "@description l")],
                assessments: unordered([
                    recognized(function(36..46)),
                    selected(Some(function(36..46)), 0),
                ]),
                ..View::default()
            },
            pending: vec![36..46],
            ..Case::default()
        },
        Case {
            name: "o05_case_arm",
            source: b"case x in\n  a)\n    # @description c\n    f() { :; }\n    ;;\nesac\n",
            view: View {
                documents: vec![row(19..35, "@description c")],
                assessments: unordered([
                    recognized(function(40..50)),
                    selected(Some(function(40..50)), 0),
                ]),
                ..View::default()
            },
            pending: vec![40..50],
            ..Case::default()
        },
        Case {
            name: "o07_after_a_pipe",
            source: b"echo a |\n# @description x\nf() { :; }\n",
            view: View {
                documents: vec![row(9..25, "@description x")],
                assessments: unordered([
                    recognized(function(26..36)),
                    selected(Some(function(26..36)), 0),
                ]),
                ..View::default()
            },
            pending: vec![26..36],
            ..Case::default()
        },
        Case {
            name: "o08_list",
            source: b"# @description j\nj() { :; } && echo ok\n",
            view: View {
                assessments: unordered([recognized(function(17..27))]),
                ..View::default()
            },
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            pending: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "o09_pipeline",
            source: b"# @description k\nk() { :; } | cat\n",
            view: View {
                assessments: unordered([recognized(function(17..27))]),
                ..View::default()
            },
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            pending: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "o10_heredoc_redirect",
            source: b"# @description c\nh() { :; } <<EOF\nbody\nEOF\n",
            view: View {
                assessments: unordered([recognized(function(17..27))]),
                ..View::default()
            },
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            pending: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "o11_body_of_a_pending_function",
            source: b"(\nouter() {\n  # @description i\n  inner() { :; }\n}\n)\n",
            view: View {
                documents: vec![row(14..30, "@description i")],
                associations: vec![owned(0, 33..47, body(10..49))],
                assessments: unordered([recognized(function(2..49))]),
                ..View::default()
            },
            owners: vec![33..47],
            pending: vec![2..49],
            ..Case::default()
        },
        Case {
            name: "n04_comment_in_back_quotes",
            source: b"x=`echo a # @description x`\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(28..44, "@description a")],
                assessments: unordered([
                    recognized(function(45..55)),
                    selected(Some(function(45..55)), 0),
                ]),
                ..View::default()
            },
            pending: vec![45..55],
            ..Case::default()
        },
    ] {
        assert_ne!(case.pending, Vec::<Range<usize>>::new());
        check(&case);
    }
}

// A function in a substitution is no function of the file: it waits, at any depth,
// also in the body of another function there. The scan says where a substitution is.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_functions_in_a_substitution_stay_pending() {
    for case in [
        Case {
            name: "o06_substitution",
            source: b"x=$(\n  # @description s\n  f() { :; }\n)\n",
            view: View {
                assessments: unordered([recognized(function(26..36)), withheld(7..23)]),
                ..View::default()
            },
            pending: vec![26..36],
            ..Case::default()
        },
        Case {
            name: "o12_back_quotes",
            source: b"x=`\n  # @description s\n  f() { :; }\n`\n",
            view: View {
                assessments: unordered([recognized(function(25..35))]),
                ..View::default()
            },
            pending: vec![25..35],
            ..Case::default()
        },
        Case {
            name: "b01_nested_function_on_a_row",
            source: b"x=$(o() { i() { :; }; }; o)\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(28..44, "@description a")],
                associations: vec![owned(0, 45..55, top(0..56))],
                assessments: unordered([recognized(function(10..20)), recognized(function(4..23))]),
                ..View::default()
            },
            owners: vec![45..55],
            pending: vec![4..23, 10..20],
            ..Case::default()
        },
        Case {
            name: "b02_nested_function_on_a_row",
            source: b"x=`o() { i() { :; }; }; o`\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(27..43, "@description a")],
                associations: vec![owned(0, 44..54, top(0..55))],
                assessments: unordered([recognized(function(3..22)), recognized(function(9..19))]),
                ..View::default()
            },
            owners: vec![44..54],
            pending: vec![3..22, 9..19],
            ..Case::default()
        },
        Case {
            name: "b03_nested_function_on_a_row",
            source: b"cat <(o() { i() { :; }; }; o)\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(30..46, "@description a")],
                associations: vec![owned(0, 47..57, top(0..58))],
                assessments: unordered([recognized(function(12..22)), recognized(function(6..25))]),
                ..View::default()
            },
            owners: vec![47..57],
            pending: vec![6..25, 12..22],
            ..Case::default()
        },
        Case {
            name: "b05_process_substitution_ast_ancestor",
            source: b"cat >(o() { i() { :; }; }; o)\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(30..46, "@description a")],
                associations: vec![owned(0, 47..57, top(0..58))],
                assessments: unordered([recognized(function(12..22)), recognized(function(6..25))]),
                ..View::default()
            },
            owners: vec![47..57],
            pending: vec![6..25, 12..22],
            ..Case::default()
        },
        Case {
            name: "b06_process_substitution_command_reading",
            source: b"cat <((:); : #); (\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(19..66)),
                    recognized(function(50..64)),
                    withheld(29..49),
                ]),
                ..View::default()
            },
            pending: vec![19..66, 50..64],
            ..Case::default()
        },
        Case {
            name: "b07_output_substitution_command_reading",
            source: b"cat >((:); : #); (\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(19..66)),
                    recognized(function(50..64)),
                    withheld(29..49),
                ]),
                ..View::default()
            },
            pending: vec![19..66, 50..64],
            ..Case::default()
        },
        Case {
            name: "b08_duplicated_output_in_a_form",
            source: b"x=$(: >&-#); (\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(15..62)),
                    recognized(function(46..60)),
                    withheld(25..45),
                ]),
                ..View::default()
            },
            pending: vec![15..62, 46..60],
            ..Case::default()
        },
        Case {
            name: "b09_duplicated_input_in_a_form",
            source: b"x=$(: <&-#); (\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(15..62)),
                    recognized(function(46..60)),
                    withheld(25..45),
                ]),
                ..View::default()
            },
            pending: vec![15..62, 46..60],
            ..Case::default()
        },
    ] {
        // No function inside the substitution is an owner; the one below it, where
        // the source has one, is.
        assert!(!case.pending.is_empty(), "{}", case.name);
        assert!(case.owners.len() <= 1, "{}", case.name);
        check(&case);
    }
    // The control: the same two functions outside a substitution are owners.
    let [case] = [Case {
        name: "b04_nested_function_plain",
        source: b"o() { i() { :; }; }\n# @description a\nf() { :; }\n",
        view: View {
            documents: vec![row(20..36, "@description a")],
            associations: vec![owned(0, 37..47, top(0..48))],
            ..View::default()
        },
        owners: vec![0..19, 6..16, 37..47],
        ..Case::default()
    }];
    assert_eq!((case.owners.len(), case.pending.len()), (3, 0));
    check(&case);
    // A nested function's direct body position normally makes it an owner.
    // The substitution ancestor overrides that position at any depth.
    let source = b"o() { cat <(g() { i() { :; }; }); }\n";
    let parser = parsed(source);
    let [index, _] = indexes(&parser, DocumentationOptions::default());
    let outer = node_at(&parser, "function_definition", 0..35);
    let nested = node_at(&parser, "function_definition", 18..28);
    assert_eq!(index.node_owners(outer).unwrap().count(), 1);
    assert!(index.node_projection(nested).unwrap().pending());
    assert!(matches!(
        index.node_owners(nested),
        Err(OwnerError::PendingOwner)
    ));

    // Hold the lexical reading fixed at command level. A real substitution
    // also makes the scan say Form, which would mask a missing ancestor guard.
    let parser = parsed(b"f() { :; }\n");
    let function = node_at(&parser, "function_definition", 0..10);
    for (in_substitution, pending) in [(false, false), (true, true)] {
        let [index, _] = indexes(&parser, DocumentationOptions::default());
        let id = index.canonical.lookup(function).unwrap();
        let mut builder = super::Builder {
            index,
            drafts: Vec::new(),
            pending_attributes: Vec::new(),
            rust_values: std::collections::HashMap::new(),
            names: crate::documentation::names::Names::default(),
            pod_doubt: super::pod_doubt::Doubt::default(),
            pod_hidden: super::pod::Hidden::default(),
            tcl_scan: crate::lang_helpers::tcl_scan::CommandScan::default(),
            shdoc: crate::lang_helpers::shdoc::Reading::default(),
        };
        let node = crate::Node::from_tree_sitter(function);
        let budget = crate::documentation::budget::CollectionBudget::default();
        builder
            .shdoc
            .enter(&builder.index.source, &node, false, &budget)
            .unwrap();
        builder
            .register::<crate::BashCode>(
                id,
                &node,
                crate::node::Ancestors::unknown(),
                in_substitution,
                &budget,
            )
            .unwrap();
        assert_eq!(
            matches!(
                builder.index.canonical.nodes[id.0].projection,
                super::Projection::Pending
            ),
            pending,
            "substitution ancestor={in_substitution}"
        );
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_here_document_text_is_never_documentation() {
    for case in [
        Case {
            name: "h01_quoted",
            source: b"cat <<'EOF'\n# @description audit\nEOF\nf() { :; }",
            owners: vec![37..47],
            ..Case::default()
        },
        Case {
            name: "h06_substitution_in_text",
            source: b"cat <<EOF\n$(\n# @description x\nf() { :; }\n)\nEOF\ng() { :; }\n",
            owners: vec![47..57],
            strangers: vec![30..40],
            ..Case::default()
        },
        Case {
            name: "h07_comment_rows_in_text",
            source: b"cat <<EOF # c \\\n\\\n# @description x\nf() { :; }\nEOF\ng() { :; }\n",
            unbound: vec![(Unattached, 10..15)],
            both: vec![(Unattached, 10..15)],
            owners: vec![50..60],
            ..Case::default()
        },
        Case {
            name: "w02_blank_after_end",
            source: b"cat <<EOF\nEOF \n# @description a\nf() { :; }\nEOF\n",
            strangers: vec![32..42],
            ..Case::default()
        },
        Case {
            name: "w03_dash_spaces",
            source: b"cat <<-EOF\n  EOF\n# @description a\nf() { :; }\n\tEOF\n",
            strangers: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "w04_split_word",
            source: b"cat <<'E'\\''F'\nE\n# @description a\nf() { :; }\nE'F\n",
            strangers: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "w13_open_quote",
            source: b"cat <<EOF \"a\nEOF\nb\"\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            owners: vec![54..64],
            ..Case::default()
        },
        Case {
            name: "w23_text_after_the_boundary",
            source: b"cat <<EOF\n EOF\nEOF\ncat <<EOF\n$(\n# @description x\nf() { :; }\n)\nEOF\n",
            strangers: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "h02_pipe",
            source: b"cat <<EOT |\n# @description x\nf() { :; }\nEOT\ngrep x\n",
            strangers: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "h03_pipe_group",
            source: b"cat <<EOT |\n{\n# @description x\nf() { :; }\n}\nEOT\ngrep x\n",
            strangers: vec![31..41],
            ..Case::default()
        },
        Case {
            name: "h04_pipe_file_tag",
            source: b"cat <<EOT |\n# @file x\n# @description y\nf() { :; }\nEOT\ngrep x\n",
            strangers: vec![39..49],
            ..Case::default()
        },
        Case {
            name: "h05_and",
            source: b"cat <<EOT &&\n# @description x\nf() { :; }\nEOT\necho ok\n",
            strangers: vec![30..40],
            ..Case::default()
        },
    ] {
        // `h07` keeps the comment behind the operator as a candidate: Bash reads it.
        assert_eq!(case.view, View::default());
        assert_eq!(case.directives, Kept::new());
        check(&case);
    }
}

// `w11` pins an outcome for a source that is no valid Bash: Bash ends the text at the
// first row `EOF` and rejects the `)` behind it, the grammar reads the text on to the
// second `EOF` and reports nothing. The unit reads the text as Bash does, has no command
// grammar, and publishes the document.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_rows_behind_a_here_document_are_read_where_bash_ends_it() {
    for case in [
        Case {
            name: "v01_plain",
            source: b"cat <<EOF\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(19..35, "@description a")],
                associations: vec![owned(0, 36..46, top(0..47))],
                ..View::default()
            },
            owners: vec![36..46],
            ..Case::default()
        },
        Case {
            name: "v02_single_quotes",
            source: b"cat <<'EOF'\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(21..37, "@description a")],
                associations: vec![owned(0, 38..48, top(0..49))],
                ..View::default()
            },
            owners: vec![38..48],
            ..Case::default()
        },
        Case {
            name: "v03_double_quotes",
            source: b"cat <<\"EOF\"\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(21..37, "@description a")],
                associations: vec![owned(0, 38..48, top(0..49))],
                ..View::default()
            },
            owners: vec![38..48],
            ..Case::default()
        },
        Case {
            name: "v04_backslash",
            source: b"cat <<\\EOF\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(20..36, "@description a")],
                associations: vec![owned(0, 37..47, top(0..48))],
                ..View::default()
            },
            owners: vec![37..47],
            ..Case::default()
        },
        Case {
            name: "v05_dash_tabs",
            source: b"cat <<-EOF\n\tbody\n\tEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(22..38, "@description a")],
                associations: vec![owned(0, 39..49, top(0..50))],
                ..View::default()
            },
            owners: vec![39..49],
            ..Case::default()
        },
        Case {
            name: "v06_pipe_on_the_row",
            source: b"cat <<EOF | grep x\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(28..44, "@description a")],
                associations: vec![owned(0, 45..55, top(0..56))],
                ..View::default()
            },
            owners: vec![45..55],
            ..Case::default()
        },
        Case {
            name: "v07_redirect_on_the_row",
            source: b"cat <<EOF >out\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(24..40, "@description a")],
                associations: vec![owned(0, 41..51, top(0..52))],
                ..View::default()
            },
            owners: vec![41..51],
            ..Case::default()
        },
        Case {
            name: "v08_in_a_function",
            source: b"u() {\n  cat <<EOF\nbody\nEOF\n}\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(29..45, "@description a")],
                associations: vec![owned(0, 46..56, top(0..57))],
                ..View::default()
            },
            owners: vec![0..28, 46..56],
            ..Case::default()
        },
        Case {
            name: "v09_empty_body",
            source: b"cat <<EOF\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(14..30, "@description a")],
                associations: vec![owned(0, 31..41, top(0..42))],
                ..View::default()
            },
            owners: vec![31..41],
            ..Case::default()
        },
        Case {
            name: "v10_shift",
            source: b"x=$(( 1 << 2 ))\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(16..32, "@description a")],
                associations: vec![owned(0, 33..43, top(0..44))],
                ..View::default()
            },
            owners: vec![33..43],
            ..Case::default()
        },
        Case {
            name: "v11_loop",
            source: b"while read -r x; do :; done <<EOF\nbody\nEOF\n# @description a\n\
                f() { :; }\n",
            view: View {
                documents: vec![row(43..59, "@description a")],
                associations: vec![owned(0, 60..70, top(0..71))],
                ..View::default()
            },
            owners: vec![60..70],
            ..Case::default()
        },
        Case {
            name: "v12_blank_in_delimiter",
            source: b"cat <<\"E F\"\nE\nE F\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(18..34, "@description a")],
                associations: vec![owned(0, 35..45, top(0..46))],
                ..View::default()
            },
            owners: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "v13_blank_first_row",
            source: b"cat <<EOF\n\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(20..36, "@description a")],
                associations: vec![owned(0, 37..47, top(0..48))],
                ..View::default()
            },
            owners: vec![37..47],
            ..Case::default()
        },
        Case {
            name: "v14_blank_rows_only",
            source: b"cat <<EOF\n\n\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(16..32, "@description a")],
                associations: vec![owned(0, 33..43, top(0..44))],
                ..View::default()
            },
            owners: vec![33..43],
            ..Case::default()
        },
        Case {
            name: "v15_double_quoted_tail",
            source: b"cat <<EOF >\"$out\"\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(27..43, "@description a")],
                associations: vec![owned(0, 44..54, top(0..55))],
                ..View::default()
            },
            owners: vec![44..54],
            ..Case::default()
        },
        Case {
            name: "v16_single_quoted_tail",
            source: b"cat <<EOF | sed 's/a|b/(c)/'\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(38..54, "@description a")],
                associations: vec![owned(0, 55..65, top(0..66))],
                ..View::default()
            },
            owners: vec![55..65],
            ..Case::default()
        },
        Case {
            name: "v17_braced_name_in_tail",
            source: b"cat <<EOF >\"${out}\"\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(29..45, "@description a")],
                associations: vec![owned(0, 46..56, top(0..57))],
                ..View::default()
            },
            owners: vec![46..56],
            ..Case::default()
        },
        Case {
            name: "v18_comment_in_tail",
            source: b"cat <<EOF # it's \\\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(28..44, "@description a")],
                associations: vec![owned(0, 45..55, top(0..56))],
                ..View::default()
            },
            unbound: vec![(Unattached, 10..18)],
            both: vec![(Unattached, 10..18)],
            owners: vec![45..55],
            ..Case::default()
        },
        Case {
            name: "v19_escaped_byte_in_tail",
            source: b"cat <<EOF a\\ b\\|\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(26..42, "@description a")],
                associations: vec![owned(0, 43..53, top(0..54))],
                ..View::default()
            },
            owners: vec![43..53],
            ..Case::default()
        },
        Case {
            name: "v20_list_on_the_row",
            source: b"cat <<EOF && true\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(27..43, "@description a")],
                associations: vec![owned(0, 44..54, top(0..55))],
                ..View::default()
            },
            owners: vec![44..54],
            ..Case::default()
        },
        Case {
            name: "v21_operator_in_the_text",
            source: b"cat <<EOF\n$(cat <<X\n X\nX\n)\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(31..47, "@description a")],
                associations: vec![owned(0, 48..58, top(0..59))],
                ..View::default()
            },
            owners: vec![48..58],
            ..Case::default()
        },
        Case {
            name: "w01_indented_end",
            source: b"# @description a\nf() { :; }\ncat <<EOF\n EOF\n# @description b\n\
                g() { :; }\nEOF\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![owned(0, 17..27, top(0..75))],
                ..View::default()
            },
            owners: vec![17..27],
            strangers: vec![60..70],
            ..Case::default()
        },
        Case {
            name: "w07_dollar",
            source: b"cat <<$x\nbody\n$x\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(17..33, "@description a")],
                associations: vec![owned(0, 34..44, top(0..45))],
                ..View::default()
            },
            owners: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "w08_inner_backslash",
            source: b"cat <<E\\OF\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(20..36, "@description a")],
                associations: vec![owned(0, 37..47, top(0..48))],
                ..View::default()
            },
            owners: vec![37..47],
            ..Case::default()
        },
        Case {
            name: "w09_continued_row",
            source: b"cat <<EOF \\\n -n\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(25..41, "@description a")],
                associations: vec![owned(0, 42..52, top(0..53))],
                ..View::default()
            },
            owners: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "w11_earlier_delimiter_row",
            source: b"cat <<EOF\n$(\nEOF\n)\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(23..39, "@description a")],
                associations: vec![owned(0, 40..50, top(0..51))],
                ..View::default()
            },
            owners: vec![40..50],
            ..Case::default()
        },
        Case {
            name: "w14_substitution_in_tail",
            source: b"cat <<EOF >\"$(echo out)\"\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(34..50, "@description a")],
                associations: vec![owned(0, 51..61, top(0..62))],
                ..View::default()
            },
            owners: vec![51..61],
            ..Case::default()
        },
        Case {
            name: "w15_back_quote_in_tail",
            source: b"cat <<EOF >`echo out`\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(31..47, "@description a")],
                associations: vec![owned(0, 48..58, top(0..59))],
                ..View::default()
            },
            owners: vec![48..58],
            ..Case::default()
        },
        Case {
            name: "w16_ansi_string_in_tail",
            source: b"cat <<EOF | tr $'\\t' x\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(32..48, "@description a")],
                associations: vec![owned(0, 49..59, top(0..60))],
                ..View::default()
            },
            owners: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "w17_expansion_in_tail",
            source: b"cat <<EOF >\"${out:-x}\"\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(32..48, "@description a")],
                associations: vec![owned(0, 49..59, top(0..60))],
                ..View::default()
            },
            owners: vec![49..59],
            ..Case::default()
        },
        Case {
            name: "w18_subshell_in_tail",
            source: b"cat <<EOF | (cat)\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(27..43, "@description a")],
                associations: vec![owned(0, 44..54, top(0..55))],
                ..View::default()
            },
            owners: vec![44..54],
            ..Case::default()
        },
        Case {
            name: "w19_or_ends_the_row",
            source: b"cat <<EOF ||\nbody\nEOF\ntrue\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(27..43, "@description a")],
                associations: vec![owned(0, 44..54, top(0..55))],
                ..View::default()
            },
            owners: vec![44..54],
            ..Case::default()
        },
        Case {
            name: "w20_comment_after_a_pipe",
            source: b"cat <<EOF | # c\nbody\nEOF\ncat\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(29..45, "@description a")],
                associations: vec![owned(0, 46..56, top(0..57))],
                ..View::default()
            },
            unbound: vec![(Unattached, 12..15)],
            both: vec![(Unattached, 12..15)],
            owners: vec![46..56],
            ..Case::default()
        },
        Case {
            name: "w21_hash_in_a_word",
            source: b"cat <<EOF a#b \\\n -n\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(29..45, "@description a")],
                associations: vec![owned(0, 46..56, top(0..57))],
                ..View::default()
            },
            owners: vec![46..56],
            ..Case::default()
        },
        Case {
            name: "w24_directive_row_first",
            source: b"cat <<EOF\n EOF\nEOF\n# shellcheck disable=SC2034\n# @description a\n\
                f() { :; }\n",
            view: View {
                documents: vec![row(47..63, "@description a")],
                associations: vec![owned(0, 64..74, top(0..75))],
                ..View::default()
            },
            directives: vec![(Directive, 19..46)],
            both: vec![(Directive, 19..46)],
            owners: vec![64..74],
            ..Case::default()
        },
    ] {
        let last = case.view.associations.last().unwrap();
        assert_eq!(last.resolution, Resolution::Resolved, "{}", case.name);
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_forms_on_one_row_and_back_quotes_are_read() {
    for case in [
        Case {
            name: "m01_substitutions_on_a_row",
            source: b"x=$(echo a) y=<(b) z=>(c)\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(26..42, "@description a")],
                associations: vec![owned(0, 43..53, top(0..54))],
                ..View::default()
            },
            owners: vec![43..53],
            ..Case::default()
        },
        Case {
            name: "m02_expansions_on_a_row",
            source: b"x=${y:-z} w=$(( 1 << 2 )) v=$[ 1 + 2 ]\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(39..55, "@description a")],
                associations: vec![owned(0, 56..66, top(0..67))],
                ..View::default()
            },
            owners: vec![56..66],
            ..Case::default()
        },
        Case {
            name: "m03_arithmetic_command_on_a_row",
            source: b"(( i++ ))\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(10..26, "@description a")],
                associations: vec![owned(0, 27..37, top(0..38))],
                ..View::default()
            },
            owners: vec![27..37],
            ..Case::default()
        },
        Case {
            name: "m04_subscript_on_a_row",
            source: b"a[i + 1]=x\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(11..27, "@description a")],
                associations: vec![owned(0, 28..38, top(0..39))],
                ..View::default()
            },
            owners: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m05_pattern_word_on_a_row",
            source: b"[[ $x =~ ^(a|b)$ ]] && :\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(25..41, "@description a")],
                associations: vec![owned(0, 42..52, top(0..53))],
                ..View::default()
            },
            owners: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "m06_extended_pattern_on_a_row",
            source: b"[[ $x == @(a|b) ]] && :\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(24..40, "@description a")],
                associations: vec![owned(0, 41..51, top(0..52))],
                ..View::default()
            },
            owners: vec![41..51],
            ..Case::default()
        },
        Case {
            name: "m07_back_quotes_on_a_row",
            source: b"x=`echo a`\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(11..27, "@description a")],
                associations: vec![owned(0, 28..38, top(0..39))],
                ..View::default()
            },
            owners: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m08_compound_assignment",
            source: b"arr=( [0]=a [k 1]=b )\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(22..38, "@description a")],
                associations: vec![owned(0, 39..49, top(0..50))],
                ..View::default()
            },
            owners: vec![39..49],
            ..Case::default()
        },
        Case {
            name: "m09_single_quote_in_braces",
            source: b"x=${y:-'a}b'}\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(14..30, "@description a")],
                associations: vec![owned(0, 31..41, top(0..42))],
                ..View::default()
            },
            owners: vec![31..41],
            ..Case::default()
        },
        Case {
            name: "m21_back_quotes_over_rows",
            source: b"x=`\necho\n`\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(11..27, "@description a")],
                associations: vec![owned(0, 28..38, top(0..39))],
                ..View::default()
            },
            owners: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m22_back_quotes_in_a_string",
            source: b"x=\"a `\necho \\\"\n` b\"\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(20..36, "@description a")],
                associations: vec![owned(0, 37..47, top(0..48))],
                ..View::default()
            },
            owners: vec![37..47],
            ..Case::default()
        },
        Case {
            name: "m23_plain_quote_in_quoted_braces",
            source: b"x=\"${y:-':'}\"\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(14..30, "@description a")],
                associations: vec![owned(0, 31..41, top(0..42))],
                ..View::default()
            },
            owners: vec![31..41],
            ..Case::default()
        },
    ] {
        assert_eq!(case.view.associations[0].resolution, Resolution::Resolved);
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_run_behind_the_place_where_the_scan_stops_is_withheld() {
    for case in [
        Case {
            name: "l02_subscript_plain_first",
            source: b"a[\n# note\n# @description LIT\n1]=x\n",
            view: View {
                assessments: unordered([withheld(3..9)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "l03_arithmetic_plain_first",
            source: b"echo $(( 1 +\n# note\n# @description LIT\n2 ))\n",
            view: View {
                assessments: unordered([withheld(13..19)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "l04_arithmetic_command",
            source: b"(( x = 1 +\n# note\n# @description LIT\n2 ))\n",
            view: View {
                assessments: unordered([withheld(11..17)]),
                ..View::default()
            },
            ..Case::default()
        },
        Case {
            name: "m11_command_substitution_over_rows",
            source: b"x=$(\necho\n)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(29..39)), withheld(12..28)]),
                ..View::default()
            },
            pending: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "m12_process_substitution_over_rows",
            source: b"cat <(\necho\n)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(31..41)), withheld(14..30)]),
                ..View::default()
            },
            pending: vec![31..41],
            ..Case::default()
        },
        Case {
            name: "m13_output_substitution_over_rows",
            source: b"echo a > >(\ncat\n)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(35..45)), withheld(18..34)]),
                ..View::default()
            },
            pending: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "m14_arithmetic_command_over_rows",
            source: b"(( x = 1 +\n2 ))\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(33..43)), withheld(16..32)]),
                ..View::default()
            },
            pending: vec![33..43],
            ..Case::default()
        },
        Case {
            name: "m15_arithmetic_expansion_over_rows",
            source: b"x=$(( 1 +\n2 ))\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(32..42)), withheld(15..31)]),
                ..View::default()
            },
            pending: vec![32..42],
            ..Case::default()
        },
        Case {
            name: "m16_braces_over_rows",
            source: b"x=${y:-\nz}\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(28..38)), withheld(11..27)]),
                ..View::default()
            },
            pending: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m17_dollar_bracket_over_rows",
            source: b"x=$[ 1 +\n2 ]\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(30..40)), withheld(13..29)]),
                ..View::default()
            },
            pending: vec![30..40],
            ..Case::default()
        },
        Case {
            name: "m18_subscript_over_rows",
            source: b"a[1 +\n2]=x\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(28..38)), withheld(11..27)]),
                ..View::default()
            },
            pending: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m19_pattern_word_over_rows",
            source: b"[[ a =~ (b\nc) ]]\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(34..44)), withheld(17..33)]),
                ..View::default()
            },
            pending: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "m31_comment_in_a_substitution",
            source: b"x=$(echo a # c)\n)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(35..45)), withheld(18..34)]),
                ..View::default()
            },
            pending: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "m32_here_document_in_a_substitution",
            source: b"x=$(cat <<EOF\nt\nEOF\n)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(39..49)), withheld(22..38)]),
                ..View::default()
            },
            pending: vec![39..49],
            ..Case::default()
        },
        Case {
            name: "m33_case_in_a_substitution",
            source: b"x=$(case a in a) echo;; esac)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(47..57)), withheld(30..46)]),
                ..View::default()
            },
            pending: vec![47..57],
            ..Case::default()
        },
        Case {
            name: "m34_shift_in_an_arithmetic_command",
            source: b"(( x = 1 << 2 ))\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(34..44)), withheld(17..33)]),
                ..View::default()
            },
            pending: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "m36_bar_in_a_pattern_word",
            source: b"[[ a =~ b|c ]]\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(32..42)), withheld(15..31)]),
                ..View::default()
            },
            pending: vec![32..42],
            ..Case::default()
        },
        Case {
            name: "m37_hash_behind_an_extended_pattern",
            source: b"!(true)#c\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(27..37)), withheld(10..26)]),
                ..View::default()
            },
            pending: vec![27..37],
            ..Case::default()
        },
        Case {
            name: "m38_brace_in_braces",
            source: b"x=${y:-{}}\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(28..38)), withheld(11..27)]),
                ..View::default()
            },
            pending: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "m42_quoted_close_in_quoted_braces",
            source: b"x=\"${y:-'}'}\"\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(31..41)), withheld(14..30)]),
                ..View::default()
            },
            pending: vec![31..41],
            ..Case::default()
        },
        Case {
            name: "m44_directive_row_opens_a_withheld_run",
            source: b"x=$(\n:\n)\n# shellcheck disable=SC2034\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(54..64)), withheld(9..36)]),
                ..View::default()
            },
            pending: vec![54..64],
            ..Case::default()
        },
        Case {
            name: "m45_unselected_run_behind_the_stop",
            source: b"x=$(\n:\n)\n# plain\n\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(35..45)), withheld(18..34)]),
                ..View::default()
            },
            pending: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "m39_hash_behind_a_compound_assignment",
            source: b"a=(b)#c\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(25..35)), withheld(8..24)]),
                ..View::default()
            },
            pending: vec![25..35],
            ..Case::default()
        },
        Case {
            name: "m40_delimiter_with_a_substitution",
            source: b"cat <<$(x)\nt\n$(x)\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(35..45)), withheld(18..34)]),
                ..View::default()
            },
            pending: vec![35..45],
            ..Case::default()
        },
        Case {
            name: "m41_nul_byte",
            source: b"echo a\x00b\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(26..36)), withheld(9..25)]),
                ..View::default()
            },
            pending: vec![26..36],
            ..Case::default()
        },
    ] {
        // Nothing behind the place is published or retained, and a function there
        // waits.
        assert_eq!(case.view.documents, Vec::new(), "{}", case.name);
        assert_eq!(
            case.view
                .assessments
                .iter()
                .filter(|entry| entry.reason == WITHHELD)
                .count(),
            1,
            "{}",
            case.name
        );
        assert_eq!(case.owners, Vec::<Range<usize>>::new());
        assert_eq!(
            [&case.unbound, &case.directives, &case.both],
            [&Kept::new(); 3]
        );
        check(&case);
    }
}

// The four inputs of finding F1. Bash reads the tag row and `f` as here-document text
// and defines `g` only. In the fourth the grammar reads the second operator as an
// error, which is a recovery node: its run is withheld.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_here_document_text_that_the_grammar_reads_as_code_gives_no_document() {
    for case in [
        Case {
            name: "q01_trailing_space",
            source: b"cat <<EOF\nEOF \n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            owners: vec![49..59],
            strangers: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "q02_indented",
            source: b"cat <<EOF\n EOF\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            owners: vec![49..59],
            strangers: vec![34..44],
            ..Case::default()
        },
        Case {
            name: "q03_dash_spaces",
            source: b"cat <<-EOF\n  EOF\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            owners: vec![51..61],
            strangers: vec![36..46],
            ..Case::default()
        },
        Case {
            name: "q04_two_operators",
            source: b"cat <<A <<B\n# @description LIT\nA\n# @description LIT\nf() { :; }\nB\n\
                g() { :; }\n",
            view: View {
                assessments: unordered([
                    recognized(function(52..62)),
                    recognized(function(65..75)),
                    withheld(33..51),
                ]),
                ..View::default()
            },
            pending: vec![52..62, 65..75],
            ..Case::default()
        },
    ] {
        assert_eq!(case.view.documents, Vec::new());
        assert_eq!(case.view.associations, Vec::new());
        assert_eq!(
            [&case.unbound, &case.directives, &case.both],
            [&Kept::new(); 3]
        );
        check(&case);
    }
}

// The 24 inputs of design review round 1, byte for byte. No row that Bash reads as
// text is a document, a candidate or the row of an association.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_inputs_of_review_round_one_publish_no_text() {
    let cases = [
        Case {
            name: "i01_astra1_cont_delim",
            source: b"cat <<EOF\nhello\\\nEOF\n# @description LIT\nf() { :; }\nEOF\n\
                # @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(55..74, "@description REAL")],
                associations: vec![owned(0, 75..85, top(0..86))],
                ..View::default()
            },
            owners: vec![75..85],
            strangers: vec![40..50],
            ..Case::default()
        },
        Case {
            name: "i02_astra1_cont_delim_dash",
            source: b"cat <<-EOF\n\thello\\\n\tEOF\n# @description LIT\nf() { :; }\n\tEOF\n\
                # @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(59..78, "@description REAL")],
                associations: vec![owned(0, 79..89, top(0..90))],
                ..View::default()
            },
            owners: vec![79..89],
            strangers: vec![43..53],
            ..Case::default()
        },
        Case {
            name: "i03_fable1_three_backslashes",
            source: b"cat <<EOF\nabc\\\\\\\nEOF\n# @description LIT\nf() { :; }\nEOF\n\
                g() { :; }\n",
            owners: vec![55..65],
            strangers: vec![40..50],
            ..Case::default()
        },
        Case {
            name: "i04_ctl_no_cont",
            source: b"cat <<EOF\nhello\nEOF\n# @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(20..39, "@description REAL")],
                associations: vec![owned(0, 40..50, top(0..51))],
                ..View::default()
            },
            owners: vec![40..50],
            ..Case::default()
        },
        Case {
            name: "i05_ctl_quoted",
            source: b"cat <<'EOF'\nhello\\\nEOF\n# @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(23..42, "@description REAL")],
                associations: vec![owned(0, 43..53, top(0..54))],
                ..View::default()
            },
            owners: vec![43..53],
            ..Case::default()
        },
        Case {
            name: "i06_ctl_two_backslashes",
            source: b"cat <<EOF\nhello\\\\\nEOF\n# @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(22..41, "@description REAL")],
                associations: vec![owned(0, 42..52, top(0..53))],
                ..View::default()
            },
            owners: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "i07_astra2_sub_nested",
            source: b"x=$(\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(36..50)),
                    recognized(function(5..52)),
                    withheld(15..35),
                ]),
                ..View::default()
            },
            pending: vec![5..52, 36..50],
            ..Case::default()
        },
        Case {
            name: "i08_astra2_sub_nested_backquote",
            source: b"x=`\nouter() {\n# @description INNER\ninner() { :; }\n}\n`\n",
            view: View {
                assessments: unordered([recognized(function(35..49)), recognized(function(4..51))]),
                ..View::default()
            },
            pending: vec![4..51, 35..49],
            ..Case::default()
        },
        Case {
            name: "i09_astra2_sub_nested_process",
            source: b"cat <(\nouter() {\n# @description INNER\ninner() { :; }\n}\n)\n",
            view: View {
                assessments: unordered([
                    recognized(function(38..52)),
                    recognized(function(7..54)),
                    withheld(17..37),
                ]),
                ..View::default()
            },
            pending: vec![7..54, 38..52],
            ..Case::default()
        },
        Case {
            name: "i10_ctl_nested_plain",
            source: b"outer() {\n# @description INNER\ninner() { :; }\n}\n",
            view: View {
                documents: vec![row(10..30, "@description INNER")],
                associations: vec![owned(0, 31..45, body(8..47))],
                ..View::default()
            },
            owners: vec![0..47, 31..45],
            ..Case::default()
        },
        Case {
            name: "i11_argon1_two_ops_pipe",
            source: b"cat <<A | cat <<B\nB\nA\n# @description LIT\nf() { :; }\nB\ng() { :; }\n",
            owners: vec![54..64],
            strangers: vec![41..51],
            ..Case::default()
        },
        Case {
            name: "i12_argon1_two_ops_and",
            source: b"cat <<A && cat <<B\nB\nA\n# @description LIT\nf() { :; }\nB\ng() { :; }\n",
            owners: vec![55..65],
            strangers: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "i13_argon1_two_ops_or",
            source: b"cat <<A || cat <<B\nB\nA\n# @description LIT\nf() { :; }\nB\ng() { :; }\n",
            owners: vec![55..65],
            strangers: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "i14_argon1_two_ops_pipe_amp",
            source: b"cat <<A |& cat <<B\nB\nA\n# @description LIT\nf() { :; }\nB\ng() { :; }\n",
            owners: vec![55..65],
            strangers: vec![42..52],
            ..Case::default()
        },
        Case {
            name: "i15_ctl_two_ops_in_order",
            source: b"cat <<A | cat <<B\nA\nB\n# @description REAL\ng() { :; }\n",
            ..Case::default()
        },
        Case {
            name: "i16_argon2_ansi_c",
            source: b": $'\\\\''\n# @description LIT\nf() { :; }\n' #'\ng() { :; }\n",
            owners: vec![44..54],
            strangers: vec![28..38],
            ..Case::default()
        },
        Case {
            name: "i17_argon2_backquote",
            source: b": `'`'`\n# @description LIT\nf() { :; }\n' #'\ng() { :; }\n",
            owners: vec![43..53],
            strangers: vec![27..37],
            ..Case::default()
        },
        Case {
            name: "i18_argon2_subscript_heredoc",
            source: b"cat <<EOF | a[b[0]=1+\nEOF\n]=1\n# @description LIT\nf() { :; }\nEOF\n\
                g() { :; }\n",
            view: View {
                assessments: unordered([
                    recognized(function(49..59)),
                    recognized(function(64..74)),
                    withheld(30..48),
                ]),
                ..View::default()
            },
            pending: vec![49..59, 64..74],
            ..Case::default()
        },
        Case {
            name: "i19_argon2_subscript",
            source: b"a[b[0]=1\n# @description LIT\nf() { :; }\n]=1\ng() { :; }\n",
            view: View {
                assessments: unordered([
                    recognized(function(28..38)),
                    recognized(function(43..53)),
                    withheld(9..27),
                ]),
                ..View::default()
            },
            pending: vec![28..38, 43..53],
            ..Case::default()
        },
        Case {
            name: "i20_argon2_r15_subscript",
            source: b"a[1\n# @description LIT\nf() { :; }\n]=1\ng() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(38..48)), withheld(4..22)]),
                ..View::default()
            },
            pending: vec![38..48],
            ..Case::default()
        },
        Case {
            name: "i21_argon2_r15_dollar_bracket",
            source: b"x=$[ 1\n# @description LIT\nf() { :; }\n]\ng() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(39..49)), withheld(7..25)]),
                ..View::default()
            },
            pending: vec![39..49],
            ..Case::default()
        },
        Case {
            name: "i22_ctl_ansi_c_closed",
            source: b": $'a\\'b'\n# @description REAL\ng() { :; }\n",
            view: View {
                documents: vec![row(10..29, "@description REAL")],
                associations: vec![owned(0, 30..40, top(0..41))],
                ..View::default()
            },
            owners: vec![30..40],
            ..Case::default()
        },
        Case {
            name: "i23_fable2_mirror",
            source: b"cat <<EOF\nE\\\nOF\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n",
            owners: vec![50..60],
            ..Case::default()
        },
        Case {
            name: "i24_fable3_let_shift",
            source: b"let x<<=2\n# @description REAL\ng() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(30..40)), withheld(10..29)]),
                ..View::default()
            },
            pending: vec![30..40],
            ..Case::default()
        },
    ];
    // expected: the 24 files of `parent-inputs/r1/inputs/`.
    assert_eq!(cases.len(), 24);
    for case in cases {
        for document in &case.view.documents {
            assert!(!document.text.contains("LIT"), "{}", case.name);
        }
        assert_eq!(
            [&case.unbound, &case.directives, &case.both],
            [&Kept::new(); 3]
        );
        check(&case);
    }
}

// `i15` and `i23` pin an outcome and no decision. Bash ends a here-document where the
// grammar does not: the rows behind it are commands for Bash and text for the grammar,
// which gives no node there. The unit has nothing to read and says nothing.
#[cfg(feature = "bash")]
#[test]
fn bash_text_that_only_the_grammar_reads_gives_no_node_and_no_assessment() {
    for (source, functions) in [
        // The row `E\`, joined to `OF`, is the delimiter for Bash.
        (
            &b"cat <<EOF\nE\\\nOF\n# @description LIT\nf() { :; }\nEOF\ng() { :; }\n"[..],
            1,
        ),
        // Two operators, texts in the order of the operators.
        (
            b"cat <<A | cat <<B\nA\nB\n# @description REAL\ng() { :; }\n",
            0,
        ),
    ] {
        let parser = parsed(source);
        assert!(!nodes_of(&parser, "heredoc_body").is_empty());
        assert_eq!(nodes_of(&parser, "comment").len(), 0);
        assert_eq!(nodes_of(&parser, "function_definition").len(), functions);
        for (unbound, directives) in [(false, false), (true, true)] {
            let view = agreed(&parser, options(unbound, directives));
            assert_eq!(view, View::default(), "{}", source.escape_ascii());
        }
    }
}

// A comment node inside back quotes or inside the text of a here-document is no row:
// no document, no assessment and no candidate under any retention option. The second
// source of each pair is the control: the same row behind the form is a document.
#[cfg(feature = "bash")]
#[test]
fn bash_rows_inside_a_form_or_a_text_give_no_document_and_no_assessment() {
    for (inside, outside) in [
        (
            &b"x=`\n# @file a\n:\n`\n"[..],
            &b"x=`\n:\n`\n# @file a\n:\n"[..],
        ),
        (
            b"cat <<EOF\n$(\n# @file a\n:\n)\nEOF\n",
            b"cat <<EOF\n$(\n:\n)\nEOF\n# @file a\n:\n",
        ),
    ] {
        for source in [inside, outside] {
            let parser = parsed(source);
            assert!(!parser.ts_tree().root_node().has_error());
            // expected: the row is a comment node of the grammar in both sources.
            assert_eq!(nodes_of(&parser, "comment").len(), 1);
        }
        let parser = parsed(inside);
        for (unbound, directives) in [(false, false), (true, false), (false, true), (true, true)] {
            let view = agreed(&parser, options(unbound, directives));
            assert_eq!(view, View::default(), "{}", inside.escape_ascii());
        }
        let view = agreed(&parsed(outside), options(false, false));
        assert_eq!(view.documents.len(), 1, "{}", outside.escape_ascii());
        assert_eq!(view.documents[0].text, "@file a");
    }
}

// `n01` pins an outcome that silently leaves out a row Bash reads as a comment: the
// unit does not split a run in which the grammar and Bash disagree.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_run_with_a_node_that_is_no_comment_for_bash_is_not_read() {
    for case in [
        Case {
            name: "c01_joined_word",
            source: b"echo a\\\n# @description c\nf() { :; }\n",
            owners: vec![25..35],
            ..Case::default()
        },
        Case {
            name: "c03_escaped_blank_in_run",
            source: b"# @description a\n\\ # b\nf() { :; }\n",
            owners: vec![23..33],
            ..Case::default()
        },
        Case {
            name: "c05_form_feed",
            source: b"# @description a\n\x0c# @arg $1 x\nf() { :; }\n",
            owners: vec![30..40],
            ..Case::default()
        },
        Case {
            name: "c09_escaped_blank_at_run",
            source: b"echo a\n\\ # @description c\nf() { :; }\n",
            owners: vec![26..36],
            ..Case::default()
        },
        Case {
            name: "n01_text_row_opens_the_run",
            source: b": $'\\\\''\n# @description LIT'\n# @arg $1 x\nf() { :; }\n",
            owners: vec![41..51],
            ..Case::default()
        },
        Case {
            name: "n02_text_behind_code_above_the_run",
            source: b": $'\\\\'' # c'\n# @description a\nf() { :; }\n",
            view: View {
                documents: vec![row(14..30, "@description a")],
                associations: vec![owned(0, 31..41, top(0..42))],
                ..View::default()
            },
            owners: vec![31..41],
            ..Case::default()
        },
    ] {
        assert_eq!(
            [&case.unbound, &case.directives, &case.both],
            [&Kept::new(); 3]
        );
        assert_eq!(case.view.assessments, Vec::new());
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_function_whose_end_is_not_confirmed_is_a_candidate() {
    // In `w22` and `n03` Bash ends the function later than the grammar: inside the
    // text of a here-document and inside a quoted string. In `n05` the scan stops
    // inside the function.
    for case in [
        Case {
            name: "w22_function_holds_the_operator",
            source: b"# @description a\nf() {\ncat <<EOF\n EOF\n}\n# @description b\n\
                g() { :; }\nEOF\n}\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![candidate(0, 17..39, top(0..74))],
                ..View::default()
            },
            owners: vec![17..39],
            strangers: vec![57..67],
            ..Case::default()
        },
        Case {
            name: "n03_function_ends_in_a_string",
            source: b"# @description a\nf() { : $'\\\\''; }\n: '; } #'\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![candidate(0, 17..34, top(0..45))],
                ..View::default()
            },
            owners: vec![17..34],
            ..Case::default()
        },
        Case {
            name: "n05_function_holds_the_stop",
            source: b"# @description a\nf() {\nx=$(\necho\n)\n}\n# @description b\ng() { :; }\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![candidate(0, 17..36, top(0..65))],
                assessments: unordered([recognized(function(54..64)), withheld(37..53)]),
                ..View::default()
            },
            owners: vec![17..36],
            pending: vec![54..64],
            ..Case::default()
        },
    ] {
        assert_eq!(
            case.view.associations[0].resolution,
            Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
        );
        assert_eq!(case.view.associations[0].owners.len(), 1);
        check(&case);
    }
}

// `c02` and `c06` pin an outcome that the decision on continuation rows may change.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_rows_that_bash_joins_to_code_open_no_run() {
    for case in [
        Case {
            name: "c02_joined_row",
            source: b"echo a \\\n# @description c\nf() { :; }\n",
            unbound: vec![(Unattached, 9..25)],
            both: vec![(Unattached, 9..25)],
            owners: vec![26..36],
            ..Case::default()
        },
        Case {
            name: "c04_escaped_blank_at_function",
            source: b"# @description a\n\\ f() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(19..29))]),
                ..View::default()
            },
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            pending: vec![19..29],
            ..Case::default()
        },
        Case {
            name: "c06_bare_row_below",
            source: b"# @description a\n\\\nf() { :; }\n",
            unbound: vec![(Unattached, 0..16)],
            both: vec![(Unattached, 0..16)],
            owners: vec![19..29],
            ..Case::default()
        },
    ] {
        assert_eq!(case.view.documents, Vec::new());
        assert_ne!(case.unbound, Kept::new());
        check(&case);
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_bare_continuation_rows_above_a_run_do_not_detach_it() {
    for case in [
        Case {
            name: "c07_bare_row_first",
            source: b"\\\n# @description c\nf() { :; }\n",
            view: View {
                documents: vec![row(2..18, "@description c")],
                associations: vec![owned(0, 19..29, top(2..30))],
                ..View::default()
            },
            owners: vec![19..29],
            ..Case::default()
        },
        Case {
            name: "c08_bare_row_above",
            source: b"echo a\n\\\n# @description c\nf() { :; }\n",
            view: View {
                documents: vec![row(9..25, "@description c")],
                associations: vec![owned(0, 26..36, top(0..37))],
                ..View::default()
            },
            owners: vec![26..36],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

// `e04` is valid Bash that the grammar cannot read; `e05` is valid Bash whose root
// node is an `ERROR`; in `e06` the recovery node is a missing node. In `w05`, `w06`,
// `w10` and `w12` the recovery node stands at a here-document operator.
#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_recovery_withholds_what_follows_and_keeps_what_stands_before() {
    for case in [
        Case {
            name: "w05_two_operators",
            source: b"cat <<A <<B\nx\nA\n# @description a\nf() { :; }\nB\n",
            view: View {
                assessments: unordered([recognized(function(33..43)), withheld(16..32)]),
                ..View::default()
            },
            pending: vec![33..43],
            ..Case::default()
        },
        Case {
            name: "w06_background",
            source: b"cat <<EOF &\n# @description a\nf() { :; }\nEOF\n",
            view: View {
                assessments: unordered([recognized(function(29..39)), withheld(12..28)]),
                ..View::default()
            },
            pending: vec![29..39],
            ..Case::default()
        },
        Case {
            name: "w10_empty_word",
            source: b"cat <<\"\"\n# @description a\nf() { :; }\n\n",
            view: View {
                assessments: unordered([recognized(function(26..36)), withheld(9..25)]),
                ..View::default()
            },
            pending: vec![26..36],
            ..Case::default()
        },
        Case {
            name: "w12_semicolon",
            source: b"cat <<EOF ;\nbody\nEOF\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(38..48)), withheld(21..37)]),
                ..View::default()
            },
            pending: vec![38..48],
            ..Case::default()
        },
        Case {
            name: "e02_inside_an_error",
            source: b"f() {\nouter() {\n  # @description i\n  inner() { :; }\n}\n",
            view: View {
                assessments: unordered([
                    recognized(function(37..51)),
                    recognized(function(6..53)),
                    withheld(18..34),
                ]),
                ..View::default()
            },
            pending: vec![6..53, 37..51],
            ..Case::default()
        },
        Case {
            name: "e03_error_beside",
            source: b"# @description a\nf() { :; } )\n",
            view: View {
                documents: vec![row(0..16, "@description a")],
                associations: vec![owned(0, 17..27, top(0..30))],
                ..View::default()
            },
            owners: vec![17..27],
            ..Case::default()
        },
        Case {
            name: "e04_after_a_recovery_node",
            source: b"! ( : <>/dev/tty ) 2>/dev/null\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(48..58)), withheld(31..47)]),
                ..View::default()
            },
            pending: vec![48..58],
            ..Case::default()
        },
        Case {
            name: "e06_after_a_missing_node",
            source: b"( )\n# @description a\nf() { :; }\n",
            view: View {
                assessments: unordered([recognized(function(21..31)), withheld(4..20)]),
                ..View::default()
            },
            pending: vec![21..31],
            ..Case::default()
        },
        Case {
            name: "e05_root_error",
            source: b"{\nfor r in a; do for e in \"b\"; do\nx=\"`echo \"\"`\"\ndone\n\
                if \"c\"; then y=\"`echo \"\"`\"\nfi\ndone\nif ]; then \"\"\nfi\n\
                z=\"`echo \"\"`\"\nif \"c\"; then \"\"\nfi\n}\n{ \"\"\n}{\"\"\n}\nc() {\n\
                if \"\" & (]) then g\nfi\n}\n# @description d\nf() if ]; then \"\" | '' | t\nfi\n\
                [x \"\"\n",
            view: View {
                assessments: unordered([
                    recognized(function(152..181)),
                    recognized(function(199..228)),
                    withheld(182..198),
                ]),
                ..View::default()
            },
            pending: vec![152..181, 199..228],
            ..Case::default()
        },
    ] {
        check(&case);
    }
}

// This test pins today's outcome and no decision. Bash reads a CR as a byte of the
// word before it: for Bash the row below the comment is no function definition, and
// `bash -n` reports a syntax error for these bytes. The grammar ends the function
// before the CR. `Ast::from_path` replaces CRLF by LF before the parse, so only a
// caller that parses bytes of its own reaches this outcome. The design review decides
// the point by name (the limit `crlf`).
#[cfg(feature = "bash")]
#[test]
fn bash_crlf_rows_give_an_owner_that_bash_does_not_read() {
    let [case] = [Case {
        name: "g01_crlf",
        source: b"# @description a\r\nf() { :; }\r\n",
        view: View {
            documents: vec![row(0..16, "@description a")],
            associations: vec![owned(0, 18..28, top(0..30))],
            ..View::default()
        },
        owners: vec![18..28],
        ..Case::default()
    }];
    assert_eq!(case.source.iter().filter(|byte| **byte == b'\r').count(), 2);
    assert_eq!(case.view.associations[0].resolution, Resolution::Resolved);
    check(&case);
}

/// A parse with included ranges, for the second entry point only.
#[cfg(feature = "bash")]
fn restricted(source: &[u8], ranges: &[Range<usize>]) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let before = &source[..offset];
        tree_sitter::Point::new(
            before.iter().filter(|byte| **byte == b'\n').count(),
            before
                .iter()
                .rev()
                .take_while(|byte| **byte != b'\n')
                .count(),
        )
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
        .set_language(&LANG::Bash.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

/// One source parsed with included ranges: what the second entry point publishes, the
/// candidates `retain_unbound` adds, and what the node queries answer for its
/// functions. `joined`: with every byte included Bash joins the tag row to the row
/// above; without it the control is a document of the function.
#[cfg(feature = "bash")]
#[derive(Default)]
struct Restricted {
    name: &'static str,
    source: &'static [u8],
    included: &'static [Range<usize>],
    view: View,
    unbound: Kept,
    owners: Vec<Range<usize>>,
    pending: Vec<Range<usize>>,
    joined: bool,
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn bash_excluded_ranges_are_barriers() {
    use crate::lang_helpers::shdoc::scan::{Place, Scan};
    let cases = [
        Restricted {
            name: "x01_hidden_row",
            source: b"# @description audit\n# hidden\nf() { :; }\n",
            included: &[0..21, 30..41],
            unbound: vec![(Unattached, 0..20)],
            view: View {
                assessments: unordered([recognized(function(30..40))]),
                ..View::default()
            },
            pending: vec![30..40],
            ..Restricted::default()
        },
        Restricted {
            name: "x02_gap_above",
            source: b"echo a\n\n# @description c\nf() { :; }\n",
            included: &[0..7, 8..36],
            unbound: vec![(Unattached, 8..24)],
            owners: vec![25..35],
            ..Restricted::default()
        },
        Restricted {
            name: "x03_gap_in_heredoc",
            source: b"cat <<EOF\nbody\nEOF\necho ok\n# @description a\nf() { :; }\n",
            included: &[0..11, 12..55],
            view: View {
                assessments: unordered([
                    recognized(function(44..54)),
                    recognized(("heredoc_redirect", 4..18)),
                    recognized(("redirected_statement", 0..18)),
                    withheld(27..43),
                ]),
                ..View::default()
            },
            pending: vec![44..54],
            ..Restricted::default()
        },
        Restricted {
            name: "x04_gap_in_a_string",
            source: b"x=\"a\nb\"\n# @description a\nf() { :; }\n",
            included: &[0..5, 6..36],
            view: View {
                assessments: unordered([
                    recognized(function(25..35)),
                    recognized(("string", 2..7)),
                    recognized(("variable_assignment", 0..7)),
                    withheld(8..24),
                ]),
                ..View::default()
            },
            pending: vec![25..35],
            ..Restricted::default()
        },
        Restricted {
            name: "x05_gap_behind_an_operator",
            source: b"cat <<EOF x\nbody\nEOF\n# @description a\nf() { :; }\n",
            included: &[0..10, 11..49],
            view: View {
                assessments: unordered([
                    recognized(function(38..48)),
                    recognized(("heredoc_redirect", 4..20)),
                    recognized(("redirected_statement", 0..20)),
                    withheld(21..37),
                ]),
                ..View::default()
            },
            pending: vec![38..48],
            ..Restricted::default()
        },
        Restricted {
            name: "x06_gap_in_a_form",
            source: b"x=$(echo a)\n# @description a\nf() { :; }\n",
            included: &[0..8, 9..40],
            view: View {
                assessments: unordered([
                    recognized(("command", 4..10)),
                    recognized(("command_name", 4..10)),
                    recognized(("command_substitution", 2..11)),
                    recognized(function(29..39)),
                    recognized(("variable_assignment", 0..11)),
                    recognized(("word", 4..10)),
                    withheld(12..28),
                ]),
                ..View::default()
            },
            pending: vec![29..39],
            ..Restricted::default()
        },
        Restricted {
            name: "x07_gap_in_a_command",
            source: b"echo a b\n:\n# @description a\nf() { :; }\n",
            included: &[0..5, 6..39],
            view: View {
                documents: vec![row(11..27, "@description a")],
                associations: vec![owned(0, 28..38, top(0..39))],
                assessments: unordered([recognized(("command", 0..8))]),
                ..View::default()
            },
            owners: vec![28..38],
            ..Restricted::default()
        },
        Restricted {
            name: "x08_gap_before_a_joined_row",
            source: b"echo a \\\n# @description a\nf() { :; }\n",
            included: &[0..5, 6..37],
            unbound: vec![(Unattached, 9..25)],
            owners: vec![26..36],
            joined: true,
            ..Restricted::default()
        },
        Restricted {
            name: "x09_gap_before_backslashes",
            source: b"echo ab\\\\\n# @description a\nf() { :; }\n",
            included: &[0..6, 7..38],
            view: View {
                assessments: unordered([
                    recognized(("command", 0..9)),
                    recognized(("word", 5..9)),
                    recognized(function(27..37)),
                    withheld(10..26),
                ]),
                ..View::default()
            },
            pending: vec![27..37],
            ..Restricted::default()
        },
        Restricted {
            name: "x10_post_gap_opens_a_string",
            source: b"a\nb\nx=$'\\\\''\n# @description d\nf() { :; }\n' #'\n",
            included: &[0..2, 4..46],
            view: View {
                assessments: unordered([recognized(function(30..40)), withheld(13..29)]),
                ..View::default()
            },
            pending: vec![30..40],
            ..Restricted::default()
        },
        Restricted {
            name: "x11_post_gap_opens_a_text",
            source: b"a\nb\ncat <<EOF\nEOF \n# @description d\nf() { :; }\nEOF\n",
            included: &[0..2, 4..51],
            view: View {
                assessments: unordered([recognized(function(36..46)), withheld(19..35)]),
                ..View::default()
            },
            pending: vec![36..46],
            ..Restricted::default()
        },
        Restricted {
            name: "x12_leading_gap_opens_a_text",
            source: b"cat <<EOF\nEOF \n# @description d\nf() { :; }\nEOF\n",
            included: &[1..47],
            view: View {
                assessments: unordered([recognized(function(32..42)), withheld(15..31)]),
                ..View::default()
            },
            pending: vec![32..42],
            ..Restricted::default()
        },
        Restricted {
            name: "x13_post_gap_command_control",
            source: b"a\nb\n:\nx=1\n# @description d\nf() { :; }\n",
            included: &[0..2, 4..38],
            view: View {
                documents: vec![row(10..26, "@description d")],
                associations: vec![owned(0, 27..37, top(0..38))],
                ..View::default()
            },
            owners: vec![27..37],
            ..Restricted::default()
        },
        Restricted {
            name: "x14_post_gap_non_comment",
            source: b"echo a b\n:\n\x0c# @description c\nf() { :; }\n",
            included: &[0..5, 6..40],
            view: View {
                assessments: unordered([recognized(("command", 0..8))]),
                ..View::default()
            },
            owners: vec![29..39],
            ..Restricted::default()
        },
        Restricted {
            name: "x17_post_gap_one_row_form",
            source: b"a\nXx=$( :)\n# @description d\nf() { :; }\n",
            included: &[0..2, 3..39],
            view: View {
                assessments: unordered([recognized(function(28..38)), withheld(11..27)]),
                ..View::default()
            },
            pending: vec![28..38],
            ..Restricted::default()
        },
    ];
    for case in cases {
        let name = case.name;
        // The control: with every byte included the run is the function's document,
        // unless Bash joins its row to the row above.
        let whole = agreed(&parsed(case.source), options(false, false));
        let resolved: Vec<_> = whole
            .associations
            .iter()
            .map(|edge| edge.resolution)
            .collect();
        if case.joined
            || matches!(
                name,
                "x10_post_gap_opens_a_string"
                    | "x11_post_gap_opens_a_text"
                    | "x12_leading_gap_opens_a_text"
                    | "x14_post_gap_non_comment"
            )
        {
            assert_eq!(whole, View::default(), "{name}");
        } else {
            assert_eq!(resolved, [Resolution::Resolved], "{name}");
        }
        let tree = restricted(case.source, case.included);
        assert!(!tree.root_node().has_error(), "{name}");
        let context = DocumentationContext::default();
        for (retain, kept) in [(false, Kept::new()), (true, case.unbound)] {
            let index = collect(
                LANG::Bash,
                &tree,
                case.source,
                &context,
                &options(retain, false),
            );
            let expected = View {
                evidence: kept,
                ..case.view.clone()
            };
            assert_eq!(published(&index.unwrap()), expected, "{name}");
        }
        let index = collect(
            LANG::Bash,
            &tree,
            case.source,
            &context,
            &options(false, false),
        )
        .unwrap();
        let mut listed = Vec::new();
        for function in function_nodes(tree.root_node()) {
            let range = function.byte_range();
            if case.owners.contains(&range) {
                assert_eq!(index.node_owners(function).unwrap().count(), 1, "{name}");
            } else {
                assert!(case.pending.contains(&range), "{name} {range:?}");
                assert!(matches!(
                    index.node_owners(function).map(Iterator::count),
                    Err(OwnerError::PendingOwner)
                ));
            }
            listed.push(range);
        }
        assert_eq!(
            listed.len(),
            case.owners.len() + case.pending.len(),
            "{name}"
        );
    }
    // x15/x16: when the excluded byte is itself an opener, a later
    // command-level feed cannot prove the state that Bash would have read.
    for (name, source, hidden) in [
        (
            "x15_hidden_single_quote",
            b"a\nx='foo\n# @description d\nf() { :; }\n'\n".as_slice(),
            4,
        ),
        (
            "x16_hidden_heredoc_opener",
            b"a\ncat <<EOF\n# @description d\nf() { :; }\nEOF\n".as_slice(),
            6,
        ),
    ] {
        let tag = source
            .windows(14)
            .position(|part| part == b"# @description")
            .unwrap();
        let budget = Counting::default();
        let mut whole = Scan::new(source, vec![0..source.len()]);
        whole.read(tag, &budget).unwrap();
        assert_eq!(whole.place(tag), Place::Text, "{name}");
        let mut restricted = Scan::new(source, vec![0..hidden, hidden + 1..source.len()]);
        restricted.read(tag, &budget).unwrap();
        assert!(
            matches!(restricted.place(tag), Place::Comment { .. }),
            "{name}"
        );
    }
    // The excluded prefix may itself have ended in a backslash. A line feed
    // with no included byte before it does not establish a fresh command line.
    let source = b"X\n# @description d\nf() { :; }\n";
    let mut scan = Scan::new(source, vec![1..source.len()]);
    scan.read(2, &Counting::default()).unwrap();
    assert_eq!(scan.place(2), Place::Unknown);
}

#[test]
fn bash_scan_reads_no_excluded_byte() {
    use crate::lang_helpers::shdoc::scan::{Place, Scan};
    // Byte 5 is excluded. Whatever it is (a letter, a quote, a backslash, `#`, a back
    // quote, a NUL byte), the scan gives the same answers, because it does not read it.
    let budget = Counting::default();
    for hidden in [b'a', b'\'', b'"', b'\\', b'#', b'`', 0] {
        let mut source = b"echo ab c\n# r\nx='\n# t\n'\n".to_vec();
        source[5] = hidden;
        let mut scan = Scan::new(&source, vec![0..5, 6..source.len()]);
        // The row behind the excluded byte is not read: its state is unknown.
        scan.advance(6, &budget).unwrap();
        assert_eq!(scan.place(6), Place::Unknown);
        // expected: the state is known again behind the line feed at byte 9, which
        // the bytes `b c` before it prove unescaped. The comment row at 10 opens its
        // line, `x=` at 14 starts a command, and the row at 18 is inside the string.
        scan.read(10, &budget).unwrap();
        assert_eq!(
            scan.place(10),
            Place::Comment {
                end: 13,
                line: true
            }
        );
        scan.advance(14, &budget).unwrap();
        assert_eq!(
            scan.place(14),
            Place::Code {
                word: true,
                line: true
            }
        );
        scan.read(18, &budget).unwrap();
        assert_eq!(scan.place(18), Place::Text);
        scan.advance(source.len(), &budget).unwrap();
        assert_eq!(scan.stopped(), None);
    }
    // expected: every byte but the excluded one is read once: 23 of 24, 7 times.
    assert_eq!(budget.charged_bytes.get(), 7 * 23);

    // Something is open at the excluded byte `hidden`: nobody knows what closes it,
    // and the scan stops there with the reason `ExcludedGap`.
    for (source, hidden, open) in [
        // expected: open are a string, a compound assignment, an operator that waits
        // for its word, a here-document that waits for its text, a form of one row,
        // and a delimiter word.
        (&b"x='a b'\n"[..], 4, true),
        (b"a=(b c)\n", 5, true),
        (b"cat << EOF\nEOF\n", 6, true),
        (b"cat <<EOF x\nEOF\n", 10, true),
        (b"x=$(a b)\n", 6, true),
        (b"cat <<EOF\nEOF\n", 8, true),
        // expected: nothing is open in a command, nor behind a string that is closed.
        (b"echo a b\n", 6, false),
        (b"echo 'a' b\n", 9, false),
    ] {
        let mut scan = Scan::new(source, vec![0..hidden, hidden + 1..source.len()]);
        scan.advance(source.len(), &Counting::default()).unwrap();
        let reason = open.then_some(UnresolvedReason::ExcludedGap);
        assert_eq!(scan.stopped(), reason, "{}", source.escape_ascii());
        let behind = scan.place(hidden) == Place::Behind(UnresolvedReason::ExcludedGap);
        assert_eq!(behind, open, "{}", source.escape_ascii());
    }
}

/// Counts the charges of a rule and can fail at one of them.
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    charged_bytes: Cell<usize>,
    charges: Cell<usize>,
    fail_at: Option<usize>,
}

impl Counting {
    fn charge(&self) -> Result<(), SourceError> {
        self.charges.set(self.charges.get() + 1);
        if self.fail_at == Some(self.charges.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl WorkBudget for Counting {
    type Error = SourceError;

    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }

    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        self.charge()
    }

    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.charged_bytes.set(self.charged_bytes.get() + count);
        self.charge()
    }
}

/// Runs `rule` with a counting budget, then once for every charge with a budget that
/// fails there, and returns the answer with the steps and bytes of the full run.
fn counted<T: std::fmt::Debug>(
    rule: impl Fn(&Counting) -> Result<T, SourceError>,
) -> (T, usize, usize) {
    let budget = Counting::default();
    let answer = rule(&budget).unwrap();
    for fail_at in 1..=budget.charges.get() {
        let failing = Counting {
            fail_at: Some(fail_at),
            ..Counting::default()
        };
        assert!(
            matches!(rule(&failing), Err(SourceError::Cancelled)),
            "charge {fail_at} of {}",
            budget.charges.get()
        );
        assert_eq!(failing.charges.get(), fail_at);
    }
    (answer, budget.steps.get(), budget.charged_bytes.get())
}

/// The comment and function nodes of `parser`, in source order.
#[cfg(feature = "bash")]
fn nodes_of<'a>(parser: &'a AnyParser, kind: &str) -> Vec<crate::Node<'a>> {
    parser
        .root_node()
        .preorder()
        .filter(|node| node.kind() == kind)
        .collect()
}

#[cfg(feature = "bash")]
#[test]
#[allow(clippy::too_many_lines)]
fn shdoc_rules_charge_the_budget_they_are_given() {
    use crate::documentation::recognition::{NativeRules, SourceRules};
    use crate::documentation::streaming::positional::{OpenerReason, RunContext};
    type Rules = NativeRules<crate::BashCode>;
    let chainless = crate::Ancestors::unknown();

    // The directive rule: the `#!` row at byte 0 is told without a scan; any other
    // row is read up to the byte after its blanks.
    for (source, at, directive, charged) in [
        (&b"#!/bin/sh\n# shellcheck x\n"[..], 0, true, 0),
        (b"#!/bin/sh\n# shellcheck x\n", 1, true, 2),
        (b"\n#!/bin/sh\n", 0, false, 1),
        (b"#  plain\n", 0, false, 3),
    ] {
        let parser = parsed(source);
        let comment = nodes_of(&parser, "comment")[at];
        let (found, steps, read) =
            counted(|budget| Rules::directive(&comment, parser.code(), budget));
        assert_eq!((found, steps, read), (directive, 0, charged));
    }

    // The target rule reads the kind of the node and nothing else: no charge, also
    // for a function that the grammar puts into a here-document redirect.
    for (source, kind, at, target) in [
        (&b"f() { :; }\n"[..], "function_definition", 0, true),
        (
            b"o() {\n  function i { :; }\n}\n",
            "function_definition",
            1,
            true,
        ),
        (
            b"cat <<EOT |\nf() { :; }\nEOT\ncat\n",
            "function_definition",
            0,
            true,
        ),
        (b"echo a\n", "command", 0, false),
    ] {
        let parser = parsed(source);
        let node = nodes_of(&parser, kind)[at];
        let (found, steps, read) =
            counted(|budget| Rules::target(&node, parser.code(), chainless, budget));
        assert_eq!((found, steps, read), (target, 0, 0));
    }

    // The opener rule, on a run of two rows under a command.
    let source = b"echo a\n# @description x\n# @arg $1 y\nf() { :; }\n";
    let parser = parsed(source);
    let context = SourceContext::new(parser.ts_tree(), source);
    let members = nodes_of(&parser, "comment");
    let below = nodes_of(&parser, "function_definition")[0];
    let run = RunContext {
        members: &members,
        below: Some(below),
        code: source,
        source: Some(&context),
    };
    let (found, steps, read) = counted(|budget| Rules::opener(&run, chainless, budget));
    assert_eq!(found, Ok(Some(OpenerReason::FollowingItem)));
    // expected: one step for each of the two rows; the rule reads no node but them.
    assert_eq!(steps, 2);
    // expected: 2 per row for the directive test and 4 for the `@arg` row (its blanks
    // twice, each with the byte that ends them).
    assert_eq!(read, 2 + 2 + 4);

    // A run that no tag opens stops after the first loop.
    let source = b"echo a\n# plain\nf() { :; }\n";
    let parser = parsed(source);
    let context = SourceContext::new(parser.ts_tree(), source);
    let members = nodes_of(&parser, "comment");
    let run = RunContext {
        members: &members,
        below: Some(nodes_of(&parser, "function_definition")[0]),
        code: source,
        source: Some(&context),
    };
    let (found, steps, read) = counted(|budget| Rules::opener(&run, chainless, budget));
    // expected: one row; 2 bytes for the directive test and 2 for the row class.
    assert_eq!((found, steps, read), (Ok(None), 1, 4));
}

/// The rows `# @file rN` of `source`.
fn probe_rows(source: &[u8]) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive(|byte| *byte == b'\n') {
        let row = line.strip_suffix(b"\n").unwrap_or(line);
        if row.starts_with(b"# @file r") {
            rows.push(start..start + row.len());
        }
        start += line.len();
    }
    rows
}

// Each source is one rule of the scan. The scan is asked, without a tree, what it
// reads at every row `# @file rN`: a comment that opens its line, a place behind
// the stop, or anything else. The expected rows are derived by `scan_rows.py` from
// `bash -n` and from the first bytes the rule says the scan does not read.
#[test]
#[allow(clippy::too_many_lines)]
fn bash_scan_reads_rows_as_bash_does() {
    use crate::lang_helpers::shdoc::scan::{Place, Scan};
    type Row = (
        &'static [u8],
        &'static [Range<usize>],
        &'static [Range<usize>],
    );
    let rows: [Row; 100] = [
        // R10.1a: a plain row.
        (b"echo a\n# @file r1\n", &[7..17], &[]),
        // R10.1a: a backslash takes the byte behind it.
        (b"echo \\'\n# @file r1\n", &[8..18], &[]),
        // R10.1a: a backslash before a blank makes `#` a byte of the word.
        (b"echo \\ #'\n# @file r1\n'\n# @file r2\n", &[23..33], &[]),
        // R10.1b: a backslash and line feed are no bytes: the word goes on.
        (b"echo a\\\n#'\n# @file r1\n'\n# @file r2\n", &[24..34], &[]),
        // R10.1b: behind a blank and a joined row a comment starts.
        (b"echo a \\\n#'\n# @file r1\n", &[12..22], &[]),
        // R10.1b: a row that holds only a backslash joins nothing to the row below.
        (b"\\\n# @file r1\n", &[2..12], &[]),
        // R10.1b: an operator may stand on both sides of a joined row.
        (
            b"cat <\\\n<EOF\n# @file r1\nEOF\n# @file r2\n",
            &[27..37],
            &[],
        ),
        // R10.1c: a single-quoted string over rows.
        (b"x='\n# @file r1\n'\n# @file r2\n", &[17..27], &[]),
        // R10.1c: a double-quoted string over rows; `\"` does not end it.
        (b"x=\"a\\\"\n# @file r1\n\"\n# @file r2\n", &[20..30], &[]),
        // R10.1c: `$'…'` over rows; `\'` does not end it.
        (b"x=$'a\\'\n# @file r1\n'\n# @file r2\n", &[21..31], &[]),
        // R10.1c: `$'\\'` ends at its second quote (finding F6).
        (
            b": $'\\\\''\n# @file r1\n' #'\n# @file r2\n",
            &[25..35],
            &[],
        ),
        // R10.1c: `$"…"` over rows.
        (b"x=$\"\n# @file r1\n\"\n# @file r2\n", &[18..28], &[]),
        // R10.1c: `$$` is a parameter: the quote behind it is a single quote.
        (b"echo $$'a\\'\n# @file r1\n", &[12..22], &[]),
        // R10.1c: in a double-quoted string `$'` opens nothing.
        (b"x=\"$'\"\n# @file r1\n", &[7..17], &[]),
        // R10.1d: back quotes over rows.
        (b"x=`\n# @file r1\n`\n# @file r2\n", &[17..27], &[]),
        // R10.1d: in back quotes a backslash takes the back quote.
        (b"x=`a\\`\n# @file r1\n`\n# @file r2\n", &[20..30], &[]),
        // R10.1d: back quotes that hold a quote (finding F6).
        (b": `'`'`\n# @file r1\n' #'\n# @file r2\n", &[24..34], &[]),
        // R10.1d: back quotes in a double-quoted string, over rows.
        (b"x=\"`\n\"\n# @file r1\n`\"\n# @file r2\n", &[21..31], &[]),
        // R10.1e: a comment hides a quote.
        (b"echo a #'\n# @file r1\n", &[10..20], &[]),
        // R10.1e: `#` inside a word is no comment.
        (b"echo a#'\n# @file r1\n'\n# @file r2\n", &[22..32], &[]),
        // R10.1e: `#` behind `;` is a comment.
        (b"echo a;#'\n# @file r1\n", &[10..20], &[]),
        // R10.1e: `$#` is a parameter.
        (b"echo $#'\n# @file r1\n'\n# @file r2\n", &[22..32], &[]),
        // R10.1e: a form feed is a byte of a word and no blank.
        (b"echo \x0c#'\n# @file r1\n'\n# @file r2\n", &[22..32], &[]),
        // R10.1e: `#` behind a quoted string is a byte of the word.
        (b"echo ''#'\n# @file r1\n'\n# @file r2\n", &[23..33], &[]),
        // R10.1f: `-` behind `>&` is a token: `#` behind it is a comment.
        (b": >&-#'\n# @file r1\n", &[8..18], &[]),
        // R10.1f: `3-` behind `<&` is a word: `#` behind it is a byte of it.
        (b": <&3-#'\n# @file r1\n'\n# @file r2\n", &[22..32], &[]),
        // R10.1g: a here-document.
        (b"cat <<EOF\n# @file r1\nEOF\n# @file r2\n", &[25..35], &[]),
        // R10.1g: an empty row does not end the text (finding F2).
        (
            b"cat <<EOF\n\n# @file r1\nEOF\n# @file r2\n",
            &[26..36],
            &[],
        ),
        // R10.1g: `<<-` drops the leading tabs.
        (
            b"cat <<-EOF\n# @file r1\n\tEOF\n# @file r2\n",
            &[27..37],
            &[],
        ),
        // R10.1g: a tab in the delimiter turns off tab stripping.
        (
            b"cat <<-'\tEOF'\n\t\tEOF\n# @file r1\n\tEOF\n# @file r2\n",
            &[36..46],
            &[],
        ),
        // R10.1g: a delimiter row with a blank behind it is text.
        (
            b"cat <<EOF\nEOF \n# @file r1\nEOF\n# @file r2\n",
            &[30..40],
            &[],
        ),
        // R10.1g: a quoted part of the word.
        (
            b"cat <<E\"O\"F\n# @file r1\nEOF\n# @file r2\n",
            &[27..37],
            &[],
        ),
        // R10.1g: a backslash in the word.
        (
            b"cat <<E\\OF\n# @file r1\nEOF\n# @file r2\n",
            &[26..36],
            &[],
        ),
        // R10.1g: a blank in a quoted word.
        (
            b"cat <<'E F'\n# @file r1\nE F\n# @file r2\n",
            &[27..37],
            &[],
        ),
        // R10.1g: a word with `$`.
        (b"cat <<$x\n# @file r1\n$x\n# @file r2\n", &[23..33], &[]),
        // R10.1g: two operators across `|` (finding F5).
        (
            b"cat <<A | cat <<B\nB\nA\n# @file r1\nB\n# @file r2\n",
            &[35..45],
            &[],
        ),
        // R10.1g: the text starts behind the line feed that ends the command.
        (
            b"cat <<EOF \"a\nEOF\nb\"\n# @file r1\nEOF\n# @file r2\n",
            &[35..45],
            &[],
        ),
        // R10.1g: the operator inside a function body.
        (
            b"f() { cat <<EOF; }\n# @file r1\nEOF\n# @file r2\n",
            &[34..44],
            &[],
        ),
        // R10.1h: unquoted word: a row with one backslash at its end is joined (finding F3).
        (
            b"cat <<EOF\na\\\nEOF\n# @file r1\nEOF\n# @file r2\n",
            &[32..42],
            &[],
        ),
        // R10.1h: three backslashes join.
        (
            b"cat <<EOF\na\\\\\\\nEOF\n# @file r1\nEOF\n# @file r2\n",
            &[34..44],
            &[],
        ),
        // R10.1h: two backslashes do not join.
        (b"cat <<EOF\na\\\\\nEOF\n# @file r1\n", &[18..28], &[]),
        // R10.1h: quoted word: no row is joined.
        (b"cat <<'EOF'\na\\\nEOF\n# @file r1\n", &[19..29], &[]),
        // R10.1i: `<<<` opens nothing.
        (b"cat <<<EOF\n# @file r1\n", &[11..21], &[]),
        // R10.2a: `$( )` with a `)` in quotes.
        (
            b"x=$(echo \")\" ')')'\n# @file r1\n'\n# @file r2\n",
            &[32..42],
            &[],
        ),
        // R10.2a: in `$( )` a backslash takes the `)`, and `<<<` opens nothing.
        (
            b"x=$(echo \\) <<<y)'\n# @file r1\n'\n# @file r2\n",
            &[32..42],
            &[],
        ),
        // R10.2a: `<( )` and `>( )`.
        (
            b"cat <(a \")\") >(b)'\n# @file r1\n'\n# @file r2\n",
            &[32..42],
            &[],
        ),
        // R10.2a: `$( )` in a double-quoted string over rows.
        (
            b"x=\"$(echo \")\")\n# @file r1\n\"\n# @file r2\n",
            &[28..38],
            &[],
        ),
        // R10.2a: `$`, a joined row and `(` in a string are `$(`.
        (
            b"x=\"$\\\n(echo \")\")\n# @file r1\n\"\n# @file r2\n",
            &[30..40],
            &[],
        ),
        // B27 (a): `<((` uses command substitution, not arithmetic balance.
        (b"cat <((:); : #); (\n# @file r1\n", &[], &[19..29]),
        // Inside the command frame `<<` is a heredoc operator, not a shift.
        (b"cat <(( : <<EOF ))\n# @file r1\n", &[], &[19..29]),
        // A nested arithmetic expansion has no power to balance the outer command.
        (
            b"cat <(x=$(( 1 << 2 )); : #); (\n# @file r1\n",
            &[],
            &[31..41],
        ),
        // R10.2b: `$(( ))`: a shift has no here-document.
        (b"x=$(( 1 << 2 ))\n# @file r1\n", &[16..26], &[]),
        // R10.2b: `$(( ))`: a hash stops the scan.
        (b"x=$(( 1 << 2 # 3 ))\n# @file r1\n", &[], &[20..30]),
        // R10.2b: `$(( ))`: an expansion containing a hash stops.
        (b"x=$(( ${y:-)) #} ))\n# @file r1\n", &[], &[20..30]),
        // R10.2b: `$(( ))`: non-arithmetic command content is contained.
        (b"x=$(( $( (a); b ) ))\n# @file r1\n", &[21..31], &[]),
        // R10.2c: `${ }` with `}` in quotes.
        (
            b"x=${y:-\"}\"'}'}'\n# @file r1\n'\n# @file r2\n",
            &[29..39],
            &[],
        ),
        // R10.2c: `${ }` in a string: a single-quoted string of plain bytes.
        (
            b"x=\"${y:-':'}\n# @file r1\n\"\n# @file r2\n",
            &[26..36],
            &[],
        ),
        // R10.2c: nested forms.
        (
            b"x=$(a ${b:-$(c \"$(d)\")})'\n# @file r1\n'\n# @file r2\n",
            &[39..49],
            &[],
        ),
        // R10.2d: `$[ ]`.
        (
            b"x=$[ 1 << 2 ]'\n# @file r1\n'\n# @file r2\n",
            &[28..38],
            &[],
        ),
        // R10.3a: an arithmetic command.
        (
            b"(( i++ )); : '\n# @file r1\n'\n# @file r2\n",
            &[28..38],
            &[],
        ),
        // R10.3a: `((` whose pair no `)` follows.
        (b"((:); :)\n# @file r1\n", &[], &[9..19]),
        // R10.3a: `<<` in `(( ))`.
        (b"(( x = 1 << 2 ))\n# @file r1\n", &[], &[17..27]),
        // R10.3b: a subscript with blanks.
        (b"a[1 + 2]=3'\n# @file r1\n'\n# @file r2\n", &[25..35], &[]),
        // R10.3b: `<<` in a subscript.
        (b"a[1<<2]=3\n# @file r1\n2]\n", &[], &[10..20]),
        // R10.3b: `[` behind a word that is no name is a byte.
        (
            b"echo a.b[1<<EOF\n# @file r1\nEOF\n# @file r2\n",
            &[31..41],
            &[],
        ),
        // R10.3c: a compound assignment with subscripts.
        (
            b"a=( [k 1]=v\n# @file r1\n)\n# @file r2\n",
            &[12..22, 25..35],
            &[],
        ),
        // R10.3c: `<<` in a subscript of a compound assignment.
        (b"a=( [1<<2]=v )\n# @file r1\n", &[], &[15..25]),
        // R10.3c: `#` behind the `)` of a compound assignment.
        (b"a=(b)#'\n# @file r1\n'\n", &[], &[8..18]),
        // R10.3d: an extended pattern.
        (b"[[ a == @(b|c) ]]\n# @file r1\n", &[18..28], &[]),
        // R10.3d: `!(`: a `<<` inside it.
        (b"!(cat <<EOF)\n# @file r1\nEOF\n", &[], &[13..23]),
        // R10.3d: `#` behind the `)` of an extended pattern.
        (b"!(:)#'\n# @file r1\n", &[], &[7..17]),
        // R10.3e: the word behind `=~` with parentheses.
        (b"[[ a =~ ^(b|c d)$ ]]\n# @file r1\n", &[21..31], &[]),
        // R10.3e: a parenthesis directly behind `=~`.
        (b"[[ a =~(b|c) ]]\n# @file r1\n", &[16..26], &[]),
        // R10.3e: a backslash in the word behind `=~` takes a blank.
        (b"[[ a =~ b\\ c ]]\n# @file r1\n", &[16..26], &[]),
        // R10.3e: `|` outside parentheses in the word behind `=~`.
        (b"[[ a =~ b|c ]]\n# @file r1\n", &[], &[15..25]),
        // R10.3e: a name, `=` and `(` in the word behind `=~`.
        (b"[[ a =~ x=(b) ]]\n# @file r1\n", &[], &[17..27]),
        // R10.3e: `#` behind a parenthesis of the word behind `=~`.
        (b"[[ a =~ (b)#c ]]\n# @file r1\n", &[], &[17..27]),
        // R10.4a: `$( )` over rows.
        (b"x=$(\n# @file r1\n)\n# @file r2\n", &[], &[5..15, 18..28]),
        // R10.4a: `<( )` over rows.
        (
            b"cat <(\n# @file r1\n)\n# @file r2\n",
            &[],
            &[7..17, 20..30],
        ),
        // R10.4a: `(( ))` over rows.
        (
            b"(( 1 +\n# @file r1\n2 ))\n# @file r2\n",
            &[],
            &[7..17, 23..33],
        ),
        // R10.4a: `$(( ))` over rows.
        (
            b"x=$(( 1 +\n# @file r1\n2 ))\n# @file r2\n",
            &[],
            &[10..20, 26..36],
        ),
        // R10.4a: `${ }` over rows.
        (
            b"x=${y:-\n# @file r1\n}\n# @file r2\n",
            &[],
            &[8..18, 21..31],
        ),
        // R10.4a: `$[ ]` over rows.
        (
            b"x=$[ 1 +\n# @file r1\n2 ]\n# @file r2\n",
            &[],
            &[9..19, 24..34],
        ),
        // R10.4a: a subscript over rows.
        (
            b"a[1 +\n# @file r1\n2]=3\n# @file r2\n",
            &[],
            &[6..16, 22..32],
        ),
        // R10.4a: the word behind `=~` over rows.
        (
            b"[[ a =~ (b\n# @file r1\nc) ]]\n# @file r2\n",
            &[],
            &[11..21, 28..38],
        ),
        // R10.4a: `$`, a joined row and `(` in a string open a substitution, here over rows.
        (
            b"x=\"$\\\n(echo \")\n# @file r1\n\")\"\n# @file r2\n",
            &[],
            &[15..25, 30..40],
        ),
        // R10.4b: a comment in `$( )`.
        (
            b"x=$(echo a # c)\n# @file r1\n)\n# @file r2\n",
            &[],
            &[16..26, 29..39],
        ),
        // R10.4b: a comment behind `$( )` in `$( )`.
        (
            b": $(: $(y)#c)\n# @file r1\n)\n# @file r2\n",
            &[],
            &[14..24, 27..37],
        ),
        // R10.4b: a here-document in `$( )`.
        (
            b"x=$(cat <<EOF\n# @file r1\nEOF\n)\n# @file r2\n",
            &[],
            &[14..24, 31..41],
        ),
        // R10.4b: `case` in `$( )`.
        (
            b"x=$(case a in a) echo;; esac)\n# @file r1\n",
            &[],
            &[30..40],
        ),
        // R10.4b: a quote in `$[ ]`.
        (b"x=$[ ']' ]\n# @file r1\n", &[], &[11..21]),
        // R10.4b: `{` in `${ }`.
        (
            b"x=${y:-{}}'\n# @file r1\n'\n# @file r2\n",
            &[],
            &[12..22, 25..35],
        ),
        // R10.4b: `${ }` in a string: a single-quoted `}`, which the POSIX mode of Bash reads as the close.
        (
            b"x=\"${y:-'}'}\n# @file r1\n\"\n# @file r2\n",
            &[],
            &[13..23, 26..36],
        ),
        // R10.4c: a delimiter word with a substitution.
        (
            b"cat <<$(x)\n# @file r1\n$(x)\n# @file r2\n",
            &[],
            &[11..21, 27..37],
        ),
        // R10.4c: a delimiter word with `${ }`.
        (
            b"cat <<${x:-a b}\n# @file r1\n${x:-a b}\n# @file r2\n",
            &[],
            &[16..26, 37..47],
        ),
        // R10.4c: a delimiter word in double quotes with `$`.
        (
            b"cat <<\"$x\"\n# @file r1\n$x\n# @file r2\n",
            &[],
            &[11..21, 25..35],
        ),
        // R10.4c: a delimiter word on the next row.
        (
            b"cat <<EOF\\\n\n# @file r1\nEOF\n# @file r2\n",
            &[],
            &[12..22, 27..37],
        ),
        // R10.4d: a NUL byte.
        (b"echo a\x00b\n# @file r1\n", &[], &[9..19]),
        // R10.1h: a joined row is the delimiter (finding F7).
        (b"cat <<EOF\nE\\\nOF\n# @file r1\nEOF\n", &[16..26], &[]),
        // R10.1g: two operators on one command: the first text first.
        (
            b"cat <<A <<B\n# @file r1\nA\nB\n# @file r2\n",
            &[27..37],
            &[],
        ),
    ];
    for (source, read, withheld) in rows {
        let budget = Counting::default();
        let mut scan = Scan::new(source, vec![0..source.len()]);
        let (mut rows_read, mut rows_withheld) = (Vec::new(), Vec::new());
        for row in probe_rows(source) {
            scan.read(row.start, &budget).unwrap();
            match scan.place(row.start) {
                Place::Comment { end, line: true } if end == row.end => rows_read.push(row),
                Place::Behind(_) => rows_withheld.push(row),
                _ => {}
            }
        }
        assert_eq!(
            (rows_read.as_slice(), rows_withheld.as_slice()),
            (read, withheld),
            "{}",
            source.escape_ascii()
        );
        // A source that the scan reads to its end costs one charge per byte.
        scan.advance(source.len(), &budget).unwrap();
        if scan.stopped().is_none() {
            assert_eq!(
                budget.charged_bytes.get(),
                source.len(),
                "{}",
                source.escape_ascii()
            );
        } else {
            assert!(
                budget.charged_bytes.get() < source.len(),
                "{}",
                source.escape_ascii()
            );
        }
    }
}

/// Scans `source` to its end or to the place where the scan stops: that place with
/// its reason, and the steps and bytes charged. Every charge can cancel the scan.
fn scan_cost(source: &[u8]) -> (Option<(usize, UnresolvedReason)>, usize, usize) {
    use crate::lang_helpers::shdoc::scan::{Place, Scan};
    counted(|budget| {
        let mut scan = Scan::new(source, vec![0..source.len()]);
        scan.advance(source.len(), budget)?;
        Ok((0..=source.len()).find_map(|at| match scan.place(at) {
            Place::Behind(reason) => Some((at, reason)),
            _ => None,
        }))
    })
}

#[test]
fn bash_scan_charges_each_byte_once_and_each_open_form_once() {
    use UnresolvedReason::GrammarBoundary;
    // A source without a stop is read to its end: one charge per byte.
    for (source, steps) in [
        // expected: no form: no step.
        (&b"echo a # c\n"[..], 0),
        (b"cat <(a) >((a))\n", 3),
        // expected: strings and back quotes over rows are states of the scan and no
        // entries of its stack.
        (b"x='a\nb' \"c\n\" $'d\n' `e\n`\n", 0),
        // expected: `$(`, the string in it and `${` in the string: 3 entries.
        (b"x=$(a \"${b}\")\n", 3),
        // expected: one step for each of the two operators; the texts cost bytes.
        (b"cat <<A | cat <<B\nB\nA\nt\nB\n", 2),
    ] {
        assert_eq!(
            scan_cost(source),
            (None, steps, source.len()),
            "{}",
            source.escape_ascii()
        );
    }
    for (source, stop, steps, bytes) in [
        // The balanced expression, nested expansion, and its nested form are
        // each charged before the hash stops the scan at the opener.
        (&b"x=$(( ${y:-)) #} ))\n"[..], 3, 3, 15),
        // expected: the form does not close on its row: its `(` is the stop. Read
        // are `x=$(` and nothing else: the line feed is looked at, not read.
        (&b"x=$(\n)\n"[..], 3, 1, 4),
        // expected: the 7 bytes of the row are read once, then the scan stops.
        (b"x=$(a b\n)\n", 3, 1, 7),
        // expected: `((`, a blank, `1`, a blank and the first `<` are read; the
        // second `<` is looked at.
        (b"(( 1 << 2 ))\n", 1, 1, 6),
    ] {
        let stopped = Some((stop, GrammarBoundary));
        // expected: one entry for the form that does not close.
        assert_eq!(
            scan_cost(source),
            (stopped, steps, bytes),
            "{}",
            source.escape_ascii()
        );
    }
    // expected: the NUL byte is read and is the stop; no form is open.
    assert_eq!(scan_cost(b"a\0b\n"), (Some((1, GrammarBoundary)), 0, 2));
    // expected: the bytes 0x01 and 0x7f are Bash's own quoting bytes; a delimiter word
    // that holds one is not read. `cat <<`, `E` and the byte are read; the stop is the
    // first byte of the word.
    for byte in [1, 0x7f] {
        let source = [&b"cat <<E"[..], &[byte], b"F\nt\nEF\n"].concat();
        assert_eq!(scan_cost(&source), (Some((6, GrammarBoundary)), 0, 8));
    }
}

#[test]
fn bash_scan_nesting_costs_one_stack_entry_per_open_form() {
    use crate::lang_helpers::shdoc::scan::Scan;
    for depth in [64, 4096, 65536] {
        // `x=`, `depth` times `$(`, `:`, `depth` times `)`, a line feed.
        let mut source = b"x=".to_vec();
        source.extend_from_slice("$(".repeat(depth).as_bytes());
        source.push(b':');
        source.extend_from_slice(")".repeat(depth).as_bytes());
        source.push(b'\n');
        let budget = Counting::default();
        let mut scan = Scan::new(&source, vec![0..source.len()]);
        scan.advance(source.len(), &budget).unwrap();
        assert_eq!(scan.stopped(), None);
        // expected: one entry of the scan's own stack per `$(`, each charged once, and
        // every byte once. The depth costs no frame of the call stack.
        assert_eq!(budget.steps.get(), depth);
        assert_eq!(budget.charged_bytes.get(), 3 * depth + 4);
    }
}

/// The checkpoints of one collection that publishes `documents` documents.
#[cfg(feature = "bash")]
fn collect_polls(parser: &AnyParser, documents: usize) -> usize {
    let before = observations().polls;
    let index = parser
        .documentation(&DocumentationContext::default(), &options(false, false))
        .unwrap();
    assert_eq!(index.documents().len(), documents);
    observations().polls - before
}

#[cfg(feature = "bash")]
#[test]
fn bash_collection_charges_the_scan_to_its_budget() {
    // Each source holds `bulk` bytes that one call of the walk makes the scan read:
    // the text of a here-document and a string that ends a command, each read when the
    // walk leaves the node that holds it; the blanks before a command, read when the
    // walk enters the command; and the blanks before a function, read when the run
    // above it is confirmed.
    let sources: [fn(usize) -> Vec<u8>; 4] = [
        |bulk| {
            let tail = b"\nEOF\n# @description a\nf() { :; }\n";
            [&b"cat <<'EOF'\n"[..], &b"x".repeat(bulk), tail].concat()
        },
        |bulk| {
            let head = b"# @description a\nf() { :; }\necho '";
            [&head[..], &b"x".repeat(bulk), b"'\n"].concat()
        },
        |bulk| {
            let head = b"# @description a\nf() { :; }\n";
            [&head[..], &b" ".repeat(bulk), b"echo a\n"].concat()
        },
        |bulk| {
            [
                &b"# @description a\n"[..],
                &b" ".repeat(bulk),
                b"f() { :; }\n",
            ]
            .concat()
        },
    ];
    let context = DocumentationContext::default();
    for source in sources {
        let small = parsed(&source(1));
        let large = parsed(&source(1 + 16 * 16384));
        assert!(!large.ts_tree().root_node().has_error());
        // expected: the scan reads the bulk once, a byte at a charge. That is 16
        // intervals of 16384 bytes, each ended by one checkpoint. Nothing else reads
        // the bulk.
        let polls = collect_polls(&large, 1);
        assert_eq!(polls - collect_polls(&small, 1), 16);
        for checkpoints in 1..=polls {
            let result = crate::documentation::budget::cancel_after(checkpoints, || {
                large.documentation(&context, &options(false, false))
            });
            assert!(matches!(result, Err(CollectionError::Cancelled)));
        }
        let index = crate::documentation::budget::cancel_after(polls + 1, || {
            large.documentation(&context, &options(false, false))
        })
        .unwrap();
        assert_eq!(
            published(&index).associations[0].resolution,
            Resolution::Resolved
        );
    }
}

/// The walk of the collector over `node` and the nodes below it, for `reading` alone.
#[cfg(feature = "bash")]
fn read_through<'a, B: WorkBudget>(
    reading: &mut crate::lang_helpers::shdoc::Reading<'a>,
    context: &SourceContext<'a>,
    node: crate::Node<'a>,
    budget: &B,
) -> Result<(), SourceError> {
    reading.enter(context, &node, node.kind() == "comment", budget)?;
    for child in node.children() {
        read_through(reading, context, child, budget)?;
    }
    reading.leave(&node, budget).map(|_| ()).map_err(Into::into)
}

#[cfg(feature = "bash")]
#[test]
fn bash_reading_charges_the_budget_it_is_given() {
    use crate::documentation::budget::NoBudget;
    use crate::documentation::recognition::NativeRules;
    use crate::documentation::streaming::positional::{SourceRequest, source_comment_with_budget};
    use crate::lang_helpers::shdoc::Reading;
    let source = b"x=$(a)\n# @description x\n# @arg $1 y\n  f() { :; }\n";
    let parser = parsed(source);
    let context = SourceContext::new(parser.ts_tree(), source);

    // The walk: `enter` and `leave` for every node. The first `enter` starts the scan.
    let ((), steps, read) = counted(|budget| {
        read_through(
            &mut Reading::default(),
            &context,
            parser.root_node(),
            budget,
        )
    });
    // expected: every byte of the source once.
    assert_eq!(read, source.len());
    // expected: to start the scan, the shared lookup of the included bytes takes one
    // step to find the one range and one to visit it, and the unit one for the piece
    // it gives; the scan takes one for the form `$(a)`.
    assert_eq!(steps, 2 + 1 + 1);

    // The confirmation of the run of two rows above `f`.
    let [_, last] = nodes_of(&parser, "comment")[..] else {
        panic!("two comments")
    };
    let (kept, steps, read) = counted(|budget| {
        let mut reading = Reading::default();
        for node in parser.root_node().preorder() {
            if node.start_byte() <= last.start_byte() {
                reading.enter(&context, &node, node.kind() == "comment", &NoBudget)?;
            }
        }
        let mut run = None;
        source_comment_with_budget::<NativeRules<crate::BashCode>, _>(
            &last,
            crate::Ancestors::unknown(),
            &mut SourceRequest {
                context: &context,
                retain_unbound: false,
                retain_directives: false,
                sink: &mut |found| run = Some(found),
            },
            &NoBudget,
        )?;
        let confirmed = reading.confirm(run.unwrap(), false, budget)?;
        Ok(confirmed.map(|run| run.members.len()))
    });
    assert_eq!(kept, Some(2));
    // expected: one step for each of the two members, and two for the search of each
    // among the two rows that the walk entered.
    assert_eq!(steps, 2 + 2 * 2);
    // expected: the line feed behind the last row and the two blanks before `f`.
    assert_eq!(read, 3);
}

// The grammar ends a comment at the end of its row, and so does the scan. The unit
// does not rely on that: a node that starts where the scan reads a comment and ends
// elsewhere is no row.
#[cfg(feature = "bash")]
#[test]
fn bash_node_that_ends_elsewhere_than_the_comment_is_no_row() {
    use crate::documentation::budget::NoBudget;
    use crate::lang_helpers::shdoc::Reading;
    let source = b"# c\n:\n";
    let parser = parsed(source);
    let context = SourceContext::new(parser.ts_tree(), source);
    let [comment] = nodes_of(&parser, "comment")[..] else {
        panic!("one comment")
    };
    let program = parser.root_node();
    // expected: both nodes start at the `#`; the comment ends at byte 3, the program
    // at byte 6.
    assert_eq!((program.start_byte(), program.end_byte()), (0, 6));
    assert_eq!((comment.start_byte(), comment.end_byte()), (0, 3));
    for (node, row) in [(comment, true), (program, false)] {
        let mut reading = Reading::default();
        reading.enter(&context, &node, true, &NoBudget).unwrap();
        assert_eq!(!reading.unread(), row);
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_run_width_costs_linear_work() {
    use crate::documentation::streaming::positional::{
        members_materialized, positional_runs_collected,
    };
    use crate::lang_helpers::shdoc::scan::{bytes_scanned, forms_opened};
    for rows in [64, 512, 2048] {
        let mut source = b"# @description x\n".repeat(rows);
        source.extend_from_slice(b"f() { :; }\n");
        let parser = parsed(&source);
        let before = (
            observations(),
            members_materialized::observed(),
            positional_runs_collected::observed(),
        );
        let scanned = (bytes_scanned::observed(), forms_opened::observed());
        let index = parser
            .documentation(&DocumentationContext::default(), &options(false, false))
            .unwrap();
        let after = observations();
        // expected: the scan reads every byte of the source once, no more and no
        // less, and opens no form.
        assert_eq!(bytes_scanned::observed() - scanned.0, source.len());
        assert_eq!(forms_opened::observed() - scanned.1, 0);
        // expected: the program, one comment per row, and the 11 nodes of `f() { :; }`:
        // the function, its name, `(`, `)`, the body, `{`, the command, its name and
        // the word of the name, `;`, `}`.
        assert_eq!(parser.root_node().preorder().count(), rows + 12);
        assert_eq!(after.visits - before.0.visits, rows + 12);
        // expected: per row 2 bytes for the directive test (the blank after `#` and the
        // `@`), asked once by the opener and once by the shared member loop. The scan
        // charges its bytes to the budget and does not use the shared byte scan.
        assert_eq!(after.classifier_bytes - before.0.classifier_bytes, 4 * rows);
        // expected: the run is scanned once, at its last row, and holds every row.
        assert_eq!(positional_runs_collected::observed() - before.2, 1);
        assert_eq!(members_materialized::observed() - before.1, rows);
        assert_eq!(after.renders - before.0.renders, 1);
        let [document] = index.documents().collect::<Vec<_>>()[..] else {
            panic!("one document")
        };
        assert_eq!(document.get().fragment_count(), rows);
        assert_eq!(index.associations().len(), 1);
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_withheld_run_costs_linear_work_and_keeps_nothing() {
    use crate::documentation::streaming::positional::{
        members_materialized, positional_runs_collected,
    };
    use crate::lang_helpers::shdoc::scan::{bytes_scanned, forms_opened};
    for rows in [64, 512, 2048] {
        // The scan stops at a substitution over rows.
        let mut source = b"x=$(\n:\n)\n".to_vec();
        source.extend_from_slice(&b"# @description x\n".repeat(rows));
        source.extend_from_slice(b"f() { :; }\n");
        let parser = parsed(&source);
        assert!(!parser.ts_tree().root_node().has_error());
        let before = (
            observations(),
            members_materialized::observed(),
            positional_runs_collected::observed(),
        );
        let scanned = (bytes_scanned::observed(), forms_opened::observed());
        let index = parser
            .documentation(&DocumentationContext::default(), &options(true, true))
            .unwrap();
        let after = observations();
        // expected: the scan reads `x=$(` and stops: 4 bytes and one form, whatever
        // the width of the run behind the stop.
        assert_eq!(bytes_scanned::observed() - scanned.0, 4);
        assert_eq!(forms_opened::observed() - scanned.1, 1);
        // expected: the counter holds the bytes that the shared byte scan reads. The run
        // costs what a selected run costs: per row 2 bytes for the directive test,
        // asked once by the opener and once by the shared member loop.
        assert_eq!(after.classifier_bytes - before.0.classifier_bytes, 4 * rows);
        // expected: the run is scanned once, at its last row, and holds every row.
        assert_eq!(positional_runs_collected::observed() - before.2, 1);
        assert_eq!(members_materialized::observed() - before.1, rows);
        assert_eq!(after.renders - before.0.renders, 0);
        assert_eq!(index.documents().len(), 0);
        assert_eq!(index.evidence().len(), 0);
        // expected: one assessment for the run, on its first row at byte 9, and one
        // for the function.
        let view = published(&index);
        assert_eq!(
            view.assessments,
            unordered([
                withheld(9..25),
                recognized(function(9 + 17 * rows..19 + 17 * rows)),
            ])
        );
    }
}

#[cfg(feature = "bash")]
#[test]
fn bash_withheld_run_charges_its_scan_to_the_collection_budget() {
    // The run stands behind a substitution over rows and is withheld. Its first row
    // is a ShellCheck directive behind `blanks` spaces.
    let source = |blanks: usize| {
        let mut source = b"x=$(\n:\n)\n#".to_vec();
        source.resize(source.len() + blanks, b' ');
        source.extend_from_slice(b"shellcheck x\n# @description x\nf() { :; }\n");
        parsed(&source)
    };
    let large = source(16 * 16384);
    assert!(!large.ts_tree().root_node().has_error());
    // expected: nothing reads the blanks before the stop. Behind it the scan that asks
    // whether the rules would select the run reads them twice, a byte at a charge: the
    // opener asks whether the row is a directive, and so does the shared member loop.
    // That is 32 intervals of 16384 bytes, each ended by one checkpoint.
    assert_eq!(collect_polls(&large, 0) - collect_polls(&source(1), 0), 32);
}
