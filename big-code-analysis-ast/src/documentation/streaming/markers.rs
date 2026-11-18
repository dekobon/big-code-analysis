//! Native marker selection over borrowed traversal context. These are binding
//! candidates for owner adapters, not a declaration index or resolved owners.

use crate::lang_helpers::doc_comment::{self, DocBinding, StretchEnd};
use crate::node::WordSlot;
use crate::{Ancestors, Checker, Node};

use super::super::{NodeEvidence, SourceContext, SourceError, SourcePieces};

// Separate slots are necessary even on one visited node: Java's empty marker
// answers differently before the same Last selector runs.
#[derive(Clone, Copy)]
pub(crate) enum CandidateView {
    Legacy,
    Source,
}

impl CandidateView {
    pub(crate) fn marker<T: Checker>(self, node: &Node, code: &[u8], ancestors: Ancestors) -> bool {
        let slot = match self {
            Self::Legacy => WordSlot::LegacyDocMarker,
            Self::Source => WordSlot::SourceDocMarker,
        };
        ancestors.classified(slot, node, || {
            marker_reads::record();
            u8::from(match self {
                Self::Legacy => T::is_doc_comment(node, code),
                Self::Source => T::is_source_doc_comment(node, code),
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
            Self::Source => T::is_source_doc_target(node, code, ancestors),
        }
    }
}

pub(crate) fn legacy_marker<T: Checker>(node: &Node, code: &[u8], ancestors: Ancestors) -> bool {
    CandidateView::Legacy.marker::<T>(node, code, ancestors)
}

// The result cannot reveal extra marker reads or evidence built then discarded.
crate::observation::counter!(marker_reads, cfg(any(test, feature = "test-support")));
crate::observation::counter!(evidence_built, cfg(any(test, feature = "test-support")));
pub use doc_comment::stretches_collected;

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
    /// Recognized occurrences in source order, without repeated gatherers.
    pub members: Vec<MarkerMember<'s>>,
    /// Following target wrapper, possibly a conditional or recovery node.
    pub target: Option<NodeEvidence<'s>>,
    /// Conditional/recovery evidence, not a set of simultaneously resolved owners.
    pub target_uncertainty: Option<super::super::UnresolvedReason>,
    /// Immediate containing syntax; owner adapters refine this context.
    pub containing: Option<NodeEvidence<'s>>,
    /// Preceding code retained for after-code placement decisions.
    pub code_before: Option<NodeEvidence<'s>>,
}

/// One physical source marker and its selection result.
#[derive(Debug)]
pub struct MarkerMember<'s> {
    /// Exact syntax node, including any grammar-owned newline.
    pub node: NodeEvidence<'s>,
    /// Included spelling, excluding a line comment's LF/CRLF terminator.
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
}

impl<'t> RunSelection<'t> {
    fn select<T: Checker>(
        stretch: &[Node<'t>],
        code_before: Option<Node<'t>>,
        end: StretchEnd<'t>,
        binding: DocBinding,
        input: (&[u8], Ancestors<'t, '_>),
    ) -> Self {
        let (code, ancestors) = input;
        let marker = |n: &Node<'t>| CandidateView::Source.marker::<T>(n, code, ancestors);
        let forward = |n: &Node<'t>| marker(n) && (binding.forward)(n, code, ancestors);
        if let StretchEnd::AtTarget { next, target } = end {
            return Self {
                kept: doc_comment::select_bound::<T>(
                    stretch,
                    code,
                    binding.kept,
                    (code_before, next, target),
                    forward,
                ),
                target: Some(target),
                gatherer: None,
            };
        }
        let result = binding.gathers.map(|gathers| {
            let gatherer = stretch
                .iter()
                .rfind(|n| marker(n) && gathers(n, code))
                .copied();
            (
                doc_comment::bind_gathered(stretch, code, gathers, marker, forward),
                gatherer,
            )
        });
        let (kept, gatherer) = result.unwrap_or_default();
        Self {
            kept,
            target: None,
            gatherer,
        }
    }

    fn members<T: Checker>(
        &self,
        stretch: &[Node<'t>],
        binding: DocBinding,
        ancestors: Ancestors<'t, '_>,
        request: &SourceRequest<'_, 't>,
    ) -> Result<Vec<MarkerMember<'t>>, SourceError> {
        // bca: suppress(halstead) -- one ordered merge pairs selected nodes with retained source evidence.
        let code = request.context.bytes();
        let mut selected = self.kept.iter().peekable();
        let mut members = Vec::new();
        for node in stretch
            .iter()
            .filter(|n| CandidateView::Source.marker::<T>(n, code, ancestors))
        {
            let kept = selected.peek().is_some_and(|n| n.id() == node.id());
            if kept {
                selected.next();
            }
            let forward = (binding.forward)(node, code, ancestors);
            let selection = if kept || !forward {
                MarkerSelection::Selected
            } else if self.target.is_some() && !self.kept.is_empty() {
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
            members.push(member);
        }
        Ok(members)
    }
}

/// Structural placement, deliberately not a semantic owner claim. Local macro
/// tokens and conditional targets retain uncertainty until an adapter resolves it.
#[derive(Clone, Copy, Debug)]
pub enum MarkerPlacement<'s> {
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
/// boundaries and selection algorithms as legacy binding, with separate
/// pre-selection predicates. A deferred member never collects its prefix.
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
    let view = CandidateView::Source;
    let end = doc_comment::stretch_end::<T>(comment, code, ancestors, binding.skips, view);
    if matches!(end, StretchEnd::Undecided) {
        return Ok(());
    }
    if matches!(end, StretchEnd::Elsewhere)
        && !request.retain_unbound
        && !T::SOURCE_LOCAL_DOCS
        && binding.gathers.is_none()
    {
        return Ok(());
    }
    let (stretch, code_before) =
        doc_comment::collect_stretch::<T>(comment, code, ancestors, binding.skips, view);
    let selection =
        RunSelection::select::<T>(&stretch, code_before, end, binding, (code, ancestors));
    let members = selection.members::<T>(&stretch, binding, ancestors, request)?;
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
    let end = if bytes.starts_with(b"//") {
        let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        node.start_byte() + line.strip_suffix(b"\r").unwrap_or(line).len()
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

#[cfg(test)]
mod tests;
