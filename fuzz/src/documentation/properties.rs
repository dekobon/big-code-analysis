use big_code_analysis_ast::documentation::*;
use std::collections::HashMap;

use super::{Check, MAX_QUERIES, maps, require};

fn address<T>(fact: FactRef<'_, T>) -> *const T {
    std::ptr::from_ref(fact.get())
}

fn selected(length: usize, count: usize, selector: u8) -> Vec<usize> {
    if length == 0 {
        return Vec::new();
    }
    let mut selected = vec![length - 1];
    for n in 0..count - 1 {
        let at = (usize::from(selector) + n * length / count) % length;
        if !selected.contains(&at) {
            selected.push(at);
        }
    }
    selected
}

#[allow(clippy::too_many_lines)] // One bounded pass validates each retained fact family.
pub(super) fn check(
    index: &DocumentationIndex<'_>,
    tree: &tree_sitter::Tree,
    source: &[u8],
) -> Check {
    let coverage = maps::Coverage::new(tree, source);
    for fragment in index.facts().fragments() {
        coverage.pieces(fragment.get().source().pieces())?;
        if let Some(node) = fragment.get().node() {
            require(
                node.node().into_raw().tree == tree.root_node().into_raw().tree,
                "fragment evidence is foreign",
            )?;
            require(
                node.node().end_byte() <= source.len(),
                "fragment evidence bounds",
            )?;
        }
    }
    let mut last = None;
    let mut mapped = None;
    for document in index.documents() {
        let key = index
            .facts()
            .document_order(document)
            .map_err(|_| "document order key")?;
        require(
            last.is_none_or(|last| last <= key),
            "documents out of source order",
        )?;
        last = Some(key);
        require(
            document.get().fragment_count() > 0,
            "document has no fragments",
        )?;
        let mut fragment_end = 0;
        for at in 0..document.get().fragment_count() {
            let fragment = index
                .facts()
                .document_fragment(document, at)
                .map_err(|_| "document fragment navigation")?;
            let pieces = fragment.get().source().pieces();
            require(
                pieces
                    .first()
                    .is_some_and(|piece| piece.range().start >= fragment_end),
                "document fragment order",
            )?;
            fragment_end = pieces.last().ok_or("empty fragment")?.range().end;
        }
        require(
            index
                .facts()
                .document_fragment(document, document.get().fragment_count())
                .err()
                == Some(FactError::MissingRecord),
            "missing fragment accepted",
        )?;
        let complete = index
            .complete_text(document)
            .map_err(|_| "same-index text handle rejected")?;
        match (document.get().content(), complete) {
            (Content::Complete { text, mappings, .. }, Some(complete)) => {
                require(
                    complete.text() == text && complete.mappings() == mappings,
                    "retained text differs",
                )?;
                maps::validate(complete, &coverage)?;
                mapped = Some(complete);
            }
            (Content::Complete { .. }, None) => return Err("complete text unavailable"),
            (_, Some(_)) => return Err("noncomplete state flattened to text"),
            (_, None) => {}
        }
        if let Some(cleaned) = index
            .source_cleaned_text(document)
            .map_err(|_| "cleaned text handle")?
        {
            maps::validate(cleaned, &coverage)?;
        }
    }
    if let Some(complete) = mapped {
        maps::queries(complete, source.last().copied().unwrap_or(0))?;
    }
    for owner in index.facts().owners() {
        match owner.get() {
            Owner::Slot(slot) => {
                coverage.pieces(slot.source.pieces())?;
                index
                    .facts()
                    .slot_enclosing(owner)
                    .map_err(|_| "slot enclosure")?;
            }
            Owner::Group(group) => {
                for piece in &group.pieces {
                    coverage.pieces(piece.source.pieces())?;
                }
            }
            Owner::Virtual(owner) => {
                coverage.pieces(owner.name.pieces())?;
                coverage.pieces(owner.declaration.pieces())?;
            }
            Owner::Node(_) | Owner::File(_) => {}
            _ => return Err("unhandled owner variant"),
        }
    }
    associations(index)?;
    qualifications(index)?;
    for assessment in index.assessments() {
        require(
            !assessment.get().reason.is_empty(),
            "assessment reason absent",
        )?;
        index
            .assessed_document(assessment)
            .map_err(|_| "assessment document navigation")?;
    }
    for evidence in index.evidence() {
        index
            .evidence_fragment(evidence)
            .map_err(|_| "candidate evidence navigation")?;
    }
    owner_queries(index, source.first().copied().unwrap_or(0))?;
    node_queries(index, tree, source.last().copied().unwrap_or(0))
}

