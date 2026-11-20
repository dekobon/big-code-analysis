//! Native marker selection over borrowed traversal context. These are binding
//! candidates for owner adapters, not a declaration index or resolved owners.

use crate::lang_helpers::doc_comment::{self, DocBinding, StretchEnd};
use crate::node::WordSlot;
use crate::{Ancestors, Checker, Node};

use super::super::{NodeEvidence, SourceContext, SourceError, SourcePieces};
use crate::documentation::budget::{NoBudget, WorkBudget, infallible};
use crate::documentation::recognition::{PublicRules, SourceRules};

// Separate slots are necessary even on one visited node: Java's empty marker
// answers differently before the same Last selector runs.
#[derive(Clone, Copy)]
pub(crate) enum CandidateView<'s> {
    Legacy,
    Source(&'s SourceContext<'s>),
}

impl CandidateView<'_> {
    pub(crate) fn marker_with_budget<R: SourceRules, B: WorkBudget>(
        self,
        node: &Node,
        code: &[u8],
        ancestors: Ancestors,
        budget: &B,
    ) -> Result<bool, B::Error> {
        if !self.covers_with_budget(node.start_byte()..node.end_byte(), budget)? {
            return Ok(false);
        }
        let slot = match self {
            Self::Legacy => WordSlot::LegacyDocMarker,
            Self::Source(_) => WordSlot::SourceDocMarker,
        };
        Ok(ancestors.try_classified(slot, node, || {
            marker_reads::record();
            Ok(u8::from(match self {
                Self::Legacy => R::Checker::is_doc_comment(node, code),
                Self::Source(_) => R::marker(node, code, budget)?,
            }))
        })? != 0)
    }
    pub(crate) fn covers_with_budget<B: WorkBudget>(
        self,
        span: std::ops::Range<usize>,
        budget: &B,
    ) -> Result<bool, B::Error> {
        match self {
            Self::Legacy => Ok(true),
            Self::Source(context) => context.covers_with_budget(span, budget),
        }
    }

    pub(crate) fn marker<T: Checker>(self, node: &Node, code: &[u8], ancestors: Ancestors) -> bool {
        infallible(self.marker_with_budget::<PublicRules<T>, _>(node, code, ancestors, &NoBudget))
    }
}

pub(crate) fn legacy_marker<T: Checker>(node: &Node, code: &[u8], ancestors: Ancestors) -> bool {
    CandidateView::Legacy.marker::<T>(node, code, ancestors)
}

// The result cannot reveal extra marker reads or evidence built then discarded.
crate::observation::counter!(marker_reads, cfg(any(test, feature = "test-support")));
crate::observation::counter!(evidence_built, cfg(any(test, feature = "test-support")));
pub use doc_comment::{gatherer_probes, stretches_collected};

/// Explicit source-only request for a language's native marker binding profile.
/// The caller visits comments once in source order, with its walk's caches.
/// Superseded and unattached candidates are retained only when requested.
pub struct SourceRequest<'r, 's> {
    /// Exact tree/source context, prepared by the source consumer alone.
    pub context: &'r SourceContext<'s>,
    /// Retain rejected and superseded candidates in addition to selected ones.
    pub retain_unbound: bool,
    /// Receives each completed stretch once, on its final comment visit.
    pub sink: &'r mut dyn FnMut(MarkerRun<'s>),
}

/// A completed native marker stretch. All node fields are canonical source
/// evidence; a target is a binding candidate, never a resolved semantic owner.
#[derive(Debug)]
pub struct MarkerRun<'s> {
    /// Source occurrences in order, including explicitly unknown gap-spanning
    /// comments when requested, without repeated gatherers.
    pub members: Vec<MarkerMember<'s>>,
    /// Following target wrapper, possibly a conditional or recovery node.
    pub target: Option<NodeEvidence<'s>>,
    /// Conditional/recovery evidence, not a set of simultaneously resolved owners.
    pub target_uncertainty: Option<super::super::UnresolvedReason>,
    /// Immediate containing syntax; owner adapters refine this context.
    pub containing: Option<NodeEvidence<'s>>,
    /// Preceding code retained for after-code placement decisions.
    pub code_before: Option<NodeEvidence<'s>>,
    /// Excluded source precedes this stretch. Composition may be incomplete even
    /// when only selected members were requested and their attachment is known.
    pub prefix_gap: bool,
    /// Excluded source remains after the last examined sibling. Local or
    /// gathered selection can be known while composition remains incomplete.
    pub suffix_gap: bool,
}

