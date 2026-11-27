//! Known source qualifications, separate from unfinished adapter assessments.

use super::*;
crate::observation::counter!(
    qualification_reads,
    cfg(any(test, feature = "test-support"))
);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct QualificationSet(u8);

impl QualificationSet {
    pub(super) fn insert(&mut self, reason: UnresolvedReason) {
        self.0 |= match reason {
            UnresolvedReason::Conditional => 1,
            UnresolvedReason::GrammarBoundary => 2,
            _ => 0,
        };
    }

    pub(super) fn headline(self) -> Option<UnresolvedReason> {
        if self.0 & 2 != 0 {
            Some(UnresolvedReason::GrammarBoundary)
        } else if self.0 & 1 != 0 {
            Some(UnresolvedReason::Conditional)
        } else {
            None
        }
    }

    pub(super) fn reasons(self) -> impl Iterator<Item = (u8, UnresolvedReason)> {
        [
            (0, UnresolvedReason::Conditional),
            (1, UnresolvedReason::GrammarBoundary),
        ]
        .into_iter()
        .filter(move |(rank, _)| self.0 & (1 << rank) != 0)
    }
}

/// Whether a known source qualification describes a candidate or the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QualificationSubject {
    /// The named declaration candidate; consult its association before claiming it.
    Candidate,
    /// The occurrence containing the explicit documentation reference.
    Reference,
}

/// One known conditional/recovery fact for a document and source occurrence.
/// These records are available by default and do not imply pending implementation.
#[derive(Debug)]
#[non_exhaustive]
pub struct Qualification<'s> {
    /// Candidate provenance versus reference-context provenance.
    pub subject: QualificationSubject,
    /// Known reason; currently Conditional or GrammarBoundary.
    pub reason: UnresolvedReason,
    /// Canonical candidate declaration or reference comment.
    pub node: NodeEvidence<'s>,
    pub(super) document: DocumentId,
    pub(super) owner: Option<OwnerId>,
    pub(super) rank: u8,
}

impl<'s> DocumentationIndex<'s> {
    /// All semantic qualifications in document, source and explicit reason order.
    pub fn qualifications(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Qualification<'s>>> {
        self.qualification_refs(&self.qualifications_ordered)
    }

    /// Known qualifications for one document, without scanning unrelated records.
    ///
    /// # Errors
    /// Rejects a document handle from another index.
    pub fn document_qualifications(
        &self,
        document: FactRef<'_, Document<'s>>,
    ) -> Result<impl ExactSizeIterator<Item = FactRef<'_, Qualification<'s>>>, FactError> {
        self.facts.check_scope(document)?;
        Ok(self.qualification_refs(&self.qualifications_by_document[document.ordinal]))
    }

    /// The document linked by a qualification.
    ///
    /// # Errors
    /// Rejects a qualification handle from another index.
    pub fn qualified_document(
        &self,
        qualification: FactRef<'_, Qualification<'s>>,
    ) -> Result<FactRef<'_, Document<'s>>, FactError> {
        self.facts.check_scope(qualification)?;
        self.facts
            .reference(&self.facts.documents, qualification.get().document.0)
            .ok_or(FactError::MissingRecord)
    }

    /// The candidate owner, or None for reference-context qualification.
    ///
    /// # Errors
    /// Rejects a qualification handle from another index.
    pub fn qualified_owner(
        &self,
        qualification: FactRef<'_, Qualification<'s>>,
    ) -> Result<Option<FactRef<'_, Owner<'s>>>, FactError> {
        self.facts.check_scope(qualification)?;
        Ok(qualification
            .get()
            .owner
            .and_then(|id| self.facts.reference(&self.facts.owners, id.0)))
    }

    pub(super) fn qualification_refs<'i>(
        &'i self,
        ids: &'i [usize],
    ) -> impl ExactSizeIterator<Item = FactRef<'i, Qualification<'s>>> {
        ids.iter().map(|&ordinal| {
            qualification_reads::record();
            FactRef {
                scope: &self.facts.scope,
                ordinal,
                fact: &self.qualifications[ordinal],
            }
        })
    }
}
