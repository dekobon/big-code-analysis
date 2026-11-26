//! Source-backed documentation records, independent of metric policy.
//!
//! Role/context predicates recognize candidates; they do not supply semantic
//! owners. Fragments add included source spelling, documents add composition
//! and content state, and associations add profile-qualified attachment.
//! Collection retains immutable source facts, local text and prepared owner queries.
//! All items are unstable direct-AST plumbing.
//!
//! The collector stores flat node, occurrence and membership arenas. Text and
//! maps are materialized once per document. Structural work is linear in visits,
//! contributing bytes and memberships, with separate range lookups
//! `O((N + F) log(R + 1) + I)`, cached sibling-position searches, and dependency
//! field-lookup costs. Numeric document/association/fragment ordering adds its
//! respective `O(M log M)` terms. Name indexing hashes and compares spelling bytes
//! incrementally; equal spellings share a bucket entry. Prepared owner queries
//! inspect only their indexed associations and linked pending scopes. Individual
//! dependency calls and allocation are not internally interruptible; crate-owned
//! scans use the collection work budget.
//! Complex C-family headers retain pending named matching; simple keys preserve
//! lexical token boundaries and stay within the canonical source scope. Conditional
//! and recovery provenance qualifies named candidates. Comment rendering clips the
//! fragment's retained intersections monotonically rather than searching global
//! included ranges for each emitted segment. Cancellation checks bound work between
//! polls; releasing accumulated partial state still costs its retained storage.
//! Known candidate/reference qualifications are separate immutable records, indexed
//! by document and owner. Ambiguity preserves each candidate's conditional/recovery
//! facts; pending assessments describe unsupported matching rather than those facts.
//! Rust direct cooked/raw doc attributes share whole-document cleanup with comments.
//! Decoded-coordinate cleanup composes retained escape origins; metadata and computed
//! carriers remain pending, independently of existing owner projections and publication.

pub(crate) mod budget;
pub(crate) mod byte_scan;
mod canonical;
pub(crate) mod collector;
mod content;
mod forms;
mod index;
mod names;
mod options;
mod ordering;
mod owners;
mod qualifications;
pub(crate) mod recognition;
#[cfg(any(test, feature = "test-support"))]
pub mod render_probe;
mod source;
#[doc(hidden)]
pub mod streaming;
mod text;

pub use content::*;
pub use index::*;
pub use options::*;
pub use owners::*;
pub use qualifications::{Qualification, QualificationSubject};
pub use source::*;

/// Current-thread production work counters for source-path qualification.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug)]
pub struct Observations {
    /// Actual collector cancellation checkpoints.
    pub polls: usize,
    /// Canonical lookup candidates validated.
    pub identity_probes: usize,
    /// Primary structural visits.
    pub visits: usize,
    /// Primary source-owner classification requests.
    pub target_classifications: usize,
    /// Materialized owner projections.
    pub owner_projections: usize,
    /// Enumerated child batches.
    pub child_batches: usize,
    /// Required cursor-field requests.
    pub field_requests: usize,
    /// Completed text renders.
    pub renders: usize,
    /// Bounded source-to-text copy chunks.
    pub copy_chunks: usize,
    /// Consumed tokenizer bytes, key hashing/copying bytes (including separators),
    /// and spelling-comparison bytes. Each key output byte charges hash and copy.
    pub name_bytes: usize,
    /// Distinct name candidates compared inside hash buckets.
    pub name_candidates: usize,
    /// Normalized name/signature key bytes retained across distinct entries.
    pub key_bytes_stored: usize,
    /// Actual source-target requests, including binding callbacks.
    pub target_queries: usize,
    /// Source-target cache misses that execute classification.
    pub target_computations: usize,
    /// Byte visits in shared classifier scans.
    pub classifier_bytes: usize,
    /// Calls that charged the collection budget for source bytes.
    pub byte_charges: usize,
    /// Outer Doxygen structural scanner iterations.
    pub structural_bytes: usize,
    /// Doxygen structural classifier calls.
    pub structural_calls: usize,
    /// Global source-intersection binary-search comparisons.
    pub intersection_searches: usize,
    /// Monotone retained-fragment piece visits during text emission.
    pub fragment_piece_visits: usize,
    /// Pending-scope query links and records visited.
    pub assessment_scope_reads: usize,
    /// Dependency field lookups made by recognition callbacks.
    pub field_lookups: usize,
    /// Dependency indexed child lookups made by recognition callbacks.
    pub child_lookups: usize,
    /// Cached per-node answers probed, including source classification.
    pub memo_probes: usize,
    /// Ancestor fold state slots examined.
    pub fold_slot_probes: usize,
    /// Recorded sibling positions compared.
    pub sibling_slot_probes: usize,
    /// Association records consumed by prepared queries.
    pub association_reads: usize,
    /// Qualification records consumed by prepared queries.
    pub qualification_reads: usize,
    /// Qualification memberships indexed during collection.
    pub qualification_memberships: usize,
    /// Output-map binary-search comparisons.
    pub map_searches: usize,
    /// Output-map runs consumed by a query.
    pub map_runs_visited: usize,
    /// Direct Rust carrier children examined during attribute recognition.
    pub carrier_children: usize,
    /// Direct Rust attribute children examined during literal classification.
    pub attribute_children: usize,
    /// Responsible source pieces copied with the work budget.
    pub piece_copies: usize,
    /// Outer Rust literal decoder iterations, excluding inner-byte charges.
    pub decoded_units: usize,
    /// Retained mapping runs examined by decoded-coordinate cleanup.
    pub projection_visits: usize,
}

