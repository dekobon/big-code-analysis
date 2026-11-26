//! Elixir documentation attributes in the source view: the module or the group of clauses
//! that a `@moduledoc` or a `@doc` with one plain string documents, the text and the bytes
//! it is copied from, and the attributes that stay pending. A source is collected through
//! both entry points, except in the tests of restricted parses, of the queries and of cost.
//! The test of the byte scans calls three functions and collects nothing. One test collects
//! a Ruby source. The last test collects nothing and pins the parse shapes the reader
//! relies on.

#[cfg(feature = "elixir")]
use super::Builder;
use super::elixir::syntax::{rejected, same, separated};
use crate::documentation::SourceError;
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "elixir")]
use crate::documentation::budget::cancel_after;
#[cfg(any(feature = "elixir", feature = "ruby"))]
use crate::documentation::*;
#[cfg(any(feature = "elixir", feature = "ruby"))]
use crate::{AnyParser, LANG};
use std::cell::Cell;
#[cfg(feature = "elixir")]
use std::fmt::Write;
#[cfg(feature = "elixir")]
use std::ops::Range;

// The short names the views use for the reasons of pending assessments.
#[cfg(feature = "elixir")]
const REASONS: [(&str, &str); 7] = [
    (
        "OUTSIDE",
        "Elixir documentation attribute outside a module body awaits interpretation",
    ),
    (
        "TEMPLATE",
        "Elixir documentation attribute in a quoted or unreadable call awaits interpretation",
    ),
    (
        "RECOVERED",
        "Recovered Elixir documentation attribute awaits interpretation",
    ),
    (
        "UNOWNED",
        "Elixir documentation attribute awaits an owner projection",
    ),
    (
        "VALUE",
        "Elixir documentation attribute value awaits decoding",
    ),
    (
        "SEVERAL",
        "Several Elixir documentation attributes await selection",
    ),
    (
        "PROJECTION",
        "Recognized declaration requires a semantic projection",
    ),
];

// What one collection publishes, each record as one line:
// - `owners`: every owner after the file, in owner order, as `module A..B` or
//   `group A..B: C..D, …` (the module of the group, then its clauses);
// - `documents`: `ROLE COMPOSITION [FRAGMENT, …] CONTENT`, with the content `"text"` and
//   one `OUTPUT<-SOURCE` per mapping, `pending`, or `unavailable REASON`;
// - `associations`: `DOCUMENT RESOLUTION OWNER by BINDING PUBLICATION`;
// - `assessments`: `AXIS REASON KIND RANGE DOCUMENT`, sorted, because the contract gives
//   assessments no order.
#[cfg(feature = "elixir")]
#[derive(Debug, PartialEq, Eq)]
struct View {
    owners: Vec<String>,
    documents: Vec<String>,
    associations: Vec<String>,
    assessments: Vec<String>,
}

#[cfg(feature = "elixir")]
type Rows = [&'static [&'static str]; 4];

#[cfg(feature = "elixir")]
fn expected(rows: Rows) -> View {
    let [owners, documents, associations, assessments] =
        rows.map(|row| row.iter().map(ToString::to_string).collect());
    View {
        owners,
        documents,
        associations,
        assessments,
    }
}

#[cfg(feature = "elixir")]
fn span(range: &Range<usize>) -> String {
    format!("{}..{}", range.start, range.end)
}

// The pieces of a spelling, which together are the bytes of its node.
#[cfg(feature = "elixir")]
fn spelled(source: &SourcePieces<'_>, node: tree_sitter::Node<'_>) -> String {
    let pieces = source.pieces();
    assert!(!source.has_excluded_gap());
    assert_eq!(
        pieces[0].range().start..pieces[pieces.len() - 1].range().end,
        node.byte_range()
    );
    let ranges: Vec<_> = pieces.iter().map(|piece| span(&piece.range())).collect();
    ranges.join("+")
}

#[cfg(feature = "elixir")]
fn owner_text(owner: &Owner<'_>) -> String {
    match owner {
        Owner::Node(module) => {
            assert_eq!(module.node().kind(), "call");
            format!("module {}", span(&module.node().byte_range()))
        }
        Owner::Group(group) => {
            assert_eq!(group.scope.node().kind(), "call");
            let clauses: Vec<_> = group
                .pieces
                .iter()
                .map(|piece| {
                    let clause = piece.node.unwrap().node();
                    assert_eq!(clause.kind(), "call");
                    // expected: a clause is fully included, so its pieces touch.
                    spelled(&piece.source, clause);
                    span(&clause.byte_range())
                })
                .collect();
            format!(
                "group {}: {}",
                span(&group.scope.node().byte_range()),
                clauses.join(", ")
            )
        }
        _ => panic!("unexpected owner variant"),
    }
}

#[cfg(feature = "elixir")]
fn document_text<'s>(
    index: &DocumentationIndex<'s>,
    document: FactRef<'_, Document<'s>>,
) -> String {
    let fact = document.get();
    let mut fragments = Vec::new();
    for position in 0..fact.fragment_count() {
        let fragment = index.facts().document_fragment(document, position).unwrap();
        assert_eq!(fragment.get().form(), FragmentForm::Attribute);
        let attribute = fragment.get().node().unwrap().node();
        assert_eq!(attribute.kind(), "unary_operator");
        fragments.push(spelled(fragment.get().source(), attribute));
    }
    // expected: the profile has no prelexing and no documentation command.
    assert!(index.source_cleaned_text(document).unwrap().is_none());
    assert_eq!(index.document_qualifications(document).unwrap().len(), 0);
    let complete = index.complete_text(document).unwrap();
    let content = match fact.content() {
        Content::Complete { .. } => {
            let complete = complete.unwrap();
            let mut text = format!("{:?}", complete.text());
            for mapping in complete.mappings() {
                let Origin::Copied(copied) = &mapping.origin else {
                    panic!("the text of a plain string is copied");
                };
                let [piece] = copied.pieces() else {
                    panic!("one piece per run");
                };
                let output = span(&mapping.output.range());
                write!(text, " {output}<-{}", span(&piece.range())).unwrap();
            }
            text
        }
        Content::PendingAdapter => {
            assert!(complete.is_none());
            "pending".to_owned()
        }
        Content::Unavailable(reason) => format!("unavailable {reason:?}"),
        other => panic!("unexpected content {other:?}"),
    };
    format!(
        "{:?} {:?} [{}] {content}",
        fact.role(),
        fact.composition(),
        fragments.join(", ")
    )
}

// Builds the view and asserts what is the same for every source of the unit.
#[cfg(feature = "elixir")]
fn published(index: &DocumentationIndex<'_>) -> View {
    // bca: suppress(abc, halstead) -- one reading of every record kind of the index.
    assert_eq!(index.profile(), Profile::Elixir);
    assert_eq!(index.capability(), Capability::Partial);
    // expected: no documentation command names its declaration.
    assert_eq!(index.qualifications().len(), 0);
    // expected: the unit retains no candidate, whatever the options are.
    assert_eq!(index.evidence().len(), 0);
    let mut owners = index.facts().owners();
    assert!(matches!(owners.next().unwrap().get(), Owner::File(_)));
    let documents: Vec<_> = index.documents().collect();
    let position = |document| {
        documents
            .iter()
            .position(|known| *known == document)
            .unwrap()
    };
    let mut associations = Vec::new();
    for association in index.associations() {
        let fact = association.get();
        assert_eq!(fact.convention.name(), "Elixir");
        assert_eq!(fact.owner_count(), 1);
        let document = index.facts().associated_document(association).unwrap();
        let first = index.facts().document_fragment(document, 0).unwrap();
        // expected: the lexical scope is the body that holds the first attribute.
        let body = first.get().node().unwrap().node().parent().unwrap();
        assert_eq!(body.kind(), "do_block");
        assert_eq!(fact.lexical_scope.node(), body);
        let owner = index.facts().associated_owner(association, 0).unwrap();
        let binding = fact.binding.map_or("-".to_owned(), |clause| {
            assert_eq!(clause.node().kind(), "call");
            span(&clause.node().byte_range())
        });
        associations.push(format!(
            "{} {:?} {} by {binding} {:?}",
            position(document),
            fact.resolution,
            owner_text(owner.get()),
            fact.publication
        ));
    }
    let mut assessments = Vec::new();
    for assessment in index.assessments() {
        let fact = assessment.get();
        let reason = REASONS
            .iter()
            .find(|(_, reason)| *reason == fact.reason)
            .map_or(fact.reason, |(label, _)| label);
        let node = fact.node.unwrap().node();
        let document = index
            .assessed_document(assessment)
            .unwrap()
            .map_or("-".to_owned(), |document| position(document).to_string());
        assessments.push(format!(
            "{:?} {reason} {} {} {document}",
            fact.axis,
            node.kind(),
            span(&node.byte_range())
        ));
    }
    assessments.sort();
    View {
        owners: owners.map(|owner| owner_text(owner.get())).collect(),
        documents: documents
            .iter()
            .map(|document| document_text(index, *document))
            .collect(),
        associations,
        assessments,
    }
}

#[cfg(feature = "elixir")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Elixir, source.to_vec(), None, None).unwrap()
}

// One view through both entry points.
#[cfg(feature = "elixir")]
fn collected(source: &[u8], options: DocumentationOptions) -> View {
    let parser = parsed(source);
    let context = DocumentationContext::default();
    let typed = published(&parser.documentation(&context, &options).unwrap());
    let raw = collect(
        LANG::Elixir,
        parser.ts_tree(),
        parser.code(),
        &context,
        &options,
    )
    .unwrap();
    assert_eq!(typed, published(&raw));
    typed
}

#[cfg(feature = "elixir")]
fn view(source: &str) -> View {
    collected(source.as_bytes(), DocumentationOptions::default())
}

#[cfg(feature = "elixir")]
fn assert_views(rows: &[(&str, Rows)]) {
    for (source, rows) in rows {
        assert_eq!(view(source), expected(*rows), "{source}");
    }
}

// The one node of that kind and byte range.
#[cfg(feature = "elixir")]
fn node_at<'t>(
    tree: &'t tree_sitter::Tree,
    kind: &str,
    range: &Range<usize>,
) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == kind && node.byte_range() == *range {
            found.push(node);
        }
        stack.extend(node.children(&mut node.walk()));
    }
    assert_eq!(found.len(), 1, "{kind} {range:?}");
    found[0]
}

#[cfg(feature = "elixir")]
fn restricted(source: &str, included: &[Range<usize>]) -> tree_sitter::Tree {
    let point = |offset: usize| {
        let mut lines = source.as_bytes()[..offset].split(|byte| *byte == b'\n');
        let column = lines.next_back().map_or(0, <[u8]>::len);
        tree_sitter::Point::new(lines.count(), column)
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
        .set_language(&LANG::Elixir.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

// A budget that counts what it is charged and never cancels.
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    scanned: Cell<usize>,
    checkpoints: Cell<usize>,
}

impl WorkBudget for Counting {
    type Error = SourceError;
    fn checkpoint(&self) -> Result<(), SourceError> {
        self.checkpoints.set(self.checkpoints.get() + 1);
        Ok(())
    }
    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        Ok(())
    }
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.scanned.set(self.scanned.get() + count);
        Ok(())
    }
}

// What the second pass charges for one source: steps, bytes and checkpoints. The pass
// reads nothing that it or the end of the collection writes, so a second run over the
// finished index does the same work.
#[cfg(feature = "elixir")]
fn charged(source: &[u8]) -> [usize; 3] {
    let parser = parsed(source);
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let mut builder = Builder {
        index,
        drafts: Vec::new(),
        pending_attributes: Vec::new(),
        rust_values: std::collections::HashMap::new(),
        names: crate::documentation::names::Names::default(),
        pod_doubt: super::pod_doubt::Doubt::default(),
        pod_hidden: super::pod::Hidden::default(),
    };
    let budget = Counting::default();
    builder.elixir_attributes(&budget).unwrap();
    [
        budget.steps.get(),
        budget.scanned.get(),
        budget.checkpoints.get(),
    ]
}

#[cfg(feature = "elixir")]
#[test]
fn elixir_primary_fixture_documents_its_definition_group() {
    let source = crate::test_support::documentation::source("primary.ex");
    assert_eq!(
        source,
        b"defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend"
    );
    let rows: Rows = [
        &["module 0..52", "group 0..52: 32..48"],
        &["Primary Single [17..29] \"audit\" 0..5<-23..28"],
        &["0 Resolved group 0..52: 32..48 by 32..48 Unknown"],
        &[],
    ];
    let options = DocumentationOptions::default();
    assert_eq!(collected(&source, options), expected(rows));
    // expected: the unit retains no candidate, so no retention option changes the view.
    for (retain_unbound, retain_directives) in [(true, false), (false, true), (true, true)] {
        let options = DocumentationOptions {
            retain_unbound,
            retain_directives,
            ..options
        };
        assert_eq!(collected(&source, options), expected(rows));
    }
    // The independent primary requirement: the fragment, the owner's range and kind, the
    // text. The owner of a definition is a group, also with one clause.
    let parser = parsed(&source);
    let index = parser
        .documentation(&DocumentationContext::default(), &options)
        .unwrap();
    let association = index.associations().next().unwrap();
    let owner = index.facts().associated_owner(association, 0).unwrap();
    let Owner::Group(group) = owner.get() else {
        panic!("a definition is a group of its clauses");
    };
    let clause = group.pieces[0].node.unwrap().node();
    let document = index.documents().next().unwrap();
    let fragment = index.facts().document_fragment(document, 0).unwrap();
    let fragment = fragment.get().source().pieces()[0].range();
    crate::test_support::documentation::assert_primary_extraction(
        LANG::Elixir,
        Ok(serde_json::json!([{
            "fragments": [[fragment.start, fragment.end]],
            "owner": [clause.start_byte(), clause.end_byte()],
            "owner_kind": clause.kind(),
            "text": index.complete_text(document).unwrap().unwrap().text(),
        }])),
    );
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_moduledoc_documents_its_module() {
    assert_views(&[
        // b01_moduledoc
        (
            "defmodule M do\n@moduledoc \"audit\"\nend",
            [
                &["module 0..37"],
                &["FilePackage Single [15..33] \"audit\" 0..5<-27..32"],
                &["0 Resolved module 0..37 by - Unknown"],
                &[],
            ],
        ),
        // b02_moduledoc_after_definition
        (
            "defmodule M do\ndef f(), do: :ok\n@moduledoc \"audit\"\nend",
            [
                &["module 0..54", "group 0..54: 15..31"],
                &["FilePackage Single [32..50] \"audit\" 0..5<-44..49"],
                &["0 Resolved module 0..54 by - Unknown"],
                &[],
            ],
        ),
        // b03_moduledoc_then_doc
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"d\"\ndef f(), do: :ok\n",
                "end"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &[
                    "FilePackage Single [15..29] \"m\" 0..1<-27..28",
                    "Primary Single [30..38] \"d\" 0..1<-36..37",
                ],
                &[
                    "0 Resolved module 0..59 by - Unknown",
                    "1 Resolved group 0..59: 39..55 by 39..55 Unknown",
                ],
                &[],
            ],
        ),
        // b04_doc_then_moduledoc
        (
            concat!(
                "defmodule M do\n@doc \"d\"\n@moduledoc \"m\"\ndef f(), do: :ok\n",
                "end"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &[
                    "Primary Single [15..23] \"d\" 0..1<-21..22",
                    "FilePackage Single [24..38] \"m\" 0..1<-36..37",
                ],
                &[
                    "0 Resolved group 0..59: 39..55 by 39..55 Unknown",
                    "1 Resolved module 0..59 by - Unknown",
                ],
                &[],
            ],
        ),
        // b05_nested_modules
        (
            concat!(
                "defmodule A do\n@moduledoc \"a\"\ndefmodule B do\n@moduledoc \"b",
                "\"\n@doc \"g\"\ndef g(), do: 1\nend\n@doc \"f\"\ndef f(), do: 2",
                "\nend"
            ),
            [
                &[
                    "module 0..115",
                    "group 0..115: 97..111",
                    "module 30..87",
                    "group 30..87: 69..83",
                ],
                &[
                    "FilePackage Single [15..29] \"a\" 0..1<-27..28",
                    "FilePackage Single [45..59] \"b\" 0..1<-57..58",
                    "Primary Single [60..68] \"g\" 0..1<-66..67",
                    "Primary Single [88..96] \"f\" 0..1<-94..95",
                ],
                &[
                    "0 Resolved module 0..115 by - Unknown",
                    "1 Resolved module 30..87 by - Unknown",
                    "2 Resolved group 30..87: 69..83 by 69..83 Unknown",
                    "3 Resolved group 0..115: 97..111 by 97..111 Unknown",
                ],
                &[],
            ],
        ),
        // b06_undocumented
        (
            "defmodule M do\ndef f(), do: :ok\nend",
            [&["module 0..35", "group 0..35: 15..31"], &[], &[], &[]],
        ),
        // b07_empty_module
        ("defmodule M do\nend", [&["module 0..18"], &[], &[], &[]]),
        // b08_two_modules
        (
            concat!(
                "defmodule A do\n@doc \"one\"\ndef f(), do: 1\nend\ndefmodule B d",
                "o\n@doc \"two\"\ndef f(), do: 2\nend\n"
            ),
            [
                &[
                    "module 0..44",
                    "group 0..44: 26..40",
                    "module 45..89",
                    "group 45..89: 71..85",
                ],
                &[
                    "Primary Single [15..25] \"one\" 0..3<-21..24",
                    "Primary Single [60..70] \"two\" 0..3<-66..69",
                ],
                &[
                    "0 Resolved group 0..44: 26..40 by 26..40 Unknown",
                    "1 Resolved group 45..89: 71..85 by 71..85 Unknown",
                ],
                &[],
            ],
        ),
        // b09_module_name_in_parentheses
        (
            "defmodule(M) do\n@moduledoc \"audit\"\nend",
            [
                &["module 0..38"],
                &["FilePackage Single [16..34] \"audit\" 0..5<-28..33"],
                &["0 Resolved module 0..38 by - Unknown"],
                &[],
            ],
        ),
        // b10_module_in_a_function_body
        (
            concat!(
                "defmodule M do\ndef g() do\ndefmodule N do\n@moduledoc \"n\"\nen",
                "d\nend\nend"
            ),
            [
                &["module 0..67", "group 0..67: 15..63", "module 26..59"],
                &["FilePackage Single [41..55] \"n\" 0..1<-53..54"],
                &["0 Resolved module 26..59 by - Unknown"],
                &[],
            ],
        ),
        // b11_moduledoc_with_a_pending_value
        (
            "defmodule M do\n@moduledoc false\nend",
            [
                &["module 0..35"],
                &["FilePackage Single [15..31] pending"],
                &["0 Resolved module 0..35 by - Unknown"],
                &["Composition VALUE unary_operator 15..31 0"],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_clauses_join_one_group_by_kind_name_and_arity() {
    assert_views(&[
        // c01_head_and_clauses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f",
                "(x), do: x\nend"
            ),
            [
                &["module 0..72", "group 0..72: 28..36, 37..52, 53..68"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..72: 28..36, 37..52, 53..68 by 28..36 Unknown"],
                &[],
            ],
        ),
        // c02_arity
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef f(x), do: x\n@doc \"two\"\ndef",
                " f(x,y), do: x+y\nend"
            ),
            [
                &["module 0..76", "group 0..76: 26..41", "group 0..76: 53..72"],
                &[
                    "Primary Single [15..25] \"one\" 0..3<-21..24",
                    "Primary Single [42..52] \"two\" 0..3<-48..51",
                ],
                &[
                    "0 Resolved group 0..76: 26..41 by 26..41 Unknown",
                    "1 Resolved group 0..76: 53..72 by 53..72 Unknown",
                ],
                &[],
            ],
        ),
        // c03_default_argument
        (
            "defmodule M do\n@doc \"audit\"\ndef f(x \\\\ 0), do: x\nend",
            [
                &["module 0..52", "group 0..52: 28..48"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..52: 28..48 by 28..48 Unknown"],
                &[],
            ],
        ),
        // c04_clause_that_is_not_adjacent
        (
            concat!(
                "defmodule M do\ndef h(x), do: x\n@doc \"audit\"\ndef i(x), do: x",
                "\ndef h(y), do: y\nend"
            ),
            [
                &[
                    "module 0..79",
                    "group 0..79: 15..30, 60..75",
                    "group 0..79: 44..59",
                ],
                &["Primary Single [31..43] \"audit\" 0..5<-37..42"],
                &["0 Resolved group 0..79: 44..59 by 44..59 Unknown"],
                &[],
            ],
        ),
        // c05_guarded_and_plain_clause
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
                "f(x), do: x\nend"
            ),
            [
                &["module 0..74", "group 0..74: 28..54, 55..70"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..74: 28..54, 55..70 by 28..54 Unknown"],
                &[],
            ],
        ),
        // c06_bare_head_and_empty_parentheses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f\ndef f(), do: :ok\ndef f w",
                "hen true, do: :no\nend"
            ),
            [
                &["module 0..79", "group 0..79: 28..33, 34..50, 51..75"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..79: 28..33, 34..50, 51..75 by 28..33 Unknown"],
                &[],
            ],
        ),
        // c07_kinds_stay_apart
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef f(x), do: x\n@doc \"two\"\ndef",
                "macro f(x), do: x\ndefp f(x), do: x\ndefmacrop f(x), do: x\nend"
            ),
            [
                &[
                    "module 0..116",
                    "group 0..116: 26..41",
                    "group 0..116: 53..73",
                    "group 0..116: 74..90",
                    "group 0..116: 91..112",
                ],
                &[
                    "Primary Single [15..25] \"one\" 0..3<-21..24",
                    "Primary Single [42..52] \"two\" 0..3<-48..51",
                ],
                &[
                    "0 Resolved group 0..116: 26..41 by 26..41 Unknown",
                    "1 Resolved group 0..116: 53..73 by 53..73 Unknown",
                ],
                &[],
            ],
        ),
        // c08_written_arguments_are_counted
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(a, # c\n b), do: a\ndef f(",
                "a, b), do: b\ndef g(a, b: 1), do: a\ndef g(a, b), do: b\ndef f(a",
                "), do: a\nend"
            ),
            [
                &[
                    "module 0..131",
                    "group 0..131: 28..51, 52..70",
                    "group 0..131: 71..92, 93..111",
                    "group 0..131: 112..127",
                ],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..131: 28..51, 52..70 by 28..51 Unknown"],
                &[],
            ],
        ),
        // c09_names_of_one_length
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef ab(x), do: x\ndef ac(x), do:",
                " x\ndef ab(y), do: y\nend"
            ),
            [
                &[
                    "module 0..82",
                    "group 0..82: 28..44, 62..78",
                    "group 0..82: 45..61",
                ],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..82: 28..44, 62..78 by 28..44 Unknown"],
                &[],
            ],
        ),
        // c10_do_on_the_next_row
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndo\nx\nend\ndef f(y) d",
                "o\ny\nend\nend"
            ),
            [
                &["module 0..67", "group 0..67: 28..45, 46..63"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..67: 28..45, 46..63 by 28..45 Unknown"],
                &[],
            ],
        ),
        // c11_unquoted_argument
        (
            "defmodule M do\n@doc \"audit\"\ndef f(unquote(x)), do: x\nend",
            [
                &["module 0..56", "group 0..56: 28..52"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..56: 28..52 by 28..52 Unknown"],
                &[],
            ],
        ),
        // c12_names_that_the_compiler_normalizes
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef \u{b5}(x), do: x\ndef ",
                "\u{3bc}(x), do: x\nend"
            ),
            [
                &["module 0..65", "group 0..65: 28..44", "group 0..65: 45..61"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..65: 28..44 by 28..44 Unknown"],
                &[],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
fn elixir_private_definitions_keep_their_document_as_ineligible() {
    assert_views(&[
        // d01_defp
        (
            "defmodule M do\n@doc \"audit\"\ndefp f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 28..45"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..49: 28..45 by 28..45 Ineligible"],
                &[],
            ],
        ),
        // d02_defmacrop
        (
            "defmodule M do\n@doc \"audit\"\ndefmacrop f(), do: :ok\nend",
            [
                &["module 0..54", "group 0..54: 28..50"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..54: 28..50 by 28..50 Ineligible"],
                &[],
            ],
        ),
        // d03_defp_with_a_pending_value
        (
            "defmodule M do\n@doc false\ndefp f(), do: :ok\nend",
            [
                &["module 0..47", "group 0..47: 26..43"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..47: 26..43 by 26..43 Ineligible"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_attribute_is_read_past_comments_and_other_attributes() {
    assert_views(&[
        // e01_semicolon_comment_blank_row_and_spec
        (
            concat!(
                "defmodule M do\n@doc \"audit\"; # note\n\n@spec f() :: atom()\nd",
                "ef f(), do: :ok\nend"
            ),
            [
                &["module 0..77", "group 0..77: 57..73"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..77: 57..73 by 57..73 Unknown"],
                &[],
            ],
        ),
        // e02_other_attributes_and_reads
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
                "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..94", "group 0..94: 74..90"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..94: 74..90 by 74..90 Unknown"],
                &[],
            ],
        ),
        // e03_crlf
        (
            "defmodule M do\r\n@doc \"audit\"\r\ndef f(), do: :ok\r\nend",
            [
                &["module 0..51", "group 0..51: 30..46"],
                &["Primary Single [16..28] \"audit\" 0..5<-22..27"],
                &["0 Resolved group 0..51: 30..46 by 30..46 Unknown"],
                &[],
            ],
        ),
        // e04_row_with_a_lone_backslash
        (
            "defmodule M do\n@doc \"audit\"\n\\\ndef f(), do: :ok\nend",
            [
                &["module 0..50", "group 0..50: 30..46"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..50: 30..46 by 30..46 Unknown"],
                &[],
            ],
        ),
        // e05_semicolon_on_one_row
        (
            "defmodule M do\n@doc \"audit\"; def f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &[],
            ],
        ),
        // e06_comment_that_ends_with_a_backslash
        (
            "defmodule M do\n@doc \"audit\" # c \\\ndef f(), do: :ok\nend",
            [
                &["module 0..54", "group 0..54: 34..50"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..54: 34..50 by 34..50 Unknown"],
                &[],
            ],
        ),
        // e07_continued_row_then_comment
        (
            "defmodule M do\n@doc \"audit\" \\\n# c\ndef f(), do: :ok\nend",
            [
                &["module 0..54", "group 0..54: 34..50"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..54: 34..50 by 34..50 Unknown"],
                &[],
            ],
        ),
        // e08_space_after_the_operator
        (
            "defmodule M do\n@ doc \"audit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..28] \"audit\" 0..5<-22..27"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &[],
            ],
        ),
        // e09_line_end_after_the_operator
        (
            "defmodule M do\n@\ndoc \"audit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..28] \"audit\" 0..5<-22..27"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &[],
            ],
        ),
        // e10_comment_after_the_operator
        (
            "defmodule M do\n@ # c\ndoc \"audit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..53", "group 0..53: 33..49"],
                &["Primary Single [15..32] \"audit\" 0..5<-26..31"],
                &["0 Resolved group 0..53: 33..49 by 33..49 Unknown"],
                &[],
            ],
        ),
        // e11_value_in_parentheses
        (
            "defmodule M do\n@doc(\"audit\")\ndef f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..28] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &[],
            ],
        ),
        // e13_moduledoc_and_reads_between
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@behaviour B\n@type t :: atom()",
                "\n@opaque o :: atom()\n@before_compile B\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..117", "group 0..117: 97..113"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..117: 97..113 by 97..113 Unknown"],
                &[],
            ],
        ),
        // e12_typedoc_between
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@typedoc \"t\"\n@type t :: atom(",
                ")\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..79", "group 0..79: 59..75"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..79: 59..75 by 59..75 Unknown"],
                &["Owner UNOWNED unary_operator 28..40 -"],
            ],
        ),
    ]);
}

