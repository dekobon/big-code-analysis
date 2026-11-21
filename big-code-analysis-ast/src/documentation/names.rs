//! Bounded same-file exact structural names and signatures.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

use super::budget::WorkBudget;
use super::canonical::NodeId;
use super::qualifications::QualificationSet;
use super::{OwnerId, SourceError, UnresolvedReason};

#[cfg(test)]
mod budget_tests;
mod grammar;
#[cfg(test)]
mod miss_tests;
mod tokens;

#[derive(Default)]
pub(super) struct Names {
    identifiers: HashMap<(NodeId, u64), Vec<NameEntry>>,
    signatures: HashMap<(NodeId, u64), Vec<NameEntry>>,
    candidates: Vec<NamedOwner>,
    hasher: RandomState,
}

struct NameEntry {
    spelling: Vec<u8>,
    owners: Vec<CandidateId>,
    complex: Vec<CandidateId>,
}

#[derive(Clone, Copy)]
struct CandidateId(usize);

#[derive(Clone, Copy)]
pub(super) struct NamedOwner {
    pub(super) owner: OwnerId,
    pub(super) qualification: QualificationSet,
}

pub(super) struct NameResolution {
    pub(super) owners: Vec<NamedOwner>,
    pub(super) reason: Option<UnresolvedReason>,
    pub(super) pending: bool,
}

crate::observation::counter!(name_bytes, cfg(any(test, feature = "test-support")));
crate::observation::counter!(name_candidates, cfg(any(test, feature = "test-support")));
crate::observation::counter!(key_bytes_stored, cfg(any(test, feature = "test-support")));

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
    let Some(tokens) = tokens::tokens(bytes, budget)? else {
        return Ok((0, Vec::new()));
    };
    emit_key(&tokens, hasher, budget)
}

fn emit_key<B: WorkBudget>(
    tokens: &[&[u8]],
    hasher: &RandomState,
    budget: &B,
) -> Result<(u64, Vec<u8>), SourceError> {
    let mut hash = hasher.build_hasher();
    let mut key = Vec::new();
    for (position, token) in tokens.iter().enumerate() {
        budget.step().map_err(Into::into)?;
        if position != 0 {
            budget.bytes(2).map_err(Into::into)?;
            name_bytes::record();
            name_bytes::record();
            hash.write_u8(0);
            key.push(0);
        }
        for &byte in *token {
            budget.bytes(2).map_err(Into::into)?;
            name_bytes::record();
            name_bytes::record();
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
    owner: CandidateId,
    complex: bool,
    budget: &B,
) -> Result<(), SourceError> {
    for entry in bucket.iter_mut() {
        budget.step().map_err(Into::into)?;
        if same_spelling(&entry.spelling, &spelling, budget)? {
            entry.owners.push(owner);
            if complex {
                entry.complex.push(owner);
            }
            return Ok(());
        }
    }
    for _ in &spelling {
        budget.bytes(1).map_err(Into::into)?;
        key_bytes_stored::record();
    }
    bucket.push(NameEntry {
        spelling,
        owners: vec![owner],
        complex: if complex { vec![owner] } else { Vec::new() },
    });
    Ok(())
}

fn lookup<'a, B: WorkBudget>(
    bucket: Option<&'a Vec<NameEntry>>,
    key: &[u8],
    budget: &B,
) -> Result<Option<&'a NameEntry>, SourceError> {
    if let Some(bucket) = bucket {
        for entry in bucket {
            budget.step().map_err(Into::into)?;
            if same_spelling(&entry.spelling, key, budget)? {
                return Ok(Some(entry));
            }
        }
    }
    Ok(None)
}

impl Names {
    #[cfg(test)]
    pub(super) fn insert<B: WorkBudget>(
        &mut self,
        name: &[u8],
        signature: &[u8],
        owner: OwnerId,
        budget: &B,
    ) -> Result<(), SourceError> {
        self.insert_candidate(
            name,
            Some(signature),
            NodeId(0),
            NamedOwner {
                owner,
                qualification: QualificationSet::default(),
            },
            budget,
        )
    }

