//! Included raw source pieces and safe canonical-node evidence.

use std::ops::Range;

// Output alone cannot reveal scanning empty ranges or seeking from the start.
crate::observation::counter!(
    included_range_visits,
    cfg(any(test, feature = "test-support"))
);

/// A nonempty, half-open source byte range and its borrowed raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePiece<'s> {
    range: Range<usize>,
    bytes: &'s [u8],
}

impl<'s> SourcePiece<'s> {
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
        let mut included = tree.included_ranges();
        included.retain(|range| range.start_byte < range.end_byte);
        Self {
            tree,
            bytes,
            included,
        }
    }

    /// Intersect a nonempty spelling in `O(log R + I)` work for R included
    /// ranges and I returned pieces. No excluded bytes are copied or returned.
    ///
    /// # Errors
    /// Rejects empty, reversed, out-of-buffer and wholly excluded spellings.
    pub fn pieces(&self, spelling: Range<usize>) -> Result<SourcePieces<'s>, SourceError> {
        // bca: suppress(halstead) -- one interval-intersection loop retains raw slices and excluded-gap evidence together.
        if spelling.start >= spelling.end || spelling.end > self.bytes.len() {
            return Err(SourceError::InvalidRange);
        }
        let first = self
            .included
            .partition_point(|range| range.end_byte <= spelling.start);
        let mut pieces = Vec::new();
        let mut included_bytes = 0;
        for range in &self.included[first..] {
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
    /// Empty, reversed or out-of-buffer spelling.
    InvalidRange,
    /// No requested bytes were included in the parse.
    Excluded,
    /// A node belongs to another tree handle.
    ForeignTree,
    /// The held tree matches but canonical node context does not.
    NoncanonicalNode,
}
