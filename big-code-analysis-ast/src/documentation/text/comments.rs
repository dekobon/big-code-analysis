//! Comment-local cleanup over source coordinates, before any output copying.

use super::source_cursor::SourceCursor;
use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{Content, Fragment, Origin, Profile, SourceContext, UnavailableReason};
use std::ops::Range;

pub(super) struct Line {
    pub(super) content: Range<usize>,
    pub(super) ending: Range<usize>,
}

pub(super) struct Block {
    pub(super) lines: Vec<Line>,
    pub(super) rust_decoration: bool,
    pub(super) markdown: bool,
}

fn charge<B: WorkBudget>(budget: &B) -> Result<(), TextError> {
    budget
        .bytes(1)
        .map_err(|error| TextError::Source(error.into()))
}

pub(super) fn margin<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<usize, TextError> {
    let mut count = 0;
    for byte in bytes {
        charge(budget)?;
        if !matches!(byte, b' ' | b'\t') {
            break;
        }
        count += 1;
    }
    Ok(count)
}

fn splice(bytes: &[u8], at: usize) -> usize {
    if bytes[at..].starts_with(b"\\\r\n") {
        3
    } else if bytes[at..].starts_with(b"\\\n") {
        2
    } else {
        0
    }
}

pub(super) fn lines<B: WorkBudget>(
    bytes: &[u8],
    range: Range<usize>,
    splices: bool,
    budget: &B,
) -> Result<Vec<Line>, TextError> {
    let mut lines = Vec::new();
    let mut start = range.start;
    let mut at = start;
    while at < range.end {
        charge(budget)?;
        let removed = if splices {
            splice(&bytes[..range.end], at)
        } else {
            0
        };
        if removed != 0 {
            for _ in 1..removed {
                charge(budget)?;
            }
            at += removed;
            continue;
        }
        if bytes[at] == b'\n' {
            let end = if at > start && bytes[at - 1] == b'\r' {
                at - 1
            } else {
                at
            };
            lines.push(Line {
                content: start..end,
                ending: end..at + 1,
            });
            start = at + 1;
        }
        at += 1;
    }
    lines.push(Line {
        content: start..range.end,
        ending: range.end..range.end,
    });
    Ok(lines)
}

fn delimiters<B: WorkBudget>(
    profile: Profile,
    bytes: &[u8],
    range: Range<usize>,
    budget: &B,
) -> Result<(Range<usize>, bool, bool), TextError> {
    let raw = &bytes[range.clone()];
    let (prefix, suffix, block) = if raw.starts_with(b"/*") {
        let prefix = if profile == Profile::GoDoc { 2 } else { 3 };
        if !raw.ends_with(b"*/") {
            return Err(TextError::Unavailable(UnavailableReason::GrammarBoundary));
        }
        (prefix.min(raw.len() - 2), 2, true)
    } else if raw.starts_with(b"//") {
        (if profile == Profile::GoDoc { 2 } else { 3 }, 0, false)
    } else if raw.starts_with(b"--[") {
        let mut equals = 3;
        while raw.get(equals) == Some(&b'=') {
            charge(budget)?;
            equals += 1;
        }
        if raw.get(equals) == Some(&b'[') {
            let close = equals - 1;
            if raw.len() < equals + 1 + close
                || raw[raw.len() - close] != b']'
                || raw.last() != Some(&b']')
            {
                return Err(TextError::Unavailable(UnavailableReason::GrammarBoundary));
            }
            for byte in &raw[raw.len() - close + 1..raw.len() - 1] {
                charge(budget)?;
                if *byte != b'=' {
                    return Err(TextError::Unavailable(UnavailableReason::GrammarBoundary));
                }
            }
            let mut prefix = equals + 1;
            while raw.get(prefix) == Some(&b'-') {
                charge(budget)?;
                prefix += 1;
            }
            (prefix, close, true)
        } else {
            (2, 0, false)
        }
    } else if raw.starts_with(b"--") {
        let mut prefix = 2;
        while raw.get(prefix) == Some(&b'-') {
            charge(budget)?;
            prefix += 1;
        }
        (prefix, 0, false)
    } else if raw.starts_with(b"#") {
        (
            if raw.starts_with(b"##") && raw.get(2) != Some(&b'#') {
                2
            } else {
                1
            },
            0,
            false,
        )
    } else if raw.starts_with(b"=begin") {
        let mut first = 0;
        while first < raw.len() && raw[first] != b'\n' {
            charge(budget)?;
            first += 1;
        }
        let mut last = raw.len();
        while last > first && raw[last - 1] != b'\n' {
            charge(budget)?;
            last -= 1;
        }
        return Ok((
            range.start + (first + 1).min(raw.len())..range.start + last,
            true,
            false,
        ));
    } else {
        return Err(TextError::Unavailable(UnavailableReason::GrammarBoundary));
    };
    let mut start = range.start + prefix;
    let end = range.end - suffix;
    if profile == Profile::Doxygen && bytes.get(start) == Some(&b'<') {
        start += 1;
    }
    Ok((start.min(end)..end, block, raw.starts_with(b"/*")))
}