    pub(super) fn insert_candidate<B: WorkBudget>(
        &mut self,
        name: &[u8],
        signature: Option<&[u8]>,
        scope: NodeId,
        owner: NamedOwner,
        budget: &B,
    ) -> Result<(), SourceError> {
        let (hash, name) = key(name, &self.hasher, budget)?;
        let signature = match signature {
            Some(signature) => {
                if let Some(tokens) = tokens::tokens(signature, budget)?
                    && tokens.len() > 1
                    && tokens::reference_name(&tokens, budget)?.is_some()
                {
                    Some(key(signature, &self.hasher, budget)?)
                } else {
                    None
                }
            }
            None => None,
        };
        let signature = signature.filter(|(_, key)| !key.is_empty());
        let id = CandidateId(self.candidates.len());
        self.candidates.push(owner);
        insert_entry(
            self.identifiers.entry((scope, hash)).or_default(),
            name,
            id,
            signature.is_none(),
            budget,
        )?;
        if let Some((hash, spelling)) = signature {
            insert_entry(
                self.signatures.entry((scope, hash)).or_default(),
                spelling,
                id,
                false,
                budget,
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn resolve<B: WorkBudget>(
        &self,
        reference: &[u8],
        budget: &B,
    ) -> Result<(Vec<OwnerId>, Option<UnresolvedReason>), SourceError> {
        let result = self.resolve_in(reference, NodeId(0), budget)?;
        Ok((
            result.owners.into_iter().map(|owner| owner.owner).collect(),
            result.reason,
        ))
    }

    pub(super) fn resolve_in<B: WorkBudget>(
        &self,
        reference: &[u8],
        scope: NodeId,
        budget: &B,
    ) -> Result<NameResolution, SourceError> {
        let Some(tokens) = tokens::tokens(reference, budget)? else {
            return Ok(NameResolution {
                owners: Vec::new(),
                reason: None,
                pending: true,
            });
        };
        let Some(name) = tokens::reference_name(&tokens, budget)? else {
            return Ok(NameResolution {
                owners: Vec::new(),
                reason: None,
                pending: true,
            });
        };
        let (hash, key) = key(reference, &self.hasher, budget)?;
        let signatures = tokens.len() > 1;
        let index = if signatures {
            &self.signatures
        } else {
            &self.identifiers
        };
        let mut owners = Vec::new();
        let mut pending = false;
        if let Some(entry) = lookup(index.get(&(scope, hash)), &key, budget)? {
            if !signatures {
                pending = !entry.complex.is_empty();
            }
            for &owner in &entry.owners {
                budget.step().map_err(Into::into)?;
                owners.push(self.candidates[owner.0]);
            }
        }
        if signatures {
            let (name_hash, name_key) = key_name(name, &self.hasher, budget)?;
            if let Some(entry) =
                lookup(self.identifiers.get(&(scope, name_hash)), &name_key, budget)?
            {
                pending = !entry.complex.is_empty();
                for &owner in &entry.complex {
                    budget.step().map_err(Into::into)?;
                    owners.push(self.candidates[owner.0]);
                }
            }
        }
        let reason = match owners.len() {
            0 => Some(UnresolvedReason::MissingName),
            1 => owners[0].qualification.headline(),
            _ => Some(UnresolvedReason::Ambiguous),
        };
        Ok(NameResolution {
            owners,
            reason,
            pending,
        })
    }
}

fn key_name<B: WorkBudget>(
    name: &[u8],
    hasher: &RandomState,
    budget: &B,
) -> Result<(u64, Vec<u8>), SourceError> {
    key(name, hasher, budget)
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
        names.candidates = vec![
            NamedOwner {
                owner: OwnerId(0),
                qualification: QualificationSet::default(),
            },
            NamedOwner {
                owner: OwnerId(1),
                qualification: QualificationSet::default(),
            },
        ];
        names.identifiers.insert(
            (NodeId(0), hash),
            vec![
                NameEntry {
                    spelling: b"g".to_vec(),
                    owners: vec![CandidateId(0)],
                    complex: Vec::new(),
                },
                NameEntry {
                    spelling: b"f".to_vec(),
                    owners: vec![CandidateId(1)],
                    complex: Vec::new(),
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
