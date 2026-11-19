//! Native marker selection over borrowed traversal context. These are binding
//! candidates for owner adapters, not a declaration index or resolved owners.

use crate::lang_helpers::doc_comment::{self, DocBinding, StretchEnd};
use crate::node::WordSlot;
use crate::{Ancestors, Checker, Node};

use super::super::{NodeEvidence, SourceContext, SourceError, SourcePieces};

// Separate slots are necessary even on one visited node: Java's empty marker
// answers differently before the same Last selector runs.
#[derive(Clone, Copy)]
pub(crate) enum CandidateView<'s> {
    Legacy,
    Source(&'s SourceContext<'s>),
}

impl CandidateView<'_> {
    pub(crate) fn covers(self, span: std::ops::Range<usize>) -> bool {
        match self {
            Self::Legacy => true,
            Self::Source(context) => context.covers(span),
        }
    }

    pub(crate) fn marker<T: Checker>(self, node: &Node, code: &[u8], ancestors: Ancestors) -> bool {
        if !self.covers(node.start_byte()..node.end_byte()) {
            return false;
        }
        let slot = match self {
            Self::Legacy => WordSlot::LegacyDocMarker,
            Self::Source(_) => WordSlot::SourceDocMarker,
        };
        ancestors.classified(slot, node, || {
            marker_reads::record();
            u8::from(match self {
                Self::Legacy => T::is_doc_comment(node, code),
                Self::Source(_) => T::is_source_doc_comment(node, code),
            })
        }) != 0
    }

    pub(crate) fn target<'t, T: Checker>(
        self,
        node: &Node<'t>,
        code: &[u8],
        ancestors: Ancestors<'t, '_>,
    ) -> bool {
        match self {
            Self::Legacy => T::is_doc_target(node, code, ancestors),
            Self::Source(_) => {
                self.covers(node.start_byte()..node.end_byte())
                    && T::is_source_doc_target(node, code, ancestors)
            }
        }
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
    prefix_gap_row: Option<usize>,
}

