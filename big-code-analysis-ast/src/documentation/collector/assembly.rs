//! Draft composition and once-per-document text materialization.

use super::*;

impl<'s> Builder<'s> {
    pub(super) fn coalesce_rust(
        &mut self,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        if self.index.profile != Profile::Rust {
            return Ok(());
        }
        let mut groups = std::collections::HashMap::<NodeId, usize>::new();
        for mut draft in std::mem::take(&mut self.drafts) {
            budget.step()?;
            if let Some(target) = draft.target
                && matches!(draft.unresolved, None | Some(UnresolvedReason::ExcludedGap))
            {
                draft.role = self.rust_role(target);
                draft.scope = self.index.canonical.nodes[target.0]
                    .parent
                    .unwrap_or(NodeId(0));
                if let Some(&index) = groups.get(&target) {
                    let composed = &mut self.drafts[index];
                    for fragment in draft.fragments {
                        budget.step()?;
                        composed.fragments.push(fragment);
                    }
                    composed.pending |= draft.pending;
                    composed.gap |= draft.gap;
                    composed.unresolved = composed.unresolved.or(draft.unresolved);
                    composed.binding = composed.binding.or(draft.binding);
                    continue;
                }
                groups.insert(target, self.drafts.len());
            }
            self.drafts.push(draft);
        }
        Ok(())
    }

