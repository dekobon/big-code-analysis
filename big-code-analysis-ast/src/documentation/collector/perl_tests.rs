//! Perl POD in the source view: which blocks before the data section are documents of the
//! file, where a document's fragment ends, its text and where each part of the text comes
//! from, what is reported as pending, which blocks are withheld because perl may read
//! their lines as literal text, and which files are marked because the grammar may hide
//! POD behind a syntax error. A source is collected through both entry points, except
//! in the tests of restricted parses and of cost. The first two tests collect nothing and
//! pin the parse shapes the reader relies on. Node queries are not part of the view.

#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "perl")]
use super::pod::{PodBlock, command_line, pod_block};
#[cfg(feature = "perl")]
use super::pod_doubt::Doubt;
#[cfg(feature = "perl")]
use super::pod_format::{format_body, format_end};
#[cfg(feature = "perl")]
use super::pod_literal::{Quote, expects_term, perl_reads};
#[cfg(feature = "perl")]
use crate::documentation::budget::{
    BYTE_INTERVAL, STEP_INTERVAL, WorkBudget, WorkPhase, cancel_after, cancel_in_phase,
};
#[cfg(feature = "perl")]
use crate::documentation::text::TextError;
#[cfg(feature = "perl")]
use crate::documentation::text::comments::render;
#[cfg(feature = "perl")]
use crate::documentation::*;
#[cfg(feature = "perl")]
use crate::lang_helpers::doc_text::{pod_comment_region, pod_comment_region_with_budget};
#[cfg(feature = "perl")]
use crate::lang_helpers::perl::opens_pod_with_budget;
#[cfg(feature = "perl")]
use crate::{AnyParser, LANG};
#[cfg(feature = "perl")]
use std::cell::Cell;
#[cfg(feature = "perl")]
use std::ops::Range;

#[cfg(feature = "perl")]
use Run::{Copied, Decoded};

#[cfg(feature = "perl")]
const EXCLUDED: &str = "Excluded bytes prevent reading this POD block";
#[cfg(feature = "perl")]
const OPENER: &str = "POD block does not open with a POD command at the start of a line";
#[cfg(feature = "perl")]
const CLOSER: &str = "POD block does not end where perl and POD processors both end it";
#[cfg(feature = "perl")]
const LINE_END: &str = "POD block holds a carriage return that no line feed follows";
#[cfg(feature = "perl")]
const DATA_SECTION: &str = "Perl data section is not read for POD blocks";
#[cfg(feature = "perl")]
const MIXED: &str = "POD block mixes documentation with comment regions";
#[cfg(feature = "perl")]
const RECOVERY: &str = "POD block follows a syntax error, where perl may read literal text";
#[cfg(feature = "perl")]
const FORMAT: &str = "POD block lies in the body of a format, or after one whose end is not known";
#[cfg(feature = "perl")]
const UNREAD_OPENER: &str =
    "POD block follows a here-document opener that the grammar did not read";
#[cfg(feature = "perl")]
const HEREDOC: &str = "POD block lies beside a here-document that perl may read differently";
#[cfg(feature = "perl")]
const LITERAL: &str = "POD block follows a quote-like literal that perl may read differently";
#[cfg(feature = "perl")]
const HIDDEN: &str = "POD may stand after a syntax error where the grammar reads no POD block";
#[cfg(feature = "perl")]
const CUT_NODE: &str = "Recognized declaration requires a semantic projection";
/// The explicit checkpoints of a collection that publishes no document: one on entry, two
/// in the range preparation, one at the end of each of the three orderings (documents,
/// associations, qualifications), and one before the index is returned.
#[cfg(feature = "perl")]
const EMPTY: usize = 7;

#[cfg(feature = "perl")]
const PRIMARY: &[u8] = b"=head2 f\n\nAudit\n\n=cut\n\nsub f {}";
#[cfg(feature = "perl")]
const TWO_BLOCKS: &[u8] = b"my $x = 1;\n=head1 A\n\na\n\n=cut\nmy $y = 2;\n=head1 B\n\nb\n\n=cut\n";
#[cfg(feature = "perl")]
const MIXED_FIXTURE: &[u8] =
    b"=head2 f\n\nAudit\n\n=begin comment\n\nnot prose\n\n=end comment\n\n\
    =cut\n\nsub f { return 1; }\n__END__\n=head1 NAME\n\nData docs\n\n=cut\n";

/// A node as kind, byte range and kind of its parent.
#[cfg(feature = "perl")]
type Leaf = (&'static str, Range<usize>, &'static str);
#[cfg(feature = "perl")]
type Ranges = Vec<Range<usize>>;
/// Steps and bytes that a reading charged.
#[cfg(feature = "perl")]
type Charged = (usize, usize);
/// A node as label, byte range and start row. The label is `ERROR` or `MISSING` for a
/// recovery node and the kind otherwise.
#[cfg(feature = "perl")]
type Shape = (&'static str, Range<usize>, usize);
/// The start of each block and the reason that withholds it.
#[cfg(feature = "perl")]
type Doubts = Vec<(usize, Option<&'static str>)>;
/// The included byte ranges of a restricted parse.
#[cfg(feature = "perl")]
type Included = &'static [Range<usize>];
/// The line that ends a format, in its body, where the unit knows it.
// test-lang-gates: hand-written(perl) — named by the test of the scans only, which is
//     built with the Perl tests
#[cfg(feature = "perl")]
type LastLine = Option<Range<usize>>;
/// What perl makes of the spelling of a literal node: whether it ends the literal at the
/// last byte, and whether the text holds a here-document opener.
// test-lang-gates: hand-written(perl) — named by the test of the scans only, which is
//     built with the Perl tests
#[cfg(feature = "perl")]
type Reading = (bool, bool);

#[cfg(feature = "perl")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "perl")]
#[derive(Debug, PartialEq, Eq)]
enum Text {
    Complete(String, Vec<(Range<usize>, Run)>),
    Pending,
    Unavailable(UnavailableReason),
}

#[cfg(feature = "perl")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    fragment: Vec<Range<usize>>,
    text: Text,
}

#[cfg(feature = "perl")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: (&'static str, Range<usize>),
    document: Option<usize>,
}

/// The documents with their fragment pieces and text state, the retained candidates and
/// the pending assessments (sorted: the contract gives them no order) of one collection.
/// `published` asserts what is the same for every source of this file; the association of
/// a document is among that.
#[cfg(feature = "perl")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "perl")]
#[derive(Clone, Copy)]
enum Retain {
    Nothing,
    Unbound,
    Directives,
}

/// A document whose text is its fragment's bytes from the first on: one copied run for
/// each line and for each line feed between two lines.
#[cfg(feature = "perl")]
fn doc(fragment: Range<usize>, text: &str) -> Doc {
    let mut map = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches('\n').len();
        for run in [at..at + content, at + content..at + line.len()] {
            if !run.is_empty() {
                map.push((
                    run.clone(),
                    Copied(fragment.start + run.start..fragment.start + run.end),
                ));
            }
        }
        at += line.len();
    }
    mapped(fragment, text, map)
}

#[cfg(feature = "perl")]
fn mapped(fragment: Range<usize>, text: &str, map: Vec<(Range<usize>, Run)>) -> Doc {
    Doc {
        fragment: vec![fragment],
        text: Text::Complete(text.to_owned(), map),
    }
}

#[cfg(feature = "perl")]
fn pending_text(fragment: Range<usize>) -> Doc {
    Doc {
        fragment: vec![fragment],
        text: Text::Pending,
    }
}

#[cfg(feature = "perl")]
fn withheld(reason: &'static str, block: Range<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Selection,
        reason,
        node: ("pod_statement", block),
        document: None,
    }
}

#[cfg(feature = "perl")]
fn data_section(leaf: Range<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Occurrence,
        reason: DATA_SECTION,
        node: ("data_not_for_compiler", leaf),
        document: None,
    }
}

/// A node that an excluded range cuts. Today's outcome of the core, and no rule of the
/// Perl reader: the node is reported as a pending declaration.
#[cfg(feature = "perl")]
fn cut(kind: &'static str, node: Range<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason: CUT_NODE,
        node: (kind, node),
        document: None,
    }
}

/// The mark of a file whose POD the grammar may hide behind a syntax error: one
/// assessment on the first recovery node of the tree.
#[cfg(feature = "perl")]
fn hidden(kind: &'static str, node: Range<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Occurrence,
        reason: HIDDEN,
        node: (kind, node),
        document: None,
    }
}

#[cfg(feature = "perl")]
fn mixed(block: Range<usize>, document: usize) -> Pending {
    Pending {
        axis: AssessmentAxis::Composition,
        reason: MIXED,
        node: ("pod_statement", block),
        document: Some(document),
    }
}

#[cfg(feature = "perl")]
fn documents<const N: usize>(documents: [Doc; N]) -> View {
    View {
        documents: documents.into(),
        ..View::default()
    }
}

#[cfg(feature = "perl")]
fn pending<const N: usize>(assessments: [Pending; N]) -> View {
    View {
        assessments: unordered(assessments),
        ..View::default()
    }
}

#[cfg(feature = "perl")]
fn unordered(assessments: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut sorted: Vec<_> = assessments.into_iter().collect();
    sorted.sort_by_cached_key(|entry| format!("{entry:?}"));
    sorted
}

#[cfg(feature = "perl")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one source piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "perl")]
fn origin_run(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "perl")]
fn site(node: tree_sitter::Node<'_>) -> (&'static str, Range<usize>) {
    (node.kind(), node.byte_range())
}

/// The pieces of a POD fragment. Its node is the block that starts where it starts, and
/// between its end and the block's end lies nothing but one line end.
#[cfg(feature = "perl")]
fn fragment_pieces(fragment: &Fragment<'_>, source: &[u8]) -> Vec<Range<usize>> {
    assert!(!fragment.source().has_excluded_gap());
    let pieces: Vec<_> = fragment
        .source()
        .pieces()
        .iter()
        .map(SourcePiece::range)
        .collect();
    assert_eq!(fragment.form(), FragmentForm::Pod);
    let block = fragment.node().unwrap().node();
    assert_eq!(
        (block.kind(), block.start_byte()),
        ("pod_statement", pieces[0].start)
    );
    let end = pieces.last().unwrap().end;
    assert!(matches!(
        &source[end..block.end_byte()],
        b"" | b"\n" | b"\r" | b"\r\n"
    ));
    pieces
}

#[cfg(feature = "perl")]
fn assessed<'i, 's>(
    index: &'i DocumentationIndex<'s>,
    ordered: &[FactRef<'i, Document<'s>>],
    assessment: FactRef<'i, Assessment<'s>>,
) -> Pending {
    Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: site(assessment.get().node.unwrap().node()),
        document: index
            .assessed_document(assessment)
            .unwrap()
            .map(|document| ordered.iter().position(|known| *known == document).unwrap()),
    }
}

#[cfg(feature = "perl")]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    // bca: suppress(halstead) -- one reading of the published lists and their shared rules.
    let facts = index.facts();
    let source = index.source();
    let ordered: Vec<_> = index.documents().collect();
    let position = |document| ordered.iter().position(|known| *known == document).unwrap();
    let documents = ordered
        .iter()
        .map(|&document| {
            assert_eq!(document.get().role(), DocumentRole::FilePackage);
            assert_eq!(document.get().composition(), Composition::Single);
            assert_eq!(document.get().fragment_count(), 1);
            let fragment = facts.document_fragment(document, 0).unwrap();
            Doc {
                fragment: fragment_pieces(fragment.get(), source),
                text: match document.get().content() {
                    Content::Complete { .. } => {
                        let complete = index.complete_text(document).unwrap().unwrap();
                        Text::Complete(
                            complete.text().to_owned(),
                            complete
                                .mappings()
                                .iter()
                                .map(|entry| (entry.output.range(), origin_run(&entry.origin)))
                                .collect(),
                        )
                    }
                    Content::PendingAdapter => Text::Pending,
                    Content::Unavailable(reason) => Text::Unavailable(*reason),
                    other => panic!("a POD text state, found {other:?}"),
                },
            }
        })
        .collect();
    // The only owner is the file, and every document belongs to it: association `i` is
    // the resolved association of document `i`.
    let [file] = facts.owners().collect::<Vec<_>>()[..] else {
        panic!("the file owner alone")
    };
    assert!(matches!(file.get(), Owner::File(evidence) if evidence.node() == root));
    assert_eq!(index.associations().len(), ordered.len());
    for (at, association) in index.associations().enumerate() {
        let attachment = association.get();
        let document = facts.associated_document(association).unwrap();
        assert_eq!(position(document), at);
        assert_eq!(attachment.resolution, Resolution::Resolved);
        assert_eq!(attachment.owner_count(), 1);
        assert_eq!(facts.associated_owner(association, 0).unwrap(), file);
        assert_eq!(attachment.convention.name(), "POD");
        assert_eq!(attachment.publication, Publication::Unknown);
        assert!(attachment.binding.is_none());
        let block = facts.document_fragment(document, 0).unwrap();
        let block = block.get().node().unwrap().node();
        assert_eq!(Some(attachment.lexical_scope.node()), block.parent());
    }
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (
                candidate.get().disposition,
                span_of(&fragment_pieces(fragment.get(), source)),
            )
        })
        .collect();
    let read = |assessment| assessed(index, &ordered, assessment);
    let assessments = unordered(index.assessments().map(read));
    // Pending POD work is filed with the file. A node that an excluded range cuts keeps
    // the core's own assessment, which is filed with that node.
    let filed = index.owner_associations(file).unwrap();
    assert_eq!(
        unordered(filed.assessments().map(read)),
        unordered(
            index
                .assessments()
                .map(read)
                .filter(|entry| entry.reason != CUT_NODE)
        )
    );
    assert_eq!(filed.associations().len(), ordered.len());
    assert_eq!(index.profile(), Profile::Pod);
    assert_eq!(index.capability(), Capability::Partial);
    // POD has no command that names a declaration, so no qualification record exists.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        evidence,
        assessments,
    }
}

#[cfg(feature = "perl")]
fn span_of(pieces: &[Range<usize>]) -> Range<usize> {
    let [piece] = pieces else {
        panic!("one source piece, found {pieces:?}")
    };
    piece.clone()
}

#[cfg(feature = "perl")]
fn parsed(source: &[u8]) -> AnyParser {
    AnyParser::parse(LANG::Perl, source.to_vec(), None, None).unwrap()
}

#[cfg(feature = "perl")]
fn options(retain: Retain) -> DocumentationOptions {
    DocumentationOptions {
        retain_unbound: matches!(retain, Retain::Unbound),
        retain_directives: matches!(retain, Retain::Directives),
        ..DocumentationOptions::default()
    }
}

#[cfg(feature = "perl")]
fn indexes(parser: &AnyParser, retain: Retain) -> [DocumentationIndex<'_>; 2] {
    let context = DocumentationContext::default();
    let options = options(retain);
    [
        parser.documentation(&context, &options).unwrap(),
        collect(
            LANG::Perl,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ]
}

#[cfg(feature = "perl")]
fn retained(source: &[u8], retain: Retain) -> View {
    let parser = parsed(source);
    let root = parser.ts_tree().root_node();
    let [typed, raw] = indexes(&parser, retain)
        .each_ref()
        .map(|index| published(index, root));
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "perl")]
fn view(source: &[u8]) -> View {
    retained(source, Retain::Nothing)
}

#[cfg(feature = "perl")]
fn assert_views<const N: usize>(cases: [(&[u8], View); N]) {
    for (source, expected) in cases {
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
    }
}

/// A raw parse that reads the given byte ranges only.
#[cfg(feature = "perl")]
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
        .set_language(&LANG::Perl.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    parser.parse(source, None).unwrap()
}

#[cfg(feature = "perl")]
fn restricted_view(source: &[u8], ranges: &[Range<usize>], retain: Retain) -> View {
    let tree = restricted(source, ranges);
    let index = collect(
        LANG::Perl,
        &tree,
        source,
        &DocumentationContext::default(),
        &options(retain),
    )
    .unwrap();
    published(&index, tree.root_node())
}

/// Every node of the given kinds, in source order, with the kind of its parent.
#[cfg(feature = "perl")]
fn nodes_of(root: tree_sitter::Node<'_>, kinds: &[&str]) -> Vec<Leaf> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
        if kinds.contains(&node.kind()) {
            assert_eq!(node.child_count(), 0, "a leaf");
            found.push((
                node.kind(),
                node.byte_range(),
                node.parent().unwrap().kind(),
            ));
        }
    }
    found
}

/// Every node that the rules for literal text read, in tree order: recovery nodes,
/// blocks, the data leaf, here-document nodes, `<<` and `.` tokens, quote-like literals,
/// and identifiers spelled `format` or like a quote-like operator.
#[cfg(feature = "perl")]
fn shapes_of(root: tree_sitter::Node<'_>, source: &[u8]) -> Vec<Shape> {
    const KINDS: [&str; 19] = [
        "pod_statement",
        "data_not_for_compiler",
        "heredoc_initializer",
        "heredoc_body_statement",
        "heredoc_end_identifier",
        "<<",
        ".",
        "string_single_quoted",
        "string_double_quoted",
        "string_qq_quoted",
        "backtick_quoted",
        "command_qx_quoted",
        "pattern_matcher",
        "pattern_matcher_m",
        "regex_pattern_qr",
        "substitution_pattern_s",
        "transliteration_tr_or_y",
        "string_q_quoted",
        "word_list_qw",
    ];
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
        let keyword = node.kind() == "identifier"
            && matches!(
                &source[node.byte_range()],
                b"format" | b"q" | b"qq" | b"qw" | b"qx" | b"m" | b"qr" | b"s" | b"tr" | b"y"
            );
        let label = if node.is_error() {
            "ERROR"
        } else if node.is_missing() {
            "MISSING"
        } else if keyword || KINDS.contains(&node.kind()) {
            node.kind()
        } else {
            continue;
        };
        found.push((label, node.byte_range(), node.start_position().row));
    }
    found
}

/// The state of the rules for literal text, fed every node in tree order as the
/// collector's walk feeds it: for each block the reason that withholds it.
#[cfg(feature = "perl")]
fn doubt_walk(
    context: &SourceContext<'_>,
    root: tree_sitter::Node<'_>,
    budget: &Counting,
) -> Result<Doubts, SourceError> {
    let mut doubt = Doubt::default();
    let mut blocks = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        doubt.enter(context, node, budget)?;
        if node.kind() == "pod_statement" {
            blocks.push((node.start_byte(), doubt.reason(node.start_byte())));
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(blocks)
}

/// What a scan answers and the bytes it charges. The scan charges no step, and it is
/// cancelled once at each of its charges.
// test-lang-gates: hand-written(perl) — called by the test of the scans only, which is
//     built with the Perl tests
#[cfg(feature = "perl")]
fn scan_charges<T: PartialEq + std::fmt::Debug>(
    scan: impl Fn(&Counting) -> Result<T, SourceError>,
) -> (T, usize) {
    let budget = Counting::default();
    let answer = scan(&budget).unwrap();
    assert_eq!(budget.steps.get(), 0);
    for fail_at in 1..=budget.charges.get() {
        assert!(matches!(
            scan(&failing_at(fail_at)),
            Err(SourceError::Cancelled)
        ));
    }
    assert_eq!(scan(&failing_at(budget.charges.get() + 1)).unwrap(), answer);
    (answer, budget.bytes.get())
}

#[cfg(feature = "perl")]
fn node_at<'t>(
    root: tree_sitter::Node<'t>,
    kind: &str,
    range: Range<usize>,
) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
        if node.kind() == kind && node.byte_range() == range {
            found.push(node);
        }
    }
    let [found] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    found
}

/// Counts every charge and fails the charge numbered `fail_at`, after counting it.
#[cfg(feature = "perl")]
#[derive(Default)]
struct Counting {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    checkpoints: Cell<usize>,
    charges: Cell<usize>,
    fail_at: Option<usize>,
}

#[cfg(feature = "perl")]
fn failing_at(charge: usize) -> Counting {
    Counting {
        fail_at: Some(charge),
        ..Counting::default()
    }
}

// test-lang-gates: hand-written(perl) — called from the trait methods of `Counting`
//     only, and the gate does not read the `cfg` of an `impl` block
#[cfg(feature = "perl")]
fn count_charge(budget: &Counting) -> Result<(), SourceError> {
    budget.charges.set(budget.charges.get() + 1);
    if budget.fail_at == Some(budget.charges.get()) {
        Err(SourceError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(feature = "perl")]
fn charged_totals(budget: &Counting) -> Charged {
    (budget.steps.get(), budget.bytes.get())
}

#[cfg(feature = "perl")]
impl WorkBudget for Counting {
    type Error = SourceError;

    #[cfg(feature = "perl")]
    fn checkpoint(&self) -> Result<(), SourceError> {
        self.checkpoints.set(self.checkpoints.get() + 1);
        Ok(())
    }

    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        count_charge(self)
    }

    #[cfg(feature = "perl")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        count_charge(self)
    }
}

