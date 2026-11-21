//! Mapping-preserving cleanup of decoded values supplied by language adapters.

use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{Content, Mapping, Origin, SourcePieces, TextRange, UnavailableReason};
use std::ops::Range;

pub(super) struct Projection<'i, 's> {
    text: &'i str,
    mappings: &'i [Mapping<'s>],
    at: usize,
}

impl<'s> Projection<'_, 's> {
    pub(super) fn new<'i>(value: &'i TextBuilder<'s>) -> Projection<'i, 's> {
        Projection {
            text: &value.text,
            mappings: &value.mappings,
            at: 0,
        }
    }

    pub(super) fn copy<B: WorkBudget>(
        &mut self,
        range: Range<usize>,
        output: &mut TextBuilder<'s>,
        budget: &B,
    ) -> Result<(), TextError> {
        if range.is_empty() {
            return Ok(());
        }
        let start = output.text.len();
        output.append_utf8(self.text[range.clone()].as_bytes(), budget)?;
        while let Some(mapping) = self.mappings.get(self.at) {
            budget
                .step()
                .map_err(|error| TextError::Source(error.into()))?;
            projection_visits::record();
            let span = mapping.output.range();
            if span.start >= range.end {
                break;
            }
            if span.end > range.start {
                let selected = span.start.max(range.start)..span.end.min(range.end);
                let origin = match &mapping.origin {
                    Origin::Copied(pieces) => {
                        let piece = &pieces.pieces()[0];
                        let source = piece.range().start + selected.start - span.start;
                        Origin::Copied(SourcePieces::single(
                            piece.slice(source..source + selected.len()),
                        ))
                    }
                    Origin::Decoded(pieces) => Origin::Decoded(copy_pieces(pieces, budget)?),
                    Origin::Expanded(pieces) => Origin::Expanded(copy_pieces(pieces, budget)?),
                    Origin::Synthetic => Origin::Synthetic,
                };
                output.mappings.push(Mapping {
                    output: TextRange::validated(
                        start + selected.start - range.start..start + selected.end - range.start,
                    ),
                    origin,
                });
            }
            if span.end > range.end {
                break;
            }
            self.at += 1;
        }
        Ok(())
    }

    fn expand<B: WorkBudget>(
        &mut self,
        at: usize,
        spaces: usize,
        output: &mut TextBuilder<'s>,
        budget: &B,
    ) -> Result<(), TextError> {
        while self.mappings[self.at].output.range().end <= at {
            budget
                .step()
                .map_err(|error| TextError::Source(error.into()))?;
            self.at += 1;
        }
        let mapping = &self.mappings[self.at];
        let origin = match &mapping.origin {
            Origin::Copied(pieces) => {
                let piece = &pieces.pieces()[0];
                let start = piece.range().start + at - mapping.output.range().start;
                Origin::Expanded(SourcePieces::single(piece.slice(start..start + 1)))
            }
            Origin::Decoded(pieces) | Origin::Expanded(pieces) => {
                Origin::Expanded(copy_pieces(pieces, budget)?)
            }
            Origin::Synthetic => Origin::Synthetic,
        };
        output.transformed(&"        "[..spaces], origin, budget)
    }

    fn newline<B: WorkBudget>(
        &mut self,
        ending: Range<usize>,
        output: &mut TextBuilder<'s>,
        budget: &B,
    ) -> Result<(), TextError> {
        if ending.len() != 2 {
            return self.copy(ending, output, budget);
        }
        let mut newline = TextBuilder::default();
        self.copy(ending, &mut newline, budget)?;
        let mut pieces = Vec::new();
        for mapping in newline.mappings {
            let source = match mapping.origin {
                Origin::Copied(pieces) | Origin::Decoded(pieces) | Origin::Expanded(pieces) => {
                    pieces
                }
                Origin::Synthetic => continue,
            };
            for piece in source.pieces() {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                pieces.push(piece.clone());
            }
        }
        output.transformed(
            "\n",
            Origin::Decoded(SourcePieces::from_included_pieces(pieces)),
            budget,
        )
    }
}

