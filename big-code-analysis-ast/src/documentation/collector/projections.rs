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
        if !is_wrapper(self.index.profile, node.kind()) {
            return self.declaration_owners(id, budget);
        }
        if !matches!(
            self.index.canonical.nodes[id.0].projection,
            Projection::Pending
        ) {
            return Ok(());
        }
        let range = self.index.canonical.nodes[id.0].children.clone();
        if self.index.profile == Profile::BcaTypescriptSource
            && node.kind() == "ambient_declaration"
            && !self.ts_ambient_continues(id, budget)?
        {
            return Ok(());
        }
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
            if self.index.profile == Profile::BcaTypescriptSource
                && matches!(record.evidence.node().kind(), "comment" | "html_comment")
            {
                continue;
            }
            if self.index.profile == Profile::BcaTypescriptSource
                && matches!(node.kind(), "expression_statement" | "await_expression")
                && !matches!(
                    record.evidence.node().kind(),
                    "internal_module" | "assignment_expression" | "await_expression"
                )
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

    pub(super) fn ts_ambient_continues<B: WorkBudget>(
        &self,
        id: NodeId,
        budget: &B,
    ) -> Result<bool, B::Error> {
        let mut keyword_end = None;
        for &child in
            &self.index.canonical.children[self.index.canonical.nodes[id.0].children.clone()]
        {
            budget.step()?;
            let node = self.index.canonical.nodes[child.0].evidence.node();
            if node.kind() == "declare" {
                keyword_end = Some(node.end_byte());
            } else if !matches!(node.kind(), "comment" | "html_comment") && node.is_named() {
                return match keyword_end {
                    Some(end) => super::ts_line_continues(
                        self.index.source.bytes(),
                        end,
                        node.start_byte(),
                        budget,
                    ),
                    None => Ok(false),
                };
            }
        }
        Ok(false)
    }

    // The owners of a node that is no wrapper: one arm per profile that gives a
    // declaration owners of its own. A profile without such a rule changes nothing.
    fn declaration_owners(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        // bca: suppress(halstead, nexits) -- per-profile declaration owner dispatch.
        let node = self.index.canonical.nodes[id.0].evidence.node();
        if self.index.profile == Profile::Doxygen
            && matches!(node.kind(), "declaration" | "field_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.c_slots(id, budget);
        }
        if self.index.profile == Profile::Javadoc
            && matches!(node.kind(), "field_declaration" | "constant_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.java_field_slots(id, budget);
        }
        if self.index.profile == Profile::CsharpXml
            && matches!(node.kind(), "field_declaration" | "event_field_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            let Some(names) = csharp_declarator_names(&self.index.canonical, id, budget)? else {
                return Ok(());
            };
            return self.field_slots(id, names, budget);
        }
        if self.index.profile == Profile::GoDoc {
            return self.go_member_owners(id, budget);
        }
        if self.index.profile == Profile::Jsdoc
            && matches!(
                node.kind(),
                "lexical_declaration" | "variable_declaration" | "using_declaration"
            )
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.js_first_declarator_owner(id, budget);
        }
        if self.index.profile == Profile::BcaTypescriptSource
            && matches!(node.kind(), "lexical_declaration" | "variable_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.ts_binding_owner(id, budget);
        }
        if self.index.profile == Profile::Kdoc
            && node.kind() == "property_declaration"
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.kotlin_property_slot(id, budget);
        }
        if self.index.profile == Profile::Yard && node.kind() == "call" {
            return self.ruby_call_owners(id, budget);
        }
        if self.index.profile == Profile::BcaPhpSource
            && matches!(node.kind(), "property_declaration" | "const_declaration")
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.php_declaration_slot(id, budget);
        }
        if self.index.profile == Profile::Ldoc
            && matches!(
                node.kind(),
                "field" | "assignment_statement" | "variable_declaration" | "return_statement"
            )
            && matches!(
                self.index.canonical.nodes[id.0].projection,
                Projection::Pending
            )
        {
            return self.lua_declaration_owners(id, budget);
        }
        Ok(())
    }

    fn php_declaration_slot(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let property =
            self.index.canonical.nodes[id.0].evidence.node().kind() == "property_declaration";
        let Some(name) = php_single_name(&self.index.canonical, id, property, budget)? else {
            return Ok(());
        };
        let source = self.index.source.pieces_with_budget(
            self.index.canonical.nodes[name.0]
                .evidence
                .node()
                .byte_range(),
            budget,
        )?;
        let enclosing = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let slot = OwnerId(self.index.facts.owners.len());
        self.index.facts.owners.push(Owner::Slot(Slot {
            enclosing,
            role: if property {
                SlotRole::Property
            } else {
                SlotRole::Binding
            },
            ordinal: 0,
            source,
        }));
        self.index.associations_by_owner.push(Vec::new());
        self.index.assessments_by_owner.push(Vec::new());
        self.index.qualifications_by_owner.push(Vec::new());
        self.index.owner_nodes.push(id);
        self.index.canonical.nodes[id.0].projection = Projection::Ready(vec![slot]);
        Ok(())
    }

    fn ts_binding_owner(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        if let Some(child) = budget::in_phase(budget, budget::WorkPhase::Projection, || {
            ts_binding_declarator(&self.index.canonical, id, budget)
        })? {
            let owner = OwnerId(self.index.facts.owners.len());
            self.add_owner(
                child,
                Owner::Node(self.index.canonical.nodes[child.0].evidence),
            );
            self.index.canonical.nodes[id.0].projection = Projection::Ready(vec![owner]);
        }
        Ok(())
    }

    fn kotlin_property_slot(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let Some(name) = kotlin_property_name(&self.index.canonical, id, budget)? else {
            return Ok(());
        };
        let range = self.index.canonical.nodes[name.0]
            .evidence
            .node()
            .byte_range();
        let source = self.index.source.pieces_with_budget(range, budget)?;
        let enclosing = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let slot = OwnerId(self.index.facts.owners.len());
        self.index.facts.owners.push(Owner::Slot(Slot {
            enclosing,
            role: SlotRole::Property,
            ordinal: 0,
            source,
        }));
        self.index.associations_by_owner.push(Vec::new());
        self.index.assessments_by_owner.push(Vec::new());
        self.index.qualifications_by_owner.push(Vec::new());
        self.index.owner_nodes.push(id);
        self.index.canonical.nodes[id.0].projection = Projection::Ready(vec![slot]);
        Ok(())
    }

    fn js_first_declarator_owner(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        if let Some(declarator) = budget::in_phase(budget, budget::WorkPhase::Projection, || {
            js_first_declarator_with_budget(&self.index.canonical, id, budget)
        })? {
            self.add_owner(id, Owner::Node(declarator));
        }
        Ok(())
    }

    fn ruby_call_owners<B: WorkBudget<Error = SourceError>>(
        &mut self,
        id: NodeId,
        budget: &B,
    ) -> Result<(), CollectionError> {
        let record = &self.index.canonical.nodes[id.0];
        if !matches!(record.projection, Projection::Pending) {
            return Ok(());
        }
        let call = record.evidence.node();
        let Some(method) = call.child_by_field_name("method") else {
            return Ok(());
        };
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return Ok(());
        };
        let spelling = &self.index.source.bytes()[method.byte_range()];
        budget.bytes(spelling.len())?;
        let visibility = matches!(
            spelling,
            b"private"
                | b"protected"
                | b"public"
                | b"module_function"
                | b"private_class_method"
                | b"public_class_method"
        );
        let attribute = ruby_attribute_names(spelling);
        if !visibility && !attribute {
            return Ok(());
        }
        if visibility && call.child_by_field_name("receiver").is_some() {
            return Ok(());
        }
        let mut names = Vec::new();
        let mut boolean = false;
        let mut first = None;
        for &child in &self.index.canonical.children[record.children.clone()] {
            budget.step()?;
            let candidate = self.index.canonical.nodes[child.0].evidence.node();
            if candidate.kind() == "comment" {
                continue;
            }
            if candidate == arguments {
                first = Some(child);
                break;
            }
        }
        let Some(arguments_id) = first else {
            return Ok(());
        };
        for &child in &self.index.canonical.children
            [self.index.canonical.nodes[arguments_id.0].children.clone()]
        {
            budget.step()?;
            let argument = self.index.canonical.nodes[child.0].evidence.node();
            if !argument.is_named() || argument.kind() == "comment" {
                continue;
            }
            if visibility {
                if matches!(argument.kind(), "method" | "singleton_method")
                    && let Projection::Ready(owners) =
                        &self.index.canonical.nodes[child.0].projection
                {
                    for _ in owners {
                        budget.step()?;
                    }
                    self.index.canonical.nodes[id.0].projection = Projection::Ready(owners.clone());
                }
                return Ok(());
            }
            if spelling == b"attr"
                && names.len() == 1
                && !boolean
                && matches!(argument.kind(), "true" | "false")
            {
                boolean = true;
                continue;
            }
            if boolean
                || !matches!(
                    argument.kind(),
                    "simple_symbol" | "delimited_symbol" | "string"
                )
                || !Self::ruby_plain_attribute_name(argument, budget)?
            {
                return Ok(());
            }
            names.push(argument.byte_range());
        }
        if names.is_empty() {
            return Ok(());
        }
        self.ruby_property_slots(id, names, budget)
    }

    fn ruby_property_slots<B: WorkBudget<Error = SourceError>>(
        &mut self,
        id: NodeId,
        names: Vec<std::ops::Range<usize>>,
        budget: &B,
    ) -> Result<(), CollectionError> {
        let enclosing = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let mut slots = Vec::new();
        for name in names {
            budget.step()?;
            let source = self.index.source.pieces_with_budget(name, budget)?;
            let slot = OwnerId(self.index.facts.owners.len());
            self.index.facts.owners.push(Owner::Slot(Slot {
                enclosing,
                role: SlotRole::Property,
                ordinal: slots.len(),
                source,
            }));
            self.index.associations_by_owner.push(Vec::new());
            self.index.assessments_by_owner.push(Vec::new());
            self.index.qualifications_by_owner.push(Vec::new());
            self.index.owner_nodes.push(id);
            slots.push(slot);
        }
        self.index.canonical.nodes[id.0].projection = Projection::Ready(slots);
        Ok(())
    }

    fn ruby_plain_attribute_name<B: WorkBudget<Error = SourceError>>(
        name: tree_sitter::Node<'_>,
        budget: &B,
    ) -> Result<bool, CollectionError> {
        if name.kind() == "simple_symbol" {
            return Ok(true);
        }
        let mut content = 0;
        let mut cursor = name.walk();
        for child in name.children(&mut cursor) {
            budget.step()?;
            if child.is_named() {
                if !matches!(child.kind(), "string_content" | "escape_sequence") {
                    return Ok(false);
                }
                content += 1;
            }
        }
        Ok(content > 0)
    }

    fn lua_declaration_owners(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        match self.index.canonical.nodes[id.0].evidence.node().kind() {
            "field" => {
                if let Some(name) = lua_field_name(&self.index.canonical, id, budget)? {
                    self.field_slots(id, vec![name], budget)?;
                }
            }
            "assignment_statement" | "variable_declaration" => {
                if lua_single_name(&self.index.canonical, id, budget)?.is_some() {
                    self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
                }
            }
            "return_statement" => {
                self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
            }
            _ => {}
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

    fn java_field_slots(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let Some(names) = java_declarator_names(&self.index.canonical, id, budget)? else {
            return Ok(());
        };
        self.field_slots(id, names, budget)
    }

    fn go_member_owners(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let record = &self.index.canonical.nodes[id.0];
        if !matches!(record.projection, Projection::Pending)
            || !matches!(
                record.evidence.node().kind(),
                "field_declaration" | "method_elem" | "type_elem"
            )
        {
            return Ok(());
        }
        let Some(names) = go_member_names(&self.index.canonical, id, budget)? else {
            return Ok(());
        };
        if go_ends_member_early(&self.index.canonical, id, budget)? {
            // The node is not the member Go reads: it stays a pending owner.
            Ok(())
        } else if names.is_empty() {
            // An interface element, or a field that embeds a type and has no name.
            self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
            Ok(())
        } else {
            self.field_slots(id, names, budget)
        }
    }

    // The declaration owns as a node, and each name owns as a field slot in it.
    fn field_slots(
        &mut self,
        id: NodeId,
        names: Vec<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let declaration = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let mut fields = Vec::new();
        for name in names {
            budget.step()?;
            let spelling = self.index.canonical.nodes[name.0]
                .evidence
                .node()
                .byte_range();
            let source = self.index.source.pieces_with_budget(spelling, budget)?;
            let field = OwnerId(self.index.facts.owners.len());
            self.index.facts.owners.push(Owner::Slot(Slot {
                enclosing: declaration,
                role: SlotRole::Field,
                ordinal: fields.len(),
                source,
            }));
            self.index.associations_by_owner.push(Vec::new());
            self.index.assessments_by_owner.push(Vec::new());
            self.index.qualifications_by_owner.push(Vec::new());
            self.index.owner_nodes.push(id);
            fields.push(field);
        }
        self.index.canonical.nodes[id.0].projection = Projection::Ready(fields);
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

// C# nests its declarators under one variable_declaration, including event fields.
// Comments and modifiers are not names; every child read costs one budget step.
pub(super) fn csharp_declarator_names<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    budget: &B,
) -> Result<Option<Vec<NodeId>>, B::Error> {
    let mut names = Vec::new();
    let mut variables = None;
    let outer = canonical.nodes[declaration.0].children.clone();
    for &child in &canonical.children[outer] {
        budget.step()?;
        if canonical.nodes[child.0].evidence.node().kind() == "variable_declaration" {
            variables = Some(child);
            break;
        }
    }
    let Some(variables) = variables else {
        return Ok(None);
    };
    for &child in &canonical.children[canonical.nodes[variables.0].children.clone()] {
        budget.step()?;
        let declarator = &canonical.nodes[child.0];
        if !matches!(declarator.evidence.node().kind(), "variable_declarator") {
            continue;
        }
        let Some(&name) = canonical.children[declarator.children.clone()].first() else {
            return Ok(None);
        };
        if canonical.nodes[name.0].evidence.node().kind() != "identifier" {
            return Ok(None);
        }
        names.push(name);
    }
    Ok((!names.is_empty()).then_some(names))
}

// Only one element is projected. Comments are not structural children.
pub(super) fn php_single_name<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    property: bool,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    let element_kind = if property {
        "property_element"
    } else {
        "const_element"
    };
    let name_kind = if property { "variable_name" } else { "name" };
    let mut element = None;
    for &child in &canonical.children[canonical.nodes[declaration.0].children.clone()] {
        budget.step()?;
        let kind = canonical.nodes[child.0].evidence.node().kind();
        if kind == "comment" {
            continue;
        }
        if kind == element_kind && element.replace(child).is_some() {
            return Ok(None);
        }
    }
    let Some(element) = element else {
        return Ok(None);
    };
    let mut name = None;
    for &child in &canonical.children[canonical.nodes[element.0].children.clone()] {
        budget.step()?;
        let kind = canonical.nodes[child.0].evidence.node().kind();
        if kind == "comment" {
            continue;
        }
        if kind == "=" {
            break;
        }
        if kind == name_kind && name.replace(child).is_some() {
            return Ok(None);
        }
    }
    Ok(name)
}

pub(super) fn js_first_declarator_with_budget<'s, B: WorkBudget>(
    canonical: &CanonicalNodes<'s>,
    id: NodeId,
    budget: &B,
) -> Result<Option<NodeEvidence<'s>>, B::Error> {
    for &child in &canonical.children[canonical.nodes[id.0].children.clone()] {
        budget.step()?;
        let declarator = &canonical.nodes[child.0];
        if declarator.evidence.node().kind() != "variable_declarator" {
            continue;
        }
        for &name in &canonical.children[declarator.children.clone()] {
            budget.step()?;
            let node = canonical.nodes[name.0].evidence.node();
            if matches!(node.kind(), "comment" | "html_comment") {
                continue;
            }
            return Ok((node.kind() == "identifier").then_some(declarator.evidence));
        }
        return Ok(None);
    }
    Ok(None)
}

