//! Static Rust strings validated and decoded in source order into temporary mapped text.

use super::*;
use crate::documentation::UnavailableReason;
use crate::documentation::text::utf8_scalar;

fn invalid() -> TextError {
    TextError::Unavailable(UnavailableReason::InvalidEncoding)
}

pub(super) fn validate<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<(), TextError> {
    let mut at = 0;
    while at < bytes.len() {
        at += utf8_scalar(&bytes[at..], budget)?.len();
    }
    Ok(())
}

pub(super) fn decode<'s, B: WorkBudget>(
    context: &SourceContext<'s>,
    fragment: &Fragment<'s>,
    range: Range<usize>,
    budget: &B,
) -> Result<TextBuilder<'s>, TextError> {
    // bca: suppress(cognitive, halstead, nexits) -- the literal state machine preserves source-order failures and complete escape origins.
    let bytes = &context.bytes()[range.clone()];
    if matches!(bytes.first(), Some(b'b' | b'c')) {
        return Err(TextError::Unavailable(
            UnavailableReason::UnsupportedDecoding,
        ));
    }
    let (body, raw) = delimiters(bytes, budget)?;
    let bytes = &bytes[..body.end];
    let mut source = SourceCursor::new(&fragment.source);
    let mut output = TextBuilder::default();
    let mut at = body.start;
    let mut copied = at;
    while at < body.end {
        budget.bytes(1).map_err(Into::into)?;
        decoded_units::record();
        let start = at;
        if bytes[at] == b'\r' || (!raw && bytes[at] == b'\\') {
            source.copy(&mut output, range.start + copied..range.start + at, budget)?;
            let scalar = if bytes[at] == b'\r' {
                if bytes.get(at + 1) != Some(&b'\n') {
                    return Err(invalid());
                }
                at += 2;
                Some('\n')
            } else {
                escape(bytes, &mut at, budget)?
            };
            if let Some(scalar) = scalar {
                let mut buffer = [0; 4];
                output.transformed(
                    scalar.encode_utf8(&mut buffer),
                    Origin::Decoded(source.pieces(range.start + start..range.start + at, budget)?),
                    budget,
                )?;
            }
            copied = at;
        } else {
            // Validate copied text before a later escape can select a different failure.
            at += utf8_scalar(&bytes[at..], budget)?.len();
        }
    }
    source.copy(&mut output, range.start + copied..range.start + at, budget)?;
    Ok(output)
}

fn delimiters<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<(Range<usize>, bool), TextError> {
    // bca: suppress(halstead, nexits) -- exact raw hash delimiters and cooked boundaries have separate rejection points.
    let boundary = || TextError::Unavailable(UnavailableReason::GrammarBoundary);
    if bytes.first() == Some(&b'"') && bytes.len() >= 2 && bytes.last() == Some(&b'"') {
        return Ok((1..bytes.len() - 1, false));
    }
    if bytes.first() != Some(&b'r') {
        return Err(boundary());
    }
    let mut at = 1;
    while bytes.get(at) == Some(&b'#') {
        budget.bytes(1).map_err(Into::into)?;
        at += 1;
    }
    let hashes = at - 1;
    if hashes > 255 || bytes.get(at) != Some(&b'"') || bytes.len() < at + 2 + hashes {
        return Err(boundary());
    }
    let end = bytes.len() - hashes - 1;
    if bytes[end] != b'"' {
        return Err(boundary());
    }
    for byte in &bytes[end + 1..] {
        budget.bytes(1).map_err(Into::into)?;
        if *byte != b'#' {
            return Err(boundary());
        }
    }
    Ok((at + 1..end, true))
}

fn escape<B: WorkBudget>(
    bytes: &[u8],
    at: &mut usize,
    budget: &B,
) -> Result<Option<char>, TextError> {
    // bca: suppress(abc, halstead, nexits) -- exhaustive Rust escape dispatch includes fallible token reads and continuation removal.
    *at += 1;
    let byte = *bytes.get(*at).ok_or_else(invalid)?;
    budget.bytes(1).map_err(Into::into)?;
    *at += 1;
    let scalar = match byte {
        b'"' => '"',
        b'\'' => '\'',
        b'\\' => '\\',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'0' => '\0',
        b'x' => {
            let high = digit(*bytes.get(*at).ok_or_else(invalid)?)?;
            let low = digit(*bytes.get(*at + 1).ok_or_else(invalid)?)?;
            budget.bytes(2).map_err(Into::into)?;
            *at += 2;
            let value = high * 16 + low;
            if value > 0x7f {
                return Err(invalid());
            }
            char::from_u32(value).ok_or_else(invalid)?
        }
        b'u' => unicode(bytes, at, budget)?,
        b'\n' | b'\r' => {
            if byte == b'\r' {
                if bytes.get(*at) != Some(&b'\n') {
                    return Err(invalid());
                }
                budget.bytes(1).map_err(Into::into)?;
                *at += 1;
            }
            while matches!(bytes.get(*at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                budget.bytes(1).map_err(Into::into)?;
                if bytes[*at] == b'\r' && bytes.get(*at + 1) != Some(&b'\n') {
                    return Err(invalid());
                }
                *at += 1;
            }
            return Ok(None);
        }
        _ => return Err(invalid()),
    };
    Ok(Some(scalar))
}

fn digit(byte: u8) -> Result<u32, TextError> {
    match byte {
        b'0'..=b'9' => Ok(u32::from(byte - b'0')),
        b'a'..=b'f' => Ok(u32::from(byte - b'a') + 10),
        b'A'..=b'F' => Ok(u32::from(byte - b'A') + 10),
        _ => Err(invalid()),
    }
}

fn unicode<B: WorkBudget>(bytes: &[u8], at: &mut usize, budget: &B) -> Result<char, TextError> {
    if bytes.get(*at) != Some(&b'{') {
        return Err(invalid());
    }
    *at += 1;
    let mut value = 0;
    let mut digits = 0;
    loop {
        budget.bytes(1).map_err(Into::into)?;
        let byte = *bytes.get(*at).ok_or_else(invalid)?;
        *at += 1;
        match byte {
            b'}' if digits != 0 => break,
            b'_' if digits != 0 => {}
            _ => {
                if digits == 6 {
                    return Err(invalid());
                }
                value = value * 16 + digit(byte)?;
                digits += 1;
            }
        }
    }
    char::from_u32(value).ok_or(TextError::Unavailable(UnavailableReason::NonScalar))
}

crate::observation::counter!(decoded_units, cfg(test));

#[cfg(test)]
mod tests;
