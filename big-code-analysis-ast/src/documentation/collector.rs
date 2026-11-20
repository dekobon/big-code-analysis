//! Coordinated source-order traversal and immutable-index construction.

use super::budget::{CollectionBudget, WorkBudget};
use super::canonical::{CanonicalNodes, NodeId, Projection};
use super::streaming::{markers, positional};
use super::*;
use crate::{Ancestors, Checker, FoldCache, Node, SiblingCache};

enum Visit {
    Enter(NodeId, usize),
    Exit(NodeId, usize),
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn collect_typed<'s, T: Checker + crate::traits::LanguageInfo>(
    language: crate::LANG,
    tree: &'s tree_sitter::Tree,
    source: &'s [u8],
    context: &DocumentationContext,
    options: &DocumentationOptions,
) -> Result<DocumentationIndex<'s>, CollectionError> {
    let budget = CollectionBudget::default();
    budget.checkpoint()?;
    let grammar = language
        .tree_sitter_language()
        .map_err(|_| CollectionError::LanguageDisabled(language))?;
    let profile = options.profile.resolve(language)?;
    if *tree.language() != grammar {
        return Err(CollectionError::GrammarMismatch);
    }
    let root = tree.root_node();
    bounds(root, source.len())?;
    let source = SourceContext::with_budget(tree, source, &budget)?;
    let canonical = CanonicalNodes::new(source.node(root)?);
    let logical_filename = if let Some(filename) = &context.logical_filename {
        let mut copied = String::new();
        for character in filename.chars() {
            budget.bytes(character.len_utf8())?;
            copied.push(character);
        }
        Some(copied)
    } else {
        None
    };
    let mut builder = Builder {
        index: DocumentationIndex {
            source,
            facts: Facts::default(),
            canonical,
            context: DocumentationContext { logical_filename },
            options: *options,
            profile,
            capability: if profile == Profile::Helper {
                Capability::NoDeclarationModel
            } else {
                Capability::Partial
            },
            assessments: Vec::new(),
            evidence: Vec::new(),
            associations_by_owner: Vec::new(),
            assessments_by_owner: Vec::new(),
            owner_nodes: Vec::new(),
            documents_ordered: Vec::new(),
            associations_ordered: Vec::new(),
        },
        drafts: Vec::new(),
        pending_attributes: Vec::new(),
        names: super::names::Names::default(),
    };
    let root_id = builder
        .index
        .canonical
        .insert(builder.index.source.node(root)?, None, 0);
    if profile != Profile::Helper {
        builder.add_owner(root_id, Owner::File(builder.index.source.node(root)?));
        builder.walk::<T>(&budget)?;
        builder.finish(&budget)?;
    }
    budget.checkpoint()?;
    Ok(builder.index)
}

fn bounds(node: tree_sitter::Node<'_>, length: usize) -> Result<(), CollectionError> {
    if node.start_byte() > node.end_byte() || node.end_byte() > length {
        Err(CollectionError::SourceBounds)
    } else {
        Ok(())
    }
}

struct Draft<'s> {
    fragments: Vec<FragmentId>,
    target: Option<NodeId>,
    scope: NodeId,
    binding: Option<NodeEvidence<'s>>,
    role: DocumentRole,
    unresolved: Option<UnresolvedReason>,
    pending: bool,
    gap: bool,
}

struct Builder<'s> {
    index: DocumentationIndex<'s>,
    drafts: Vec<Draft<'s>>,
    pending_attributes: Vec<(NodeId, FragmentId, Option<NodeId>)>,
    names: super::names::Names,
}

trait OwnerCandidates: Checker {
    fn owner_candidate<'s>(node: &Node<'s>, code: &[u8], ancestors: Ancestors<'s, '_>) -> bool;
}

