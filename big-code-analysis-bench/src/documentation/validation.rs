//! Untimed semantic and grammar-shape checks for the documentation workloads.

use super::{HarnessError, Parsed, SourceProbe, failure, require};
use big_code_analysis_ast::documentation::{
    AssessmentAxis, Content, DocumentationIndex, OriginView, Owner, Resolution, UnresolvedReason,
};

pub(super) fn documents(
    index: &DocumentationIndex<'_>,
    count: usize,
    fragments: usize,
    owners: usize,
    expected: &str,
) -> Result<(), HarnessError> {
    require(index.documents().len() == count, "document count")?;
    require(index.associations().len() == count, "association count")?;
    for association in index.associations() {
        require(
            association.get().resolution == Resolution::Resolved,
            "resolved association",
        )?;
        require(
            association.get().owner_count() == owners,
            "owner edge count",
        )?;
        let document = index
            .facts()
            .associated_document(association)
            .map_err(failure)?;
        require(
            document.get().fragment_count() == fragments,
            "fragment count",
        )?;
        let text = index
            .complete_text(document)
            .map_err(failure)?
            .ok_or_else(|| HarnessError("complete text absent".into()))?;
        require(text.text() == expected, "complete text mismatch")?;
        let mut end = 0;
        for portion in text.origins(0..expected.len()).map_err(failure)? {
            require(portion.output.start == end, "map coverage start")?;
            end = portion.output.end;
            if let OriginView::Copied(piece) = portion.origin {
                require(
                    piece.bytes() == &expected.as_bytes()[portion.output],
                    "copied origin bytes",
                )?;
            }
        }
        require(end == expected.len(), "map coverage end")?;
        for position in 0..owners {
            let owner = index
                .facts()
                .associated_owner(association, position)
                .map_err(failure)?;
            let query = index.owner_associations(owner).map_err(failure)?;
            require(
                query.associations().collect::<Vec<_>>() == [association],
                "owner association adjacency",
            )?;
        }
    }
    Ok(())
}

pub(super) fn fixture(
    probe: &SourceProbe,
    size: usize,
    parsed: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    if matches!(
        probe.key,
        "depth"
            | "width"
            | "fragments"
            | "single-line-lua"
            | "single-line-jsdoc"
            | "c-slots"
            | "wide-error"
            | "missing"
            | "names"
            | "single-line-php"
            | "single-line-ruby"
            | "single-line-doxygen"
            | "text-fixed-fanout"
            | "fanout-fixed-text"
            | "visible-declarator-depth"
            | "ruby-modifiers"
            | "java-undocumented"
            | "pending-scope"
    ) {
        original_fixture(probe.key, size, parsed, index)?;
    } else {
        additional_fixture(probe, size, parsed, index)?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)] // Explicit independent expected outcomes for the finite fixture inventory.
