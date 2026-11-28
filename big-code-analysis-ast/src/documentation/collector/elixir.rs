//! Elixir documentation attributes, read once the walk has recorded the tree.
//!
//! A `@moduledoc` documents its `defmodule … do … end` module. A `@doc`
//! documents the group of clauses that the next definition statement of the
//! same module body belongs to. The value must be one plain string. Every
//! other documentation attribute stays visible as a pending assessment.

use super::*;
use crate::documentation::text::{TextBuilder, TextError};

mod body;
pub(super) mod syntax;

use body::{Carrier, Group, Reading};
use syntax::{
    Attribute, Gap, Kind, Word, at_operator, attribute, children, covers, kind, node, step,
};

const OUTSIDE: &str = "Elixir documentation attribute outside a module body awaits interpretation";
const TEMPLATE: &str =
    "Elixir documentation attribute in a quoted or unreadable call awaits interpretation";
const RECOVERED: &str = "Recovered Elixir documentation attribute awaits interpretation";
const UNOWNED: &str = "Elixir documentation attribute awaits an owner projection";

// The axis and the reason of the assessment that a document without a text carries.
type Pending = (AssessmentAxis, &'static str);

const VALUE: Pending = (
    AssessmentAxis::Composition,
    "Elixir documentation attribute value awaits decoding",
);
const SEVERAL: Pending = (
    AssessmentAxis::Selection,
    "Several Elixir documentation attributes await selection",
);

// Where the second pass stands when it reaches a node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Code,
    // Inside a `quote`, or a call whose first word is not fully included.
    Template,
    // Below a statement that nothing separates from the one before it.
    Unreached,
    // The children of a `defmodule` call with a body.
    Module { call: NodeId, body: NodeId },
    // The body of that module.
    Body(NodeId),
    Statement,
    // The children of an `@` operator: its operand is no call statement.
    Attribute,
}

impl Place {
    fn of(self, child: NodeId) -> Self {
        match self {
            Self::Module { call, body } if child == body => Self::Body(call),
            Self::Module { .. } => Self::Code,
            place => place,
        }
    }

    // Whether anything below is read as a module or a definition.
    fn reads(self) -> bool {
        !matches!(self, Self::Template | Self::Unreached)
    }

    // Why a documentation attribute that no body pass reads has no document.
    fn reason(self, joined: bool) -> &'static str {
        match self {
            Self::Template => TEMPLATE,
            Self::Unreached => RECOVERED,
            _ if joined => RECOVERED,
            _ => OUTSIDE,
        }
    }
}

#[derive(Clone, Copy)]
struct Visit {
    id: NodeId,
    place: Place,
    // How the node stands to the statement before it, if it is a statement
    // or follows a recovery.
    gap: Option<Gap>,
    // The grammar recovered in a statement that is, or holds, the node.
    doubt: bool,
}

struct Attachment {
    role: DocumentRole,
    binding: Option<NodeId>,
    hidden: bool,
    // The grammar recovered in a statement around the owner.
    doubt: bool,
    publication: Publication,
}

// The text of one plain string, or why the attributes have none yet.
fn content<'s, B: WorkBudget>(
    index: &DocumentationIndex<'s>,
    carriers: &[Carrier],
    budget: &B,
) -> Result<(Content<'s>, Option<Pending>), SourceError> {
    let [
        Carrier {
            text: Some(text), ..
        },
    ] = carriers
    else {
        let reason = if carriers.len() == 1 { VALUE } else { SEVERAL };
        return Ok((Content::PendingAdapter, Some(reason)));
    };
    if syntax::rejected(&index.source.bytes()[text.clone()], budget).map_err(Into::into)? {
        return Ok((Content::PendingAdapter, Some(VALUE)));
    }
    let mut builder = TextBuilder::default();
    match builder.source(&index.source, text.clone(), budget) {
        Ok(()) => Ok((builder.finish(), None)),
        Err(TextError::Source(error)) => Err(error),
        Err(TextError::Unavailable(reason)) => Ok((Content::Unavailable(reason), None)),
    }
}