pub(super) fn prepare<B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'_>,
    fragment: &Fragment<'_>,
    budget: &B,
    source_cleaned: bool,
) -> Result<Block, TextError> {
    if fragment.source.has_excluded_gap() {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    }
    let pieces = fragment.source.pieces();
    let Some(first) = pieces.first() else {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    };
    let Some(last) = pieces.last() else {
        return Err(TextError::Unavailable(UnavailableReason::ExcludedGap));
    };
    let raw = first.range().start..last.range().end;
    let bytes = context.bytes();
    if profile == Profile::Javadoc && !source_cleaned {
        for pair in bytes[raw.clone()].windows(2) {
            charge(budget)?;
            if pair == b"\\u" {
                return Err(TextError::Unavailable(
                    UnavailableReason::UnsupportedDecoding,
                ));
            }
        }
    }
    let (body, block, c_block) = delimiters(profile, bytes, raw, budget)?;
    let mut lines = lines(bytes, body, profile == Profile::Doxygen, budget)?;
    if profile != Profile::Rust && (profile != Profile::Javadoc || block) {
        if (c_block || !block)
            && lines
                .first()
                .is_some_and(|line| bytes.get(line.content.start) == Some(&b' '))
        {
            lines[0].content.start += 1;
        }
        if c_block
            && let Some(last) = lines.last_mut()
            && !last.content.is_empty()
            && bytes[last.content.end - 1] == b' '
        {
            last.content.end -= 1;
        }
        if c_block && profile != Profile::GoDoc {
            for line in lines.iter_mut().skip(1) {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                let indent = margin(&bytes[line.content.clone()], budget)?;
                let star = line.content.start + indent;
                if bytes.get(star) == Some(&b'*') && star < line.content.end {
                    line.content.start = star + 1;
                    if line.content.start < line.content.end && bytes[line.content.start] == b' ' {
                        line.content.start += 1;
                    }
                } else if star == line.content.end {
                    line.content.start = star;
                }
            }
        }
    }
    Ok(Block {
        lines,
        rust_decoration: profile == Profile::Rust && block,
        markdown: profile == Profile::Javadoc && !block,
    })
}

fn append_range<'s, B: WorkBudget>(
    builder: &mut TextBuilder<'s>,
    context: &SourceContext<'s>,
    source: &mut SourceCursor<'_, 's>,
    range: Range<usize>,
    splices: bool,
    budget: &B,
) -> Result<(), TextError> {
    if !splices {
        return source.copy(builder, range, budget);
    }
    let mut start = range.start;
    let mut at = start;
    while at < range.end {
        charge(budget)?;
        let removed = splice(&context.bytes()[..range.end], at);
        if removed == 0 {
            at += 1;
            continue;
        }
        for _ in 1..removed {
            charge(budget)?;
        }
        source.copy(builder, start..at, budget)?;
        at += removed;
        start = at;
    }
    source.copy(builder, start..range.end, budget)
}

pub(in crate::documentation) fn render<'s, B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    render_local(profile, context, fragments, budget, false)
}

pub(in crate::documentation) fn render_source_cleaned<'s, B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    render_local(profile, context, fragments, budget, true)
}