// Decision E5 leaves this undecided: by the reading of the compiler the next definition
// takes the attribute whatever stands between. The unit publishes no owner and one
// assessment; a later unit decides.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_statement_between_leaves_the_attribute_without_an_owner() {
    assert_views(&[
        // f01_match_between
        (
            "defmodule M do\n@doc \"audit\"\nx = 1\ndef f(), do: :ok\nend",
            [
                &["module 0..54", "group 0..54: 34..50"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f02_alias_between
        (
            "defmodule M do\n@doc \"audit\"\nalias A.B\ndef f(), do: :ok\nend",
            [
                &["module 0..58", "group 0..58: 38..54"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f03_nested_module_between
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefmodule N do\ndef g(), do: 1\n",
                "end\ndef f(), do: :ok\nend"
            ),
            [
                &[
                    "module 0..82",
                    "group 0..82: 62..78",
                    "module 28..61",
                    "group 28..61: 43..57",
                ],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f04_end_of_the_body
        (
            "defmodule M do\n@doc \"audit\"\nend",
            [
                &["module 0..31"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f05_unary_statement_between
        (
            "defmodule M do\n@doc \"audit\"\n- 1\ndef f(), do: :ok\nend",
            [
                &["module 0..52", "group 0..52: 32..48"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f06_callback_takes_the_attribute
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@callback g() :: atom()\ndef f()",
                ", do: :ok\nend"
            ),
            [
                &["module 0..72", "group 0..72: 52..68"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f07_macrocallback_takes_the_attribute
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@macrocallback g() :: atom()\nde",
                "f f(), do: :ok\nend"
            ),
            [
                &["module 0..77", "group 0..77: 57..73"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f08_two_attributes_released
        (
            concat!(
                "defmodule M do\n@doc \"a\"\n@doc \"b\"\nx = 1\ndef f(), do: :ok",
                "\nend"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &[],
                &[],
                &[
                    "Owner UNOWNED unary_operator 15..23 -",
                    "Owner UNOWNED unary_operator 24..32 -",
                ],
            ],
        ),
        // f09_list_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\n[0]\ndef f(), do: :ok\nend",
            [
                &["module 0..52", "group 0..52: 32..48"],
                &[],
                &[],
                &["Owner UNOWNED unary_operator 15..27 -"],
            ],
        ),
        // f10_next_run_after_a_release
        (
            concat!(
                "defmodule M do\n@doc \"a\"\nx = 1\n@doc \"b\"\ndef f(), do: :ok",
                "\nend"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &["Primary Single [30..38] \"b\" 0..1<-36..37"],
                &["0 Resolved group 0..59: 39..55 by 39..55 Unknown"],
                &["Owner UNOWNED unary_operator 15..23 -"],
            ],
        ),
    ]);
}

// A pending outcome: these definitions are not read yet.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_definitions_that_are_not_read_stay_pending() {
    assert_views(&[
        // g01_defguard
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefguard is_ok(x) when x == :ok",
                "\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..80", "group 0..80: 60..76"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..59 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g02_defguardp
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefguardp is_ok(x) when x == :ok",
                "\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..81", "group 0..81: 61..77"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..60 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g03_defdelegate
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefdelegate g(x), to: N\ndef f()",
                ", do: :ok\nend"
            ),
            [
                &["module 0..72", "group 0..72: 52..68"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..51 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g04_operator_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef a + b, do: a\ndef f(), do: :",
                "ok\nend"
            ),
            [
                &["module 0..65", "group 0..65: 45..61"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..44 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g05_unquote_head_with_arguments
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef unquote(name)(x), do: x\ndef",
                " f(), do: :ok\nend"
            ),
            [
                &["module 0..76", "group 0..76: 56..72"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..55 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g06_unquote_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef unquote(name), do: 1\ndef f(",
                "), do: :ok\nend"
            ),
            [
                &["module 0..73", "group 0..73: 53..69"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..52 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g07_spliced_arguments
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef g(unquote_splicing(args)), d",
                "o: 1\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..85", "group 0..85: 65..81"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..64 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g08_definition_call_in_parentheses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef(g(x), do: x)\ndef f(), do: :",
                "ok\nend"
            ),
            [
                &["module 0..65", "group 0..65: 45..61"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..44 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g09_head_without_parentheses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef g x, y do\nx\nend\ndef f(), ",
                "do: :ok\nend"
            ),
            [
                &["module 0..68", "group 0..68: 48..64"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..47 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g10_qualified_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef N.g(y), do: y\ndef f(), do: ",
                ":ok\nend"
            ),
            [
                &["module 0..66", "group 0..66: 46..62"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..45 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g11_guarded_operator_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef a + b when a > 0, do: a\ndef",
                " f(), do: :ok\nend"
            ),
            [
                &["module 0..76", "group 0..76: 56..72"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..55 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g12_private_operator_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefp a + b, do: a\ndef f(), do: ",
                ":ok\nend"
            ),
            [
                &["module 0..66", "group 0..66: 46..62"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..45 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g13_macro_with_an_unquote_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefmacro unquote(name)(x), do: x",
                "\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..81", "group 0..81: 61..77"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..60 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // g14_private_macro_with_a_qualified_head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefmacrop N.g(y), do: y\ndef f()",
                ", do: :ok\nend"
            ),
            [
                &["module 0..72", "group 0..72: 52..68"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 28..51 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
    ]);
}

// Every value but one plain string is a pending outcome of this unit.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_plain_string_is_the_only_value_with_text() {
    assert_views(&[
        // h01_empty_string
        (
            "defmodule M do\n@doc \"\"\ndef f(), do: :ok\nend",
            [
                &["module 0..43", "group 0..43: 23..39"],
                &["Primary Single [15..22] \"\""],
                &["0 Resolved group 0..43: 23..39 by 23..39 Unknown"],
                &[],
            ],
        ),
        // h02_hash_without_brace
        (
            "defmodule M do\n@doc \"a#b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] \"a#b\" 0..3<-21..24"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &[],
            ],
        ),
        // h03_line_end_inside
        (
            "defmodule M do\n@doc \"au\ndit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..28] \"au\\ndit\" 0..6<-21..27"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &[],
            ],
        ),
        // h04_crlf_inside
        (
            "defmodule M do\n@doc \"au\r\ndit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..50", "group 0..50: 30..46"],
                &["Primary Single [15..29] \"au\\r\\ndit\" 0..7<-21..28"],
                &["0 Resolved group 0..50: 30..46 by 30..46 Unknown"],
                &[],
            ],
        ),
        // h05_two_byte_character
        (
            "defmodule M do\n@doc \"\u{e9}t\u{e9}\"\ndef f(), do: :ok\nend",
            [
                &["module 0..48", "group 0..48: 28..44"],
                &["Primary Single [15..27] \"\u{e9}t\u{e9}\" 0..5<-21..26"],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &[],
            ],
        ),
        // h06_tab_inside
        (
            "defmodule M do\n@doc \"a\tb\"\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] \"a\\tb\" 0..3<-21..24"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &[],
            ],
        ),
        // h10_heredoc
        (
            concat!(
                "defmodule M do\n@doc \"\"\"\naudit\n\"\"\"\ndef f(), do: :ok\nen",
                "d"
            ),
            [
                &["module 0..54", "group 0..54: 34..50"],
                &["Primary Single [15..33] pending"],
                &["0 Resolved group 0..54: 34..50 by 34..50 Unknown"],
                &["Composition VALUE unary_operator 15..33 0"],
            ],
        ),
        // h11_escape
        (
            "defmodule M do\n@doc \"a\\nb\"\ndef f(), do: :ok\nend",
            [
                &["module 0..47", "group 0..47: 27..43"],
                &["Primary Single [15..26] pending"],
                &["0 Resolved group 0..47: 27..43 by 27..43 Unknown"],
                &["Composition VALUE unary_operator 15..26 0"],
            ],
        ),
        // h12_interpolation
        (
            "defmodule M do\n@doc \"a#{x}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..49", "group 0..49: 29..45"],
                &["Primary Single [15..28] pending"],
                &["0 Resolved group 0..49: 29..45 by 29..45 Unknown"],
                &["Composition VALUE unary_operator 15..28 0"],
            ],
        ),
        // h13_raw_sigil
        (
            "defmodule M do\n@doc ~S(a)\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h14_cooked_sigil
        (
            "defmodule M do\n@doc ~s(a)\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h15_charlist
        (
            "defmodule M do\n@doc 'audit'\ndef f(), do: :ok\nend",
            [
                &["module 0..48", "group 0..48: 28..44"],
                &["Primary Single [15..27] pending"],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &["Composition VALUE unary_operator 15..27 0"],
            ],
        ),
        // h16_false
        (
            "defmodule M do\n@doc false\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h17_nil
        (
            "defmodule M do\n@doc nil\ndef f(), do: :ok\nend",
            [
                &["module 0..44", "group 0..44: 24..40"],
                &["Primary Single [15..23] pending"],
                &["0 Resolved group 0..44: 24..40 by 24..40 Unknown"],
                &["Composition VALUE unary_operator 15..23 0"],
            ],
        ),
        // h18_keyword_metadata
        (
            "defmodule M do\n@doc since: \"1\"\ndef f(), do: :ok\nend",
            [
                &["module 0..51", "group 0..51: 31..47"],
                &["Primary Single [15..30] pending"],
                &["0 Resolved group 0..51: 31..47 by 31..47 Unknown"],
                &["Composition VALUE unary_operator 15..30 0"],
            ],
        ),
        // h19_concatenation
        (
            "defmodule M do\n@doc \"a\" <> \"b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..51", "group 0..51: 31..47"],
                &["Primary Single [15..30] pending"],
                &["0 Resolved group 0..51: 31..47 by 31..47 Unknown"],
                &["Composition VALUE unary_operator 15..30 0"],
            ],
        ),
        // h20_text_and_metadata
        (
            concat!(
                "defmodule M do\n@doc \"audit\", since: \"1\"\ndef f(), do: :ok\n",
                "end"
            ),
            [
                &["module 0..60", "group 0..60: 40..56"],
                &["Primary Single [15..39] pending"],
                &["0 Resolved group 0..60: 40..56 by 40..56 Unknown"],
                &["Composition VALUE unary_operator 15..39 0"],
            ],
        ),
        // h21_block_after_a_space
        (
            "defmodule M do\n@doc (\"audit\")\ndef f(), do: :ok\nend",
            [
                &["module 0..50", "group 0..50: 30..46"],
                &["Primary Single [15..29] pending"],
                &["0 Resolved group 0..50: 30..46 by 30..46 Unknown"],
                &["Composition VALUE unary_operator 15..29 0"],
            ],
        ),
        // h22_do_block
        (
            "defmodule M do\n@doc \"audit\" do\nend\ndef f(), do: :ok\nend",
            [
                &["module 0..55", "group 0..55: 35..51"],
                &["Primary Single [15..34] pending"],
                &["0 Resolved group 0..55: 35..51 by 35..51 Unknown"],
                &["Composition VALUE unary_operator 15..34 0"],
            ],
        ),
        // h23_lone_cr_inside
        (
            "defmodule M do\n@doc \"a\rb\"\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h24_vertical_tab_inside
        (
            "defmodule M do\n@doc \"a\u{b}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h25_form_feed_inside
        (
            "defmodule M do\n@doc \"a\u{c}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..46", "group 0..46: 26..42"],
                &["Primary Single [15..25] pending"],
                &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
                &["Composition VALUE unary_operator 15..25 0"],
            ],
        ),
        // h26_next_line_character
        (
            "defmodule M do\n@doc \"a\u{85}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..47", "group 0..47: 27..43"],
                &["Primary Single [15..26] pending"],
                &["0 Resolved group 0..47: 27..43 by 27..43 Unknown"],
                &["Composition VALUE unary_operator 15..26 0"],
            ],
        ),
        // h27_line_separator
        (
            "defmodule M do\n@doc \"a\u{2028}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..48", "group 0..48: 28..44"],
                &["Primary Single [15..27] pending"],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &["Composition VALUE unary_operator 15..27 0"],
            ],
        ),
        // h28_right_to_left_override
        (
            "defmodule M do\n@doc \"a\u{202e}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..48", "group 0..48: 28..44"],
                &["Primary Single [15..27] pending"],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &["Composition VALUE unary_operator 15..27 0"],
            ],
        ),
        // h29_pop_directional_isolate
        (
            "defmodule M do\n@doc \"a\u{2069}b\"\ndef f(), do: :ok\nend",
            [
                &["module 0..48", "group 0..48: 28..44"],
                &["Primary Single [15..27] pending"],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &["Composition VALUE unary_operator 15..27 0"],
            ],
        ),
        // h30_comment_in_parentheses
        (
            "defmodule M do\n@doc(# c\n\"audit\")\ndef f(), do: :ok\nend",
            [
                &["module 0..53", "group 0..53: 33..49"],
                &["Primary Single [15..32] \"audit\" 0..5<-25..30"],
                &["0 Resolved group 0..53: 33..49 by 33..49 Unknown"],
                &[],
            ],
        ),
        // h31_atom
        (
            "defmodule M do\n@doc :audit\ndef f(), do: :ok\nend",
            [
                &["module 0..47", "group 0..47: 27..43"],
                &["Primary Single [15..26] pending"],
                &["0 Resolved group 0..47: 27..43 by 27..43 Unknown"],
                &["Composition VALUE unary_operator 15..26 0"],
            ],
        ),
        // h32_number
        (
            "defmodule M do\n@doc 1\ndef f(), do: :ok\nend",
            [
                &["module 0..42", "group 0..42: 22..38"],
                &["Primary Single [15..21] pending"],
                &["0 Resolved group 0..42: 22..38 by 22..38 Unknown"],
                &["Composition VALUE unary_operator 15..21 0"],
            ],
        ),
        // h33_neighbours_of_the_rejected
        (
            concat!(
                "defmodule M do\n@doc \"a\u{84}\u{2027}\u{202f}\u{2065}\u{206a}b",
                "\"\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &[concat!(
                    "Primary Single [15..38] \"a\\u{84}\u{2027}\\u{202f}\\u{2065}\\u{",
                    "206a}b\" 0..16<-21..37"
                )],
                &["0 Resolved group 0..59: 39..55 by 39..55 Unknown"],
                &[],
            ],
        ),
        // h35_backslash_before_a_line_end_inside
        (
            "defmodule M do\n@doc \"a\\\nb\"\ndef f(), do: :ok\nend",
            [
                &["module 0..47", "group 0..47: 27..43"],
                &["Primary Single [15..26] pending"],
                &["0 Resolved group 0..47: 27..43 by 27..43 Unknown"],
                &["Composition VALUE unary_operator 15..26 0"],
            ],
        ),
    ]);
    // A byte that is no UTF-8 is in no `&str`.
    let source = b"defmodule M do\n@doc \"a\xffb\"\ndef f(), do: :ok\nend";
    let rows: Rows = [
        &["module 0..46", "group 0..46: 26..42"],
        &["Primary Single [15..25] unavailable InvalidEncoding"],
        &["0 Resolved group 0..46: 26..42 by 26..42 Unknown"],
        &[],
    ];
    assert_eq!(
        collected(source, DocumentationOptions::default()),
        expected(rows)
    );
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_rows_the_language_continues_belong_to_the_value() {
    assert_views(&[
        // n01_pipe_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
            [
                &["module 0..55", "group 0..55: 35..51"],
                &["Primary Single [15..34] pending"],
                &["0 Resolved group 0..55: 35..51 by 35..51 Unknown"],
                &["Composition VALUE unary_operator 15..34 0"],
            ],
        ),
        // n02_concatenation_on_the_next_row
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n<> \"more\"\ndef f(), do: :ok\ne",
                "nd"
            ),
            [
                &["module 0..58", "group 0..58: 38..54"],
                &["Primary Single [15..37] pending"],
                &["0 Resolved group 0..58: 38..54 by 38..54 Unknown"],
                &["Composition VALUE unary_operator 15..37 0"],
            ],
        ),
        // n03_when_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\nwhen x\ndef f(), do: :ok\nend",
            [
                &["module 0..55", "group 0..55: 35..51"],
                &["Primary Single [15..34] pending"],
                &["0 Resolved group 0..55: 35..51 by 35..51 Unknown"],
                &["Composition VALUE unary_operator 15..34 0"],
            ],
        ),
        // n04_do_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\ndo\nend\ndef f(), do: :ok\nend",
            [
                &["module 0..55", "group 0..55: 35..51"],
                &["Primary Single [15..34] pending"],
                &["0 Resolved group 0..55: 35..51 by 35..51 Unknown"],
                &["Composition VALUE unary_operator 15..34 0"],
            ],
        ),
        // n05_comment_row_then_pipe
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n# c\n|> g()\ndef f(), do: :ok\ne",
                "nd"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &["Primary Single [15..38] pending"],
                &["0 Resolved group 0..59: 39..55 by 39..55 Unknown"],
                &["Composition VALUE unary_operator 15..38 0"],
            ],
        ),
        // n06_blank_rows_then_pipe
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n\n\n|> g()\ndef f(), do: :ok\nen",
                "d"
            ),
            [
                &["module 0..57", "group 0..57: 37..53"],
                &["Primary Single [15..36] pending"],
                &["0 Resolved group 0..57: 37..53 by 37..53 Unknown"],
                &["Composition VALUE unary_operator 15..36 0"],
            ],
        ),
        // n07_not_in_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\nnot in x\ndef f(), do: :ok\nend",
            [
                &["module 0..57", "group 0..57: 37..53"],
                &["Primary Single [15..36] pending"],
                &["0 Resolved group 0..57: 37..53 by 37..53 Unknown"],
                &["Composition VALUE unary_operator 15..36 0"],
            ],
        ),
        // n08_dot_on_the_next_row
        (
            "defmodule M do\n@doc \"audit\"\n.g()\ndef f(), do: :ok\nend",
            [
                &["module 0..53", "group 0..53: 33..49"],
                &["Primary Single [15..32] pending"],
                &["0 Resolved group 0..53: 33..49 by 33..49 Unknown"],
                &["Composition VALUE unary_operator 15..32 0"],
            ],
        ),
    ]);
}

