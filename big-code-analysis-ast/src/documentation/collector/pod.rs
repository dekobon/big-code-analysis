//! Perl POD blocks before the data section. A block that perl, POD
//! processors and the grammar read alike documents the file.
//!
//! The free functions read the source and decide. The `Builder` methods
//! record what was decided.

use std::ops::Range;

use super::*;
use crate::documentation::budget::{WorkPhase, in_phase};
use crate::documentation::byte_scan as scan;
use crate::lang_helpers::doc_text::pod_comment_region_with_budget;
use crate::lang_helpers::perl::opens_pod_with_budget;

const EXCLUDED: &str = "Excluded bytes prevent reading this POD block";
const OPENER: &str = "POD block does not open with a POD command at the start of a line";
const CLOSER: &str = "POD block does not end where perl and POD processors both end it";
const LINE_END: &str = "POD block holds a carriage return that no line feed follows";
const DATA_SECTION: &str = "Perl data section is not read for POD blocks";
const MIXED: &str = "POD block mixes documentation with comment regions";
const HIDDEN: &str = "POD may stand after a syntax error where the grammar reads no POD block";

/// POD that the grammar may hide behind a syntax error: the first recovery
/// node of the tree, and where the bytes behind it that no block holds are
/// still to be searched for a line that starts like a POD command.
#[derive(Debug, Default)]
pub(super) struct Hidden {
    recovery: Option<NodeId>,
    unread: Option<usize>,
}

/// What one `pod_statement` node is to the source view.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PodBlock {
    /// Not read, for the reason given.
    Withheld(&'static str),
    /// Nothing but `comment` regions. The spelling ends at this byte.
    CommentOnly(usize),
    /// Documentation of the file. The spelling ends at this byte.
    Documents(usize),
}

pub(super) fn pod_block<B: WorkBudget>(
    source: &SourceContext<'_>,
    node: tree_sitter::Node<'_>,
    budget: &B,
) -> Result<PodBlock, B::Error> {
    let code = source.bytes();
    let (start, end) = (node.start_byte(), node.end_byte());
    // The byte before the block and the byte after it are read as well.
    let read = start.saturating_sub(1)..code.len().min(end.saturating_add(1));
    if !source.covers_with_budget(read, budget)? {
        return Ok(PodBlock::Withheld(EXCLUDED));
    }
    let spelling = start..spelling_end(code, start, end);
    let reason = match outside(code, start, end) {
        None => inside(code, &spelling, budget)?,
        reason => reason,
    };
    Ok(if let Some(reason) = reason {
        PodBlock::Withheld(reason)
    } else if pod_comment_region_with_budget(&code[start..end], budget)? {
        PodBlock::CommentOnly(spelling.end)
    } else {
        PodBlock::Documents(spelling.end)
    })
}

/// The spelling ends before the line end of the block's last line. Only a
/// block that no `=cut` line ends holds that line end itself.
fn spelling_end(code: &[u8], start: usize, end: usize) -> usize {
    let block = &code[start..end];
    let mut length = block.len();
    if block.last() == Some(&b'\n') {
        length -= 1;
    }
    if code.get(start + length) == Some(&b'\n') && block[..length].last() == Some(&b'\r') {
        length -= 1;
    }
    start + length
}

/// perl opens POD only at the start of a line, and ends a line only at a
/// line feed. The grammar opens a block anywhere and cuts one at a NUL byte.
fn outside(code: &[u8], start: usize, end: usize) -> Option<&'static str> {
    if start > 0 && code[start - 1] != b'\n' {
        Some(OPENER)
    } else if code.get(end).is_some_and(|&next| next != b'\n') {
        Some(CLOSER)
    } else {
        None
    }
}

/// The lines of a block, where its three readers differ. `Pod::Simple`
/// wants a command word and no `=cut` to open a block. perl does not end
/// one at `=cut` before a letter. POD processors end a line at any
/// carriage return, perl only at a line feed.
fn inside<B: WorkBudget>(
    code: &[u8],
    spelling: &Range<usize>,
    budget: &B,
) -> Result<Option<&'static str>, B::Error> {
    let block = &code[spelling.clone()];
    if !opens_pod_with_budget(block, budget)? || block.starts_with(b"=cut") {
        return Ok(Some(OPENER));
    }
    let mut last_line = 0;
    let mut lone_return = false;
    for (at, &byte) in block.iter().enumerate() {
        budget.bytes(1)?;
        match byte {
            b'\n' => last_line = at + 1,
            b'\r' => lone_return |= code.get(spelling.start + at + 1) != Some(&b'\n'),
            _ => {}
        }
    }
    let closer = &block[last_line..];
    let unread = last_line > 0
        && closer.starts_with(b"=cut")
        && closer.get(4).is_some_and(u8::is_ascii_alphabetic);
    Ok(if unread {
        Some(CLOSER)
    } else if lone_return {
        Some(LINE_END)
    } else {
        None
    })
}

