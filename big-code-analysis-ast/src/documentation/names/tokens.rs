//! Narrow lexical keys for exact static C-family signatures.
//! Each consumed input byte is charged once in this scanner, including delimiters.

use crate::documentation::SourceError;
use crate::documentation::budget::WorkBudget;

pub(super) fn tokens<'s, B: WorkBudget>(
    bytes: &'s [u8],
    budget: &B,
) -> Result<Option<Vec<&'s [u8]>>, SourceError> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        budget.bytes(1).map_err(Into::into)?;
        super::name_bytes::record();
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' {
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                budget.bytes(1).map_err(Into::into)?;
                super::name_bytes::record();
                at += 1;
            }
        } else if bytes[at..].starts_with(b"/*") {
            charge(1, budget)?;
            at += 2;
            while at < bytes.len() && !bytes[at..].starts_with(b"*/") {
                charge(1, budget)?;
                at += 1;
            }
            if at == bytes.len() {
                return Ok(None);
            }
            charge(2, budget)?;
            at += 2;
            continue;
        } else if bytes[at..].starts_with(b"&&") || bytes[at..].starts_with(b"::") {
            charge(1, budget)?;
            at += 2;
        } else if matches!(bytes[at], b'(' | b')' | b',' | b'*' | b'&') {
            at += 1;
        } else {
            return Ok(None);
        }
        budget.step().map_err(Into::into)?;
        result.push(&bytes[start..at]);
    }
    Ok(Some(result))
}

fn charge<B: WorkBudget>(count: usize, budget: &B) -> Result<(), SourceError> {
    budget.bytes(count).map_err(Into::into)?;
    for _ in 0..count {
        super::name_bytes::record();
    }
    Ok(())
}

pub(super) fn reference_name<'s, B: WorkBudget>(
    tokens: &[&'s [u8]],
    budget: &B,
) -> Result<Option<&'s [u8]>, SourceError> {
    if tokens.len() == 1 && super::grammar::identifier(tokens[0]) {
        return Ok(Some(tokens[0]));
    }
    let mut open = None;
    for (at, token) in tokens.iter().enumerate() {
        budget.step().map_err(Into::into)?;
        if *token == b"::" {
            return Ok(None);
        }
        if *token == b"(" {
            if open.is_some() {
                return Ok(None);
            }
            open = Some(at);
        }
        if *token == b")" && at + 1 != tokens.len() {
            return Ok(None);
        }
    }
    let Some(open) = open else {
        return Ok(None);
    };
    if open < 2 || tokens.last() != Some(&b")".as_slice()) {
        return Ok(None);
    }
    let name = tokens[open - 1];
    if !super::grammar::identifier(name)
        || !super::grammar::return_type(&tokens[..open - 1], budget)?
    {
        return Ok(None);
    }
    Ok(super::grammar::parameters(&tokens[open + 1..tokens.len() - 1], budget)?.then_some(name))
}
