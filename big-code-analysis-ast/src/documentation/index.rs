//! Immutable source-bound facts and prepared owner queries.

use super::canonical::{CanonicalNodes, Projection};
use super::*;

/// Whether the selected grammar has a source declaration model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Implemented source paths coexist with default-visible pending forms.
    Partial,
    /// A helper grammar has no declaration model.
    NoDeclarationModel,
}

/// Independent assessment dimensions; pending work is never semantic absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssessmentAxis {
    /// Recognition of a potential documentation carrier.
    Occurrence,
    /// Selection among recognized forms.
    Selection,
    /// Projection onto a semantic declaration or slot.
    Owner,
    /// Composition and local text of selected occurrences.
    Composition,
    /// Publication eligibility under the named profile.
    Publication,
}

/// A default-visible pending implementation assessment.
#[derive(Debug)]
pub struct Assessment<'s> {
    /// The unfinished decision, independent of other dimensions.
    pub axis: AssessmentAxis,
    /// Source form or implementation limitation.
    pub reason: &'static str,
    /// Canonical evidence when a specific occurrence is known.
    pub node: Option<NodeEvidence<'s>>,
    pub(super) document: Option<DocumentId>,
}

/// Retained nonselected source evidence, independently of pending assessments.
#[derive(Debug)]
pub struct CandidateEvidence {
    pub(super) fragment: FragmentId,
    /// Why this occurrence does not contribute to selected prose.
    pub disposition: CandidateDisposition,
}

/// Source-selection evidence, not an owner or publication conclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateDisposition {
    /// A later candidate replaced this occurrence.
    Superseded,
    /// The current profile established no attachment.
    Unattached,
    /// A recognized non-prose directive.
    Directive,
}

/// A caller-owned immutable index borrowing exactly one parse and source buffer.
#[derive(Debug)]
pub struct DocumentationIndex<'s> {
    pub(super) source: SourceContext<'s>,
    pub(super) facts: Facts<'s>,
    pub(super) canonical: CanonicalNodes<'s>,
    pub(super) context: DocumentationContext,
    pub(super) options: DocumentationOptions,
    pub(super) profile: Profile,
    pub(super) capability: Capability,
    pub(super) assessments: Vec<Assessment<'s>>,
    pub(super) evidence: Vec<CandidateEvidence>,
    pub(super) associations_by_owner: Vec<Vec<usize>>,
    pub(super) assessments_by_owner: Vec<Vec<usize>>,
    pub(super) owner_nodes: Vec<super::canonical::NodeId>,
    pub(super) documents_ordered: Vec<usize>,
    pub(super) associations_ordered: Vec<usize>,
    pub(super) source_cleaned: Vec<Option<Content<'s>>>,
    pub(super) qualifications: Vec<Qualification<'s>>,
    pub(super) qualifications_ordered: Vec<usize>,
    pub(super) qualifications_by_document: Vec<Vec<usize>>,
    pub(super) qualifications_by_owner: Vec<Vec<usize>>,
}

/// Prepared results for one semantic owner, including known associations and
/// pending assessments in separate borrowed sequences.
pub struct OwnerAssociations<'i, 's> {
    index: &'i DocumentationIndex<'s>,
    associations: &'i [usize],
    assessments: &'i [usize],
    scope: Option<super::canonical::NodeId>,
    projection_pending: bool,
    qualifications: &'i [usize],
}

/// A canonical syntax projection with its known owners and pending assessment.
/// Multi-slot declarations retain all owners without choosing one implicitly.
pub struct NodeProjection<'i, 's> {
    index: &'i DocumentationIndex<'s>,
    id: super::canonical::NodeId,
    owners: &'i [OwnerId],
    pending: bool,
}

impl<'i, 's> NodeProjection<'i, 's> {
    /// Whether semantic normalization remains incomplete.
    #[must_use]
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Known semantic owners, in stored source/slot order.
    pub fn owners(&self) -> impl ExactSizeIterator<Item = FactRef<'i, Owner<'s>>> + '_ {
        self.owners.iter().map(|id| FactRef {
            scope: &self.index.facts.scope,
            ordinal: id.0,
            fact: &self.index.facts.owners[id.0],
        })
    }

    /// Scope assessments affecting the queried syntax, including pending owners.
    pub fn assessments(&self) -> impl Iterator<Item = FactRef<'i, Assessment<'s>>> + '_ {
        scope_assessments(self.index, Some(self.id)).map(|ordinal| FactRef {
            scope: &self.index.facts.scope,
            ordinal,
            fact: &self.index.assessments[ordinal],
        })
    }
}

fn scope_assessments<'i>(
    index: &'i DocumentationIndex<'_>,
    mut scope: Option<super::canonical::NodeId>,
) -> impl Iterator<Item = usize> + 'i {
    let mut at = 0;
    std::iter::from_fn(move || {
        loop {
            assessment_scope_reads::record();
            let node = &index.canonical.nodes[scope?.0];
            if let Some(&assessment) = node.pending.get(at) {
                at += 1;
                return Some(assessment);
            }
            scope = node.pending_parent;
            at = 0;
        }
    })
}