fn additional_fixture(
    probe: &SourceProbe,
    size: usize,
    parsed: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    let nodes = parsed.nodes();
    let count = |kind| nodes.iter().filter(|(node, _)| node.kind() == kind).count();
    require(
        parsed.tree().root_node().has_error() == (probe.key == "recovery-depth"),
        format!("{}: parse shape", probe.id),
    )?;
    match probe.key {
        "ranges" => documents(index, size, 1, 1, "audit")?,
        "prepared" => {
            documents(
                index,
                1,
                size,
                1,
                &std::iter::repeat_n("a", size)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )?;
            let doc = index
                .documents()
                .next()
                .ok_or_else(|| HarnessError("document absent".into()))?;
            let text = index
                .complete_text(doc)
                .map_err(failure)?
                .ok_or_else(|| HarnessError("text absent".into()))?;
            require(text.mappings().len() == size * 2 - 1, "prepared map count")?;
            let portions: Vec<_> = text
                .origins(size * 2 - 2..size * 2 - 1)
                .map_err(failure)?
                .collect();
            require(portions.len() == 1, "late map K")?;
            let OriginView::Copied(piece) = &portions[0].origin else {
                return Err(HarnessError("late map not copied".into()));
            };
            require(
                piece.range() == ((size - 1) * 5 + 3..(size - 1) * 5 + 4),
                "late map source range",
            )?;
            require(
                nodes.iter().any(|(node, _)| {
                    node.kind() == "function_item" && node.start_byte() == 5 * size
                }),
                "prepared function anchor",
            )?;
        }
        "inline-spans" => documents(index, 1, 1, 1, &"`x` ".repeat(size)[..size * 4 - 1])?,
        "gathered-disposition" => {
            require(index.documents().len() == 1, "gathered document count")?;
            require(
                index
                    .documents()
                    .next()
                    .is_some_and(|d| d.get().fragment_count() == size + 1),
                "gathered fragment count",
            )?;
        }
        "complex-signatures" => {
            require(
                index.documents().len() == 0 && count("declaration") == size / 16 + 1,
                "complex declaration shape",
            )?;
            require(
                index
                    .assessments()
                    .filter(|a| a.get().reason == "Complex signature awaits exact named matching")
                    .count()
                    == size / 16,
                "complex signature pending count",
            )?;
        }
        "go-fold" => require(
            index.documents().len() == 0 && count("field_declaration") == size / 16 + 1,
            "go fold shape",
        )?,
        "text-budget-boundary" => documents(index, 1, 1, 1, &"abcd".repeat(size * 2))?,
        "ruby-fold" => documents(index, 1, 1, 1, "audit")?,
        "multiline-adjacent-ranges" => documents(
            index,
            1,
            1,
            1,
            &std::iter::repeat_n("audit", size)
                .collect::<Vec<_>>()
                .join("\n"),
        )?,
        "qualified-candidates" | "qualified-references" | "qualified-unrelated" => {
            qualifications(probe, size, index)?;
        }
        "pending-ancestry" | "pending-late" => {
            let leaf = nodes
                .iter()
                .rev()
                .find(|(node, _)| node.kind() == "function_item")
                .ok_or_else(|| HarnessError("pending query leaf absent".into()))?
                .0;
            let query = index.node_associations(leaf).map_err(failure)?;
            let expected = if probe.key == "pending-ancestry" {
                2 * size
            } else {
                2
            };
            require(query.assessments().count() == expected, "pending output K")?;
            require(count("mod_item") == size, "pending lexical depth")?;
            require(
                index.assessments().len()
                    == if probe.key == "pending-ancestry" {
                        3 * size
                    } else {
                        3
                    },
                "pending retained count",
            )?;
        }
        "wide-depth" => {
            documents(index, 32, 1, 1, "audit")?;
            require(
                count("mod_item") == size && count("function_item") == 32,
                "wide-depth shape",
            )?;
        }
        "comment-run-depth" => {
            documents(
                index,
                1,
                size,
                1,
                &std::iter::repeat_n("a", size)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )?;
            require(count("mod_item") == size, "comment-run depth")?;
        }
        "recovery-depth" => {
            require(
                count("field_declaration") == size + 31,
                "nested struct fields plus recovery sibling width",
            )?;
            require(count("struct_specifier") == size, "recovery struct depth")?;
            require(
                nodes.iter().any(|(node, depth)| {
                    node.kind() == "field_declaration_list"
                        && node.has_error()
                        && node.child_count() >= 64
                        && *depth >= size
                }),
                "deep recovered sibling parent absent",
            )?;
            require(index.documents().len() == 32, "recovery document count")?;
            super::attachment_validation::validate(probe.key, size, parsed, index)?;
        }
        "conditional-depth" => {
            require(
                count("struct_specifier") == size && count("preproc_if") == size,
                "conditional structure depth",
            )?;
            require(index.documents().len() == 1, "conditional document count")?;
            super::attachment_validation::validate(probe.key, size, parsed, index)?;
        }
        "missing-name" => {
            require(count("declaration") == size, "unrelated named declarations")?;
            require(
                index.associations().len() == 1,
                "missing-name association count",
            )?;
            require(
                index.associations().next().is_some_and(|a| {
                    a.get().resolution == Resolution::Unresolved(UnresolvedReason::MissingName)
                        && a.get().owner_count() == 0
                }),
                "valid MissingName outcome",
            )?;
        }
        "rejected" | "directives" => evidence(probe, size, parsed, index)?,
        _ => return Err(HarnessError(format!("unvalidated workload {}", probe.id))),
    }
    Ok(())
}