impl<T: Checker + crate::traits::LanguageInfo> OwnerCandidates for T {
    fn owner_candidate<'s>(node: &Node<'s>, code: &[u8], ancestors: Ancestors<'s, '_>) -> bool {
        match T::lang() {
            crate::LANG::Typescript => {
                crate::roles::typescript::doc_target::owns(node, code, ancestors)
            }
            crate::LANG::Tsx => crate::roles::tsx::doc_target::owns(node, code, ancestors),
            _ => T::is_source_doc_target(node, code, ancestors),
        }
    }
}

// Counts the actual primary visits and owner classification requests.
crate::observation::counter!(visits, cfg(any(test, feature = "test-support")));
crate::observation::counter!(
    target_classifications,
    cfg(any(test, feature = "test-support"))
);
crate::observation::counter!(owner_projections, cfg(any(test, feature = "test-support")));
crate::observation::counter!(child_batches, cfg(any(test, feature = "test-support")));

impl<'s> Builder<'s> {
    fn add_owner(&mut self, id: NodeId, owner: Owner<'s>) {
        owner_projections::record();
        let owner_id = OwnerId(self.index.facts.owners.len());
        self.index.facts.owners.push(owner);
        self.index.associations_by_owner.push(Vec::new());
        self.index.assessments_by_owner.push(Vec::new());
        self.index.owner_nodes.push(id);
        self.index.canonical.nodes[id.0].projection = Projection::Ready(vec![owner_id]);
    }