// A pending outcome: the selection among several attributes is a later unit.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_several_attributes_for_one_owner_form_one_pending_document() {
    assert_views(&[
        // i01_attribute_before_a_later_clause
        (
            concat!(
                "defmodule M do\n@doc \"old\"\ndef g(0), do: 0\n@doc \"new\"\ndef",
                " g(x), do: x\nend"
            ),
            [
                &["module 0..72", "group 0..72: 26..41, 53..68"],
                &["Primary AttributeSequence [15..25, 42..52] pending"],
                &["0 Resolved group 0..72: 26..41, 53..68 by 26..41 Unknown"],
                &["Selection SEVERAL unary_operator 15..25 0"],
            ],
        ),
        // i02_two_in_one_run
        (
            concat!(
                "defmodule M do\n@doc \"old\"\n@doc \"new\"\ndef f(), do: :ok\nen",
                "d"
            ),
            [
                &["module 0..57", "group 0..57: 37..53"],
                &["Primary AttributeSequence [15..25, 26..36] pending"],
                &["0 Resolved group 0..57: 37..53 by 37..53 Unknown"],
                &["Selection SEVERAL unary_operator 15..25 0"],
            ],
        ),
        // i03_metadata_then_text
        (
            concat!(
                "defmodule M do\n@doc since: \"1\"\n@doc \"audit\"\ndef f(), do: ",
                ":ok\nend"
            ),
            [
                &["module 0..64", "group 0..64: 44..60"],
                &["Primary AttributeSequence [15..30, 31..43] pending"],
                &["0 Resolved group 0..64: 44..60 by 44..60 Unknown"],
                &["Selection SEVERAL unary_operator 15..30 0"],
            ],
        ),
        // i04_false_then_text
        (
            concat!(
                "defmodule M do\n@doc false\n@doc \"audit\"\ndef f(), do: :ok\nen",
                "d"
            ),
            [
                &["module 0..59", "group 0..59: 39..55"],
                &["Primary AttributeSequence [15..25, 26..38] pending"],
                &["0 Resolved group 0..59: 39..55 by 39..55 Unknown"],
                &["Selection SEVERAL unary_operator 15..25 0"],
            ],
        ),
        // i05_two_moduledocs
        (
            concat!(
                "defmodule M do\n@moduledoc \"a\"\ndef f(), do: :ok\n@moduledoc ",
                "\"b\"\nend"
            ),
            [
                &["module 0..65", "group 0..65: 30..46"],
                &["FilePackage AttributeSequence [15..29, 47..61] pending"],
                &["0 Resolved module 0..65 by - Unknown"],
                &["Selection SEVERAL unary_operator 15..29 0"],
            ],
        ),
        // i06_binding_is_the_first_taker
        (
            concat!(
                "defmodule M do\ndef g(0), do: 0\n@doc \"old\"\ndef g(1), do: 1\n",
                "@doc \"new\"\ndef g(x), do: x\nend"
            ),
            [
                &["module 0..88", "group 0..88: 15..30, 42..57, 69..84"],
                &["Primary AttributeSequence [31..41, 58..68] pending"],
                &["0 Resolved group 0..88: 15..30, 42..57, 69..84 by 42..57 Unknown"],
                &["Selection SEVERAL unary_operator 31..41 0"],
            ],
        ),
    ]);
}

// A pending outcome: these attributes are assessed and have no document yet.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_attributes_outside_a_module_body_are_assessed() {
    assert_views(&[
        // j01_file_level
        (
            "@doc \"audit\"\ndef f(), do: :ok",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 0..12 -",
                    "Owner PROJECTION call 13..29 -",
                ],
            ],
        ),
        // j02_function_body
        (
            "defmodule M do\ndef g() do\n@doc \"audit\"\n:ok\nend\nend",
            [
                &["module 0..50", "group 0..50: 15..46"],
                &[],
                &[],
                &["Occurrence OUTSIDE unary_operator 26..38 -"],
            ],
        ),
        // j03_block_in_a_module_body
        (
            concat!(
                "defmodule M do\nif true do\n@doc \"audit\"\ndef f(), do: :ok\nen",
                "d\nend"
            ),
            [
                &["module 0..63"],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 26..38 -",
                    "Owner PROJECTION call 39..55 -",
                ],
            ],
        ),
        // j04_keyword_form_of_defmodule
        (
            "defmodule M, do: (@doc \"audit\"; def f(), do: :ok)",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 18..30 -",
                    "Owner PROJECTION call 0..49 -",
                    "Owner PROJECTION call 32..48 -",
                ],
            ],
        ),
        // j05_defprotocol
        (
            "defprotocol P do\n@doc \"audit\"\ndef f(x)\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 17..29 -",
                    "Owner PROJECTION call 0..42 -",
                    "Owner PROJECTION call 30..38 -",
                ],
            ],
        ),
        // j06_defimpl
        (
            "defimpl P, for: M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 21..33 -",
                    "Owner PROJECTION call 0..54 -",
                    "Owner PROJECTION call 34..50 -",
                ],
            ],
        ),
        // j07_moduledoc_and_typedoc_at_file_level
        (
            "@moduledoc \"m\"\n@typedoc \"t\"\n",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 0..14 -",
                    "Occurrence OUTSIDE unary_operator 15..27 -",
                ],
            ],
        ),
        // j08_attribute_as_an_operand
        (
            "defmodule M do\nx = @doc \"audit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..52", "group 0..52: 32..48"],
                &[],
                &[],
                &["Occurrence OUTSIDE unary_operator 19..31 -"],
            ],
        ),
        // j09_section_of_a_module_block
        (
            concat!(
                "defmodule M do\ndef f(), do: :ok\nelse\n@doc \"audit\"\ndef g(),",
                " do: 1\nend"
            ),
            [
                &["module 0..68", "group 0..68: 15..31"],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 37..49 -",
                    "Owner PROJECTION call 50..64 -",
                ],
            ],
        ),
    ]);
}

// A pending outcome: what a quoted attribute becomes is undecided.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_quoted_code_is_a_template() {
    assert_views(&[
        // k01_quote_in_a_module
        (
            concat!(
                "defmodule M do\nquote do\n@doc \"audit\"\ndef f(), do: :ok\nend",
                "\nend"
            ),
            [
                &["module 0..61"],
                &[],
                &[],
                &["Occurrence TEMPLATE unary_operator 24..36 -"],
            ],
        ),
        // k02_module_in_a_quote
        (
            concat!(
                "quote do\ndefmodule M do\n@moduledoc \"audit\"\n@doc \"x\"\ndef ",
                "f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence TEMPLATE unary_operator 24..42 -",
                    "Occurrence TEMPLATE unary_operator 43..51 -",
                ],
            ],
        ),
        // k03_keyword_form_of_quote
        (
            "quote do: (@doc \"audit\"; def f(), do: :ok)",
            [
                &[],
                &[],
                &[],
                &["Occurrence TEMPLATE unary_operator 11..23 -"],
            ],
        ),
        // k04_definition_after_a_quote
        (
            concat!(
                "defmodule M do\nquote do\n@doc \"t\"\ndef g(), do: 1\nend\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..85", "group 0..85: 65..81"],
                &["Primary Single [52..64] \"audit\" 0..5<-58..63"],
                &["0 Resolved group 0..85: 65..81 by 65..81 Unknown"],
                &["Occurrence TEMPLATE unary_operator 24..32 -"],
            ],
        ),
        // k05_module_after_a_quote
        (
            concat!(
                "quote do\nquote do\n@doc \"a\"\nend\nend\ndefmodule M do\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend"
            ),
            [
                &["module 35..83", "group 35..83: 63..79"],
                &["Primary Single [50..62] \"audit\" 0..5<-56..61"],
                &["0 Resolved group 35..83: 63..79 by 63..79 Unknown"],
                &["Occurrence TEMPLATE unary_operator 18..26 -"],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_statement_with_a_syntax_error_is_not_read() {
    assert_views(&[
        // l01_definition_with_an_error
        (
            concat!(
                "defmodule M do\n  @doc \"audit\"\n  def f(, do: :ok\n  @doc \"ne",
                "xt\"\n  def g(), do: :ok\nend\n"
            ),
            [
                &["group 0..84: 64..80"],
                &["Primary Single [50..61] \"next\" 0..4<-56..60"],
                &["0 Resolved group 0..84: 64..80 by 64..80 Unknown"],
                &[
                    "Owner PROJECTION call 0..84 -",
                    "Owner PROJECTION call 32..47 -",
                    "Owner UNOWNED unary_operator 17..29 -",
                ],
            ],
        ),
        // l07_defp_with_an_error
        (
            concat!(
                "defmodule M do\n  @doc \"audit\"\n  defp f(, do: :ok\n  @doc \"n",
                "ext\"\n  def g(), do: :ok\nend\n"
            ),
            [
                &["group 0..85: 65..81"],
                &["Primary Single [51..62] \"next\" 0..4<-57..61"],
                &["0 Resolved group 0..85: 65..81 by 65..81 Unknown"],
                &[
                    "Owner PROJECTION call 0..85 -",
                    "Owner PROJECTION call 32..48 -",
                    "Owner UNOWNED unary_operator 17..29 -",
                ],
            ],
        ),
        // l08_defmacro_with_an_error
        (
            concat!(
                "defmodule M do\n  @doc \"audit\"\n  defmacro f(, do: :ok\n  @doc",
                " \"next\"\n  def g(), do: :ok\nend\n"
            ),
            [
                &["group 0..89: 69..85"],
                &["Primary Single [55..66] \"next\" 0..4<-61..65"],
                &["0 Resolved group 0..89: 69..85 by 69..85 Unknown"],
                &[
                    "Owner PROJECTION call 0..89 -",
                    "Owner PROJECTION call 32..52 -",
                    "Owner UNOWNED unary_operator 17..29 -",
                ],
            ],
        ),
        // l09_defmacrop_with_an_error
        (
            concat!(
                "defmodule M do\n  @doc \"audit\"\n  defmacrop f(, do: :ok\n  @do",
                "c \"next\"\n  def g(), do: :ok\nend\n"
            ),
            [
                &["group 0..90: 70..86"],
                &["Primary Single [56..67] \"next\" 0..4<-62..66"],
                &["0 Resolved group 0..90: 70..86 by 70..86 Unknown"],
                &[
                    "Owner PROJECTION call 0..90 -",
                    "Owner PROJECTION call 32..53 -",
                    "Owner UNOWNED unary_operator 17..29 -",
                ],
            ],
        ),
        // l02_attribute_with_an_error
        (
            "defmodule M do\n@doc \"a\" \"b\"\ndef f(), do: :ok\nend",
            [
                &["group 0..48: 28..44"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 15..27 -",
                    "Owner PROJECTION call 0..48 -",
                ],
            ],
        ),
        // l03_moduledoc_with_an_error_ends_the_run
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@moduledoc \"a\" \"b\"\ndef f(),",
                " do: :ok\nend"
            ),
            [
                &["group 0..67: 47..63"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 28..46 -",
                    "Owner PROJECTION call 0..67 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // l04_module_without_its_end
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :",
                "ok\n"
            ),
            [
                &["group 0..60: 43..59"],
                &["Primary Single [30..42] \"audit\" 0..5<-36..41"],
                &["0 Resolved group 0..60: 43..59 by 43..59 Unknown"],
                &[
                    "Owner PROJECTION call 0..60 -",
                    "Owner UNOWNED unary_operator 15..29 -",
                ],
            ],
        ),
        // l05_error_node_between
        (
            "defmodule M do\n@doc \"audit\"\n1 def f(), do: :ok\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..50 -",
                    "Owner PROJECTION call 30..46 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // l06_other_attribute_with_an_error
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@spec f( :: atom()\ndef f(), do:",
                " :ok\nend"
            ),
            [
                &["group 0..67: 47..63"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..67 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // l11_error_row_between
        (
            "defmodule M do\n@doc \"audit\"\n)\ndef f(), do: :ok\nend",
            [
                &["group 0..50: 30..46"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..50 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // l12_nul_byte_in_a_comment
        (
            "defmodule M do\n@doc \"audit\" # c\u{0} d\ndef f(), do: :ok\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 15..51 -",
                    "Owner PROJECTION call 0..55 -",
                ],
            ],
        ),
        // l10_nul_byte_in_a_string
        (
            "defmodule M do\n@doc \"a\u{0}b\"\ndef f(), do: :ok\nend",
            [
                &["group 0..46: 26..42"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 15..25 -",
                    "Owner PROJECTION call 0..46 -",
                ],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_statement_needs_a_line_end_or_semicolon_before_it() {
    assert_views(&[
        // m01_definition_after_end_on_one_row
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do",
                ": :ok\nend"
            ),
            [
                &["group 0..67: 28..46"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..67: 28..46 by 28..46 Unknown"],
                &[
                    "Owner PROJECTION call 0..67 -",
                    "Owner PROJECTION call 47..63 -",
                ],
            ],
        ),
        // m02_attribute_after_end_on_one_row
        (
            concat!(
                "defmodule M do\ndef g() do\n:ok\nend @doc \"audit\"\ndef f(), do",
                ": :ok\nend"
            ),
            [
                &["group 0..67: 15..33", "group 0..67: 47..63"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 34..46 -",
                    "Owner PROJECTION call 0..67 -",
                ],
            ],
        ),
        // m03_definition_after_a_heredoc_on_one_row
        (
            "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..54 -",
                    "Owner PROJECTION call 34..50 -",
                    "Owner UNOWNED unary_operator 15..33 -",
                ],
            ],
        ),
        // m04_attribute_after_a_continued_row
        (
            concat!(
                "defmodule M do\ndef g() do\n:ok\nend \\\n@doc \"audit\"\ndef f()",
                ", do: :ok\nend"
            ),
            [
                &["group 0..69: 15..33", "group 0..69: 49..65"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 36..48 -",
                    "Owner PROJECTION call 0..69 -",
                ],
            ],
        ),
        // m05_definition_after_a_continued_crlf_row
        (
            concat!(
                "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" \\\r\ndef f(), do: :o",
                "k\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..57 -",
                    "Owner PROJECTION call 37..53 -",
                    "Owner UNOWNED unary_operator 15..33 -",
                ],
            ],
        ),
        // m06_definition_after_a_moduledoc_on_one_row
        (
            concat!(
                "defmodule M do\n@moduledoc \"\"\"\naudit\n\"\"\" def f(), do: :o",
                "k\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..60 -",
                    "Owner PROJECTION call 40..56 -",
                    "Owner UNOWNED unary_operator 15..39 -",
                ],
            ],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
fn elixir_error_node_above_a_module_leaves_its_associations_unresolved() {
    assert_views(&[
        // p01_module_under_an_error_node
        (
            concat!(
                "defmodule A do\n) ) (\ndefmodule M do\n@moduledoc \"m\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 21..84", "group 21..84: 64..80"],
                &[
                    "FilePackage Single [36..50] \"m\" 0..1<-48..49",
                    "Primary Single [51..63] \"audit\" 0..5<-57..62",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 21..84 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 21..84: 64..80 by 64..80 Unk",
                        "nown"
                    ),
                ],
                &["Owner PROJECTION call 0..11 -"],
            ],
        ),
    ]);
}

// These spellings set or define the same things for the compiler. The unit does not see
// them; the rows pin that nothing is claimed for them.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_spellings_the_unit_does_not_see_publish_nothing() {
    assert_views(&[
        // q01_qualified_attribute_and_definition
        (
            concat!(
                "defmodule M do\nKernel.@(doc \"audit\")\nKernel.def f(), do: :ok",
                "\nend"
            ),
            [&["module 0..64"], &[], &[], &[]],
        ),
        // q02_qualified_defmodule
        (
            "Kernel.defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 22..34 -",
                    "Owner PROJECTION call 35..51 -",
                ],
            ],
        ),
        // q03_attribute_set_by_a_call
        (
            concat!(
                "defmodule M do\nModule.put_attribute(__MODULE__, :doc, {1, \"aud",
                "it\"})\ndef f(), do: :ok\nend"
            ),
            [&["module 0..88", "group 0..88: 68..84"], &[], &[], &[]],
        ),
        // q04_macros_that_document
        (
            concat!(
                "defmodule M do\ndocs \"audit\"\nuse Documented, doc: \"audit\"\n",
                "def f(), do: :ok\nend"
            ),
            [&["module 0..77", "group 0..77: 57..73"], &[], &[], &[]],
        ),
        // q05_other_attribute_names
        (
            concat!(
                "defmodule M do\n@docs \"audit\"\n@moduledocs \"audit\"\n@typedoc",
                "s \"audit\"\ndef f(), do: :ok\nend"
            ),
            [&["module 0..87", "group 0..87: 67..83"], &[], &[], &[]],
        ),
        // q06_definitions_in_a_quote_and_under_an_operator
        (
            concat!(
                "quote do\ndef f(), do: :ok\ndefmodule M do\nend\nend\n@\ndef g()",
                ", do: 1\n"
            ),
            [&[], &[], &[], &[]],
        ),
    ]);
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_excluded_ranges_bar_attachment_and_hide_words() {
    let rows: [(&str, &[Range<usize>], Rows); 12] = [
        // o01_gap_between_attribute_and_definition
        (
            "defmodule M do\n@doc \"audit\"\n# x\ndef f(), do: :ok\nend",
            &[0..28, 32..52],
            [
                &["group 0..52: 32..48"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Unresolved(ExcludedGap) group 0..52: 32..48 by 32..48 Unknown"],
                &[
                    "Owner PROJECTION call 0..52 -",
                    "Owner PROJECTION do_block 12..52 -",
                ],
            ],
        ),
        // o02_later_whole_pair
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n# x\ndef f(), do: :ok\n@doc \"la",
                "ter\"\ndef g(), do: 1\nend"
            ),
            &[0..28, 32..80],
            [
                &["group 0..80: 32..48", "group 0..80: 62..76"],
                &[
                    "Primary Single [15..27] \"audit\" 0..5<-21..26",
                    "Primary Single [49..61] \"later\" 0..5<-55..60",
                ],
                &[
                    "0 Unresolved(ExcludedGap) group 0..80: 32..48 by 32..48 Unknown",
                    "1 Resolved group 0..80: 62..76 by 62..76 Unknown",
                ],
                &[
                    "Owner PROJECTION call 0..80 -",
                    "Owner PROJECTION do_block 12..80 -",
                ],
            ],
        ),
        // o03_gap_between_two_clauses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(0), do: 0\n# x\ndef f(x), ",
                "do: x\nend"
            ),
            &[0..44, 48..67],
            [
                &["group 0..67: 28..43, 48..63"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..67: 28..43, 48..63 by 28..43 Unknown"],
                &[
                    "Owner PROJECTION call 0..67 -",
                    "Owner PROJECTION do_block 12..67 -",
                ],
            ],
        ),
        // o04_gap_inside_a_definition
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(0), do: (\n# x\n0)\ndef f(",
                "x), do: x\nend"
            ),
            &[0..44, 48..70],
            [
                &["group 0..70: 51..66"],
                &[],
                &[],
                &[
                    "Owner PROJECTION arguments 32..50 -",
                    "Owner PROJECTION block 42..50 -",
                    "Owner PROJECTION call 0..70 -",
                    "Owner PROJECTION call 28..50 -",
                    "Owner PROJECTION do_block 12..70 -",
                    "Owner PROJECTION keywords 38..50 -",
                    "Owner PROJECTION pair 38..50 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // o08_statement_after_the_gap_releases_the_attribute
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n# x\nx = 1\n@doc \"later\"\ndef ",
                "g(), do: 1\nend"
            ),
            &[0..28, 32..69],
            [
                &["group 0..69: 51..65"],
                &["Primary Single [38..50] \"later\" 0..5<-44..49"],
                &["0 Resolved group 0..69: 51..65 by 51..65 Unknown"],
                &[
                    "Owner PROJECTION call 0..69 -",
                    "Owner PROJECTION do_block 12..69 -",
                    "Owner UNOWNED unary_operator 15..27 -",
                ],
            ],
        ),
        // o09_gap_with_no_attribute_waiting
        (
            concat!(
                "defmodule M do\ndef f(0), do: 0\n# x\n@doc \"later\"\ndef g(), d",
                "o: 1\nend"
            ),
            &[0..31, 35..66],
            [
                &["group 0..66: 15..30", "group 0..66: 48..62"],
                &["Primary Single [35..47] \"later\" 0..5<-41..46"],
                &["0 Resolved group 0..66: 48..62 by 48..62 Unknown"],
                &[
                    "Owner PROJECTION call 0..66 -",
                    "Owner PROJECTION do_block 12..66 -",
                ],
            ],
        ),
        // o05_gap_inside_the_attribute
        (
            "defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            &[0..23, 25..48],
            [
                &["group 0..48: 28..44"],
                &[],
                &[],
                &[
                    "Owner PROJECTION arguments 20..27 -",
                    "Owner PROJECTION call 0..48 -",
                    "Owner PROJECTION call 16..27 -",
                    "Owner PROJECTION do_block 12..48 -",
                    "Owner PROJECTION quoted_content 21..26 -",
                    "Owner PROJECTION string 20..27 -",
                    "Owner PROJECTION unary_operator 15..27 -",
                ],
            ],
        ),
        // o06_gap_inside_the_module_word
        (
            "defmoXXdule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            &[0..5, 7..50],
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence TEMPLATE unary_operator 17..29 -",
                    "Owner PROJECTION call 0..50 -",
                    "Owner PROJECTION identifier 0..11 -",
                ],
            ],
        ),
        // o10_gap_inside_an_attribute_at_file_level
        (
            "@doc \"audit\"\ndef f(), do: :ok",
            &[0..8, 10..29],
            [
                &[],
                &[],
                &[],
                &[
                    "Owner PROJECTION arguments 5..12 -",
                    "Owner PROJECTION call 1..12 -",
                    "Owner PROJECTION call 13..29 -",
                    "Owner PROJECTION quoted_content 6..11 -",
                    "Owner PROJECTION string 5..12 -",
                    "Owner PROJECTION unary_operator 0..12 -",
                ],
            ],
        ),
        // o07_adjacent_ranges
        (
            "defmodule M do\n@doc \"audit\"\ndef f(), do: :ok\nend",
            &[0..23, 23..48],
            [
                &["module 0..48", "group 0..48: 28..44"],
                &[concat!(
                    "Primary Single [15..23+23..27] \"audit\" 0..2<-21..23 2..5<-23..",
                    "26"
                )],
                &["0 Resolved group 0..48: 28..44 by 28..44 Unknown"],
                &[],
            ],
        ),
        // o11_gap_before_a_module_at_file_level
        (
            concat!(
                ":ok\n# x\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef ",
                "f(), do: :ok\nend"
            ),
            &[0..4, 8..71],
            [
                &["module 8..71", "group 8..71: 51..67"],
                &[
                    "FilePackage Single [23..37] \"x\" 0..1<-35..36",
                    "Primary Single [38..50] \"audit\" 0..5<-44..49",
                ],
                &[
                    "0 Resolved module 8..71 by - Unknown",
                    "1 Resolved group 8..71: 51..67 by 51..67 Unknown",
                ],
                &[],
            ],
        ),
        // o12_excluded_gap_after_an_escaped_line_feed
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@foo ?\\\n# x\ndef f(), do: :ok",
                "\nend"
            ),
            &[0..36, 40..60],
            [
                &["group 0..60: 40..56"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Unresolved(ExcludedGap) group 0..60: 40..56 by 40..56 Unknown"],
                &[
                    "Owner PROJECTION call 0..60 -",
                    "Owner PROJECTION do_block 12..60 -",
                ],
            ],
        ),
    ];
    for (source, included, rows) in rows {
        let tree = restricted(source, included);
        let index = collect(
            LANG::Elixir,
            &tree,
            source.as_bytes(),
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
        assert_eq!(published(&index), expected(rows), "{source} {included:?}");
    }
}

// The labels of the assessments that a query surfaces, sorted.
#[cfg(feature = "elixir")]
fn surfaced<'i, 's: 'i>(
    assessments: impl Iterator<Item = FactRef<'i, Assessment<'s>>>,
) -> Vec<&'static str> {
    let mut labels: Vec<_> = assessments
        .map(|assessment| {
            let reason = assessment.get().reason;
            REASONS
                .iter()
                .find(|(_, known)| *known == reason)
                .map_or(reason, |(label, _)| label)
        })
        .collect();
    labels.sort_unstable();
    labels
}

