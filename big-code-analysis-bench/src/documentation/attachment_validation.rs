//! Exact source-derived attachment oracles for combined structural ladders.

use super::{HarnessError, Parsed, failure, require};
use big_code_analysis_ast::documentation::{
    Document, DocumentationIndex, FactRef, FragmentForm, Origin, Owner, Resolution, SlotRole,
    SourcePieces, UnresolvedReason,
};
use big_code_analysis_ast::tree_sitter::Node;
use std::collections::HashMap;
use std::ops::Range;

fn node_at<'s>(
    nodes: &HashMap<(usize, usize), Node<'s>>,
    range: Range<usize>,
    kind: &str,
) -> Result<Node<'s>, HarnessError> {
    nodes
        .get(&(range.start, range.end))
        .copied()
        .filter(|n| n.kind() == kind)
        .ok_or_else(|| HarnessError(format!("expected {kind} at {range:?}")))
}

fn exact_piece(
    pieces: &SourcePieces<'_>,
    range: Range<usize>,
    bytes: &[u8],
) -> Result<(), HarnessError> {
    require(
        !pieces.has_excluded_gap() && pieces.pieces().len() == 1,
        "exact source piece count",
    )?;
    let piece = &pieces.pieces()[0];
    require(
        piece.range() == range && piece.bytes() == bytes,
        "exact source piece range/bytes",
    )
}

fn comment_text<'s>(
    index: &DocumentationIndex<'s>,
    doc: FactRef<'_, Document<'s>>,
    start: usize,
) -> Result<(), HarnessError> {
    require(
        doc.get().fragment_count() == 1,
        "combined fixture fragment count",
    )?;
    let fragment = index.facts().document_fragment(doc, 0).map_err(failure)?;
    require(
        fragment.get().form() == FragmentForm::Comment,
        "combined fixture comment form",
    )?;
    exact_piece(fragment.get().source(), start..start + 10, b"/**audit*/")?;
    require(
        fragment.get().node().is_some_and(|n| {
            n.node().byte_range() == (start..start + 10) && n.node().kind() == "comment"
        }),
        "combined fixture comment node",
    )?;
    let text = index
        .complete_text(doc)
        .map_err(failure)?
        .ok_or_else(|| HarnessError("combined fixture complete text absent".into()))?;
    require(
        text.text() == "audit" && text.mappings().len() == 1,
        "combined fixture audit text/map count",
    )?;
    let mapping = &text.mappings()[0];
    require(
        mapping.output.range() == (0..5),
        "combined fixture map output",
    )?;
    let Origin::Copied(pieces) = &mapping.origin else {
        return Err(HarnessError(
            "combined fixture origin must be copied".into(),
        ));
    };
    exact_piece(pieces, start + 3..start + 8, b"audit")
}

#[allow(clippy::too_many_lines)] // Shared fixture oracle keeps source ranges and the corresponding owner identities together.
pub(super) fn validate(
    key: &str,
    size: usize,
    parsed: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    let recovery = key == "recovery-depth";
    let count = if recovery { 32 } else { 1 };
    require(
        index.documents().len() == count && index.associations().len() == count,
        "combined fixture D/A count",
    )?;
    require(
        index.qualifications().len() == 0,
        "direct attachment must have Q=0",
    )?;
    let resolution = if recovery {
        Resolution::Resolved
    } else {
        Resolution::Unresolved(UnresolvedReason::Conditional)
    };
    require(
        index
            .associations()
            .all(|a| a.get().owner_count() == 1 && a.get().resolution == resolution),
        "combined fixture resolution/owner count",
    )?;
    // A single canonical traversal supplies identity without per-output parent walks.
    let nodes: HashMap<_, _> = parsed
        .nodes()
        .into_iter()
        .filter(|(n, _)| {
            matches!(
                n.kind(),
                "field_declaration" | "field_declaration_list" | "preproc_if"
            )
        })
        .map(|(n, _)| ((n.start_byte(), n.end_byte()), n))
        .collect();
    let base = if recovery { 11 * size + 2 } else { 20 * size };
    let scope_range = if recovery {
        // The final opener's brace is two bytes before the 11n prefix end;
        // 32 leaf units and the first closing brace end this field list.
        11 * size - 2..base + 32 * 18 + 1
    } else {
        // #if FLAG\n is nine bytes; the leaf unit is 21 and #endif is six.
        base - 9..base + 21 + 6
    };
    let scope = node_at(
        &nodes,
        scope_range,
        if recovery {
            "field_declaration_list"
        } else {
            "preproc_if"
        },
    )?;
    for (ordinal, (doc, association)) in index.documents().zip(index.associations()).enumerate() {
        let start = base + ordinal * 18;
        let declaration = node_at(
            &nodes,
            start + 11..start + if recovery { 17 } else { 20 },
            "field_declaration",
        )?;
        require(
            &parsed.source()[declaration.byte_range()]
                == if recovery {
                    b"int x;".as_slice()
                } else {
                    b"void f();".as_slice()
                },
            "combined fixture leaf declaration spelling",
        )?;
        require(
            index
                .facts()
                .associated_document(association)
                .map_err(failure)?
                == doc,
            "combined fixture document association identity",
        )?;
        let fact = association.get();
        require(
            fact.binding.is_some_and(|n| n.node() == declaration)
                && fact.lexical_scope.node() == scope,
            "combined fixture binding/scope identity",
        )?;
        let owner = index
            .facts()
            .associated_owner(association, 0)
            .map_err(failure)?;
        if recovery {
            let Owner::Slot(slot) = owner.get() else {
                return Err(HarnessError("recovery owner must be field slot".into()));
            };
            require(
                slot.role == SlotRole::Field && slot.ordinal == 0,
                "recovery field slot role/ordinal",
            )?;
            exact_piece(&slot.source, start + 15..start + 16, b"x")?;
            let enclosing = index.facts().slot_enclosing(owner).map_err(failure)?;
            require(
                matches!(enclosing.get(), Owner::Node(n) if n.node() == declaration),
                "recovery slot enclosure identity",
            )?;
        } else {
            require(
                matches!(owner.get(), Owner::Node(n) if n.node() == declaration),
                "conditional leaf owner identity",
            )?;
        }
        require(
            index
                .owner_associations(owner)
                .map_err(failure)?
                .associations()
                .collect::<Vec<_>>()
                == [association],
            "combined fixture owner adjacency",
        )?;
        comment_text(index, doc, start)?;
    }
    Ok(())
}
