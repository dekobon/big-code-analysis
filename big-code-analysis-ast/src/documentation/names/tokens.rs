//! Narrow lexical keys for exact static C-family signatures.

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
            at += 2;
            while at + 1 < bytes.len() && !bytes[at..].starts_with(b"*/") {
                budget.bytes(1).map_err(Into::into)?;
                super::name_bytes::record();
                at += 1;
            }
            if at + 1 >= bytes.len() {
                return Ok(None);
            }
            at += 2;
            continue;
        } else if bytes[at..].starts_with(b"&&") || bytes[at..].starts_with(b"::") {
            at += 2;
        } else if matches!(bytes[at], b'(' | b')' | b',' | b'*' | b'&') {
            at += 1;
        } else {
            return Ok(None);
        }
        result.push(&bytes[start..at]);
    }
    Ok(Some(result))
}

pub(super) fn reference_name<'s, B: WorkBudget>(
    tokens: &[&'s [u8]],
    budget: &B,
) -> Result<Option<&'s [u8]>, SourceError> {
    if tokens.len() == 1 && identifier(tokens[0]) {
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
    Ok(identifier(name).then_some(name))
}

fn identifier(bytes: &[u8]) -> bool {
    bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
}