    fn walk<T: Checker + crate::traits::LanguageInfo>(
        &mut self,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let mut stack = vec![Visit::Enter(NodeId(0), 0)];
        let mut chain = Vec::new();
        let folds = FoldCache::default();
        let mut siblings = SiblingCache::default();
        let root = Node::from_tree_sitter(self.index.canonical.nodes[0].evidence.node());
        let mut cursor = root.as_tree_sitter().walk();
        let mut children = Vec::new();
        let mut fields = Vec::new();
        while let Some(visit) = stack.pop() {
            budget.step()?;
            match visit {
                Visit::Exit(id, depth) => {
                    chain.truncate(depth);
                    self.finalize_projection(id, budget)?;
                }
                Visit::Enter(id, depth) => {
                    visits::record();
                    chain.truncate(depth);
                    let node =
                        Node::from_tree_sitter(self.index.canonical.nodes[id.0].evidence.node());
                    bounds(node.as_tree_sitter(), self.index.source.bytes().len())?;
                    let ancestors = Ancestors::for_walk(&chain, &node, &folds, &siblings);
                    self.register::<T>(id, &node, ancestors, budget)?;
                    self.recognize::<T>(id, &node, ancestors, budget)?;
                    children.clear();
                    fields.clear();
                    cursor.reset(node.as_tree_sitter());
                    if cursor.goto_first_child() {
                        loop {
                            budget.step()?;
                            let child = cursor.node();
                            bounds(child, self.index.source.bytes().len())?;
                            children.push(Node::from_tree_sitter(child));
                            fields.push(if fields_needed(self.index.profile, node.kind()) {
                                field_requests::record();
                                cursor.field_name()
                            } else {
                                None
                            });
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                    child_batches::record();
                    self.index
                        .canonical
                        .batch(id, &children, &self.index.source, budget)?;
                    let mut following = None;
                    for &child in self.index.canonical.children
                        [self.index.canonical.nodes[id.0].children.clone()]
                    .iter()
                    .rev()
                    {
                        budget.step()?;
                        let record = &mut self.index.canonical.nodes[child.0];
                        record.following = following;
                        let child_node = Node::from_tree_sitter(record.evidence.node());
                        if !T::is_comment(&child_node) && child_node.kind() != "attribute_item" {
                            following = Some(child);
                        }
                    }
                    for (&child, field) in self.index.canonical.children
                        [self.index.canonical.nodes[id.0].children.clone()]
                    .iter()
                    .zip(&fields)
                    {
                        budget.step()?;
                        self.index.canonical.nodes[child.0].field = *field;
                    }
                    siblings.record_with_budget(&node, depth, children.iter().copied(), budget)?;
                    stack.push(Visit::Exit(id, depth));
                    for &child in self.index.canonical.children
                        [self.index.canonical.nodes[id.0].children.clone()]
                    .iter()
                    .rev()
                    {
                        budget.step()?;
                        stack.push(Visit::Enter(child, depth + 1));
                    }
                    chain.push(node);
                }
            }
        }
        debug_assert!(chain.is_empty());
        Ok(())
    }

    fn register<T: Checker + crate::traits::LanguageInfo>(
        &mut self,
        id: NodeId,
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        if id.0 == 0 {
            return Ok(());
        }
        budget.step()?;
        target_classifications::record();
        let wrapper_child = matches!(
            self.index.profile,
            Profile::Jsdoc | Profile::BcaTypescriptSource
        ) && ancestors
            .parent(node)
            .is_some_and(|parent| is_wrapper(self.index.profile, parent.kind()));
        let qualifies = if wrapper_child {
            T::owner_candidate(node, self.index.source.bytes(), ancestors)
        } else {
            T::is_source_doc_target(node, self.index.source.bytes(), ancestors)
        };
        if qualifies {
            if is_wrapper(self.index.profile, node.kind())
                || needs_slot(self.index.profile, node.kind())
                || (self.index.profile == Profile::Doxygen
                    && matches!(
                        node.kind(),
                        "preproc_if"
                            | "preproc_ifdef"
                            | "preproc_else"
                            | "preproc_elif"
                            | "preproc_elifdef"
                            | "linkage_specification"
                            | "template_declaration"
                            | "expression_statement"
                    ))
                || node.is_error()
                || node.is_missing()
            {
                self.index.canonical.nodes[id.0].projection = Projection::Pending;
            } else {
                self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
            }
        }
        Ok(())
    }

    fn recognize<T: Checker>(
        &mut self,
        id: NodeId,
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        budget.step()?;
        if self.index.profile == Profile::Rust
            && matches!(node.kind(), "attribute_item" | "inner_attribute_item")
        {
            self.rust_attribute(id, node, budget)?;
        }
        if !T::is_comment(node) {
            return Ok(());
        }
        let mut marker_runs = Vec::new();
        markers::source_comment::<T>(
            node,
            ancestors,
            &mut markers::SourceRequest {
                context: &self.index.source,
                retain_unbound: self.index.options.retain_unbound,
                sink: &mut |run| marker_runs.push(run),
            },
        )?;
        for run in marker_runs {
            self.marker_run(run, budget)?;
        }
        let mut positional_runs = Vec::new();
        positional::source_comment::<T>(
            node,
            ancestors,
            &mut positional::SourceRequest {
                context: &self.index.source,
                retain_unbound: self.index.options.retain_unbound,
                retain_directives: self.index.options.retain_directives,
                sink: &mut |run| positional_runs.push(run),
            },
        )?;
        for run in positional_runs {
            self.positional_run(run, budget)?;
        }
        if self.index.profile == Profile::Ldoc
            && ancestors.parent(node).is_some_and(|parent| {
                matches!(parent.kind(), "parameters" | "field" | "table_constructor")
            })
        {
            let assessment = self.index.assessments.len();
            self.index.assessments.push(Assessment {
                axis: AssessmentAxis::Selection,
                reason: "Native LDoc parameter/table comment awaits adapter context",
                node: Some(self.index.canonical.nodes[id.0].evidence),
                document: None,
            });
            let scope = self.index.canonical.nodes[id.0].parent.unwrap_or(NodeId(0));
            self.index.canonical.nodes[scope.0].pending.push(assessment);
        }
        Ok(())
    }

    fn evidence_id(
        &self,
        node: Option<NodeEvidence<'s>>,
    ) -> Result<Option<NodeId>, CollectionError> {
        node.map(|node| self.index.canonical.lookup(node.node()).map_err(Into::into))
            .transpose()
    }

    fn fragment(
        &mut self,
        source: SourcePieces<'s>,
        node: NodeEvidence<'s>,
        form: FragmentForm,
    ) -> FragmentId {
        let id = FragmentId(self.index.facts.fragments.len());
        self.index.facts.fragments.push(Fragment {
            source,
            node: Some(node),
            form,
        });
        id
    }

    fn marker_run(
        &mut self,
        run: markers::MarkerRun<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let target = self.evidence_id(run.target)?;
        let scope = self.evidence_id(run.containing)?.unwrap_or(NodeId(0));
        let code_before = self.evidence_id(run.code_before)?;
        let mut local: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
        let mut gathered: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        let mut forward = Draft {
            fragments: Vec::new(),
            target,
            scope,
            binding: run.target,
            role: DocumentRole::Primary,
            unresolved: run.target_uncertainty,
            pending: false,
            gap: run.prefix_gap || run.suffix_gap,
        };
        for member in run.members {
            budget.step()?;
            if member.selection != markers::MarkerSelection::Selected {
                let fragment = self.fragment(member.source, member.node, FragmentForm::Comment);
                self.index.evidence.push(CandidateEvidence {
                    fragment,
                    disposition: if member.selection == markers::MarkerSelection::Superseded {
                        CandidateDisposition::Superseded
                    } else {
                        CandidateDisposition::Unattached
                    },
                });
                continue;
            }
            let id = self.fragment(member.source, member.node, FragmentForm::Comment);
            if matches!(member.placement, markers::MarkerPlacement::Forward) {
                forward.fragments.push(id);
                forward.unresolved = forward.unresolved.or(member.uncertainty);
            } else {
                let mut draft = Draft {
                    fragments: vec![id],
                    target: None,
                    scope,
                    binding: run.target,
                    role: DocumentRole::Primary,
                    unresolved: member.uncertainty,
                    pending: false,
                    gap: run.prefix_gap || run.suffix_gap,
                };
                let local_node =
                    if let markers::MarkerPlacement::Gathered(gatherer) = member.placement {
                        gatherer
                    } else {
                        member.node
                    };
                self.local_target(&mut draft, local_node, member.placement, budget)?;
                if self.index.profile == Profile::Doxygen {
                    let raw = &self.index.source.bytes()[member.node.node().byte_range()];
                    if matches!(raw.get(..4), Some(b"///<" | b"//!<" | b"/**<" | b"/*!<")) {
                        draft.target = code_before;
                        draft.pending = code_before.is_none();
                    }
                }
                if matches!(
                    member.placement,
                    markers::MarkerPlacement::Gatherer | markers::MarkerPlacement::Gathered(_)
                ) {
                    if let Some(&index) = gathered.get(&local_node.node().id()) {
                        self.drafts[index].fragments.push(id);
                    } else {
                        gathered.insert(local_node.node().id(), self.drafts.len());
                        self.drafts.push(draft);
                    }
                } else if self.index.profile == Profile::Rust
                    && let Some(target) = draft.target
                {
                    if let Some(&index) = local.get(&target) {
                        self.drafts[index].fragments.push(id);
                    } else {
                        local.insert(target, self.drafts.len());
                        self.drafts.push(draft);
                    }
                } else {
                    self.drafts.push(draft);
                }
            }
        }
        if !forward.fragments.is_empty() {
            self.drafts.push(forward);
        }
        Ok(())
    }

    fn positional_run(
        &mut self,
        run: positional::SourceRun<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let target = self.evidence_id(run.target)?;
        let scope = self.evidence_id(run.containing)?.unwrap_or(NodeId(0));
        let mut draft = Draft {
            fragments: Vec::new(),
            target,
            scope,
            binding: run.target,
            role: DocumentRole::Primary,
            unresolved: run.uncertainty,
            pending: false,
            gap: run.prefix_gap || run.suffix_gap,
        };
        for member in run.members {
            budget.step()?;
            let fragment = self.fragment(member.source, member.node, FragmentForm::Comment);
            if member.selected {
                draft.fragments.push(fragment);
            } else {
                self.index.evidence.push(CandidateEvidence {
                    fragment,
                    disposition: if member.directive {
                        CandidateDisposition::Directive
                    } else {
                        CandidateDisposition::Unattached
                    },
                });
            }
        }
        if draft.fragments.is_empty() {
            return Ok(());
        }
        match run.selection {
            positional::RunSelection::Opener(positional::OpenerReason::FirstFile) => {
                draft.target = Some(NodeId(0));
                draft.role = DocumentRole::FilePackage;
            }
            positional::RunSelection::Opener(
                positional::OpenerReason::Explicit | positional::OpenerReason::Tagged,
            ) => {
                draft.target = None;
                draft.pending = true;
            }
            _ => {}
        }
        self.drafts.push(draft);
        Ok(())
    }

    fn local_target(
        &self,
        draft: &mut Draft<'s>,
        node: NodeEvidence<'s>,
        placement: markers::MarkerPlacement<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        budget.step()?;
        if draft.unresolved == Some(UnresolvedReason::Expansion) {
            return Ok(());
        }
        if self.index.profile == Profile::Doxygen {
            let raw = &self.index.source.bytes()[node.node().byte_range()];
            if let Some((command, _)) = crate::lang_helpers::doxygen::structural_command(raw) {
                if command == b"file" {
                    draft.target = Some(NodeId(0));
                    draft.role = DocumentRole::FilePackage;
                } else {
                    draft.target = None;
                    draft.pending = true;
                }
                return Ok(());
            }
        }
        if self.index.profile == Profile::Doxygen
            && !matches!(
                placement,
                markers::MarkerPlacement::Gatherer | markers::MarkerPlacement::Gathered(_)
            )
        {
            draft.pending = true;
        }
        match self.index.profile {
            Profile::Rust => {
                if draft.scope.0 == 0 {
                    draft.target = Some(NodeId(0));
                    draft.role = DocumentRole::FilePackage;
                } else if let Some(parent) = self.index.canonical.nodes[draft.scope.0].parent {
                    let evidence = self.index.canonical.nodes[parent.0].evidence;
                    if matches!(evidence.node().kind(), "mod_item" | "function_item") {
                        draft.target = Some(parent);
                        if evidence.node().kind() == "mod_item" {
                            draft.role = DocumentRole::FilePackage;
                        }
                    } else {
                        draft.pending = true;
                    }
                } else {
                    draft.pending = true;
                }
            }
            Profile::BcaPhpSource if draft.scope.0 == 0 => {
                draft.target = Some(NodeId(0));
                draft.role = DocumentRole::FilePackage;
            }
            Profile::Jsdoc | Profile::BcaTypescriptSource => {
                let raw = &self.index.source.bytes()[node.node().byte_range()];
                let file = if self.index.profile == Profile::BcaTypescriptSource {
                    crate::lang_helpers::doc_text::tsdoc_module(raw)
                } else {
                    crate::lang_helpers::doc_text::jsdoc_file(raw)
                };
                if file {
                    draft.target = Some(NodeId(0));
                    draft.role = DocumentRole::FilePackage;
                } else {
                    draft.pending = true;
                }
            }
            _ => draft.pending = true,
        }
        Ok(())
    }

    fn rust_attribute(
        &mut self,
        id: NodeId,
        node: &Node<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let mut is_doc = false;
        for child in node.children() {
            budget.step()?;
            if child.kind() != "attribute" {
                continue;
            }
            let mut cursor = child.cursor();
            for path in child.children_with(&mut cursor) {
                budget.step()?;
                if !path.is_named() {
                    continue;
                }
                if path.kind() == "identifier"
                    && self.index.source.covers(path.start_byte()..path.end_byte())
                {
                    let bytes = &self.index.source.bytes()[path.start_byte()..path.end_byte()];
                    is_doc = matches!(bytes, b"doc" | b"cfg_attr");
                }
                break;
            }
        }
        if !is_doc {
            return Ok(());
        }
        let mut target = if node.kind() == "attribute_item" {
            self.index.canonical.nodes[id.0].following
        } else {
            let parent = self.index.canonical.nodes[id.0].parent.unwrap_or(NodeId(0));
            if parent.0 == 0 {
                Some(parent)
            } else {
                self.index.canonical.nodes[parent.0]
                    .parent
                    .filter(|&outer| {
                        matches!(
                            self.index.canonical.nodes[outer.0].evidence.node().kind(),
                            "mod_item" | "function_item"
                        )
                    })
            }
        };
        if let Some(candidate) = target {
            let target_node = self.index.canonical.nodes[candidate.0].evidence.node();
            if node.kind() == "attribute_item"
                && !self
                    .index
                    .source
                    .covers(node.end_byte()..target_node.start_byte())
            {
                target = None;
            }
        }
        let assessment = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: AssessmentAxis::Occurrence,
            reason: "Structured Rust doc/cfg_attr carrier awaits interpretation",
            node: Some(self.index.canonical.nodes[id.0].evidence),
            document: None,
        });
        let pending_scope = target
            .or(self.index.canonical.nodes[id.0].parent)
            .unwrap_or(NodeId(0));
        self.index.canonical.nodes[pending_scope.0]
            .pending
            .push(assessment);
        let publication = self.index.assessments.len();
        self.index.assessments.push(Assessment {
            axis: AssessmentAxis::Publication,
            reason: "Rust documentation/publication metadata awaits interpretation",
            node: Some(self.index.canonical.nodes[id.0].evidence),
            document: None,
        });
        self.index.canonical.nodes[pending_scope.0]
            .pending
            .push(publication);
        let source = self
            .index
            .source
            .pieces_with_budget(node.start_byte()..node.end_byte(), budget)?;
        let fragment = self.fragment(
            source,
            self.index.canonical.nodes[id.0].evidence,
            FragmentForm::Attribute,
        );
        self.pending_attributes.push((id, fragment, target));
        Ok(())
    }