// The node of a written statement: the first `kind` node that spells `spelling`.
#[cfg(feature = "elixir")]
fn written<'t>(
    tree: &'t tree_sitter::Tree,
    source: &str,
    kind: &str,
    spelling: &str,
) -> tree_sitter::Node<'t> {
    let start = source.find(spelling).unwrap();
    node_at(tree, kind, &(start..start + spelling.len()))
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_node_and_owner_queries_answer_for_the_group_and_the_module() {
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    // Every clause answers for its group, and the module for itself.
    let source = "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f(x), do: x\nend";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let tree = parser.ts_tree();
    let association = index.associations().next().unwrap();
    let group = index.facts().associated_owner(association, 0).unwrap();
    for clause in ["def f(x)", "def f(0), do: 0", "def f(x), do: x"] {
        let clause = written(tree, source, "call", clause);
        let projection = index.node_projection(clause).unwrap();
        assert!(!projection.pending());
        assert!(projection.owners().eq([group]));
        assert!(index.node_owners(clause).unwrap().eq([group]));
        let query = index.node_associations(clause).unwrap();
        assert!(!query.projection_pending());
        assert!(query.associations().eq([association]));
        assert_eq!(query.assessments().count(), 0);
    }
    let owned = index.owner_associations(group).unwrap();
    assert!(owned.associations().eq([association]));
    assert_eq!(owned.qualifications().len(), 0);
    let module = index.node_associations(tree.root_node().child(0).unwrap());
    let module = module.unwrap();
    assert!(!module.projection_pending());
    assert_eq!(module.associations().len(), 0);
    assert_eq!(module.assessments().count(), 0);
    // An attribute, a body, a head and a token are no owners.
    for (kind, spelling) in [
        ("unary_operator", "@doc \"audit\""),
        ("do_block", &source[12..]),
        ("call", "f(0)"),
        ("identifier", "defmodule"),
        ("end", "end"),
    ] {
        let node = written(tree, source, kind, spelling);
        assert!(matches!(
            index.node_associations(node),
            Err(OwnerError::UnsupportedOwner)
        ));
        assert!(matches!(
            index.node_projection(node),
            Err(OwnerError::UnsupportedOwner)
        ));
        assert!(matches!(
            index.node_owners(node),
            Err(OwnerError::UnsupportedOwner)
        ));
    }
    // An attribute without an owner is surfaced by its module and by every group in it.
    let source = "defmodule M do\n@doc \"audit\"\nx = 1\ndef f(), do: :ok\nend";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let tree = parser.ts_tree();
    for node in [
        tree.root_node().child(0).unwrap(),
        written(tree, source, "call", "def f(), do: :ok"),
    ] {
        let query = index.node_associations(node).unwrap();
        assert!(!query.projection_pending());
        assert_eq!(query.associations().len(), 0);
        assert_eq!(surfaced(query.assessments()), ["UNOWNED"]);
    }
    // A definition that is not read is a pending owner.
    let source = "defmodule M do\n@doc \"audit\"\ndef a + b, do: a\nend";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let definition = written(parser.ts_tree(), source, "call", "def a + b, do: a");
    assert!(index.node_projection(definition).unwrap().pending());
    assert!(matches!(
        index.node_owners(definition),
        Err(OwnerError::PendingOwner)
    ));
    let query = index.node_associations(definition).unwrap();
    assert!(query.projection_pending());
    assert_eq!(surfaced(query.assessments()), ["PROJECTION", "UNOWNED"]);
    // A pending value is surfaced by the owner of its document.
    let source = "defmodule M do\n@doc false\ndefp f(), do: :ok\nend";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let clause = written(parser.ts_tree(), source, "call", "defp f(), do: :ok");
    let query = index.node_associations(clause).unwrap();
    assert_eq!(query.associations().len(), 1);
    assert_eq!(surfaced(query.assessments()), ["VALUE"]);
    // A module with a syntax error is a pending owner, and a quoted module is none.
    let source = "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :ok\n";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let module = parser.ts_tree().root_node().child(0).unwrap();
    assert!(module.has_error());
    assert!(index.node_projection(module).unwrap().pending());
    let source = "quote do\ndefmodule M do\n@moduledoc \"audit\"\nend\nend";
    let parser = parsed(source.as_bytes());
    let index = parser.documentation(&context, &options).unwrap();
    let module = written(
        parser.ts_tree(),
        source,
        "call",
        "defmodule M do\n@moduledoc \"audit\"\nend",
    );
    assert!(matches!(
        index.node_projection(module),
        Err(OwnerError::UnsupportedOwner)
    ));
}

#[test]
fn elixir_gap_string_and_name_scans_read_each_byte_once() {
    // (the bytes between two statements, whether `?\` or `?` takes the first line
    // feed, whether a line feed counts, whether they separate them, the bytes read)
    let gaps: [(&[u8], bool, bool, bool, usize); 21] = [
        // expected: a line end; the scan stops there.
        (b"\n", false, true, true, 1),
        (b"  \n  ", false, true, true, 3),
        (b"\r\n", false, true, true, 2),
        // expected: no line end and no `;`.
        (b"", false, true, false, 0),
        (b"  ", false, true, false, 2),
        // expected: a backslash continues the line over LF and over CR LF.
        (b" \\\n", false, true, false, 3),
        (b" \\\r\n", false, true, false, 4),
        // expected: the line end after a continued one is real.
        (b" \\\n\n", false, true, true, 4),
        // expected: a backslash that is not before the line end continues nothing.
        (b"\\ \n", false, true, true, 3),
        (b";", false, true, true, 1),
        (b" ; \n", false, true, true, 2),
        // expected: a comment runs to its line end, which no backslash continues, and
        // a `;` inside it separates nothing.
        (b" # c; \\\n", false, true, true, 8),
        (b" # c;", false, true, false, 5),
        (b" \\\n# c\n", false, true, true, 7),
        // expected: `?\` or `?` takes the first line feed; a second line feed, a `;` or
        // a comment after it separates.
        (b"\n  ", true, true, false, 3),
        (b"\n\n", true, true, true, 2),
        (b"\n;", true, true, true, 2),
        (b"\n# c\n", true, true, true, 5),
        // expected: behind a `,` token no line feed counts; a `;` outside a comment
        // does, and a line feed still ends a comment before `;`.
        (b"\n\n", false, false, false, 2),
        (b" # c;\n", false, false, false, 6),
        (b" # c\n;", false, false, true, 6),
    ];
    for (gap, escaped, lines, apart, read) in gaps {
        let budget = Counting::default();
        assert_eq!(
            separated(gap, escaped, lines, &budget),
            Ok(apart),
            "{gap:?}"
        );
        assert_eq!(budget.scanned.get(), read, "{gap:?}");
        assert_eq!(budget.steps.get() + budget.checkpoints.get(), 0, "{gap:?}");
    }
    // (the bytes between two quotes, whether the compiler rejects one, the bytes read)
    let texts: [(&str, bool, usize); 20] = [
        ("", false, 0),
        ("audit", false, 5),
        // expected: LF and CR LF are kept; a CR without LF is rejected.
        ("a\nb", false, 3),
        ("a\r\nb", false, 4),
        ("a\rb", true, 2),
        ("a\r", true, 2),
        ("\u{b}", true, 1),
        ("\u{c}", true, 1),
        // expected: U+0085, U+2028 and U+2029 are line breaks; their neighbours are text.
        ("\u{84}", false, 2),
        ("\u{85}", true, 1),
        ("\u{86}", false, 2),
        ("\u{2027}", false, 3),
        ("\u{2028}", true, 1),
        ("\u{2029}", true, 1),
        // expected: U+202A to U+202E and U+2066 to U+2069 are bidirectional controls.
        ("\u{202a}", true, 1),
        ("\u{202e}", true, 1),
        ("\u{202f}", false, 3),
        ("\u{2065}", false, 3),
        ("\u{2066}x\u{2069}", true, 1),
        ("\u{206a}", false, 3),
    ];
    for (text, found, read) in texts {
        let budget = Counting::default();
        assert_eq!(rejected(text.as_bytes(), &budget), Ok(found), "{text:?}");
        assert_eq!(budget.scanned.get(), read, "{text:?}");
        assert_eq!(budget.steps.get() + budget.checkpoints.get(), 0, "{text:?}");
    }
    // (two names, whether they are the same bytes, the bytes read)
    let names: [(&str, &str, bool, usize); 6] = [
        ("ab", "ab", true, 2),
        // expected: the comparison stops at the first byte that differs.
        ("ab", "ac", false, 2),
        ("xb", "ab", false, 1),
        // expected: names of two lengths differ, and no byte is read.
        ("ab", "abc", false, 0),
        ("abc", "ab", false, 0),
        ("", "", true, 0),
    ];
    for (left, right, equal, read) in names {
        let source = format!("{left}{right}");
        let budget = Counting::default();
        let found = same(
            source.as_bytes(),
            0..left.len(),
            left.len()..source.len(),
            &budget,
        );
        assert_eq!(found, Ok(equal), "{left} {right}");
        assert_eq!(budget.scanned.get(), read, "{left} {right}");
        assert_eq!(
            budget.steps.get() + budget.checkpoints.get(),
            0,
            "{left} {right}"
        );
    }
}

// `count` clauses of one function.
#[cfg(feature = "elixir")]
fn clauses(count: usize) -> String {
    format!("defmodule M do\n{}end", "def f(x), do: x\n".repeat(count))
}

// `count` documented functions of one clause each.
#[cfg(feature = "elixir")]
fn functions(count: usize) -> String {
    let mut source = "defmodule M do\n".to_owned();
    for at in 0..count {
        writeln!(source, "@doc \"d\"\ndef f{at}(x), do: x").unwrap();
    }
    source + "end"
}

// `count` attributes before one definition.
#[cfg(feature = "elixir")]
fn attributes(count: usize) -> String {
    format!(
        "defmodule M do\n{}def f(), do: :ok\nend",
        "@doc \"d\"\n".repeat(count)
    )
}

// `count` attributes at file level.
#[cfg(feature = "elixir")]
fn strays(count: usize) -> String {
    "@doc \"d\"\n".repeat(count)
}

// Two clauses of a function with `count` arguments and a name of `count` letters.
#[cfg(feature = "elixir")]
fn wide(count: usize) -> String {
    let head = format!("{}({})", "n".repeat(count), vec!["x"; count].join(", "));
    format!("defmodule M do\ndef {head}, do: 0\ndef {head}, do: 1\nend")
}

// One documented function with `count` comment rows in the parentheses of its attribute
// and as many in its argument list.
#[cfg(feature = "elixir")]
fn commented(count: usize) -> String {
    let rows = "# c\n".repeat(count);
    format!("defmodule M do\n@doc({rows}\"audit\")\ndef f({rows}x), do: x\nend")
}

// `count` statements at file level.
#[cfg(feature = "elixir")]
fn statements(count: usize) -> String {
    "x = 1\n".repeat(count)
}

// `count` rows, each a module that is joined to the expression before it.
#[cfg(feature = "elixir")]
fn joined(count: usize) -> String {
    "x = 1 defmodule X do\n@doc \"d\"\nend\n".repeat(count)
}

// One statement: `count` operators deep, with a syntax error in the block at its end.
#[cfg(feature = "elixir")]
fn chain(count: usize) -> String {
    format!("{}(1 1)", "a ++ ".repeat(count))
}

// `count` blocks, each in the one before, with a syntax error in the last.
#[cfg(feature = "elixir")]
fn blocks(count: usize) -> String {
    format!("{}1 1{}", "(".repeat(count), ")".repeat(count))
}

