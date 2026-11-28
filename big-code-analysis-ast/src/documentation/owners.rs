//! Semantic owners and profile-qualified associations.

use super::{DocumentId, NodeEvidence, OwnerId, SourcePieces};

/// A semantic source owner; none of these variants implies a metric space.
#[derive(Debug)]
#[non_exhaustive]
pub enum Owner<'s> {
    /// A concrete declaration, including explicitly written constructors.
    Node(NodeEvidence<'s>),
    /// A location-backed field, parameter, property or binding.
    Slot(Slot<'s>),
    /// The file represented by this parse root.
    File(NodeEvidence<'s>),
    /// One logical declaration with ordered recovered pieces or explicit clauses.
    Group(OwnerGroup<'s>),
    /// An explicitly documented virtual target, never a fabricated syntax node.
    Virtual(VirtualOwner<'s>),
}

/// A source slot that need not have its own syntax node.
#[derive(Debug)]
#[non_exhaustive]
pub struct Slot<'s> {
    pub(crate) enclosing: OwnerId,
    /// The slot's semantic role, independent of grammar kind IDs.
    pub role: SlotRole,
    /// Zero-based ordinal within that enclosing owner's role.
    pub ordinal: usize,
    /// Included source spelling identifying this slot.
    pub source: SourcePieces<'s>,
}

/// Location-backed owner roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlotRole {
    /// Named or positional field.
    Field,
    /// Function/type parameter.
    Parameter,
    /// Property, including promoted constructor parameters.
    Property,
    /// Binding or declared constant.
    Binding,
}

/// One contributing declaration piece, not an envelope across intervening code.
#[derive(Debug)]
#[non_exhaustive]
pub struct OwnerPiece<'s> {
    /// Included source spelling of this piece.
    pub source: SourcePieces<'s>,
    /// Exact syntax evidence when present.
    pub node: Option<NodeEvidence<'s>>,
}

/// One source-written declaration group, without synthesized arities or symbols.
#[derive(Debug)]
#[non_exhaustive]
pub struct OwnerGroup<'s> {
    /// Lexical enclosing context used to qualify this group.
    pub scope: NodeEvidence<'s>,
    /// Ordered explicit heads, clauses or recovered declaration pieces.
    pub pieces: Vec<OwnerPiece<'s>>,
}

/// An explicit virtual declaration described by local markup.
#[derive(Debug)]
#[non_exhaustive]
pub struct VirtualOwner<'s> {
    /// Included name spelling; byte-backed even when not UTF-8.
    pub name: SourcePieces<'s>,
    /// The directive spelling that establishes this virtual owner.
    pub declaration: SourcePieces<'s>,
}

/// Attachment disposition. A resolved set asserts every retained owner;
/// alternatives preserve uncertainty and assert none of them individually.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolution {
    /// Every listed qualifying owner is resolved.
    Resolved,
    /// Available candidates with an explicit reason resolution failed.
    Unresolved(UnresolvedReason),
    /// Source evidence without a selected owner.
    Unattached,
}

/// Why an association is not a concrete owner claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnresolvedReason {
    /// Mutually conditional source alternatives.
    Conditional,
    /// Several same-file matches with insufficient disambiguation.
    Ambiguous,
    /// No matching source target.
    MissingName,
    /// Name or target requires execution.
    Dynamic,
    /// Macro expansion or quoted code does not establish a concrete owner.
    Expansion,
    /// Recovery or known grammar boundaries are insufficient.
    GrammarBoundary,
    /// Excluded source bytes prevent recognizing a continuous spelling or attachment.
    ExcludedGap,
}

/// Known publication eligibility under a convention, separate from attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Publication {
    /// Known eligible under the named profile/context.
    Eligible,
    /// Source docs exist but the named profile excludes publication.
    Ineligible,
    /// Not enough context to decide, or runtime existence is unknown.
    Unknown,
}

/// A named attachment convention. The adapter supplies its contract name;
/// this is neither an inferred language nor an invitation to mix profiles.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Convention(pub(crate) &'static str);

impl Convention {
    /// Contract-qualified name, such as `BcaPhpSource` or `PhpDocumentor`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.0
    }
}

/// An attachment edge/set sharing its document with any other associations.
#[derive(Debug)]
#[non_exhaustive]
pub struct Association<'s> {
    pub(crate) document: DocumentId,
    pub(crate) owners: Vec<OwnerId>,
    /// Resolved set versus uncertain alternatives; never an existence boolean.
    pub resolution: Resolution,
    /// Convention responsible for selection and attachment.
    pub convention: Convention,
    /// Original wrapper seen by binding, such as `export` or an annotation.
    pub binding: Option<NodeEvidence<'s>>,
    /// Lexical containing scope, independently of normalized semantic owners.
    pub lexical_scope: NodeEvidence<'s>,
    /// Known publication result under the convention and supplied context.
    pub publication: Publication,
}

impl Association<'_> {
    /// Number of resolved owners or uncertain candidates, as distinguished by
    /// `resolution`. This is the qualifying subset, not all syntactic targets.
    #[must_use]
    pub fn owner_count(&self) -> usize {
        self.owners.len()
    }
}