fn ruby_attribute_names(spelling: &[u8]) -> bool {
    matches!(
        spelling,
        b"attr" | b"attr_reader" | b"attr_writer" | b"attr_accessor"
    )
}

#[cfg(test)]
pub(super) fn ruby_attribute_name<B: WorkBudget<Error = SourceError>>(
    name: tree_sitter::Node<'_>,
    budget: &B,
) -> Result<bool, CollectionError> {
    Builder::ruby_plain_attribute_name(name, budget)
}

#[cfg(test)]
pub(super) fn ruby_slot_cost<B: WorkBudget<Error = SourceError>>(
    index: DocumentationIndex<'_>,
    node: tree_sitter::Node<'_>,
    budget: &B,
) -> Result<usize, CollectionError> {
    let id = index.canonical.lookup(node)?;
    let mut builder = Builder {
        index,
        drafts: Vec::new(),
        pending_attributes: Vec::new(),
        rust_values: std::collections::HashMap::new(),
        names: super::super::names::Names::default(),
        pod_doubt: crate::documentation::collector::pod_doubt::Doubt::default(),
        pod_hidden: crate::documentation::collector::pod::Hidden::default(),
        tcl_scan: crate::lang_helpers::tcl_scan::CommandScan::default(),
        shdoc: crate::lang_helpers::shdoc::Reading::default(),
    };
    builder.index.canonical.nodes[id.0].projection = Projection::Pending;
    builder.ruby_call_owners(id, budget)?;
    match &builder.index.canonical.nodes[id.0].projection {
        Projection::Ready(owners) => Ok(owners.len()),
        _ => Ok(0),
    }
}