/// Read qualification counters without resetting other consumers' observations.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn observations() -> Observations {
    Observations {
        polls: budget::polls::observed(),
        identity_probes: canonical::identity_probes::observed(),
        visits: collector::visits::observed(),
        target_classifications: collector::target_classifications::observed(),
        owner_projections: collector::owner_projections::observed(),
        child_batches: collector::child_batches::observed(),
        field_requests: collector::field_requests::observed(),
        renders: text::renders::observed(),
        copy_chunks: text::copy_chunks::observed(),
        name_bytes: names::name_bytes::observed(),
        name_candidates: names::name_candidates::observed(),
        key_bytes_stored: names::key_bytes_stored::observed(),
        target_queries: recognition::target_queries::observed(),
        target_computations: recognition::target_computations::observed(),
        classifier_bytes: byte_scan::bytes_read::observed(),
        byte_charges: budget::byte_charges::observed(),
        structural_bytes: crate::lang_helpers::doxygen::structural_bytes::observed(),
        structural_calls: crate::lang_helpers::doxygen::structural_calls::observed(),
        intersection_searches: source::intersection_searches::observed(),
        fragment_piece_visits: text::source_cursor::fragment_piece_visits::observed(),
        assessment_scope_reads: index::assessment_scope_reads::observed(),
        field_lookups: crate::node::field_lookups::observed(),
        child_lookups: crate::node::child_lookups::observed(),
        memo_probes: crate::node::memo_probes::observed(),
        fold_slot_probes: crate::node::fold_slot_probes::observed(),
        sibling_slot_probes: crate::node::sibling_slot_probes::observed(),
        association_reads: index::association_reads::observed(),
        qualification_reads: qualifications::qualification_reads::observed(),
        qualification_memberships: collector::qualifications::memberships::observed(),
        map_searches: content::map_searches::observed(),
        map_runs_visited: content::map_runs_visited::observed(),
        carrier_children: collector::carrier_children::observed(),
        attribute_children: collector::attribute_children::observed(),
        piece_copies: source::piece_copies::observed(),
        decoded_units: text::rust::literal::decoded_units::observed(),
        projection_visits: text::cleanup::projection_visits::observed(),
    }
}

/// Collect source-attached documentation from a borrowed parse.
///
/// The tree must be a complete parse of these exact bytes with the selected
/// grammar. Grammar and buffer bounds are checked, but same-length substituted
/// source, shared-grammar dialect identity, inaccurate source points and historical
/// parse cancellation cannot be reconstructed from a raw tree. Prefer a typed
/// parser's documentation method when retained completion provenance is available.
/// Tree-sitter's included-range bulk copy, individual FFI calls and allocation
/// calls cannot be interrupted internally.
///
/// # Errors
/// Returns cancellation, disabled-language, profile/adapter availability, grammar
/// mismatch and detectable source-context failures before publishing an index.
pub fn collect<'s>(
    language: crate::LANG,
    tree: &'s tree_sitter::Tree,
    source: &'s [u8],
    context: &DocumentationContext,
    options: &DocumentationOptions,
) -> Result<DocumentationIndex<'s>, CollectionError> {
    if crate::cancel::requested() {
        return Err(CollectionError::Cancelled);
    }
    crate::langs::collect_documentation(language, tree, source, context, options)
}

/// One physical occurrence, whose spelling may be narrower than its syntax node.
#[derive(Debug)]
#[non_exhaustive]
pub struct Fragment<'s> {
    pub(crate) source: SourcePieces<'s>,
    pub(crate) node: Option<NodeEvidence<'s>>,
    pub(crate) form: FragmentForm,
}