crate::observation::counter!(projection_visits, cfg(any(test, feature = "test-support")));

fn copy_pieces<'s, B: WorkBudget>(
    pieces: &SourcePieces<'s>,
    budget: &B,
) -> Result<SourcePieces<'s>, TextError> {
    pieces
        .copy_with_budget(budget)
        .map_err(|error| TextError::Source(error.into()))
}

pub(super) fn python<'s, B: WorkBudget>(
    value: Content<'s>,
    budget: &B,
) -> Result<Content<'s>, TextError> {
    let Content::Complete { text, mappings } = value else {
        return Ok(value);
    };
    let mut input = Projection {
        text: &text,
        mappings: &mappings,
        at: 0,
    };
    let mut output = TextBuilder::default();
    let mut start = 0;
    let mut column = 0;
    for (at, scalar) in text.char_indices() {
        budget
            .bytes(scalar.len_utf8())
            .map_err(|error| TextError::Source(error.into()))?;
        match scalar {
            '\t' => {
                input.copy(start..at, &mut output, budget)?;
                let spaces = 8 - column % 8;
                input.expand(at, spaces, &mut output, budget)?;
                column += spaces;
                start = at + 1;
            }
            '\n' | '\r' => column = 0,
            _ => column += 1,
        }
    }
    input.copy(start..text.len(), &mut output, budget)?;
    python_margin(output.finish(), budget)
}

fn python_margin<'s, B: WorkBudget>(
    value: Content<'s>,
    budget: &B,
) -> Result<Content<'s>, TextError> {
    let Content::Complete { text, mappings } = value else {
        return Err(TextError::Unavailable(UnavailableReason::InvalidEncoding));
    };
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        if byte == b'\n' {
            lines.push(start..at);
            start = at + 1;
        }
    }
    lines.push(start..text.len());
    let mut margin = usize::MAX;
    for (position, line) in lines.iter_mut().enumerate() {
        let mut indent = 0;
        let mut prefix_bytes = 0;
        for scalar in text[line.clone()].chars() {
            budget
                .bytes(scalar.len_utf8())
                .map_err(|error| TextError::Source(error.into()))?;
            if !scalar.is_whitespace() {
                break;
            }
            indent += 1;
            prefix_bytes += scalar.len_utf8();
        }
        if position == 0 {
            line.start += prefix_bytes;
        } else if prefix_bytes < line.len() {
            margin = margin.min(indent);
        }
    }
    if margin != usize::MAX {
        for line in lines.iter_mut().skip(1) {
            budget
                .step()
                .map_err(|error| TextError::Source(error.into()))?;
            let mut removed = 0;
            for scalar in text[line.clone()].chars().take(margin) {
                budget
                    .bytes(scalar.len_utf8())
                    .map_err(|error| TextError::Source(error.into()))?;
                removed += scalar.len_utf8();
            }
            line.start += removed;
        }
    }
    let mut first = 0;
    let mut end = lines.len();
    while first < end && lines[first].is_empty() {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        first += 1;
    }
    while end > first && lines[end - 1].is_empty() {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        end -= 1;
    }
    let mut input = Projection {
        text: &text,
        mappings: &mappings,
        at: 0,
    };
    let mut output = TextBuilder::default();
    for (position, line) in lines[first..end].iter().enumerate() {
        if position != 0 {
            let newline = lines[first + position - 1].end;
            input.copy(newline..newline + 1, &mut output, budget)?;
        }
        input.copy(line.clone(), &mut output, budget)?;
    }
    Ok(output.finish())
}