// One documented function with `count` spaces after its attribute.
#[cfg(feature = "elixir")]
fn gapped(count: usize) -> String {
    format!(
        "defmodule M do\n@doc \"audit\"{}\ndef f(), do: :ok\nend",
        " ".repeat(count)
    )
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_pass_charges_every_loop_and_byte_scan() {
    // Each row is the steps, bytes and checkpoints of the second pass, as the charge
    // table of the pass gives them for the parse of the source: a step for every node
    // and for every child pushed, for every child that a count or a place of children
    // passes, for every child that is compared with the statement before it, for every
    // node searched for a recovery, for every statement of a body, for every coverage
    // check and for every record filed; a byte for every byte of a gap, of a name
    // hashed or compared and of a text scanned, and two for every byte of text copied.
    let fixed: [(&str, [usize; 3]); 47] = [
        // a01_primary
        // expected: steps: walk 59 + children 38 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text 10.
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            [128, 17, 2],
        ),
        // b03_moduledoc_then_doc
        // expected: steps: walk 77 + children 55 + gaps 6 + tail 8 + statements 5 + coverage 10
        //     + filed 4 + pieces 10 + copy 2; bytes: gap 2 + name 1 + scan 2 + text 4.
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"d\"\ndef f(), do: :ok\n",
                "end"
            ),
            [177, 9, 4],
        ),
        // c01_head_and_clauses
        // expected: steps: walk 107 + children 69 + gaps 7 + tail 12 + statements 6 + coverage
        //     18 + arguments 3 + candidates 2 + filed 5 + pieces 10 + copy 1; bytes: gap 3 +
        //     name 5 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f",
                "(x), do: x\nend"
            ),
            [240, 23, 2],
        ),
        // c08_written_arguments_are_counted
        // expected: steps: walk 197 + children 113 + gaps 9 + tail 20 + statements 8 + coverage
        //     28 + arguments 13 + candidates 2 + filed 9 + pieces 14 + copy 1; bytes: gap 5 +
        //     name 7 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(a, # c\n b), do: a\ndef f(",
                "a, b), do: b\ndef g(a, b: 1), do: a\ndef g(a, b), do: b\ndef f(a",
                "), do: a\nend"
            ),
            [414, 27, 2],
        ),
        // c09_names_of_one_length
        // expected: steps: walk 117 + children 71 + gaps 7 + tail 12 + statements 6 + coverage
        //     18 + arguments 3 + candidates 1 + filed 6 + pieces 10 + copy 1; bytes: gap 3 +
        //     name 8 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef ab(x), do: x\ndef ac(x), do:",
                " x\ndef ab(y), do: y\nend"
            ),
            [252, 26, 2],
        ),
        // e01_semicolon_comment_blank_row_and_spec
        // expected: steps: walk 97 + children 51 + gaps 8 + tail 10 + statements 7 + coverage 12
        //     + filed 3 + pieces 6 + copy 1; bytes: gap 2 + name 1 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"; # note\n\n@spec f() :: atom()\nd",
                "ef f(), do: :ok\nend"
            ),
            [195, 18, 2],
        ),
        // e10_comment_after_the_operator
        // expected: steps: walk 61 + children 42 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text 10.
        (
            "defmodule M do\n@ # c\ndoc \"audit\"\ndef f(), do: :ok\nend",
            [134, 17, 2],
        ),
        // f08_two_attributes_released
        // expected: steps: walk 85 + children 55 + gaps 7 + tail 9 + statements 6 + coverage 12
        //     + marks 2 + filed 2 + pieces 2; bytes: gap 3 + name 1.
        (
            concat!(
                "defmodule M do\n@doc \"a\"\n@doc \"b\"\nx = 1\ndef f(), do: :ok",
                "\nend"
            ),
            [180, 4, 0],
        ),
        // g04_operator_head
        // expected: steps: walk 83 + children 53 + gaps 6 + tail 8 + statements 5 + coverage 12
        //     + marks 2 + filed 2 + pieces 2; bytes: gap 2 + name 1.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef a + b, do: a\ndef f(), do: :",
                "ok\nend"
            ),
            [173, 3, 0],
        ),
        // h01_empty_string
        // expected: steps: walk 57 + children 37 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 4; bytes: gap 1 + name 1.
        (
            "defmodule M do\n@doc \"\"\ndef f(), do: :ok\nend",
            [122, 2, 0],
        ),
        // h10_heredoc
        // expected: steps: walk 59 + children 38 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 4; bytes: gap 1 + name 1.
        (
            concat!(
                "defmodule M do\n@doc \"\"\"\naudit\n\"\"\"\ndef f(), do: :ok\nen",
                "d"
            ),
            [125, 2, 0],
        ),
        // h23_lone_cr_inside
        // expected: steps: walk 59 + children 38 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 4; bytes: gap 1 + name 1 + scan 2.
        (
            "defmodule M do\n@doc \"a\rb\"\ndef f(), do: :ok\nend",
            [125, 4, 0],
        ),
        // i02_two_in_one_run
        // expected: steps: walk 77 + children 55 + gaps 6 + tail 8 + statements 5 + coverage 10
        //     + filed 4 + pieces 6; bytes: gap 2 + name 1.
        (
            concat!(
                "defmodule M do\n@doc \"old\"\n@doc \"new\"\ndef f(), do: :ok\nen",
                "d"
            ),
            [171, 3, 0],
        ),
        // j01_file_level
        // expected: steps: walk 45 + children 11 + gaps 2 + tail 4 + coverage 4; bytes: gap 1.
        ("@doc \"audit\"\ndef f(), do: :ok", [66, 1, 0]),
        // k02_module_in_a_quote
        // expected: steps: walk 87 + children 16 + gaps 1 + coverage 3.
        (
            concat!(
                "quote do\ndefmodule M do\n@moduledoc \"audit\"\n@doc \"x\"\ndef ",
                "f(), do: :ok\nend\nend"
            ),
            [107, 0, 0],
        ),
        // l02_attribute_with_an_error
        // expected: steps: tear 28 + walk 71 + children 30 + gaps 19 + tail 7 + recovery 37 +
        //     statements 4 + coverage 8 + marks 2 + filed 2 + pieces 2; bytes: gap 2 + name 1.
        (
            "defmodule M do\n@doc \"a\" \"b\"\ndef f(), do: :ok\nend",
            [210, 3, 0],
        ),
        // l04_module_without_its_end
        // expected: steps: tear 13 + walk 77 + children 55 + gaps 9 + tail 8 + recovery 13 +
        //     statements 5 + coverage 9 + marks 2 + filed 3 + pieces 6 + copy 1; bytes: gap 2 +
        //     name 1 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :",
                "ok\n"
            ),
            [201, 18, 2],
        ),
        // m01_definition_after_end_on_one_row
        // expected: steps: tear 12 + walk 83 + children 43 + gaps 12 + tail 6 + recovery 13 +
        //     statements 5 + coverage 11 + marks 2 + filed 3 + pieces 6 + copy 1; bytes: gap 2 +
        //     name 1 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do",
                ": :ok\nend"
            ),
            [197, 18, 2],
        ),
        // h30_comment_in_parentheses
        // expected: steps: walk 65 + children 44 + gaps 5 + tail 3 + statements 4 + coverage 8 +
        //     filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text 10.
        (
            "defmodule M do\n@doc(# c\n\"audit\")\ndef f(), do: :ok\nend",
            [139, 17, 2],
        ),
        // p01_module_under_an_error_node
        // expected: steps: tear 12 + walk 99 + children 59 + gaps 14 + tail 9 + recovery 6 +
        //     statements 5 + coverage 12 + filed 4 + pieces 10 + copy 2; bytes: gap 3 + name 1 +
        //     scan 6 + text 12.
        (
            concat!(
                "defmodule A do\n) ) (\ndefmodule M do\n@moduledoc \"m\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [232, 22, 4],
        ),
        // r01_doc_read_with_a_comment
        // expected: steps: walk 57 + children 32 + gaps 5 + tail 3 + statements 4 + coverage 8 +
        //     filed 2 + pieces 2; bytes: gap 1 + name 1.
        (
            "defmodule M do\n@doc( # c\n)\ndef f(), do: :ok\nend",
            [113, 2, 0],
        ),
        // r05_read_before_a_real_attribute
        // expected: steps: walk 75 + children 49 + gaps 6 + tail 7 + statements 5 + coverage 10
        //     + filed 3 + pieces 6 + copy 1; bytes: gap 2 + name 1 + scan 5 + text 10.
        (
            concat!(
                "defmodule M do\n@doc( # c\n)\n@doc \"audit\"\ndef f(), do: :ok\n",
                "end"
            ),
            [162, 18, 2],
        ),
        // t01_comment_before_the_do_of_a_module
        // expected: steps: walk 61 + children 40 + gaps 5 + tail 4 + statements 4 + coverage 8 +
        //     filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text 10.
        (
            "defmodule M # c\ndo\n@doc \"audit\"\ndef f(), do: :ok\nend",
            [132, 17, 2],
        ),
        // t03_comment_after_when
        // expected: steps: walk 79 + children 45 + gaps 5 + tail 4 + statements 4 + coverage 9 +
        //     arguments 1 + filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text
        //     10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when # c\n is_atom(x), ",
                "do: x\nend"
            ),
            [157, 17, 2],
        ),
        // t10_two_comments_in_a_guarded_macro
        // expected: steps: walk 79 + children 49 + gaps 8 + tail 4 + statements 4 + coverage 9 +
        //     arguments 1 + filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text
        //     10.
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefmacro m(x) # c\n when is_atom",
                "(x) # d\n do\nx\nend\nend"
            ),
            [164, 17, 2],
        ),
        // s01_module_after_a_module_at_file_level
        // expected: steps: tear 3 + walk 109 + children 40 + gaps 5 + tail 2 + statements 3 +
        //     coverage 7 + filed 1 + pieces 4 + copy 1; bytes: gap 1 + scan 1 + text 2.
        (
            concat!(
                "defmodule A do\n@moduledoc \"a\"\nend defmodule X do\n@moduledoc",
                " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend"
            ),
            [175, 4, 2],
        ),
        // s02_module_after_a_definition_in_a_body
        // expected: steps: tear 11 + walk 115 + children 42 + gaps 11 + tail 2 + recovery 12 +
        //     statements 4 + coverage 11 + marks 1 + filed 2 + pieces 2; bytes: gap 1 + name 1.
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [213, 2, 0],
        ),
        // s13_module_after_an_expression_at_file_level
        // expected: steps: tear 9 + walk 87 + children 17 + gaps 6 + tail 1 + recovery 10 +
        //     coverage 4; bytes: gap 1.
        (
            concat!(
                "x = 1 defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            [134, 1, 0],
        ),
        // s14_module_after_a_string
        // expected: steps: tear 25 + walk 109 + children 23 + gaps 16 + tail 1 + recovery 21 +
        //     statements 3 + coverage 6 + marks 1; bytes: gap 1.
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            [205, 1, 0],
        ),
        // u05_module_as_an_argument_of_a_call
        // expected: steps: walk 93 + children 57 + gaps 6 + tail 8 + statements 5 + coverage 10
        //     + filed 4 + pieces 10 + copy 2; bytes: gap 2 + name 1 + scan 6 + text 12.
        (
            concat!(
                "IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
                " f(), do: :ok\nend)"
            ),
            [195, 21, 4],
        ),
        // v01_expression_read_into_a_match
        // expected: steps: tear 8 + walk 85 + children 55 + gaps 10 + tail 8 + recovery 8 +
        //     statements 5 + coverage 11 + filed 4 + pieces 10 + copy 2; bytes: gap 3 + name 1 +
        //     scan 6 + text 12.
        (
            concat!(
                "1 y = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            [206, 22, 4],
        ),
        // v05_error_in_a_head_and_a_sound_statement_after_it
        // expected: steps: tear 23 + walk 179 + children 114 + gaps 25 + tail 22 + recovery 25 +
        //     statements 11 + coverage 24 + marks 3 + filed 7 + pieces 16 + copy 3; bytes: gap 5
        //     + name 2 + scan 9 + text 18.
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef a(, do: (\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\n)\n@doc \"",
                "two\"\ndef b(), do: :ok\nend"
            ),
            [452, 34, 6],
        ),
        // v06_recovery_among_the_clauses_of_a_function
        // expected: steps: tear 14 + walk 115 + children 55 + gaps 16 + tail 11 + recovery 12 +
        //     statements 5 + coverage 12 + filed 4 + pieces 10 + copy 2; bytes: gap 4 + name 1 +
        //     scan 6 + text 12.
        (
            concat!(
                "f = fn\n1 1 -> :ok\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [256, 23, 4],
        ),
        // v07_attribute_read_into_the_next_definition
        // expected: steps: tear 20 + walk 121 + children 64 + gaps 21 + tail 9 + recovery 28 +
        //     statements 5 + coverage 13 + filed 4 + pieces 10 + copy 2; bytes: gap 3 + name 1 +
        //     scan 6 + text 12.
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [297, 22, 4],
        ),
        // w01_string_without_its_end
        // expected: steps: tear 21 + walk 89 + children 21 + gaps 16 + recovery 22 + statements
        //     5 + coverage 7 + marks 3.
        (
            concat!(
                "defmodule M do\nx = \"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc \"",
                "REAL\"\ndef real, do: :ok\nend"
            ),
            [184, 0, 0],
        ),
        // w03_statements_before_the_tear_are_read
        // expected: steps: tear 22 + walk 147 + children 53 + gaps 17 + tail 8 + recovery 23 +
        //     statements 7 + coverage 14 + marks 3 + filed 3 + pieces 6 + copy 1; bytes: gap 2 +
        //     name 1 + scan 3 + text 6.
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef a(), do: :ok\nx = \"s\n@doc \"",
                "two\"\ndef b(), do: :ok\n@doc \"three\"\ndef c(), do: :ok\nend"
            ),
            [304, 12, 2],
        ),
        // x01_escaped_line_feed_at_file_level
        // expected: steps: walk 79 + children 17 + gaps 2 + coverage 4; bytes: gap 1.
        (
            concat!(
                "?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [102, 1, 0],
        ),
        // x11_escaped_line_feed_between_clauses_of_a_function
        // expected: steps: walk 105 + children 14 + gaps 6 + tail 2 + coverage 3; bytes: gap 1.
        (
            concat!(
                "fn\n1 -> ?\\\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audi",
                "t\"\ndef f(), do: :ok\nend\nend"
            ),
            [130, 1, 0],
        ),
        // x17_assigned_escaped_line_feed_at_file_level
        // expected: steps: walk 85 + children 17 + gaps 2 + tail 1 + coverage 4; bytes: gap 1.
        (
            concat!(
                "x = ?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
                "ion\"\n  def f(), do: :ok\nend\n"
            ),
            [109, 1, 0],
        ),
        // x19_bare_question_mark_and_line_feed_at_file_level
        // expected: steps: tear 5 + walk 81 + children 17 + gaps 3 + tail 1 + recovery 2 +
        //     coverage 4; bytes: gap 1.
        (
            concat!(
                "?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [113, 1, 0],
        ),
        // x21_bare_question_mark_after_a_character_token
        // expected: steps: tear 6 + walk 83 + children 17 + gaps 4 + tail 1 + recovery 3 +
        //     coverage 4; bytes: gap 1.
        (
            concat!(
                "???\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [118, 1, 0],
        ),
        // x23_line_feed_after_a_comma
        // expected: steps: tear 6 + walk 83 + children 17 + gaps 4 + tail 1 + recovery 3 +
        //     coverage 4; bytes: gap 1.
        (
            concat!(
                "foo,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [118, 1, 0],
        ),
        // y01_second_line_feed_after_an_escaped_line_feed
        // expected: steps: walk 79 + children 55 + gaps 7 + tail 8 + statements 5 + coverage 11
        //     + filed 4 + pieces 10 + copy 2; bytes: gap 4 + name 1 + scan 14 + text 28.
        (
            concat!(
                "?\\\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [181, 47, 4],
        ),
        // y28_semicolon_after_a_comment_after_a_comma
        // expected: steps: tear 8 + walk 87 + children 55 + gaps 11 + tail 10 + recovery 2 +
        //     statements 5 + coverage 12 + filed 4 + pieces 10 + copy 2; bytes: gap 13 + name 1
        //     + scan 14 + text 28.
        (
            concat!(
                "foo, # c\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"fun",
                "ction\"\n  def f(), do: :ok\nend\n"
            ),
            [206, 56, 4],
        ),
        // z01_comment_with_a_rejected_character
        // expected: steps: walk 61 + children 38 + gaps 6 + tail 4 + statements 5 + coverage 8 +
        //     filed 3 + pieces 6 + copy 1; bytes: gap 8 + name 1 + scan 5 + text 10.
        (
            "defmodule M do\n@doc \"audit\" # c\u{c} d\ndef f(), do: :ok\nend",
            [132, 24, 2],
        ),
        // z04_empty_clause_body
        // expected: steps: tear 23 + walk 113 + children 25 + gaps 21 + tail 10 + recovery 28 +
        //     coverage 9; bytes: gap 3.
        (
            concat!(
                "case x do\n  a ->\n  b -> defmodule X do\n    @moduledoc \"m\"\n",
                "    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            [229, 3, 0],
        ),
        // z05_operator_row_behind_an_escaped_line_feed
        // expected: steps: walk 83 + children 16 + gaps 2 + coverage 3; bytes: gap 1.
        (
            concat!(
                "?\\\n- defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [104, 1, 0],
        ),
    ];
    for (source, charges) in fixed {
        assert_eq!(charged(source.as_bytes()), charges, "{source}");
    }
    // Twice the size costs in proportion: no loop of the pass is worse than linear.
    let grown: [(String, [usize; 3]); 20] = [
        // expected: steps: walk 239 + children 134 + gaps 11 + tail 28 + statements 10 +
        //     coverage 41 + arguments 8 + candidates 7 + filed 9 + pieces 16; bytes: gap 7 +
        //     name 15.
        (clauses(8), [503, 22, 0]),
        // expected: steps: walk 1807 + children 1030 + gaps 67 + tail 252 + statements 66 +
        //     coverage 321 + arguments 64 + candidates 63 + filed 65 + pieces 128; bytes: gap 63
        //     + name 127.
        (clauses(64), [3863, 190, 0]),
        // expected: steps: walk 383 + children 270 + gaps 19 + tail 60 + statements 18 +
        //     coverage 57 + arguments 8 + filed 24 + pieces 48 + copy 8; bytes: gap 15 + name 16
        //     + scan 8 + text 16.
        (functions(8), [895, 55, 16]),
        // expected: steps: walk 2959 + children 2118 + gaps 131 + tail 508 + statements 130 +
        //     coverage 449 + arguments 64 + filed 192 + pieces 384 + copy 64; bytes: gap 127 +
        //     name 182 + scan 64 + text 128.
        (functions(64), [6999, 501, 128]),
        // expected: steps: walk 185 + children 157 + gaps 12 + tail 32 + statements 11 +
        //     coverage 22 + filed 10 + pieces 18; bytes: gap 8 + name 1.
        (attributes(8), [447, 9, 0]),
        // expected: steps: walk 1193 + children 1109 + gaps 68 + tail 256 + statements 67 +
        //     coverage 134 + filed 66 + pieces 130; bytes: gap 64 + name 1.
        (attributes(64), [3023, 65, 0]),
        // expected: steps: walk 145 + children 56 + gaps 8 + tail 28 + coverage 15; bytes: gap
        //     7.
        (strays(8), [252, 7, 0]),
        // expected: steps: walk 1153 + children 448 + gaps 64 + tail 252 + coverage 127; bytes:
        //     gap 63.
        (strays(64), [2044, 63, 0]),
        // expected: steps: walk 127 + children 66 + gaps 5 + tail 4 + statements 4 + coverage 11
        //     + arguments 30 + candidates 1 + filed 3 + pieces 4; bytes: gap 1 + name 24.
        (wide(8), [255, 25, 0]),
        // expected: steps: walk 575 + children 290 + gaps 5 + tail 4 + statements 4 + coverage
        //     11 + arguments 254 + candidates 1 + filed 3 + pieces 4; bytes: gap 1 + name 192.
        (wide(64), [1151, 193, 0]),
        // expected: steps: walk 97 + children 67 + gaps 5 + tail 3 + statements 4 + coverage 8 +
        //     arguments 1 + filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text
        //     10.
        (commented(8), [195, 17, 2]),
        // expected: steps: walk 321 + children 235 + gaps 5 + tail 3 + statements 4 + coverage 8
        //     + arguments 1 + filed 3 + pieces 6 + copy 1; bytes: gap 1 + name 1 + scan 5 + text
        //     10.
        (commented(64), [587, 17, 2]),
        // expected: steps: walk 65 + gaps 8 + tail 7 + coverage 7; bytes: gap 7.
        (statements(8), [87, 7, 0]),
        // expected: steps: walk 513 + gaps 64 + tail 63 + coverage 63; bytes: gap 63.
        (statements(64), [703, 63, 0]),
        // expected: steps: tear 65 + walk 337 + children 80 + gaps 48 + tail 29 + recovery 80 +
        //     coverage 31; bytes: gap 15.
        (joined(8), [670, 15, 0]),
        // expected: steps: tear 513 + walk 2689 + children 640 + gaps 384 + tail 253 + recovery
        //     640 + coverage 255; bytes: gap 127.
        (joined(64), [5374, 127, 0]),
        // expected: steps: tear 41 + walk 61 + gaps 30 + tail 1 + recovery 62 + coverage 1.
        (chain(8), [196, 0, 0]),
        // expected: steps: tear 265 + walk 397 + gaps 198 + tail 1 + recovery 454 + coverage 1.
        (chain(64), [1316, 0, 0]),
        // expected: steps: tear 37 + walk 55 + gaps 27 + tail 1 + recovery 27 + coverage 1.
        (blocks(8), [148, 0, 0]),
        // expected: steps: tear 261 + walk 391 + gaps 195 + tail 1 + recovery 195 + coverage 1.
        (blocks(64), [1044, 0, 0]),
    ];
    for (source, charges) in grown {
        let lead = &source[..source.len().min(48)];
        assert_eq!(charged(source.as_bytes()), charges, "{lead}");
    }
}

#[cfg(feature = "elixir")]
#[test]
fn elixir_pass_charges_the_collection_budget() {
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let polls = |source: &str| {
        let parser = parsed(source.as_bytes());
        let before = observations().polls;
        parser.documentation(&context, &options).unwrap();
        observations().polls - before
    };
    // expected: the 65,536 bytes between the attribute and the definition are read one
    // by one under the collection budget, which polls once per 16,384 bytes. The two
    // sources have the same tree, so nothing else differs.
    let source = gapped(65_535);
    let checkpoints = polls(&source);
    assert_eq!(checkpoints - polls(&gapped(0)), 4);
    // A cancellation at any checkpoint ends the collection without an index.
    let parser = parsed(source.as_bytes());
    let nodes = parser.root_node().preorder().count();
    let mut scanning = 0;
    for poll in 1..=checkpoints {
        let before = observations();
        let result = cancel_after(poll, || parser.documentation(&context, &options));
        assert!(matches!(result, Err(CollectionError::Cancelled)));
        let after = observations();
        // The walk has entered every node and only the file is an owner: the pass is
        // reading the body, before the module becomes an owner.
        if after.visits - before.visits == nodes
            && after.owner_projections - before.owner_projections == 1
        {
            scanning += 1;
        }
    }
    // expected: the four checkpoints of the gap.
    assert_eq!(scanning, 4);
    let index = cancel_after(checkpoints + 1, || parser.documentation(&context, &options));
    assert_eq!(index.unwrap().documents().len(), 1);
    // The next plain collection is whole.
    let rows: Rows = [
        &["module 0..52", "group 0..52: 32..48"],
        &["Primary Single [17..29] \"audit\" 0..5<-23..28"],
        &["0 Resolved group 0..52: 32..48 by 32..48 Unknown"],
        &[],
    ];
    assert_eq!(
        view("defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend"),
        expected(rows)
    );
}

// The pass reads words of Elixir. A Ruby call that starts with such a word is no
// definition, and the pass must not reach it.
#[cfg(feature = "ruby")]
#[test]
fn elixir_pass_runs_for_the_elixir_profile_only() {
    let source = b"defguard(x)\ndefp y\n";
    let parser = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None).unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(index.profile(), Profile::Yard);
    let root = parser.ts_tree().root_node();
    assert!(!root.has_error());
    assert_eq!(root.named_child_count(), 2);
    for at in 0..2 {
        // Control: each statement has the shape the pass reads as a definition.
        let call = root.named_child(at).unwrap();
        assert_eq!(call.kind(), "call");
        assert_eq!(call.child(0).unwrap().kind(), "identifier");
        assert!(matches!(
            index.node_projection(call),
            Err(OwnerError::UnsupportedOwner)
        ));
    }
    assert_eq!(index.assessments().len(), 0);
}

// The compiler drops a comment, so an argument list that holds only comments is the
// empty one: the attribute is read, and nothing is set.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_attribute_read_with_a_comment_sets_nothing() {
    assert_views(&[
        // r01_doc_read_with_a_comment
        (
            "defmodule M do\n@doc( # c\n)\ndef f(), do: :ok\nend",
            [&["module 0..47", "group 0..47: 27..43"], &[], &[], &[]],
        ),
        // r02_moduledoc_read_with_a_comment
        (
            "defmodule M do\n@moduledoc( # c\n)\ndef f(), do: :ok\nend",
            [&["module 0..53", "group 0..53: 33..49"], &[], &[], &[]],
        ),
        // r03_typedoc_read_with_a_comment
        (
            "defmodule M do\n@typedoc( # c\n)\ndef f(), do: :ok\nend",
            [&["module 0..51", "group 0..51: 31..47"], &[], &[], &[]],
        ),
        // r04_read_with_two_comments
        (
            "defmodule M do\n@doc( # a\n# b\n)\ndef f(), do: :ok\nend",
            [&["module 0..51", "group 0..51: 31..47"], &[], &[], &[]],
        ),
        // r05_read_before_a_real_attribute
        (
            concat!(
                "defmodule M do\n@doc( # c\n)\n@doc \"audit\"\ndef f(), do: :ok\n",
                "end"
            ),
            [
                &["module 0..60", "group 0..60: 40..56"],
                &["Primary Single [27..39] \"audit\" 0..5<-33..38"],
                &["0 Resolved group 0..60: 40..56 by 40..56 Unknown"],
                &[],
            ],
        ),
        // r06_read_after_a_real_attribute
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@doc( # c\n)\ndef f(), do: :ok\n",
                "end"
            ),
            [
                &["module 0..60", "group 0..60: 40..56"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..60: 40..56 by 40..56 Unknown"],
                &[],
            ],
        ),
        // r07_moduledoc_read_after_a_real_one
        (
            "defmodule M do\n@moduledoc \"audit\"\n@moduledoc( # c\n)\nend",
            [
                &["module 0..55"],
                &["FilePackage Single [15..33] \"audit\" 0..5<-27..32"],
                &["0 Resolved module 0..55 by - Unknown"],
                &[],
            ],
        ),
        // r08_reads_at_file_level
        (
            "@doc( # c\n)\n@moduledoc( # c\n)\n@typedoc( # c\n)\n",
            [&[], &[], &[], &[]],
        ),
        // r09_read_in_a_quote
        ("quote do\n@doc( # c\n)\nend", [&[], &[], &[], &[]]),
    ]);
}

// No count and no place of a child depends on a comment: each row publishes what it
// publishes with a line end in place of the comment.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_comment_is_no_structural_child() {
    assert_views(&[
        // t01_comment_before_the_do_of_a_module
        (
            "defmodule M # c\ndo\n@doc \"audit\"\ndef f(), do: :ok\nend",
            [
                &["module 0..52", "group 0..52: 32..48"],
                &["Primary Single [19..31] \"audit\" 0..5<-25..30"],
                &["0 Resolved group 0..52: 32..48 by 32..48 Unknown"],
                &[],
            ],
        ),
        // t02_comment_before_the_do_of_a_definition
        (
            "defmodule M do\n@doc \"audit\"\ndef f() # c\ndo\n:ok\nend\nend",
            [
                &["module 0..54", "group 0..54: 28..50"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..54: 28..50 by 28..50 Unknown"],
                &[],
            ],
        ),
        // t03_comment_after_when
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when # c\n is_atom(x), ",
                "do: x\nend"
            ),
            [
                &["module 0..68", "group 0..68: 28..64"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..68: 28..64 by 28..64 Unknown"],
                &[],
            ],
        ),
        // t04_comment_before_when
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when is_atom(x), ",
                "do: x\nend"
            ),
            [
                &["module 0..68", "group 0..68: 28..64"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..68: 28..64 by 28..64 Unknown"],
                &[],
            ],
        ),
        // t05_comment_before_the_do_of_a_guarded_definition
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when is_atom(x) # c\ndo",
                "\nx\nend\nend"
            ),
            [
                &["module 0..69", "group 0..69: 28..65"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..69: 28..65 by 28..65 Unknown"],
                &[],
            ],
        ),
        // t06_comment_after_the_value_in_parentheses
        (
            "defmodule M do\n@doc(\"audit\" # c\n)\ndef f(), do: :ok\nend",
            [
                &["module 0..54", "group 0..54: 34..50"],
                &["Primary Single [15..33] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..54: 34..50 by 34..50 Unknown"],
                &[],
            ],
        ),
        // t07_comment_between_the_head_and_the_keywords
        (
            "defmodule M do\n@doc \"audit\"\ndef f(), # c\n do: :ok\nend",
            [
                &["module 0..53", "group 0..53: 28..49"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..53: 28..49 by 28..49 Unknown"],
                &[],
            ],
        ),
        // t08_comment_before_the_do_of_a_bare_head
        (
            "defmodule M do\n@doc \"audit\"\ndef f # c\n do\n:ok\nend\nend",
            [
                &["module 0..53", "group 0..53: 28..49"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..53: 28..49 by 28..49 Unknown"],
                &[],
            ],
        ),
        // t09_comment_at_the_end_of_every_row
        (
            concat!(
                "defmodule M do # c\n@doc \"audit\" # c\ndef f(), do: :ok # c\nen",
                "d"
            ),
            [
                &["module 0..60", "group 0..60: 36..52"],
                &["Primary Single [19..31] \"audit\" 0..5<-25..30"],
                &["0 Resolved group 0..60: 36..52 by 36..52 Unknown"],
                &[],
            ],
        ),
        // t10_two_comments_in_a_guarded_macro
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndefmacro m(x) # c\n when is_atom",
                "(x) # d\n do\nx\nend\nend"
            ),
            [
                &["module 0..80", "group 0..80: 28..76"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..80: 28..76 by 28..76 Unknown"],
                &[],
            ],
        ),
        // t11_comment_in_the_parentheses_of_a_moduledoc
        (
            "defmodule M do\n@moduledoc( # c\n\"audit\")\nend",
            [
                &["module 0..43"],
                &["FilePackage Single [15..39] \"audit\" 0..5<-32..37"],
                &["0 Resolved module 0..43 by - Unknown"],
                &[],
            ],
        ),
        // t12_comment_in_an_empty_argument_list
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f( # c\n), do: :ok\ndef f(),",
                " do: :error\nend"
            ),
            [
                &["module 0..73", "group 0..73: 28..49, 50..69"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..73: 28..49, 50..69 by 28..49 Unknown"],
                &[],
            ],
        ),
        // t13_guarded_clause_with_a_comment_joins_its_group
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when x > 0, do: x",
                "\ndef f(x), do: x\nend"
            ),
            [
                &["module 0..79", "group 0..79: 28..59, 60..75"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..79: 28..59, 60..75 by 28..59 Unknown"],
                &[],
            ],
        ),
    ]);
}

