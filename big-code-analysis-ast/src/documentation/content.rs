//! Local content states and forward-map coordinates, without a renderer.

use super::SourcePieces;
use std::ops::Range;

/// Document-local UTF-8 byte coordinates, distinct from source coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRange(Range<usize>);

impl TextRange {
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
}

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