/// Whether a line that starts with `=` and an ASCII letter starts in `span`.
/// With an excluded byte in the span or directly before it, nothing is read
/// and the worst case is taken.
pub(super) fn command_line<B: WorkBudget>(
    source: &SourceContext<'_>,
    span: Range<usize>,
    budget: &B,
) -> Result<bool, B::Error> {
    if span.start >= span.end {
        return Ok(false);
    }
    let code = source.bytes();
    if !source.covers_with_budget(span.start.saturating_sub(1)..span.end, budget)? {
        return Ok(true);
    }
    let mut first = span.start == 0 || code[span.start - 1] == b'\n';
    let (mut command, mut found) = (false, false);
    for &byte in &code[span] {
        scan::one(budget)?;
        found = command && byte.is_ascii_alphabetic();
        if found {
            break;
        }
        command = first && byte == b'=';
        first = byte == b'\n';
    }
    Ok(found)
}

impl<'s> Builder<'s> {
    pub(super) fn pod_node(
        &mut self,
        id: NodeId,
        node: &Node<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let raw = node.as_tree_sitter();
        self.pod_doubt.enter(&self.index.source, raw, budget)?;
        match node.kind() {
            "pod_statement" => {
                self.pod_unread(raw.byte_range(), budget)?;
                self.pod_statement(id, budget)
            }
            "data_not_for_compiler" => {
                self.pod_unread(raw.start_byte()..self.index.source.bytes().len(), budget)?;
                let leaf = Some(self.index.canonical.nodes[id.0].evidence);
                self.pod_pending(leaf, AssessmentAxis::Occurrence, DATA_SECTION, None);
                Ok(())
            }
            _ => {
                if self.pod_hidden.recovery.is_none() && (raw.is_error() || raw.is_missing()) {
                    self.pod_hidden = Hidden {
                        recovery: Some(id),
                        unread: Some(raw.start_byte()),
                    };
                }
                Ok(())
            }
        }
    }

    /// The end of the walk: the rest of the input is searched.
    pub(super) fn pod_end(&mut self, budget: &CollectionBudget) -> Result<(), CollectionError> {
        let end = self.index.source.bytes().len();
        in_phase(budget, WorkPhase::Classification, || {
            self.pod_unread(end..end, budget)
        })
    }

    /// Searches the bytes between the first recovery node and `span` that
    /// no block before held, and goes on behind `span` the next time. At the
    /// first line that starts like a POD command the file gets one
    /// assessment on that node, and the search ends.
    fn pod_unread(
        &mut self,
        span: Range<usize>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let Some(from) = self.pod_hidden.unread else {
            return Ok(());
        };
        self.pod_hidden.unread = Some(from.max(span.end));
        if command_line(&self.index.source, from..span.start, budget)? {
            let recovery = self.pod_hidden.recovery;
            let node = recovery.map(|id| self.index.canonical.nodes[id.0].evidence);
            self.pod_pending(node, AssessmentAxis::Occurrence, HIDDEN, None);
            self.pod_hidden.unread = None;
        }
        Ok(())
    }

    fn pod_statement(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let block = self.index.canonical.nodes[id.0].evidence;
        let read = match pod_block(&self.index.source, block.node(), budget)? {
            PodBlock::Withheld(reason) => PodBlock::Withheld(reason),
            read => self
                .pod_doubt
                .reason(block.node().start_byte())
                .map_or(read, PodBlock::Withheld),
        };
        match read {
            PodBlock::Withheld(reason) => {
                self.pod_pending(Some(block), AssessmentAxis::Selection, reason, None);
            }
            PodBlock::CommentOnly(end) if self.index.options.retain_directives => {
                let fragment = self.pod_fragment(block, end, budget)?;
                self.index.evidence.push(CandidateEvidence {
                    fragment,
                    disposition: CandidateDisposition::Directive,
                });
            }
            PodBlock::CommentOnly(_) => {}
            PodBlock::Documents(end) => {
                let fragment = self.pod_fragment(block, end, budget)?;
                self.drafts.push(Draft {
                    fragments: vec![fragment],
                    target: Some(NodeId(0)),
                    scope: self.index.canonical.nodes[id.0].parent.unwrap_or(NodeId(0)),
                    binding: None,
                    role: DocumentRole::FilePackage,
                    unresolved: None,
                    pending: false,
                    gap: false,
                });
            }
        }
        Ok(())
    }

    fn pod_fragment(
        &mut self,
        block: NodeEvidence<'s>,
        end: usize,
        budget: &CollectionBudget,
    ) -> Result<FragmentId, CollectionError> {
        let spelling = block.node().start_byte()..end;
        let source = self.index.source.pieces_with_budget(spelling, budget)?;
        Ok(self.fragment(source, block, FragmentForm::Pod))
    }

    /// Pending POD work is filed with the file, which every POD document
    /// belongs to: owner 0.
    fn pod_pending(
        &mut self,
        node: Option<NodeEvidence<'s>>,
        axis: AssessmentAxis,
        reason: &'static str,
        document: Option<DocumentId>,
    ) {
        self.index.assessments_by_owner[0].push(self.index.assessments.len());
        self.index.assessments.push(Assessment {
            axis,
            reason,
            node,
            document,
        });
    }

    /// A block whose text waits for a join rule is pending work on its
    /// document, not a document without text.
    pub(super) fn pod_pending_text(&mut self, document: DocumentId) {
        let composed = &self.index.facts.documents[document.0];
        if self.index.profile == Profile::Pod && matches!(composed.content, Content::PendingAdapter)
        {
            let block = self.index.facts.fragments[composed.fragments[0].0].node;
            self.pod_pending(block, AssessmentAxis::Composition, MIXED, Some(document));
        }
    }
}