// Two statements need a `;` or a line end between them. Where they have none, the second
// is not read and nothing below it is, in every node whose children are statements.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_joined_statement_and_what_it_holds_are_not_read() {
    assert_views(&[
        // s01_module_after_a_module_at_file_level
        (
            concat!(
                "defmodule A do\n@moduledoc \"a\"\nend defmodule X do\n@moduledoc",
                " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend"
            ),
            [
                &["module 0..33"],
                &["FilePackage Single [15..29] \"a\" 0..1<-27..28"],
                &["0 Resolved module 0..33 by - Unknown"],
                &[
                    "Occurrence RECOVERED unary_operator 49..63 -",
                    "Occurrence RECOVERED unary_operator 64..76 -",
                    "Owner PROJECTION call 34..97 -",
                ],
            ],
        ),
        // s02_module_after_a_definition_in_a_body
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["group 0..101: 15..33"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 49..63 -",
                    "Occurrence RECOVERED unary_operator 64..76 -",
                    "Owner PROJECTION call 0..101 -",
                    "Owner PROJECTION call 34..97 -",
                ],
            ],
        ),
        // s03_module_after_an_expression_in_a_definition
        (
            concat!(
                "defmodule M do\ndef g() do\n:ok defmodule X do\n@moduledoc \"x\"",
                "\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 45..59 -",
                    "Occurrence RECOVERED unary_operator 60..72 -",
                    "Owner PROJECTION call 0..101 -",
                    "Owner PROJECTION call 15..97 -",
                    "Owner PROJECTION call 30..93 -",
                ],
            ],
        ),
        // s04_module_in_a_block_of_a_call
        (
            concat!(
                "if true do\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc",
                " \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 40..54 -",
                    "Occurrence RECOVERED unary_operator 55..67 -",
                    "Owner PROJECTION call 25..88 -",
                ],
            ],
        ),
        // s05_module_in_an_after_block
        (
            concat!(
                "try do\n:ok\nafter\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 46..60 -",
                    "Occurrence RECOVERED unary_operator 61..73 -",
                    "Owner PROJECTION call 31..94 -",
                ],
            ],
        ),
        // s06_module_in_an_else_block
        (
            concat!(
                "if true do\n:ok\nelse\nfn -> :ok end defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 49..63 -",
                    "Occurrence RECOVERED unary_operator 64..76 -",
                    "Owner PROJECTION call 34..97 -",
                ],
            ],
        ),
        // s07_module_in_a_rescue_block
        (
            concat!(
                "try do\n:ok\nrescue\nfn -> :ok end defmodule X do\n@moduledoc \"",
                "x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 47..61 -",
                    "Occurrence RECOVERED unary_operator 62..74 -",
                    "Owner PROJECTION call 32..95 -",
                ],
            ],
        ),
        // s08_module_in_a_catch_block
        (
            concat!(
                "try do\n:ok\ncatch\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 46..60 -",
                    "Occurrence RECOVERED unary_operator 61..73 -",
                    "Owner PROJECTION call 31..94 -",
                ],
            ],
        ),
        // s09_module_in_the_body_of_a_clause
        (
            concat!(
                "f = fn ->\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 39..53 -",
                    "Occurrence RECOVERED unary_operator 54..66 -",
                    "Owner PROJECTION call 24..87 -",
                ],
            ],
        ),
        // s10_module_in_a_parenthesized_block
        (
            concat!(
                "(\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\n)"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 31..45 -",
                    "Occurrence RECOVERED unary_operator 46..58 -",
                    "Owner PROJECTION call 16..79 -",
                ],
            ],
        ),
        // s13_module_after_an_expression_at_file_level
        (
            concat!(
                "x = 1 defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 21..35 -",
                    "Occurrence RECOVERED unary_operator 36..48 -",
                    "Owner PROJECTION call 6..69 -",
                ],
            ],
        ),
        // s19_module_after_a_continued_row
        (
            concat!(
                "x = 1 \\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef ",
                "f(), do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 23..37 -",
                    "Occurrence RECOVERED unary_operator 38..50 -",
                    "Owner PROJECTION call 8..71 -",
                ],
            ],
        ),
        // s11_clause_after_a_clause_of_an_anonymous_function
        (
            concat!(
                "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 32..46 -",
                    "Occurrence RECOVERED unary_operator 47..59 -",
                ],
            ],
        ),
        // s12_module_below_a_joined_definition
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend def g() do\ndefmodule X do",
                "\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\n",
                "end"
            ),
            [
                &["group 0..116: 15..33"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 60..74 -",
                    "Occurrence RECOVERED unary_operator 75..87 -",
                    "Owner PROJECTION call 0..116 -",
                    "Owner PROJECTION call 34..112 -",
                ],
            ],
        ),
        // s14_module_after_a_string
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 34..48 -",
                    "Occurrence RECOVERED unary_operator 49..61 -",
                    "Owner PROJECTION call 0..86 -",
                    "Owner PROJECTION call 19..82 -",
                ],
            ],
        ),
        // s15_joined_attribute_in_a_definition
        (
            "defmodule M do\ndef g() do\n:ok @doc \"audit\"\nend\nend",
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 30..42 -",
                    "Owner PROJECTION call 0..50 -",
                    "Owner PROJECTION call 15..46 -",
                ],
            ],
        ),
        // s16_quote_below_a_joined_statement
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend if true do\nquote do\n@doc ",
                "\"audit\"\nend\nend\nend"
            ),
            [
                &["group 0..78: 15..33"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 54..66 -",
                    "Owner PROJECTION call 0..78 -",
                ],
            ],
        ),
        // s22_joined_quote
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend quote do\n@doc \"audit\"\ne",
                "nd\nend"
            ),
            [
                &["group 0..63: 15..33"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 43..55 -",
                    "Owner PROJECTION call 0..63 -",
                ],
            ],
        ),
        // s17_joined_module_in_a_quote
        (
            concat!(
                "quote do\n:ok defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\n",
                "def f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence TEMPLATE unary_operator 28..42 -",
                    "Occurrence TEMPLATE unary_operator 43..55 -",
                ],
            ],
        ),
        // s18_joined_protocol
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend defprotocol P do\n@doc \"au",
                "dit\"\ndef f(x)\nend\nend"
            ),
            [
                &["group 0..80: 15..33"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 51..63 -",
                    "Owner PROJECTION call 0..80 -",
                    "Owner PROJECTION call 34..76 -",
                ],
            ],
        ),
        // s20_joined_guard_definition
        (
            "defmodule M do\ndef x() do\n:ok\nend defguard g(x) when x\nend",
            [
                &["group 0..58: 15..33"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 0..58 -",
                    "Owner PROJECTION call 34..54 -",
                ],
            ],
        ),
        // s21_joined_attribute_at_file_level
        (
            "x = 1 @doc \"audit\"\n",
            [
                &[],
                &[],
                &[],
                &["Occurrence RECOVERED unary_operator 6..18 -"],
            ],
        ),
    ]);
}

// Controls of the test above: with a separator, or where the grammar asks for none, the
// module is read.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_module_that_is_apart_or_an_operand_is_read() {
    assert_views(&[
        // u01_module_after_a_line_end
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend\ndefmodule X do\n@moduledoc",
                " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[
                    "module 0..101",
                    "group 0..101: 15..33",
                    "module 34..97",
                    "group 34..97: 77..93",
                ],
                &[
                    "FilePackage Single [49..63] \"x\" 0..1<-61..62",
                    "Primary Single [64..76] \"audit\" 0..5<-70..75",
                ],
                &[
                    "0 Resolved module 34..97 by - Unknown",
                    "1 Resolved group 34..97: 77..93 by 77..93 Unknown",
                ],
                &[],
            ],
        ),
        // u02_module_after_a_semicolon
        (
            concat!(
                "x = 1; defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f()",
                ", do: :ok\nend"
            ),
            [
                &["module 7..70", "group 7..70: 50..66"],
                &[
                    "FilePackage Single [22..36] \"x\" 0..1<-34..35",
                    "Primary Single [37..49] \"audit\" 0..5<-43..48",
                ],
                &[
                    "0 Resolved module 7..70 by - Unknown",
                    "1 Resolved group 7..70: 50..66 by 50..66 Unknown",
                ],
                &[],
            ],
        ),
        // u03_module_after_a_comment_and_a_line_end
        (
            concat!(
                "x = 1 # c\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
                " f(), do: :ok\nend"
            ),
            [
                &["module 10..73", "group 10..73: 53..69"],
                &[
                    "FilePackage Single [25..39] \"x\" 0..1<-37..38",
                    "Primary Single [40..52] \"audit\" 0..5<-46..51",
                ],
                &[
                    "0 Resolved module 10..73 by - Unknown",
                    "1 Resolved group 10..73: 53..69 by 53..69 Unknown",
                ],
                &[],
            ],
        ),
        // u04_module_as_the_right_side_of_a_match
        (
            concat!(
                "x = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), d",
                "o: :ok\nend"
            ),
            [
                &["module 4..67", "group 4..67: 47..63"],
                &[
                    "FilePackage Single [19..33] \"x\" 0..1<-31..32",
                    "Primary Single [34..46] \"audit\" 0..5<-40..45",
                ],
                &[
                    "0 Resolved module 4..67 by - Unknown",
                    "1 Resolved group 4..67: 47..63 by 47..63 Unknown",
                ],
                &[],
            ],
        ),
        // u05_module_as_an_argument_of_a_call
        (
            concat!(
                "IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
                " f(), do: :ok\nend)"
            ),
            [
                &["module 11..74", "group 11..74: 54..70"],
                &[
                    "FilePackage Single [26..40] \"x\" 0..1<-38..39",
                    "Primary Single [41..53] \"audit\" 0..5<-47..52",
                ],
                &[
                    "0 Resolved module 11..74 by - Unknown",
                    "1 Resolved group 11..74: 54..70 by 54..70 Unknown",
                ],
                &[],
            ],
        ),
        // u07_module_after_a_semicolon_in_a_clause
        (
            concat!(
                "f = fn -> :ok; defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
                "\ndef f(), do: :ok\nend end"
            ),
            [
                &["module 15..78", "group 15..78: 58..74"],
                &[
                    "FilePackage Single [30..44] \"x\" 0..1<-42..43",
                    "Primary Single [45..57] \"audit\" 0..5<-51..56",
                ],
                &[
                    "0 Resolved module 15..78 by - Unknown",
                    "1 Resolved group 15..78: 58..74 by 58..74 Unknown",
                ],
                &[],
            ],
        ),
        // u06_module_in_a_rescue_section_on_the_row_of_a_statement
        (
            concat!(
                "try do :ok rescue _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 23..86", "group 23..86: 66..82"],
                &[
                    "FilePackage Single [38..52] \"x\" 0..1<-50..51",
                    "Primary Single [53..65] \"audit\" 0..5<-59..64",
                ],
                &[
                    "0 Resolved module 23..86 by - Unknown",
                    "1 Resolved group 23..86: 66..82 by 66..82 Unknown",
                ],
                &[],
            ],
        ),
        // u08_module_in_an_after_section_on_the_row_of_a_statement
        (
            concat!(
                "try do :ok after\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 17..80", "group 17..80: 60..76"],
                &[
                    "FilePackage Single [32..46] \"x\" 0..1<-44..45",
                    "Primary Single [47..59] \"audit\" 0..5<-53..58",
                ],
                &[
                    "0 Resolved module 17..80 by - Unknown",
                    "1 Resolved group 17..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // u09_module_in_a_catch_section_on_the_row_of_a_statement
        (
            concat!(
                "try do :ok catch _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 22..85", "group 22..85: 65..81"],
                &[
                    "FilePackage Single [37..51] \"x\" 0..1<-49..50",
                    "Primary Single [52..64] \"audit\" 0..5<-58..63",
                ],
                &[
                    "0 Resolved module 22..85 by - Unknown",
                    "1 Resolved group 22..85: 65..81 by 65..81 Unknown",
                ],
                &[],
            ],
        ),
        // u10_module_in_an_else_section_on_the_row_of_a_statement
        (
            concat!(
                "if true do :ok else\ndefmodule X do\n@moduledoc \"x\"\n@doc \"au",
                "dit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 20..83", "group 20..83: 63..79"],
                &[
                    "FilePackage Single [35..49] \"x\" 0..1<-47..48",
                    "Primary Single [50..62] \"audit\" 0..5<-56..61",
                ],
                &[
                    "0 Resolved module 20..83 by - Unknown",
                    "1 Resolved group 20..83: 63..79 by 63..79 Unknown",
                ],
                &[],
            ],
        ),
    ]);
}

// The grammar recovered inside the statement that holds the module. The module is read,
// and no association of it is resolved.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_recovery_in_a_statement_leaves_what_it_holds_unresolved() {
    assert_views(&[
        // v01_expression_read_into_a_match
        (
            concat!(
                "1 y = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            [
                &["module 6..69", "group 6..69: 49..65"],
                &[
                    "FilePackage Single [21..35] \"x\" 0..1<-33..34",
                    "Primary Single [36..48] \"audit\" 0..5<-42..47",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 6..69 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 6..69: 49..65 by 49..65 Unkn",
                        "own"
                    ),
                ],
                &[],
            ],
        ),
        // v02_call_read_into_the_next_call
        (
            concat!(
                "f() if true do\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
                "\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 15..78", "group 15..78: 58..74"],
                &[
                    "FilePackage Single [30..44] \"x\" 0..1<-42..43",
                    "Primary Single [45..57] \"audit\" 0..5<-51..56",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 15..78 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 15..78: 58..74 by 58..74 Unk",
                        "nown"
                    ),
                ],
                &[],
            ],
        ),
        // v03_expression_read_into_a_qualified_call
        (
            concat!(
                "1 IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\nd",
                "ef f(), do: :ok\nend)"
            ),
            [
                &["module 13..76", "group 13..76: 56..72"],
                &[
                    "FilePackage Single [28..42] \"x\" 0..1<-40..41",
                    "Primary Single [43..55] \"audit\" 0..5<-49..54",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 13..76 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 13..76: 56..72 by 56..72 Unk",
                        "nown"
                    ),
                ],
                &[],
            ],
        ),
        // v07_attribute_read_into_the_next_definition
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 25..88", "group 25..88: 68..84"],
                &[
                    "FilePackage Single [40..54] \"x\" 0..1<-52..53",
                    "Primary Single [55..67] \"audit\" 0..5<-61..66",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 25..88 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 25..88: 68..84 by 68..84 Unk",
                        "nown"
                    ),
                ],
                &[
                    "Occurrence OUTSIDE unary_operator 0..92 -",
                    "Owner PROJECTION call 14..21 -",
                ],
            ],
        ),
        // v06_recovery_among_the_clauses_of_a_function
        (
            concat!(
                "f = fn\n1 1 -> :ok\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &["module 23..86", "group 23..86: 66..82"],
                &[
                    "FilePackage Single [38..52] \"x\" 0..1<-50..51",
                    "Primary Single [53..65] \"audit\" 0..5<-59..64",
                ],
                &[
                    "0 Resolved module 23..86 by - Unknown",
                    "1 Resolved group 23..86: 66..82 by 66..82 Unknown",
                ],
                &[],
            ],
        ),
        // v04_definition_read_into_the_next_definition
        (
            concat!(
                "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            [
                &["module 43..106", "group 43..106: 86..102"],
                &[
                    "FilePackage Single [58..72] \"x\" 0..1<-70..71",
                    "Primary Single [73..85] \"audit\" 0..5<-79..84",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 43..106 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 43..106: 86..102 by 86..102 ",
                        "Unknown"
                    ),
                ],
                &[
                    "Owner PROJECTION call 0..114 -",
                    "Owner PROJECTION call 15..110 -",
                    "Owner PROJECTION call 32..39 -",
                ],
            ],
        ),
        // v05_error_in_a_head_and_a_sound_statement_after_it
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef a(, do: (\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\n)\n@doc \"",
                "two\"\ndef b(), do: :ok\nend"
            ),
            [
                &[
                    "group 0..137: 117..133",
                    "module 40..103",
                    "group 40..103: 83..99",
                ],
                &[
                    "FilePackage Single [55..69] \"x\" 0..1<-67..68",
                    "Primary Single [70..82] \"audit\" 0..5<-76..81",
                    "Primary Single [106..116] \"two\" 0..3<-112..115",
                ],
                &[
                    "0 Unresolved(GrammarBoundary) module 40..103 by - Unknown",
                    concat!(
                        "1 Unresolved(GrammarBoundary) group 40..103: 83..99 by 83..99 Un",
                        "known"
                    ),
                    "2 Resolved group 0..137: 117..133 by 117..133 Unknown",
                ],
                &[
                    "Owner PROJECTION call 0..137 -",
                    "Owner PROJECTION call 26..105 -",
                    "Owner UNOWNED unary_operator 15..25 -",
                ],
            ],
        ),
    ]);
}

