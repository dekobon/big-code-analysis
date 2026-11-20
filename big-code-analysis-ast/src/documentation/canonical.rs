//! Canonical identities and flat child batches; no parent recovery on lookup.

use std::collections::HashMap;
use std::ops::Range;

use super::budget::WorkBudget;
use super::{NodeEvidence, OwnerId, SourceContext, SourceError};
use crate::Node;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct NodeId(pub(super) usize);

#[derive(Debug)]
pub(super) enum Projection {
    Unsupported,
    Pending,
    Ready(Vec<OwnerId>),
}

#[derive(Debug)]
pub(super) struct CanonicalNode<'s> {
    pub(super) evidence: NodeEvidence<'s>,
    pub(super) parent: Option<NodeId>,
    pub(super) ordinal: usize,
    pub(super) field: Option<&'static str>,
    pub(super) children: Range<usize>,
    pub(super) projection: Projection,
    pub(super) pending: Vec<usize>,
    pub(super) pending_parent: Option<NodeId>,
    pub(super) following: Option<NodeId>,
}

#[derive(Debug)]
pub(super) struct CanonicalNodes<'s> {
    root: NodeEvidence<'s>,
    by_id: HashMap<usize, Vec<NodeId>>,
    pub(super) nodes: Vec<CanonicalNode<'s>>,
    pub(super) children: Vec<NodeId>,
}

// Counts candidate validations, including hash collisions and zero-width nodes.
crate::observation::counter!(identity_probes, cfg(any(test, feature = "test-support")));

impl<'s> CanonicalNodes<'s> {
    pub(super) fn new(root: NodeEvidence<'s>) -> Self {
        Self {
            root,
            by_id: HashMap::new(),
            nodes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub(super) fn insert(
        &mut self,
        evidence: NodeEvidence<'s>,
        parent: Option<NodeId>,
        ordinal: usize,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.by_id.entry(evidence.node().id()).or_default().push(id);
        self.nodes.push(CanonicalNode {
            evidence,
            parent,
            ordinal,
            field: None,
            children: 0..0,
            projection: Projection::Unsupported,
            pending: Vec::new(),
            pending_parent: None,
            following: None,
        });
        id
    }

    pub(super) fn lookup(&self, node: tree_sitter::Node<'_>) -> Result<NodeId, SourceError> {
        if node.into_raw().tree != self.root.node().into_raw().tree {
            return Err(SourceError::ForeignTree);
        }
        if let Some(candidates) = self.by_id.get(&node.id()) {
            for &id in candidates {
                identity_probes::record();
                if self.nodes[id.0].evidence.validate(node).is_ok() {
                    return Ok(id);
                }
            }
        }
        Err(SourceError::NoncanonicalNode)
    }

    pub(super) fn batch<B: WorkBudget>(
        &mut self,
        id: NodeId,
        children: &[Node<'s>],
        context: &SourceContext<'s>,
        budget: &B,
    ) -> Result<(), SourceError> {
        let start = self.children.len();
        for (ordinal, child) in children.iter().enumerate() {
            budget.step().map_err(Into::into)?;
            let child_id = self.insert(context.node(child.as_tree_sitter())?, Some(id), ordinal);
            self.children.push(child_id);
        }
        self.nodes[id.0].children = start..self.children.len();
        Ok(())
    }
}