crate::observation::counter!(
    assessment_scope_reads,
    cfg(any(test, feature = "test-support"))
);
crate::observation::counter!(association_reads, cfg(any(test, feature = "test-support")));

impl<'i, 's> OwnerAssociations<'i, 's> {
    /// Known conditional/recovery facts for this candidate, independently of
    /// association ambiguity and pending implementation assessments.
    pub fn qualifications(
        &self,
    ) -> impl ExactSizeIterator<Item = FactRef<'i, Qualification<'s>>> + '_ {
        self.index.qualification_refs(self.qualifications)
    }
    /// Whether the queried syntax still requires a semantic owner projection.
    /// Known associations and pending assessment records remain accessible.
    #[must_use]
    pub fn projection_pending(&self) -> bool {
        self.projection_pending
    }
    /// Known association records, without scanning other owners.
    pub fn associations(&self) -> impl ExactSizeIterator<Item = FactRef<'i, Association<'s>>> + '_ {
        self.associations.iter().map(|&ordinal| {
            association_reads::record();
            FactRef {
                scope: &self.index.facts.scope,
                ordinal,
                fact: &self.index.facts.associations[ordinal],
            }
        })
    }

    /// Pending assessments affecting this owner, visible under default options.
    pub fn assessments(&self) -> impl Iterator<Item = FactRef<'i, Assessment<'s>>> + '_ {
        self.assessments
            .iter()
            .copied()
            .chain(scope_assessments(self.index, self.scope))
            .map(|ordinal| FactRef {
                scope: &self.index.facts.scope,
                ordinal,
                fact: &self.index.assessments[ordinal],
            })
    }
}

/// Owner lookup outcomes distinct from a supported, assessed empty result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OwnerError {
    /// A node is outside this profile's declaration model.
    UnsupportedOwner,
    /// A recognized owner form awaits semantic projection.
    PendingOwner,
    /// This wrapper projects to more than one owner; enumerate those owners.
    MultipleOwners,
    /// Canonical node validation failed.
    Source(SourceError),
    /// A borrowed handle belongs to another index.
    Fact(FactError),
}

