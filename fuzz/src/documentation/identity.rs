use std::collections::HashMap;

use big_code_analysis_ast::documentation::{Document, FactRef, NodeEvidence, Owner};

use super::{Check, require};

pub(super) fn address<T>(fact: FactRef<'_, T>) -> *const T {
    std::ptr::from_ref(fact.get())
}

pub(super) fn evidence(left: Option<NodeEvidence<'_>>, right: Option<NodeEvidence<'_>>) -> Check {
    match (left, right) {
        (Some(left), Some(right)) => left
            .validate(right.node())
            .map_err(|_| "nondeterministic node evidence"),
        (None, None) => Ok(()),
        _ => Err("nondeterministic optional evidence"),
    }
}

#[derive(Default)]
pub(super) struct References<'s> {
    candidates: HashMap<(*const Document<'s>, usize), Vec<NodeEvidence<'s>>>,
}

impl<'s> References<'s> {
    pub fn insert(&mut self, document: FactRef<'_, Document<'s>>, node: NodeEvidence<'s>) {
        self.candidates
            .entry((address(document), node.node().id()))
            .or_default()
            .push(node);
    }

    pub fn validate(&self, document: FactRef<'_, Document<'s>>, node: NodeEvidence<'_>) -> Check {
        require(
            self.candidates
                .get(&(address(document), node.node().id()))
                .is_some_and(|candidates| {
                    candidates
                        .iter()
                        .any(|candidate| candidate.validate(node.node()).is_ok())
                }),
            "qualification reference evidence is not a document fragment",
        )
    }
}

pub(super) fn candidate(owner: &Owner<'_>, node: NodeEvidence<'_>) -> Check {
    match owner {
        Owner::Node(owner) | Owner::File(owner) => owner
            .validate(node.node())
            .map_err(|_| "qualification candidate evidence differs from owner"),
        Owner::Slot(_) | Owner::Group(_) | Owner::Virtual(_) => Ok(()),
        _ => Err("unhandled candidate owner variant"),
    }
}