/// `pod_block` for the block that starts at `start`, with the steps and bytes it charged.
/// With `sweep`, the reading is also cancelled once at each of its charges.
#[cfg(feature = "perl")]
fn block_charges(source: &[u8], start: usize, sweep: bool) -> (PodBlock, Charged) {
    let parser = parsed(source);
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let (_, range, _) = nodes_of(parser.ts_tree().root_node(), &["pod_statement"])
        .into_iter()
        .find(|(_, range, _)| range.start == start)
        .unwrap();
    let block = node_at(parser.ts_tree().root_node(), "pod_statement", range);
    let budget = Counting::default();
    let answer = pod_block(&context, block, &budget).unwrap();
    if sweep {
        for fail_at in 1..=budget.charges.get() {
            // expected: every charge is passed on, so each of them can cancel the reading.
            assert!(matches!(
                pod_block(&context, block, &failing_at(fail_at)),
                Err(SourceError::Cancelled)
            ));
        }
        let next = failing_at(budget.charges.get() + 1);
        assert_eq!(pod_block(&context, block, &next).unwrap(), answer);
    }
    (answer, charged_totals(&budget))
}

#[cfg(feature = "perl")]
fn tall(rows: usize, row: &str, last: &str) -> Vec<u8> {
    format!("{}{last}", row.repeat(rows)).into_bytes()
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_parse_shapes_the_reader_relies_on() {
    // expected: a block starts at its `=`. One that a `=cut` line ends runs to the end of
    // that line and holds no line feed after it, but holds the CR of a CRLF. One that no
    // `=cut` line ends runs to the end of the input, final line end included. A NUL byte
    // ends a block where it stands. The text after `__END__` or `__DATA__` is one leaf.
    let block = |range: Range<usize>, parent: &'static str| ("pod_statement", range, parent);
    let file = |range: Range<usize>| ("pod_statement", range, "source_file");
    let cases: [(&[u8], bool, Vec<Leaf>); 39] = [
        (PRIMARY, false, vec![file(0..21)]),
        (
            b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n",
            false,
            vec![file(0..22)],
        ),
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.\n",
            false,
            vec![file(12..39)],
        ),
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.",
            false,
            vec![file(12..38)],
        ),
        (
            b"=head1 NAME\n\nNever closed.\n\n\n",
            false,
            vec![file(0..29)],
        ),
        (b"=pod\n\nx\n\n=cut", false, vec![file(0..13)]),
        (b"=pod\r\n\r\nx\r\n", false, vec![file(0..11)]),
        (b"=pod\r\n\r\nx\r\n\r\n=cut\r\n", false, vec![file(0..18)]),
        (b"=pod\n", false, vec![file(0..5)]),
        (b"=pod\n=cut\n", false, vec![file(0..9)]),
        (
            b"=head1 A\n\na\n\n=cut here is text\nmy $y = 2;\n",
            false,
            vec![file(0..30)],
        ),
        (b"=pod\n\ny\n\n=CUT\nmy $x;\n", false, vec![file(0..21)]),
        (TWO_BLOCKS, false, vec![file(11..28), file(40..57)]),
        (
            b"=head1 A\n\na\n\n=cut\n=head1 B\n\nb\n\n=cut\n",
            false,
            vec![file(0..17), file(18..35)],
        ),
        (
            b"sub f {\n    my $x = 1;\n\n=pod\n\nDoc.\n\n=cut\n\n    return $x;\n}\n",
            false,
            vec![block(24..40, "block")],
        ),
        (
            b"sub o {\n  sub i {\n=head2 i\n\nInner.\n\n=cut\n  }\n}\n",
            false,
            vec![block(18..40, "block")],
        ),
        (
            MIXED_FIXTURE,
            false,
            vec![
                file(0..62),
                ("data_not_for_compiler", 91..121, "special_literal"),
            ],
        ),
        (
            b"__END__\n=head1 NAME\n\nAudit\n\n=cut\n",
            false,
            vec![("data_not_for_compiler", 7..33, "special_literal")],
        ),
        (
            b"__DATA__\nrow\n",
            false,
            vec![("data_not_for_compiler", 8..13, "special_literal")],
        ),
        (
            b"__END__",
            false,
            vec![("data_not_for_compiler", 7..7, "special_literal")],
        ),
        (
            b"sub f {\n__END__\n=pod\n\nx\n\n=cut\n",
            true,
            vec![("data_not_for_compiler", 15..30, "special_literal")],
        ),
        // The grammar opens a block wherever a statement may start, at any `=` word.
        (b"my $x = 1;\n=cut\nmy $y = 2;\n", false, vec![file(11..27)]),
        (b"my $x = 1;\n=1\ntext\n=cut\n", false, vec![file(11..23)]),
        (
            b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\n",
            false,
            vec![file(13..36)],
        ),
        (
            b"my $x = 1; =head1 NAME\n\ntext\n\n=cut\n",
            false,
            vec![file(11..34)],
        ),
        (b"\xef\xbb\xbf=pod\n\nx\n\n=cut\n", false, vec![file(3..16)]),
        // `=cutting` ends a block, and the `=cut` below it opens one.
        (
            b"=head1 A\n\ntext\n\n=cutting\nmy $y = 2;\n=cut\n",
            false,
            vec![file(0..24), file(36..41)],
        ),
        (b"=pod\n\na\x00b\n\n=cut\n", true, vec![file(0..7)]),
        (
            b"=pod\n\nx\n\n=cut\x00foo\nmy $y;\n",
            true,
            vec![file(0..13)],
        ),
        (b"=pod\r\rx\r\r=cut\rmy $x;\r", false, vec![file(0..21)]),
        (b"=pod\n\nx\n\n=cut\r", false, vec![file(0..14)]),
        // Recovery beside a block leaves the node of the block as it is.
        (b"%%%% (\n=pod\n\nx\n\n=cut\n", true, vec![file(7..20)]),
        (
            b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
            true,
            vec![file(11..24)],
        ),
        (
            b"sub f {\n=pod\n\nx\n\n=cut\n",
            true,
            vec![block(8..21, "block")],
        ),
        (
            b"sub f {\nreturn\n=pod\n\nx\n\n=cut\n1;\n}\n",
            true,
            vec![block(15..28, "block")],
        ),
        // No block where the text is part of another node.
        (
            b"my $t = <<'EOT';\n=head1 NAME\n\nnot pod\n\n=cut\nEOT\n",
            false,
            vec![],
        ),
        (b"my $s = \"abc\n=pod\n\nx\n\n=cut\n", true, vec![]),
        (b"my @a = (\n1,\n=pod\n\nx\n\n=cut\n2);\n", true, vec![]),
        (b"=pod", true, vec![]),
    ];
    for (source, recovered, expected) in cases {
        let parser = parsed(source);
        let root = parser.ts_tree().root_node();
        assert_eq!(root.kind(), "source_file", "{}", source.escape_ascii());
        assert_eq!(root.has_error(), recovered, "{}", source.escape_ascii());
        assert_eq!(
            nodes_of(root, &["pod_statement", "data_not_for_compiler"]),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
    // The scanner reads the included bytes as one text: an excluded line feed after
    // `=cut` joins the next row to the block, and an excluded `=cut` joins two blocks.
    let cut: [(&[Range<usize>], Ranges); 8] = [
        (&[0..20, 21..58], vec![11..28, 40..57]),
        (&[0..10, 11..58], vec![11..28, 40..57]),
        (&[0..28, 29..58], vec![11..39, 40..57]),
        (&[0..24, 28..58], vec![11..57]),
        (&[0..5, 6..58], vec![11..28, 40..57]),
        (&[0..57], vec![11..28, 40..57]),
        (&[0..15, 15..58], vec![11..28, 40..57]),
        (&[0..11, 12..58], vec![40..57]),
    ];
    for (ranges, expected) in cut {
        let tree = restricted(TWO_BLOCKS, ranges);
        let blocks: Vec<_> = nodes_of(tree.root_node(), &["pod_statement"])
            .into_iter()
            .map(|(_, range, _)| range)
            .collect();
        assert_eq!(blocks, expected, "{ranges:?}");
    }
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_literal_parse_shapes_the_doubt_rules_rely_on() {
    // expected: per source, whether the tree holds an error, and in tree order the nodes
    // that the rules for literal text read. A format is a call for the grammar: only the
    // identifier `format` and, where the grammar keeps it, the `.` of its last line say
    // that it is there. A here-document that the grammar reads has an initializer, and
    // its body starts on the row after the initializer's row. One that it does not read
    // is a `<<` operator, a recovery node, or part of a literal. A literal node holds no
    // other literal node. A block is a `pod_statement` wherever a statement may start.
    // Where a literal starts and ends is the grammar's claim, and the last rows show
    // claims that perl does not share: `q`, `tr` and `y` behind an escaped opening
    // bracket end late; `CORE::q` is a name and the lines behind it are code; a `/`
    // behind `time` starts a pattern; a `'` behind a name starts a string; an operator
    // word before `=>` or in a subscript starts a literal. `<<`, a blank and a quoted
    // tag is a shift. Behind a literal that the grammar does not parse, a block has no
    // node.
    let cases: [(&[u8], bool, Vec<Shape>); 69] = [
        (
            b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
            true,
            vec![("MISSING", 10..10, 0), ("pod_statement", 11..24, 1)],
        ),
        (
            b"my $s = <<\"\";\n=pod\n\nprint \"MARK\";\n",
            true,
            vec![
                ("ERROR", 8..10, 0),
                ("<<", 8..10, 0),
                ("string_double_quoted", 10..12, 0),
                ("pod_statement", 14..34, 1),
            ],
        ),
        (
            b"1;\n\x04\n=pod\n\nd\n\n=cut\n",
            true,
            vec![
                ("ERROR", 3..4, 1),
                ("ERROR", 3..4, 1),
                ("pod_statement", 5..18, 2),
            ],
        ),
        (
            b"format STDOUT =\n=pod\n.\n",
            true,
            vec![
                ("ERROR", 0..15, 0),
                ("identifier", 0..6, 0),
                ("pod_statement", 16..23, 1),
            ],
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("pod_statement", 19..32, 2),
                (".", 35..36, 8),
            ],
        ),
        (
            b"format =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("pod_statement", 12..25, 2),
                (".", 28..29, 8),
            ],
        ),
        (
            b"format STDOUT =\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 18..19, 2),
                ("pod_statement", 24..40, 5),
            ],
        ),
        (
            b"format STDOUT =\n1\n. \t\r\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 18..19, 2),
                ("pod_statement", 27..40, 5),
                ("string_double_quoted", 47..53, 10),
            ],
        ),
        (
            b"format STDOUT =\r\n1\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \"\
          MARK\";\r\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 20..21, 2),
                ("pod_statement", 29..47, 5),
                ("string_double_quoted", 54..60, 10),
            ],
        ),
        (
            b"format A =\nformat B =\n1\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("identifier", 11..17, 1),
                (".", 24..25, 3),
                ("pod_statement", 30..43, 6),
                ("string_double_quoted", 50..56, 11),
            ],
        ),
        (
            b"my $o = bless {};\nsub format { 7 }\nmy $v = $o->format;\n\n=pod\n\nd\n\n=cu\
          t\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 22..28, 1),
                ("identifier", 47..53, 2),
                ("pod_statement", 56..69, 4),
                ("string_double_quoted", 76..82, 9),
            ],
        ),
        (
            b"FORMAT STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            false,
            vec![("pod_statement", 19..32, 2), (".", 35..36, 8)],
        ),
        (
            b"format F =\na 'b\n.\nmy $s = ';\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                ("string_single_quoted", 13..27, 1),
                ("pod_statement", 29..42, 4),
                ("ERROR", 43..60, 9),
            ],
        ),
        (
            b"format F =\n1; print <<EOT;\n.\nmy $s = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                ("heredoc_initializer", 20..25, 1),
                ("<<", 20..22, 1),
                ("heredoc_body_statement", 26..42, 1),
                ("heredoc_end_identifier", 39..42, 4),
                ("pod_statement", 43..56, 5),
                ("ERROR", 57..74, 10),
            ],
        ),
        (
            b"my $s = \"@{[ <<EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_double_quoted", 8..22, 0),
                ("pod_statement", 24..37, 1),
                ("string_double_quoted", 48..54, 7),
            ],
        ),
        (
            b"my $s = qq{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_qq_quoted", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $r = 'a' =~ /(?{ <<EOT })/;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("pattern_matcher", 15..29, 0),
                ("pod_statement", 31..44, 1),
                ("string_double_quoted", 55..61, 7),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ s/a/<<EOT/e;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("substitution_pattern_s", 19..30, 1),
                ("pod_statement", 32..45, 2),
                ("string_double_quoted", 56..62, 8),
            ],
        ),
        (
            b"my $r = 'a' =~ m{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("pattern_matcher_m", 15..30, 0),
                ("pod_statement", 32..45, 1),
                ("string_double_quoted", 56..62, 7),
            ],
        ),
        (
            b"my $r = qr{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("regex_pattern_qr", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $s = `@{[ <<EOT ]}`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("backtick_quoted", 8..22, 0),
                ("pod_statement", 24..37, 1),
                ("string_double_quoted", 48..54, 7),
            ],
        ),
        (
            b"my $s = qx{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("command_qx_quoted", 8..24, 0),
                ("pod_statement", 26..39, 1),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"M\
          ARK\";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("string_double_quoted", 31..36, 1),
                ("pod_statement", 38..51, 2),
                ("string_double_quoted", 62..68, 8),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<1;\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
            true,
            vec![
                ("<<", 29..31, 1),
                ("pod_statement", 34..47, 2),
                ("ERROR", 48..49, 7),
                ("string_double_quoted", 56..62, 8),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("pod_statement", 37..50, 2),
                ("string_double_quoted", 61..67, 8),
            ],
        ),
        (
            b"print {*STDOUT} <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            true,
            vec![
                ("<<", 16..18, 0),
                ("MISSING", 21..21, 0),
                ("pod_statement", 23..36, 1),
                ("string_double_quoted", 47..53, 7),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            true,
            vec![
                ("<<", 29..31, 1),
                ("MISSING", 34..34, 1),
                ("pod_statement", 36..49, 2),
                ("string_double_quoted", 60..66, 8),
            ],
        ),
        (
            b"my $s = 1<<2;\nmy $t = $s << 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 9..11, 0),
                ("<<", 25..27, 1),
                ("pod_statement", 32..45, 3),
                ("string_double_quoted", 52..58, 8),
            ],
        ),
        (
            b"my $s = <<EOT; \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("pod_statement", 16..29, 1),
                ("heredoc_body_statement", 29..33, 5),
                ("heredoc_end_identifier", 30..33, 6),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $s = <<EOT;\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nEOT\r\nprint \"MARK\";\r\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("pod_statement", 16..34, 1),
                ("heredoc_body_statement", 34..38, 5),
                ("heredoc_end_identifier", 35..38, 6),
                ("string_double_quoted", 46..52, 7),
            ],
        ),
        (
            b"my $s = <<EOT; \nlit\nEOT\n;\nmy $t = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            true,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 25..39, 3),
                ("heredoc_end_identifier", 36..39, 5),
                ("pod_statement", 40..53, 6),
                ("ERROR", 54..71, 11),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 24..37, 3),
                ("string_double_quoted", 48..54, 9),
            ],
        ),
        (
            b"my $s = <<~EOT;\nlit\nEOT\t\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..14, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 15..23, 0),
                ("heredoc_end_identifier", 20..23, 2),
                ("pod_statement", 25..38, 3),
                ("string_double_quoted", 49..55, 9),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 24..37, 4),
                ("string_double_quoted", 44..50, 9),
            ],
        ),
        (
            b"my @s = (<<EOT, <<EOU);\na\nEOT\nb\nEOU\n\n=pod\n\nd\n\n=cut\nprint \"MARK\"\
          ;\n",
            false,
            vec![
                ("heredoc_initializer", 9..14, 0),
                ("<<", 9..11, 0),
                ("heredoc_initializer", 16..21, 0),
                ("<<", 16..18, 0),
                ("heredoc_body_statement", 23..29, 0),
                ("heredoc_end_identifier", 26..29, 2),
                ("heredoc_body_statement", 29..35, 2),
                ("heredoc_end_identifier", 32..35, 4),
                ("pod_statement", 37..50, 6),
                ("string_double_quoted", 57..63, 11),
            ],
        ),
        (
            b"my $s = <<EOT; # c\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 18..26, 0),
                ("heredoc_end_identifier", 23..26, 2),
                ("pod_statement", 28..41, 4),
                ("string_double_quoted", 48..54, 9),
            ],
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\r\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..22, 0),
                ("heredoc_end_identifier", 19..22, 2),
                ("pod_statement", 25..38, 4),
                ("string_double_quoted", 45..51, 9),
            ],
        ),
        (
            b"my $s = q!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..13, 0),
                ("pod_statement", 15..28, 1),
                ("ERROR", 30..31, 6),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my $s = q{a\\};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..13, 0),
                ("pod_statement", 15..28, 1),
                ("ERROR", 29..31, 6),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my @s = qw!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            true,
            vec![
                ("word_list_qw", 8..14, 0),
                ("pod_statement", 16..29, 1),
                ("ERROR", 31..32, 6),
                ("string_double_quoted", 39..45, 7),
            ],
        ),
        (
            b"my @s = qw{a{b};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            true,
            vec![
                ("word_list_qw", 8..15, 0),
                ("pod_statement", 17..30, 1),
                ("ERROR", 31..33, 6),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $s = q{a\\\\};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..14, 0),
                ("pod_statement", 17..30, 2),
                ("string_double_quoted", 37..43, 7),
            ],
        ),
        (
            b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..16, 0),
                ("pod_statement", 19..32, 2),
                ("string_double_quoted", 39..45, 7),
            ],
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("word_list_qw", 8..15, 0),
                ("pod_statement", 18..31, 2),
                ("string_double_quoted", 38..44, 7),
            ],
        ),
        (
            b"my %h; my $s = qq{ $h{qq{ $h{qq{ x }} }} };\n\n=pod\n\nd\n\n=cut\nprint \"MA\
          RK\";\n",
            false,
            vec![
                ("string_qq_quoted", 15..42, 0),
                ("pod_statement", 45..58, 2),
                ("string_double_quoted", 65..71, 7),
            ],
        ),
        (
            b"my $s = <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..15, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 16..34, 0),
                ("heredoc_end_identifier", 31..34, 6),
                ("string_double_quoted", 41..47, 7),
            ],
        ),
        (
            b"my $s = <<EOA;\n@{[ <<EOT ]}\n=pod\n\nd\n\n=cut\nEOT\nEOA\nprint \"MARK\";\n",
            false,
            vec![
                ("heredoc_initializer", 8..13, 0),
                ("<<", 8..10, 0),
                ("heredoc_body_statement", 14..49, 0),
                ("heredoc_end_identifier", 46..49, 8),
                ("string_double_quoted", 56..62, 9),
            ],
        ),
        (
            b"my $s = q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            false,
            vec![
                ("string_q_quoted", 8..26, 0),
                ("string_double_quoted", 34..40, 7),
            ],
        ),
        (
            b"format STDOUT =\nline\n.\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("identifier", 0..6, 0),
                (".", 21..22, 2),
                ("ERROR", 23..27, 3),
                ("string_double_quoted", 43..49, 8),
            ],
        ),
        (
            b"format Pkg'F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            true,
            vec![("ERROR", 0..39, 0), ("identifier", 0..6, 0)],
        ),
        (
            b"my $s = q'a\\'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("ERROR", 0..46, 0),
                ("ERROR", 8..13, 0),
                ("string_q_quoted", 8..13, 0),
            ],
        ),
        (
            b"my $x = (1;\nmy $s = \"\n=pod\n\";\n__END__\n=pod\n\nd\n\n=cut\n",
            true,
            vec![
                ("MISSING", 10..10, 0),
                ("string_double_quoted", 20..28, 1),
                ("data_not_for_compiler", 37..52, 4),
            ],
        ),
        (
            b"format F =\n@x;\nqq{@{[ <<TAG ]}};\n1\n.\n2;\n=pod\n\nd\n\n=cut\nTAG\n.\n2;\
          \n",
            false,
            vec![
                ("identifier", 0..6, 0),
                ("string_qq_quoted", 15..31, 2),
                (".", 35..36, 4),
                ("pod_statement", 40..53, 6),
                (".", 58..59, 12),
            ],
        ),
        (
            b"format F =\n@x;\n$a, $b\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 22..23, 3),
                ("pod_statement", 28..41, 6),
                ("string_double_quoted", 48..54, 11),
            ],
        ),
        (
            b"format F =\n@x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 0..6, 0),
                (".", 14..15, 2),
                ("pod_statement", 20..33, 5),
                ("string_double_quoted", 40..46, 10),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} <<  \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
            false,
            vec![
                ("<<", 31..33, 1),
                ("string_double_quoted", 35..40, 1),
                ("pod_statement", 42..55, 2),
                ("string_double_quoted", 66..72, 8),
            ],
        ),
        (
            b"print STDOUT << \"\";\n=pod\n\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 13..15, 0),
                ("string_double_quoted", 16..18, 0),
                ("pod_statement", 20..40, 1),
            ],
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh << \"EOT\";\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
            false,
            vec![
                ("<<", 29..31, 1),
                ("string_double_quoted", 32..37, 1),
                ("pod_statement", 40..53, 3),
                ("string_double_quoted", 60..66, 8),
            ],
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("<<", 22..24, 0),
                ("pod_statement", 30..43, 2),
                ("string_double_quoted", 50..56, 7),
            ],
        ),
        (
            b"my $x = q(a\\(b); my $s = \"x);\n=pod\n\nd\n\n=cut\n\";\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 8..28, 0),
                ("pod_statement", 30..43, 1),
                ("string_double_quoted", 44..59, 6),
                ("ERROR", 53..58, 7),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ tr{a\\{}{b}; my $s = \"x}{y};\n=pod\n\nd\n\n=cut\n\";\np\
          rint \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("transliteration_tr_or_y", 19..45, 1),
                ("pod_statement", 47..60, 2),
                ("string_double_quoted", 61..76, 7),
                ("ERROR", 70..75, 8),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ y(a\\()(b); my $s = \"x)(y);\n=pod\n\nd\n\n=cut\n\";\npr\
          int \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("transliteration_tr_or_y", 19..44, 1),
                ("pod_statement", 46..59, 2),
                ("string_double_quoted", 60..75, 7),
                ("ERROR", 69..74, 8),
            ],
        ),
        (
            b"my $t = 'a';\n$t =~ s{a}\n  {b}g; $t =~ tr{a} {b};\n\n=pod\n\nd\n\n=cut\npri\
          nt \"MARK\";\n",
            false,
            vec![
                ("string_single_quoted", 8..11, 0),
                ("substitution_pattern_s", 19..30, 1),
                ("transliteration_tr_or_y", 38..47, 2),
                ("pod_statement", 50..63, 4),
                ("string_double_quoted", 70..76, 9),
            ],
        ),
        (
            b"my $x = CORE::q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            false,
            vec![
                ("identifier", 14..15, 0),
                ("pod_statement", 17..30, 1),
                ("string_double_quoted", 40..46, 7),
            ],
        ),
        (
            b"my $z = time / 2; my $s = \"/; 1; \"; my $u = \";\n=pod\n\nd\n\n=cut\n\";\n# \
          \"\nprint \"MARK\";\n",
            true,
            vec![
                ("pattern_matcher", 13..28, 0),
                ("string_double_quoted", 33..45, 0),
                ("pod_statement", 47..60, 1),
                ("ERROR", 61..67, 6),
                ("string_double_quoted", 61..67, 6),
                ("string_double_quoted", 74..80, 8),
            ],
        ),
        (
            b"my $x = Foo'bar(1); my $s = '; 1; '; my $u = ';\n=pod\n\nd\n\n=cut\n';\n# '\
          \nprint \"MARK\";\n",
            true,
            vec![
                ("string_single_quoted", 11..29, 0),
                ("string_single_quoted", 34..46, 0),
                ("pod_statement", 48..61, 1),
                ("ERROR", 62..68, 6),
                ("string_single_quoted", 62..68, 6),
                ("string_double_quoted", 75..81, 8),
            ],
        ),
        (
            b"my %k = (y => 1, s => 2, q => 3);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("transliteration_tr_or_y", 9..28, 0),
                ("pod_statement", 35..48, 2),
                ("string_double_quoted", 55..61, 7),
            ],
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            false,
            vec![
                ("pattern_matcher", 14..17, 0),
                ("string_single_quoted", 19..24, 0),
                ("pattern_matcher", 48..51, 0),
                ("pattern_matcher", 66..69, 0),
                ("pod_statement", 76..89, 2),
                ("string_double_quoted", 96..102, 7),
            ],
        ),
        (
            b"my %k; my $v = $k{q}; my $w = { 1 };\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            true,
            vec![
                ("string_q_quoted", 18..35, 0),
                ("MISSING", 35..35, 0),
                ("pod_statement", 38..51, 2),
                ("string_double_quoted", 58..64, 7),
            ],
        ),
    ];
    for (source, recovered, expected) in cases {
        let parser = parsed(source);
        let root = parser.ts_tree().root_node();
        assert_eq!(root.has_error(), recovered, "{}", source.escape_ascii());
        assert_eq!(
            shapes_of(root, source),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_block_documents_the_file() {
    assert_views([
        (PRIMARY, documents([doc(0..21, "=head2 f\n\nAudit")])),
        (
            b"=head2 f\n\nAudit\n\n=cut\n\nsub f {}\nsub f {}",
            documents([doc(0..21, "=head2 f\n\nAudit")]),
        ),
        (
            b"package Foo;\n\n=head1 NAME\n\nFoo\n\n=cut\n\nsub new {}\n",
            documents([doc(14..36, "=head1 NAME\n\nFoo")]),
        ),
        (
            b"=begin html\n\n<p>x</p>\n\n=end html\n\n=cut\n",
            documents([doc(0..38, "=begin html\n\n<p>x</p>\n\n=end html")]),
        ),
        (
            b"=begin :comment\n\nx\n\n=end :comment\n\n=cut\n",
            documents([doc(0..39, "=begin :comment\n\nx\n\n=end :comment")]),
        ),
        (
            b"=item\t*\n\ny\n\n=cut\n",
            documents([doc(0..16, "=item\t*\n\ny")]),
        ),
        (
            b"=pod\x0bx\n\ny\n\n=cut\n",
            documents([doc(0..15, "=pod\x0bx\n\ny")]),
        ),
        (
            b"=pod\n\n__END__\n\n=cut\nmy $x;\n",
            documents([doc(0..19, "=pod\n\n__END__")]),
        ),
    ]);
    let parser = parsed(PRIMARY);
    let root = parser.ts_tree().root_node();
    for index in indexes(&parser, Retain::Nothing) {
        let association = index.associations().next().unwrap();
        let file = index.node_associations(root).unwrap();
        assert!(!file.projection_pending());
        assert_eq!(file.associations().collect::<Vec<_>>(), [association]);
        assert_eq!(file.assessments().count(), 0);
        // Today's outcome, expected to change: no `sub` is an owner in this unit, so a
        // query for one is unsupported and not an empty answer.
        let sub = node_at(root, "function_definition", 23..31);
        assert!(matches!(
            index.node_associations(sub),
            Err(OwnerError::UnsupportedOwner)
        ));
        assert!(matches!(
            index.node_projection(sub),
            Err(OwnerError::UnsupportedOwner)
        ));
        // A block is no owner either.
        let block = node_at(root, "pod_statement", 0..21);
        assert!(matches!(
            index.node_owners(block),
            Err(OwnerError::UnsupportedOwner)
        ));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_blocks_are_documents_in_source_order() {
    assert_views([
        (
            b"my $x = 1;\n=head1 A\n\na\n\n=cut\nmy $y = 2;\n=head1 B\n\nb\n\n=cut\nmy $z = 3;\n",
            documents([doc(11..28, "=head1 A\n\na"), doc(40..57, "=head1 B\n\nb")]),
        ),
        (
            b"=head1 A\n\na\n\n=cut\n=head1 B\n\nb\n\n=cut\n",
            documents([doc(0..17, "=head1 A\n\na"), doc(18..35, "=head1 B\n\nb")]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_block_in_a_body_is_scoped_to_the_body() {
    let cases: [(&[u8], View, Range<usize>); 2] = [
        (
            b"sub f {\n    my $x = 1;\n\n=pod\n\nDoc.\n\n=cut\n\n    return $x;\n}\n",
            documents([doc(24..40, "=pod\n\nDoc.")]),
            6..59,
        ),
        (
            b"sub o {\n  sub i {\n=head2 i\n\nInner.\n\n=cut\n  }\n}\n",
            documents([doc(18..40, "=head2 i\n\nInner.")]),
            16..45,
        ),
    ];
    for (source, expected, body) in cases {
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        let parser = parsed(source);
        for index in indexes(&parser, Retain::Nothing) {
            let association = index.associations().next().unwrap();
            // expected: the body that holds the block, not the file and not the sub.
            assert_eq!(
                site(association.get().lexical_scope.node()),
                ("block", body.clone())
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_fragment_ends_before_the_line_end_of_its_last_line() {
    let crlf = |fragment: Range<usize>, text: &str, name: Range<usize>, body: Range<usize>| {
        mapped(
            fragment,
            text,
            vec![
                (0..name.len(), Copied(name.clone())),
                (name.len()..name.len() + 1, Decoded(name.end..name.end + 2)),
                (
                    name.len() + 1..name.len() + 2,
                    Decoded(name.end + 2..name.end + 4),
                ),
                (name.len() + 2..text.len(), Copied(body)),
            ],
        )
    };
    assert_views([
        // A `=cut` line ends the block: its line end is outside the node and the fragment.
        (
            b"=head1 A\n\na\n\n=cut here is text\nmy $y = 2;\n",
            documents([doc(0..30, "=head1 A\n\na")]),
        ),
        (b"=pod\n\nx\n\n=cut", documents([doc(0..13, "=pod\n\nx")])),
        // The node holds the CR of `=cut\r\n`; the fragment does not.
        (
            b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n",
            documents([crlf(0..21, "=head1 A\n\na", 0..8, 12..13)]),
        ),
        (
            b"=pod\r\n\r\nx\r\n\r\n=cut\r\n",
            documents([crlf(0..17, "=pod\n\nx", 0..4, 8..9)]),
        ),
        // No `=cut` line: the node holds the final line end; the fragment does not.
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.\n",
            documents([doc(12..38, "=head1 NAME\n\nNever closed.")]),
        ),
        (
            b"my $x = 1;\n\n=head1 NAME\n\nNever closed.",
            documents([doc(12..38, "=head1 NAME\n\nNever closed.")]),
        ),
        (
            b"=head1 NAME\n\nNever closed.\n\n\n",
            documents([doc(0..28, "=head1 NAME\n\nNever closed.")]),
        ),
        (
            b"=pod\r\n\r\nx\r\n",
            documents([crlf(0..9, "=pod\n\nx", 0..4, 8..9)]),
        ),
        (b"=pod\n", documents([doc(0..4, "=pod")])),
        // `=CUT` ends nothing for perl, for POD processors and for the grammar.
        (
            b"=pod\n\ny\n\n=CUT\nmy $x;\n",
            documents([doc(0..20, "=pod\n\ny\n\n=CUT\nmy $x;")]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_text_keeps_its_lines_and_drops_the_cut_line() {
    assert_views([
        (b"=pod\n=cut\n", documents([doc(0..9, "=pod")])),
        // `=cut` before a digit or a form feed ends the block for all three readers.
        (
            b"=head1 A\n\ntext\n\n=cut2\nmy $y = 2;\n",
            documents([doc(0..21, "=head1 A\n\ntext")]),
        ),
        (
            b"=pod\n\nx\n\n=cut\x0cfoo\nmy $y;\n",
            documents([doc(0..17, "=pod\n\nx")]),
        ),
        // Only a line of no bytes is an empty boundary line: a line of one space stays.
        (
            b"=pod\n\nx\n \n=cut\n",
            documents([doc(0..14, "=pod\n\nx\n ")]),
        ),
        // Blank lines inside stay; those before the `=cut` line go.
        (
            b"=pod\n\n\n\ny\n\n\n=cut\n",
            documents([doc(0..16, "=pod\n\n\n\ny")]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_comment_only_pod_is_no_document() {
    let sources: [(&[u8], View, Ranges); 3] = [
        (
            b"=begin comment\n\nnot prose\n\n=end comment\n\n=cut\n",
            View::default(),
            vec![0..45],
        ),
        // Two candidates, in source order.
        (
            b"=begin comment\n\na\n\n=end comment\n\n=cut\n\nmy $x;\n\n=for comment b\n\n=cut\n",
            View::default(),
            vec![0..37, 47..67],
        ),
        // The comment-only block is withheld beside a block that is published.
        (
            b"=begin comment\n\nx\n\n=end comment\n\n=cut\n\n=head1 A\n\na\n\n=cut\n",
            documents([doc(39..56, "=head1 A\n\na")]),
            vec![0..37],
        ),
    ];
    for (source, mut expected, candidates) in sources {
        assert_eq!(view(source), expected, "{}", source.escape_ascii());
        // A comment-only block is a directive, not an unattached candidate.
        assert_eq!(
            retained(source, Retain::Unbound),
            expected,
            "{}",
            source.escape_ascii()
        );
        expected.evidence = candidates
            .into_iter()
            .map(|range| (CandidateDisposition::Directive, range))
            .collect();
        assert_eq!(
            retained(source, Retain::Directives),
            expected,
            "{}",
            source.escape_ascii()
        );
    }
    // A document is no candidate under either option.
    for retain in [Retain::Unbound, Retain::Directives] {
        assert_eq!(retained(PRIMARY, retain), view(PRIMARY));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_with_a_comment_region_has_pending_text() {
    assert_views([
        (
            MIXED_FIXTURE,
            View {
                documents: vec![pending_text(0..62)],
                assessments: unordered([mixed(0..62, 0), data_section(91..121)]),
                ..View::default()
            },
        ),
        (
            b"=for comment\nInternal note.\n\n=head1 NAME\n\nFoo - bar\n\n=cut\n",
            View {
                documents: vec![pending_text(0..57)],
                assessments: vec![mixed(0..57, 0)],
                ..View::default()
            },
        ),
        // The assessment names the document whose text waits, here the second.
        (
            b"=head1 A\n\na\n\n=cut\n\n=head1 B\n\n=for comment x\n\nb\n\n=cut\n",
            View {
                documents: vec![doc(0..17, "=head1 A\n\na"), pending_text(19..52)],
                assessments: vec![mixed(19..52, 1)],
                ..View::default()
            },
        ),
        // A comment command line inside a paragraph is ordinary text for `Pod::Simple`.
        // This unit does not tell paragraphs apart and lets the text wait.
        (
            b"=head1 A\n\ntext\n=for comment x\n\nmore\n\n=cut\n",
            View {
                documents: vec![pending_text(0..41)],
                assessments: vec![mixed(0..41, 0)],
                ..View::default()
            },
        ),
    ]);
    // The document keeps no complete text and no source-cleaned text.
    let parser = parsed(MIXED_FIXTURE);
    for index in indexes(&parser, Retain::Nothing) {
        let document = index.documents().next().unwrap();
        assert!(index.complete_text(document).unwrap().is_none());
        assert!(index.source_cleaned_text(document).unwrap().is_none());
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_block_with_a_foreign_opener_is_withheld() {
    // These are undecided states, not answers: perl, `Pod::Simple` and the grammar do not
    // read the same block here. No option retains such a block as a candidate.
    let cases: [(&[u8], Range<usize>); 12] = [
        (b" =pod\n\nx\n\n=cut\n", 1..14),
        (b"my $x = 1;\n=cut\nmy $y = 2;\n", 11..27),
        (b"=cut\n\nx\n\n=cut\nmy $y;\n", 0..13),
        (b"my $x = 1;\n=1\ntext\n=cut\n", 11..23),
        (b"my $x = 1;\n=_x\ntext\n=cut\n", 11..24),
        (b"=head-x\n\ntext\n\n=cut\n", 0..19),
        (b"=pod.\n\nx\n\n=cut\n", 0..14),
        (b"my $x;\n=cutting\ntext\n=cut\nmy $y;\n", 7..25),
        (b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\n", 13..36),
        (b"my $x = 1; =head1 NAME\n\ntext\n\n=cut\n", 11..34),
        (b"my $x;\r=pod\n\nx\n\n=cut\n", 7..20),
        // perl and `Pod::Simple` skip a byte order mark at the start of the input and
        // read this block. This unit does not.
        (b"\xef\xbb\xbf=pod\n\nx\n\n=cut\n", 3..16),
    ];
    for (source, block) in cases {
        let expected = pending([withheld(OPENER, block)]);
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                retained(source, retain),
                expected,
                "{}",
                source.escape_ascii()
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_block_with_a_disputed_end_is_withheld() {
    let cases: [(&[u8], View); 3] = [
        // perl does not end a block at `=cutting`; the grammar does, and reads the
        // `=cut` below it as a second block.
        (
            b"=head1 A\n\ntext\n\n=cutting\nmy $y = 2;\n=cut\n",
            pending([withheld(CLOSER, 0..24), withheld(OPENER, 36..41)]),
        ),
        // The grammar ends a block at a NUL byte; perl reads on to `=cut`. The byte is an
        // `ERROR` node, and the `=cut` line behind it has no node: the file is marked.
        (
            b"=pod\n\na\x00b\n\n=cut\n",
            View {
                assessments: unordered([withheld(CLOSER, 0..7), hidden("ERROR", 7..8)]),
                ..View::default()
            },
        ),
        (
            b"=pod\n\nx\n\n=cut\x00foo\nmy $y;\n",
            pending([withheld(CLOSER, 0..13)]),
        ),
    ];
    for (source, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                retained(source, retain),
                expected,
                "{}",
                source.escape_ascii()
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_block_with_a_lone_carriage_return_is_withheld() {
    assert_views([
        (
            b"=pod\n\na\rb\r\n\n=cut\n",
            pending([withheld(LINE_END, 0..16)]),
        ),
        (
            b"=pod\r\rx\r\r=cut\rmy $x;\r",
            pending([withheld(LINE_END, 0..21)]),
        ),
        (
            b"=pod\n\na\rb\n\n=cut\n",
            pending([withheld(LINE_END, 0..15)]),
        ),
        (b"=pod\n\nx\n\n=cut\r", pending([withheld(LINE_END, 0..14)])),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_withheld_block_leaves_the_next_block_published() {
    assert_views([(
        b"my $x = 1;\n  =head1 X\n\nx\n\n=cut\n=head1 A\n\na\n\n=cut\n",
        View {
            documents: vec![doc(31..48, "=head1 A\n\na")],
            assessments: vec![withheld(OPENER, 13..30)],
            ..View::default()
        },
    )]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_beside_excluded_bytes_is_withheld() {
    let b = || doc(40..57, "=head1 B\n\nb");
    let a = || doc(11..28, "=head1 A\n\na");
    let cases: [(&[Range<usize>], View); 6] = [
        // An excluded byte inside the block.
        (
            &[0..20, 21..58],
            View {
                documents: vec![b()],
                assessments: vec![withheld(EXCLUDED, 11..28)],
                ..View::default()
            },
        ),
        // The byte before the block: whether the block starts a line is not known.
        (
            &[0..10, 11..58],
            View {
                documents: vec![b()],
                assessments: vec![withheld(EXCLUDED, 11..28)],
                ..View::default()
            },
        ),
        // The line feed after `=cut`: the grammar joins the next row to the block.
        (
            &[0..28, 29..58],
            View {
                documents: vec![b()],
                assessments: vec![withheld(EXCLUDED, 11..39)],
                ..View::default()
            },
        ),
        // The `=cut` line itself: the grammar joins both blocks.
        (&[0..24, 28..58], pending([withheld(EXCLUDED, 11..57)])),
        // The byte after the block, here the last byte of the input.
        (
            &[0..57],
            View {
                documents: vec![a()],
                assessments: vec![withheld(EXCLUDED, 40..57)],
                ..View::default()
            },
        ),
        // Excluded bytes elsewhere withhold no block. Today's outcome of the core, not a
        // rule of this unit: the statement that the gap cuts is reported as a pending
        // declaration.
        (
            &[0..5, 6..58],
            View {
                documents: vec![a(), b()],
                assessments: vec![Pending {
                    axis: AssessmentAxis::Owner,
                    reason: CUT_NODE,
                    node: ("binary_expression", 0..9),
                    document: None,
                }],
                ..View::default()
            },
        ),
    ];
    for (ranges, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                restricted_view(TWO_BLOCKS, ranges, retain),
                expected,
                "{ranges:?}"
            );
        }
    }
    // Control: the same source without an excluded byte publishes both blocks.
    assert_eq!(view(TWO_BLOCKS), documents([a(), b()]));
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_across_adjacent_ranges_keeps_one_piece_per_range() {
    let tree = restricted(TWO_BLOCKS, &[0..15, 15..58]);
    let index = collect(
        LANG::Perl,
        &tree,
        TWO_BLOCKS,
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    // expected: no byte is excluded, so the block is read; its fragment and the first
    // line of its text are cut where the two ranges meet.
    assert_eq!(
        published(&index, tree.root_node()),
        documents([
            Doc {
                fragment: vec![11..15, 15..28],
                text: Text::Complete(
                    "=head1 A\n\na".to_owned(),
                    vec![
                        (0..4, Copied(11..15)),
                        (4..8, Copied(15..19)),
                        (8..9, Copied(19..20)),
                        (9..10, Copied(20..21)),
                        (10..11, Copied(21..22)),
                    ]
                ),
            },
            doc(40..57, "=head1 B\n\nb"),
        ])
    );
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_after_a_recovery_node_is_withheld() {
    // After an `ERROR` or a missing node the grammar has lost its place, and perl may
    // read the lines that follow as literal text. In the first three sources it does: a
    // here-document with an empty tag, and a format. In the next two perl stops reading
    // at a control byte. perl rejects the last four files. Every block that follows
    // such a node in the tree is withheld. Each of these blocks has a node, so no file
    // is marked for POD that the grammar hides.
    assert_views([
        (
            b"my $s = <<\"\";\n=pod\n\nprint \"MARK\";\n",
            pending([withheld(RECOVERY, 14..34)]),
        ),
        (
            b"my $s = <<'';\n=pod\n\nprint \"MARK\";\n",
            pending([withheld(RECOVERY, 14..34)]),
        ),
        (
            b"format STDOUT =\n=pod\n.\n",
            pending([withheld(RECOVERY, 16..23)]),
        ),
        (
            b"1;\n\x04\n=pod\n\nd\n\n=cut\n",
            pending([withheld(RECOVERY, 5..18)]),
        ),
        (
            b"1;\n\x1a\n=pod\n\nd\n\n=cut\n",
            pending([withheld(RECOVERY, 5..18)]),
        ),
        (
            b"%%%% (\n=pod\n\nx\n\n=cut\n",
            pending([withheld(RECOVERY, 7..20)]),
        ),
        (
            b"my $x = 1,\n=pod\n\nx\n\n=cut\n2;\n",
            pending([withheld(RECOVERY, 11..24)]),
        ),
        (
            b"sub f {\nreturn\n=pod\n\nx\n\n=cut\n1;\n}\n",
            pending([withheld(RECOVERY, 15..28)]),
        ),
        (
            b"my $x = (1;\n\n=head1 A\n\na\n\n=cut\n",
            pending([withheld(RECOVERY, 13..30)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_before_the_first_recovery_node_is_read() {
    // A recovery node says nothing about the lines before it. perl rejects each of these
    // files; `Pod::Simple` reads the block, and so does this unit. In the last source the
    // block after the node is withheld.
    assert_views([
        (
            b"sub f {\n=pod\n\nx\n\n=cut\n",
            documents([doc(8..21, "=pod\n\nx")]),
        ),
        (
            b"=pod\n\nx\n\n=cut\n}}}}\n",
            documents([doc(0..13, "=pod\n\nx")]),
        ),
        (
            b"=pod\n\nx\n\n=cut\n}}}}\n=pod\n\ny\n\n=cut\n",
            View {
                documents: vec![doc(0..13, "=pod\n\nx")],
                assessments: unordered([withheld(RECOVERY, 19..32)]),
                ..View::default()
            },
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_in_a_format_is_withheld() {
    // perl reads the lines of a format as picture text, up to a line that holds a dot and
    // then only blanks, tabs or carriage returns. The grammar knows no format: it reads
    // a call, and a block wherever a line starts with `=`. The opener is the keyword,
    // an optional name, `=` and the end of the line, with white space and comments
    // between them. A format without a last line runs to the end of the input. A
    // comment-only block in a format is reported like any block there, and a block that
    // the reader's own tests withhold keeps their reason. In the last four sources the
    // grammar reads the picture text into the code after the format, or the format
    // goes on behind a line that looks like its last.
    assert_views([
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 19..32)]),
        ),
        (
            b"format =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 12..25)]),
        ),
        (
            b"format Pkg::F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 19..32)]),
        ),
        (
            b"format STDOUT=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 18..31)]),
        ),
        (
            b"format STDOUT =  \t\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 22..35)]),
        ),
        (
            b"format STDOUT = # c\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 23..36)]),
        ),
        (
            b"format\nSTDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 19..32)]),
        ),
        (
            b"format STDOUT\n=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 19..32)]),
        ),
        (
            b"format # a\nSTDOUT # b\n=\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 27..40)]),
        ),
        (
            b"  format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 21..34)]),
        ),
        (
            b"my $q = 1; format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 30..43)]),
        ),
        (
            b"sub g {\nformat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n}\n",
            pending([withheld(FORMAT, 27..40)]),
        ),
        (
            b"format STDOUT =\r\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 20..33)]),
        ),
        (
            b"L: format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 22..35)]),
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n2;\n",
            pending([withheld(FORMAT, 19..32)]),
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            View {
                documents: vec![doc(41..57, "=pod\n\nreal")],
                assessments: unordered([withheld(FORMAT, 19..32)]),
                ..View::default()
            },
        ),
        (
            b"format STDOUT =\n1;\n=begin comment\n\nx\n\n=end comment\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 19..56)]),
        ),
        (
            b"format STDOUT =\n1;\n=cut\n1\n.\n2;\n",
            pending([withheld(OPENER, 19..31)]),
        ),
        (
            b"format F =\na 'b\n.\nmy $s = ';\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 29..42)]),
        ),
        (
            b"format F =\n1; print <<EOT;\n.\nmy $s = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            pending([withheld(FORMAT, 43..56)]),
        ),
        (
            b"format STDOUT =\n1\n. x;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([withheld(FORMAT, 23..36)]),
        ),
        (
            b"format STDOUT =\n1\n .\n2;\n=pod\n\nd\n\n=cut\n3\n.\n4;\n",
            pending([withheld(FORMAT, 24..37)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_after_a_format_whose_end_is_not_known_is_withheld() {
    // Behind a picture line that holds `@` or `^` perl reads one line of values, which is
    // code: a here-document or a literal that opens there runs over the dot line, and
    // the format ends at a later dot. In the first two sources perl reads the block as
    // text of that here-document and of that literal. The unit reads a value line only
    // if it is a list of named variables. Any other value line makes the end of the
    // format unknown, and every later block is withheld. perl rejects the third file: a
    // dot and a blank end no format where values are read.
    assert_views([
        (
            b"format F =\n@x;\nqq{@{[ <<TAG ]}};\n1\n.\n2;\n=pod\n\nd\n\n=cut\nTAG\n.\n2;\
          \n",
            pending([withheld(FORMAT, 40..53)]),
        ),
        (
            b"format F =\n@x;\nq!abc\\!;\n1\n.\n2;\n=pod\n\nd\n\n=cut\n!\n.\n2;\n",
            pending([withheld(FORMAT, 31..44)]),
        ),
        (
            b"format F =\n@x\n. \n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 21..34)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_outside_a_format_is_a_document() {
    // Controls: a block after the last line of a format and one before a format; a last
    // line with blanks and with CRLF; two formats; an opener line inside a body, which
    // is picture text; and the word `format` where it opens nothing: a method, a hash
    // key, a subscript, in a string, in a comment, in upper case, and as part of
    // another word. In the last six sources a picture line holds a field: the value
    // line behind it is a list of named variables, or the dot that ends the format
    // stands there; a comment line with a field takes no value line.
    assert_views([
        (
            b"format STDOUT =\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            documents([doc(24..40, "=pod\n\nreal")]),
        ),
        (
            b"=pod\n\nd\n\n=cut\nformat STDOUT =\n1\n.\n2;\nprint \"MARK\";\n",
            documents([doc(0..13, "=pod\n\nd")]),
        ),
        (
            b"format STDOUT =\n1\n. \t\r\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(27..40, "=pod\n\nd")]),
        ),
        (
            b"format STDOUT =\r\n1\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \"\
          MARK\";\r\n",
            documents([mapped(
                29..46,
                "=pod\n\nd",
                vec![
                    (0..4, Copied(29..33)),
                    (4..5, Decoded(33..35)),
                    (5..6, Decoded(35..37)),
                    (6..7, Copied(37..38)),
                ],
            )]),
        ),
        (
            b"format A =\n1\n.\n2;\nformat B =\n3\n.\n4;\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
            documents([doc(37..50, "=pod\n\nd")]),
        ),
        (
            b"format A =\nformat B =\n1\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(30..43, "=pod\n\nd")]),
        ),
        (
            b"my $o = bless {};\nsub format { 7 }\nmy $v = $o->format;\n\n=pod\n\nd\n\n=cu\
          t\nprint \"MARK\";\n",
            documents([doc(56..69, "=pod\n\nd")]),
        ),
        (
            b"my %h = (format => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(24..37, "=pod\n\nd")]),
        ),
        (
            b"my %h = (format\n=> 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(24..37, "=pod\n\nd")]),
        ),
        (
            b"my $s = 'format STDOUT =';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(28..41, "=pod\n\nd")]),
        ),
        (
            b"# format STDOUT =\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(19..32, "=pod\n\nd")]),
        ),
        (
            b"FORMAT STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            documents([doc(19..32, "=pod\n\nd")]),
        ),
        (
            b"my %h; $h{format} =\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(24..37, "=pod\n\nd")]),
        ),
        (
            b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            documents([doc(35..48, "=pod\n\nd")]),
        ),
        (
            b"forXmat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            documents([doc(20..33, "=pod\n\nd")]),
        ),
        (
            b"format F =\n@x;\n$a, $b\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(28..41, "=pod\n\nd")]),
        ),
        (
            b"format F =\n@x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(20..33, "=pod\n\nd")]),
        ),
        (
            b"format F =\n~~@x;\n$a\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(26..39, "=pod\n\nd")]),
        ),
        (
            b"format F =\n1\n# @x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(24..37, "=pod\n\nd")]),
        ),
        (
            b"format F =\n@x;\n$a,\n@y;\n$b;\ntext\n. \n2;\n\n=pod\n\nd\n\n=cut\nprint \"M\
          ARK\";\n",
            documents([doc(39..52, "=pod\n\nd")]),
        ),
        (
            b"format F =\r\n@x;\r\n$a\r\n.\r\n2;\r\n\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nprint \
          \"MARK\";\r\n",
            documents([mapped(
                30..47,
                "=pod\n\nd",
                vec![
                    (0..4, Copied(30..34)),
                    (4..5, Decoded(34..36)),
                    (5..6, Decoded(36..38)),
                    (6..7, Copied(38..39)),
                ],
            )]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_after_an_unread_here_document_opener_is_withheld() {
    // perl opens a here-document inside a literal that it evaluates: a string, `qq`,
    // backticks, `qx`, a pattern or a substitution. The grammar reads no code in a
    // literal. It also reads the opener behind a file handle in a variable or in braces
    // as a shift. In both cases the lines of the here-document are code for the grammar,
    // and every later block is withheld. In the last five sources blanks or tabs stand
    // between `<<` or `<<~` and a quoted tag, which may be empty.
    assert_views([
        (
            b"my $s = \"<<<EOT\";\n\n=pod\n\nd\n\n=cut\n",
            pending([withheld(UNREAD_OPENER, 19..32)]),
        ),
        (
            b"my $fh = \\*STDOUT; print {$fh}\t<<2;\n=pod\n\nmy $inside = 6 * 7;\n\n=cut\n2\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 36..67)]),
        ),
        (
            b"my $s = \"@{[ <<EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 24..37)]),
        ),
        (
            b"my $s = \"${\\ <<EOT }\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 23..36)]),
        ),
        (
            b"my $s = qq{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 26..39)]),
        ),
        (
            b"my $r = 'a' =~ /(?{ <<EOT })/;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 31..44)]),
        ),
        (
            b"my $t = 'a';\n$t =~ s/a/<<EOT/e;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 32..45)]),
        ),
        (
            b"my $r = 'a' =~ m{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 32..45)]),
        ),
        (
            b"my $r = qr{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 26..39)]),
        ),
        (
            b"my $s = \"@{[ <<~EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 25..38)]),
        ),
        (
            b"my $s = qq{@{[ <<\"EOT\" ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 28..41)]),
        ),
        (
            b"my $s = qq{@{[ <<1 ]}};\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 24..37)]),
        ),
        (
            b"my $s = qq{@{[ <<\"\" ]}};\n=pod\n\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 25..45)]),
        ),
        (
            b"my $s = `@{[ <<EOT ]}`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 24..37)]),
        ),
        (
            b"my $s = qx{@{[ <<EOT ]}};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 26..39)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"M\
          ARK\";\n",
            pending([withheld(UNREAD_OPENER, 38..51)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
            pending([withheld(UNREAD_OPENER, 38..51)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<`EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
            pending([withheld(UNREAD_OPENER, 38..51)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<\\EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MAR\
          K\";\n",
            pending([withheld(UNREAD_OPENER, 37..50)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<_E;\n=pod\n\nd\n\n=cut\n_E\nprint \"MARK\";\
          \n",
            pending([withheld(UNREAD_OPENER, 35..48)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<1;\n=pod\n\nd\n\n=cut\n1\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 34..47)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            pending([withheld(UNREAD_OPENER, 37..50)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\
          \";\n",
            pending([withheld(UNREAD_OPENER, 36..49)]),
        ),
        (
            b"print {*STDOUT} <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 23..36)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} <<  \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 42..55)]),
        ),
        (
            b"print STDOUT << \"\";\n=pod\n\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 20..40)]),
        ),
        (
            b"print {*STDOUT} <<\t\t'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 27..40)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} <<~ \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 42..55)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} << `EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"\
          MARK\";\n",
            pending([withheld(UNREAD_OPENER, 41..54)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_beside_a_misread_here_document_is_withheld() {
    // The grammar starts the body of a here-document late when a blank or a carriage
    // return stands behind the statement of its opener, and it ends the body at a tag
    // line that holds a blank or a tab behind the tag, where perl reads on. A block
    // between an opener and its body, and every block after such a body, is withheld.
    assert_views([
        (
            b"my $s = <<EOT; \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(HEREDOC, 16..29)]),
        ),
        (
            b"my $s = <<EOT;\r\n=pod\r\n\r\nd\r\n\r\n=cut\r\nEOT\r\nprint \"MARK\";\r\n",
            pending([withheld(HEREDOC, 16..34)]),
        ),
        (
            b"my $s = <<EOT; \nlit\nEOT\n;\nmy $t = '\nEOT\n=pod\n\nd\n\n=cut\n';\nprint \
          \"MARK\";\n",
            pending([withheld(HEREDOC, 40..53)]),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(HEREDOC, 24..37)]),
        ),
        (
            b"my $s = <<~EOT;\nlit\nEOT\t\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            pending([withheld(HEREDOC, 25..38)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_after_a_literal_perl_reads_differently_is_withheld() {
    // Where a literal starts and ends is a claim of the grammar. The unit reads the
    // spelling of each literal node by perl's rule: a backslash hides the next byte,
    // brackets of the delimiter's kind nest, any other delimiter ends the literal at its
    // next occurrence. Where perl ends the literal before or behind the last byte of the
    // node, every later block is withheld. The grammar ends `q` and `qw` at a delimiter
    // that a backslash escapes, and `qw` at the first closing bracket although brackets
    // nest: too early. It ends `q`, `tr` and `y` behind an escaped opening bracket too
    // late, and perl reads the block as text of the string behind them. The last three
    // sources are literals that perl starts where the grammar does not, and the other
    // way round: `CORE::q`, which the grammar reads as a name; a division behind `time`,
    // which it reads as the start of a pattern; and the old package separator, which it
    // reads as the start of a string.
    assert_views([
        (
            b"my $s = q!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 15..28)]),
        ),
        (
            b"my $s = q{a\\};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 15..28)]),
        ),
        (
            b"my @s = qw!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 16..29)]),
        ),
        (
            b"my @s = qw{a{b};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 17..30)]),
        ),
        (
            b"my $x = q(a\\(b); my $s = \"x);\n=pod\n\nd\n\n=cut\n\";\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 30..43)]),
        ),
        (
            b"my $t = 'a';\n$t =~ tr{a\\{}{b}; my $s = \"x}{y};\n=pod\n\nd\n\n=cut\n\";\np\
          rint \"MARK\";\n",
            pending([withheld(LITERAL, 47..60)]),
        ),
        (
            b"my $t = 'a';\n$t =~ y(a\\()(b); my $s = \"x)(y);\n=pod\n\nd\n\n=cut\n\";\npr\
          int \"MARK\";\n",
            pending([withheld(LITERAL, 46..59)]),
        ),
        (
            b"my $x = CORE::q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 17..30)]),
        ),
        (
            b"my $z = time / 2; my $s = \"/; 1; \"; my $u = \";\n=pod\n\nd\n\n=cut\n\";\n# \
          \"\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 47..60)]),
        ),
        (
            b"my $x = Foo'bar(1); my $s = '; 1; '; my $u = ';\n=pod\n\nd\n\n=cut\n';\n# '\
          \nprint \"MARK\";\n",
            pending([withheld(LITERAL, 48..61)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_pod_after_a_literal_the_grammar_reads_is_a_document() {
    // Controls: `<<` and a word in a literal that perl does not evaluate; shift
    // operators; `<<` in a string with no tag behind it; a here-document that the
    // grammar reads, also two on one row, with a comment behind the opener, and with a
    // CRLF behind the tag; an escaped backslash before the delimiter of `q`; brackets
    // that nest in `q`; a plain word list; literals in the code of a literal; a shift
    // with blanks and no quote behind the operator; two bracketed parts with white space
    // between; `#` and `'` as delimiters; flags; an escaped closing bracket in `qq` and
    // `qr`; patterns behind `split`, `=~` and `grep`; divisions; a name of two bytes.
    assert_views([
        (
            b"my @s = qw{<<EOT};\n\n=pod\n\nd\n\n=cut\n",
            documents([doc(20..33, "=pod\n\nd")]),
        ),
        (
            b"my $s = 'a <<EOT b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(22..35, "=pod\n\nd")]),
        ),
        (
            b"my $s = q{<<EOT};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(19..32, "=pod\n\nd")]),
        ),
        (
            b"my $s = 1<<2;\nmy $t = $s << 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(32..45, "=pod\n\nd")]),
        ),
        (
            b"my $s = \"<<< x >>> 1 << 2\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(29..42, "=pod\n\nd")]),
        ),
        (
            b"my $s = \"<<\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(15..28, "=pod\n\nd")]),
        ),
        (
            b"my $t = 2; my $s = $t <<$t;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(29..42, "=pod\n\nd")]),
        ),
        (
            b"my $s = \"a b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(16..29, "=pod\n\nd")]),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(24..37, "=pod\n\nd")]),
        ),
        (
            b"my $k = 1;\nmy $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(35..48, "=pod\n\nd")]),
        ),
        (
            b"my @s = (<<EOT, <<EOU);\na\nEOT\nb\nEOU\n\n=pod\n\nd\n\n=cut\nprint \"MARK\"\
          ;\n",
            documents([doc(37..50, "=pod\n\nd")]),
        ),
        (
            b"my $s = <<EOT; # c\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(28..41, "=pod\n\nd")]),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\r\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(25..38, "=pod\n\nd")]),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            documents([doc(36..49, "=pod\n\nd")]),
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(30..43, "=pod\n\nd")]),
        ),
        (
            b"my $s = q{a\\\\};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(17..30, "=pod\n\nd")]),
        ),
        (
            b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(19..32, "=pod\n\nd")]),
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(18..31, "=pod\n\nd")]),
        ),
        (
            b"my %h; my $s = qq{ $h{qq{ $h{qq{ x }} }} };\n\n=pod\n\nd\n\n=cut\nprint \"MA\
          RK\";\n",
            documents([doc(45..58, "=pod\n\nd")]),
        ),
        (
            b"my $t = 'a';\n$t =~ s{a}\n  {b}g; $t =~ tr{a} {b};\n\n=pod\n\nd\n\n=cut\npri\
          nt \"MARK\";\n",
            documents([doc(50..63, "=pod\n\nd")]),
        ),
        (
            b"my $s = q#a#;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(15..28, "=pod\n\nd")]),
        ),
        (
            b"my $s = q'ab'; my $m = 'a' =~ m'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(37..50, "=pod\n\nd")]),
        ),
        (
            b"my $t = 'a';\n$t =~ s/a/b/gi; $t =~ tr/a/b/r; my $m = $t =~ m{a}x;\n\n=pod\n\
          \nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(67..80, "=pod\n\nd")]),
        ),
        (
            b"my $s = qq{a\\}b}; my $r = qr(a\\)b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            documents([doc(37..50, "=pod\n\nd")]),
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(76..89, "=pod\n\nd")]),
        ),
        (
            b"my $z = 4 / 2; my $w = $z / 2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(32..45, "=pod\n\nd")]),
        ),
        (
            b"my $v = 1; sub go { 7 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            documents([doc(25..38, "=pod\n\nd")]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_literal_text_the_grammar_reads_holds_no_pod_block() {
    // Today's outcomes: in each source the grammar gives no `pod_statement` for the lines
    // that start with `=`. Nothing is published. One source ends in a data section,
    // which is reported. In three sources the tree holds a recovery node before those
    // lines: the file is marked.
    let cases: [(&[u8], View); 29] = [
        (
            b"my $s = <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = <<'EOT';\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = <<~EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = <<\\EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"sub g { 7 }\nmy $s = g(<<EOT);\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my %h; my $s = $h{<<EOT};\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my @s = (<<EOT, <<EOU);\n=pod\n\nd\n\n=cut\nEOT\n=head1 X\n\n=cut\nEOU\nprin\
          t \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = <<EOA;\n@{[ <<EOT ]}\n=pod\n\nd\n\n=cut\nEOT\nEOA\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"print STDOUT <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my %h = (a => <<EOT,\n=pod\n\nd\n\n=cut\nEOT\n  b => 2);\nprint \"MARK\";\n",
            pending([hidden("ERROR", 0..64)]),
        ),
        (
            b"my $s = q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = q(\n=pod\n\nd\n\n=cut\n);\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = q X\n=pod\n\nd\n\n=cut\nX;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = q#\n=pod\n\nd\n\n=cut\n#;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = qq{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = 'a' =~ m{\n=pod\n\nd\n\n=cut\n}x;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = qr/\n=pod\n\nd\n\n=cut\n/x;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = 'a'; $s =~ s/\n=pod\n\nd\n\n=cut\n/b/x;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = 'a'; $s =~ s{a}{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = 'a'; $s =~ tr/a/\n=pod\n\nd\n\n=cut\n/;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = 'a'; $s =~ y/a/\n=pod\n\nd\n\n=cut\n/;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = '\n=pod\n\nd\n\n=cut\n';\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = `\n=pod\n\nd\n\n=cut\n`;\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = qx{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"my $s = <<`EOT`;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            View::default(),
        ),
        (
            b"1; __END__\n=pod\n\nd\n\n=cut\n",
            pending([data_section(10..25)]),
        ),
        (
            b"format STDOUT =\nline\n.\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([hidden("ERROR", 23..27)]),
        ),
        (
            b"format Pkg'F =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            pending([hidden("ERROR", 0..39)]),
        ),
        (b"my $y\n=pod\n;\nprint \"MARK\";\n", View::default()),
    ];
    for (source, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                retained(source, retain),
                expected,
                "{}",
                source.escape_ascii()
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_end_marker_as_a_hash_key_is_pinned() {
    // This test pins today's outcome, not a decision: `__END__` and `__DATA__` before
    // `=>` are hash keys for perl, which compiles the lines behind them and reads the
    // block as POD. The grammar gives a data leaf there, and the unit reports a data
    // section that perl does not have.
    assert_views([
        (
            b"my %h = (__END__ => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([data_section(16..53)]),
        ),
        (
            b"my %h = (__DATA__ => 1);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([data_section(17..54)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_hidden_behind_recovery_is_reported() {
    // Behind a recovery node the grammar may give no `pod_statement` for a POD block.
    // The file then gets one assessment on the first recovery node of the tree, if a
    // line that starts with `=` and an ASCII letter stands behind that node, before the
    // data section and outside every block. First source: perl reads the block as POD;
    // the grammar does not parse the literal before it. Second source: the node is a
    // missing node, the line stands in a string, and the search ends at the data leaf.
    assert_views([
        (
            b"my $s = q'a\\'b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([hidden("ERROR", 0..46)]),
        ),
        (
            b"my $x = (1;\nmy $s = \"\n=pod\n\";\n__END__\n=pod\n\nd\n\n=cut\n",
            View {
                assessments: unordered([data_section(37..52), hidden(")", 10..10)]),
                ..View::default()
            },
        ),
    ]);
    // Not marked: the only such line stands in the data section; it stands before the
    // first recovery node; no line behind the node starts with `=` and a letter; there
    // is no such line at all.
    assert_views([
        (
            b"my $x = (1;\n__END__\n=pod\n\nd\n\n=cut\n",
            pending([data_section(19..34)]),
        ),
        (b"my $s = '\n=pod\n';\nmy $x = (1;\n", View::default()),
        (
            b"my $x = (1;\nmy $s = \"\n=1\n==\n= a\n =pod\nx=pod\n\";\n",
            View::default(),
        ),
        (b"my $x = (1;\nmy $y = 2;\n", View::default()),
    ]);
    // An excluded byte behind the node is not read, and the worst case holds: the file
    // is marked. The cut nodes are today's outcome of the core.
    let cases: [(&[u8], Included, View); 1] = [(
        b"my $x = (1;\nmy $y = 2;\n",
        &[0..14, 15..23],
        View {
            assessments: unordered([
                cut("binary_expression", 12..21),
                cut("variable_declaration", 12..17),
                hidden(")", 10..10),
            ]),
            ..View::default()
        },
    )];
    for (source, ranges, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                restricted_view(source, ranges, retain),
                expected,
                "{ranges:?}"
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_known_over_withholding_is_pinned() {
    // Today's outcomes, not decisions: perl reads each of these blocks as POD, and the
    // unit withholds it. A real block after a here-document with an empty tag; after a
    // name with the old package separator, which the grammar does not parse; after a
    // string that holds `<<` and a word; after a here-document inside a literal has
    // ended; after a here-document whose body the grammar starts late; and after a shift
    // that is written like an opener with a digit tag. After a format with a value line
    // that holds a closed string or brackets over two lines; after the word `format` as
    // a method name or a glob name before `=` and a line end; after a shift by a quoted
    // string or by a bareword behind a variable; after a `q` literal that the grammar
    // ends late, with the string behind it closed on its row; after an operator word
    // before `=>` or in a subscript, which the grammar reads as a literal; and after a
    // pattern behind a word that the unit does not know as a keyword.
    assert_views([
        (
            b"my $s = <<\"\";\nlit\n\nprint \"MARK\";\n\n=pod\n\nreal\n\n=cut\n",
            pending([withheld(RECOVERY, 34..50)]),
        ),
        (
            b"sub isn't { 1 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(RECOVERY, 17..30)]),
        ),
        (
            b"my $s = \"a<<b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 17..30)]),
        ),
        (
            b"my $s = qq{@{[ <<\"1;\" ]}};\nlit;\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            pending([withheld(UNREAD_OPENER, 36..49)]),
        ),
        (
            b"my $s = <<EOT; \nlit;\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(HEREDOC, 26..39)]),
        ),
        (
            b"my $t = 2; my $s = $t <<2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 28..41)]),
        ),
        (
            b"format F =\n@x;\n\"a\"\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 25..38)]),
        ),
        (
            b"format F =\n@x;\n[ $a,\n$b ]\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 32..45)]),
        ),
        (
            b"my $o = bless {};\n$o->format =\n1;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 35..48)]),
        ),
        (
            b"sub g { 7 }\n*format =\n\\&g;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(FORMAT, 28..41)]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh << \"EOT\";\n\n=pod\n\nd\n\n=cut\nprint \"MAR\
          K\";\n",
            pending([withheld(UNREAD_OPENER, 40..53)]),
        ),
        (
            b"my $t = 2; my $x = $t <<EOT;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(UNREAD_OPENER, 30..43)]),
        ),
        (
            b"my $x = q(a\\(b); my $s = \"x)\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 32..45)]),
        ),
        (
            b"my %k = (y => 1, s => 2, q => 3);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 35..48)]),
        ),
        (
            b"my $m = lc /A/;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 17..30)]),
        ),
        (
            b"my %k; my $v = $k{q}; my $w = { 1 };\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            pending([withheld(LITERAL, 38..51)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_source_filter_is_a_named_limitation() {
    // Today's outcome, not a decision: code that runs while perl compiles can change
    // what perl reads after it. The unit reads the bytes of the file and publishes the
    // block.
    assert_views([(
        b"use Filter::Util::Call;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
        documents([doc(25..38, "=pod\n\nd")]),
    )]);
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_doubt_beside_excluded_bytes_takes_the_worst_case() {
    // An excluded byte is not read. Where a rule would have to read one, the worst case
    // holds: a format whose opener or body cannot be read runs to the end of the input;
    // an identifier with an excluded byte may be the keyword; a shift with an excluded
    // byte beside it may be an opener; a literal with an excluded byte may hold an
    // opener or end elsewhere; a tag line with an excluded byte behind the tag may not end
    // the here-document. The second row is the control: two ranges that touch exclude
    // nothing. In the last four rows the excluded byte stands before the node: in the
    // bytes before a pattern and before a single-quoted string that say whether perl
    // starts a literal there; behind the blanks of a shift; and in the six bytes before
    // a name of two bytes, which may be `CORE::`. The cut nodes are today's outcome of
    // the core.
    let cases: [(&[u8], Included, View); 12] = [
        (
            b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..25, 26..63],
            View {
                assessments: unordered([
                    withheld(FORMAT, 35..48),
                    cut("binary_expression", 23..32),
                    cut("variable_declaration", 23..28),
                ]),
                ..View::default()
            },
        ),
        (
            b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..25, 25..63],
            documents([doc(35..48, "=pod\n\nd")]),
        ),
        (
            b"forXmat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            &[0..3, 4..41],
            View {
                assessments: unordered([
                    withheld(FORMAT, 20..33),
                    cut("call_expression_with_spaced_args", 0..18),
                    cut("call_expression_with_bareword", 0..7),
                    cut("identifier", 0..7),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $t = 2; my $s = $t <<$t;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..21, 22..57],
            View {
                assessments: unordered([
                    withheld(UNREAD_OPENER, 29..42),
                    cut("binary_expression", 11..26),
                    cut("binary_expression", 19..26),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $s = \"a b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..44],
            View {
                assessments: unordered([
                    withheld(LITERAL, 16..29),
                    cut("binary_expression", 0..13),
                    cut("string_double_quoted", 8..13),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..23, 24..64],
            pending([withheld(HEREDOC, 36..49)]),
        ),
        (
            b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..47],
            View {
                assessments: unordered([
                    withheld(LITERAL, 19..32),
                    cut("binary_expression", 0..16),
                    cut("string_q_quoted", 8..16),
                ]),
                ..View::default()
            },
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..12, 13..46],
            View {
                assessments: unordered([
                    withheld(LITERAL, 18..31),
                    cut("binary_expression", 0..15),
                    cut("word_list_qw", 8..15),
                    cut("list_item", 11..14),
                ]),
                ..View::default()
            },
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..13, 14..104],
            View {
                assessments: unordered([
                    withheld(LITERAL, 76..89),
                    cut("binary_expression", 0..24),
                    cut("call_expression_with_spaced_args", 8..24),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $s = 'a <<EOT b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..7, 8..50],
            View {
                assessments: unordered([
                    withheld(LITERAL, 22..35),
                    cut("binary_expression", 0..19),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..51, 52..58],
            View {
                assessments: unordered([
                    withheld(UNREAD_OPENER, 30..43),
                    cut("arguments", 50..56),
                    cut("call_expression_with_spaced_args", 44..56),
                    cut("string_double_quoted", 50..56),
                ]),
                ..View::default()
            },
        ),
        (
            b"my $v = 1; sub go { 7 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..53],
            pending([withheld(LITERAL, 25..38)]),
        ),
    ];
    for (source, ranges, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                restricted_view(source, ranges, retain),
                expected,
                "{ranges:?}"
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_text_outside_a_pod_node_publishes_nothing() {
    // Today's outcomes, not decisions on these forms: the grammar gives no `pod_statement`
    // here, so nothing is published, whatever is retained. In the last four sources the
    // tree holds a recovery node, and a line behind it starts like a POD command: the
    // file is marked, and nothing else is pending.
    let cases: [(&[u8], View); 11] = [
        (b"# leads f\nsub f {}\n", View::default()),
        (
            b"my $t = <<'EOT';\n=head1 NAME\n\nnot pod\n\n=cut\nEOT\n",
            View::default(),
        ),
        (
            b"my $s = \"\n=pod\n\nx\n\n=cut\n\";\nmy $after = 2;\n",
            View::default(),
        ),
        (
            b"my @a = qw(\n=pod\n\nx\n\n=cut\n);\nmy $after = 2;\n",
            View::default(),
        ),
        (
            b"my $r = qr/\n=pod\n\nx\n\n=cut\n/x;\nmy $after = 2;\n",
            View::default(),
        ),
        (b"sub f { return 1; }\n", View::default()),
        (b"", View::default()),
        (
            b"my $s = \"abc\n=pod\n\nx\n\n=cut\n",
            pending([hidden("ERROR", 0..27)]),
        ),
        (
            b"my @a = (\n1,\n=pod\n\nx\n\n=cut\n2);\n",
            pending([hidden("ERROR", 13..26)]),
        ),
        (
            b"my $x = 1\n=pod\n\nx\n\n=cut\n;\n",
            pending([hidden("ERROR", 16..23)]),
        ),
        (b"=pod", pending([hidden("ERROR", 0..4)])),
    ];
    for (source, expected) in cases {
        for retain in [Retain::Nothing, Retain::Unbound, Retain::Directives] {
            assert_eq!(
                retained(source, retain),
                expected,
                "{}",
                source.escape_ascii()
            );
        }
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_data_section_is_reported_and_not_read() {
    assert_views([
        (
            b"__END__\n=head1 NAME\n\nAudit\n\n=cut\n",
            pending([data_section(7..33)]),
        ),
        (b"__DATA__\nrow\n", pending([data_section(8..13)])),
        (b"__END__", pending([data_section(7..7)])),
        (
            b"sub f {\n__END__\n=pod\n\nx\n\n=cut\n",
            pending([data_section(15..30)]),
        ),
    ]);
}

#[cfg(feature = "perl")]
#[test]
fn perl_pod_with_invalid_utf8_has_no_text() {
    assert_views([(
        b"=head1 A\n\n\xff\n\n=cut\n",
        documents([Doc {
            fragment: vec![0..17],
            text: Text::Unavailable(UnavailableReason::InvalidEncoding),
        }]),
    )]);
}

#[cfg(feature = "perl")]
#[test]
fn pod_block_charges_what_it_reads() {
    // expected: one step for the coverage search in every row. Bytes: the command word
    // up to the byte that ends it; then every byte of the spelling; then, per line of the
    // block until the answer is known, the line with its line feed, the bytes up to the
    // first that is no white space, and the white space after a `=for`, `=begin` or
    // `=end` command with the byte that ends it.
    let cases: [(&[u8], usize, PodBlock, Charged); 11] = [
        // 6 (`head2 `) + 21 + 9 (`=head2 f` and LF) + 1 (`=`).
        (PRIMARY, 0, PodBlock::Documents(21), (1, 37)),
        // 4 (`pod` and LF) + 9 + 5 + 1.
        (b"=pod\n=cut\n", 0, PodBlock::Documents(9), (1, 19)),
        // 6 + 21 (the CR of the last line end is outside the spelling) + 10 + 1.
        (
            b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n",
            0,
            PodBlock::Documents(21),
            (1, 38),
        ),
        // 6 + 28 + 12 + 1: the final line feed is outside the spelling.
        (
            b"=head1 NAME\n\nNever closed.\n\n\n",
            0,
            PodBlock::Documents(28),
            (1, 47),
        ),
        // 6 + 45 + 53: every line is read to the end of the block. Lines: 15 + 1 + 2;
        // 1; 10 + 1; 1; 13 + 1 + 2; 1; 4 + 1. An empty line costs its line feed only.
        (
            b"=begin comment\n\nnot prose\n\n=end comment\n\n=cut\n",
            0,
            PodBlock::CommentOnly(45),
            (1, 104),
        ),
        // 6 + 62 + 10: the first line already documents.
        (MIXED_FIXTURE, 0, PodBlock::Documents(62), (1, 78)),
        // Withheld blocks stop early. A stray `=cut`: the word and its line feed.
        (
            b"my $x = 1;\n=cut\nmy $y = 2;\n",
            11,
            PodBlock::Withheld(OPENER),
            (1, 4),
        ),
        // Not at the start of a line: no byte of the block is read.
        (
            b"my $x = 1;\n  =head1 NAME\n\ntext\n\n=cut\n",
            13,
            PodBlock::Withheld(OPENER),
            (1, 0),
        ),
        // Cut at a NUL byte: no byte of the block is read.
        (
            b"=pod\n\na\x00b\n\n=cut\n",
            0,
            PodBlock::Withheld(CLOSER),
            (1, 0),
        ),
        // `=cutting`: 6 + 24, and no line is classified.
        (
            b"=head1 A\n\ntext\n\n=cutting\nmy $y = 2;\n=cut\n",
            0,
            PodBlock::Withheld(CLOSER),
            (1, 30),
        ),
        // A lone CR: 4 + 15, and no line is classified.
        (
            b"=pod\n\na\rb\n\n=cut\n",
            0,
            PodBlock::Withheld(LINE_END),
            (1, 19),
        ),
    ];
    for (source, start, answer, charged) in cases {
        assert_eq!(
            block_charges(source, start, true),
            (answer, charged),
            "{}",
            source.escape_ascii()
        );
    }
    // An excluded byte beside the block: the coverage search and nothing else.
    let tree = restricted(TWO_BLOCKS, &[0..10, 11..58]);
    let context = SourceContext::new(&tree, TWO_BLOCKS);
    let budget = Counting::default();
    let block = node_at(tree.root_node(), "pod_statement", 11..28);
    assert_eq!(
        pod_block(&context, block, &budget).unwrap(),
        PodBlock::Withheld(EXCLUDED)
    );
    // expected: two continuous runs, so the search probes twice.
    assert_eq!(charged_totals(&budget), (2, 0));
}

// test-lang-gates: hand-written(perl) — the scans read Perl bytes and name no grammar;
//     the counting budget that measures them is built with the Perl tests
#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn pod_doubt_scans_charge_what_they_read() {
    // expected: a scan charges no step and one byte per byte it reads, up to and
    // including the byte that ends it. Every charge is passed on, so each can cancel.
    // The bytes behind the keyword `format`, and where the body starts. Read: white
    // space and comments with the byte that ends them; the name with the byte that ends
    // it; white space and comments again; behind the `=`, blanks with the byte that
    // ends them; a comment or the line feed. A line feed that ends a comment is read
    // twice. First row: ` S` (2), `STDOUT ` (7), ` =` (2), the line feed twice (2).
    // perl takes blanks, tabs, a carriage return and a comment behind the `=`, and
    // nothing else: no text, no form feed, no vertical tab, no `;`, no end of input.
    let openers: [(&[u8], Option<usize>, usize); 25] = [
        (b" STDOUT =\n1\n", Some(10), 13),
        (b" =\n", Some(3), 6),
        (b"=\n", Some(2), 5),
        (b" Pkg::F =\n", Some(10), 13),
        (b" A'B =\n", Some(7), 10),
        (b" _f =\n", Some(6), 9),
        (b" F1 =\n", Some(6), 9),
        (b"\x0bSTDOUT\x0b=\n", Some(10), 13),
        (b"\nSTDOUT\n=\n", Some(10), 13),
        (b" # a\nSTDOUT # b\n=\n", Some(18), 25),
        (b" STDOUT = # c\n", Some(14), 17),
        (b" STDOUT =  \t\r\n", Some(14), 17),
        (b"\tSTDOUT\t=\n", Some(10), 13),
        (b" STDOUT = # c", Some(13), 16),
        (b" STDOUT = 1;\n", None, 13),
        (b" STDOUT =\x0c\n", None, 12),
        (b" STDOUT =\x0b\n", None, 12),
        (b" STDOUT =;\n", None, 12),
        (b" STDOUT ==\n", None, 12),
        (b" STDOUT =", None, 11),
        (b" => 1);\n", None, 5),
        (b";\n", None, 3),
        (b" { 7 }\n", None, 4),
        (b" STDOUT\n", None, 10),
        (b"", None, 0),
    ];
    for (after, body, bytes) in openers {
        assert_eq!(
            scan_charges(|budget| format_body(after, budget)),
            (body, bytes),
            "{}",
            after.escape_ascii()
        );
    }
    // A body, and the line that ends the format for perl 5.34.0, where the unit knows
    // it. Where picture text may stand: a dot at the start of a line, then only blanks,
    // tabs and carriage returns, with or without a line feed. Text, `;`, `#`, a form
    // feed or a vertical tab behind the dot, a second dot, and white space before the
    // dot do not end it. Behind a picture line that holds `@` or `^` perl reads values:
    // there a dot ends the format only directly before the line end, and any other line
    // is read only if it is a list of named variables. A comment, a string, a call, an
    // expression, a bracket, a variable with a digit as name: the end is not known.
    // Read: each byte once, up to and including the line that ends the format or the
    // value line that is not read. First row: `1` and its line feed (2), the dot and its
    // line feed (2).
    let bodies: [(&[u8], LastLine, usize); 55] = [
        (b"1\n.\n2;\n", Some(2..4), 4),
        (b".\n", Some(0..2), 2),
        (b"1\n.", Some(2..3), 3),
        (b"1\n. \t\n2;\n", Some(2..6), 6),
        (b"1\n.\r\n", Some(2..5), 5),
        (b"1\n. \r\r\n", Some(2..7), 7),
        (b"1\r\n.\r\n2;\r\n", Some(3..6), 6),
        (b"1\n. ", Some(2..4), 4),
        (b"1\n. x\n.\n", Some(6..8), 8),
        (b"1\n .\n", None, 5),
        (b"1\n\t.\n", None, 5),
        (b"1\n..\n", None, 5),
        (b"1\n.;\n", None, 5),
        (b"1\n.#c\n", None, 6),
        (b"1\n.\x0c\n", None, 5),
        (b"1\n.\x0b\n", None, 5),
        (b"1\n2\n", None, 4),
        (b"", None, 0),
        (b"@<<<\n$a\n.\n", Some(8..10), 10),
        (
            b"@<<< ^>>>\n$a, @l, %h, $P::q, $::r ;\t\r\n.\n",
            Some(38..40),
            40,
        ),
        (b"@<<<\n.\n2;\n", Some(5..7), 7),
        (b"@<<<\n.\r\n", Some(5..8), 8),
        (b"@<<<\n.", None, 6),
        (b"@<<<\n. \n.\n", None, 8),
        (b"@<<<\n.\r\r\n.\n", None, 9),
        (b"@<<<\n\n.\n", Some(6..8), 8),
        (b"@<<<\n$a,\n$b\n.\n", Some(12..14), 14),
        (b"~~ ^<<<\n$a\n.\n", Some(11..13), 13),
        (b"# @<<<\n.\n", Some(7..9), 9),
        (b"@<<<\n# c\n.\n", None, 9),
        (b"@<<<\n$a # c\n.\n", None, 12),
        (b"@<<<\nq(a)\n.\n", None, 10),
        (b"@<<<\n\"a\"\n.\n", None, 9),
        (b"@<<<\nf($a)\n.\n", None, 11),
        (b"@<<<\n{ $a }\n.\n", None, 12),
        (b"@<<<\n$a . $b\n.\n", None, 13),
        (b"@<<<\n<<E\n.\n", None, 9),
        (b"@<<<\n$\n.\n", None, 7),
        (b"@<<<\n$a, $\n.\n", None, 11),
        (b"@<<<\n$1\n.\n", None, 8),
        (b"@<<<\ns\n.\n", None, 7),
        (b"@<<<\n$a x 2\n.\n", None, 12),
        (b"@<<<\n__END__\n.\n", None, 13),
        (b"@<<<\n$a\n@>>>\n$b\n.\n", Some(16..18), 18),
        (b"@<<<\n$a\n@>>>\nq!\n.\n", None, 16),
        (b"a@b\n$a", None, 6),
        (b"@<<<\n$a:b\n.\n", Some(10..12), 12),
        (b"@<<<\n$a%\n.\n", None, 9),
        (b"@<<<\n@$a\n.\n", None, 9),
        (b"@<<<\n$_a1\n.\n", Some(10..12), 12),
        (b"^<<<\nq(a)\n.\n", None, 10),
        (b"# @x\n. \n", Some(5..8), 8),
        (b"@<<<\n$a\r\n.\n", Some(9..11), 11),
        (b"@<<<\n$a;$b\n.\n", Some(11..13), 13),
        (b"x\n@<<<\n\t\n. \t\n", Some(9..13), 13),
    ];
    for (body, last, bytes) in bodies {
        assert_eq!(
            scan_charges(|budget| format_end(body, budget)),
            (last, bytes),
            "{}",
            body.escape_ascii()
        );
    }
    // The spelling of a literal node of a kind: whether perl ends the literal at the last
    // byte of the spelling, and whether its text holds `<<` and a tag. A backslash hides
    // the next byte; behind an opening bracket the literal ends at the matching closing
    // bracket, and brackets of that kind nest; any other delimiter ends it at its next
    // occurrence; `s`, `tr` and `y` have two parts; white space and comments may stand
    // before a delimiter, and a `#` directly behind the operator is the delimiter; ASCII
    // letters behind the last delimiter are flags. An operator word before `=>` or
    // before a closing bracket is a name. A tag is a quote behind optional blanks, or a
    // letter, a digit or `_`, behind an optional `~` and, for a word, an optional
    // backslash; the delimiters are no part of the text. Read: each byte once, up to the
    // byte that decides. First row: the grammar's node `q!a\!` ends at the escaped
    // delimiter; perl does not end the literal there, and all five bytes are read.
    let literals: [(&str, &[u8], Reading, usize); 129] = [
        ("string_q_quoted", b"q!a\\!", (false, false), 5),
        ("string_q_quoted", b"q!a\\!b!", (true, false), 7),
        ("string_q_quoted", b"q{a\\}", (false, false), 5),
        ("string_q_quoted", b"q{a\\\\}", (true, false), 6),
        ("string_q_quoted", b"q{a\\\\\\}", (false, false), 7),
        ("string_q_quoted", b"q{a{b}c}", (true, false), 8),
        ("string_q_quoted", b"q{}", (true, false), 3),
        ("string_q_quoted", b"q(a\\(b)", (true, false), 7),
        (
            "string_q_quoted",
            b"q(a\\(b); my $s = \"x)",
            (false, false),
            8,
        ),
        ("string_q_quoted", b"q(a(b)", (false, false), 6),
        ("string_q_quoted", b"q(a)b)", (false, false), 5),
        ("string_q_quoted", b"q XaX", (true, false), 5),
        ("string_q_quoted", b"q\n(a)", (true, false), 5),
        ("string_q_quoted", b"q # c\n # d\n(a)", (true, false), 14),
        ("string_q_quoted", b"q#a#", (true, false), 4),
        ("string_q_quoted", b"q #a#", (false, false), 5),
        ("string_q_quoted", b"q'a\\'b'", (true, false), 7),
        ("string_q_quoted", b"q\\a\\", (true, false), 4),
        ("string_q_quoted", b"q => 1, 2 =", (false, false), 4),
        ("string_q_quoted", b"q\n=>=a=", (false, false), 4),
        ("string_q_quoted", b"q # c\n=> 1, 2 =", (false, false), 8),
        ("string_q_quoted", b"q=a=", (true, false), 4),
        ("string_q_quoted", b"q===", (false, false), 4),
        ("string_q_quoted", b"q}a}", (false, false), 2),
        ("string_q_quoted", b"q)a)", (false, false), 2),
        ("string_q_quoted", b"q ]a]", (false, false), 3),
        ("string_q_quoted", b"q>a>", (false, false), 2),
        ("string_q_quoted", b"q", (false, false), 1),
        ("string_q_quoted", b"q ", (false, false), 2),
        ("string_q_quoted", b"x(a)", (false, false), 2),
        ("string_q_quoted", b"", (false, false), 0),
        ("string_q_quoted", b"q(a)x", (false, false), 5),
        ("string_q_quoted", b"q(<<E)", (true, true), 6),
        ("word_list_qw", b"qw!a\\!", (false, false), 6),
        ("word_list_qw", b"qw{a{b}", (false, false), 7),
        ("word_list_qw", b"qw{a{b}}", (true, false), 8),
        ("word_list_qw", b"qw(a b)", (true, false), 7),
        ("word_list_qw", b"qw()", (true, false), 4),
        ("word_list_qw", b"qq(a)", (false, false), 3),
        ("word_list_qw", b"q(a)", (false, false), 3),
        ("string_qq_quoted", b"qq{@{[ <<EOT ]}}", (true, true), 16),
        ("string_qq_quoted", b"qq{<<\"E\"}", (true, true), 9),
        ("string_qq_quoted", b"qq<<E>", (false, false), 6),
        ("string_qq_quoted", b"qq{a}", (true, false), 5),
        ("string_qq_quoted", b"qq{<<\"\"}", (true, true), 8),
        ("string_qq_quoted", b"qq\"<<\"", (true, false), 6),
        ("string_qq_quoted", b"qq{a{<<}E}", (true, false), 10),
        (
            "string_double_quoted",
            b"\"@{[ <<EOT ]}\"",
            (true, true),
            14,
        ),
        (
            "string_double_quoted",
            b"\"${\\ <<EOT }\"",
            (true, true),
            13,
        ),
        ("string_double_quoted", b"\"<<~EOT\"", (true, true), 8),
        ("string_double_quoted", b"\"<<\\EOT\"", (true, true), 8),
        ("string_double_quoted", b"\"<<~\\EOT\"", (true, true), 9),
        ("string_double_quoted", b"\"<<_e\"", (true, true), 6),
        ("string_double_quoted", b"\"<<1\"", (true, true), 5),
        ("string_double_quoted", b"\"<< \t'E'\"", (true, true), 9),
        ("string_double_quoted", b"\"<<~ `E`\"", (true, true), 9),
        ("string_double_quoted", b"\"a<<b\"", (true, true), 6),
        (
            "string_double_quoted",
            b"\"<<< x >>> 1 << 2\"",
            (true, false),
            18,
        ),
        ("string_double_quoted", b"\"<< EOT\"", (true, false), 8),
        ("string_double_quoted", b"\"<<\"", (true, false), 4),
        ("string_double_quoted", b"\"<< \"", (true, false), 5),
        ("string_double_quoted", b"\"<\"", (true, false), 3),
        ("string_double_quoted", b"\"a < b\"", (true, false), 7),
        ("string_double_quoted", b"\"<<-\"", (true, false), 5),
        ("string_double_quoted", b"\"<<$x\"", (true, false), 6),
        ("string_double_quoted", b"\"\"", (true, false), 2),
        ("string_double_quoted", b"\"a\\\"b\"", (true, false), 6),
        ("string_double_quoted", b"\"a\"b\"", (false, false), 4),
        ("string_double_quoted", b"\"a", (false, false), 2),
        ("string_double_quoted", b"\"<<\\\"E\"", (true, false), 7),
        ("string_double_quoted", b"\"\\<<E\"", (true, true), 6),
        ("string_single_quoted", b"'a <<EOT b'", (true, true), 11),
        ("string_single_quoted", b"'a\\'b'", (true, false), 6),
        ("string_single_quoted", b"'a'b'", (false, false), 4),
        ("string_single_quoted", b"''", (true, false), 2),
        ("backtick_quoted", b"`@{[ <<EOT ]}`", (true, true), 14),
        ("backtick_quoted", b"`a`", (true, false), 3),
        ("command_qx_quoted", b"qx{@{[ <<EOT ]}}", (true, true), 16),
        ("command_qx_quoted", b"qx{a}", (true, false), 5),
        ("pattern_matcher", b"/(?{ <<EOT })/", (true, true), 14),
        ("pattern_matcher", b"/a[\\/]b/", (true, false), 8),
        ("pattern_matcher", b"/a/gi", (true, false), 5),
        ("pattern_matcher", b"/a/g1", (false, false), 5),
        ("pattern_matcher", b"/a[/]b/", (false, false), 5),
        ("pattern_matcher", b"/ 2; my $s = \"/", (true, false), 15),
        ("pattern_matcher_m", b"m{@{[ <<EOT ]}}", (true, true), 15),
        ("pattern_matcher_m", b"m'a'", (true, false), 4),
        ("pattern_matcher_m", b"m{a{1}}x", (true, false), 8),
        ("pattern_matcher_m", b"m{a\\{}", (true, false), 6),
        ("pattern_matcher_m", b"m #a#", (false, false), 5),
        ("pattern_matcher_m", b"m/a/ x", (false, false), 5),
        ("regex_pattern_qr", b"qr{@{[ <<EOT ]}}", (true, true), 16),
        ("regex_pattern_qr", b"qr/a/i", (true, false), 6),
        ("regex_pattern_qr", b"qr(a\\()", (true, false), 7),
        ("substitution_pattern_s", b"s/a/<<EOT/e", (true, true), 11),
        ("substitution_pattern_s", b"s/a/b/", (true, false), 6),
        ("substitution_pattern_s", b"s/a/b\\/", (false, false), 7),
        ("substitution_pattern_s", b"s/a/b\\/c/", (true, false), 9),
        ("substitution_pattern_s", b"s{a}{b}", (true, false), 7),
        ("substitution_pattern_s", b"s{a} \n {b}", (true, false), 10),
        (
            "substitution_pattern_s",
            b"s{a} # c\n {b}g",
            (true, false),
            14,
        ),
        ("substitution_pattern_s", b"s{a}/b/", (true, false), 7),
        ("substitution_pattern_s", b"s{a}#b#", (true, false), 7),
        ("substitution_pattern_s", b"s{a} #b#", (false, false), 8),
        ("substitution_pattern_s", b"s{a\\{}{b}", (true, false), 9),
        ("substitution_pattern_s", b"s{a}{b\\{}", (true, false), 9),
        ("substitution_pattern_s", b"s{a}", (false, false), 4),
        ("substitution_pattern_s", b"s{a} ", (false, false), 5),
        ("substitution_pattern_s", b"s XaX\\XX", (true, false), 8),
        ("substitution_pattern_s", b"s\"a<<\"b\"", (true, false), 8),
        (
            "substitution_pattern_s",
            b"s => 1, 2 => 3",
            (false, false),
            4,
        ),
        ("substitution_pattern_s", b"s(a)(b)", (true, false), 7),
        ("substitution_pattern_s", b"s<a><b>", (true, false), 7),
        ("substitution_pattern_s", b"s[a][b]e", (true, false), 8),
        ("transliteration_tr_or_y", b"tr/a/b/", (true, false), 7),
        ("transliteration_tr_or_y", b"y/a/b/", (true, false), 6),
        ("transliteration_tr_or_y", b"tr{a\\{}{b}", (true, false), 10),
        ("transliteration_tr_or_y", b"y(a\\()(b)", (true, false), 9),
        ("transliteration_tr_or_y", b"tr{a}{b}", (true, false), 8),
        ("transliteration_tr_or_y", b"tr [a] [b]", (true, false), 10),
        ("transliteration_tr_or_y", b"y!a!b\\!", (false, false), 7),
        ("transliteration_tr_or_y", b"tr/a/b/cdsr", (true, false), 11),
        ("transliteration_tr_or_y", b"tr/a/<<E/", (true, true), 9),
        (
            "transliteration_tr_or_y",
            b"y => 1, s => 2, q =>",
            (false, false),
            4,
        ),
        (
            "transliteration_tr_or_y",
            b"y} = 1; $h{s} = 2; $h{q}",
            (false, false),
            2,
        ),
        ("transliteration_tr_or_y", b"ty/a/b/", (false, false), 3),
        ("transliteration_tr_or_y", b"t/a/b/", (false, false), 3),
        ("transliteration_tr_or_y", b"y", (false, false), 1),
        ("transliteration_tr_or_y", b"tr\x0b/a/b/", (true, false), 8),
    ];
    for (kind, spelling, reads, bytes) in literals {
        let quote = Quote::of(kind).unwrap();
        assert_eq!(
            scan_charges(|budget| perl_reads(spelling, quote, budget)),
            (reads, bytes),
            "{kind} {}",
            spelling.escape_ascii()
        );
    }
    // The bytes before a `/`, whether they start at the start of the input, and whether
    // perl surely expects a term behind them, so that the `/` starts a pattern: at the
    // start of the input; behind `(`, `,`, `=`, `~`, `!`, `&`, `|`, `?`, `:`, `;` or `{`
    // that is not the name of a variable; and behind a keyword that takes a term and is
    // not part of a longer name. White space may stand between. Anything else may be a
    // division: a call without arguments, a variable, a closing bracket, a number.
    let leads: [(&[u8], bool, bool); 70] = [
        (b"", true, true),
        (b"  \n", true, true),
        (b" ", false, false),
        (b"my $m = (", false, true),
        (b"f(1,", false, true),
        (b"$t =~ ", false, true),
        (b"$t !~\n\t", false, true),
        (b"my $m = ", false, true),
        (b"$a && ", false, true),
        (b"$a || ", false, true),
        (b"$a ? ", false, true),
        (b"$a ? 1 : ", false, true),
        (b"1; ", false, true),
        (b"if (1) { ", false, true),
        (b"$m = ! ", false, true),
        (b"{ ", false, false),
        (b"! ", true, true),
        (b"split ", false, false),
        (b"split", false, false),
        (b"  grep  ", false, true),
        (b"1 if ", true, true),
        (b"if", true, true),
        (b"if", false, false),
        (b" unless ", false, true),
        (b" while ", false, true),
        (b" until ", false, true),
        (b" and ", false, true),
        (b" or ", false, true),
        (b" not ", false, true),
        (b" xor ", false, true),
        (b" return ", false, true),
        (b" map ", false, true),
        (b"time ", false, false),
        (b"length $t ", false, false),
        (b"$? ", false, false),
        (b"$! ", false, false),
        (b"$, ", false, false),
        (b"$; ", false, false),
        (b"${ ", false, false),
        (b"@{ ", false, false),
        (b"%! ", false, false),
        (b"?", false, false),
        (b"?", true, true),
        (b"($t) ", false, false),
        (b"$l[0] ", false, false),
        (b"$h{a} ", false, false),
        (b"4 ", false, false),
        (b"'a' ", false, false),
        (b"$o->split ", false, false),
        (b"Foo::split ", false, false),
        (b"Foo'if ", false, false),
        (b"&grep ", false, false),
        (b"$if ", false, false),
        (b"@map ", false, false),
        (b"%or ", false, false),
        (b"*and ", false, false),
        (b"xsplit ", false, false),
        (b"split_ ", false, false),
        (b"when ", false, false),
        (b"print ", false, false),
        (b" . ", false, false),
        (b" + ", false, false),
        (b"$i++ ", false, false),
        (b"} ", false, false),
        (b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", false, false),
        (b"xxxxxxxxxxxxxxxxxxxxxxxxxxxsplit", false, false),
        (b"xxxxxxxxxxxxxxxxxxxxxxxxxx split", false, true),
        (b"                                ", false, false),
        (b"                               ", true, true),
        (b"# c\n", false, false),
    ];
    for (before, whole, term) in leads {
        assert_eq!(
            expects_term(before, whole),
            term,
            "{}",
            before.escape_ascii()
        );
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_hidden_pod_search_charges_what_it_reads() {
    // expected: per source and span, whether a line that starts with `=` and an ASCII
    // letter starts in the span, and the bytes read: each byte of the span once, up to
    // and including the letter. The byte before the span says whether its first byte
    // starts a line. A span that is not empty costs one coverage search, which is one
    // step with one included run. Every charge is passed on, so each can cancel.
    let spans: [(&[u8], Range<usize>, bool, usize); 15] = [
        (b"1;\n=pod\n", 0..8, true, 5),
        (b"1;\n=pod\n", 3..8, true, 2),
        (b"1;\n=pod\n", 4..8, false, 4),
        (b"1;\n=pod\n", 3..4, false, 1),
        (b"1;\n=pod\n", 3..5, true, 2),
        (b"=pod\n", 0..5, true, 2),
        (b"=1\n==\n= a\n=_\n", 0..13, false, 13),
        (b"a =b\n", 0..5, false, 5),
        (b"1;\n\n=\n=Z", 0..8, true, 8),
        (b"1;\n=pod\n", 8..8, false, 0),
        (b"", 0..0, false, 0),
        (b"x\n=a\n=b\n", 0..8, true, 4),
        (b"x\r=a\n", 0..5, false, 5),
        (b"a =b\n", 2..5, false, 3),
        (b"1;\n=pod\n", 2..5, true, 3),
    ];
    for (source, span, found, bytes) in spans {
        let parser = parsed(source);
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let budget = Counting::default();
        assert_eq!(
            command_line(&context, span.clone(), &budget).unwrap(),
            found,
            "{span:?} {}",
            source.escape_ascii()
        );
        assert_eq!(
            charged_totals(&budget),
            (usize::from(!span.is_empty()), bytes),
            "{span:?} {}",
            source.escape_ascii()
        );
        for fail_at in 1..=budget.charges.get() {
            assert!(matches!(
                command_line(&context, span.clone(), &failing_at(fail_at)),
                Err(SourceError::Cancelled)
            ));
        }
    }
    // expected: a span with an excluded byte, or with one directly before it, is not
    // read: the answer is the worst case, and no byte is charged. Two included runs:
    // the coverage search probes twice.
    let source: &[u8] = b"1;
2;
3;
";
    let tree = restricted(source, &[0..4, 5..9]);
    let context = SourceContext::new(&tree, source);
    for (span, found, charged) in [
        (5..9, true, (2, 0)),
        (6..9, false, (2, 3)),
        (0..5, true, (2, 0)),
    ] {
        let budget = Counting::default();
        assert_eq!(command_line(&context, span, &budget).unwrap(), found);
        assert_eq!(charged_totals(&budget), charged);
    }
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn pod_doubt_walk_charges_its_searches_and_scans() {
    // expected: per source, for each block the reason that withholds it, and the steps
    // and bytes of the whole walk. One step per coverage search in one included run: for
    // an identifier of six bytes or more; for the rest of the input behind one that
    // spells `format`; for an identifier of one or two bytes and the six bytes before
    // it; for a literal and the bytes before it that are read; for the byte before and
    // the byte behind a `<<` operator that is no opener for the grammar, and for the
    // rest of the input where a blank or a tab stands behind it; for the two bytes
    // behind an end tag. The bytes are those of the scans, and for a literal the bytes
    // before it: one before a single-quoted string, up to 32 before a pattern. After the
    // first place that makes every later block doubtful, nothing is read. Second row:
    // two searches; ` S`, `STDOUT `, ` =` and the line feed twice (13), then the eight
    // lines of the body up to the dot line (21). The identifier `STDOUT` lies in the
    // format and is not looked at.
    let cases: [(&[u8], Doubts, Charged); 36] = [
        (
            b"%%%% (\n=pod\n\nx\n\n=cut\n",
            vec![(7, Some(RECOVERY))],
            (0, 0),
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            vec![(19, Some(FORMAT))],
            (2, 34),
        ),
        (
            b"format STDOUT =\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            vec![(24, None)],
            (2, 17),
        ),
        (
            b"format A =\nformat B =\n1\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(30, None)],
            (3, 29),
        ),
        (
            b"my $o = bless {};\nsub format { 7 }\nmy $v = $o->format;\n\n=pod\n\nd\n\n=cu\
          t\nprint \"MARK\";\n",
            vec![(56, None)],
            (5, 13),
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n\n=pod\n\nreal\n\n=cut\n",
            vec![(19, Some(FORMAT)), (41, None)],
            (2, 34),
        ),
        (
            b"format STDOUT =\n1;\n=pod\n\nd\n\n=cut\n2;\n",
            vec![(19, Some(FORMAT))],
            (2, 33),
        ),
        (
            b"my $s = \"@{[ <<EOT ]}\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            vec![(24, Some(UNREAD_OPENER))],
            (1, 14),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh <<\"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \"M\
          ARK\";\n",
            vec![(38, Some(UNREAD_OPENER))],
            (2, 0),
        ),
        (
            b"my $s = 1<<2;\nmy $t = $s << 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(32, None)],
            (4, 7),
        ),
        (
            b"my $s = \"<<< x >>> 1 << 2\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(29, None)],
            (2, 24),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(24, None)],
            (2, 6),
        ),
        (
            b"my @s = (<<EOT, <<EOU);\na\nEOT\nb\nEOU\n\n=pod\n\nd\n\n=cut\nprint \"MARK\"\
          ;\n",
            vec![(37, None)],
            (3, 6),
        ),
        (
            b"my $s = <<EOT; \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            vec![(16, Some(HEREDOC))],
            (0, 0),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT \n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            vec![(24, Some(HEREDOC))],
            (1, 0),
        ),
        (
            b"my $s = q!a\\!;\n=pod\n\nd\n\n=cut\n!;\nprint \"MARK\";\n",
            vec![(15, Some(LITERAL))],
            (1, 5),
        ),
        (
            b"my @s = qw{a{b};\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            vec![(17, Some(LITERAL))],
            (1, 7),
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(18, None)],
            (2, 13),
        ),
        (
            b"print STDOUT <<EOT;\n=pod\n\nd\n\n=cut\nEOT\nprint \"MARK\";\n",
            vec![],
            (3, 6),
        ),
        (
            b"format F =\n@x;\nqq{@{[ <<TAG ]}};\n1\n.\n2;\n=pod\n\nd\n\n=cut\nTAG\n.\n2;\
          \n",
            vec![(40, Some(FORMAT))],
            (2, 30),
        ),
        (
            b"format F =\n@x;\n$a, $b\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(28, None)],
            (3, 27),
        ),
        (
            b"format F =\n@x\n.\n2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(20, None)],
            (3, 19),
        ),
        (
            b"format F =\n@x;\n$a,\n@y;\n$b;\ntext\n. \n2;\n\n=pod\n\nd\n\n=cut\nprint \"M\
          ARK\";\n",
            vec![(39, None)],
            (3, 38),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint {$fh} <<  \"EOT\";\n=pod\n\nd\n\n=cut\nEOT\nprint \
          \"MARK\";\n",
            vec![(42, Some(UNREAD_OPENER))],
            (3, 2),
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(30, None)],
            (3, 8),
        ),
        (
            b"my $x = q(a\\(b); my $s = \"x);\n=pod\n\nd\n\n=cut\n\";\nprint \"MARK\";\n",
            vec![(30, Some(LITERAL))],
            (1, 8),
        ),
        (
            b"my $t = 'a';\n$t =~ tr{a\\{}{b}; my $s = \"x}{y};\n=pod\n\nd\n\n=cut\n\";\np\
          rint \"MARK\";\n",
            vec![(47, Some(LITERAL))],
            (2, 15),
        ),
        (
            b"my $t = 'a';\n$t =~ s{a}\n  {b}g; $t =~ tr{a} {b};\n\n=pod\n\nd\n\n=cut\npri\
          nt \"MARK\";\n",
            vec![(50, None)],
            (4, 30),
        ),
        (
            b"my $t = 'a';\n$t =~ s/a/b/gi; $t =~ tr/a/b/r; my $m = $t =~ m{a}x;\n\n=pod\n\
          \nd\n\n=cut\nprint \"MARK\";\n",
            vec![(67, None)],
            (5, 31),
        ),
        (
            b"my $x = CORE::q{\n=pod\n\nd\n\n=cut\n};\nprint \"MARK\";\n",
            vec![(17, Some(LITERAL))],
            (1, 0),
        ),
        (
            b"my $z = time / 2; my $s = \"/; 1; \"; my $u = \";\n=pod\n\nd\n\n=cut\n\";\n# \
          \"\nprint \"MARK\";\n",
            vec![(47, Some(LITERAL))],
            (1, 28),
        ),
        (
            b"my $x = Foo'bar(1); my $s = '; 1; '; my $u = ';\n=pod\n\nd\n\n=cut\n';\n# '\
          \nprint \"MARK\";\n",
            vec![(48, Some(LITERAL))],
            (1, 19),
        ),
        (
            b"my %k = (y => 1, s => 2, q => 3);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(35, Some(LITERAL))],
            (1, 4),
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(76, None)],
            (5, 99),
        ),
        (
            b"my $m = lc /A/;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(17, Some(LITERAL))],
            (2, 14),
        ),
        (
            b"my $v = 1; sub go { 7 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            vec![(25, None)],
            (2, 6),
        ),
    ];
    for (source, blocks, charged) in cases {
        let parser = parsed(source);
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let root = parser.ts_tree().root_node();
        let budget = Counting::default();
        assert_eq!(
            doubt_walk(&context, root, &budget).unwrap(),
            blocks,
            "{}",
            source.escape_ascii()
        );
        assert_eq!(
            charged_totals(&budget),
            charged,
            "{}",
            source.escape_ascii()
        );
        for fail_at in 1..=budget.charges.get() {
            assert!(matches!(
                doubt_walk(&context, root, &failing_at(fail_at)),
                Err(SourceError::Cancelled)
            ));
        }
    }
    // expected: with two included runs a coverage search probes twice, and a span that
    // holds an excluded byte is not read: no byte is charged for it.
    let cut: [(&[u8], Included, Doubts, Charged); 12] = [
        (
            b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..25, 26..63],
            vec![(35, Some(FORMAT))],
            (4, 0),
        ),
        (
            b"format STDOUT =\n1\n.\n2;\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..25, 25..63],
            vec![(35, None)],
            (3, 23),
        ),
        (
            b"forXmat STDOUT =\n1;\n=pod\n\nd\n\n=cut\n1\n.\n2;\n",
            &[0..3, 4..41],
            vec![(20, Some(FORMAT))],
            (4, 34),
        ),
        (
            b"my $t = 2; my $s = $t <<$t;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..21, 22..57],
            vec![(29, Some(UNREAD_OPENER))],
            (2, 0),
        ),
        (
            b"my $s = \"a b\";\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..44],
            vec![(16, Some(LITERAL))],
            (2, 0),
        ),
        (
            b"my $s = <<EOT;\nlit\nEOT\n\nmy $k = 3;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\
          \n",
            &[0..23, 24..64],
            vec![(36, Some(HEREDOC))],
            (2, 0),
        ),
        (
            b"my $s = q{a{b}c};\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..47],
            vec![(19, Some(LITERAL))],
            (2, 0),
        ),
        (
            b"my @s = qw(a b);\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..12, 13..46],
            vec![(18, Some(LITERAL))],
            (2, 0),
        ),
        (
            b"my @m = split /,/, 'a,b'; my $n = 1 if $m[0] =~ /a/; my @o = grep /b/, @m;\n\
          \n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..13, 14..104],
            vec![(76, Some(LITERAL))],
            (2, 0),
        ),
        (
            b"my $s = 'a <<EOT b';\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..7, 8..50],
            vec![(22, Some(LITERAL))],
            (2, 0),
        ),
        (
            b"my $t = 2; my $x = $t <<  2;\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..51, 52..58],
            vec![(30, Some(UNREAD_OPENER))],
            (4, 0),
        ),
        (
            b"my $v = 1; sub go { 7 }\n\n=pod\n\nd\n\n=cut\nprint \"MARK\";\n",
            &[0..10, 11..53],
            vec![(25, Some(LITERAL))],
            (2, 0),
        ),
    ];
    for (source, ranges, blocks, charged) in cut {
        let tree = restricted(source, ranges);
        let context = SourceContext::new(&tree, source);
        let budget = Counting::default();
        assert_eq!(
            doubt_walk(&context, tree.root_node(), &budget).unwrap(),
            blocks,
            "{ranges:?}"
        );
        assert_eq!(charged_totals(&budget), charged, "{ranges:?}");
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_operator_behind_core_is_read_as_a_literal() {
    // `CORE::q` and the other quote-like operators behind `CORE::` are operators for
    // perl. The grammar reads a name and a call, and no literal node. The unit takes an
    // identifier that spells such an operator directly behind `CORE::` for a literal
    // whose end it does not know: every later block is withheld. The tenth source starts
    // with `CORE::`. Controls: another name behind `CORE::`; an operator word behind
    // another package; a name of three bytes; and `q` as the first bytes of the input.
    let cases: [(&[u8], Doubts); 14] = [
        (
            b"my $x = CORE::q(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(21, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::qq(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::qw(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::qx(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::m(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(21, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::qr(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::s(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(21, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::tr(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::y(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(21, Some(LITERAL))],
        ),
        (
            b"CORE::q(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(13, Some(LITERAL))],
        ),
        (
            b"my $x = CORE::lc(1);\n\n=pod\n\nd\n\n=cut\n",
            vec![(22, None)],
        ),
        (
            b"my $x = Foo::q(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(20, None)],
        ),
        (
            b"my $x = CORE::qqq(a);\n\n=pod\n\nd\n\n=cut\n",
            vec![(23, None)],
        ),
        (b"q(a);\n\n=pod\n\nd\n\n=cut\n", vec![(7, None)]),
    ];
    for (source, blocks) in cases {
        let parser = parsed(source);
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let walked = doubt_walk(&context, parser.ts_tree().root_node(), &Counting::default());
        assert_eq!(walked.unwrap(), blocks, "{}", source.escape_ascii());
    }
}

// test-lang-gates: hand-written(perl) — the classifiers read POD bytes and name no
//     grammar; the counting budget that measures them is built with the Perl tests
#[cfg(feature = "perl")]
#[test]
fn pod_classifiers_charge_what_they_read_and_answer_as_before() {
    assert!(pod_comment_region(b"=for comment\nold();"));
    assert!(!pod_comment_region(b"=forcomment\nold();\n\n=cut"));
    let counted = |text: &[u8]| {
        let budget = Counting::default();
        let answer = pod_comment_region_with_budget(text, &budget).unwrap();
        assert_eq!(answer, pod_comment_region(text), "{}", text.escape_ascii());
        for fail_at in 1..=budget.charges.get() {
            assert!(matches!(
                pod_comment_region_with_budget(text, &failing_at(fail_at)),
                Err(SourceError::Cancelled)
            ));
        }
        (answer, charged_totals(&budget))
    };
    // expected: no step. Bytes per line: the line with its line feed, the bytes up to
    // the first that is no white space, and after a comment command and white space the
    // white space with the byte that ends it.
    // `=for comment`: 13 + 1 + 2. `old();`: 7 + 1. Empty line: 1. `=cut`: 4 + 1.
    assert_eq!(counted(b"=for comment\nold();\n\n=cut"), (true, (0, 30)));
    // `=begin  comment`: 16 + 1 + 3. `=end comment`: 13 + 1 + 2. `=cut`: 4 + 1.
    assert_eq!(
        counted(b"=begin  comment\n=end comment\n=cut"),
        (true, (0, 41))
    );
    // `=pod` documents: 5 + 1, and nothing after it is read.
    assert_eq!(counted(b"=pod\n\nText.\n\n=cut"), (false, (0, 6)));
    // `=for comment`: 13 + 1 + 2. `note`: 5 + 1. Empty line: 1. `Plain text paragraph.`
    // documents: 22 + 1.
    assert_eq!(
        counted(b"=for comment\nnote\n\nPlain text paragraph.\n\n=cut"),
        (false, (0, 46))
    );
    // A line of white space is read once to its end by the blank test: 4 + 3.
    assert_eq!(counted(b" \t \n=cut"), (false, (0, 12)));
    // A line of one space ends the `=for comment` paragraph, so `prose` documents:
    // 15 + 1 + 2, then 2 + 1, then 6 + 1.
    assert_eq!(counted(b"=for comment x\n \nprose\n=cut"), (false, (0, 28)));
    // The cases of the legacy unit test answer alike through both forms.
    for text in [
        &b"=begin comment\n\nold();\n\n=end comment\n\n=cut"[..],
        b"=for comment\nold();\n",
        b"=begin comment\nold();\n",
        b"=for comment a\n\n=for comment b\n\n=cut",
        b"=begin comment\nx\n=end comment\n=for comment y\n\n=cut",
        b"=for comment\r\nold();\r\n\r\n=cut\r\n",
        b"=head1 NAME\n\nFoo\n\n=cut",
        b"=begin html\n\n<p>x</p>\n\n=end html\n\n=cut",
        b"=begin comments\n=end comments\n=cut",
        b"=beginning",
        b"=for comment\nInternal note.\n\n=head1 NAME\n\nFoo - bar\n\n=cut",
        b"=begin comment\n\nold();\n\n=end comment\n\n=head1 X\n\nDoc.\n\n=cut",
        b"",
    ] {
        counted(text);
    }
    let opens = |line: &[u8]| {
        let budget = Counting::default();
        let answer = opens_pod_with_budget(line, &budget).unwrap();
        for fail_at in 1..=budget.charges.get() {
            assert!(matches!(
                opens_pod_with_budget(line, &failing_at(fail_at)),
                Err(SourceError::Cancelled)
            ));
        }
        (answer, charged_totals(&budget))
    };
    // expected: the command word and the byte that ends it; a line without `=` reads
    // nothing.
    assert_eq!(opens(b"=head1 NAME"), (true, (0, 6)));
    assert_eq!(opens(b"=pod"), (true, (0, 3)));
    assert_eq!(opens(b"=cut"), (false, (0, 3)));
    assert_eq!(opens(b"=1"), (false, (0, 1)));
    assert_eq!(opens(b"=head-x"), (false, (0, 5)));
    assert_eq!(opens(b"=pod\x0bx"), (true, (0, 4)));
    assert_eq!(opens(b"text"), (false, (0, 0)));
}

#[cfg(feature = "perl")]
#[test]
fn pod_text_charges_its_lines_and_copies() {
    let rendered = |source: &[u8]| {
        let parser = parsed(source);
        let [index, _] = indexes(&parser, Retain::Nothing);
        let fragment = &index.facts.fragments[0];
        let budget = Counting::default();
        let content = render(Profile::Pod, &index.source, &[fragment], &budget).unwrap();
        for fail_at in 1..=budget.charges.get() {
            assert!(matches!(
                render(
                    Profile::Pod,
                    &index.source,
                    &[fragment],
                    &failing_at(fail_at)
                ),
                Err(TextError::Source(SourceError::Cancelled))
            ));
        }
        (
            matches!(content, Content::Complete { .. }),
            budget.steps.get(),
            budget.bytes.get(),
            budget.checkpoints.get(),
        )
    };
    // expected: steps: one for the fragment, one per line that is kept before the empty
    // boundary lines go (4), three in the emitter (two empty-block tests and the block),
    // one for the dropped empty line, and two per copied run (4 runs). Bytes: the 21 of
    // the fragment for the line split, and twice the 15 of the text for validation and
    // copy. Two checkpoints per copied run.
    assert_eq!(rendered(PRIMARY), (true, 17, 51, 8));
    // One line, one run: 1 + 1 + 3 + 2 steps; 9 + 2 * 4 bytes.
    assert_eq!(rendered(b"=pod\n=cut\n"), (true, 7, 17, 2));
    // A decoded CRLF costs one step and two bytes: 1 + 4 + 3 + 1 + 2 + 1 + 1 + 2 steps;
    // 21 + 2 * 11 bytes.
    assert_eq!(
        rendered(b"=head1 A\r\n\r\na\r\n\r\n=cut\r\nsub f {}\r\n"),
        (true, 15, 43, 8)
    );
    // Pending text stops at the comment command: one step for the fragment and one for
    // each of the five lines up to `=begin comment`; the 62 bytes of the line split and
    // the space after `=begin` with the byte that ends it. Nothing is copied.
    assert_eq!(rendered(MIXED_FIXTURE), (false, 6, 64, 0));
}

#[cfg(feature = "perl")]
#[test]
fn pod_text_of_a_fragment_with_a_gap_is_unavailable() {
    // The collector withholds such a block, so this state is built by hand. The excluded
    // byte is the `u` of `=cut`: no line of the text would be copied across it.
    let source = b"=pod\n\nx\n\n=cut\n";
    let tree = restricted(source, &[0..11, 12..14]);
    let context = SourceContext::new(&tree, source);
    let block = nodes_of(tree.root_node(), &["pod_statement"]);
    let [(_, range, _)] = &block[..] else {
        panic!("one block, found {block:?}")
    };
    let block = node_at(tree.root_node(), "pod_statement", range.clone());
    let fragment = Fragment {
        source: context.pieces(0..13).unwrap(),
        node: Some(context.node(block).unwrap()),
        form: FragmentForm::Pod,
    };
    assert!(fragment.source().has_excluded_gap());
    assert!(matches!(
        render(Profile::Pod, &context, &[&fragment], &Counting::default()),
        Err(TextError::Unavailable(UnavailableReason::ExcludedGap))
    ));
}

#[cfg(feature = "perl")]
#[test]
fn perl_block_classification_is_charged_to_the_collection() {
    // A comment-only block of 4,000 rows: no document, so no text is built.
    let source = tall(4_000, "=for comment x\n\n", "=cut\n");
    let parser = parsed(&source);
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let (answer, (steps, bytes)) = block_charges(&source, 0, false);
    assert_eq!(answer, PodBlock::CommentOnly(source.len() - 1));
    // expected: the command word (4), the spelling, and per row 15 + 1 + 2 for the
    // command line and 1 for the empty line, then 4 + 1 for `=cut`.
    assert_eq!((steps, bytes), (1, 4 + source.len() - 1 + 4_000 * 19 + 5));
    let before = observations();
    let index = parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    assert_eq!(index.documents().len(), 0);
    // expected: the seven checkpoints of a collection without a document (`EMPTY`), then
    // one for each 16 KiB that the block's reading charges. The walk of the two nodes
    // charges no byte and fewer steps than one interval.
    assert_eq!(polls, EMPTY + bytes / BYTE_INTERVAL);
    for checkpoint in 1..=polls {
        assert!(matches!(
            cancel_after(checkpoint, || parser.documentation(&context, &options)),
            Err(CollectionError::Cancelled)
        ));
    }
    assert!(cancel_after(polls + 1, || parser.documentation(&context, &options)).is_ok());
    assert!(matches!(
        cancel_in_phase(WorkPhase::Classification, || parser
            .documentation(&context, &options)),
        Err(CollectionError::Cancelled)
    ));
    assert_eq!(view(&source), View::default());
}

#[cfg(feature = "perl")]
#[test]
fn perl_fragment_pieces_are_charged_to_the_collection() {
    // One comment-only block of 4,095 bytes and its line feed, read through 4,096
    // one-byte ranges that touch: no byte is excluded.
    let source = format!("=for comment {}\n\n=cut\n", "x".repeat(4_076)).into_bytes();
    let ranges: Vec<_> = (0..source.len()).map(|at| at..at + 1).collect();
    assert_eq!(ranges.len(), 4_096);
    let tree = restricted(&source, &ranges);
    assert_eq!(
        nodes_of(tree.root_node(), &["pod_statement"]),
        [("pod_statement", 0..4_095, "source_file")]
    );
    let collected = |retain| {
        let before = observations();
        let index = collect(
            LANG::Perl,
            &tree,
            &source,
            &DocumentationContext::default(),
            &options(retain),
        )
        .unwrap();
        let pieces: Vec<_> = index
            .evidence()
            .map(|candidate| {
                let fragment = index.evidence_fragment(candidate).unwrap();
                assert!(!fragment.get().source().has_excluded_gap());
                fragment.get().source().pieces().len()
            })
            .collect();
        assert_eq!(index.documents().len(), 0);
        (pieces, observations().polls - before.polls)
    };
    // expected: the seven checkpoints of a collection without a document, and four in
    // the range preparation, which steps once per range. The block's reading charges
    // 8,198 bytes, less than one interval, and the walk of two nodes a few steps.
    assert_eq!(collected(Retain::Nothing), (vec![], EMPTY + 4));
    // expected: cutting the fragment visits each of the 4,096 ranges, one step each,
    // after 12 steps of search: four more checkpoints. One piece per range of the block.
    assert_eq!(collected(Retain::Directives), (vec![4_095], EMPTY + 8));
}

#[cfg(feature = "perl")]
#[test]
fn perl_block_text_is_charged_to_the_collection() {
    // One documenting block of 4,000 paragraphs.
    let source = tall(4_000, "=head1 A\n\n", "=cut\n");
    let parser = parsed(&source);
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let (answer, (steps, bytes)) = block_charges(&source, 0, false);
    assert_eq!(answer, PodBlock::Documents(source.len() - 1));
    assert_eq!(steps, 1);
    let before = observations();
    let index = parser.documentation(&context, &options).unwrap();
    let polls = observations().polls - before.polls;
    let document = index.documents().next().unwrap();
    let text = index.complete_text(document).unwrap().unwrap();
    // expected: 4,000 lines and the 3,999 line feeds between them and the empty lines,
    // twice: `=head1 A`, LF, LF per paragraph, less the two line feeds after the last.
    assert_eq!(text.mappings().len(), 3 * 4_000 - 2);
    let budget = Counting::default();
    render(
        Profile::Pod,
        &index.source,
        &[&index.facts.fragments[0]],
        &budget,
    )
    .unwrap();
    // expected: two checkpoints per copied run of the text.
    assert_eq!(budget.checkpoints.get(), 2 * text.mappings().len());
    // expected: one document adds four checkpoints to the seven of `EMPTY`: a chunk and
    // an end for the order of its fragments, and a chunk each for the document order and
    // the association order.
    let explicit = EMPTY + 4 + budget.checkpoints.get();
    // expected: at least the explicit checkpoints, and at most one more per interval of
    // the bytes and steps that the block's reading and its text charge. The walk of the
    // two nodes and the assembly of one document charge fewer than 100 steps, which can
    // complete one more interval.
    let intervals = (bytes + budget.bytes.get()) / BYTE_INTERVAL
        + (steps + budget.steps.get()) / STEP_INTERVAL
        + 1;
    assert!(polls >= explicit, "{polls} < {explicit}");
    assert!(
        polls <= explicit + intervals,
        "{polls} > {explicit} + {intervals}"
    );
    assert!(matches!(
        cancel_in_phase(WorkPhase::Text, || parser.documentation(&context, &options)),
        Err(CollectionError::Cancelled)
    ));
    // A cancelled collection leaves nothing behind: the next one succeeds.
    assert_eq!(
        parser
            .documentation(&context, &options)
            .unwrap()
            .documents()
            .len(),
        1
    );
}

#[cfg(feature = "perl")]
#[test]
fn perl_many_blocks_cost_linear_work() {
    for blocks in [64, 512] {
        let source = tall(blocks, "=head1 A\n\na\n\n=cut\n", "");
        let parser = parsed(&source);
        // expected: the root and one leaf per block.
        assert_eq!(parser.root_node().preorder().count(), blocks + 1);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        // expected: one visit and one child batch per node.
        assert_eq!(after.visits - before.visits, blocks + 1);
        assert_eq!(after.child_batches - before.child_batches, blocks + 1);
        // expected: the file is the only owner.
        assert_eq!(after.owner_projections - before.owner_projections, 1);
        // expected: one text per document, built once.
        assert_eq!(after.renders - before.renders, blocks);
        // expected: no field name is requested or looked up for a Perl node.
        assert_eq!(after.field_requests - before.field_requests, 0);
        assert_eq!(after.field_lookups - before.field_lookups, 0);
        // expected: one fragment per block, cut with one search in one included range.
        assert_eq!(
            after.intersection_searches - before.intersection_searches,
            blocks
        );
        // expected: per block the command word `head1 ` (6), and the first line of the
        // comment-region test: `=head1 A` with its line feed (9) and its first byte (1).
        assert_eq!(
            after.classifier_bytes - before.classifier_bytes,
            16 * blocks
        );
        assert_eq!(index.documents().len(), blocks);
        assert_eq!(index.associations().len(), blocks);
        let last = index.documents().last().unwrap();
        let fragment = index.facts().document_fragment(last, 0).unwrap();
        assert_eq!(
            span(fragment.get().source()),
            18 * (blocks - 1)..18 * blocks - 1
        );
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_many_formats_cost_linear_work() {
    let block = "\n=pod\n\nd\n\n=cut\n";
    let collected = |source: &[u8]| {
        let parser = parsed(source);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let after = observations();
        // expected: one visit per node.
        assert_eq!(
            after.visits - before.visits,
            parser.root_node().preorder().count()
        );
        let bytes = after.classifier_bytes - before.classifier_bytes;
        // expected: within the bound of the rules for literal text: seven times the
        // input, where no literal is read. The block's own reading is counted too and
        // is far below the rest.
        assert!(bytes <= 7 * source.len(), "{bytes}");
        (index.documents().len(), bytes)
    };
    for formats in [64, 512] {
        // Many formats, each read once from its own opener.
        let source = tall(formats, "format F =\n1\n.\n2;\n", block);
        // expected: per format the opener ` F`, `F `, ` =` and the line feed twice (8)
        // and the two lines of the body (4). The block after the last format is a
        // document: the command word `pod` with its line feed (4), and its first line
        // with the line feed and its first byte (6).
        assert_eq!(collected(&source), (1, 12 * formats + 10));
        // One format whose body holds many opener lines: they are picture text, and no
        // scan starts from one of them.
        let body = "format B =\n".repeat(formats);
        let source = format!("format A =\n{body}1\n.\n2;\n{block}").into_bytes();
        // expected: the opener (8), every line of the body once (11 per opener line, 4
        // for the last two), and the block (10).
        assert_eq!(collected(&source), (1, 8 + 11 * formats + 4 + 10));
        // The same, and a second format behind it: the state moves on to the next
        // opener, 12 bytes, and the second block is a document too.
        let source = format!("format A =\n{body}1\n.\n2;\n{block}format C =\n3\n.\n4;\n{block}");
        assert_eq!(
            collected(source.as_bytes()),
            (2, 8 + 11 * formats + 4 + 10 + 12 + 10)
        );
        // A format without a last line: the body is read once to the end of the input,
        // and the block in it is withheld without reading it.
        let source = format!("format A =\n{body}1;\n{block}").into_bytes();
        // expected: the opener (8), the opener lines (11 each), `1;` (3), and the six
        // lines of the rest: an empty line, `=pod`, an empty line, `d`, an empty line,
        // `=cut` (15). The block passes the reader's own tests first: 4 and 6 bytes.
        assert_eq!(collected(&source), (0, 8 + 11 * formats + 3 + 15 + 10));
        // Many formats with a picture line that holds a field and a value line that is
        // a named variable.
        let source = tall(formats, "format F =\n@x;\n$a\n.\n2;\n", block);
        // expected: per format the opener (8), the picture line (4), the value line (3)
        // and the dot line (2): each byte of a body once.
        assert_eq!(collected(&source), (1, 17 * formats + 10));
        // The same with a string as the value: the end of the first format is not
        // known. It runs to the end of the input, and no later opener is read.
        let source = tall(formats, "format F =\n@x;\n\"a\"\n.\n2;\n", block);
        // expected: the opener (8), the picture line (4) and the value line (4), once.
        assert_eq!(collected(&source), (0, 16 + 10));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_many_literals_cost_linear_work() {
    let block = "\n=pod\n\nd\n\n=cut\n";
    for literals in [64, 512] {
        // A row, and what it holds: the bytes of its literal nodes, its patterns and its
        // single-quoted strings.
        let collected = |row: &str, (in_literals, patterns, quoted): (usize, usize, usize)| {
            let source = tall(literals, row, block);
            let parser = parsed(&source);
            let before = observations();
            let index = parser
                .documentation(
                    &DocumentationContext::default(),
                    &DocumentationOptions::default(),
                )
                .unwrap();
            let after = observations();
            assert_eq!(
                after.visits - before.visits,
                parser.root_node().preorder().count()
            );
            let bytes = after.classifier_bytes - before.classifier_bytes;
            // expected: within the bound: seven times the input; each byte of a literal
            // node once; 32 bytes before each pattern and one before each
            // single-quoted string.
            let bound = 7 * source.len() + literals * (in_literals + 32 * patterns + quoted);
            assert!(bytes <= bound, "{bytes} > {bound}");
            (index.documents().len(), bytes)
        };
        // expected: each string is read once, from its first byte to its last (8). Its
        // text holds `<<` and no tag. The block is a document: 10 bytes.
        assert_eq!(
            collected("my $s = \"a << b\";\n", (8, 0, 0)),
            (1, 8 * literals + 10)
        );
        // expected: each word list is read once (7).
        assert_eq!(
            collected("my @s = qw(a b);\n", (7, 0, 0)),
            (1, 7 * literals + 10)
        );
        // expected: each transliteration is read once, with both parts (7).
        assert_eq!(
            collected("my $n = tr/a/b/;\n", (7, 0, 0)),
            (1, 7 * literals + 10)
        );
        // expected: per row the byte before the string and the string (1 and 3), the 32
        // bytes before the pattern and the pattern (32 and 3). The first pattern has
        // only 27 bytes before it.
        assert_eq!(
            collected("my $s = 'a'; my $m = $s =~ /a/;\n", (6, 1, 1)),
            (1, 39 * literals - 5 + 10)
        );
        // expected: the first string holds an opener. It is read once (7), and no later
        // string is read. The block is withheld after its own tests.
        assert_eq!(collected("my $s = \"a <<b\";\n", (7, 0, 0)), (0, 7 + 10));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_hidden_pod_search_costs_linear_work() {
    let collected = |source: &[u8]| {
        let parser = parsed(source);
        let before = observations();
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let bytes = observations().classifier_bytes - before.classifier_bytes;
        // expected: the search reads no byte twice.
        assert!(bytes <= 2 * source.len(), "{bytes}");
        (index.documents().len(), index.assessments().len(), bytes)
    };
    for rows in [64, 512] {
        // A syntax error and many rows of code: the search reads every byte once, from
        // the `ERROR` node at the first byte to the end, and finds no line that starts
        // like a POD command.
        let source = format!("}}\n{}", "my $x = 1;\n".repeat(rows));
        assert_eq!(collected(source.as_bytes()), (0, 0, 2 + 11 * rows));
        // The same with many blocks: each has a node and is withheld, and the search
        // does not read it. expected: the two bytes before the first block, and the
        // line feed behind each block (1 per block); the reader's own tests (10 per
        // block).
        let source = format!("}}\n{}", "=pod\n\nd\n\n=cut\n".repeat(rows));
        assert_eq!(collected(source.as_bytes()), (0, rows, 11 * rows + 2));
        // A line in a string that starts like a POD command: the search ends there, and
        // the rows behind it are not read. expected: `}`, a line feed, `my $s = "`, a
        // line feed, `=p` (14). The file is marked.
        let source = format!("}}\nmy $s = \"\n=pod\n\";\n{}", "my $x = 1;\n".repeat(rows));
        assert_eq!(collected(source.as_bytes()), (0, 1, 14));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_doubt_scans_are_charged_to_the_collection() {
    // A format with a picture line of 70,000 blanks, a string of 70,000 letters, and a
    // syntax error before a number of 70,000 digits. Per source: the bytes that the
    // rules for literal text read, and the bytes that the collection charges.
    let sources = [
        (
            format!("format F =\n1{}\n.\n2;\n", " ".repeat(70_000)),
            70_012,
            70_012,
        ),
        (
            format!("my $s = \"{}\";\n", "a".repeat(70_000)),
            70_002,
            70_002,
        ),
        (format!("}}\nmy $x = {};\n", "1".repeat(70_000)), 0, 70_012),
    ];
    for (source, walk, read) in sources {
        let parser = parsed(source.as_bytes());
        let context = DocumentationContext::default();
        let options = DocumentationOptions::default();
        let budget = Counting::default();
        let walked = doubt_walk(
            &SourceContext::new(parser.ts_tree(), parser.code()),
            parser.ts_tree().root_node(),
            &budget,
        );
        assert_eq!(walked.unwrap(), vec![]);
        // expected: the format: the opener (8), the picture line with its line feed
        // (70,002) and the dot line (2). The string: every byte once. Behind the syntax
        // error the rules for literal text read nothing; the search for POD that the
        // grammar hides reads every byte of the input once.
        assert_eq!(budget.bytes.get(), walk);
        let before = observations();
        let index = parser.documentation(&context, &options).unwrap();
        let polls = observations().polls - before.polls;
        assert_eq!(index.documents().len(), 0);
        // expected: the seven checkpoints of a collection without a document, then one
        // for each 16 KiB that the scans charge. The walk of the few nodes charges no
        // other byte and fewer steps than one interval.
        assert_eq!(polls, EMPTY + read / BYTE_INTERVAL);
        for checkpoint in 1..=polls {
            assert!(matches!(
                cancel_after(checkpoint, || parser.documentation(&context, &options)),
                Err(CollectionError::Cancelled)
            ));
        }
        assert!(cancel_after(polls + 1, || parser.documentation(&context, &options)).is_ok());
        assert!(matches!(
            cancel_in_phase(WorkPhase::Classification, || parser
                .documentation(&context, &options)),
            Err(CollectionError::Cancelled)
        ));
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_empty_source_has_the_file_owner_and_nothing_else() {
    let parser = parsed(b"");
    for index in indexes(&parser, Retain::Nothing) {
        assert_eq!(
            published(&index, parser.ts_tree().root_node()),
            View::default()
        );
        let root = index
            .node_associations(parser.ts_tree().root_node())
            .unwrap();
        assert!(!root.projection_pending());
        assert_eq!(root.associations().len(), 0);
    }
}

#[cfg(feature = "perl")]
#[test]
#[allow(clippy::too_many_lines)]
fn perl_literal_forms_that_perl_reads_as_text_are_known_differences() {
    // Today's outcome, not a decision: in each source perl 5.34.0 reads the lines of the
    // block as text of a literal, Pod::Simple reads them as a POD block, and the unit
    // publishes the block. The provenance record names the seven forms.
    assert_views([
        // A comment between a term and a `/`: perl reads a division, the unit a pattern.
        (
            b"my $z = time # =\n/ 2; my $s = \"/; 1; \"; my $u = \";\n\
              =pod\n\nmy $inside = 6 * 7;\n\n=cut\n\";\n# \";\nprint \"MARK\";\n",
            documents([doc(51..82, "=pod\n\nmy $inside = 6 * 7;")]),
        ),
        (
            b"my $z = time # a,\n/ 2; my $s = \"/; 1; \"; my $u = \";\n\
              =pod\n\nmy $inside = 6 * 7;\n\n=cut\n\";\n# \";\nprint \"MARK\";\n",
            documents([doc(52..83, "=pod\n\nmy $inside = 6 * 7;")]),
        ),
        (
            b"my $z = time # return\n/ 2; my $s = \"/; 1; \"; my $u = \";\n\
              =pod\n\nmy $inside = 6 * 7;\n\n=cut\n\";\n# \";\nprint \"MARK\";\n",
            documents([doc(56..87, "=pod\n\nmy $inside = 6 * 7;")]),
        ),
        // `<<` and a digit behind `}` or behind a vertical tab opens a here-document.
        (
            b"my $fh = \\*STDOUT;\nprint {$fh}<<1;\n=pod\n\nLIT\n\n=cut\n1\n1;\n",
            documents([doc(35..50, "=pod\n\nLIT")]),
        ),
        (
            b"print {*STDOUT}<<1;\n=pod\n\nLIT\n\n=cut\n1\n1;\n",
            documents([doc(20..35, "=pod\n\nLIT")]),
        ),
        (
            b"my $fh = \\*STDOUT;\nprint $fh\x0b<<1;\n=pod\n\nLIT\n\n=cut\n1\n1;\n",
            documents([doc(34..49, "=pod\n\nLIT")]),
        ),
        // An escaped quote opens a here-document tag inside an evaluated literal.
        (
            b"my $s = \"@{[ <<\\\"EOT\\\" ]}\";\n=pod\n\nLIT\n\n=cut\nEOT\n1;\n",
            documents([doc(28..43, "=pod\n\nLIT")]),
        ),
        (
            b"my $s = \"@{[ << \\\"EOT\\\" ]}\";\n=pod\n\nLIT\n\n=cut\nEOT\n1;\n",
            documents([doc(29..44, "=pod\n\nLIT")]),
        ),
        (
            b"my $s = qq'@{[ <<\\'EOT\\' ]}';\n=pod\n\nLIT\n\n=cut\nEOT\n1;\n",
            documents([doc(30..45, "=pod\n\nLIT")]),
        ),
        // A quote-like operator in the picture text of a format reaches over the dot line.
        (
            b"format F =\nq\n.\nmy $s = \".;\n=pod\n\nLIT\n\n=cut\n\"; # \";\n1;\n",
            documents([doc(27..42, "=pod\n\nLIT")]),
        ),
        // A blank between a sigil and a name that spells a keyword.
        (
            b"my $split = 4; my $z = length $ split / 2; my $s = \"/; 1; \"; my $u = \";\n\
              =pod\n\nLIT\n\n=cut\n\"; # \";\n1;\n",
            documents([doc(72..87, "=pod\n\nLIT")]),
        ),
        // A lexical subroutine named like a keyword or like a quote-like operator.
        (
            b"my sub split () { 4 } my $z = split / 2; my $s = \"/; 1; \"; my $u = \";\n\
              =pod\n\nLIT\n\n=cut\n\"; # \";\n1;\n",
            documents([doc(70..85, "=pod\n\nLIT")]),
        ),
        (
            b"my sub q { 1 } my $x = q(\"a);\n=pod\n\nLIT\n\n=cut\n\"); # \";\n1;\n",
            documents([doc(30..45, "=pod\n\nLIT")]),
        ),
        // A quote character in a prototype.
        (
            b"sub q (\"$) { 1 } my $s = \") { 1 }\n=pod\n\nLIT\n\n=cut\n\"; # \";\n1;\n",
            documents([doc(34..49, "=pod\n\nLIT")]),
        ),
    ]);
}

// test-lang-gates: hand-written(perl) — the row helper is used only for Perl data leaves.
#[cfg(feature = "perl")]
#[test]
fn perl_data_section_row_helper_keeps_its_original_boundaries() {
    use crate::lang_helpers::perl::{PerlDataSectionRows, perl_data_section_rows};

    let cases: [(&[u8], usize, &[PerlDataSectionRows]); 3] = [
        (
            b"__END__\n=for comment\nold();\n\n=cut\n",
            7,
            &[PerlDataSectionRows::Pod {
                first: 1,
                last: 4,
                documentation: false,
            }],
        ),
        (
            b"__DATA__\n=pod\n=head1 X\n=cut2\n",
            8,
            &[PerlDataSectionRows::Pod {
                first: 1,
                last: 3,
                documentation: true,
            }],
        ),
        (
            b"__DATA__\n=pod\n=cut2\nplain data\n",
            8,
            &[
                PerlDataSectionRows::Pod {
                    first: 1,
                    last: 2,
                    documentation: true,
                },
                PerlDataSectionRows::Data(3),
            ],
        ),
    ];
    for (source, leaf_start, expected) in cases {
        let mut rows = Vec::new();
        perl_data_section_rows(&source[leaf_start..], 0, |row| rows.push(row));
        assert_eq!(rows, expected, "{}", source.escape_ascii());
    }
}
