//! Comment-local cleanup over source coordinates, before any output copying.

use super::{TextBuilder, TextError};
use crate::documentation::budget::WorkBudget;
use crate::documentation::{Content, Fragment, Origin, Profile, SourceContext, UnavailableReason};
use std::ops::Range;

struct Line {
    content: Range<usize>,
    ending: Range<usize>,
}

struct Block {
    lines: Vec<Line>,
    rust_decoration: bool,
    markdown: bool,
}

fn charge<B: WorkBudget>(budget: &B) -> Result<(), TextError> {
    budget
        .bytes(1)
        .map_err(|error| TextError::Source(error.into()))
}

fn margin<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<usize, TextError> {
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

fn lines<B: WorkBudget>(
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

fn prepare<B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'_>,
    fragment: &Fragment<'_>,
    budget: &B,
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
    if profile == Profile::Javadoc {
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
    range: Range<usize>,
    splices: bool,
    budget: &B,
) -> Result<(), TextError> {
    if !splices {
        return builder.source(context, range, budget);
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
        builder.source(context, start..at, budget)?;
        at += removed;
        start = at;
    }
    builder.source(context, start..range.end, budget)
}

pub(in crate::documentation) fn render<'s, B: WorkBudget>(
    profile: Profile,
    context: &SourceContext<'s>,
    fragments: &[&Fragment<'s>],
    budget: &B,
) -> Result<Content<'s>, TextError> {
    let mut blocks = Vec::new();
    for fragment in fragments {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        blocks.push(prepare(profile, context, fragment, budget)?);
    }
    if profile == Profile::Rust {
        for block in &mut blocks {
            beautify(block, context.bytes(), budget)?;
        }
    }
    if profile == Profile::Rust || blocks.first().is_some_and(|block| block.markdown) {
        let mut minimum = usize::MAX;
        for block in &blocks {
            for line in &block.lines {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                let indent = margin(&context.bytes()[line.content.clone()], budget)?;
                if indent < line.content.len() {
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
                    let indent = margin(&context.bytes()[line.content.clone()], budget)?;
                    if indent < line.content.len() {
                        line.content.start += minimum;
                    }
                }
            }
        }
    }
    let mut builder = TextBuilder::default();
    for (position, block) in blocks.iter().enumerate() {
        budget
            .step()
            .map_err(|error| TextError::Source(error.into()))?;
        if position != 0 {
            builder.transformed("\n", Origin::Synthetic, budget)?;
        }
        let mut first = 0;
        let mut end = block.lines.len();
        if profile != Profile::Rust {
            while first < end && block.lines[first].content.is_empty() {
                budget
                    .step()
                    .map_err(|error| TextError::Source(error.into()))?;
                first += 1;
            }
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
                line.content.clone(),
                profile == Profile::Doxygen,
                budget,
            )?;
            if index + first + 1 < end {
                if line.ending.len() == 2 {
                    builder.transformed(
                        "\n",
                        Origin::Decoded(context.pieces_with_budget(line.ending.clone(), budget)?),
                        budget,
                    )?;
                } else {
                    builder.source(context, line.ending.clone(), budget)?;
                }
            }
        }
    }
    Ok(builder.finish())
}

fn beautify<B: WorkBudget>(block: &mut Block, bytes: &[u8], budget: &B) -> Result<(), TextError> {
    if block.lines.len() < 2 {
        return Ok(());
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
        if let Some(line) = block.lines.first() {
            let indent = margin(&bytes[line.content.clone()], budget)?;
            if bytes.get(line.content.start + indent) != Some(&b'*') {
                start = 1;
            }
        }
        while start < end
            && margin(&bytes[block.lines[start].content.clone()], budget)?
                == block.lines[start].content.len()
        {
            start += 1;
        }
        while end > start
            && margin(&bytes[block.lines[end - 1].content.clone()], budget)?
                == block.lines[end - 1].content.len()
        {
            end -= 1;
        }
    }
    let mut star_column = None;
    for line in &block.lines[start..end] {
        let indent = margin(&bytes[line.content.clone()], budget)?;
        if line.content.start + indent >= line.content.end
            || bytes[line.content.start + indent] != b'*'
        {
            return Ok(());
        }
        if star_column.is_some_and(|column| column != indent) {
            return Ok(());
        }
        star_column = Some(indent);
    }
    let Some(column) = star_column else {
        return Ok(());
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
    Ok(())
}