fn qualifications(
    probe: &SourceProbe,
    size: usize,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    use big_code_analysis_ast::documentation::QualificationSubject;
    let (documents, owners, total) = match probe.key {
        "qualified-candidates" => (1, size, size),
        "qualified-references" => (size, 2, 2 * size),
        _ => (size, 1, size),
    };
    require(
        index.documents().len() == documents && index.associations().len() == documents,
        "qualification D/A counts",
    )?;
    require(
        index.qualifications().len() == total,
        "qualification output Q",
    )?;
    for association in index.associations() {
        require(
            association.get().owner_count() == owners,
            "qualification edge count",
        )?;
        let doc = index
            .facts()
            .associated_document(association)
            .map_err(failure)?;
        require(
            index.document_qualifications(doc).map_err(failure)?.len() == owners,
            "document qualification adjacency",
        )?;
        let expected_resolution = if owners > 1 {
            UnresolvedReason::Ambiguous
        } else {
            UnresolvedReason::Conditional
        };
        require(
            association.get().resolution == Resolution::Unresolved(expected_resolution),
            "qualified association resolution",
        )?;
        let ordinal = index
            .document_qualifications(doc)
            .map_err(failure)?
            .next()
            .ok_or_else(|| HarnessError("document has no qualification".into()))?;
        require(
            index.qualified_document(ordinal).map_err(failure)? == doc,
            "qualification document identity",
        )?;
    }
    for q in index.qualifications() {
        require(
            q.get().subject == QualificationSubject::Candidate
                && q.get().reason == UnresolvedReason::Conditional,
            "candidate qualification kind",
        )?;
        require(
            index.qualified_owner(q).map_err(failure)?.is_some(),
            "candidate qualification owner identity",
        )?;
    }
    Ok(())
}

fn evidence(
    probe: &SourceProbe,
    size: usize,
    parsed: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    use big_code_analysis_ast::documentation::CandidateDisposition;
    let mut off = *probe;
    off.evidence = false;
    let control = parsed.collect(&off)?;
    require(
        control.evidence().len() == 0,
        "off control materialized evidence",
    )?;
    let expected = if probe.key == "rejected" {
        size - 1
    } else {
        size
    };
    require(
        index.evidence().len() == expected,
        "retained evidence count",
    )?;
    require(
        index.documents().len() == usize::from(probe.key == "rejected"),
        "evidence document count",
    )?;
    if probe.key == "directives" {
        require(
            index
                .evidence()
                .all(|e| e.get().disposition == CandidateDisposition::Directive),
            "directive disposition",
        )?;
    } else {
        require(
            index
                .evidence()
                .all(|e| e.get().disposition == CandidateDisposition::Superseded),
            "rejected disposition",
        )?;
        documents(&control, 1, 1, 1, "audit")?;
    }
    require(
        control.documents().len() == index.documents().len(),
        "retention changed selected documents",
    )?;
    Ok(())
}

fn shape(name: &str, size: usize, parser: &Parsed) -> Result<(), HarnessError> {
    let root = parser.tree().root_node();
    require(
        root.has_error() == matches!(name, "wide-error" | "missing"),
        format!("{name}: parse error shape"),
    )?;
    let walk = parser.nodes();
    let maximum = walk.iter().map(|(_, depth)| *depth).max().unwrap_or(0);
    let nodes: Vec<_> = walk.into_iter().map(|(node, _)| node).collect();
    let count = |kind| nodes.iter().filter(|node| node.kind() == kind).count();
    match name {
        "depth" => {
            require(
                count("mod_item") == size && maximum >= 2 * size,
                "module depth",
            )?;
        }
        "width" => require(root.child_count() == size * 2, "sibling width")?,
        "fragments" => require(count("comment") == size, "comment fragments")?,
        "c-slots" | "fanout-fixed-text" => {
            require(count("identifier") == size, "declaration slots")?;
        }
        "visible-declarator-depth" => {
            require(
                count("array_declarator") == size && maximum >= size,
                "array depth",
            )?;
        }
        "ruby-modifiers" => {
            require(
                count("if_modifier") == size && maximum >= size,
                "modifier depth",
            )?;
        }
        "java-undocumented" => {
            require(
                count("class_declaration") == size && maximum >= 2 * size,
                "class depth",
            )?;
        }
        "pending-scope" => require(count("function_item") == size, "pending scope functions")?,
        "names" => require(count("function_definition") == size, "named functions")?,
        "missing" => {
            require(
                nodes.iter().filter(|node| node.is_missing()).count() == 1 && maximum >= size,
                "missing token depth",
            )?;
        }
        "wide-error" => {
            let error = nodes
                .iter()
                .find(|node| node.is_error())
                .ok_or_else(|| HarnessError("wide ERROR absent".into()))?;
            require(
                error.child_count() >= size && count("field_declaration") == size,
                "wide recovery fields",
            )?;
        }
        _ => require(
            matches!(
                name,
                "single-line-lua"
                    | "single-line-jsdoc"
                    | "single-line-php"
                    | "single-line-ruby"
                    | "single-line-doxygen"
                    | "text-fixed-fanout"
            ),
            "unregistered shape",
        )?,
    }
    Ok(())
}