fn owner_anchor(owner: &Owner<'_>) -> (usize, usize) {
    match owner {
        Owner::Node(node) | Owner::File(node) => (node.node().start_byte(), 0),
        Owner::Slot(slot) => (slot.source.pieces()[0].range().start, slot.ordinal),
        Owner::Group(group) => (group.scope.node().start_byte(), 0),
        Owner::Virtual(owner) => (owner.declaration.pieces()[0].range().start, 0),
        _ => (0, 0),
    }
}

fn associations(index: &DocumentationIndex<'_>) -> Check {
    let mut last = None;
    for association in index.associations() {
        let document = index
            .facts()
            .associated_document(association)
            .map_err(|_| "association document navigation")?;
        let key = index
            .facts()
            .document_order(document)
            .map_err(|_| "association document order")?;
        let anchor = if association.get().owner_count() == 0 {
            (0, 0)
        } else {
            owner_anchor(
                index
                    .facts()
                    .associated_owner(association, 0)
                    .map_err(|_| "association owner navigation")?
                    .get(),
            )
        };
        require(
            last.is_none_or(|last| last <= (key, anchor)),
            "associations out of source order",
        )?;
        last = Some((key, anchor));
        for at in 0..association.get().owner_count() {
            index
                .facts()
                .associated_owner(association, at)
                .map_err(|_| "association owner navigation")?;
        }
        require(
            index
                .facts()
                .associated_owner(association, association.get().owner_count())
                .err()
                == Some(FactError::MissingRecord),
            "missing owner accepted",
        )?;
        require(
            association.get().convention.name() == index.profile().name(),
            "association convention",
        )?;
    }
    Ok(())
}

fn qualifications(index: &DocumentationIndex<'_>) -> Check {
    let mut by_document: HashMap<_, Vec<_>> = HashMap::new();
    let mut last = None;
    for qualification in index.qualifications() {
        let fact = qualification.get();
        let document = index
            .qualified_document(qualification)
            .map_err(|_| "qualified document")?;
        let owner = index
            .qualified_owner(qualification)
            .map_err(|_| "qualified owner")?;
        require(
            owner.is_some() == (fact.subject == QualificationSubject::Candidate),
            "qualification subject identity",
        )?;
        let rank = match fact.reason {
            UnresolvedReason::Conditional => 0,
            UnresolvedReason::GrammarBoundary => 1,
            _ => return Err("unknown qualification reason"),
        };
        let key = (
            index
                .facts()
                .document_order(document)
                .map_err(|_| "qualified document order")?,
            fact.node.node().start_byte(),
            fact.node.node().end_byte(),
            rank,
        );
        require(
            last.is_none_or(|last| last <= key),
            "qualifications out of source order",
        )?;
        last = Some(key);
        by_document
            .entry(address(document))
            .or_default()
            .push(qualification);
    }
    for document in index.documents() {
        let expected = by_document.remove(&address(document)).unwrap_or_default();
        require(
            index
                .document_qualifications(document)
                .map_err(|_| "document qualification lookup")?
                .eq(expected),
            "document qualification adjacency",
        )?;
    }
    require(
        by_document.is_empty(),
        "qualification refers to unenumerated document",
    )
}

