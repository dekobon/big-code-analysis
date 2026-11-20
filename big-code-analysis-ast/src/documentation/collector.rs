//! Coordinated source-order traversal and immutable-index construction.

use super::budget::{CollectionBudget, WorkBudget};
use super::canonical::{CanonicalNodes, NodeId, Projection};
use super::recognition::{NativeRules, SourceRules};
use super::streaming::{markers, positional};
use super::*;
use crate::{Ancestors, Checker, FoldCache, Node, SiblingCache};

mod assembly;
mod projections;

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
            source_cleaned: Vec::new(),
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
    fn owner_candidate<'s>(
        node: &Node<'s>,
        code: &[u8],
        ancestors: Ancestors<'s, '_>,
        budget: &CollectionBudget,
    ) -> Result<bool, SourceError>;
}

impl<T: Checker + crate::traits::LanguageInfo> OwnerCandidates for T {
    fn owner_candidate<'s>(
        node: &Node<'s>,
        code: &[u8],
        ancestors: Ancestors<'s, '_>,
        budget: &CollectionBudget,
    ) -> Result<bool, SourceError> {
        match T::lang() {
            crate::LANG::Typescript => {
                crate::roles::typescript::doc_target_with_budget(node, code, ancestors, budget)
            }
            crate::LANG::Tsx => {
                crate::roles::tsx::doc_target_with_budget(node, code, ancestors, budget)
            }
            _ => NativeRules::<T>::target(node, code, ancestors, budget),
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
                    if transparent_doc_context(node.kind())
                        && let Some(parent) = self.index.canonical.nodes[id.0].parent
                    {
                        self.index.canonical.nodes[id.0].owner_context =
                            self.index.canonical.nodes[parent.0].owner_context;
                    }
                    let ancestors = Ancestors::for_walk(&chain, &node, &folds, &siblings);
                    super::budget::in_phase(
                        budget,
                        super::budget::WorkPhase::Classification,
                        || {
                            self.register::<T>(id, &node, ancestors, budget)?;
                            self.recognize::<T>(id, &node, ancestors, budget)
                        },
                    )?;
                    self.record_children::<T>(id, &mut cursor, &mut children, &mut fields, budget)?;
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

    fn record_children<T: Checker>(
        &mut self,
        id: NodeId,
        cursor: &mut tree_sitter::TreeCursor<'s>,
        children: &mut Vec<Node<'s>>,
        fields: &mut Vec<Option<&'static str>>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let node = self.index.canonical.nodes[id.0].evidence.node();
        children.clear();
        fields.clear();
        cursor.reset(node);
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
            .batch(id, children, &self.index.source, budget)?;
        let mut following = None;
        for (&child, field) in self.index.canonical.children
            [self.index.canonical.nodes[id.0].children.clone()]
        .iter()
        .zip(fields)
        .rev()
        {
            budget.step()?;
            let record = &mut self.index.canonical.nodes[child.0];
            record.field = *field;
            record.following = following;
            let child_node = Node::from_tree_sitter(record.evidence.node());
            if !T::is_comment(&child_node) && child_node.kind() != "attribute_item" {
                following = Some(child);
            }
        }
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
        if !self
            .index
            .source
            .covers_with_budget(node.start_byte()..node.end_byte(), budget)?
        {
            if node.is_named() && !T::is_comment(node) {
                self.index.canonical.nodes[id.0].projection = Projection::Pending;
            }
            return Ok(());
        }
        let wrapper_child = matches!(
            self.index.profile,
            Profile::Jsdoc | Profile::BcaTypescriptSource
        ) && ancestors
            .parent(node)
            .is_some_and(|parent| is_wrapper(self.index.profile, parent.kind()));
        let qualifies = if wrapper_child {
            T::owner_candidate(node, self.index.source.bytes(), ancestors, budget)?
        } else {
            NativeRules::<T>::target(node, self.index.source.bytes(), ancestors, budget)?
        };
        if qualifies {
            if is_wrapper(self.index.profile, node.kind())
                || needs_slot(self.index.profile, node.kind())
                || super::forms::owner_pending(self.index.profile, node, ancestors)
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

    fn recognize<T: Checker + crate::traits::LanguageInfo>(
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
        if let super::forms::CommentAssessment::Pending(reason) = super::forms::assess(
            self.index.profile,
            node,
            ancestors,
            &self.index.source,
            budget,
        )? {
            let assessment = self.index.assessments.len();
            self.index.assessments.push(Assessment {
                axis: AssessmentAxis::Selection,
                reason,
                node: Some(self.index.canonical.nodes[id.0].evidence),
                document: None,
            });
            let parent = self.index.canonical.nodes[id.0].parent.unwrap_or(NodeId(0));
            let scope = self.index.canonical.nodes[parent.0].owner_context;
            self.index.canonical.nodes[scope.0].pending.push(assessment);
        }
        let mut marker_runs = Vec::new();
        markers::source_comment_with_budget::<NativeRules<T>, _>(
            node,
            ancestors,
            &mut markers::SourceRequest {
                context: &self.index.source,
                retain_unbound: self.index.options.retain_unbound,
                sink: &mut |run| marker_runs.push(run),
            },
            budget,
        )?;
        for run in marker_runs {
            self.marker_run(run, budget)?;
        }
        let mut positional_runs = Vec::new();
        positional::source_comment_with_budget::<NativeRules<T>, _>(
            node,
            ancestors,
            &mut positional::SourceRequest {
                context: &self.index.source,
                retain_unbound: self.index.options.retain_unbound,
                retain_directives: self.index.options.retain_directives,
                sink: &mut |run| positional_runs.push(run),
            },
            budget,
        )?;
        for run in positional_runs {
            self.positional_run(run, budget)?;
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
                if self.index.profile == Profile::BcaPhpSource
                    && draft.role == DocumentRole::FilePackage
                    && let Some(target) = target
                {
                    let kind = self.index.canonical.nodes[target.0].evidence.node().kind();
                    if matches!(kind, "enum_declaration" | "expression_statement") {
                        draft.target = Some(target);
                        draft.role = DocumentRole::Primary;
                        draft.pending = kind == "expression_statement";
                    }
                }
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
            if let Some((command, _)) =
                crate::lang_helpers::doxygen::structural_command_with_budget(raw, budget)?
            {
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
                    crate::lang_helpers::doc_text::tsdoc_module_with_budget(raw, budget)?
                } else {
                    crate::lang_helpers::doc_text::jsdoc_file_with_budget(raw, budget)?
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
                    && self
                        .index
                        .source
                        .covers_with_budget(path.start_byte()..path.end_byte(), budget)?
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
                    .covers_with_budget(node.end_byte()..target_node.start_byte(), budget)?
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
        && (matches!(
            kind,
            "declaration" | "field_declaration" | "function_definition"
        ) || kind.ends_with("declarator")))
        || (matches!(profile, Profile::Jsdoc | Profile::BcaTypescriptSource)
            && kind == "export_statement")
        || (profile == Profile::Yard && matches!(kind, "if_modifier" | "unless_modifier"))
}

fn transparent_doc_context(kind: &str) -> bool {
    matches!(
        kind,
        "parameters"
            | "formal_parameters"
            | "function_value_parameters"
            | "class_parameters"
            | "class_parameter"
            | "parameter"
            | "required_parameter"
            | "optional_parameter"
            | "simple_parameter"
            | "property_promotion_parameter"
            | "modifiers"
    )
}

crate::observation::counter!(field_requests, cfg(any(test, feature = "test-support")));

#[cfg(test)]
mod assessment_tests;
#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod cost_tests;
#[cfg(test)]
mod projection_tests;
#[cfg(test)]
mod tests;
