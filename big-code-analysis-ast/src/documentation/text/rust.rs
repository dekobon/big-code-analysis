//! Rust mixed-fragment cleanup in decoded coordinates with composed origins.

use super::cleanup::Projection;
use super::comments::{self, Block, blank, margin};
use super::source_cursor::SourceCursor;
use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{Content, Fragment, Origin, Profile, SourceContext};
use std::ops::Range;

pub(in crate::documentation) mod literal;

pub(in crate::documentation) enum Input<'f, 's> {
    Comment(&'f Fragment<'s>),
    Literal(&'f Fragment<'s>, Range<usize>),
}

struct Prepared<'s> {
    block: Block,
    decoded: Option<TextBuilder<'s>>,
}

impl<'s> Prepared<'s> {
    fn new<B: WorkBudget>(
        context: &SourceContext<'s>,
        input: &Input<'_, 's>,
        budget: &B,
    ) -> Result<Self, TextError> {
        // bca: suppress(nexits) -- decoding, source validation and beautification propagate independent failures.
        let (mut block, decoded) = match input {
            Input::Comment(fragment) => {
                let block = comments::prepare(Profile::Rust, context, fragment, budget, false)?;
                for line in &block.lines {
                    budget.step().map_err(Into::into)?;
                    literal::validate(&context.bytes()[line.content.clone()], budget)?;
                }
                (block, None)
            }
            Input::Literal(fragment, range) => {
                let value = literal::decode(context, fragment, range.clone(), budget)?;
                let block = Block {
                    lines: comments::lines(
                        value.text.as_bytes(),
                        0..value.text.len(),
                        false,
                        budget,
                    )?,
                    rust_decoration: false,
                    markdown: false,
                };
                (block, Some(value))
            }
        };
        let bytes = decoded
            .as_ref()
            .map_or(context.bytes(), |value| value.text.as_bytes());
        comments::rust_fragments(std::slice::from_mut(&mut block), bytes, budget)?;
        Ok(Self { block, decoded })
    }

    fn bytes<'a>(&'a self, context: &'a SourceContext<'_>) -> &'a [u8] {
        self.decoded
            .as_ref()
            .map_or(context.bytes(), |value| value.text.as_bytes())
    }

    fn minimum<B: WorkBudget>(
        &self,
        context: &SourceContext<'s>,
        add: usize,
        budget: &B,
    ) -> Result<usize, TextError> {
        let mut minimum = usize::MAX;
        for line in &self.block.lines {
            budget.step().map_err(Into::into)?;
            let text = &self.bytes(context)[line.content.clone()];
            if !blank(text, true, budget)? {
                minimum = minimum
                    .min(margin(text, budget)? + if self.decoded.is_some() { add } else { 0 });
            }
        }
        Ok(minimum)
    }
}

pub(in crate::documentation) fn render<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    inputs: &[Input<'_, 's>],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    // bca: suppress(nexits, halstead) -- whole-document preparation, margin selection and mapped emission are separately fallible passes.
    let mut prepared = Vec::new();
    let mut sugared = false;
    let mut nonsugared = false;
    for input in inputs {
        budget.step().map_err(Into::into)?;
        sugared |= matches!(input, Input::Comment(_));
        nonsugared |= matches!(input, Input::Literal(_, _));
        prepared.push(Prepared::new(context, input, budget)?);
    }
    let add = usize::from(sugared && nonsugared);
    let mut minimum = usize::MAX;
    for fragment in &prepared {
        budget.step().map_err(Into::into)?;
        minimum = minimum.min(fragment.minimum(context, add, budget)?);
    }
    let mut output = TextBuilder::default();
    for (position, (fragment, input)) in prepared.iter().zip(inputs).enumerate() {
        budget.step().map_err(Into::into)?;
        if position != 0 {
            output.transformed("\n", Origin::Synthetic, budget)?;
        }
        let indent = if minimum == usize::MAX {
            0
        } else {
            minimum.saturating_sub(if fragment.decoded.is_some() { add } else { 0 })
        };
        emit(fragment, input, context, indent, &mut output, budget)?;
    }
    Ok(output.finish())
}

fn emit<'s, B: WorkBudget>(
    fragment: &Prepared<'s>,
    input: &Input<'_, 's>,
    context: &SourceContext<'s>,
    indent: usize,
    output: &mut TextBuilder<'s>,
    budget: &B,
) -> Result<(), TextError> {
    // bca: suppress(cognitive, halstead, nargs, nexits) -- one ordered pass emits source or decoded coordinates with distinct newline origins.
    let (Input::Comment(raw) | Input::Literal(raw, _)) = input;
    let mut source = SourceCursor::new(&raw.source);
    let mut projection = fragment.decoded.as_ref().map(Projection::new);
    for (position, line) in fragment.block.lines.iter().enumerate() {
        budget.step().map_err(Into::into)?;
        let mut range = line.content.clone();
        if !blank(&fragment.bytes(context)[range.clone()], true, budget)? {
            range.start += indent;
        }
        if let Some(projection) = &mut projection {
            projection.copy(range, output, budget)?;
            if position + 1 < fragment.block.lines.len() && !line.ending.is_empty() {
                // str::lines removes a decoded CR; only the LF token is responsible.
                projection.copy(line.ending.end - 1..line.ending.end, output, budget)?;
            }
        } else {
            source.copy(output, range, budget)?;
            if position + 1 < fragment.block.lines.len() {
                source.newline(output, line.ending.clone(), budget)?;
            }
        }
    }
    Ok(())
}
