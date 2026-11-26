//! Fallible byte-slice scans used by recognition and text kernels.

use super::budget::WorkBudget;

crate::observation::counter!(bytes_read, cfg(any(test, feature = "test-support")));

/// Charges one byte that the caller reads itself, in a scan that keeps a
/// state of its own from byte to byte.
pub(crate) fn one<B: WorkBudget>(budget: &B) -> Result<(), B::Error> {
    budget.bytes(1)?;
    bytes_read::record();
    Ok(())
}

pub(crate) fn position<B: WorkBudget>(
    bytes: &[u8],
    predicate: impl Fn(u8) -> bool,
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    for (at, &byte) in bytes.iter().enumerate() {
        budget.bytes(1)?;
        bytes_read::record();
        if predicate(byte) {
            return Ok(Some(at));
        }
    }
    Ok(None)
}

pub(crate) fn rposition<B: WorkBudget>(
    bytes: &[u8],
    predicate: impl Fn(u8) -> bool,
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    for (at, &byte) in bytes.iter().enumerate().rev() {
        budget.bytes(1)?;
        bytes_read::record();
        if predicate(byte) {
            return Ok(Some(at));
        }
    }
    Ok(None)
}

pub(crate) fn trim_start<'s, B: WorkBudget>(
    bytes: &'s [u8],
    budget: &B,
) -> Result<&'s [u8], B::Error> {
    let first = position(bytes, |byte| !byte.is_ascii_whitespace(), budget)?.unwrap_or(bytes.len());
    Ok(&bytes[first..])
}

pub(crate) fn trim_end<'s, B: WorkBudget>(
    bytes: &'s [u8],
    budget: &B,
) -> Result<&'s [u8], B::Error> {
    let end = rposition(bytes, |byte| !byte.is_ascii_whitespace(), budget)?.map_or(0, |at| at + 1);
    Ok(&bytes[..end])
}

pub(crate) fn trim<'s, B: WorkBudget>(bytes: &'s [u8], budget: &B) -> Result<&'s [u8], B::Error> {
    trim_end(trim_start(bytes, budget)?, budget)
}

pub(crate) struct Lines<'s, 'b, B> {
    remaining: Option<&'s [u8]>,
    budget: &'b B,
}

impl<B> Clone for Lines<'_, '_, B> {
    fn clone(&self) -> Self {
        Self {
            remaining: self.remaining,
            budget: self.budget,
        }
    }
}

pub(crate) fn lines<'s, 'b, B: WorkBudget>(bytes: &'s [u8], budget: &'b B) -> Lines<'s, 'b, B> {
    Lines {
        remaining: Some(bytes),
        budget,
    }
}

impl<'s, B: WorkBudget> Iterator for Lines<'s, '_, B> {
    type Item = Result<&'s [u8], B::Error>;
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.remaining.take()?;
        Some(match position(bytes, |byte| byte == b'\n', self.budget) {
            Ok(Some(at)) => {
                self.remaining = Some(&bytes[at + 1..]);
                Ok(&bytes[..at])
            }
            Ok(None) => Ok(bytes),
            Err(error) => Err(error),
        })
    }
}

pub(crate) fn contains_ci<B: WorkBudget>(
    bytes: &[u8],
    needle: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    for window in bytes.windows(needle.len()) {
        budget.bytes(needle.len())?;
        if window.eq_ignore_ascii_case(needle) {
            return Ok(true);
        }
    }
    Ok(false)
}