#[cfg(test)]
pub(super) fn ruby_call_comment_cost<B: WorkBudget<Error = SourceError>>(
    mut index: DocumentationIndex<'_>,
    call: tree_sitter::Node<'_>,
    comment: tree_sitter::Node<'_>,
    budget: &B,
) -> Result<usize, CollectionError> {
    let call_id = index.canonical.lookup(call)?;
    let comment_id = index.canonical.lookup(comment)?;
    let old = index.canonical.nodes[call_id.0].children.clone();
    let children = index.canonical.children[old].to_vec();
    let start = index.canonical.children.len();
    for (at, child) in children.into_iter().enumerate() {
        if at == 1 {
            index.canonical.children.push(comment_id);
        }
        index.canonical.children.push(child);
    }
    index.canonical.nodes[call_id.0].children = start..index.canonical.children.len();
    ruby_slot_cost(index, call, budget)
}

pub(super) fn ts_binding_declarator<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    for &child in &canonical.children[canonical.nodes[declaration.0].children.clone()] {
        budget.step()?;
        let record = &canonical.nodes[child.0];
        if record.evidence.node().kind() != "variable_declarator" {
            continue;
        }
        budget.step()?;
        return Ok(record
            .evidence
            .node()
            .child(0)
            .filter(|name| name.kind() == "identifier")
            .map(|_| child));
    }
    Ok(None)
}

