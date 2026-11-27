//! Included raw source pieces and safe canonical-node evidence.

use std::ops::Range;

use super::budget::{NoBudget, WorkBudget, WorkPhase, in_phase, infallible};

// Output alone cannot reveal scanning empty ranges or seeking from the start.
crate::observation::counter!(
    included_range_visits,
    cfg(any(test, feature = "test-support"))
);

// Responsible-piece cloning is work that retained-map equality cannot reveal.
crate::observation::counter!(piece_copies, cfg(any(test, feature = "test-support")));

/// A nonempty, half-open source byte range and its borrowed raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePiece<'s> {
    range: Range<usize>,
    bytes: &'s [u8],
}

impl<'s> SourcePiece<'s> {
    pub(super) fn slice(&self, range: Range<usize>) -> Self {
        Self {
            bytes: &self.bytes[range.start - self.range.start..range.end - self.range.start],
            range,
        }
    }
    /// Half-open byte coordinates in the actual parsed buffer.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Raw bytes, without UTF-8 conversion or replacement characters.
    #[must_use]
    pub fn bytes(&self) -> &'s [u8] {
        self.bytes
    }
}

/// Ordered intersections of one spelling with the tree's included ranges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePieces<'s> {
    pieces: Vec<SourcePiece<'s>>,
    has_excluded_gap: bool,
}

impl<'s> SourcePieces<'s> {
    pub(in crate::documentation) fn from_included_pieces(pieces: Vec<SourcePiece<'s>>) -> Self {
        Self {
            pieces,
            has_excluded_gap: false,
        }
    }
    pub(in crate::documentation) fn copy_with_budget<B: WorkBudget>(
        &self,
        budget: &B,
    ) -> Result<Self, B::Error> {
        let mut pieces = Vec::new();
        for piece in &self.pieces {
            budget.step()?;
            pieces.push(piece.clone());
            piece_copies::record();
        }
        Ok(Self {
            pieces,
            has_excluded_gap: self.has_excluded_gap,
        })
    }
    pub(super) fn single(piece: SourcePiece<'s>) -> Self {
        Self {
            pieces: vec![piece],
            has_excluded_gap: false,
        }
    }
    /// The individual pieces; intervening excluded bytes are never returned.
    #[must_use]
    pub fn pieces(&self) -> &[SourcePiece<'s>] {
        &self.pieces
    }

    /// Whether any of the requested spelling was excluded from parsing.
    #[must_use]
    pub fn has_excluded_gap(&self) -> bool {
        self.has_excluded_gap
    }
}

/// Borrowed parse input with included ranges prepared once for binary search.
/// The caller must supply the exact buffer used to produce the tree.
#[derive(Debug)]
pub struct SourceContext<'s> {
    tree: &'s tree_sitter::Tree,
    bytes: &'s [u8],
    included: Vec<tree_sitter::Range>,
    continuous: Vec<Range<usize>>,
}

impl<'s> SourceContext<'s> {
    /// The actual parsed bytes borrowed by this request.
    #[must_use]
    pub fn bytes(&self) -> &'s [u8] {
        self.bytes
    }

    /// Bind the actual parsed source to its tree. This does not reparse or
    /// infer completion; adopted/raw inputs retain the caller's precondition.
    #[must_use]
    pub fn new(tree: &'s tree_sitter::Tree, bytes: &'s [u8]) -> Self {
        infallible(Self::with_budget(tree, bytes, &NoBudget))
    }

    pub(crate) fn with_budget<B: WorkBudget>(
        tree: &'s tree_sitter::Tree,
        bytes: &'s [u8],
        budget: &B,
    ) -> Result<Self, B::Error> {
        in_phase(budget, WorkPhase::Ranges, || {
            Self::prepare(tree, bytes, budget)
        })
    }

    fn prepare<B: WorkBudget>(
        tree: &'s tree_sitter::Tree,
        bytes: &'s [u8],
        budget: &B,
    ) -> Result<Self, B::Error> {
        budget.checkpoint()?;
        // The safe dependency copies the range array atomically. Only our
        // subsequent normalization loop can be interrupted internally.
        let raw = tree.included_ranges();
        budget.checkpoint()?;
        let mut included = Vec::new();
        let mut continuous: Vec<Range<usize>> = Vec::new();
        for range in raw {
            budget.step()?;
            let start = range.start_byte.min(bytes.len());
            let end = range.end_byte.min(bytes.len());
            if start == end {
                continue;
            }
            included.push(range);
            if let Some(last) = continuous.last_mut()
                && last.end == start
            {
                last.end = end;
            } else {
                continuous.push(start..end);
            }
        }
        Ok(Self {
            tree,
            bytes,
            included,
            continuous,
        })
    }

    /// Intersect a nonempty spelling in `O(log R + I)` work for R included
    /// ranges and I returned pieces. No excluded bytes are copied or returned.
    ///
    /// # Errors
    /// Rejects empty, reversed, out-of-buffer and wholly excluded spellings.
    pub fn pieces(&self, spelling: Range<usize>) -> Result<SourcePieces<'s>, SourceError> {
        self.pieces_with_budget(spelling, &NoBudget)
    }

    pub(crate) fn pieces_with_budget<B: WorkBudget>(
        &self,
        spelling: Range<usize>,
        budget: &B,
    ) -> Result<SourcePieces<'s>, SourceError> {
        in_phase(budget, WorkPhase::Ranges, || {
            self.intersect(spelling, budget)
        })
    }

    fn intersect<B: WorkBudget>(
        &self,
        spelling: Range<usize>,
        budget: &B,
    ) -> Result<SourcePieces<'s>, SourceError> {
        // bca: suppress(halstead) -- one interval-intersection loop retains raw slices and excluded-gap evidence together.
        if spelling.start >= spelling.end || spelling.end > self.bytes.len() {
            return Err(SourceError::InvalidRange);
        }
        let mut first = 0;
        let mut end = self.included.len();
        while first < end {
            budget.step().map_err(Into::into)?;
            intersection_searches::record();
            let middle = first + (end - first) / 2;
            if self.included[middle].end_byte <= spelling.start {
                first = middle + 1;
            } else {
                end = middle;
            }
        }
        let mut pieces = Vec::new();
        let mut included_bytes = 0;
        for range in &self.included[first..] {
            budget.step().map_err(Into::into)?;
            included_range_visits::record();
            if range.start_byte >= spelling.end {
                break;
            }
            let start = spelling.start.max(range.start_byte);
            let end = spelling.end.min(range.end_byte);
            if start < end {
                pieces.push(SourcePiece {
                    range: start..end,
                    bytes: &self.bytes[start..end],
                });
                included_bytes += end - start;
            }
        }
        if pieces.is_empty() {
            return Err(SourceError::Excluded);
        }
        Ok(SourcePieces {
            pieces,
            has_excluded_gap: included_bytes != spelling.end - spelling.start,
        })
    }

    /// Whether the whole interval is available, without joining across a gap.
    #[cfg(test)]
    pub(crate) fn covers(&self, span: Range<usize>) -> bool {
        infallible(self.covers_with_budget(span, &NoBudget))
    }

    pub(crate) fn covers_with_budget<B: WorkBudget>(
        &self,
        span: Range<usize>,
        budget: &B,
    ) -> Result<bool, B::Error> {
        if span.start > span.end || span.end > self.bytes.len() {
            return Ok(false);
        }
        if span.is_empty() {
            return Ok(true);
        }
        let first = super::budget::binary_search_end(
            self.continuous.len(),
            |at| self.continuous[at].end <= span.start,
            budget,
        )?;
        included_range_visits::record();
        Ok(self
            .continuous
            .get(first)
            .is_some_and(|range| range.start <= span.start && range.end >= span.end))
    }

    /// Record a node reached canonically by a collector cursor. The caller
    /// supplies canonical traversal context; foreign trees are rejected here.
    /// Later validation compares against this retained value without parent walks.
    ///
    /// # Errors
    /// Rejects a node from a different tree, including a cloned tree.
    pub fn node(&self, canonical: tree_sitter::Node<'s>) -> Result<NodeEvidence<'s>, SourceError> {
        if canonical.into_raw().tree != self.tree.root_node().into_raw().tree {
            return Err(SourceError::ForeignTree);
        }
        Ok(NodeEvidence { canonical })
    }
}