/// One physical source marker or uncertain comment and its selection result.
#[derive(Debug)]
pub struct MarkerMember<'s> {
    /// Exact syntax node, including any grammar-owned newline.
    pub node: NodeEvidence<'s>,
    /// Included spelling, excluding a line comment's LF/CRLF terminator.
    /// May extend one byte beyond `node` to preserve an included lone CR that
    /// the grammar left outside the comment node.
    pub source: SourcePieces<'s>,
    /// Selection within this profile, independent of legacy billing.
    pub selection: MarkerSelection,
    /// Binding direction/context, still requiring the language's owner adapter.
    pub placement: MarkerPlacement<'s>,
    /// Known source uncertainty; no semantic owner is inferred from macro tokens.
    pub uncertainty: Option<super::super::UnresolvedReason>,
}

/// Native profile selection of a recognized marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerSelection {
    /// Contributes to forward or local source documentation.
    Selected,
    /// Another candidate of the stretch replaced this forward marker.
    Superseded,
    /// No target was selected, including after-code and rejected trivia.
    Unattached,
}

struct RunSelection<'t> {
    kept: Vec<Node<'t>>,
    target: Option<Node<'t>>,
    gatherer: Option<Node<'t>>,
    bound: Option<doc_comment::BoundSelection<'t>>,
    gap: bool,
    suffix_gap: bool,
    prefix_gap_row: Option<usize>,
}

