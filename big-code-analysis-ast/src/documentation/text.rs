//! Append-only local text with exact origins and bounded UTF-8 validation.

use std::ops::Range;

use super::budget::{BYTE_INTERVAL, WorkBudget};
use super::{
    Content, Mapping, Origin, SourceContext, SourceError, SourcePieces, TextRange,
    UnavailableReason,
};

pub(super) mod comments;

#[derive(Debug)]
pub(super) enum TextError {
    Source(SourceError),
    Unavailable(UnavailableReason),
}

impl From<SourceError> for TextError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

#[derive(Default)]
pub(super) struct TextBuilder<'s> {
    text: String,
    mappings: Vec<Mapping<'s>>,
}

// The same document must not be rendered once per association or retained read.
crate::observation::counter!(renders, cfg(any(test, feature = "test-support")));
crate::observation::counter!(copy_chunks, cfg(any(test, feature = "test-support")));

impl<'s> TextBuilder<'s> {
    pub(super) fn copied<B: WorkBudget>(
        &mut self,
        pieces: &SourcePieces<'s>,
        budget: &B,
    ) -> Result<(), TextError> {
        if pieces.has_excluded_gap() {
            return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
        }
        for piece in pieces.pieces() {
            budget.step().map_err(Into::into)?;
            let start = self.text.len();
            self.append_utf8(piece.bytes(), budget)?;
            self.push_mapping(start, Origin::Copied(SourcePieces::single(piece.clone())));
        }
        Ok(())
    }

    pub(super) fn transformed<B: WorkBudget>(
        &mut self,
        text: &str,
        origin: Origin<'s>,
        budget: &B,
    ) -> Result<(), TextError> {
        let start = self.text.len();
        self.append_utf8(text.as_bytes(), budget)?;
        self.push_mapping(start, origin);
        Ok(())
    }

    pub(super) fn source<B: WorkBudget>(
        &mut self,
        context: &SourceContext<'s>,
        range: Range<usize>,
        budget: &B,
    ) -> Result<(), TextError> {
        if !range.is_empty() {
            self.copied(&context.pieces_with_budget(range, budget)?, budget)?;
        }
        Ok(())
    }

    fn append_utf8<B: WorkBudget>(&mut self, bytes: &[u8], budget: &B) -> Result<(), TextError> {
        // bca: suppress(nexits, halstead) -- validation, scalar carry and copying each require a separate fallible checkpoint.
        let mut at = 0;
        while at < bytes.len() {
            let end = (at + BYTE_INTERVAL).min(bytes.len());
            budget.checkpoint().map_err(Into::into)?;
            budget.bytes(end - at).map_err(Into::into)?;
            let valid = match std::str::from_utf8(&bytes[at..end]) {
                Ok(text) => text,
                Err(error) if error.error_len().is_none() && end < bytes.len() => {
                    budget.checkpoint().map_err(Into::into)?;
                    budget.bytes(error.valid_up_to()).map_err(Into::into)?;
                    std::str::from_utf8(&bytes[at..at + error.valid_up_to()])
                        .map_err(|_| TextError::Unavailable(UnavailableReason::InvalidEncoding))?
                }
                Err(_) => return Err(TextError::Unavailable(UnavailableReason::InvalidEncoding)),
            };
            budget.checkpoint().map_err(Into::into)?;
            budget.bytes(valid.len()).map_err(Into::into)?;
            self.text.push_str(valid);
            copy_chunks::record();
            at += valid.len();
        }
        Ok(())
    }

    fn push_mapping(&mut self, start: usize, origin: Origin<'s>) {
        if start != self.text.len() {
            self.mappings.push(Mapping {
                output: TextRange::validated(start..self.text.len()),
                origin,
            });
        }
    }

    pub(super) fn finish(self) -> Content<'s> {
        renders::record();
        Content::Complete {
            text: self.text,
            mappings: self.mappings,
        }
    }
}

#[cfg(test)]
mod tests;
