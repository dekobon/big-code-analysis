use super::*;
use crate::documentation::budget::NoBudget;

fn miss_index(unrelated: usize, collision: bool) -> Names {
    let mut names = Names::default();
    let (hash, _) = key(b"absent", &names.hasher, &NoBudget).unwrap();
    for at in 0..unrelated {
        // Different scopes deterministically keep these buckets out of the query,
        // even when RandomState happens to assign equal hash values.
        let owner = CandidateId(names.candidates.len());
        names.candidates.push(NamedOwner {
            owner: OwnerId(at),
            qualification: QualificationSet::default(),
        });
        names.identifiers.insert(
            (NodeId(at + 1), hash),
            vec![NameEntry {
                spelling: b"absent".to_vec(),
                owners: vec![owner],
                complex: vec![],
            }],
        );
    }
    if collision {
        let first = CandidateId(names.candidates.len());
        let second = CandidateId(names.candidates.len() + 1);
        for ordinal in [unrelated, unrelated + 1] {
            names.candidates.push(NamedOwner {
                owner: OwnerId(ordinal),
                qualification: QualificationSet::default(),
            });
        }
        names.identifiers.insert(
            (NodeId(0), hash),
            vec![
                NameEntry {
                    spelling: b"absorb".to_vec(),
                    owners: vec![first],
                    complex: vec![],
                },
                NameEntry {
                    spelling: b"absentx".to_vec(),
                    owners: vec![second],
                    complex: vec![],
                },
            ],
        );
    }
    names
}

#[test]
fn absent_bucket_miss_never_compares_unrelated_names() {
    for unrelated in [0, 8, 64, 256] {
        let names = miss_index(unrelated, false);
        let before_bytes = name_bytes::observed();
        let before_comparisons = name_candidates::observed();
        let result = names.resolve_in(b"absent", NodeId(0), &NoBudget).unwrap();
        assert_eq!(result.reason, Some(UnresolvedReason::MissingName));
        assert!(result.owners.is_empty() && !result.pending);
        // Two six-byte token scans plus six bytes hashed and copied: 24.
        assert_eq!(name_bytes::observed() - before_bytes, 24);
        assert_eq!(name_candidates::observed() - before_comparisons, 0);
    }
}

#[test]
fn collision_bucket_miss_compares_only_its_nonmatching_spellings() {
    for unrelated in [0, 8, 64, 256] {
        let names = miss_index(unrelated, true);
        let before_bytes = name_bytes::observed();
        let before_comparisons = name_candidates::observed();
        let result = names.resolve_in(b"absent", NodeId(0), &NoBudget).unwrap();
        assert_eq!(result.reason, Some(UnresolvedReason::MissingName));
        assert!(result.owners.is_empty() && !result.pending);
        // Key processing is 24 bytes; `absorb` compares four bytes through
        // the differing fourth byte, and `absentx` fails on length alone.
        assert_eq!(name_bytes::observed() - before_bytes, 28);
        assert_eq!(name_candidates::observed() - before_comparisons, 2);
    }
}