// A destructuring property has no single declaration name. Comment nodes are trivia,
// including when the grammar places one before the identifier in its declaration.
pub(super) fn kotlin_property_name<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    property: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    // bca: suppress(nexits) -- two-stage child scan with budget propagation.
    let mut declaration = None;
    for &child in &canonical.children[canonical.nodes[property.0].children.clone()] {
        budget.step()?;
        match canonical.nodes[child.0].evidence.node().kind() {
            "multi_variable_declaration" => return Ok(None),
            "variable_declaration" if declaration.replace(child).is_some() => return Ok(None),
            _ => {}
        }
    }
    let Some(declaration) = declaration else {
        return Ok(None);
    };
    for &child in &canonical.children[canonical.nodes[declaration.0].children.clone()] {
        budget.step()?;
        match canonical.nodes[child.0].evidence.node().kind() {
            "line_comment" | "block_comment" => {}
            "identifier" | "simple_identifier" => return Ok(Some(child)),
            _ => return Ok(None),
        }
    }
    Ok(None)
}

// Field names are not requested for this profile (`fields_needed`), so a
// declarator is told by its kind and its name by position.
pub(super) fn java_declarator_names<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    budget: &B,
) -> Result<Option<Vec<NodeId>>, B::Error> {
    let mut names = Vec::new();
    for &child in &canonical.children[canonical.nodes[declaration.0].children.clone()] {
        budget.step()?;
        let declarator = &canonical.nodes[child.0];
        if declarator.evidence.node().kind() != "variable_declarator" {
            continue;
        }
        match canonical.children[declarator.children.clone()].first() {
            Some(&name) if canonical.nodes[name.0].evidence.node().kind() == "identifier" => {
                names.push(name);
            }
            // An unnamed variable (`_`) has no name to own documentation. A
            // declarator without a child is not reachable from an error-free parse.
            _ => return Ok(None),
        }
    }
    // The grammar requires one declarator; an empty list is not reachable either.
    Ok((!names.is_empty()).then_some(names))
}