#[allow(clippy::too_many_lines)] // One explicit expected outcome per benchmark workload.
fn original_fixture(
    name: &str,
    size: usize,
    parser: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    shape(name, size, parser)?;
    match name {
        "depth" | "ruby-modifiers" | "visible-declarator-depth" => {
            documents(index, 1, 1, 1, "audit")?;
        }
        "width" => documents(index, size, 1, 1, "audit")?,
        "fragments" => documents(
            index,
            1,
            size,
            1,
            &std::iter::repeat_n("audit", size)
                .collect::<Vec<_>>()
                .join("\n"),
        )?,
        "c-slots" => documents(index, 1, 1, size, "audit")?,
        "single-line-lua" | "single-line-ruby" => {
            documents(index, 1, 1, 1, &"audit ".repeat(size))?;
        }
        "single-line-jsdoc" | "single-line-php" | "single-line-doxygen" => {
            documents(index, 1, 1, 1, &"audit ".repeat(size)[..size * 6 - 1])?;
        }
        "text-fixed-fanout" => documents(index, 1, 1, 2, &"audit ".repeat(size)[..size * 6 - 1])?,
        "fanout-fixed-text" => documents(index, 1, 1, size, &"audit ".repeat(1024)[..6143])?,
        "java-undocumented" => {
            require(
                index.documents().len() == 0 && index.facts().owners().len() == size + 2,
                "undocumented owner count",
            )?;
        }
        "missing" => {
            require(
                index.documents().len() == 0 && index.associations().len() == 0,
                "missing token output",
            )?;
        }
        "wide-error" => {
            require(index.documents().len() == 1, "recovery document count")?;
            let association = index
                .associations()
                .next()
                .ok_or_else(|| HarnessError("recovery association absent".into()))?;
            require(
                association.get().resolution
                    == Resolution::Unresolved(UnresolvedReason::GrammarBoundary)
                    && association.get().owner_count() == 0,
                "recovery association state",
            )?;
        }
        "pending-scope" => {
            require(index.documents().len() == 1, "pending document count")?;
            let document = index
                .documents()
                .next()
                .ok_or_else(|| HarnessError("pending attribute absent".into()))?;
            require(
                matches!(document.get().content(), Content::PendingAdapter)
                    && index.assessments().len() == 3,
                "pending state",
            )?;
            let last = parser
                .nodes()
                .into_iter()
                .map(|(node, _)| node)
                .rfind(|node| node.kind() == "function_item")
                .ok_or_else(|| HarnessError("last function absent".into()))?;
            let query = index.node_associations(last).map_err(failure)?;
            require(
                query.associations().len() == 0,
                "pending owner associations",
            )?;
            require(
                query
                    .assessments()
                    .map(|assessment| assessment.get().axis)
                    .collect::<Vec<_>>()
                    == [AssessmentAxis::Occurrence, AssessmentAxis::Publication],
                "pending assessment axes",
            )?;
        }
        "names" => {
            require(
                index.documents().len() == size && index.associations().len() == size,
                "named D/A counts",
            )?;
            for (ordinal, association) in index.associations().enumerate() {
                require(
                    association.get().resolution == Resolution::Resolved
                        && association.get().owner_count() == 1,
                    "named resolution",
                )?;
                let owner = index
                    .facts()
                    .associated_owner(association, 0)
                    .map_err(failure)?;
                let Owner::Node(node) = owner.get() else {
                    return Err(HarnessError("named owner is not a node".into()));
                };
                require(
                    &parser.source()[node.node().byte_range()]
                        == format!("void f{ordinal}() {{}}").as_bytes(),
                    "named owner spelling",
                )?;
                let document = index
                    .facts()
                    .associated_document(association)
                    .map_err(failure)?;
                require(
                    index
                        .complete_text(document)
                        .map_err(failure)?
                        .ok_or_else(|| HarnessError("named text absent".into()))?
                        .text()
                        == format!("@fn void f{ordinal}()"),
                    "named reference spelling",
                )?;
            }
        }
        _ => return Err(HarnessError(format!("unvalidated workload {name}"))),
    }
    Ok(())
}