fn owner_queries(index: &DocumentationIndex<'_>, selector: u8) -> Check {
    let owners: Vec<_> = index.facts().owners().collect();
    for at in selected(owners.len(), MAX_QUERIES / 2, selector) {
        let owner = owners[at];
        let query = index
            .owner_associations(owner)
            .map_err(|_| "same-index owner rejected")?;
        let mut expected = Vec::new();
        for association in index.associations() {
            for position in 0..association.get().owner_count() {
                if index
                    .facts()
                    .associated_owner(association, position)
                    .map_err(|_| "owner membership")?
                    == owner
                {
                    expected.push(association);
                }
            }
        }
        require(
            query.associations().eq(expected),
            "owner association adjacency",
        )?;
        let mut expected = Vec::new();
        for qualification in index.qualifications() {
            if index
                .qualified_owner(qualification)
                .map_err(|_| "qualification owner")?
                == Some(owner)
            {
                expected.push(qualification);
            }
        }
        require(
            query.qualifications().eq(expected),
            "owner qualification adjacency",
        )?;
        require(
            !query.projection_pending(),
            "concrete owner projection pending",
        )?;
        for assessment in query.assessments() {
            index
                .assessed_document(assessment)
                .map_err(|_| "owner assessment foreign")?;
        }
    }
    Ok(())
}

fn node_queries(index: &DocumentationIndex<'_>, tree: &tree_sitter::Tree, selector: u8) -> Check {
    let mut nodes = Vec::new();
    let mut cursor = tree.walk();
    loop {
        nodes.push(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return selected_nodes(index, &nodes, selector);
            }
        }
    }
}

