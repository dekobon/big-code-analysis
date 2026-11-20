//! Semantic wrapper and declarator projections, finalized once on exit.

use super::*;

impl Builder<'_> {
    pub(super) fn finalize_projection(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        budget.step()?;
        let node = self.index.canonical.nodes[id.0].evidence.node();
        if self.index.profile == Profile::Doxygen
            && node.has_error()
            && matches!(
                node.kind(),
                "function_definition" | "declaration" | "field_declaration"
            )
        {
            self.index_recovered_name(id, budget)?;
        }
        if node.has_error()
            || node.is_missing()
            || !self
                .index
                .source
                .covers_with_budget(node.byte_range(), budget)?
        {
            return Ok(());
        }
        if self.index.profile == Profile::Doxygen && node.kind() == "function_definition" {
            self.index_function_definition(id, budget)?;
        }
        if self.index.profile == Profile::Doxygen
            && matches!(node.kind(), "declaration" | "field_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.c_slots(id, budget);
        }
        if !is_wrapper(self.index.profile, node.kind())
            || !matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return Ok(());
        }
        let range = self.index.canonical.nodes[id.0].children.clone();
        let mut owners = Vec::new();
        for &child in &self.index.canonical.children[range] {
            budget.step()?;
            let record = &self.index.canonical.nodes[child.0];
            if matches!(
                self.index.profile,
                Profile::Jsdoc | Profile::BcaTypescriptSource
            ) && node.kind() == "export_statement"
                && !matches!(record.field, Some("declaration" | "value"))
            {
                continue;
            }
            if self.index.profile == Profile::Yard
                && matches!(node.kind(), "if_modifier" | "unless_modifier")
                && record.field != Some("body")
            {
                continue;
            }
            if (self.index.profile == Profile::Ldoc
                || matches!(node.kind(), "then" | "ambient_declaration"))
                && !record.evidence.node().is_named()
            {
                continue;
            }
            match &record.projection {
                Projection::Ready(ids) => {
                    for &owner in ids {
                        budget.step()?;
                        owners.push(owner);
                    }
                }
                Projection::Pending => {
                    self.index.canonical.nodes[id.0].projection = Projection::Pending;
                    return Ok(());
                }
                Projection::Unsupported => {
                    self.index.canonical.nodes[id.0].projection = Projection::Unsupported;
                    return Ok(());
                }
            }
            break;
        }
        if !owners.is_empty() {
            self.index.canonical.nodes[id.0].projection = Projection::Ready(owners);
        }
        Ok(())
    }

    fn declarator_child(
        &self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<Option<NodeId>, CollectionError> {
        for &child in
            &self.index.canonical.children[self.index.canonical.nodes[id.0].children.clone()]
        {
            budget.step()?;
            if self.index.canonical.nodes[child.0].field == Some("declarator") {
                return Ok(Some(child));
            }
        }
        Ok(None)
    }

    fn declared_name(
        &self,
        mut current: NodeId,
        budget: &CollectionBudget,
    ) -> Result<Option<(NodeId, bool)>, CollectionError> {
        let mut function = false;
        loop {
            budget.step()?;
            let node = self.index.canonical.nodes[current.0].evidence.node();
            if matches!(node.kind(), "identifier" | "field_identifier") {
                return Ok(Some((current, function)));
            }
            function |= node.kind() == "function_declarator";
            let Some(child) = self.declarator_child(current, budget)? else {
                return Ok(None);
            };
            current = child;
        }
    }

    fn c_slots(&mut self, id: NodeId, budget: &CollectionBudget) -> Result<(), CollectionError> {
        let mut work = Vec::new();
        for &child in
            &self.index.canonical.children[self.index.canonical.nodes[id.0].children.clone()]
        {
            budget.step()?;
            if self.index.canonical.nodes[child.0].field == Some("declarator") {
                let Some(declared) = self.declared_name(child, budget)? else {
                    return Ok(());
                };
                work.push(declared);
            }
        }
        if work.is_empty() {
            return Ok(());
        }
        if work.len() > 1 {
            for &(_, function) in &work {
                budget.step()?;
                if function {
                    return Ok(());
                }
            }
        }
        let enclosing = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let mut slots = Vec::new();
        for (name, function) in work {
            budget.step()?;
            let node = self.index.canonical.nodes[name.0].evidence.node();
            if function {
                slots.push(enclosing);
                let declaration = self.index.canonical.nodes[id.0].evidence.node();
                let mut end = declaration.end_byte();
                if self.index.source.bytes().get(end.saturating_sub(1)) == Some(&b';') {
                    end -= 1;
                }
                let complex = self.complex_header(id, budget)?;
                let record = &self.index.canonical.nodes[id.0];
                self.names.insert_candidate(
                    &self.index.source.bytes()[node.byte_range()],
                    (!complex).then(|| &self.index.source.bytes()[declaration.start_byte()..end]),
                    record.name_scope,
                    super::super::names::NamedOwner {
                        owner: enclosing,
                        qualification: record.qualification,
                    },
                    budget,
                )?;
                if complex {
                    self.pending_name(id);
                }
            } else {
                let source = self
                    .index
                    .source
                    .pieces_with_budget(node.byte_range(), budget)?;
                let slot = OwnerId(self.index.facts.owners.len());
                self.index.facts.owners.push(Owner::Slot(Slot {
                    enclosing,
                    role: SlotRole::Field,
                    ordinal: slots.len(),
                    source,
                }));
                self.index.associations_by_owner.push(Vec::new());
                self.index.assessments_by_owner.push(Vec::new());
                self.index.qualifications_by_owner.push(Vec::new());
                self.index.owner_nodes.push(id);
                slots.push(slot);
            }
        }
        self.index.canonical.nodes[id.0].projection = Projection::Ready(slots);
        Ok(())
    }

    fn index_function_definition(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let Projection::Ready(owners) = &self.index.canonical.nodes[id.0].projection else {
            return Ok(());
        };
        let Some(&owner) = owners.first() else {
            return Ok(());
        };
        let node = self.index.canonical.nodes[id.0].evidence.node();
        let Some(declarator) = self.declarator_child(id, budget)? else {
            return Ok(());
        };
        let Some((name, _)) = self.declared_name(declarator, budget)? else {
            return Ok(());
        };
        let end = self.index.canonical.nodes[declarator.0]
            .evidence
            .node()
            .end_byte();
        if self
            .index
            .source
            .covers_with_budget(node.start_byte()..end, budget)?
        {
            let complex = self.complex_header(id, budget)?;
            let record = &self.index.canonical.nodes[id.0];
            self.names.insert_candidate(
                &self.index.source.bytes()[self.index.canonical.nodes[name.0]
                    .evidence
                    .node()
                    .byte_range()],
                (!complex).then(|| &self.index.source.bytes()[node.start_byte()..end]),
                record.name_scope,
                super::super::names::NamedOwner {
                    owner,
                    qualification: record.qualification,
                },
                budget,
            )?;
            if complex {
                self.pending_name(id);
            }
        }
        Ok(())
    }

    fn pending_name(&mut self, id: NodeId) {
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: AssessmentAxis::Owner,
            reason: "Complex signature awaits exact named matching",
            node: Some(self.index.canonical.nodes[id.0].evidence),
            document: None,
        });
        self.index.canonical.nodes[id.0].pending.push(assessment);
    }

    fn complex_header(
        &self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<bool, CollectionError> {
        for &child in
            &self.index.canonical.children[self.index.canonical.nodes[id.0].children.clone()]
        {
            budget.step()?;
            let record = &self.index.canonical.nodes[child.0];
            if record.field != Some("body") && record.signature_complex {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn index_recovered_name(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let Some(declarator) = self.declarator_child(id, budget)? else {
            return Ok(());
        };
        let Some((name, true)) = self.declared_name(declarator, budget)? else {
            return Ok(());
        };
        let name_node = self.index.canonical.nodes[name.0].evidence.node();
        if name_node.is_missing()
            || !self
                .index
                .source
                .covers_with_budget(name_node.byte_range(), budget)?
        {
            return Ok(());
        }
        let owner = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        self.index.canonical.nodes[id.0].projection = Projection::Pending;
        self.names.insert_candidate(
            &self.index.source.bytes()[name_node.byte_range()],
            None,
            self.index.canonical.nodes[id.0].name_scope,
            super::super::names::NamedOwner {
                owner,
                qualification: {
                    let mut qualification = self.index.canonical.nodes[id.0].qualification;
                    qualification.insert(UnresolvedReason::GrammarBoundary);
                    qualification
                },
            },
            budget,
        )?;
        self.pending_name(id);
        Ok(())
    }
}