crate::observation::counter!(
    intersection_searches,
    cfg(any(test, feature = "test-support"))
);

#[cfg(all(test, feature = "rust"))]
mod coverage_tests {
    use super::*;

    #[test]
    fn coverage_seeks_to_late_ranges_and_stops_at_the_first_gap() {
        let source = b"a ".repeat(256);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&crate::LANG::Rust.tree_sitter_language().unwrap())
            .unwrap();
        let ranges: Vec<_> = (0..256)
            .map(|i| tree_sitter::Range {
                start_byte: i * 2,
                end_byte: i * 2 + 1,
                start_point: tree_sitter::Point::new(0, i * 2),
                end_point: tree_sitter::Point::new(0, i * 2 + 1),
            })
            .collect();
        parser.set_included_ranges(&ranges).unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let context = SourceContext::new(&tree, &source);
        let before = included_range_visits::observed();
        assert!(context.covers(510..511));
        // expected: the last range only; earlier ranges are binary-searched past.
        assert_eq!(included_range_visits::observed() - before, 1);
        let before = included_range_visits::observed();
        assert!(!context.covers(0..511));
        // expected: the first continuous interval cannot contain the whole request.
        assert_eq!(included_range_visits::observed() - before, 1);
    }
}

/// A safe borrowed canonical value; raw pointers are only compared transiently.
/// This retains the tree-sitter node's `Send + Sync` guarantees.
#[derive(Clone, Copy, Debug)]
pub struct NodeEvidence<'s> {
    canonical: tree_sitter::Node<'s>,
}

impl<'s> NodeEvidence<'s> {
    /// Exact node recorded during canonical traversal.
    #[must_use]
    pub fn node(self) -> tree_sitter::Node<'s> {
        self.canonical
    }

    /// Validate tree and full node context rather than only ID or byte range.
    ///
    /// # Errors
    /// Distinguishes foreign trees from changed/noncanonical node values.
    pub fn validate(self, node: tree_sitter::Node<'_>) -> Result<(), SourceError> {
        let expected = self.canonical.into_raw();
        let actual = node.into_raw();
        if actual.tree != expected.tree {
            return Err(SourceError::ForeignTree);
        }
        if actual.id != expected.id
            || actual.context != expected.context
            || node.kind_id() != self.canonical.kind_id()
            || node.range() != self.canonical.range()
        {
            return Err(SourceError::NoncanonicalNode);
        }
        Ok(())
    }
}

/// Source evidence failures, distinct from text availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceError {
    /// Active collection cancellation; no successful partial result is returned.
    Cancelled,
    /// Empty, reversed or out-of-buffer spelling.
    InvalidRange,
    /// No requested bytes were included in the parse.
    Excluded,
    /// A node belongs to another tree handle.
    ForeignTree,
    /// The held tree matches but canonical node context does not.
    NoncanonicalNode,
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "documentation source operation cancelled",
            Self::InvalidRange => "documentation source range is invalid",
            Self::Excluded => "documentation source range is excluded",
            Self::ForeignTree => "documentation node belongs to another tree",
            Self::NoncanonicalNode => "documentation node is not canonical",
        })
    }
}

impl std::error::Error for SourceError {}