// The compiler pairs the quotes of a file from its start. Where the grammar dropped a quote,
// it reads the rest of the file with text and code exchanged, so no statement from there on
// is read. These are pending outcomes: the file does not compile.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_statement_behind_a_torn_literal_is_not_read() {
    assert_views(&[
        // w01_string_without_its_end
        (
            concat!(
                "defmodule M do\nx = \"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc \"",
                "REAL\"\ndef real, do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 50..61 -",
                    "Owner PROJECTION call 0..83 -",
                    "Owner PROJECTION call 33..49 -",
                    "Owner PROJECTION call 62..79 -",
                ],
            ],
        ),
        // w02_quoted_atom_without_its_end
        (
            concat!(
                "defmodule M do\nx = :\"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc ",
                "\"REAL\"\ndef real, do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 51..62 -",
                    "Owner PROJECTION call 0..84 -",
                    "Owner PROJECTION call 34..50 -",
                    "Owner PROJECTION call 63..80 -",
                ],
            ],
        ),
        // w03_statements_before_the_tear_are_read
        (
            concat!(
                "defmodule M do\n@doc \"one\"\ndef a(), do: :ok\nx = \"s\n@doc \"",
                "two\"\ndef b(), do: :ok\n@doc \"three\"\ndef c(), do: :ok\nend"
            ),
            [
                &["group 0..111: 26..42"],
                &["Primary Single [15..25] \"one\" 0..3<-21..24"],
                &["0 Resolved group 0..111: 26..42 by 26..42 Unknown"],
                &[
                    "Occurrence RECOVERED unary_operator 78..90 -",
                    "Owner PROJECTION call 0..111 -",
                    "Owner PROJECTION call 61..77 -",
                    "Owner PROJECTION call 91..107 -",
                ],
            ],
        ),
        // w04_doubled_quote
        (
            concat!(
                "defmodule M do\n@doc \"\"audit\"\ndef f(), do: :ok\n@doc \"next",
                "\"\ndef g(), do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 15..45 -",
                    "Occurrence RECOVERED unary_operator 46..57 -",
                    "Owner PROJECTION call 0..78 -",
                    "Owner PROJECTION call 58..74 -",
                ],
            ],
        ),
        // w05_module_behind_a_tear_at_file_level
        (
            concat!(
                "x = \"a\ny = \"b\"\ndefmodule X do\n@moduledoc \"x\"\n@doc \"aud",
                "it\"\ndef f(), do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 30..44 -",
                    "Occurrence RECOVERED unary_operator 45..57 -",
                    "Owner PROJECTION call 15..78 -",
                ],
            ],
        ),
        // w06_tear_in_the_body_of_a_definition
        (
            concat!(
                "defmodule M do\ndef h() do\nx = \"a\ny = \"b\"\nend\n@doc \"REAL",
                "\"\ndef real(), do: :ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 45..56 -",
                    "Owner PROJECTION call 0..80 -",
                    "Owner PROJECTION call 15..44 -",
                    "Owner PROJECTION call 57..76 -",
                ],
            ],
        ),
        // w07_two_torn_literals
        (
            concat!(
                "defmodule M do\nx = \"s\n@doc \"two\"\ndef b(), do: :ok\n@doc \"",
                "three\"\ndef c(), do: :ok\ny = \"t\n@doc \"four\"\ndef d(), do: ",
                ":ok\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 50..62 -",
                    "Owner PROJECTION call 0..119 -",
                    "Owner PROJECTION call 33..49 -",
                    "Owner PROJECTION call 63..79 -",
                ],
            ],
        ),
    ]);
}

// The tokenizer takes a line feed into `?\\` and `?`, and records no end of line
// behind `,`. A statement that only such a line feed separates from the token is
// joined, in every kind of sequence.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_line_feed_in_a_character_or_after_a_comma_joins_the_next_statement() {
    assert_views(&[
        // x01_escaped_line_feed_at_file_level
        (
            concat!(
                "?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 20..39 -",
                    "Occurrence RECOVERED unary_operator 42..57 -",
                    "Owner PROJECTION call 3..80 -",
                ],
            ],
        ),
        // x02_escaped_line_feed_in_a_module_body
        (
            concat!(
                "defmodule O do\n  ?\\\n  defmodule M do\n    @moduledoc \"module",
                "\"\n    @doc \"function\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            [
                &["module 0..111"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 41..60 -",
                    "Occurrence RECOVERED unary_operator 65..80 -",
                    "Owner PROJECTION call 22..107 -",
                ],
            ],
        ),
        // x03_escaped_line_feed_in_a_definition_body
        (
            concat!(
                "defmodule O do\n  def g() do\n    ?\\\ndefmodule M do\n  @module",
                "doc \"module\"\n  @doc \"function\"\n  def f(), do: :ok\nend\n  ",
                "end\nend\n"
            ),
            [
                &["module 0..122", "group 0..122: 17..118"],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 52..71 -",
                    "Occurrence RECOVERED unary_operator 74..89 -",
                    "Owner PROJECTION call 35..112 -",
                ],
            ],
        ),
        // x04_escaped_line_feed_in_a_block_of_a_call
        (
            concat!(
                "if true do\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 29..43 -",
                    "Occurrence RECOVERED unary_operator 44..56 -",
                    "Owner PROJECTION call 14..77 -",
                ],
            ],
        ),
        // x05_escaped_line_feed_in_an_after_block
        (
            concat!(
                "try do\n:ok\nafter\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 35..49 -",
                    "Occurrence RECOVERED unary_operator 50..62 -",
                    "Owner PROJECTION call 20..83 -",
                ],
            ],
        ),
        // x06_escaped_line_feed_in_an_else_block
        (
            concat!(
                "if true do\n:ok\nelse\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 38..52 -",
                    "Occurrence RECOVERED unary_operator 53..65 -",
                    "Owner PROJECTION call 23..86 -",
                ],
            ],
        ),
        // x07_escaped_line_feed_in_a_rescue_block
        (
            concat!(
                "try do\n:ok\nrescue\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc",
                " \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 36..50 -",
                    "Occurrence RECOVERED unary_operator 51..63 -",
                    "Owner PROJECTION call 21..84 -",
                ],
            ],
        ),
        // x08_escaped_line_feed_in_a_catch_block
        (
            concat!(
                "try do\n:ok\ncatch\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 35..49 -",
                    "Occurrence RECOVERED unary_operator 50..62 -",
                    "Owner PROJECTION call 20..83 -",
                ],
            ],
        ),
        // x09_escaped_line_feed_in_the_body_of_a_clause
        (
            concat!(
                "f = fn ->\n?\\\ndefmodule X do\n@moduledoc \"x\"\n@doc \"audit\"",
                "\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 28..42 -",
                    "Occurrence RECOVERED unary_operator 43..55 -",
                    "Owner PROJECTION call 13..76 -",
                ],
            ],
        ),
        // x10_escaped_line_feed_in_parentheses
        (
            concat!(
                "(?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n)\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 21..40 -",
                    "Occurrence RECOVERED unary_operator 43..58 -",
                    "Owner PROJECTION call 4..81 -",
                ],
            ],
        ),
        // x11_escaped_line_feed_between_clauses_of_a_function
        (
            concat!(
                "fn\n1 -> ?\\\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audi",
                "t\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 31..45 -",
                    "Occurrence RECOVERED unary_operator 46..58 -",
                ],
            ],
        ),
        // x12_escaped_line_feed_before_doc
        (
            concat!(
                "defmodule O do\n  ?\\\n  @doc \"function\"\n  def f(), do: :ok\n",
                "end\n"
            ),
            [
                &["module 0..60", "group 0..60: 40..56"],
                &[],
                &[],
                &["Occurrence RECOVERED unary_operator 22..37 -"],
            ],
        ),
        // x13_escaped_line_feed_after_moduledoc_before_doc
        (
            concat!(
                "defmodule M do\n  @moduledoc \"module\"\n  ?\\\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 0..82", "group 0..82: 62..78"],
                &["FilePackage Single [17..36] \"module\" 0..6<-29..35"],
                &["0 Resolved module 0..82 by - Unknown"],
                &["Occurrence RECOVERED unary_operator 44..59 -"],
            ],
        ),
        // x14_escaped_line_feed_between_doc_and_definition
        (
            concat!(
                "defmodule M do\n  @doc \"function\"\n  ?\\\n  def f(), do: :ok\n",
                "end\n"
            ),
            [
                &["module 0..60"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 40..56 -",
                    "Owner UNOWNED unary_operator 17..32 -",
                ],
            ],
        ),
        // x16_assigned_escaped_line_feed_between_doc_and_definition
        (
            concat!(
                "defmodule M do\n  @doc \"function\"\n  x = ?\\\n  def f(), do: :",
                "ok\nend\n"
            ),
            [
                &["module 0..64"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 44..60 -",
                    "Owner UNOWNED unary_operator 17..32 -",
                ],
            ],
        ),
        // x15_escaped_line_feed_as_the_value_of_doc
        (
            "defmodule M do\n  @doc ?\\\n  def f(), do: :ok\nend\n",
            [
                &["module 0..47"],
                &[],
                &[],
                &[
                    "Owner PROJECTION call 27..43 -",
                    "Owner UNOWNED unary_operator 17..24 -",
                ],
            ],
        ),
        // x17_assigned_escaped_line_feed_at_file_level
        (
            concat!(
                "x = ?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
                "ion\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 24..43 -",
                    "Occurrence RECOVERED unary_operator 46..61 -",
                    "Owner PROJECTION call 7..84 -",
                ],
            ],
        ),
        // x18_escaped_line_feed_then_a_continued_row
        (
            concat!(
                "?\\\n\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
                "ion\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 22..41 -",
                    "Occurrence RECOVERED unary_operator 44..59 -",
                    "Owner PROJECTION call 5..82 -",
                ],
            ],
        ),
        // x19_bare_question_mark_and_line_feed_at_file_level
        (
            concat!(
                "?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 19..38 -",
                    "Occurrence RECOVERED unary_operator 41..56 -",
                    "Owner PROJECTION call 2..79 -",
                ],
            ],
        ),
        // x20_bare_question_mark_and_line_feed_in_a_module_body
        (
            concat!(
                "defmodule O do\n  ?\n  defmodule M do\n    @moduledoc \"module\"",
                "\n    @doc \"function\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 40..59 -",
                    "Occurrence RECOVERED unary_operator 64..79 -",
                    "Owner PROJECTION call 0..110 -",
                    "Owner PROJECTION call 21..106 -",
                ],
            ],
        ),
        // x21_bare_question_mark_after_a_character_token
        (
            concat!(
                "???\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 21..40 -",
                    "Occurrence RECOVERED unary_operator 43..58 -",
                    "Owner PROJECTION call 4..81 -",
                ],
            ],
        ),
        // x22_bare_question_mark_and_line_feed_in_a_clause
        (
            concat!(
                "fn\n1 -> ?\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 30..44 -",
                    "Occurrence RECOVERED unary_operator 45..57 -",
                ],
            ],
        ),
        // x23_line_feed_after_a_comma
        (
            concat!(
                "foo,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 22..41 -",
                    "Occurrence RECOVERED unary_operator 44..59 -",
                    "Owner PROJECTION call 5..82 -",
                ],
            ],
        ),
        // x24_two_line_feeds_after_a_comma
        (
            concat!(
                "foo,\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
                "on\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 23..42 -",
                    "Occurrence RECOVERED unary_operator 45..60 -",
                    "Owner PROJECTION call 6..83 -",
                ],
            ],
        ),
        // x25_comment_after_a_comma
        (
            concat!(
                "foo, # c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
                "tion\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 26..45 -",
                    "Occurrence RECOVERED unary_operator 48..63 -",
                    "Owner PROJECTION call 9..86 -",
                ],
            ],
        ),
        // x26_semicolon_in_a_comment_after_a_comma
        (
            concat!(
                "foo, # c; d\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"f",
                "unction\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 29..48 -",
                    "Occurrence RECOVERED unary_operator 51..66 -",
                    "Owner PROJECTION call 12..89 -",
                ],
            ],
        ),
        // x27_semicolon_character_without_a_separator
        (
            concat!(
                "?;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"\n",
                "  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 19..38 -",
                    "Occurrence RECOVERED unary_operator 41..56 -",
                    "Owner PROJECTION call 2..79 -",
                ],
            ],
        ),
        // x28_continued_row_after_a_question_mark_identifier
        (
            concat!(
                "foo?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
                "on\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 23..42 -",
                    "Occurrence OUTSIDE unary_operator 45..60 -",
                    "Owner PROJECTION call 6..17 -",
                    "Owner PROJECTION call 63..79 -",
                ],
            ],
        ),
    ]);
}