// Go ends a line at a comment that holds a line break, and a line end after a name, a
// type or a closing bracket ends the member. The grammar reads on, so a member with
// such a comment among its children may not be the member Go reads: `None`. Otherwise
// the names of a field declaration, in order; none for a field that embeds a type or
// for an interface element.
pub(super) fn go_member_names<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    member: NodeId,
    budget: &B,
) -> Result<Option<Vec<NodeId>>, B::Error> {
    let field = canonical.nodes[member.0].evidence.node().kind() == "field_declaration";
    let mut names = Vec::new();
    for &child in &canonical.children[canonical.nodes[member.0].children.clone()] {
        budget.step()?;
        let node = canonical.nodes[child.0].evidence.node();
        match node.kind() {
            "comment" if node.start_position().row != node.end_position().row => {
                return Ok(None);
            }
            "field_identifier" if field => names.push(child),
            _ => {}
        }
    }
    Ok(Some(names))
}

// Counts the nodes `go_ends_member_early` reads. No assertion on an index can tell
// whether a nested body is read once or once for every member around it.
crate::observation::counter!(member_reads);

// Go puts a semicolon at a line end after a name, a literal or a closing bracket, and a
// comment that ends a line is such a line end. The grammar reads some members on past
// it (`X [2]\nint`, `X func() /* a\n b */ int`), and then the node is not the member Go
// reads. A token is a node without children, or a string literal. The members between
// the braces of a nested struct or interface type and the statements of a function
// literal are not tokens of this member, so no node is read for two members.
pub(super) fn go_ends_member_early<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    member: NodeId,
    budget: &B,
) -> Result<bool, B::Error> {
    let mut open = vec![(canonical.nodes[member.0].children.clone(), false)];
    let mut line_end = None;
    while let Some((children, body)) = open.last_mut() {
        let Some(at) = children.next() else {
            open.pop();
            continue;
        };
        budget.step()?;
        member_reads::record();
        let record = &canonical.nodes[canonical.children[at].0];
        let node = record.evidence.node();
        let kind = node.kind();
        if kind == "comment" {
            continue;
        }
        if !record.children.is_empty()
            && !matches!(kind, "raw_string_literal" | "interpreted_string_literal")
        {
            let body = matches!(kind, "field_declaration_list" | "interface_type" | "block");
            open.push((record.children.clone(), body));
            continue;
        }
        if line_end.is_some_and(|row| row < node.start_position().row) {
            return Ok(true);
        }
        line_end = go_line_end(node);
        if *body && kind == "{" {
            children.start = children.start.max(children.end.saturating_sub(1));
        }
    }
    Ok(false)
}