impl<'s> DocumentationIndex<'s> {
    /// Immutable physical records and association-navigation operations.
    #[must_use]
    pub fn facts(&self) -> &Facts<'s> {
        &self.facts
    }

    /// The exact parsed source bytes.
    #[must_use]
    pub fn source(&self) -> &'s [u8] {
        self.source.bytes()
    }

    /// Copied caller metadata, with no inferred filesystem provenance.
    #[must_use]
    pub fn context(&self) -> &DocumentationContext {
        &self.context
    }

    /// The selected collection options.
    #[must_use]
    pub fn options(&self) -> DocumentationOptions {
        self.options
    }

    /// The resolved named profile, never `Default`.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Partial source support or an explicit helper capability.
    #[must_use]
    pub fn capability(&self) -> Capability {
        self.capability
    }

    /// All pending assessments, including occurrence/owner/composition uncertainty.
    pub fn assessments(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Assessment<'s>>> {
        self.facts.references(&self.assessments)
    }

    /// Opt-in rejected, superseded and directive evidence in collection order.
    pub fn evidence(&self) -> impl ExactSizeIterator<Item = FactRef<'_, CandidateEvidence>> {
        self.facts.references(&self.evidence)
    }

    /// The physical spelling retained for a nonselected candidate.
    ///
    /// # Errors
    /// Rejects references from another index.
    pub fn evidence_fragment(
        &self,
        evidence: FactRef<'_, CandidateEvidence>,
    ) -> Result<FactRef<'_, Fragment<'s>>, FactError> {
        self.facts.check_scope(evidence)?;
        self.facts
            .reference(&self.facts.fragments, evidence.fact.fragment.0)
            .ok_or(FactError::MissingRecord)
    }

    /// The document affected by a retained assessment, if one has been composed.
    ///
    /// # Errors
    /// Rejects an assessment borrowed from another index.
    pub fn assessed_document(
        &self,
        assessment: FactRef<'_, Assessment<'s>>,
    ) -> Result<Option<FactRef<'_, Document<'s>>>, FactError> {
        self.facts.check_scope(assessment)?;
        Ok(assessment
            .fact
            .document
            .and_then(|id| self.facts.reference(&self.facts.documents, id.0)))
    }

    /// Documents in deterministic anchor, role and creation order.
    pub fn documents(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Document<'s>>> {
        self.documents_ordered.iter().map(|&ordinal| FactRef {
            scope: &self.facts.scope,
            ordinal,
            fact: &self.facts.documents[ordinal],
        })
    }

    /// Associations in document order, then owner anchor and slot order.
    pub fn associations(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Association<'s>>> {
        self.associations_ordered.iter().map(|&ordinal| {
            association_reads::record();
            FactRef {
                scope: &self.facts.scope,
                ordinal,
                fact: &self.facts.associations[ordinal],
            }
        })
    }

    /// Constant-time access to validated complete text. Other content states
    /// return `None` and remain inspectable on the retained document.
    ///
    /// # Errors
    /// Rejects a document reference obtained from another index.
    pub fn complete_text(
        &self,
        document: FactRef<'_, Document<'s>>,
    ) -> Result<Option<CompleteText<'_, 's>>, FactError> {
        self.facts.check_scope(document)?;
        Ok(match &self.facts.documents[document.ordinal].content {
            Content::Complete { text, mappings } => Some(CompleteText { text, mappings }),
            _ => None,
        })
    }

    /// Source-cleaned spelling retained where semantic prelexing is unsupported.
    /// This is explicitly not a complete decoded documentation value.
    ///
    /// # Errors
    /// Rejects a foreign document handle.
    pub fn source_cleaned_text(
        &self,
        document: FactRef<'_, Document<'s>>,
    ) -> Result<Option<CompleteText<'_, 's>>, FactError> {
        self.facts.check_scope(document)?;
        Ok(match &self.source_cleaned[document.ordinal] {
            Some(Content::Complete { text, mappings }) => Some(CompleteText { text, mappings }),
            _ => None,
        })
    }

    /// Prepared query using an index-qualified semantic owner handle.
    ///
    /// # Errors
    /// Rejects a handle from another index.
    pub fn owner_associations(
        &self,
        owner: FactRef<'_, Owner<'s>>,
    ) -> Result<OwnerAssociations<'_, 's>, OwnerError> {
        self.facts.check_scope(owner).map_err(OwnerError::Fact)?;
        Ok(self.prepared_owner(owner.ordinal))
    }

    /// Canonical-node convenience for a single-owner projection.
    ///
    /// # Errors
    /// Distinguishes identity failures, unsupported and pending forms, and
    /// wrappers projecting to several owners.
    pub fn node_associations(
        &self,
        node: tree_sitter::Node<'_>,
    ) -> Result<OwnerAssociations<'_, 's>, OwnerError> {
        let id = self.canonical.lookup(node).map_err(OwnerError::Source)?;
        match &self.canonical.nodes[id.0].projection {
            Projection::Ready(owners) if owners.is_empty() => Err(OwnerError::UnsupportedOwner),
            Projection::Pending => Ok(OwnerAssociations {
                index: self,
                associations: &[],
                assessments: &[],
                scope: Some(id),
                projection_pending: true,
                qualifications: &self.canonical.nodes[id.0].qualifications,
            }),
            Projection::Ready(owners) if owners.len() == 1 => {
                let ordinal = owners[0].0;
                Ok(self.prepared_owner(ordinal))
            }
            Projection::Ready(_) => Err(OwnerError::MultipleOwners),
            Projection::Unsupported => Err(OwnerError::UnsupportedOwner),
        }
    }

    /// Semantic owners projected from a canonical node, including multi-slot
    /// declarations. The projection is stored once during collection.
    ///
    /// # Errors
    /// Rejects noncanonical/foreign nodes and unsupported or pending forms.
    pub fn node_owners(
        &self,
        node: tree_sitter::Node<'_>,
    ) -> Result<impl ExactSizeIterator<Item = FactRef<'_, Owner<'s>>>, OwnerError> {
        let id = self.canonical.lookup(node).map_err(OwnerError::Source)?;
        let owners = match &self.canonical.nodes[id.0].projection {
            Projection::Ready(owners) => owners.as_slice(),
            Projection::Pending => return Err(OwnerError::PendingOwner),
            Projection::Unsupported => return Err(OwnerError::UnsupportedOwner),
        };
        Ok(owners.iter().map(|id| FactRef {
            scope: &self.facts.scope,
            ordinal: id.0,
            fact: &self.facts.owners[id.0],
        }))
    }

    /// Inspect a canonical node's complete or pending owner projection.
    ///
    /// # Errors
    /// Rejects foreign/noncanonical nodes and syntax outside the declaration model.
    pub fn node_projection(
        &self,
        node: tree_sitter::Node<'_>,
    ) -> Result<NodeProjection<'_, 's>, OwnerError> {
        let id = self.canonical.lookup(node).map_err(OwnerError::Source)?;
        let (owners, pending) = match &self.canonical.nodes[id.0].projection {
            Projection::Ready(owners) => (owners.as_slice(), false),
            Projection::Pending => (&[][..], true),
            Projection::Unsupported => return Err(OwnerError::UnsupportedOwner),
        };
        Ok(NodeProjection {
            index: self,
            id,
            owners,
            pending,
        })
    }

    fn prepared_owner(&self, ordinal: usize) -> OwnerAssociations<'_, 's> {
        let id = self.owner_nodes[ordinal];
        let node = &self.canonical.nodes[id.0];
        let scope = if node.pending.is_empty() {
            node.pending_parent
        } else {
            Some(id)
        };
        OwnerAssociations {
            index: self,
            associations: &self.associations_by_owner[ordinal],
            assessments: &self.assessments_by_owner[ordinal],
            scope,
            projection_pending: false,
            qualifications: &self.qualifications_by_owner[ordinal],
        }
    }
}
