//! Positional run evidence over the same final-member scan as legacy LOC.

use crate::lang_helpers::doc_comment::{self, PositionalRun};
use crate::{Ancestors, Checker, Node};

use super::super::{NodeEvidence, SourceContext, SourceError, SourcePieces, UnresolvedReason};
use super::markers::CandidateView;

pub use doc_comment::{members_materialized, positional_predecessors, positional_runs_collected};
crate::observation::counter!(evidence_built, cfg(any(test, feature = "test-support")));

/// Why the final comment cannot defer to another member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    /// Another non-comment token follows, including missing and ERROR tokens.
    Token,
    /// A blank source row separates this run from the next sibling.
    BlankLine,
    /// The language's long-comment terminator ends the run.
    Terminator,
    /// No further parsed sibling exists. `SourceRun::suffix_gap` distinguishes
    /// unknown excluded source from a complete scope ending.
    EndOfScope,
    /// Excluded bytes interrupt the run or the following token.
    ExcludedGap,
}

/// Content-based selection, independent of a concrete declaration owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenerReason {
    /// An explicit directive opens documentation without requiring a target.
    Explicit,
    /// LDoc's first qualifying file block is module documentation.
    FirstFile,
    /// A tagged LDoc block supplies its own structural evidence.
    Tagged,
    /// A marked LDoc block precedes a supported item.
    FollowingItem,
}

/// Selection of a positional run under the native source convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunSelection {
    /// An ordinary run precedes a supported candidate.
    FollowingTarget,
    /// The opener, rather than bare adjacency, makes this documentation.
    Opener(OpenerReason),
    /// No native attachment was established.
    Unattached,
}

/// Completed run context for language content predicates. All members are
/// siblings on the caller's actual chain, in source order.
pub struct RunContext<'r, 't> {
    /// Complete ordered comment run, before directive filtering.
    pub members: &'r [Node<'t>],
    /// Immediate next-row sibling, before candidate classification.
    pub below: Option<Node<'t>>,
    /// Actual parsed buffer.
    pub code: &'r [u8],
    /// Source-only included-range context; absent on the legacy path.
    pub source: Option<&'r SourceContext<'t>>,
}

/// A source request. Rejected runs and directive-only members are independently
/// opt-in; the legacy path constructs neither this request nor source evidence.
pub struct SourceRequest<'r, 's> {
    /// Exact borrowed parse context.
    pub context: &'r SourceContext<'s>,
    /// Retain runs that have no native attachment.
    pub retain_unbound: bool,
    /// Retain recognized non-prose directives in their original order.
    pub retain_directives: bool,
    /// Receives a completed run at its final comment visit.
    pub sink: &'r mut dyn FnMut(SourceRun<'s>),
}

/// Positional binding evidence; owner normalization remains language-specific.
#[derive(Debug)]
pub struct SourceRun<'s> {
    /// Retained physical occurrences, in source order.
    pub members: Vec<RunMember<'s>>,
    /// Native selection reason, never a concrete owner assertion.
    pub selection: RunSelection,
    /// Following supported candidate, even when an opener selects a virtual or
    /// file document instead. Adapters must inspect `selection` before binding.
    pub target: Option<NodeEvidence<'s>>,
    /// Immediate following sibling, including a non-target or recovery token.
    pub following: Option<NodeEvidence<'s>>,
    /// Immediate lexical container from the walk's chain.
    pub containing: Option<NodeEvidence<'s>>,
    /// Preceding token when known. Never fabricated across excluded source.
    pub code_before: Option<NodeEvidence<'s>>,
    /// Excluded source before this run may make composition incomplete.
    pub prefix_gap: bool,
    /// Excluded source remains after parsed sibling exhaustion. Independently
    /// known selection survives, but composition or attachment is incomplete.
    pub suffix_gap: bool,
    /// Why the run ended.
    pub end: RunEnd,
    /// An excluded gap or recovery boundary prevented a certain decision.
    pub uncertainty: Option<UnresolvedReason>,
}

/// A comment occurrence; raw bytes need not be valid UTF-8.
#[derive(Debug)]
pub struct RunMember<'s> {
    /// Exact canonical syntax identity and grammar-owned span.
    pub node: NodeEvidence<'s>,
    /// Included comment spelling, without a line-comment LF/CRLF terminator.
    pub source: SourcePieces<'s>,
    /// Whether the language excludes this member from documentation prose.
    pub directive: bool,
    /// Whether this occurrence contributes prose under the native selection.
    /// Trailing prefixes and directives remain evidence only.
    pub selected: bool,
}

/// Stream native positional evidence at a comment visit. Ordinary extras that
/// are comments join; every other token, including an extra ERROR, stops a run.
/// No generic leading-comment inference is enabled here.
///
/// # Errors
/// Rejects foreign parse context or invalid source ranges before emitting a run.
pub fn source_comment<'s, T: Checker>(
    comment: &Node<'s>,
    ancestors: Ancestors<'s, '_>,
    request: &mut SourceRequest<'_, 's>,
) -> Result<(), SourceError> {
    // bca: suppress(nexits) -- defer or reject before canonicalizing a complete run; every identity failure precedes the sink.
    if !T::SOURCE_POSITIONAL_DOCS {
        return Ok(());
    }
    request.context.node(comment.as_tree_sitter())?;
    let code = request.context.bytes();
    let Some(run) = doc_comment::positional_run::<T>(
        comment,
        code,
        ancestors,
        CandidateView::Source(request.context),
    ) else {
        return Ok(());
    };
    let collected = SourceMembers::collect(&run, ancestors, request.context);
    let decision = SourceDecision::new::<T>(&run, &collected, ancestors, request.context);
    if decision.selection == RunSelection::Unattached && !request.retain_unbound {
        return Ok(());
    }
    let members = source_members::<T>(&collected, decision.selection, request)?;
    if !members.is_empty() {
        let canonical = |node: Node<'s>| request.context.node(node.as_tree_sitter());
        (request.sink)(SourceRun {
            members,
            selection: decision.selection,
            target: decision.target.map(canonical).transpose()?,
            following: run.next.map(canonical).transpose()?,
            containing: ancestors.parent(comment).map(canonical).transpose()?,
            code_before: run.code_before.map(canonical).transpose()?,
            prefix_gap: run.prefix_gap,
            suffix_gap: run.suffix_gap,
            end: run.end,
            uncertainty: decision.uncertainty,
        });
    }
    Ok(())
}