// Controls of the test above: a second line feed, a `;` or a comment behind the token,
// another character token, an identifier or atom ending in `?`, a string ending in
// `?\\`, a binary operator or `.` that continues the expression in one node, or a
// comma inside a list.
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_separator_behind_a_character_or_a_comma_is_read() {
    assert_views(&[
        // y01_second_line_feed_after_an_escaped_line_feed
        (
            concat!(
                "?\\\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y02_semicolon_after_an_escaped_line_feed
        (
            concat!(
                "?\\\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y03_comment_after_an_escaped_line_feed
        (
            concat!(
                "?\\\n# c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
                "tion\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 7..84", "group 7..84: 64..80"],
                &[
                    "FilePackage Single [24..43] \"module\" 0..6<-36..42",
                    "Primary Single [46..61] \"function\" 0..8<-52..60",
                ],
                &[
                    "0 Resolved module 7..84 by - Unknown",
                    "1 Resolved group 7..84: 64..80 by 64..80 Unknown",
                ],
                &[],
            ],
        ),
        // y04_escaped_backslash_character
        (
            concat!(
                "?\\\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y05_plain_character
        (
            concat!(
                "?a\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y06_identifier_ending_in_a_question_mark
        (
            concat!(
                "foo?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 5..82", "group 5..82: 62..78"],
                &[
                    "FilePackage Single [22..41] \"module\" 0..6<-34..40",
                    "Primary Single [44..59] \"function\" 0..8<-50..58",
                ],
                &[
                    "0 Resolved module 5..82 by - Unknown",
                    "1 Resolved group 5..82: 62..78 by 62..78 Unknown",
                ],
                &[],
            ],
        ),
        // y07_atom_ending_in_a_question_mark
        (
            concat!(
                ":a?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y08_string_ending_in_question_mark_and_backslash
        (
            concat!(
                "\"a?\\\\\"\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"fu",
                "nction\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 7..84", "group 7..84: 64..80"],
                &[
                    "FilePackage Single [24..43] \"module\" 0..6<-36..42",
                    "Primary Single [46..61] \"function\" 0..8<-52..60",
                ],
                &[
                    "0 Resolved module 7..84 by - Unknown",
                    "1 Resolved group 7..84: 64..80 by 64..80 Unknown",
                ],
                &[],
            ],
        ),
        // y09_escaped_line_feed_inside_a_call
        (
            concat!(
                "f(?\\\n)\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
                "tion\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 7..84", "group 7..84: 64..80"],
                &[
                    "FilePackage Single [24..43] \"module\" 0..6<-36..42",
                    "Primary Single [46..61] \"function\" 0..8<-52..60",
                ],
                &[
                    "0 Resolved module 7..84 by - Unknown",
                    "1 Resolved group 7..84: 64..80 by 64..80 Unknown",
                ],
                &[],
            ],
        ),
        // y10_escaped_line_feed_then_an_operator_row
        (
            concat!(
                "x = ?\\\n+ 1\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"",
                "function\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 11..88", "group 11..88: 68..84"],
                &[
                    "FilePackage Single [28..47] \"module\" 0..6<-40..46",
                    "Primary Single [50..65] \"function\" 0..8<-56..64",
                ],
                &[
                    "0 Resolved module 11..88 by - Unknown",
                    "1 Resolved group 11..88: 68..84 by 68..84 Unknown",
                ],
                &[],
            ],
        ),
        // y11_escaped_space_character
        (
            concat!(
                "?\\ \ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y12_space_character
        (
            concat!(
                "? \ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y13_two_escaped_line_feeds_then_a_blank_row
        (
            concat!(
                "?\\\n?\\\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"fu",
                "nction\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 7..84", "group 7..84: 64..80"],
                &[
                    "FilePackage Single [24..43] \"module\" 0..6<-36..42",
                    "Primary Single [46..61] \"function\" 0..8<-52..60",
                ],
                &[
                    "0 Resolved module 7..84 by - Unknown",
                    "1 Resolved group 7..84: 64..80 by 64..80 Unknown",
                ],
                &[],
            ],
        ),
        // y14_escaped_hash_character
        (
            concat!(
                "?\\#\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y15_hash_character
        (
            concat!(
                "?#\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y16_line_feed_then_a_semicolon
        (
            concat!(
                "1\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y17_leading_semicolon
        (
            concat!(
                ";defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"\n ",
                " def f(), do: :ok\nend\n"
            ),
            [
                &["module 1..78", "group 1..78: 58..74"],
                &[
                    "FilePackage Single [18..37] \"module\" 0..6<-30..36",
                    "Primary Single [40..55] \"function\" 0..8<-46..54",
                ],
                &[
                    "0 Resolved module 1..78 by - Unknown",
                    "1 Resolved group 1..78: 58..74 by 58..74 Unknown",
                ],
                &[],
            ],
        ),
        // y18_two_semicolons
        (
            concat!(
                "1;;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y19_second_line_feed_after_a_bare_question_mark
        (
            concat!(
                "?\n\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y20_semicolon_after_a_bare_question_mark
        (
            concat!(
                "?\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y21_comment_after_a_bare_question_mark
        (
            concat!(
                "?\n# c\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
                "on\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 6..83", "group 6..83: 63..79"],
                &[
                    "FilePackage Single [23..42] \"module\" 0..6<-35..41",
                    "Primary Single [45..60] \"function\" 0..8<-51..59",
                ],
                &[
                    "0 Resolved module 6..83 by - Unknown",
                    "1 Resolved group 6..83: 63..79 by 63..79 Unknown",
                ],
                &[],
            ],
        ),
        // y22_pipe_operator_behind_an_escaped_line_feed
        (
            concat!(
                "?\\\n|> defmodule M do\n  @moduledoc \"module\"\n  @doc \"functi",
                "on\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 6..83", "group 6..83: 63..79"],
                &[
                    "FilePackage Single [23..42] \"module\" 0..6<-35..41",
                    "Primary Single [45..60] \"function\" 0..8<-51..59",
                ],
                &[
                    "0 Resolved module 6..83 by - Unknown",
                    "1 Resolved group 6..83: 63..79 by 63..79 Unknown",
                ],
                &[],
            ],
        ),
        // y23_dot_call_behind_an_escaped_line_feed
        (
            concat!(
                "?\\\n.foo(defmodule M do\n  @moduledoc \"module\"\n  @doc \"func",
                "tion\"\n  def f(), do: :ok\nend)\n"
            ),
            [
                &["module 8..85", "group 8..85: 65..81"],
                &[
                    "FilePackage Single [25..44] \"module\" 0..6<-37..43",
                    "Primary Single [47..62] \"function\" 0..8<-53..61",
                ],
                &[
                    "0 Resolved module 8..85 by - Unknown",
                    "1 Resolved group 8..85: 65..81 by 65..81 Unknown",
                ],
                &[],
            ],
        ),
        // y24_minus_on_one_row
        (
            concat!(
                "?a - defmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 5..82", "group 5..82: 62..78"],
                &[
                    "FilePackage Single [22..41] \"module\" 0..6<-34..40",
                    "Primary Single [44..59] \"function\" 0..8<-50..58",
                ],
                &[
                    "0 Resolved module 5..82 by - Unknown",
                    "1 Resolved group 5..82: 62..78 by 62..78 Unknown",
                ],
                &[],
            ],
        ),
        // y25_access_on_one_row
        (
            concat!(
                "?a[defmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend]\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // y26_comma_in_a_list
        (
            concat!(
                "[1,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend]\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // y27_semicolon_after_a_comma
        (
            concat!(
                "foo,\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 6..83", "group 6..83: 63..79"],
                &[
                    "FilePackage Single [23..42] \"module\" 0..6<-35..41",
                    "Primary Single [45..60] \"function\" 0..8<-51..59",
                ],
                &[
                    "0 Resolved module 6..83 by - Unknown",
                    "1 Resolved group 6..83: 63..79 by 63..79 Unknown",
                ],
                &[],
            ],
        ),
        // y28_semicolon_after_a_comment_after_a_comma
        (
            concat!(
                "foo, # c\n;defmodule M do\n  @moduledoc \"module\"\n  @doc \"fun",
                "ction\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 10..87", "group 10..87: 67..83"],
                &[
                    "FilePackage Single [27..46] \"module\" 0..6<-39..45",
                    "Primary Single [49..64] \"function\" 0..8<-55..63",
                ],
                &[
                    "0 Resolved module 10..87 by - Unknown",
                    "1 Resolved group 10..87: 67..83 by 67..83 Unknown",
                ],
                &[],
            ],
        ),
        // y29_clause_with_a_body_before_a_module_clause
        (
            concat!(
                "case x do\n  a -> 1\n  b -> defmodule X do\n    @moduledoc \"m\"",
                "\n    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            [
                &["module 26..99", "group 26..99: 77..93"],
                &[
                    "FilePackage Single [45..59] \"m\" 0..1<-57..58",
                    "Primary Single [64..72] \"d\" 0..1<-70..71",
                ],
                &[
                    "0 Resolved module 26..99 by - Unknown",
                    "1 Resolved group 26..99: 77..93 by 77..93 Unknown",
                ],
                &[],
            ],
        ),
    ]);
}

// Characterization fixtures that pin today's outcome on four known differences between
// the pinned grammar and the compiler: (a) a comment with a character that the
// tokenizer rejects (`z01`: the grammar reads a comment and the document is published);
// (b) `?\\` and a CR, and `?` and a CR, which the tokenizer rejects (`z02`, `z03`: the
// grammar reads a character token and the module behind the line feed is published);
// (c) an empty clause body, which the compiler accepts with a warning (`z04`: the
// grammar fails and puts the module of the next clause under a joined node, with no
// owner and no assessment); (d) a row starting with `-`, `+` or `[` behind `?\\` and a
// line feed, which continues the expression for the compiler (`z05`: the grammar starts
// a new statement, which B6.2 joins and withholds).
#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_known_differences_of_the_grammar_keep_their_outcomes() {
    assert_views(&[
        // z01_comment_with_a_rejected_character
        (
            "defmodule M do\n@doc \"audit\" # c\u{c} d\ndef f(), do: :ok\nend",
            [
                &["module 0..55", "group 0..55: 35..51"],
                &["Primary Single [15..27] \"audit\" 0..5<-21..26"],
                &["0 Resolved group 0..55: 35..51 by 35..51 Unknown"],
                &[],
            ],
        ),
        // z02_escaped_carriage_return_character
        (
            concat!(
                "?\\\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 4..81", "group 4..81: 61..77"],
                &[
                    "FilePackage Single [21..40] \"module\" 0..6<-33..39",
                    "Primary Single [43..58] \"function\" 0..8<-49..57",
                ],
                &[
                    "0 Resolved module 4..81 by - Unknown",
                    "1 Resolved group 4..81: 61..77 by 61..77 Unknown",
                ],
                &[],
            ],
        ),
        // z03_carriage_return_character
        (
            concat!(
                "?\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &["module 3..80", "group 3..80: 60..76"],
                &[
                    "FilePackage Single [20..39] \"module\" 0..6<-32..38",
                    "Primary Single [42..57] \"function\" 0..8<-48..56",
                ],
                &[
                    "0 Resolved module 3..80 by - Unknown",
                    "1 Resolved group 3..80: 60..76 by 60..76 Unknown",
                ],
                &[],
            ],
        ),
        // z04_empty_clause_body
        (
            concat!(
                "case x do\n  a ->\n  b -> defmodule X do\n    @moduledoc \"m\"\n",
                "    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence OUTSIDE unary_operator 43..57 -",
                    "Occurrence OUTSIDE unary_operator 62..70 -",
                    "Owner PROJECTION call 75..91 -",
                ],
            ],
        ),
        // z05_operator_row_behind_an_escaped_line_feed
        (
            concat!(
                "?\\\n- defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            [
                &[],
                &[],
                &[],
                &[
                    "Occurrence RECOVERED unary_operator 22..41 -",
                    "Occurrence RECOVERED unary_operator 44..59 -",
                ],
            ],
        ),
    ]);
}

// The children of a node: their kinds, with `!` after a token that the parser invented and
// `?` after a child that holds a syntax error.
#[cfg(feature = "elixir")]
fn shape(node: tree_sitter::Node<'_>) -> String {
    let mut cursor = node.walk();
    let kinds: Vec<_> = node
        .children(&mut cursor)
        .map(|child| {
            let mark = if child.is_missing() {
                "!"
            } else if child.has_error() {
                "?"
            } else {
                ""
            };
            format!("{}{mark}", child.kind())
        })
        .collect();
    kinds.join(" ")
}

// Every node of the tree, in the order of the source.
#[cfg(feature = "elixir")]
fn preorder(tree: &tree_sitter::Tree) -> Vec<tree_sitter::Node<'_>> {
    let mut nodes = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        nodes.push(node);
        let children: Vec<_> = node.children(&mut node.walk()).collect();
        stack.extend(children.into_iter().rev());
    }
    nodes
}

#[cfg(feature = "elixir")]
#[test]
#[allow(clippy::too_many_lines)]
fn elixir_parse_shapes_the_attribute_reader_relies_on() {
    // (the source, a node kind, which node of that kind in source order, its children)
    let shapes: [(&str, &str, usize, &str); 95] = [
        // a module: its word, its name, its body
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            0,
            "identifier arguments do_block",
        ),
        // a body: `do`, the statements, `end`
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // an attribute: the operator and its operand
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "unary_operator",
            0,
            "@ call",
        ),
        // the operand: the name and the value
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments",
        ),
        // a value without parentheses
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            1,
            "string",
        ),
        // a plain string
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content \"",
        ),
        // a definition without a block
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            2,
            "identifier arguments",
        ),
        // its head is the first child
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            2,
            "call , keywords",
        ),
        // a head: the name and the argument list
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "call",
            3,
            "identifier arguments",
        ),
        // an empty argument list
        (
            "defmodule M do\n  @doc \"audit\"\n  def f(), do: :ok\nend",
            "arguments",
            3,
            "( )",
        ),
        // a value in parentheses
        (
            "defmodule M do\n@doc(\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( string )",
        ),
        // after a space the parentheses are a block
        (
            "defmodule M do\n@doc (\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "block",
        ),
        // an empty string has no content
        (
            "defmodule M do\n@doc \"\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" \"",
        ),
        // a heredoc has other quotes
        (
            concat!(
                "defmodule M do\n@doc \"\"\"\naudit\n\"\"\"\ndef f(), do: :ok\nen",
                "d"
            ),
            "string",
            0,
            "\"\"\" quoted_content \"\"\"",
        ),
        // an escape is a child
        (
            "defmodule M do\n@doc \"a\\nb\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content escape_sequence quoted_content \"",
        ),
        // an interpolation is a child
        (
            "defmodule M do\n@doc \"a#{x}b\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content interpolation quoted_content \"",
        ),
        // a line end inside is content
        (
            "defmodule M do\n@doc \"au\ndit\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content \"",
        ),
        // a block after the value belongs to the operand
        (
            "defmodule M do\n@doc \"audit\" do\nend\ndef f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments do_block",
        ),
        // two values
        (
            concat!(
                "defmodule M do\n@doc \"audit\", since: \"1\"\ndef f(), do: :ok\n",
                "end"
            ),
            "arguments",
            1,
            "string , keywords",
        ),
        // a comment before the operand; the operand is last
        (
            "defmodule M do\n@ # c\ndoc \"audit\"\ndef f(), do: :ok\nend",
            "unary_operator",
            0,
            "@ comment call",
        ),
        // a read has no call
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
                "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
            ),
            "unary_operator",
            4,
            "@ identifier",
        ),
        // a read with parentheses
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n@impl true\n@deprecated \"x\"\n@",
                "foo 1\n@doc\n@doc()\ndef f(), do: :ok\nend"
            ),
            "arguments",
            5,
            "( )",
        ),
        // a bare head
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f\ndef f(), do: :ok\ndef f w",
                "hen true, do: :no\nend"
            ),
            "arguments",
            2,
            "identifier",
        ),
        // a guarded head is the first child
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
                "f(x), do: x\nend"
            ),
            "arguments",
            2,
            "binary_operator , keywords",
        ),
        // the head is the left operand of `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when x > 0, do: x\ndef ",
                "f(x), do: x\nend"
            ),
            "binary_operator",
            0,
            "call when binary_operator",
        ),
        // a comment among the arguments
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(a, # c\n b), do: a\ndef f(",
                "a, b), do: b\ndef g(a, b: 1), do: a\ndef g(a, b), do: b\ndef f(a",
                "), do: a\nend"
            ),
            "arguments",
            3,
            "( identifier , comment identifier )",
        ),
        // `do` on the next row belongs to the definition
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndo\nx\nend\ndef f(y) d",
                "o\ny\nend\nend"
            ),
            "call",
            2,
            "identifier arguments do_block",
        ),
        // a definition call in parentheses starts with `(`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef(g(x), do: x)\ndef f(), do: :",
                "ok\nend"
            ),
            "arguments",
            2,
            "( call , keywords )",
        ),
        // a head without parentheses has no `(`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef g x, y do\nx\nend\ndef f(), ",
                "do: :ok\nend"
            ),
            "arguments",
            3,
            "identifier , identifier",
        ),
        // the name of this head is a call
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef unquote(name)(x), do: x\ndef",
                " f(), do: :ok\nend"
            ),
            "call",
            3,
            "call arguments",
        ),
        // the name of this head is a `dot`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef N.g(y), do: y\ndef f(), do: ",
                ":ok\nend"
            ),
            "call",
            3,
            "dot arguments",
        ),
        // the operator row is no statement
        (
            "defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // it belongs to the value
        (
            "defmodule M do\n@doc \"audit\"\n|> g()\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "binary_operator",
        ),
        // also behind a comment row
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\n# c\n|> g()\ndef f(), do: :ok\ne",
                "nd"
            ),
            "arguments",
            1,
            "binary_operator",
        ),
        // `do` on the next row belongs to the operand
        (
            "defmodule M do\n@doc \"audit\"\ndo\nend\ndef f(), do: :ok\nend",
            "call",
            1,
            "identifier arguments do_block",
        ),
        // a prefix operator starts a statement
        (
            "defmodule M do\n@doc \"audit\"\n- 1\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator unary_operator call end",
        ),
        // a continued row has no node
        (
            "defmodule M do\n@doc \"audit\"\n\\\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // `;` and a comment are children; a line end is none
        (
            concat!(
                "defmodule M do\n@doc \"audit\"; # note\n\n@spec f() :: atom()\nd",
                "ef f(), do: :ok\nend"
            ),
            "do_block",
            0,
            "do unary_operator ; comment unary_operator call end",
        ),
        // two statements on one row: no node shows it
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do",
                ": :ok\nend"
            ),
            "do_block",
            0,
            "do unary_operator call call end",
        ),
        // the same after a heredoc
        (
            "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator call end",
        ),
        // a missing `end` is an invented child
        (
            concat!(
                "defmodule M do\n@moduledoc \"m\"\n@doc \"audit\"\ndef f(), do: :",
                "ok\n"
            ),
            "do_block",
            0,
            "do unary_operator unary_operator call end!",
        ),
        // an `ERROR` child
        (
            "defmodule M do\n@doc \"audit\"\n1 def f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator ERROR? call end",
        ),
        // an attribute that holds an error
        (
            "defmodule M do\n@doc \"a\" \"b\"\ndef f(), do: :ok\nend",
            "do_block",
            0,
            "do unary_operator? call end",
        ),
        // the keyword form has no body child
        (
            "defmodule M, do: (@doc \"audit\"; def f(), do: :ok)",
            "call",
            0,
            "identifier arguments",
        ),
        // a section is a child of the body
        (
            concat!(
                "defmodule M do\ndef f(), do: :ok\nelse\n@doc \"audit\"\ndef g(),",
                " do: 1\nend"
            ),
            "do_block",
            0,
            "do call else_block end",
        ),
        // a `quote` with a block
        (
            concat!(
                "defmodule M do\nquote do\n@doc \"audit\"\ndef f(), do: :ok\nend",
                "\nend"
            ),
            "call",
            1,
            "identifier do_block",
        ),
        // a qualified definition starts with a `dot`
        (
            concat!(
                "defmodule M do\nKernel.@(doc \"audit\")\nKernel.def f(), do: :ok",
                "\nend"
            ),
            "call",
            3,
            "dot arguments",
        ),
        // a read with a comment in its parentheses
        (
            "defmodule M do\n@doc( # c\n)\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( comment )",
        ),
        // a comment before a value in parentheses
        (
            "defmodule M do\n@doc(# c\n\"audit\")\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( comment string )",
        ),
        // a comment after the value
        (
            "defmodule M do\n@doc(\"audit\" # c\n)\ndef f(), do: :ok\nend",
            "arguments",
            1,
            "( string comment )",
        ),
        // a comment before the `do` of a module
        (
            "defmodule M # c\ndo\n@doc \"audit\"\ndef f(), do: :ok\nend",
            "call",
            0,
            "identifier arguments comment do_block",
        ),
        // a comment before the `do` of a definition
        (
            "defmodule M do\n@doc \"audit\"\ndef f() # c\ndo\n:ok\nend\nend",
            "call",
            2,
            "identifier arguments comment do_block",
        ),
        // a comment after `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) when # c\n is_atom(x), ",
                "do: x\nend"
            ),
            "binary_operator",
            0,
            "call when comment call",
        ),
        // a comment before `when`
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x) # c\n when is_atom(x), ",
                "do: x\nend"
            ),
            "binary_operator",
            0,
            "call comment when call",
        ),
        // a comment between the head and the keywords
        (
            "defmodule M do\n@doc \"audit\"\ndef f(), # c\n do: :ok\nend",
            "arguments",
            2,
            "call , comment keywords",
        ),
        // a comment in an empty argument list
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f( # c\n), do: :ok\ndef f(),",
                " do: :error\nend"
            ),
            "arguments",
            3,
            "( comment )",
        ),
        // the file: two modules on one row are two clean children
        (
            concat!(
                "defmodule A do\n@moduledoc \"a\"\nend defmodule X do\n@moduledoc",
                " \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend"
            ),
            "source",
            0,
            "call call",
        ),
        // a body: the same, and no child shows the error
        (
            concat!(
                "defmodule M do\ndef x() do\n:ok\nend defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do call call end",
        ),
        // the block of a definition: the expression before is an `ERROR`
        (
            concat!(
                "defmodule M do\ndef g() do\n:ok defmodule X do\n@moduledoc \"x\"",
                "\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "do_block",
            1,
            "do ERROR? call end",
        ),
        // a block of a call: two clean children again
        (
            concat!(
                "if true do\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc",
                " \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do anonymous_function call end",
        ),
        // an `after` block
        (
            concat!(
                "try do\n:ok\nafter\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "after_block",
            0,
            "after anonymous_function call",
        ),
        // an `else` block
        (
            concat!(
                "if true do\n:ok\nelse\nfn -> :ok end defmodule X do\n@moduledoc ",
                "\"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "else_block",
            0,
            "else anonymous_function call",
        ),
        // a `rescue` block
        (
            concat!(
                "try do\n:ok\nrescue\nfn -> :ok end defmodule X do\n@moduledoc \"",
                "x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "rescue_block",
            0,
            "rescue anonymous_function call",
        ),
        // a `catch` block
        (
            concat!(
                "try do\n:ok\ncatch\nfn -> :ok end defmodule X do\n@moduledoc \"x",
                "\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "catch_block",
            0,
            "catch anonymous_function call",
        ),
        // the body of a clause
        (
            concat!(
                "f = fn ->\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "body",
            0,
            "anonymous_function call",
        ),
        // a parenthesized block
        (
            concat!(
                "(\nfn -> :ok end defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\n)"
            ),
            "block",
            0,
            "( anonymous_function call )",
        ),
        // an anonymous function: one clause that holds the second
        (
            concat!(
                "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn stab_clause? end",
        ),
        // the body of that clause stands behind an `ERROR`
        (
            concat!(
                "fn\n0 -> :ok 1 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audit",
                "\"\ndef f(), do: :ok\nend\nend"
            ),
            "stab_clause",
            0,
            "arguments -> ERROR? body?",
        ),
        // a section follows the statements with no separator
        (
            concat!(
                "try do :ok rescue _ -> defmodule X do\n@moduledoc \"x\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "do_block",
            0,
            "do atom rescue_block end",
        ),
        // a `;` or a line end between two statements is no node
        (
            concat!(
                "defmodule M do\n@doc \"audit\"\ndef f(x)\ndef f(0), do: 0\ndef f",
                "(x), do: x\nend"
            ),
            "do_block",
            0,
            "do unary_operator call call call end",
        ),
        // the expression before is an `ERROR` in the operator
        (
            concat!(
                "x = 1 defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            "binary_operator",
            0,
            "identifier = ERROR? call",
        ),
        // a string read as the key of a keyword
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            "pair",
            0,
            "quoted_keyword? call",
        ),
        // that key holds an error which no child shows
        (
            concat!(
                "defmodule M do\n\"s\" defmodule X do\n@moduledoc \"x\"\n@doc \"a",
                "udit\"\ndef f(), do: :ok\nend\nend"
            ),
            "quoted_keyword",
            0,
            "\" quoted_content \"",
        ),
        // the expression before is the left side; the `ERROR` is the next word
        (
            concat!(
                "1 y = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(),",
                " do: :ok\nend"
            ),
            "binary_operator",
            0,
            "integer ERROR? = call",
        ),
        // two definitions on one row: one call with the block of the second
        (
            concat!(
                "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "call",
            1,
            "identifier arguments? do_block",
        ),
        // the first body is an `ERROR` beside the second definition
        (
            concat!(
                "defmodule M do\ndef a(), do: :ok def g() do\ndefmodule X do\n@mo",
                "duledoc \"x\"\n@doc \"audit\"\ndef f(), do: :ok\nend\nend\nend"
            ),
            "pair",
            0,
            "keyword ERROR? call",
        ),
        // an attribute and a definition on one row: one attribute with the block
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "unary_operator",
            0,
            "@ call?",
        ),
        // its string is a key that holds an error which no child shows
        (
            concat!(
                "@doc \"before\" def g() do\ndefmodule X do\n@moduledoc \"x\"\n@d",
                "oc \"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "quoted_keyword",
            0,
            "\" quoted_content \"",
        ),
        // an `ERROR` among the clauses of a function
        (
            concat!(
                "f = fn\n1 1 -> :ok\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc ",
                "\"audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn ERROR? stab_clause stab_clause end",
        ),
        // an `ERROR` child of the file holds the module
        (
            concat!(
                "defmodule A do\n) ) (\ndefmodule M do\n@moduledoc \"m\"\n@doc \"",
                "audit\"\ndef f(), do: :ok\nend\nend"
            ),
            "source",
            0,
            "call ERROR?",
        ),
        // a string without its end: the third quote is a child of an `ERROR`
        (
            concat!(
                "defmodule M do\nx = \"a\n@doc \"LIT\"\ndef lit, do: :ok\n@doc \"",
                "REAL\"\ndef real, do: :ok\nend"
            ),
            "ERROR",
            0,
            "string sigil_name \"",
        ),
        // a quote too many: the same
        (
            concat!(
                "defmodule M do\n@doc \"\"audit\"\ndef f(), do: :ok\n@doc \"next",
                "\"\ndef g(), do: :ok\nend"
            ),
            "ERROR",
            0,
            "\" \" \"",
        ),
        // an `ERROR` in a string holds no quote
        (
            "defmodule M do\n@doc \"a\u{0}b\"\ndef f(), do: :ok\nend",
            "string",
            0,
            "\" quoted_content ERROR? \"",
        ),
        // the right side of a match
        (
            concat!(
                "x = defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef f(), d",
                "o: :ok\nend"
            ),
            "binary_operator",
            0,
            "identifier = call",
        ),
        // an argument of a call
        (
            concat!(
                "IO.inspect(defmodule X do\n@moduledoc \"x\"\n@doc \"audit\"\ndef",
                " f(), do: :ok\nend)"
            ),
            "arguments",
            0,
            "( call )",
        ),
        // `?\` before a line feed: a two-byte `char` child before the module
        (
            concat!(
                "?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // `?\` at the end of a clause gives two clean `stab_clause` children
        (
            concat!(
                "fn\n1 -> ?\\\n2 -> defmodule X do\n@moduledoc \"x\"\n@doc \"audi",
                "t\"\ndef f(), do: :ok\nend\nend"
            ),
            "anonymous_function",
            0,
            "fn stab_clause stab_clause end",
        ),
        // `x = ?\`: the `char` node is the last leaf of the operator
        (
            concat!(
                "x = ?\\\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"funct",
                "ion\"\n  def f(), do: :ok\nend\n"
            ),
            "binary_operator",
            0,
            "identifier = char",
        ),
        // `?` before a line feed: an `ERROR` leaf of one byte
        (
            concat!(
                "?\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function\"",
                "\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "ERROR",
        ),
        // `???` before a line feed: an `ERROR` with `char` and an inner `ERROR`
        (
            concat!(
                "???\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "char ERROR",
        ),
        // `foo,` before a line feed: the last leaf of the `ERROR` is `,`
        (
            concat!(
                "foo,\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "ERROR",
            0,
            "identifier ,",
        ),
        // `?\` before CR LF: a three-byte `char` child that holds the CR
        (
            concat!(
                "?\\\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // `?` before CR LF: a two-byte `char` child that holds the CR
        (
            concat!(
                "?\r\ndefmodule M do\n  @moduledoc \"module\"\n  @doc \"function",
                "\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char call",
        ),
        // an empty clause body: `b -> defmodule X` is a call with an `ERROR` `->` and
        //     `arguments`
        (
            concat!(
                "case x do\n  a ->\n  b -> defmodule X do\n    @moduledoc \"m\"\n",
                "    @doc \"d\"\n    def f(), do: :ok\n  end\nend\n"
            ),
            "call",
            1,
            "identifier ERROR? arguments do_block",
        ),
        // `-` on the row behind `?\` starts a `unary_operator` statement
        (
            concat!(
                "?\\\n- defmodule M do\n  @moduledoc \"module\"\n  @doc \"functio",
                "n\"\n  def f(), do: :ok\nend\n"
            ),
            "source",
            0,
            "char unary_operator",
        ),
    ];
    for (source, kind, nth, children) in shapes {
        let parser = parsed(source.as_bytes());
        let nodes = preorder(parser.ts_tree());
        let node = nodes.iter().filter(|node| node.kind() == kind).nth(nth);
        assert_eq!(shape(*node.unwrap()), children, "{kind} {nth} of {source}");
        // The reader takes a child by its place among the children that are no comment.
        // The grammar's field names say the same: the word of a call is its first child,
        // an operand is the last child of its operator, the sides of a binary operator
        // stand around it, and the quotes of a string are its first and last child.
        for node in nodes.iter().filter(|node| !node.has_error()) {
            let children: Vec<_> = node
                .children(&mut node.walk())
                .filter(|child| child.kind() != "comment")
                .collect();
            let fields: &[(&str, usize)] = match node.kind() {
                "call" => &[("target", 0)],
                "unary_operator" => &[("operator", 0), ("operand", children.len() - 1)],
                "binary_operator" => &[("left", 0), ("operator", 1), ("right", 2)],
                "string" => &[("quoted_start", 0), ("quoted_end", children.len() - 1)],
                _ => &[],
            };
            for (field, place) in fields {
                assert_eq!(
                    node.child_by_field_name(field),
                    children.get(*place).copied(),
                    "{field} of {kind} in {source}"
                );
            }
        }
    }
    // Two statements on one row: the body holds a syntax error that no child shows. The
    // reader therefore reads the bytes between two statements itself.
    for source in [
        "defmodule M do\n@doc \"audit\"\ndef f() do\n:ok\nend def g(), do: :ok\nend",
        "defmodule M do\n@doc \"\"\"\naudit\n\"\"\" def f(), do: :ok\nend",
    ] {
        let parser = parsed(source.as_bytes());
        let nodes = preorder(parser.ts_tree());
        let body = nodes.iter().find(|node| node.kind() == "do_block").unwrap();
        assert!(body.has_error(), "{source}");
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            assert!(!child.has_error() && !child.is_missing(), "{source}");
        }
    }
}