// The row on which a token ends, if Go ends a line after it: an identifier or a literal,
// which are the named tokens of the grammar, or a closing bracket.
fn go_line_end(token: tree_sitter::Node<'_>) -> Option<usize> {
    (token.is_named() || matches!(token.kind(), ")" | "]" | "}")).then(|| token.end_position().row)
}

pub(super) fn lua_field_name<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    field: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    for &child in &canonical.children[canonical.nodes[field.0].children.clone()] {
        budget.step()?;
        let kind = canonical.nodes[child.0].evidence.node().kind();
        if kind != "comment" {
            return Ok((kind == "identifier").then_some(child));
        }
    }
    Ok(None)
}

pub(super) fn lua_declared_list<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    let kind = canonical.nodes[declaration.0].evidence.node().kind();
    let children = canonical.nodes[declaration.0].children.clone();
    for &child in &canonical.children[children] {
        budget.step()?;
        let child_kind = canonical.nodes[child.0].evidence.node().kind();
        if child_kind == "comment" || !canonical.nodes[child.0].evidence.node().is_named() {
            continue;
        }
        return match (kind, child_kind) {
            ("variable_declaration", "assignment_statement") => {
                lua_declared_list(canonical, child, budget)
            }
            ("variable_declaration" | "assignment_statement", "variable_list") => Ok(Some(child)),
            _ => Ok(None),
        };
    }
    Ok(None)
}

pub(super) fn lua_list_single_name<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    list: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    let mut name = None;
    for &child in &canonical.children[canonical.nodes[list.0].children.clone()] {
        budget.step()?;
        let node = canonical.nodes[child.0].evidence.node();
        if node.kind() == "comment" || !node.is_named() {
            continue;
        }
        if !matches!(node.kind(), "identifier" | "dot_index_expression") || name.is_some() {
            return Ok(None);
        }
        name = Some(child);
    }
    Ok(name)
}

pub(super) fn lua_single_name<B: WorkBudget>(
    canonical: &CanonicalNodes<'_>,
    declaration: NodeId,
    budget: &B,
) -> Result<Option<NodeId>, B::Error> {
    let Some(list) = lua_declared_list(canonical, declaration, budget)? else {
        return Ok(None);
    };
    lua_list_single_name(canonical, list, budget)
}
