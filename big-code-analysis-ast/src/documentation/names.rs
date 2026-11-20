//! Bounded same-file exact structural names and signatures.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use super::budget::WorkBudget;
use super::{OwnerId, SourceError, UnresolvedReason};

#[derive(Default)]
pub(super) struct Names {
    identifiers: HashMap<u64, Vec<NameEntry>>,
    signatures: HashMap<u64, Vec<NameEntry>>,
    hasher: RandomState,
}

struct NameEntry {
    spelling: Vec<u8>,
    owners: Vec<OwnerId>,
}

crate::observation::counter!(name_bytes, cfg(any(test, feature = "test-support")));
crate::observation::counter!(name_candidates, cfg(any(test, feature = "test-support")));

fn key<B: WorkBudget>(
    bytes: &[u8],
    hasher: &RandomState,
    budget: &B,
) -> Result<(u64, Vec<u8>), SourceError> {
    super::budget::in_phase(budget, super::budget::WorkPhase::Names, || {
        normalized_key(bytes, hasher, budget)
    })
}

fn normalized_key<B: WorkBudget>(
    bytes: &[u8],
    hasher: &RandomState,
    budget: &B,
) -> Result<(u64, Vec<u8>), SourceError> {
    let mut hash = hasher.build_hasher();
    let mut key = Vec::new();
    for &byte in bytes {
        budget.bytes(1).map_err(Into::into)?;
        name_bytes::record();
        if !byte.is_ascii_whitespace() {
            hash.write_u8(byte);
            key.push(byte);
        }
    }
    Ok((hash.finish(), key))
}

fn same_spelling<B: WorkBudget>(
    left: &[u8],
    right: &[u8],
    budget: &B,
) -> Result<bool, SourceError> {
    name_candidates::record();
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        budget.bytes(1).map_err(Into::into)?;
        name_bytes::record();
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

fn insert_entry<B: WorkBudget>(
    bucket: &mut Vec<NameEntry>,
    spelling: Vec<u8>,
    owner: OwnerId,
    budget: &B,
) -> Result<(), SourceError> {
    for entry in bucket.iter_mut() {
        budget.step().map_err(Into::into)?;
        if same_spelling(&entry.spelling, &spelling, budget)? {
            entry.owners.push(owner);
            return Ok(());
        }
    }
    bucket.push(NameEntry {
        spelling,
        owners: vec![owner],
    });
    Ok(())
}

impl Names {
    pub(super) fn insert<B: WorkBudget>(
        &mut self,
        name: &[u8],
        signature: &[u8],
        owner: OwnerId,
        budget: &B,
    ) -> Result<(), SourceError> {
        let (hash, name) = key(name, &self.hasher, budget)?;
        insert_entry(
            self.identifiers.entry(hash).or_default(),
            name,
            owner,
            budget,
        )?;
        let (hash, signature) = key(signature, &self.hasher, budget)?;
        insert_entry(
            self.signatures.entry(hash).or_default(),
            signature,
            owner,
            budget,
        )?;
        Ok(())
    }

    pub(super) fn resolve<B: WorkBudget>(
        &self,
        reference: &[u8],
        budget: &B,
    ) -> Result<(Vec<OwnerId>, Option<UnresolvedReason>), SourceError> {
        let (hash, key) = key(reference, &self.hasher, budget)?;
        let mut signatures = false;
        for byte in &key {
            budget.bytes(1).map_err(Into::into)?;
            signatures |= *byte == b'(';
        }
        let index = if signatures {
            &self.signatures
        } else {
            &self.identifiers
        };
        let mut owners = Vec::new();
        if let Some(candidates) = index.get(&hash) {
            for entry in candidates {
                budget.step().map_err(Into::into)?;
                if same_spelling(&entry.spelling, &key, budget)? {
                    for &owner in &entry.owners {
                        budget.step().map_err(Into::into)?;
                        owners.push(owner);
                    }
                    break;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documentation::budget::NoBudget;

    #[test]
    fn duplicate_names_share_spelling_and_query_compares_once() {
        let mut names = Names::default();
        let name = b"a".repeat(4096);
        for ordinal in 0..128 {
            names
                .insert(&name, &name, OwnerId(ordinal), &NoBudget)
                .unwrap();
        }
        assert_eq!(names.identifiers.values().map(Vec::len).sum::<usize>(), 1);
        let entry = &names.identifiers.values().next().unwrap()[0];
        assert_eq!(entry.spelling.len(), 4096);
        assert_eq!(entry.owners.len(), 128);
        let before = name_candidates::observed();
        let (owners, reason) = names.resolve(&name, &NoBudget).unwrap();
        assert_eq!(reason, Some(UnresolvedReason::Ambiguous));
        assert_eq!(owners.len(), 128);
        assert_eq!(name_candidates::observed() - before, 1);
    }

    #[test]
    fn hash_collision_does_not_substitute_another_name() {
        let mut names = Names::default();
        let (hash, _) = key(b"f", &names.hasher, &NoBudget).unwrap();
        names.identifiers.insert(
            hash,
            vec![
                NameEntry {
                    spelling: b"g".to_vec(),
                    owners: vec![OwnerId(0)],
                },
                NameEntry {
                    spelling: b"f".to_vec(),
                    owners: vec![OwnerId(1)],
                },
            ],
        );
        let before = name_candidates::observed();
        let (owners, reason) = names.resolve(b"f", &NoBudget).unwrap();
        assert_eq!(reason, None);
        assert_eq!(owners.iter().map(|owner| owner.0).collect::<Vec<_>>(), [1]);
        assert_eq!(name_candidates::observed() - before, 2);
    }
}