fn render_local<'s, B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    budget: &B,
    source_cleaned: bool,
) -> Result<Content<'s>, TextError> {
    let mut blocks = Vec::new();
    for fragment in fragments {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        blocks.push(prepare(profile, context, fragment, budget, source_cleaned)?);
    }
    if profile == Profile::Rust {
        rust_fragments(&mut blocks, context.bytes(), budget)?;
    }
    if profile == Profile::Rust || blocks.first().is_some_and(|block| block.markdown) {
        let mut minimum = usize::MAX;
        for block in &blocks {
            for line in &block.lines {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                let indent = margin(&context.bytes()[line.content.clone()], budget)?;
                if !blank(
                    &context.bytes()[line.content.clone()],
                    profile == Profile::Rust,
                    budget,
                )? {
                    minimum = minimum.min(indent);
                }
            }
        }
        if minimum != usize::MAX {
            for block in &mut blocks {
                for line in &mut block.lines {
                    budget
                        .step()
                        .map_err(|error| TextError::Source(error.into()))?;
                    if !blank(
                        &context.bytes()[line.content.clone()],
                        profile == Profile::Rust,
                        budget,
                    )? {
                        line.content.start += minimum;
                    }
                }
            }
        }
    }
    emit_blocks(profile, context, fragments, &blocks, budget)
}

fn emit_blocks<'s, B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    blocks: &[Block],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    let mut builder = TextBuilder::default();
    let mut first_block = 0;
    let mut last_block = blocks.len();
    if profile != Profile::Rust {
        while first_block < last_block && empty_block(&blocks[first_block], budget)? {
            first_block += 1;
        }
        while last_block > first_block && empty_block(&blocks[last_block - 1], budget)? {
            last_block -= 1;
        }
    }
    for (position, block) in blocks.iter().enumerate().take(last_block).skip(first_block) {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        if position != first_block {
            builder.transformed("\n", Origin::Synthetic, budget)?;
        }
        let mut source = SourceCursor::new(&fragments[position].source);
        let mut first = 0;
        let mut end = block.lines.len();
        if profile != Profile::Rust && position == first_block {
            while first < end && block.lines[first].content.is_empty() {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                first += 1;
            }
        }
        if profile != Profile::Rust && position + 1 == last_block {
            while end > first && block.lines[end - 1].content.is_empty() {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                end -= 1;
            }
        }
        for (index, line) in block.lines[first..end].iter().enumerate() {
            append_range(
                &mut builder,
                context,
                &mut source,
                line.content.clone(),
                profile == Profile::Doxygen,
                budget,
            )?;
            if index + first + 1 < end {
                source.newline(&mut builder, line.ending.clone(), budget)?;
            }
        }
    }
    Ok(builder.finish())
}