fn selected_nodes(
    index: &DocumentationIndex<'_>,
    nodes: &[tree_sitter::Node<'_>],
    selector: u8,
) -> Check {
    for at in selected(nodes.len(), MAX_QUERIES / 2, selector) {
        let node = nodes[at];
        match index.node_projection(node) {
            Err(OwnerError::UnsupportedOwner) => {
                require(
                    matches!(
                        index.node_associations(node),
                        Err(OwnerError::UnsupportedOwner)
                    ),
                    "unsupported projection query",
                )?;
                require(
                    matches!(index.node_owners(node), Err(OwnerError::UnsupportedOwner)),
                    "unsupported owners query",
                )?;
            }
            Ok(projection) if projection.pending() => {
                require(
                    projection.owners().len() == 0 && projection.assessments().next().is_some(),
                    "pending projection lost assessment",
                )?;
                require(
                    matches!(index.node_owners(node), Err(OwnerError::PendingOwner)),
                    "pending owners query",
                )?;
                let query = index
                    .node_associations(node)
                    .map_err(|_| "pending query must remain inspectable")?;
                require(
                    query.projection_pending() && query.assessments().next().is_some(),
                    "pending query flattened to empty",
                )?;
            }
            Ok(projection) => {
                let owners: Vec<_> = projection.owners().collect();
                require(
                    index
                        .node_owners(node)
                        .map_err(|_| "known projection owners")?
                        .eq(owners.iter().copied()),
                    "projection owner enumeration",
                )?;
                match owners.len() {
                    0 => require(
                        matches!(
                            index.node_associations(node),
                            Err(OwnerError::UnsupportedOwner)
                        ),
                        "empty projection query",
                    )?,
                    1 => {
                        let query = index
                            .node_associations(node)
                            .map_err(|_| "known node query")?;
                        let expected = index
                            .owner_associations(owners[0])
                            .map_err(|_| "known owner query")?;
                        require(
                            query.associations().eq(expected.associations()),
                            "node-owner association agreement",
                        )?;
                        require(
                            query.assessments().eq(expected.assessments()),
                            "node-owner pending agreement",
                        )?;
                        require(
                            query.qualifications().eq(expected.qualifications()),
                            "node-owner qualification agreement",
                        )?;
                    }
                    _ => require(
                        matches!(
                            index.node_associations(node),
                            Err(OwnerError::MultipleOwners)
                        ),
                        "multi-owner projection query",
                    )?,
                }
            }
            _ => return Err("canonical cursor node rejected"),
        }
    }
    Ok(())
}

pub(super) fn negative_nodes(index: &DocumentationIndex<'_>, tree: &tree_sitter::Tree) -> Check {
    let cloned = tree.clone();
    require(
        matches!(
            index.node_projection(cloned.root_node()),
            Err(OwnerError::Source(SourceError::ForeignTree))
        ),
        "cloned tree accepted",
    )?;
    let shifted = tree.root_node_with_offset(1, tree_sitter::Point::new(0, 1));
    require(
        matches!(
            index.node_projection(shifted),
            Err(OwnerError::Source(SourceError::NoncanonicalNode))
        ),
        "shifted node accepted",
    )?;
    let mut edited = tree.root_node();
    edited.edit(&tree_sitter::InputEdit {
        start_byte: 0,
        old_end_byte: 0,
        new_end_byte: 1,
        start_position: tree_sitter::Point::new(0, 0),
        old_end_position: tree_sitter::Point::new(0, 0),
        new_end_position: tree_sitter::Point::new(0, 1),
    });
    require(
        matches!(
            index.node_projection(edited),
            Err(OwnerError::Source(SourceError::NoncanonicalNode))
        ),
        "edited node accepted",
    )
}

#[allow(clippy::too_many_lines)] // Compare each public record family in observed order.
pub(super) fn repeat<'s>(first: &DocumentationIndex<'s>, second: &DocumentationIndex<'s>) -> Check {
    require(
        first.documents().len() == second.documents().len()
            && first.associations().len() == second.associations().len()
            && first.facts().fragments().len() == second.facts().fragments().len()
            && first.facts().owners().len() == second.facts().owners().len()
            && first.assessments().len() == second.assessments().len()
            && first.evidence().len() == second.evidence().len()
            && first.qualifications().len() == second.qualifications().len(),
        "nondeterministic record count",
    )?;
    for (left, right) in first.facts().fragments().zip(second.facts().fragments()) {
        require(
            left.get().source() == right.get().source() && left.get().form() == right.get().form(),
            "nondeterministic retained fragments",
        )?;
    }
    let first_owners: HashMap<_, _> = first
        .facts()
        .owners()
        .enumerate()
        .map(|(at, owner)| (address(owner), at))
        .collect();
    let second_owners: HashMap<_, _> = second
        .facts()
        .owners()
        .enumerate()
        .map(|(at, owner)| (address(owner), at))
        .collect();
    for (left, right) in first.facts().owners().zip(second.facts().owners()) {
        require(
            std::mem::discriminant(left.get()) == std::mem::discriminant(right.get())
                && owner_anchor(left.get()) == owner_anchor(right.get()),
            "nondeterministic owners",
        )?;
    }
    for (left, right) in first.documents().zip(second.documents()) {
        require(
            first
                .facts()
                .document_order(left)
                .map_err(|_| "first document order")?
                == second
                    .facts()
                    .document_order(right)
                    .map_err(|_| "second document order")?,
            "nondeterministic document order",
        )?;
        require(
            left.get().composition() == right.get().composition()
                && left.get().fragment_count() == right.get().fragment_count(),
            "nondeterministic composition",
        )?;
        for at in 0..left.get().fragment_count() {
            let a = first
                .facts()
                .document_fragment(left, at)
                .map_err(|_| "first fragment")?;
            let b = second
                .facts()
                .document_fragment(right, at)
                .map_err(|_| "second fragment")?;
            require(
                a.get().source() == b.get().source() && a.get().form() == b.get().form(),
                "nondeterministic fragments",
            )?;
        }
        match (left.get().content(), right.get().content()) {
            (
                Content::Complete {
                    text: a,
                    mappings: am,
                    ..
                },
                Content::Complete {
                    text: b,
                    mappings: bm,
                    ..
                },
            ) => require(a == b && am == bm, "nondeterministic complete content")?,
            (Content::Unavailable(a), Content::Unavailable(b)) => {
                require(a == b, "nondeterministic unavailable reason")?
            }
            (a, b) => require(
                std::mem::discriminant(a) == std::mem::discriminant(b),
                "nondeterministic content state",
            )?,
        }
        require(
            second.complete_text(left).err() == Some(FactError::ForeignIndex),
            "foreign document accepted",
        )?;
    }
    for (left, right) in first.associations().zip(second.associations()) {
        require(
            left.get().resolution == right.get().resolution
                && left.get().publication == right.get().publication
                && left.get().owner_count() == right.get().owner_count(),
            "nondeterministic association",
        )?;
        let a = first
            .facts()
            .associated_document(left)
            .map_err(|_| "first associated document")?;
        let b = second
            .facts()
            .associated_document(right)
            .map_err(|_| "second associated document")?;
        require(
            first
                .facts()
                .document_order(a)
                .map_err(|_| "first associated order")?
                == second
                    .facts()
                    .document_order(b)
                    .map_err(|_| "second associated order")?,
            "nondeterministic association document",
        )?;
        for position in 0..left.get().owner_count() {
            let a = first
                .facts()
                .associated_owner(left, position)
                .map_err(|_| "first associated owner")?;
            let b = second
                .facts()
                .associated_owner(right, position)
                .map_err(|_| "second associated owner")?;
            require(
                first_owners.get(&address(a)) == second_owners.get(&address(b)),
                "nondeterministic association owner identity",
            )?;
        }
        require(
            second.facts().associated_document(left).err() == Some(FactError::ForeignIndex),
            "foreign association accepted",
        )?;
    }
    for (left, right) in first.qualifications().zip(second.qualifications()) {
        let a = left.get();
        let b = right.get();
        require(
            a.subject == b.subject && a.reason == b.reason && a.node.node() == b.node.node(),
            "nondeterministic qualifications",
        )?;
        let a = first
            .qualified_owner(left)
            .map_err(|_| "first qualified owner")?
            .map(|owner| first_owners[&address(owner)]);
        let b = second
            .qualified_owner(right)
            .map_err(|_| "second qualified owner")?
            .map(|owner| second_owners[&address(owner)]);
        require(a == b, "nondeterministic qualification owner identity")?;
        let a = first
            .qualified_document(left)
            .map_err(|_| "first qualified document")?;
        let b = second
            .qualified_document(right)
            .map_err(|_| "second qualified document")?;
        require(
            first
                .facts()
                .document_order(a)
                .map_err(|_| "first qualified order")?
                == second
                    .facts()
                    .document_order(b)
                    .map_err(|_| "second qualified order")?,
            "nondeterministic qualification document identity",
        )?;
    }
    for (left, right) in first.assessments().zip(second.assessments()) {
        let a = left.get();
        let b = right.get();
        require(
            a.axis == b.axis
                && a.reason == b.reason
                && a.node.map(NodeEvidence::node) == b.node.map(NodeEvidence::node),
            "nondeterministic assessments",
        )?;
        let a = first
            .assessed_document(left)
            .map_err(|_| "first assessed document")?
            .map(|document| first.facts().document_order(document))
            .transpose()
            .map_err(|_| "first assessed order")?;
        let b = second
            .assessed_document(right)
            .map_err(|_| "second assessed document")?
            .map(|document| second.facts().document_order(document))
            .transpose()
            .map_err(|_| "second assessed order")?;
        require(a == b, "nondeterministic assessment document identity")?;
    }
    for (left, right) in first.evidence().zip(second.evidence()) {
        require(
            left.get().disposition == right.get().disposition,
            "nondeterministic evidence disposition",
        )?;
        let a = first
            .evidence_fragment(left)
            .map_err(|_| "first evidence fragment")?;
        let b = second
            .evidence_fragment(right)
            .map_err(|_| "second evidence fragment")?;
        require(
            a.get().source() == b.get().source(),
            "nondeterministic evidence source",
        )?;
    }
    if let Some(owner) = first.facts().owners().next() {
        require(
            matches!(
                second.owner_associations(owner),
                Err(OwnerError::Fact(FactError::ForeignIndex))
            ),
            "foreign owner accepted",
        )?;
    }
    if let Some(assessment) = first.assessments().next() {
        require(
            second.assessed_document(assessment).err() == Some(FactError::ForeignIndex),
            "foreign assessment accepted",
        )?;
    }
    if let Some(qualification) = first.qualifications().next() {
        require(
            second.qualified_document(qualification).err() == Some(FactError::ForeignIndex),
            "foreign qualification accepted",
        )?;
    }
    if let Some(evidence) = first.evidence().next() {
        require(
            second.evidence_fragment(evidence).err() == Some(FactError::ForeignIndex),
            "foreign candidate evidence accepted",
        )?;
    }
    Ok(())
}