struct SourceDecision<'t> {
    selection: RunSelection,
    target: Option<Node<'t>>,
    uncertainty: Option<UnresolvedReason>,
}

struct SourceMembers<'t> {
    nodes: Vec<Node<'t>>,
    eligible_start: usize,
    prefix_placement_gap: bool,
}

impl<'t> SourceMembers<'t> {
    fn collect(
        run: &PositionalRun<'t>,
        ancestors: Ancestors<'t, '_>,
        source: &SourceContext<'t>,
    ) -> Self {
        let nodes = run.materialize(ancestors);
        let mut eligible_start = run.first_eligible.map_or(nodes.len(), |first| {
            nodes.partition_point(|node| node.start_byte() < first.start_byte())
        });
        let prefix_placement_gap = run.prefix_gap
            && nodes
                .get(eligible_start)
                .is_some_and(|first| !complete_row_prefix(first, source));
        if prefix_placement_gap {
            let row = nodes[eligible_start].start_row();
            eligible_start += nodes[eligible_start..]
                .iter()
                .take_while(|node| node.start_row() == row)
                .count();
        }
        Self {
            nodes,
            eligible_start,
            prefix_placement_gap,
        }
    }
}

fn complete_row_prefix(first: &Node<'_>, source: &SourceContext<'_>) -> bool {
    let start = first
        .start_byte()
        .saturating_sub(first.as_tree_sitter().start_position().column);
    source.covers(start..first.start_byte())
        && source.bytes()[start..first.start_byte()]
            .iter()
            .all(u8::is_ascii_whitespace)
}

impl<'t> SourceDecision<'t> {
    fn new<T: Checker>(
        run: &PositionalRun<'t>,
        collected: &SourceMembers<'t>,
        ancestors: Ancestors<'t, '_>,
        source: &SourceContext<'t>,
    ) -> Self {
        let mut decision = Self {
            selection: RunSelection::Unattached,
            target: None,
            uncertainty: (run.suffix_gap || collected.prefix_placement_gap)
                .then_some(UnresolvedReason::ExcludedGap),
        };
        if run.end == RunEnd::ExcludedGap
            || !source.covers(run.first.start_byte()..run.last.end_byte())
        {
            decision.uncertainty = Some(UnresolvedReason::ExcludedGap);
            return decision;
        }
        if collected.eligible_start == collected.nodes.len() {
            return decision;
        }
        decision.target = run
            .below
            .filter(|n| T::is_source_doc_target(n, source.bytes(), ancestors));
        let context = RunContext {
            members: &collected.nodes[collected.eligible_start..],
            below: run.below,
            code: source.bytes(),
            source: Some(source),
        };
        match T::source_doc_run_opener(&context, ancestors) {
            Ok(Some(reason)) => decision.selection = RunSelection::Opener(reason),
            Ok(None) if T::SOURCE_POSITIONAL_TARGETS && decision.target.is_some() => {
                decision.selection = RunSelection::FollowingTarget;
            }
            Ok(None) => {}
            Err(reason) => decision.uncertainty = Some(reason),
        }
        if decision.uncertainty.is_none() {
            decision.uncertainty = run
                .next
                .filter(|n| n.has_error() || n.is_missing())
                .map(|_| UnresolvedReason::GrammarBoundary);
        }
        decision
    }
}

fn source_members<'s, T: Checker>(
    collected: &SourceMembers<'s>,
    selection: RunSelection,
    request: &SourceRequest<'_, 's>,
) -> Result<Vec<RunMember<'s>>, SourceError> {
    let mut members = Vec::new();
    for (index, node) in collected.nodes.iter().enumerate() {
        let ineligible = index < collected.eligible_start;
        if ineligible && !request.retain_unbound {
            continue;
        }
        let directive = request.context.covers(node.start_byte()..node.end_byte())
            && T::is_doc_run_directive(node, request.context.bytes());
        if directive && !request.retain_directives {
            continue;
        }
        evidence_built::record();
        members.push(RunMember {
            node: request.context.node(node.as_tree_sitter())?,
            source: request
                .context
                .pieces(node.start_byte()..spelling_end(request.context, node))?,
            directive,
            selected: !ineligible && !directive && selection != RunSelection::Unattached,
        });
    }
    Ok(members)
}

fn spelling_end(context: &SourceContext<'_>, node: &Node<'_>) -> usize {
    let bytes = doc_comment::node_bytes(node, context.bytes());
    if bytes.starts_with(b"//")
        || bytes.starts_with(b"#")
        || (bytes.starts_with(b"--") && !doc_comment::lua_long_comment(bytes))
    {
        super::markers::line_spelling_end(context, node)
    } else {
        node.end_byte()
    }
}

#[cfg(test)]
mod tests;
