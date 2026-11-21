//! Store semantic provenance once, then prepare document/owner adjacency in order.

use super::*;
use crate::documentation::names::NamedOwner;
use crate::documentation::qualifications::QualificationSet;
crate::observation::counter!(memberships, cfg(any(test, feature = "test-support")));

impl<'s> Builder<'s> {
    pub(super) fn candidate_qualifications(
        &mut self,
        document: DocumentId,
        candidate: NamedOwner,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let node = self.index.canonical.nodes[self.index.owner_nodes[candidate.owner.0].0].evidence;
        for (rank, reason) in candidate.qualification.reasons() {
            budget.step()?;
            self.index.qualifications.push(Qualification {
                subject: QualificationSubject::Candidate,
                reason,
                node,
                document,
                owner: Some(candidate.owner),
                rank,
            });
        }
        Ok(())
    }

    pub(super) fn reference_qualifications(
        &mut self,
        document: DocumentId,
        node: NodeEvidence<'s>,
        qualification: QualificationSet,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        for (rank, reason) in qualification.reasons() {
            budget.step()?;
            self.index.qualifications.push(Qualification {
                subject: QualificationSubject::Reference,
                reason,
                node,
                document,
                owner: None,
                rank,
            });
        }
        Ok(())
    }

    pub(super) fn order_qualifications(
        &mut self,
        document_ranks: &[usize],
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let mut keys = Vec::new();
        for (ordinal, qualification) in self.index.qualifications.iter().enumerate() {
            budget.step()?;
            keys.push((
                document_ranks[qualification.document.0],
                qualification.node.node().start_byte(),
                qualification.node.node().end_byte(),
                qualification.rank,
                ordinal,
            ));
        }
        for (_, _, _, _, ordinal) in super::super::ordering::ordered(keys, budget)? {
            budget.step()?;
            let qualification = &self.index.qualifications[ordinal];
            self.index.qualifications_ordered.push(ordinal);
            self.index.qualifications_by_document[qualification.document.0].push(ordinal);
            memberships::record();
            if let Some(owner) = qualification.owner {
                self.index.qualifications_by_owner[owner.0].push(ordinal);
                memberships::record();
                self.index.canonical.nodes[self.index.owner_nodes[owner.0].0]
                    .qualifications
                    .push(ordinal);
                memberships::record();
            }
        }
        Ok(())
    }
}
