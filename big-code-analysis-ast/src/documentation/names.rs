//! Bounded same-file exact structural names and signatures.

use std::collections::HashMap;

use super::budget::WorkBudget;
use super::{OwnerId, SourceError, UnresolvedReason};

#[derive(Default)]
pub(super) struct Names {
    names: HashMap<u64, Vec<(Vec<u8>, OwnerId)>>,
    signatures: HashMap<u64, Vec<(Vec<u8>, OwnerId)>>,
}

fn key<B: WorkBudget>(bytes: &[u8], budget: &B) -> Result<(u64, Vec<u8>), SourceError> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut key = Vec::new();
    for &byte in bytes {
        budget.bytes(1).map_err(Into::into)?;
        if !byte.is_ascii_whitespace() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            key.push(byte);
        }
    }
    Ok((hash, key))
}

impl Names {
    pub(super) fn insert<B: WorkBudget>(
        &mut self,
        name: &[u8],
        signature: &[u8],
        owner: OwnerId,
        budget: &B,
    ) -> Result<(), SourceError> {
        let (hash, name) = key(name, budget)?;
        self.names.entry(hash).or_default().push((name, owner));
        let (hash, signature) = key(signature, budget)?;
        self.signatures
            .entry(hash)
            .or_default()
            .push((signature, owner));
        Ok(())
    }

    pub(super) fn resolve<B: WorkBudget>(
        &self,
        reference: &[u8],
        budget: &B,
    ) -> Result<(Vec<OwnerId>, Option<UnresolvedReason>), SourceError> {
        let (hash, key) = key(reference, budget)?;
        let mut signatures = false;
        for byte in &key {
            budget.bytes(1).map_err(Into::into)?;
            signatures |= *byte == b'(';
        }
        let index = if signatures {
            &self.signatures
        } else {
            &self.names
        };
        let mut owners = Vec::new();
        if let Some(candidates) = index.get(&hash) {
            for (candidate, owner) in candidates {
                budget.step().map_err(Into::into)?;
                if candidate.len() != key.len() {
                    continue;
                }
                let mut matches = true;
                for (left, right) in candidate.iter().zip(&key) {
                    budget.bytes(1).map_err(Into::into)?;
                    if left != right {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    owners.push(*owner);
                }
            }
        }
        let reason = match owners.len() {
            0 => Some(UnresolvedReason::MissingName),
            1 => None,
            _ => Some(UnresolvedReason::Ambiguous),
        };
        Ok((owners, reason))
    }
}
