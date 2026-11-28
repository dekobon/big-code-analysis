//! Python body docstrings: the runs of string statements, their fragments and text state.

use super::*;
use crate::Python;
use crate::lang_helpers::python::{DocstringRun, completed_docstring_run_with_budget};

const ATTRIBUTE_RUN: &str = "Python attribute docstring awaits binding-owner projection";
const CARRIAGE_RETURN: &str =
    "Python literal with a carriage return awaits line-ending normalization";

impl<'s> Builder<'s> {
    /// Collects the docstring run that `node` completes, if it is the value of a
    /// run's final member: one draft for each member of a body run, or one
    /// assessment for a run that follows an assignment.
    pub(super) fn python_string(
        &mut self,
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        // bca: suppress(nexits) -- three early returns; the reader and a lookup can fail.
        if !matches!(
            node.kind_id().into(),
            Python::String | Python::ConcatenatedString
        ) {
            return Ok(());
        }
        let mut members = Vec::new();
        let Some(run) = completed_docstring_run_with_budget(
            node,
            self.index.source.bytes(),
            ancestors,
            budget,
            |_, value| members.push(value),
        )?
        else {
            return Ok(());
        };
        // The reader finds the statement of `node` upward, and the walk reads each
        // value downward. Past a recovered `ERROR` inside parentheses the two differ:
        // `node` is then not the value of the run's last statement.
        if members.last().is_none_or(|last| last.id() != node.id()) {
            return Ok(());
        }
        let owner = self.index.canonical.lookup(run.owner.as_tree_sitter())?;
        if run.after.is_some() {
            self.python_attribute_run(owner, &run, budget)
        } else {
            self.python_body_run(owner, &run, &members, budget)
        }
    }

    /// A run after an assignment gives no document in this unit: one assessment
    /// names its first statement, on the owner's own list or, where the owner has
    /// no projection yet, on its node.
    fn python_attribute_run(
        &mut self,
        owner: NodeId,
        run: &DocstringRun<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let first = self.index.canonical.lookup(run.first.as_tree_sitter())?;
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: AssessmentAxis::Owner,
            reason: ATTRIBUTE_RUN,
            node: Some(self.index.canonical.nodes[first.0].evidence),
            document: None,
        });
        let record = &mut self.index.canonical.nodes[owner.0];
        match &record.projection {
            Projection::Ready(ids) => {
                for id in ids {
                    budget.step()?;
                    self.index.assessments_by_owner[id.0].push(assessment);
                }
            }
            // No parsed source gives `Unsupported` here: the owner is the root or a
            // node the Python target rule accepts.
            Projection::Pending | Projection::Unsupported => record.pending.push(assessment),
        }
        Ok(())
    }

    /// A run that opens its body: one draft for each member value, the first as the
    /// primary document. A member is marked unresolved when the source does not
    /// cover everything from the start of the file, or of the owning definition, to
    /// the member's value.
    fn python_body_run(
        &mut self,
        owner: NodeId,
        run: &DocstringRun<'s>,
        members: &[Node<'s>],
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let first = self.index.canonical.lookup(run.first.as_tree_sitter())?;
        let anchor = if run.owner.kind_id() == Python::Module {
            0
        } else {
            run.owner.start_byte()
        };
        let scope = self.index.canonical.nodes[first.0]
            .parent
            .unwrap_or(NodeId(0));
        for (position, value) in members.iter().enumerate() {
            budget.step()?;
            let covered = self
                .index
                .source
                .covers_with_budget(anchor..value.start_byte(), budget)?;
            let fragments = self.python_fragments(value, budget)?;
            self.drafts.push(Draft {
                fragments,
                target: Some(owner),
                scope,
                binding: None,
                role: if position == 0 {
                    DocumentRole::Primary
                } else {
                    DocumentRole::Additional
                },
                unresolved: (!covered).then_some(UnresolvedReason::ExcludedGap),
                pending: false,
                gap: false,
            });
        }
        Ok(())
    }

    /// The fragments of one member value: the value itself, or each `string` of a
    /// concatenation in source order.
    fn python_fragments(
        &mut self,
        value: &Node<'s>,
        budget: &CollectionBudget,
    ) -> Result<Vec<FragmentId>, CollectionError> {
        let mut fragments = Vec::new();
        // A value that holds a syntax error is one fragment, whole: its children
        // are not read.
        if !value.has_error() && value.kind_id() == Python::ConcatenatedString {
            for inner in value.children() {
                budget.step()?;
                if inner.kind_id() == Python::String {
                    fragments.push(self.python_fragment(&inner, budget)?);
                }
            }
        }
        // Also a concatenation without a `string` child, which no parsed source gives.
        if fragments.is_empty() {
            fragments.push(self.python_fragment(value, budget)?);
        }
        Ok(fragments)
    }

    fn python_fragment(
        &mut self,
        node: &Node<'s>,
        budget: &CollectionBudget,
    ) -> Result<FragmentId, CollectionError> {
        bounds(node.as_tree_sitter(), self.index.source.bytes().len())?;
        let source = self
            .index
            .source
            .pieces_with_budget(node.start_byte()..node.end_byte(), budget)?;
        let evidence = self.index.source.node(node.as_tree_sitter())?;
        Ok(self.fragment(source, evidence, FragmentForm::Literal))
    }

    /// Whether the draft is a Python docstring: only this adapter creates `Literal`
    /// fragments.
    pub(super) fn literal_draft(&self, draft: &Draft<'s>) -> bool {
        draft
            .fragments
            .first()
            .is_some_and(|id| self.index.facts.fragments[id.0].form == FragmentForm::Literal)
    }

    pub(super) fn python_content(
        &self,
        draft: &Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<Content<'s>, CollectionError> {
        use crate::documentation::text::TextError;
        let mut fragments = Vec::with_capacity(draft.fragments.len());
        for id in &draft.fragments {
            budget.step()?;
            fragments.push(&self.index.facts.fragments[id.0]);
        }
        match crate::documentation::text::python::render(&self.index.source, &fragments, budget) {
            Ok(content) => Ok(content),
            Err(TextError::Source(error)) => Err(error.into()),
            Err(TextError::Unavailable(reason)) => Ok(Content::Unavailable(reason)),
        }
    }

    /// Records that the text of `document` waits for line-ending normalization, on
    /// each resolved owner or, without one, on the target node.
    pub(super) fn python_pending_text(
        &mut self,
        document: DocumentId,
        owners: &[OwnerId],
        target: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        // Every Python draft names its owner node, so no parsed source gives `None`.
        let target = target.ok_or(SourceError::NoncanonicalNode)?;
        let node = self.index.facts.documents[document.0]
            .fragments
            .first()
            .and_then(|id| self.index.facts.fragments[id.0].node);
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: AssessmentAxis::Composition,
            reason: CARRIAGE_RETURN,
            node,
            document: Some(document),
        });
        for owner in owners {
            budget.step()?;
            self.index.assessments_by_owner[owner.0].push(assessment);
        }
        if owners.is_empty() {
            self.index.canonical.nodes[target.0]
                .pending
                .push(assessment);
        }
        Ok(())
    }
}
