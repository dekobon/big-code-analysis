//! Local content states and forward-map coordinates, without a renderer.

use super::{SourcePiece, SourcePieces};
use std::ops::Range;

/// Document-local UTF-8 byte coordinates, distinct from source coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRange(Range<usize>);

impl TextRange {
    pub(super) fn validated(range: Range<usize>) -> Self {
        Self(range)
    }
    /// Validate a range against its output string. Empty ranges, including EOF,
    /// are valid; endpoints inside a scalar are not.
    ///
    /// # Errors
    /// Rejects reversed/out-of-bounds ranges or split UTF-8 scalars.
    pub fn new(text: &str, range: Range<usize>) -> Result<Self, TextRangeError> {
        if range.start > range.end || range.end > text.len() {
            return Err(TextRangeError::Bounds);
        }
        if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
            return Err(TextRangeError::ScalarBoundary);
        }
        Ok(Self(range))
    }

    /// Half-open output byte coordinates.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.0.clone()
    }
}

/// Invalid output coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextRangeError {
    /// Reversed or out-of-bounds range.
    Bounds,
    /// Endpoint inside a UTF-8 scalar.
    ScalarBoundary,
}

/// The source responsible for one output run. Disjoint included pieces stay
/// disjoint, including when a decoded token crosses an excluded range.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Origin<'s> {
    /// A byte-for-byte copy; a subrange query clips its source correspondingly.
    Copied(SourcePieces<'s>),
    /// A decoded escape/token; any intersecting scalar returns the whole token.
    Decoded(SourcePieces<'s>),
    /// A token expanded to more bytes, such as indentation tabs.
    Expanded(SourcePieces<'s>),
    /// Inserted output, with no responsible source bytes.
    Synthetic,
}

/// One ordered output run and its origin. A complete map partitions the output;
/// validation and range queries are the text builder's responsibility.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Mapping<'s> {
    /// Document-local output coordinates.
    pub output: TextRange,
    /// The source transformation for this run.
    pub origin: Origin<'s>,
}

/// Local prose state. Missing documentation is absence of a document, not a
/// variant here. Raw bytes remain on the document's physical fragments.
#[derive(Debug)]
#[non_exhaustive]
pub enum Content<'s> {
    /// Complete decoded/cleaned local text; `""` is present-empty prose.
    #[non_exhaustive]
    Complete {
        /// UTF-8 local markup, without generator expansion.
        text: String,
        /// Ordered forward origins; empty for empty text.
        mappings: Vec<Mapping<'s>>,
    },
    /// Explicit `false` or equivalent source suppression.
    Disabled,
    /// Explicit nil/absence value, retained as a source occurrence.
    ExplicitAbsence,
    /// Documentation metadata with no prose value.
    MetadataOnly,
    /// A complete value cannot be provided; raw fragments are retained.
    Unavailable(UnavailableReason),
    /// An adapter has not assessed every contributing occurrence or composition
    /// rule. Consult the index's assessment records for the pending form.
    PendingAdapter,
}

/// Borrowed, builder-validated local text and forward mappings from one index.
#[derive(Clone, Copy, Debug)]
pub struct CompleteText<'i, 's> {
    pub(super) text: &'i str,
    pub(super) mappings: &'i [Mapping<'s>],
}

impl<'i, 's> CompleteText<'i, 's> {
    /// Retained UTF-8 local markup, without reconstruction.
    #[must_use]
    pub fn text(self) -> &'i str {
        self.text
    }

    /// All retained mapping runs, without validation or reconstruction.
    #[must_use]
    pub fn mappings(self) -> &'i [Mapping<'s>] {
        self.mappings
    }

    /// Query exact output coordinates in `O(log(S + 1) + K)` work without
    /// allocating, where S is the run count and K the returned origins.
    /// Valid empty queries, including EOF, take constant time.
    ///
    /// # Errors
    /// Rejects reversed/out-of-bounds coordinates and split UTF-8 scalars.
    pub fn origins(self, range: Range<usize>) -> Result<MappingPortions<'i, 's>, TextRangeError> {
        let range = TextRange::new(self.text, range)?.range();
        let first = if range.is_empty() {
            self.mappings.len()
        } else {
            self.mappings.partition_point(|mapping| {
                map_searches::record();
                mapping.output.0.end <= range.start
            })
        };
        Ok(MappingPortions {
            remaining: self.mappings[first..].iter(),
            range,
        })
    }
}

// Late tiny queries must not walk every preceding run or clone token pieces.
crate::observation::counter!(map_searches, cfg(any(test, feature = "test-support")));
crate::observation::counter!(map_runs_visited, cfg(any(test, feature = "test-support")));

/// An allocation-free forward-origin view. Copied origins are clipped exactly;
/// transformed origins borrow all responsible token pieces.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OriginView<'i, 's> {
    /// A contiguous source slice clipped to the output portion.
    Copied(SourcePiece<'s>),
    /// All included pieces of a decoded token.
    Decoded(&'i [SourcePiece<'s>]),
    /// All included pieces of an expanded token.
    Expanded(&'i [SourcePiece<'s>]),
    /// Inserted output without source bytes.
    Synthetic,
}

/// One intersecting output portion and its exact origin.
#[derive(Debug, PartialEq, Eq)]
pub struct MappingPortion<'i, 's> {
    /// Half-open UTF-8 byte coordinates in the complete text.
    pub output: Range<usize>,
    /// The responsible source bytes or synthetic origin.
    pub origin: OriginView<'i, 's>,
}

/// Lazy, allocation-free portions of a validated mapping query.
#[derive(Debug)]
pub struct MappingPortions<'i, 's> {
    remaining: std::slice::Iter<'i, Mapping<'s>>,
    range: Range<usize>,
}

impl<'i, 's> Iterator for MappingPortions<'i, 's> {
    type Item = MappingPortion<'i, 's>;

    fn next(&mut self) -> Option<Self::Item> {
        let mapping = self.remaining.next()?;
        if mapping.output.0.start >= self.range.end {
            self.remaining = [].iter();
            return None;
        }
        map_runs_visited::record();
        let output =
            self.range.start.max(mapping.output.0.start)..self.range.end.min(mapping.output.0.end);
        let origin = match &mapping.origin {
            Origin::Copied(pieces) => {
                let piece = &pieces.pieces()[0];
                let start = piece.range().start + output.start - mapping.output.0.start;
                OriginView::Copied(piece.slice(start..start + output.len()))
            }
            Origin::Decoded(pieces) => OriginView::Decoded(pieces.pieces()),
            Origin::Expanded(pieces) => OriginView::Expanded(pieces.pieces()),
            Origin::Synthetic => OriginView::Synthetic,
        };
        Some(MappingPortion { output, origin })
    }
}

impl std::iter::FusedIterator for MappingPortions<'_, '_> {}

/// Why complete local text cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnavailableReason {
    /// Dynamic interpolation, substitution or computed expression.
    Dynamic,
    /// External or macro-expanded content would require evaluation.
    External,
    /// Unsupported escape or semantic prelexing.
    UnsupportedDecoding,
    /// Invalid source encoding or escape spelling.
    InvalidEncoding,
    /// Decoding produces a value outside Unicode scalar values.
    NonScalar,
    /// Required content crosses excluded source bytes.
    ExcludedGap,
    /// Recovery or a known grammar limitation obscures content boundaries.
    GrammarBoundary,
}