pub(super) fn ruff<'s, B: WorkBudget>(
    value: Content<'s>,
    budget: &B,
) -> Result<Content<'s>, TextError> {
    let Content::Complete { text, mappings } = value else {
        return Ok(value);
    };
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        budget
            .bytes(1)
            .map_err(|error| TextError::Source(error.into()))?;
        if byte == b'\n' {
            let end = if at > start && text.as_bytes()[at - 1] == b'\r' {
                at - 1
            } else {
                at
            };
            lines.push((start..end, end..at + 1));
            start = at + 1;
        }
    }
    lines.push((start..text.len(), text.len()..text.len()));
    let mut prefix = "";
    for (line, _) in &lines {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        let bytes = &text.as_bytes()[line.clone()];
        let margin = crate::documentation::byte_scan::position(
            bytes,
            |byte| !matches!(byte, b' ' | b'\t'),
            budget,
        )
        .map_err(|error| TextError::Source(error.into()))?
        .unwrap_or(bytes.len());
        if margin < bytes.len() {
            prefix = &text[line.start..line.start + margin];
            break;
        }
    }
    for (line, _) in &mut lines {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        let mut matching = line.len() >= prefix.len();
        for (actual, expected) in text.as_bytes()[line.clone()].iter().zip(prefix.bytes()) {
            budget
                .bytes(1)
                .map_err(|error| TextError::Source(error.into()))?;
            if *actual != expected {
                matching = false;
                break;
            }
        }
        if matching {
            line.start += prefix.len();
        }
    }
    let mut first = 0;
    let mut end = lines.len();
    while first < end && lines[first].0.is_empty() {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        first += 1;
    }
    while end > first && lines[end - 1].0.is_empty() {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        end -= 1;
    }
    let mut input = Projection {
        text: &text,
        mappings: &mappings,
        at: 0,
    };
    let mut output = TextBuilder::default();
    for (position, (line, ending)) in lines[first..end].iter().enumerate() {
        input.copy(line.clone(), &mut output, budget)?;
        if position + first + 1 < end {
            input.newline(ending.clone(), &mut output, budget)?;
        }
    }
    Ok(output.finish())
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use super::*;
    use crate::documentation::budget::NoBudget;
    use crate::documentation::text::literals::{LiteralPolicy, decode};
    use crate::documentation::{CompleteText, OriginView, SourceContext};
    use crate::{AnyParser, LANG};

    #[test]
    fn whole_python_document_cleanup_keeps_escape_and_tab_origins() {
        let source = "\"\"\"  first\\n\\tsecond  \\n\\t  third\\n\"\"\"";
        let parser =
            AnyParser::parse(LANG::Python, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
        let mut builder = TextBuilder::default();
        decode(
            &context,
            3..source.len() - 3,
            LiteralPolicy::PythonCooked,
            &mut builder,
            &NoBudget,
        )
        .unwrap();
        let Content::Complete { text, mappings } = python(builder.finish(), &NoBudget).unwrap()
        else {
            panic!("complete value")
        };
        assert_eq!(text, "first\nsecond  \n  third");
        let view = CompleteText {
            text: &text,
            mappings: &mappings,
        };
        let OriginView::Decoded(pieces) = view.origins(5..6).unwrap().next().unwrap().origin else {
            panic!("decoded newline")
        };
        assert_eq!(pieces[0].bytes(), b"\\n");
        assert_eq!(view.origins(15..17).unwrap().next().unwrap().output, 15..17);
    }

    #[test]
    fn python_cleanup_counts_unicode_margin_in_characters() {
        let source = "\"\"\"\n\u{2003}alpha\n beta\n\"\"\"";
        let parser =
            AnyParser::parse(LANG::Python, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source.as_bytes());
        let mut builder = TextBuilder::default();
        decode(
            &context,
            3..source.len() - 3,
            LiteralPolicy::Raw,
            &mut builder,
            &NoBudget,
        )
        .unwrap();
        let Content::Complete { text, .. } = python(builder.finish(), &NoBudget).unwrap() else {
            panic!("complete value")
        };
        assert_eq!(text, "alpha\nbeta");
    }
}