impl<'t> RunSelection<'t> {
    fn select<T: Checker>(
        collected: &doc_comment::CollectedStretch<'t>,
        end: StretchEnd<'t>,
        binding: DocBinding,
        context: &SourceContext<'t>,
        ancestors: Ancestors<'t, '_>,
    ) -> Self {
        let stretch = &collected.members;
        let code_before = collected.code_before;
        let code = context.bytes();
        let marker = |n: &Node<'t>| CandidateView::Source(context).marker::<T>(n, code, ancestors);
        let forward = |n: &Node<'t>| {
            marker(n) && T::source_doc_binds_forward(n, context, ancestors) == Some(true)
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
            let mut bound = doc_comment::select_bound::<T>(
                stretch,
                code,
                binding.kept,
                doc_comment::StretchBounds {
                    code_before,
                    next,
                    target,
                },
                forward,
            );
            return Self {
                kept: std::mem::take(&mut bound.nodes),
                target: Some(target),
                gatherer: None,
                bound: Some(bound),
                gap: false,
                prefix_gap_row,
            };
        }
        let result = binding
            .gathers
            .map(|gathers| doc_comment::bind_gathered(stretch, code, gathers, marker, forward));
        let doc_comment::Gathered { kept, gatherer } = result.unwrap_or(doc_comment::Gathered {
            kept: Vec::new(),
            gatherer: None,
        });
        Self {
            kept,
            target: None,
            gatherer,
            bound: None,
            gap: matches!(end, StretchEnd::ExcludedGap),
            prefix_gap_row: None,
        }
    }

    fn members<T: Checker>(
        &self,
        stretch: &[Node<'t>],
        binding: DocBinding,
        ancestors: Ancestors<'t, '_>,
        request: &SourceRequest<'_, 't>,
    ) -> Result<Vec<MarkerMember<'t>>, SourceError> {
        // bca: suppress(halstead, cognitive, cyclomatic, abc) -- one ordered merge preserves selection, uncertain tokens and lazy evidence without rescanning the stretch.
        let code = request.context.bytes();
        let mut selected = self.kept.iter().peekable();
        let mut members = Vec::new();
        for (index, node) in stretch.iter().enumerate() {
            if !request.context.covers(node.start_byte()..node.end_byte()) {
                if request.retain_unbound && T::is_comment(node) {
                    let mut evidence = member(
                        request.context,
                        node,
                        MarkerSelection::Unattached,
                        MarkerPlacement::Unknown,
                    )?;
                    evidence.uncertainty = Some(super::super::UnresolvedReason::ExcludedGap);
                    members.push(evidence);
                }
                continue;
            }
            if !CandidateView::Source(request.context).marker::<T>(node, code, ancestors) {
                continue;
            }
            let kept = selected.peek().is_some_and(|n| n.id() == node.id());
            if kept {
                selected.next();
            }
            let Some(forward) = T::source_doc_binds_forward(node, request.context, ancestors)
            else {
                if request.retain_unbound {
                    let mut evidence = member(
                        request.context,
                        node,
                        MarkerSelection::Unattached,
                        MarkerPlacement::Unknown,
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
                (false, _, _) if binding.gathers.is_some_and(|gathers| gathers(node, code)) => {
                    MarkerPlacement::Gatherer
                }
                (false, _, _) => MarkerPlacement::Local,
                (true, true, Some(gatherer)) => {
                    MarkerPlacement::Gathered(request.context.node(gatherer.as_tree_sitter())?)
                }
                _ => MarkerPlacement::Forward,
            };
            let mut member = member(request.context, node, selection, placement)?;
            member.uncertainty = T::source_doc_uncertainty(node, code, ancestors);
            if forward && ((self.gap && !kept) || self.prefix_gap_row == Some(node.start_row())) {
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
    // bca: suppress(nexits, halstead) -- early deferral and source identity failures propagate before any sink call.
    let Some(binding) = T::source_doc_binding() else {
        return Ok(());
    };
    let code = request.context.bytes();
    request.context.node(comment.as_tree_sitter())?;
    let view = CandidateView::Source(request.context);
    let end = doc_comment::stretch_end::<T>(comment, code, ancestors, binding.skips, view);
    if matches!(end, StretchEnd::Undecided) {
        return Ok(());
    }
    if matches!(end, StretchEnd::Elsewhere | StretchEnd::ExcludedGap)
        && !request.retain_unbound
        && !T::SOURCE_LOCAL_DOCS
        && binding.gathers.is_none()
    {
        return Ok(());
    }
    let collected =
        doc_comment::collect_stretch::<T>(comment, code, ancestors, binding.skips, view);
    let code_before = collected.code_before;
    let selection = RunSelection::select::<T>(&collected, end, binding, request.context, ancestors);
    let members = selection.members::<T>(&collected.members, binding, ancestors, request)?;
    if !members.is_empty() {
        let canonical = |node: Node<'s>| request.context.node(node.as_tree_sitter());
        (request.sink)(MarkerRun {
            members,
            target: selection.target.map(canonical).transpose()?,
            target_uncertainty: selection
                .target
                .and_then(|node| T::source_doc_uncertainty(&node, code, ancestors)),
            containing: ancestors.parent(comment).map(canonical).transpose()?,
            code_before: code_before.map(canonical).transpose()?,
            prefix_gap: collected.prefix_gap,
        });
    }
    Ok(())
}

fn member<'s>(
    context: &SourceContext<'s>,
    node: &Node<'s>,
    selection: MarkerSelection,
    placement: MarkerPlacement<'s>,
) -> Result<MarkerMember<'s>, SourceError> {
    evidence_built::record();
    let bytes = doc_comment::node_bytes(node, context.bytes());
    let end =
        if bytes.starts_with(b"//") && context.covers(node.start_byte()..node.start_byte() + 2) {
            line_spelling_end(context, node)
        } else {
            node.end_byte()
        };
    Ok(MarkerMember {
        node: context.node(node.as_tree_sitter())?,
        source: context.pieces(node.start_byte()..end)?,
        selection,
        placement,
        uncertainty: None,
    })
}

pub(super) fn line_spelling_end(context: &SourceContext<'_>, node: &Node<'_>) -> usize {
    let code = context.bytes();
    let end = node.end_byte();
    let lf = if code.get(end - 1) == Some(&b'\n') {
        end - 1
    } else {
        end
    };
    if code.get(lf) == Some(&b'\n') && context.covers(lf..lf + 1) {
        if code.get(lf - 1) == Some(&b'\r') && context.covers(lf - 1..lf + 1) {
            return lf - 1;
        }
        return lf;
    }
    // Some grammars exclude CR from the node even when no included LF follows it.
    if code.get(end) == Some(&b'\r')
        && context.covers(end..end + 1)
        && !(code.get(end + 1) == Some(&b'\n') && context.covers(end + 1..end + 2))
    {
        return end + 1;
    }
    end
}

#[cfg(test)]
mod regressions;
#[cfg(test)]
mod tests;