    fn finalize_projection(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        budget.step()?;
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
            if let Projection::Ready(ids) = &self.index.canonical.nodes[child.0].projection {
                for &owner in ids {
                    budget.step()?;
                    owners.push(owner);
                }
                break;
            }
        }
        if !owners.is_empty() {
            self.index.canonical.nodes[id.0].projection = Projection::Ready(owners);
            self.index.canonical.nodes[id.0].pending.clear();
        }
        Ok(())
    }

    fn c_slots(&mut self, id: NodeId, budget: &CollectionBudget) -> Result<(), CollectionError> {
        let enclosing = OwnerId(self.index.facts.owners.len());
        self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
        let mut slots = Vec::new();
        let mut work = Vec::new();
        for &child in
            &self.index.canonical.children[self.index.canonical.nodes[id.0].children.clone()]
        {
            budget.step()?;
            if self.index.canonical.nodes[child.0].field == Some("declarator") {
                work.push(child);
            }
        }
        let mut ordinal = 0;
        for mut child in work {
            let mut function = false;
            loop {
                budget.step()?;
                let record = &self.index.canonical.nodes[child.0];
                let node = record.evidence.node();
                if matches!(node.kind(), "identifier" | "field_identifier") {
                    if function {
                        slots.push(enclosing);
                        let declaration = self.index.canonical.nodes[id.0].evidence.node();
                        let mut end = declaration.end_byte();
                        if self.index.source.bytes().get(end.saturating_sub(1)) == Some(&b';') {
                            end -= 1;
                        }
                        self.names.insert(
                            &self.index.source.bytes()[node.byte_range()],
                            &self.index.source.bytes()[declaration.start_byte()..end],
                            enclosing,
                            budget,
                        )?;
                    } else {
                        let source = self
                            .index
                            .source
                            .pieces_with_budget(node.byte_range(), budget)?;
                        let slot = OwnerId(self.index.facts.owners.len());
                        self.index.facts.owners.push(Owner::Slot(Slot {
                            enclosing,
                            role: SlotRole::Field,
                            ordinal,
                            source,
                        }));
                        self.index.associations_by_owner.push(Vec::new());
                        self.index.assessments_by_owner.push(Vec::new());
                        self.index.owner_nodes.push(id);
                        slots.push(slot);
                        ordinal += 1;
                    }
                    break;
                }
                function |= node.kind() == "function_declarator";
                let mut inner = None;
                for &candidate in &self.index.canonical.children[record.children.clone()] {
                    budget.step()?;
                    if self.index.canonical.nodes[candidate.0].field == Some("declarator") {
                        inner = Some(candidate);
                        break;
                    }
                }
                let Some(next) = inner else {
                    break;
                };
                child = next;
            }
        }
        self.index.canonical.nodes[id.0].projection = if slots.is_empty() {
            Projection::Pending
        } else {
            self.index.canonical.nodes[id.0].pending.clear();
            Projection::Ready(slots)
        };
        Ok(())
    }

    fn finish(&mut self, budget: &CollectionBudget) -> Result<(), CollectionError> {
        self.combine_pending(budget)?;
        for draft in std::mem::take(&mut self.drafts) {
            budget.step()?;
            self.document(draft, budget)?;
        }
        self.pending_scopes(budget)?;
        self.order_results(budget)
    }

    fn pending_scopes(&mut self, budget: &CollectionBudget) -> Result<(), CollectionError> {
        for id in 0..self.index.canonical.nodes.len() {
            budget.step()?;
            if matches!(
                self.index.canonical.nodes[id].projection,
                Projection::Pending
            ) {
                let assessment = self.index.assessments.len();
                self.index.assessments.push(Assessment {
                    axis: AssessmentAxis::Owner,
                    reason: "Recognized declaration requires a semantic projection",
                    node: Some(self.index.canonical.nodes[id].evidence),
                    document: None,
                });
                self.index.canonical.nodes[id].pending.push(assessment);
            }
            if let Some(parent) = self.index.canonical.nodes[id].parent {
                let parent_node = &self.index.canonical.nodes[parent.0];
                let inherited = if parent_node.pending.is_empty() {
                    parent_node.pending_parent
                } else {
                    Some(parent)
                };
                self.index.canonical.nodes[id].pending_parent = inherited;
            }
        }
        Ok(())
    }

    fn order_results(&mut self, budget: &CollectionBudget) -> Result<(), CollectionError> {
        let mut keys = Vec::new();
        for (ordinal, document) in self.index.facts.documents.iter().enumerate() {
            budget.step()?;
            let fragment = &self.index.facts.fragments[document.fragments[0].0];
            let first = fragment
                .source
                .pieces()
                .first()
                .ok_or(SourceError::Excluded)?
                .range();
            let last = fragment
                .source
                .pieces()
                .last()
                .ok_or(SourceError::Excluded)?
                .range();
            keys.push((first.start, last.end, document.role.source_rank(), ordinal));
        }
        for key in super::ordering::ordered(keys, budget)? {
            budget.step()?;
            self.index.documents_ordered.push(key.3);
        }
        let mut ranks = Vec::new();
        for _ in 0..self.index.facts.documents.len() {
            budget.step()?;
            ranks.push(0);
        }
        for (rank, &id) in self.index.documents_ordered.iter().enumerate() {
            budget.step()?;
            ranks[id] = rank;
        }
        let mut associations = Vec::new();
        for (ordinal, association) in self.index.facts.associations.iter().enumerate() {
            budget.step()?;
            let (anchor, slot) = association.owners.first().map_or((0, 0), |id| {
                let node = &self.index.canonical.nodes[self.index.owner_nodes[id.0].0];
                match &self.index.facts.owners[id.0] {
                    Owner::Slot(slot) => (slot.source.pieces()[0].range().start, slot.ordinal),
                    _ => (node.evidence.node().start_byte(), node.ordinal),
                }
            });
            associations.push((ranks[association.document.0], anchor, slot, ordinal));
        }
        for (_, _, _, ordinal) in super::ordering::ordered(associations, budget)? {
            budget.step()?;
            self.index.associations_ordered.push(ordinal);
        }
        for owners in &mut self.index.associations_by_owner {
            budget.step()?;
            owners.clear();
        }
        for &ordinal in &self.index.associations_ordered {
            budget.step()?;
            for &owner in &self.index.facts.associations[ordinal].owners {
                budget.step()?;
                self.index.associations_by_owner[owner.0].push(ordinal);
            }
        }
        Ok(())
    }

    fn combine_pending(&mut self, budget: &CollectionBudget) -> Result<(), CollectionError> {
        let mut by_target = std::collections::HashMap::new();
        for (index, draft) in self.drafts.iter().enumerate() {
            budget.step()?;
            if let Some(target) = draft.target {
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

    fn document(
        &mut self,
        mut draft: Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let (content, attribute) = self.render_draft(&mut draft, budget)?;
        let composition = if attribute {
            Composition::AttributeSequence
        } else if draft.fragments.len() == 1 {
            Composition::Single
        } else {
            Composition::CommentRun
        };
        let document = DocumentId(self.index.facts.documents.len());
        self.index.facts.documents.push(Document {
            fragments: draft.fragments,
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
                if let Some((b"fn", name)) = crate::lang_helpers::doxygen::structural_command(raw) {
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
            for owner in &owners {
                budget.step()?;
                self.index.assessments_by_owner[owner.0].push(assessment);
            }
            if owners.is_empty() {
                self.index.canonical.nodes[draft.target.unwrap_or(draft.scope).0]
                    .pending
                    .push(assessment);
            }
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

    fn render_draft(
        &self,
        draft: &mut Draft<'s>,
        budget: &CollectionBudget,
    ) -> Result<(Content<'s>, bool), CollectionError> {
        let mut keys = Vec::new();
        for id in &draft.fragments {
            budget.step()?;
            let start = self.index.facts.fragments[id.0].source.pieces()[0]
                .range()
                .start;
            keys.push((start, id.0));
        }
        draft.fragments.clear();
        for (_, id) in super::ordering::ordered(keys, budget)? {
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
            match super::text::comments::render(
                self.index.profile,
                &self.index.source,
                &fragments,
                budget,
            ) {
                Ok(content) => content,
                Err(super::text::TextError::Source(error)) => return Err(error.into()),
                Err(super::text::TextError::Unavailable(reason)) => Content::Unavailable(reason),
            }
        };
        Ok((content, attribute))
    }
}

fn is_wrapper(profile: Profile, kind: &str) -> bool {
    match profile {
        Profile::Jsdoc | Profile::BcaTypescriptSource => {
            matches!(kind, "export_statement" | "ambient_declaration")
        }
        Profile::Yard => matches!(
            kind,
            "if_modifier" | "unless_modifier" | "body_statement" | "block_body" | "then"
        ),
        Profile::Ldoc => kind == "block",
        _ => false,
    }
}

fn needs_slot(profile: Profile, kind: &str) -> bool {
    match profile {
        Profile::Doxygen => matches!(kind, "declaration" | "field_declaration"),
        Profile::GoDoc => matches!(kind, "field_declaration" | "method_elem" | "type_elem"),
        Profile::Jsdoc | Profile::BcaTypescriptSource => matches!(
            kind,
            "lexical_declaration"
                | "variable_declaration"
                | "expression_statement"
                | "required_parameter"
                | "optional_parameter"
        ),
        _ => false,
    }
}

fn fields_needed(profile: Profile, kind: &str) -> bool {
    (profile == Profile::Doxygen
        && (matches!(kind, "declaration" | "field_declaration") || kind.ends_with("declarator")))
        || (matches!(profile, Profile::Jsdoc | Profile::BcaTypescriptSource)
            && kind == "export_statement")
        || (profile == Profile::Yard && matches!(kind, "if_modifier" | "unless_modifier"))
}

crate::observation::counter!(field_requests, cfg(any(test, feature = "test-support")));

#[cfg(test)]
mod tests;