fn empty_block<B: WorkBudget>(block: &Block, budget: &B) -> Result<bool, TextError> {
    for line in &block.lines {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        if !line.content.is_empty() {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn blank<B: WorkBudget>(
    bytes: &[u8],
    unicode: bool,
    budget: &B,
) -> Result<bool, TextError> {
    budget
        .step()
        .map_err(|error| TextError::Source(error.into()))?;
    if !unicode {
        return Ok(crate::documentation::byte_scan::position(
            bytes,
            |byte| !matches!(byte, b' ' | b'\t'),
            budget,
        )
        .map_err(|error| TextError::Source(error.into()))?
        .is_none());
    }
    let mut at = 0;
    while at < bytes.len() {
        let text = super::utf8_scalar(&bytes[at..], budget)?;
        if text.chars().any(|character| !character.is_whitespace()) {
            return Ok(false);
        }
        at += text.len();
    }
    Ok(true)
}

pub(super) fn rust_fragments<B: WorkBudget>(
    blocks: &mut [Block],
    bytes: &[u8],
    budget: &B,
) -> Result<(), TextError> {
    for block in blocks {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        // str::lines omits a terminal empty line both before beautification
        // and when rustdoc reads the changed, joined fragment.
        if block.lines.len() > 1
            && block
                .lines
                .last()
                .is_some_and(|line| line.content.is_empty())
        {
            block.lines.pop();
        }
        let changed = beautify(block, bytes, budget)?;
        if changed {
            // Rejoining adds LF after every retained line except the last. The
            // second str::lines pass removes one CR, even before a terminal empty.
            let rejoined = block.lines.len().saturating_sub(1);
            for line in &mut block.lines[..rejoined] {
                budget.step().map_err(Into::into)?;
                if bytes[line.content.clone()].last() == Some(&b'\r') {
                    line.content.end -= 1;
                }
            }
        }
        if changed
            && block.lines.len() > 1
            && block
                .lines
                .last()
                .is_some_and(|line| line.content.is_empty())
        {
            block.lines.pop();
        }
    }
    Ok(())
}

fn rust_first_line_star<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<bool, TextError> {
    let mut at = 0;
    while at < bytes.len() {
        let scalar = super::utf8_scalar(&bytes[at..], budget)?;
        if scalar.chars().any(|character| !character.is_whitespace()) {
            return Ok(scalar == "*");
        }
        at += scalar.len();
    }
    Ok(false)
}

fn beautify<B: WorkBudget>(block: &mut Block, bytes: &[u8], budget: &B) -> Result<bool, TextError> {
    if block.lines.len() < 2
        && block
            .lines
            .first()
            .is_none_or(|line| line.ending.is_empty())
    {
        return Ok(false);
    }
    let all_stars = |range: Range<usize>| -> Result<bool, TextError> {
        for byte in &bytes[range] {
            charge(budget)?;
            if *byte != b'*' {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let first = usize::from(all_stars(block.lines[0].content.clone())?);
    let mut end = block.lines.len();
    if end > first
        && !block.lines[end - 1].content.is_empty()
        && all_stars(block.lines[end - 1].content.clone())?
    {
        end -= 1;
    }
    let changed = first != 0 || end != block.lines.len();
    let mut selected = Vec::new();
    for line in block.lines.drain(..).skip(first).take(end - first) {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        selected.push(line);
    }
    block.lines = selected;
    let mut start = 0;
    let mut end = block.lines.len();
    if block.rust_decoration {
        if let Some(line) = block.lines.first()
            && !rust_first_line_star(&bytes[line.content.clone()], budget)?
        {
            start = 1;
        }
        while start < end && blank(&bytes[block.lines[start].content.clone()], true, budget)? {
            start += 1;
        }
        while end > start && blank(&bytes[block.lines[end - 1].content.clone()], true, budget)? {
            end -= 1;
        }
    }
    let mut star_column = None;
    for line in &block.lines[start..end] {
        crate::documentation::budget::in_phase(
            budget,
            crate::documentation::budget::WorkPhase::Decoration,
            || budget.step(),
        )
        .map_err(|error| TextError::Source(error.into()))?;
        let indent = margin(&bytes[line.content.clone()], budget)?;
        if line.content.start + indent >= line.content.end
            || bytes[line.content.start + indent] != b'*'
        {
            return Ok(changed);
        }
        if star_column.is_some_and(|column| column != indent) {
            return Ok(changed);
        }
        star_column = Some(indent);
    }
    let Some(column) = star_column else {
        return Ok(changed);
    };
    let prefix =
        &bytes[block.lines[start].content.start..block.lines[start].content.start + column];
    for line in &mut block.lines {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        let content = &bytes[line.content.clone()];
        let mut matching = content.len() >= prefix.len();
        for (actual, expected) in content.iter().zip(prefix) {
            charge(budget)?;
            if actual != expected {
                matching = false;
                break;
            }
        }
        if matching {
            line.content.start += column;
            let content = &bytes[line.content.clone()];
            if block.rust_decoration
                && (content == b"*" || content.starts_with(b"* ") || content.starts_with(b"**"))
            {
                line.content.start += 1;
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod remediation_tests {
    use super::*;
    use crate::documentation::SourceError;
    use crate::documentation::budget::{CollectionBudget, WorkPhase, cancel_in_phase};
    use std::cell::Cell;

    #[derive(Default)]
    struct CountingBudget {
        bytes: Cell<usize>,
        steps: Cell<usize>,
        fail_byte: Option<usize>,
        fail_step: Option<usize>,
    }

    impl WorkBudget for CountingBudget {
        type Error = SourceError;

        fn checkpoint(&self) -> Result<(), SourceError> {
            Ok(())
        }

        fn step(&self) -> Result<(), SourceError> {
            self.steps.set(self.steps.get() + 1);
            if self.fail_step == Some(self.steps.get()) {
                Err(SourceError::Cancelled)
            } else {
                Ok(())
            }
        }

        fn bytes(&self, count: usize) -> Result<(), SourceError> {
            self.bytes.set(self.bytes.get() + count);
            if self.fail_byte.is_some_and(|at| self.bytes.get() >= at) {
                Err(SourceError::Cancelled)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn unicode_first_line_scan_is_bounded_and_charges_each_scalar() {
        for size in [8, 32, 4096] {
            let source = format!("{}*", "\u{2003}".repeat(size));
            let budget = CountingBudget::default();
            assert!(rust_first_line_star(source.as_bytes(), &budget).unwrap());
            // expected: N three-byte scalars followed by one ASCII star, no line steps.
            assert_eq!((budget.bytes.get(), budget.steps.get()), (3 * size + 1, 0));
            let budget = CountingBudget {
                fail_byte: Some(6),
                ..CountingBudget::default()
            };
            assert!(matches!(
                rust_first_line_star(source.as_bytes(), &budget),
                Err(TextError::Source(SourceError::Cancelled))
            ));
            assert_eq!(budget.bytes.get(), 6);
        }
        for bytes in [b"".as_slice(), b" \t", "\u{a0}".as_bytes()] {
            assert!(!rust_first_line_star(bytes, &CountingBudget::default()).unwrap());
        }
        let bytes = "\u{2003}*".as_bytes();
        assert!(matches!(
            rust_first_line_star(&bytes[..2], &CountingBudget::default()),
            Err(TextError::Unavailable(UnavailableReason::InvalidEncoding))
        ));
        assert!(!rust_first_line_star(&bytes[..3], &CountingBudget::default()).unwrap());
    }

    #[test]
    fn second_line_inspection_is_charged_before_trimming() {
        let bytes = b"\na\r\r\nb";
        for fail_step in [None, Some(5)] {
            let mut block = Block {
                lines: lines(bytes, 0..bytes.len(), false, &CountingBudget::default()).unwrap(),
                rust_decoration: false,
                markdown: false,
            };
            let budget = CountingBudget {
                fail_step,
                ..CountingBudget::default()
            };
            let result = rust_fragments(std::slice::from_mut(&mut block), bytes, &budget);
            // expected: one block, two selected lines, one decoration and one rejoined inspection.
            assert_eq!(budget.steps.get(), 5);
            assert_eq!(budget.bytes.get(), 2); // last-line all-stars and first-line ASCII margin.
            if fail_step.is_some() {
                assert!(matches!(
                    result,
                    Err(TextError::Source(SourceError::Cancelled))
                ));
                assert_eq!(block.lines[0].content, 1..3);
            } else {
                result.unwrap();
                assert_eq!(block.lines[0].content, 1..2);
            }
            assert_eq!(block.lines[0].ending, 3..5);
        }
    }

    #[test]
    fn star_column_pass_cancels_on_line_budget_before_byte_budget() {
        let bytes = b"*";
        let mut block = Block {
            lines: (0..2048)
                .map(|_| Line {
                    content: 0..1,
                    ending: 0..0,
                })
                .collect(),
            rust_decoration: false,
            markdown: false,
        };
        let result = cancel_in_phase(WorkPhase::Decoration, || {
            beautify(&mut block, bytes, &CollectionBudget::default())
        });
        assert!(matches!(
            result,
            Err(TextError::Source(
                crate::documentation::SourceError::Cancelled
            ))
        ));
        let mut next = Block {
            lines: vec![Line {
                content: 0..1,
                ending: 0..0,
            }],
            rust_decoration: false,
            markdown: false,
        };
        assert!(!beautify(&mut next, bytes, &CollectionBudget::default()).unwrap());
    }
}