impl<'s> Fragment<'s> {
    /// Included raw spelling, available even when decoding is unavailable.
    #[must_use]
    pub fn source(&self) -> &SourcePieces<'s> {
        &self.source
    }

    /// Exact syntax evidence, not necessarily the fragment's spelling range.
    #[must_use]
    pub fn node(&self) -> Option<NodeEvidence<'s>> {
        self.node
    }

    /// The syntax form, independent of the document's semantic role.
    #[must_use]
    pub fn form(&self) -> FragmentForm {
        self.form
    }
}

/// The spelling carrying a documentation occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FragmentForm {
    /// A line or block comment.
    Comment,
    /// A string literal, including its delimiters.
    Literal,
    /// A whole documentation attribute, including its value syntax.
    Attribute,
    /// POD spelling inside an opaque syntax node.
    Pod,
    /// A literal Tcl namespace preamble word.
    Preamble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FragmentId(pub(crate) usize);
#[derive(Clone, Copy, Debug)]
pub(crate) struct DocumentId(pub(crate) usize);
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnerId(pub(crate) usize);

/// One logical document, stored once regardless of owner fanout.
#[derive(Debug)]
#[non_exhaustive]
pub struct Document<'s> {
    pub(crate) fragments: Vec<FragmentId>,
    pub(crate) composition: Composition,
    pub(crate) role: DocumentRole,
    pub(crate) content: Content<'s>,
}

impl<'s> Document<'s> {
    /// Number of ordered physical occurrences composing this document.
    #[must_use]
    pub fn fragment_count(&self) -> usize {
        self.fragments.len()
    }

    /// How ordered fragments compose; additional Python strings are documents
    /// of their own, not a concatenation mode.
    #[must_use]
    pub fn composition(&self) -> Composition {
        self.composition
    }

    /// The semantic documentation role.
    #[must_use]
    pub fn role(&self) -> DocumentRole {
        self.role
    }

    /// Local text state, independent of whether an owner resolved.
    #[must_use]
    pub fn content(&self) -> &Content<'s> {
        &self.content
    }
}

/// Ordered physical-fragment composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Composition {
    /// One physical occurrence.
    Single,
    /// Adjacent static literals with no separator.
    LiteralConcatenation,
    /// A selected comment run or gathered blocks.
    CommentRun,
    /// Language-defined composition of comments and attributes.
    AttributeSequence,
}

/// Semantic roles in the contract's explicit source-order tie-break order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentRole {
    /// Primary item or module string documentation.
    Primary,
    /// A separate additional Python string expression.
    Additional,
    /// A PEP 258 string describing a binding.
    AttributeBinding,
    /// A Sphinx attribute comment.
    Sphinx,
    /// Documentation preceding a declaration.
    LeadingComment,
    /// Documentation in a declaration's body.
    BodyComment,
    /// File or package documentation.
    FilePackage,
    /// Documentation declaring a virtual target.
    Virtual,
    /// Explicitly opted-in BCA inference.
    Inferred,
}

impl DocumentRole {
    /// Stable role rank within a source-order key, not an enum discriminant.
    #[must_use]
    pub const fn source_rank(self) -> u8 {
        match self {
            Self::Primary => 0,
            Self::Additional => 1,
            Self::AttributeBinding => 2,
            Self::Sphinx => 3,
            Self::LeadingComment => 4,
            Self::BodyComment => 5,
            Self::FilePackage => 6,
            Self::Virtual => 7,
            Self::Inferred => 8,
        }
    }
}

/// Immutable record storage. Construction and indexed owner lookup belong to
/// the collector; an empty store does not claim a completed collection.
#[derive(Debug, Default)]
pub struct Facts<'s> {
    scope: u8,
    pub(crate) fragments: Vec<Fragment<'s>>,
    pub(crate) documents: Vec<Document<'s>>,
    pub(crate) owners: Vec<Owner<'s>>,
    pub(crate) associations: Vec<Association<'s>>,
}

/// An index-local borrowed occurrence. Equality is identity, never text equality.
/// Moving storage before borrowing it is valid. No persistent ID or pointer
/// ordering is exposed, and two stores over one tree have distinct identities.
#[derive(Debug)]
pub struct FactRef<'i, T> {
    scope: &'i u8,
    ordinal: usize,
    fact: &'i T,
}

impl<T> Copy for FactRef<'_, T> {}
impl<T> Clone for FactRef<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for FactRef<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.scope, other.scope) && self.ordinal == other.ordinal
    }
}
impl<T> Eq for FactRef<'_, T> {}
impl<'i, T> FactRef<'i, T> {
    /// Borrow the retained record without reconstruction.
    #[must_use]
    pub fn get(self) -> &'i T {
        self.fact
    }
}