impl<'t> RunSelection<'t> {
    fn select<R: SourceRules, B: WorkBudget>(
        collected: &doc_comment::CollectedStretch<'t>,
        end: StretchEnd<'t>,
        binding: DocBinding,
        context: &SourceContext<'t>,
        ancestors: Ancestors<'t, '_>,
        budget: &B,
    ) -> Result<Self, B::Error> {
        let stretch = &collected.members;
        let code_before = collected.code_before;
        let code = context.bytes();
        let marker = |n: &Node<'t>| {
            CandidateView::Source(context).marker_with_budget::<R, B>(n, code, ancestors, budget)
        };
        let forward = |n: &Node<'t>| {
            Ok(marker(n)? && R::forward(n, context, ancestors, budget)? == Some(true))
        };
        if let StretchEnd::AtTarget { next, target } = end {
            // Only a leading-trivia policy that rejects after-code comments
            // needs the missing prefix; later rows already have a line break.
            let prefix_gap_row = match binding.kept {
                doc_comment::KeptDocs::Leading { after_code, .. }
                    if collected.prefix_gap && !after_code(&target) =>
                {
                    stretch.first().map(Node::start_row)
                }
                _ => None,
            };
            let mut bound = doc_comment::select_bound_with_budget::<R::Checker, B>(
                stretch,
                code,
                binding.kept,
                doc_comment::StretchBounds {
                    code_before,
                    next,
                    target,
                },
                forward,
                budget,
            )?;
            return Ok(Self {
                kept: std::mem::take(&mut bound.nodes),
                target: Some(target),
                gatherer: None,
                bound: Some(bound),
                gap: false,
                suffix_gap: false,
                prefix_gap_row,
            });
        }
        let result = if binding.gathers.is_some() {
            Some(doc_comment::bind_gathered_with_budget(
                stretch,
                code,
                |node| R::gathers(node, code, budget),
                marker,
                forward,
                budget,
            )?)
        } else {
            None
        };
        let doc_comment::Gathered { kept, gatherer } = result.unwrap_or(doc_comment::Gathered {
            kept: Vec::new(),
            gatherer: None,
        });
        Ok(Self {
            kept,
            target: None,
            gatherer,
            bound: None,
            gap: matches!(end, StretchEnd::ExcludedGap),
            suffix_gap: matches!(end, StretchEnd::SuffixGap),
            prefix_gap_row: None,
        })
    }

    fn members<R: SourceRules, B: WorkBudget>(
        &self,
        stretch: &[Node<'t>],
        binding: DocBinding,
        ancestors: Ancestors<'t, '_>,
        request: &SourceRequest<'_, 't>,
        budget: &B,
    ) -> Result<Vec<MarkerMember<'t>>, SourceError> {
        // bca: suppress(halstead, cognitive, cyclomatic, abc) -- one ordered merge preserves selection, uncertain tokens and lazy evidence without rescanning the stretch.
        let code = request.context.bytes();
        let mut selected = self.kept.iter().peekable();
        let mut members = Vec::new();
        for (index, node) in stretch.iter().enumerate() {
            budget.step().map_err(Into::into)?;
            if !request
                .context
                .covers_with_budget(node.start_byte()..node.end_byte(), budget)
                .map_err(Into::into)?
            {
                if request.retain_unbound && R::Checker::is_comment(node) {
                    let mut evidence = member(
                        request.context,
                        node,
                        MarkerSelection::Unattached,
                        MarkerPlacement::Unknown,
                        budget,
                    )?;
                    evidence.uncertainty = Some(super::super::UnresolvedReason::ExcludedGap);
                    members.push(evidence);
                }
                continue;
            }
            if !CandidateView::Source(request.context)
                .marker_with_budget::<R, B>(node, code, ancestors, budget)
                .map_err(Into::into)?
            {
                continue;
            }
            let kept = selected.peek().is_some_and(|n| n.id() == node.id());
            if kept {
                selected.next();
            }
            let Some(forward) =
                R::forward(node, request.context, ancestors, budget).map_err(Into::into)?
            else {
                if request.retain_unbound {
                    let mut evidence = member(
                        request.context,
                        node,
                        MarkerSelection::Unattached,
                        MarkerPlacement::Unknown,
                        budget,
                    )?;
                    evidence.uncertainty = Some(super::super::UnresolvedReason::ExcludedGap);
                    members.push(evidence);
                }
                continue;
            };
            let selection = if kept || !forward {
                MarkerSelection::Selected
            } else if self
                .bound
                .as_ref()
                .is_some_and(|bound| bound.eligible(index, stretch, code))
                && !self.kept.is_empty()
            {
                MarkerSelection::Superseded
            } else {
                MarkerSelection::Unattached
            };
            if selection != MarkerSelection::Selected && !request.retain_unbound {
                continue;
            }
            let placement = match (forward, kept, self.gatherer) {
                (false, _, _)
                    if binding.gathers.is_some()
                        && R::gathers(node, code, budget).map_err(Into::into)? =>
                {
                    MarkerPlacement::Gatherer
                }
                (false, _, _) => MarkerPlacement::Local,
                (true, true, Some(gatherer)) => {
                    MarkerPlacement::Gathered(request.context.node(gatherer.as_tree_sitter())?)
                }
                _ => MarkerPlacement::Forward,
            };
            let mut member = member(request.context, node, selection, placement, budget)?;
            member.uncertainty = R::Checker::source_doc_uncertainty(node, code, ancestors);
            if self.suffix_gap
                || (forward
                    && ((self.gap && !kept) || self.prefix_gap_row == Some(node.start_row())))
            {
                member.uncertainty = Some(super::super::UnresolvedReason::ExcludedGap);
            }
            members.push(member);
        }
        Ok(members)
    }
}

/// Structural placement, deliberately not a semantic owner claim. Local macro
/// tokens and conditional targets retain uncertainty until an adapter resolves it.
#[derive(Clone, Copy, Debug)]
pub enum MarkerPlacement<'s> {
    /// Excluded source prevents recognizing the marker or deciding its direction.
    Unknown,
    /// Candidate for the following target wrapper.
    Forward,
    /// Inner, trailing, signature, structural or expansion-local documentation.
    Local,
    /// A structural comment with its own document, retained once even when it
    /// also gathers preceding forward markers.
    Gatherer,
    /// Contributes to this structural comment's document.
    Gathered(NodeEvidence<'s>),
}

/// Recognize native source markers at a comment visit. Uses the same stretch
/// selection algorithms as legacy binding, with separate pre-selection predicates
/// and source-only gap barriers. A deferred member never collects its prefix.
///
/// # Errors
/// Propagates source identity/range failures; no successful partial run is emitted.
pub fn source_comment<'s, T: Checker>(
    comment: &Node<'s>,
    ancestors: Ancestors<'s, '_>,
    request: &mut SourceRequest<'_, 's>,
) -> Result<(), SourceError> {
    source_comment_with_budget::<PublicRules<T>, _>(comment, ancestors, request, &NoBudget)
}

