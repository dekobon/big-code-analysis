//! Draft composition and once-per-document text materialization.

use super::*;

impl<'s> Builder<'s> {
    pub(super) fn combine_pending(
        &mut self,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let mut by_target = std::collections::HashMap::new();
        for (index, draft) in self.drafts.iter().enumerate() {
            budget.step()?;
            if let Some(target) = draft.target && matches!(draft.role, DocumentRole::Primary | DocumentRole::FilePackage) {
                by_target.insert(target, index);
            }
        }
        for (node, fragment, target) in std::mem::take(&mut self.pending_attributes) {
            budget.step()?;
            if let Some(&index) = target.as_ref().and_then(|target| by_target.get(target)) {
                self.drafts[index].fragments.push(fragment);
                self.drafts[index].pending = true;
            } else {
                let scope = self.index.canonical.nodes[node.0]
                    .parent
                    .unwrap_or(NodeId(0));
                if let Some(target) = target {
                    by_target.insert(target, self.drafts.len());
                }
                self.drafts.push(Draft {
                    fragments: vec![fragment],
                    target,
                    scope,
                    binding: target.map(|id| self.index.canonical.nodes[id.0].evidence),
                    role: if target.is_some_and(|id| {
                        id.0 == 0
                            || self.index.canonical.nodes[id.0].evidence.node().kind() == "mod_item"
                    }) {
                        DocumentRole::FilePackage
                    } else {
                        DocumentRole::Primary
                    },
                    unresolved: None,
                    pending: true,
                    gap: false,
                });
            }
        }
        Ok(())
    }

    pub(super) fn document(
        &mut self,
        mut draft: Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let (content, attribute) = self.render_draft(&mut draft, budget)?;
        let prelex = self.index.profile == Profile::Javadoc
            && matches!(
                content,
                Content::Unavailable(UnavailableReason::UnsupportedDecoding)
            );
        let spelling = if prelex {
            self.render_spelling(&draft, budget)?
        } else {
            None
        };
        self.index.source_cleaned.push(spelling);
        if prelex {
            draft.unresolved = Some(UnresolvedReason::GrammarBoundary);
        }
        let composition = if attribute {
            Composition::AttributeSequence
        } else if draft.fragments.len() == 1 {
            Composition::Single
        } else {
            Composition::CommentRun
        };
        let document = DocumentId(self.index.facts.documents.len());
        self.index.facts.documents.push(Document {
            fragments: std::mem::take(&mut draft.fragments),
            composition,
            role: draft.role,
            content,
        });
        let mut owners = Vec::new();
        let mut pending = draft.pending;
        let mut named_resolution = None;
        if self.index.profile == Profile::Doxygen && draft.target.is_none() {
            let fragment =
                &self.index.facts.fragments[self.index.facts.documents[document.0].fragments[0].0];
            if let Some(node) = fragment.node {
                let raw = &self.index.source.bytes()[node.node().byte_range()];
                if let Some((b"fn", name)) =
                    crate::lang_helpers::doxygen::structural_command_with_budget(raw, budget)?
                {
                    let (found, reason) = self.names.resolve(name, budget)?;
                    owners = found;
                    named_resolution =
                        Some(reason.map_or(Resolution::Resolved, Resolution::Unresolved));
                    pending = false;
                }
            }
        }
        if let Some(target) = draft.target {
            match &self.index.canonical.nodes[target.0].projection {
                Projection::Ready(ids) => {
                    for &id in ids {
                        budget.step()?;
                        owners.push(id);
                    }
                }
                Projection::Pending | Projection::Unsupported => pending = true,
            }
        }
        if pending {
            self.pending_document(&draft, document, &owners, attribute, budget)?;
        }
        if !owners.is_empty() || draft.unresolved.is_some() || named_resolution.is_some() {
            let association = self.index.facts.associations.len();
            for owner in &owners {
                budget.step()?;
                self.index.associations_by_owner[owner.0].push(association);
            }
            self.index.facts.associations.push(Association {
                document,
                owners,
                resolution: named_resolution.unwrap_or_else(|| {
                    draft
                        .unresolved
                        .map_or(Resolution::Resolved, Resolution::Unresolved)
                }),
                convention: Convention(self.index.profile.name()),
                binding: draft.binding,
                lexical_scope: self.index.canonical.nodes[draft.scope.0].evidence,
                publication: Publication::Unknown,
            });
        }
        Ok(())
    }

    fn pending_document(
        &mut self,
        draft: &Draft<'s>,
        document: DocumentId,
        owners: &[OwnerId],
        attribute: bool,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: if attribute {
                AssessmentAxis::Composition
            } else {
                AssessmentAxis::Owner
            },
            reason: if attribute {
                "Rust doc/cfg_attr carrier awaits adapter composition"
            } else {
                "Selected local or wrapper form awaits owner projection"
            },
            node: draft.binding,
            document: Some(document),
        });
        for owner in owners {
            budget.step()?;
            self.index.assessments_by_owner[owner.0].push(assessment);
        }
        if owners.is_empty() {
            self.index.canonical.nodes[draft.target.unwrap_or(draft.scope).0]
                .pending
                .push(assessment);
        }
        Ok(())
    }

    fn render_draft(
        &self,
        draft: &mut Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<(Content<'s>, bool), CollectionError> {
        let mut keys = Vec::new();
        for id in &draft.fragments {
            budget.step()?;
            keys.push((
                self.index.facts.fragments[id.0].source.pieces()[0]
                    .range()
                    .start,
                id.0,
            ));
        }
        draft.fragments.clear();
        for (_, id) in crate::documentation::ordering::ordered(keys, budget)? {
            budget.step()?;
            draft.fragments.push(FragmentId(id));
        }
        let mut fragments = Vec::new();
        let mut attribute = false;
        for id in &draft.fragments {
            budget.step()?;
            let fragment = &self.index.facts.fragments[id.0];
            attribute |= fragment.form == FragmentForm::Attribute;
            fragments.push(fragment);
        }
        let content = if attribute {
            Content::PendingAdapter
        } else if draft.gap {
            Content::Unavailable(UnavailableReason::ExcludedGap)
        } else {
            match crate::documentation::text::comments::render(
                self.index.profile,
                &self.index.source,
                &fragments,
                budget,
            ) {
                Ok(content) => content,
                Err(crate::documentation::text::TextError::Source(error)) => {
                    return Err(error.into());
                }
                Err(crate::documentation::text::TextError::Unavailable(reason)) => {
                    Content::Unavailable(reason)
                }
            }
        };
        Ok((content, attribute))
    }

    fn render_spelling(
        &self,
        draft: &Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<Option<Content<'s>>, CollectionError> {
        let mut fragments = Vec::new();
        for id in &draft.fragments {
            budget.step()?;
            fragments.push(&self.index.facts.fragments[id.0]);
        }
        match crate::documentation::text::comments::render_source_cleaned(
            self.index.profile,
            &self.index.source,
            &fragments,
            budget,
        ) {
            Ok(content) => Ok(Some(content)),
            Err(crate::documentation::text::TextError::Source(error)) => Err(error.into()),
            Err(crate::documentation::text::TextError::Unavailable(_)) => Ok(None),
        }
    }
}