    fn rust_role(&self, target: NodeId) -> DocumentRole {
        if target.0 == 0
            || self.index.canonical.nodes[target.0].evidence.node().kind() == "mod_item"
        {
            DocumentRole::FilePackage
        } else {
            DocumentRole::Primary
        }
    }
    pub(super) fn combine_pending(
        &mut self,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let mut by_target = std::collections::HashMap::new();
        for (index, draft) in self.drafts.iter().enumerate() {
            budget.step()?;
            if let Some(target) = draft.target
                && matches!(draft.unresolved, None | Some(UnresolvedReason::ExcludedGap))
                && matches!(
                    draft.role,
                    DocumentRole::Primary | DocumentRole::FilePackage
                )
            {
                by_target.insert(target, index);
            }
        }
        for (node, fragment, target) in std::mem::take(&mut self.pending_attributes) {
            budget.step()?;
            let pending = matches!(self.rust_values.get(&fragment), Some(rust::Value::Pending));
            let binding = target
                .filter(|_| {
                    self.index.canonical.nodes[node.0].evidence.node().kind() == "attribute_item"
                })
                .map(|id| self.index.canonical.nodes[id.0].evidence);
            if let Some(&index) = target.as_ref().and_then(|target| by_target.get(target)) {
                let draft = &mut self.drafts[index];
                draft.fragments.push(fragment);
                draft.pending |= pending;
                draft.binding = draft.binding.or(binding);
            } else {
                let scope = self.index.canonical.nodes[target.unwrap_or(node).0]
                    .parent
                    .unwrap_or(NodeId(0));
                if let Some(target) = target {
                    by_target.insert(target, self.drafts.len());
                }
                self.drafts.push(Draft {
                    fragments: vec![fragment],
                    target,
                    scope,
                    binding,
                    role: target.map_or(DocumentRole::Primary, |id| self.rust_role(id)),
                    unresolved: None,
                    pending: pending || target.is_none(),
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
        self.index.qualifications_by_document.push(Vec::new());
        self.index.facts.documents.push(Document {
            fragments: std::mem::take(&mut draft.fragments),
            composition,
            role: draft.role,
            content,
        });
        self.pod_pending_text(document);
        let mut owners = Vec::new();
        let mut pending = draft.pending;
        let mut named_resolution = None;
        if self.index.profile == Profile::Doxygen
            && draft.target.is_none()
            && !draft.gap
            && let Some(result) = self.named_document(document, draft.scope, budget)?
        {
            for candidate in result.owners {
                budget.step()?;
                self.candidate_qualifications(document, candidate, budget)?;
                owners.push(candidate.owner);
            }
            named_resolution = result
                .reason
                .map(Resolution::Unresolved)
                .or_else(|| (!result.pending).then_some(Resolution::Resolved));
            pending = result.pending;
        }
        if let Some(target) = draft.target {
            match &self.index.canonical.nodes[target.0].projection {
                Projection::Ready(ids) => {
                    draft.unresolved = draft.unresolved.or(self.index.canonical.nodes[target.0]
                        .qualification
                        .headline());
                    for &id in ids {
                        budget.step()?;
                        owners.push(id);
                    }
                }
                Projection::Pending | Projection::Unsupported => pending = true,
            }
        }
        if named_resolution == Some(Resolution::Unresolved(UnresolvedReason::Ambiguous)) {
            draft.unresolved = None;
        }
        if pending {
            let unassessed_content = matches!(
                self.index.facts.documents[document.0].content,
                Content::PendingAdapter
            );
            self.pending_document(
                &draft,
                document,
                &owners,
                attribute && unassessed_content,
                budget,
            )?;
            self.pending_conditional_selection(draft.target, document);
        }
        if (!owners.is_empty() && (!pending || draft.target.is_some()))
            || draft.unresolved.is_some()
            || named_resolution.is_some()
        {
            self.index.facts.associations.push(Association {
                document,
                owners,
                resolution: draft
                    .unresolved
                    .map(Resolution::Unresolved)
                    .or(named_resolution)
                    .unwrap_or(Resolution::Resolved),
                convention: Convention(self.index.profile.name()),
                binding: draft.binding,
                lexical_scope: self.index.canonical.nodes[draft.scope.0].evidence,
                publication: self.publication(draft.target, budget)?,
            });
        }
        Ok(())
    }

    fn publication(
        &self,
        target: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<Publication, SourceError> {
        if self.index.profile == Profile::Javadoc
            && target.is_some_and(|target| {
                self.index.canonical.nodes[target.0].evidence.node().kind() == "package_declaration"
            })
        {
            java_package_publication(self.index.context.logical_filename.as_deref(), budget)
        } else {
            Ok(Publication::Unknown)
        }
    }

    fn pending_document(
        &mut self,
        draft: &Draft<'s>,
        document: DocumentId,
        owners: &[OwnerId],
        pending_attribute_composition: bool,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: if pending_attribute_composition {
                AssessmentAxis::Composition
            } else {
                AssessmentAxis::Owner
            },
            reason: if pending_attribute_composition {
                "Rust doc/cfg_attr carrier awaits adapter composition"
            } else if self.index.profile == Profile::Doxygen && draft.target.is_none() {
                "Exact named matching awaits supported signature grammar"
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
            let scope = draft
                .target
                .unwrap_or(self.index.canonical.nodes[draft.scope.0].owner_context);
            self.index.canonical.nodes[scope.0].pending.push(assessment);
        }
        Ok(())
    }

    fn pending_conditional_selection(&mut self, target: Option<NodeId>, document: DocumentId) {
        if self.index.profile == Profile::CsharpXml
            && let Some(target) = target
            && conditional_kind(self.index.canonical.nodes[target.0].evidence.node().kind())
        {
            let assessment = self.index.assessments.len();
            self.index.assessments.push(Assessment {
                axis: AssessmentAxis::Selection,
                reason: "Conditional XML documentation selection awaits branch interpretation",
                node: Some(self.index.canonical.nodes[target.0].evidence),
                document: Some(document),
            });
            self.index.canonical.nodes[target.0]
                .pending
                .push(assessment);
        }
    }

    fn named_document(
        &mut self,
        document: DocumentId,
        scope: NodeId,
        budget: &CollectionBudget,
    ) -> Result<Option<super::super::names::NameResolution>, CollectionError> {
        for id in self.index.facts.documents[document.0]
            .fragments
            .iter()
            .rev()
        {
            budget.step()?;
            let fragment = &self.index.facts.fragments[id.0];
            let Some(node) = fragment.node else {
                continue;
            };
            if !self
                .index
                .source
                .covers_with_budget(node.node().byte_range(), budget)?
            {
                return Ok(None);
            }
            let raw = &self.index.source.bytes()[node.node().byte_range()];
            if let Some((command, name)) =
                crate::lang_helpers::doxygen::structural_command_with_budget(raw, budget)?
            {
                if command != b"fn" {
                    return Ok(None);
                }
                let record = &self.index.canonical.nodes[scope.0];
                let scope = if matches!(
                    record.evidence.node().kind(),
                    "namespace_definition"
                        | "class_specifier"
                        | "struct_specifier"
                        | "compound_statement"
                ) {
                    scope
                } else {
                    record.name_scope
                };
                let qualification = record.qualification;
                let mut result = self.names.resolve_in(name, scope, budget)?;
                self.reference_qualifications(document, node, qualification, budget)?;
                if result.reason != Some(UnresolvedReason::Ambiguous) && !result.pending {
                    result.reason = qualification.headline().or(result.reason);
                }
                return Ok(Some(result));
            }
        }
        Ok(None)
    }

    fn render_draft(
        &self,
        draft: &mut Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<(Content<'s>, bool), CollectionError> {
        let mut keys = Vec::new();
        let mut attribute = false;
        for id in &draft.fragments {
            budget.step()?;
            let fragment = &self.index.facts.fragments[id.0];
            attribute |= fragment.form == FragmentForm::Attribute;
            keys.push((fragment.source.pieces()[0].range().start, id.0));
        }
        draft.fragments.clear();
        for (_, id) in crate::documentation::ordering::ordered(keys, budget)? {
            budget.step()?;
            draft.fragments.push(FragmentId(id));
        }
        let result = if attribute {
            self.rust_content(draft, budget)
        } else if draft.gap {
            Ok(Content::Unavailable(UnavailableReason::ExcludedGap))
        } else {
            let mut fragments = Vec::with_capacity(draft.fragments.len());
            for id in &draft.fragments {
                budget.step()?;
                fragments.push(&self.index.facts.fragments[id.0]);
            }
            crate::documentation::text::comments::render(
                self.index.profile,
                &self.index.source,
                &fragments,
                budget,
            )
        };
        let content = match result {
            Ok(content) => content,
            Err(crate::documentation::text::TextError::Source(error)) => {
                return Err(error.into());
            }
            Err(crate::documentation::text::TextError::Unavailable(reason)) => {
                Content::Unavailable(reason)
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

pub(super) fn java_package_publication<B: WorkBudget>(
    filename: Option<&str>,
    budget: &B,
) -> Result<Publication, SourceError> {
    let Some(name) = filename else {
        return Ok(Publication::Unknown);
    };
    let expected = b"package-info.java";
    if name.len() != expected.len() {
        return Ok(Publication::Ineligible);
    }
    for (&actual, &wanted) in name.as_bytes().iter().zip(expected) {
        crate::documentation::byte_scan::one(budget).map_err(Into::into)?;
        if actual != wanted {
            return Ok(Publication::Ineligible);
        }
    }
    Ok(Publication::Eligible)
}