pub(crate) fn source_comment_with_budget<'s, R: SourceRules, B: WorkBudget>(
    comment: &Node<'s>,
    ancestors: Ancestors<'s, '_>,
    request: &mut SourceRequest<'_, 's>,
    budget: &B,
) -> Result<(), SourceError> {
    // bca: suppress(nexits, halstead) -- early deferral and source identity failures propagate before any sink call.
    let Some(binding) = R::Checker::source_doc_binding() else {
        return Ok(());
    };
    let code = request.context.bytes();
    request.context.node(comment.as_tree_sitter())?;
    let view = CandidateView::Source(request.context);
    let end = doc_comment::stretch_end_with_budget::<R, B>(
        comment,
        code,
        ancestors,
        |node, ancestors| R::skips(node, ancestors, budget),
        view,
        budget,
    )
    .map_err(Into::into)?;
    if matches!(end, StretchEnd::Undecided) {
        return Ok(());
    }
    if matches!(
        end,
        StretchEnd::Elsewhere | StretchEnd::ExcludedGap | StretchEnd::SuffixGap
    ) && !request.retain_unbound
        && !R::Checker::SOURCE_LOCAL_DOCS
        && binding.gathers.is_none()
    {
        return Ok(());
    }
    let collected = doc_comment::collect_stretch_with_budget::<R, B>(
        comment,
        code,
        ancestors,
        |node, ancestors| R::skips(node, ancestors, budget),
        view,
        budget,
    )
    .map_err(Into::into)?;
    let code_before = collected.code_before;
    let selection =
        RunSelection::select::<R, B>(&collected, end, binding, request.context, ancestors, budget)
            .map_err(Into::into)?;
    let members =
        selection.members::<R, B>(&collected.members, binding, ancestors, request, budget)?;
    if !members.is_empty() {
        let canonical = |node: Node<'s>| request.context.node(node.as_tree_sitter());
        (request.sink)(MarkerRun {
            members,
            target: selection.target.map(canonical).transpose()?,
            target_uncertainty: selection
                .target
                .and_then(|node| R::Checker::source_doc_uncertainty(&node, code, ancestors)),
            containing: ancestors.parent(comment).map(canonical).transpose()?,
            code_before: code_before.map(canonical).transpose()?,
            prefix_gap: collected.prefix_gap,
            suffix_gap: selection.suffix_gap,
        });
    }
    Ok(())
}

fn member<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    node: &Node<'s>,
    selection: MarkerSelection,
    placement: MarkerPlacement<'s>,
    budget: &B,
) -> Result<MarkerMember<'s>, SourceError> {
    evidence_built::record();
    let bytes = doc_comment::node_bytes(node, context.bytes());
    let end = if bytes.starts_with(b"//")
        && context
            .covers_with_budget(node.start_byte()..node.start_byte() + 2, budget)
            .map_err(Into::into)?
    {
        line_spelling_end(context, node, budget).map_err(Into::into)?
    } else {
        node.end_byte()
    };
    Ok(MarkerMember {
        node: context.node(node.as_tree_sitter())?,
        source: context.pieces_with_budget(node.start_byte()..end, budget)?,
        selection,
        placement,
        uncertainty: None,
    })
}

pub(super) fn line_spelling_end<B: WorkBudget>(
    context: &SourceContext<'_>,
    node: &Node<'_>,
    budget: &B,
) -> Result<usize, B::Error> {
    let code = context.bytes();
    let end = node.end_byte();
    let lf = if context.covers_with_budget(end.saturating_sub(1)..end, budget)?
        && code.get(end.wrapping_sub(1)) == Some(&b'\n')
    {
        end - 1
    } else {
        end
    };
    if context.covers_with_budget(lf..lf + 1, budget)? && code.get(lf) == Some(&b'\n') {
        if context.covers_with_budget(lf.saturating_sub(1)..lf + 1, budget)?
            && code.get(lf.wrapping_sub(1)) == Some(&b'\r')
        {
            return Ok(lf - 1);
        }
        return Ok(lf);
    }
    // Some grammars exclude CR from the node even when no included LF follows it.
    if context.covers_with_budget(end..end + 1, budget)?
        && code.get(end) == Some(&b'\r')
        && !(context.covers_with_budget(end + 1..end + 2, budget)?
            && code.get(end + 1) == Some(&b'\n'))
    {
        return Ok(end + 1);
    }
    Ok(end)
}

#[cfg(test)]
mod regressions;
#[cfg(test)]
mod tests;
