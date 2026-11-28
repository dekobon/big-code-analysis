use std::collections::HashMap;

use big_code_analysis_ast::documentation::{CompleteText, DocumentationIndex, FactRef, Owner};

use super::{
    Check,
    identity::{address, evidence},
    require,
};

pub(super) fn ordinals<'i, T: 'i>(
    facts: impl Iterator<Item = FactRef<'i, T>>,
) -> HashMap<*const T, usize> {
    facts
        .enumerate()
        .map(|(at, fact)| (address(fact), at))
        .collect()
}

pub(super) fn corresponding<T>(
    left: FactRef<'_, T>,
    right: FactRef<'_, T>,
    a: &HashMap<*const T, usize>,
    b: &HashMap<*const T, usize>,
) -> Check {
    let left = a.get(&address(left)).ok_or("first unenumerated record")?;
    let right = b.get(&address(right)).ok_or("second unenumerated record")?;
    require(left == right, "nondeterministic linked identity")
}

pub(super) fn complete(
    left: Option<CompleteText<'_, '_>>,
    right: Option<CompleteText<'_, '_>>,
) -> Check {
    match (left, right) {
        (Some(left), Some(right)) => require(
            left.text() == right.text() && left.mappings() == right.mappings(),
            "nondeterministic source-cleaned text",
        ),
        (None, None) => Ok(()),
        _ => Err("nondeterministic source-cleaned availability"),
    }
}

pub(super) fn owners<'s>(
    first: &DocumentationIndex<'s>,
    second: &DocumentationIndex<'s>,
    a: &HashMap<*const Owner<'s>, usize>,
    b: &HashMap<*const Owner<'s>, usize>,
) -> Check {
    for (left, right) in first.facts().owners().zip(second.facts().owners()) {
        match (left.get(), right.get()) {
            (Owner::Node(a), Owner::Node(b)) | (Owner::File(a), Owner::File(b)) => {
                evidence(Some(*a), Some(*b))?;
            }
            (Owner::Slot(x), Owner::Slot(y)) => {
                require(
                    x.role == y.role && x.ordinal == y.ordinal && x.source == y.source,
                    "nondeterministic slot fields",
                )?;
                corresponding(
                    first
                        .facts()
                        .slot_enclosing(left)
                        .map_err(|_| "first slot enclosure")?,
                    second
                        .facts()
                        .slot_enclosing(right)
                        .map_err(|_| "second slot enclosure")?,
                    a,
                    b,
                )?;
            }
            (Owner::Group(a), Owner::Group(b)) => {
                evidence(Some(a.scope), Some(b.scope))?;
                require(
                    a.pieces.len() == b.pieces.len(),
                    "nondeterministic group pieces",
                )?;
                for (a, b) in a.pieces.iter().zip(&b.pieces) {
                    require(a.source == b.source, "nondeterministic group source")?;
                    evidence(a.node, b.node)?;
                }
            }
            (Owner::Virtual(a), Owner::Virtual(b)) => require(
                a.name == b.name && a.declaration == b.declaration,
                "nondeterministic virtual spelling",
            )?,
            _ => return Err("nondeterministic owner variant"),
        }
    }
    Ok(())
}
