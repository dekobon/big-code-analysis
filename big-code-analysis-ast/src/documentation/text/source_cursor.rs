//! Monotone clipping within one fragment's already-retained intersections.

use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{SourcePiece, SourcePieces, UnavailableReason};
use std::ops::Range;

pub(super) struct SourceCursor<'f, 's> {
    pieces: &'f [SourcePiece<'s>],
    at: usize,
}

impl<'f, 's> SourceCursor<'f, 's> {
    pub(super) fn new(source: &'f SourcePieces<'s>) -> Self {
        Self {
            pieces: source.pieces(),
            at: 0,
        }
    }

    pub(super) fn pieces<B: WorkBudget>(
        &mut self,
        range: Range<usize>,
        budget: &B,
    ) -> Result<SourcePieces<'s>, TextError> {
        let mut pieces = Vec::new();
        let mut covered = range.start;
        while let Some(piece) = self.pieces.get(self.at) {
            budget
                .step()
                .map_err(|error| TextError::Source(error.into()))?;
            fragment_piece_visits::record();
            let span = piece.range();
            if span.start >= range.end {
                break;
            }
            if span.end > range.start {
                let start = span.start.max(range.start);
                if start != covered {
                    return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
                }
                covered = span.end.min(range.end);
                pieces.push(piece.slice(start..covered));
            }
            if span.end > range.end {
                break;
            }
            self.at += 1;
        }
        if covered != range.end {
            return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
        }
        Ok(SourcePieces::from_included_pieces(pieces))
    }

    pub(super) fn copy<B: WorkBudget>(
        &mut self,
        builder: &mut TextBuilder<'s>,
        range: Range<usize>,
        budget: &B,
    ) -> Result<(), TextError> {
        if !range.is_empty() {
            builder.copied(&self.pieces(range, budget)?, budget)?;
        }
        Ok(())
    }
}

crate::observation::counter!(
    fragment_piece_visits,
    cfg(any(test, feature = "test-support"))
);