impl Builder<'_> {
    // A second pass over the recorded tree, top down, so that a `quote` and
    // a joined statement are known before what they hold. It reads nothing
    // that it or `finish` writes.
    pub(super) fn elixir_attributes<B: WorkBudget>(
        &mut self,
        budget: &B,
    ) -> Result<(), SourceError> {
        let tear = syntax::tear(&self.index.canonical, budget).map_err(Into::into)?;
        let mut stack = vec![Visit {
            id: NodeId(0),
            place: Place::Code,
            gap: None,
            doubt: false,
        }];
        while let Some(visit) = stack.pop() {
            self.elixir_visit(visit, tear, &mut stack, budget)?;
        }
        Ok(())
    }

    // Reads one node and pushes its children, the first on top.
    fn elixir_visit<B: WorkBudget>(
        &mut self,
        mut visit: Visit,
        tear: Option<usize>,
        stack: &mut Vec<Visit>,
        budget: &B,
    ) -> Result<(), SourceError> {
        step(budget)?;
        let inner = self.elixir_node(&visit, budget)?;
        let gaps = self.elixir_gaps(&mut visit, inner, tear, budget)?;
        let children = children(&self.index.canonical, visit.id);
        for (at, &child) in children.iter().enumerate().rev() {
            step(budget)?;
            stack.push(Visit {
                id: child,
                place: inner.of(child),
                gap: gaps.get(at).copied().flatten(),
                doubt: visit.doubt,
            });
        }
        Ok(())
    }

    // Reads one node and answers where its children stand.
    fn elixir_node<B: WorkBudget>(
        &mut self,
        visit: &Visit,
        budget: &B,
    ) -> Result<Place, SourceError> {
        let &Visit { id, place, gap, .. } = visit;
        let joined = gap == Some(Gap::Joined);
        let attribute = at_operator(&self.index.canonical, id, budget).map_err(Into::into)?;
        // The body pass has read an attribute that is a statement.
        if attribute && place != Place::Statement {
            self.elixir_stray(id, place.reason(joined), budget)?;
        }
        if !place.reads() {
            return Ok(place);
        }
        // The operand of `@` names an attribute and calls nothing.
        if place != Place::Attribute && kind(&self.index.canonical, id) == "call" {
            return self.elixir_call(id, place, joined, budget);
        }
        Ok(match place {
            _ if joined => Place::Unreached,
            _ if attribute => Place::Attribute,
            Place::Body(_) => Place::Statement,
            _ => Place::Code,
        })
    }

    // What the pass knows before it descends from a node: whether the
    // grammar recovered in the statement that the node is, and how the
    // children stand to the statements before them. The body of a module is
    // read with the same answers, so no pass reads a statement that another
    // found joined.
    fn elixir_gaps<B: WorkBudget>(
        &mut self,
        visit: &mut Visit,
        inner: Place,
        tear: Option<usize>,
        budget: &B,
    ) -> Result<Vec<Option<Gap>>, SourceError> {
        if !inner.reads() {
            return Ok(Vec::new());
        }
        let canonical = &self.index.canonical;
        // A statement below a broken one is in doubt already.
        visit.doubt = visit.doubt
            || visit.gap.is_some()
                && syntax::broken(canonical, visit.id, budget).map_err(Into::into)?;
        let gaps = syntax::gaps(&self.index, visit.id, tear, budget)?;
        if let Place::Body(module) = visit.place {
            self.elixir_module(module, visit, &gaps, budget)?;
        }
        Ok(gaps)
    }

    // Reads a call by its first word. A joined call is a statement that the
    // compiler does not reach: nothing below it is read, and a module or a
    // definition stays visible as a pending owner.
    fn elixir_call<B: WorkBudget>(
        &mut self,
        id: NodeId,
        place: Place,
        joined: bool,
        budget: &B,
    ) -> Result<Place, SourceError> {
        let word = syntax::word(&self.index, id, budget)?;
        if !joined && word == Word::Quote {
            return Ok(Place::Template);
        }
        if !joined && word == Word::Module {
            let body = syntax::module_body(&self.index.canonical, id, budget);
            // The body is read when the pass reaches it.
            if let Some(body) = body.map_err(Into::into)? {
                return Ok(Place::Module { call: id, body });
            }
        }
        // The body pass has read a definition that is a statement.
        let read = place == Place::Statement && matches!(word, Word::Definition(_) | Word::Unread);
        if word.declares() && !read {
            self.index.canonical.nodes[id.0].projection = Projection::Pending;
        }
        Ok(if joined {
            Place::Unreached
        } else {
            Place::Code
        })
    }

    fn elixir_assess(
        &mut self,
        axis: AssessmentAxis,
        reason: &'static str,
        carrier: NodeId,
        scope: NodeId,
    ) {
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis,
            reason,
            node: Some(self.index.canonical.nodes[carrier.0].evidence),
            document: None,
        });
        self.index.canonical.nodes[scope.0].pending.push(assessment);
    }

    // A documentation attribute that no body pass reads.
    fn elixir_stray<B: WorkBudget>(
        &mut self,
        id: NodeId,
        reason: &'static str,
        budget: &B,
    ) -> Result<(), SourceError> {
        let range = node(&self.index.canonical, id).byte_range();
        if covers(&self.index, range, budget)?
            && matches!(
                attribute(&self.index, id, budget).map_err(Into::into)?,
                Attribute::Doc | Attribute::Moduledoc | Attribute::Typedoc
            )
        {
            let scope = self.index.canonical.nodes[id.0].parent.unwrap_or(NodeId(0));
            self.elixir_assess(AssessmentAxis::Occurrence, reason, id, scope);
        }
        Ok(())
    }

    // Reads the body of a module and files what its statements document.
    fn elixir_module<B: WorkBudget>(
        &mut self,
        module: NodeId,
        body: &Visit,
        gaps: &[Option<Gap>],
        budget: &B,
    ) -> Result<(), SourceError> {
        let doubt = body.doubt;
        let mut reading = body::read(&self.index, body.id, gaps, budget)?;
        let owner = self.elixir_module_owner(module, &mut reading, budget)?;
        self.elixir_marks(module, &reading, budget)?;
        if !reading.moduledocs.is_empty() {
            let attachment = Attachment {
                role: DocumentRole::FilePackage,
                binding: None,
                hidden: false,
                doubt,
                publication: Publication::Unknown,
            };
            self.elixir_document(&reading.moduledocs, owner, &attachment, budget)?;
        }
        self.elixir_groups(module, &reading.groups, doubt, budget)
    }

    // The module owns its documentation unless it holds a syntax error or
    // excluded bytes. Its statements are read either way.
    fn elixir_module_owner<B: WorkBudget>(
        &mut self,
        module: NodeId,
        reading: &mut Reading,
        budget: &B,
    ) -> Result<OwnerId, SourceError> {
        let scope = self.index.canonical.nodes[module.0].evidence;
        let owner = OwnerId(self.index.facts.owners.len());
        if scope.node().has_error() || !covers(&self.index, scope.node().byte_range(), budget)? {
            reading.pending.push(module);
            // `moduledocs` is emptied here, so the caller never uses `owner`.
            reading.unowned.append(&mut reading.moduledocs);
        } else {
            self.add_owner(module, Owner::Node(scope));
        }
        Ok(owner)
    }

    fn elixir_groups<B: WorkBudget>(
        &mut self,
        module: NodeId,
        groups: &[Group],
        doubt: bool,
        budget: &B,
    ) -> Result<(), SourceError> {
        for group in groups {
            step(budget)?;
            self.elixir_group(module, group, doubt, budget)?;
        }
        Ok(())
    }

    // What the body pass found and no owner takes.
    fn elixir_marks<B: WorkBudget>(
        &mut self,
        module: NodeId,
        reading: &Reading,
        budget: &B,
    ) -> Result<(), SourceError> {
        for carrier in &reading.unowned {
            step(budget)?;
            self.elixir_assess(AssessmentAxis::Owner, UNOWNED, carrier.node, module);
        }
        for &carrier in &reading.recovered {
            step(budget)?;
            self.elixir_assess(AssessmentAxis::Occurrence, RECOVERED, carrier, module);
        }
        for &definition in &reading.pending {
            step(budget)?;
            self.index.canonical.nodes[definition.0].projection = Projection::Pending;
        }
        Ok(())
    }

    fn elixir_group<B: WorkBudget>(
        &mut self,
        module: NodeId,
        group: &Group,
        doubt: bool,
        budget: &B,
    ) -> Result<(), SourceError> {
        let scope = self.index.canonical.nodes[module.0].evidence;
        let owner = OwnerId(self.index.facts.owners.len());
        let mut pieces = Vec::new();
        for &clause in &group.clauses {
            step(budget)?;
            let evidence = self.index.canonical.nodes[clause.0].evidence;
            let source = self
                .index
                .source
                .pieces_with_budget(evidence.node().byte_range(), budget)?;
            pieces.push(OwnerPiece {
                source,
                node: Some(evidence),
            });
            self.index.canonical.nodes[clause.0].projection = Projection::Ready(vec![owner]);
        }
        // A query on any clause answers for the group; the first clause
        // anchors it.
        self.add_owner(group.clauses[0], Owner::Group(OwnerGroup { scope, pieces }));
        if group.carriers.is_empty() {
            return Ok(());
        }
        let attachment = Attachment {
            role: DocumentRole::Primary,
            binding: group.binding,
            hidden: group.hidden,
            doubt,
            // The compiler discards the documentation of a private definition.
            publication: if matches!(group.kind, Kind::Defp | Kind::Defmacrop) {
                Publication::Ineligible
            } else {
                Publication::Unknown
            },
        };
        self.elixir_document(&group.carriers, owner, &attachment, budget)
    }

    fn elixir_fragments<B: WorkBudget>(
        &mut self,
        carriers: &[Carrier],
        budget: &B,
    ) -> Result<Vec<FragmentId>, SourceError> {
        let mut fragments = Vec::new();
        for carrier in carriers {
            step(budget)?;
            let evidence = self.index.canonical.nodes[carrier.node.0].evidence;
            let source = self
                .index
                .source
                .pieces_with_budget(evidence.node().byte_range(), budget)?;
            fragments.push(self.fragment(source, evidence, FragmentForm::Attribute));
        }
        Ok(fragments)
    }

    // One document for the attributes of one owner, with its association.
    fn elixir_document<B: WorkBudget>(
        &mut self,
        carriers: &[Carrier],
        owner: OwnerId,
        attachment: &Attachment,
        budget: &B,
    ) -> Result<(), SourceError> {
        let fragments = self.elixir_fragments(carriers, budget)?;
        let (content, pending) = content(&self.index, carriers, budget)?;
        let document = DocumentId(self.index.facts.documents.len());
        self.index.source_cleaned.push(None);
        self.index.qualifications_by_document.push(Vec::new());
        self.index.facts.documents.push(Document {
            fragments,
            composition: if carriers.len() == 1 {
                Composition::Single
            } else {
                Composition::AttributeSequence
            },
            role: attachment.role,
            content,
        });
        let first = carriers[0].node;
        if let Some((axis, reason)) = pending {
            let assessment = self.index.assessments.len();
            self.index.assessments.push(Assessment {
                axis,
                reason,
                node: Some(self.index.canonical.nodes[first.0].evidence),
                document: Some(document),
            });
            self.index.assessments_by_owner[owner.0].push(assessment);
        }
        self.elixir_associate(document, owner, first, attachment);
        Ok(())
    }

    fn elixir_associate(
        &mut self,
        document: DocumentId,
        owner: OwnerId,
        first: NodeId,
        attachment: &Attachment,
    ) {
        // An `ERROR` above the owner is a recovery in a statement around it,
        // so the doubt covers what the walk records for such a node.
        let unresolved = attachment
            .hidden
            .then_some(UnresolvedReason::ExcludedGap)
            .or(attachment
                .doubt
                .then_some(UnresolvedReason::GrammarBoundary));
        let scope = self.index.canonical.nodes[first.0]
            .parent
            .unwrap_or(NodeId(0));
        self.index.facts.associations.push(Association {
            document,
            owners: vec![owner],
            resolution: unresolved.map_or(Resolution::Resolved, Resolution::Unresolved),
            convention: Convention(self.index.profile.name()),
            binding: attachment
                .binding
                .map(|id| self.index.canonical.nodes[id.0].evidence),
            lexical_scope: self.index.canonical.nodes[scope.0].evidence,
            publication: attachment.publication,
        });
    }
}