impl<'s> Facts<'s> {
    fn reference<'i, T>(&'i self, records: &'i [T], ordinal: usize) -> Option<FactRef<'i, T>> {
        Some(FactRef {
            scope: &self.scope,
            ordinal,
            fact: records.get(ordinal)?,
        })
    }

    fn references<'i, T>(
        &'i self,
        records: &'i [T],
    ) -> impl ExactSizeIterator<Item = FactRef<'i, T>> {
        records.iter().enumerate().map(|(ordinal, fact)| FactRef {
            scope: &self.scope,
            ordinal,
            fact,
        })
    }

    /// Physical occurrences in retained order.
    pub fn fragments(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Fragment<'s>>> {
        self.references(&self.fragments)
    }

    /// Logical documents in retained order.
    pub fn documents(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Document<'s>>> {
        self.references(&self.documents)
    }

    /// Semantic owners, independently of metric spaces.
    pub fn owners(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Owner<'s>>> {
        self.references(&self.owners)
    }

    /// Profile-qualified attachment records in retained order.
    pub fn associations(&self) -> impl ExactSizeIterator<Item = FactRef<'_, Association<'s>>> {
        self.references(&self.associations)
    }

    /// The fragment at a composition position, or an identity/bounds error.
    ///
    /// # Errors
    /// Rejects handles from another store and absent composition positions.
    pub fn document_fragment(
        &self,
        document: FactRef<'_, Document<'s>>,
        position: usize,
    ) -> Result<FactRef<'_, Fragment<'s>>, FactError> {
        self.check_scope(document)?;
        let id = document
            .fact
            .fragments
            .get(position)
            .ok_or(FactError::MissingRecord)?;
        self.reference(&self.fragments, id.0)
            .ok_or(FactError::MissingRecord)
    }

    /// The shared document referred to by an association.
    ///
    /// # Errors
    /// Rejects foreign handles or an invalid internal document reference.
    pub fn associated_document(
        &self,
        association: FactRef<'_, Association<'s>>,
    ) -> Result<FactRef<'_, Document<'s>>, FactError> {
        self.check_scope(association)?;
        self.reference(&self.documents, association.fact.document.0)
            .ok_or(FactError::MissingRecord)
    }

    /// One resolved owner or uncertain candidate, in the association's order.
    /// Consult `resolution` before treating a candidate as an owner claim.
    ///
    /// # Errors
    /// Rejects foreign handles or an absent owner position.
    pub fn associated_owner(
        &self,
        association: FactRef<'_, Association<'s>>,
        position: usize,
    ) -> Result<FactRef<'_, Owner<'s>>, FactError> {
        self.check_scope(association)?;
        let id = association
            .fact
            .owners
            .get(position)
            .ok_or(FactError::MissingRecord)?;
        self.reference(&self.owners, id.0)
            .ok_or(FactError::MissingRecord)
    }

    /// A slot's enclosing semantic owner, distinct from lexical scope.
    ///
    /// # Errors
    /// Rejects a foreign handle, a non-slot owner or an invalid enclosure.
    pub fn slot_enclosing(
        &self,
        owner: FactRef<'_, Owner<'s>>,
    ) -> Result<FactRef<'_, Owner<'s>>, FactError> {
        self.check_scope(owner)?;
        let Owner::Slot(slot) = owner.fact else {
            return Err(FactError::MissingRecord);
        };
        self.reference(&self.owners, slot.enclosing.0)
            .ok_or(FactError::MissingRecord)
    }

    /// Source ordering key, kept separate from occurrence equality.
    ///
    /// # Errors
    /// Rejects foreign handles and a document without a physical fragment.
    pub fn document_order(
        &self,
        document: FactRef<'_, Document<'s>>,
    ) -> Result<(usize, usize, u8, usize), FactError> {
        let fragment = self.document_fragment(document, 0)?;
        let first = fragment
            .fact
            .source
            .pieces()
            .first()
            .ok_or(FactError::MissingRecord)?;
        let range = first.range();
        let end = fragment
            .fact
            .source
            .pieces()
            .last()
            .ok_or(FactError::MissingRecord)?
            .range()
            .end;
        Ok((
            range.start,
            end,
            document.fact.role.source_rank(),
            document.ordinal,
        ))
    }

    fn check_scope<T>(&self, reference: FactRef<'_, T>) -> Result<(), FactError> {
        if std::ptr::eq(&raw const self.scope, reference.scope) {
            Ok(())
        } else {
            Err(FactError::ForeignIndex)
        }
    }
}

/// Identity or storage lookup failure; not an empty owner query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FactError {
    /// A handle belongs to another store, even if it borrows the same tree.
    ForeignIndex,
    /// The requested record or composition position does not exist.
    MissingRecord,
}

#[cfg(test)]
mod tests;
