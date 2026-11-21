//! Separate owned parse, borrowed index and prepared query lifetimes.

use std::hint::black_box;

use big_code_analysis::{Ast, Source};
use big_code_analysis_ast::documentation::render_probe::PreparedRender;
use big_code_analysis_ast::documentation::{
    CompleteText, Content, Document, DocumentationContext, DocumentationIndex,
    DocumentationOptions, FactRef, Owner, collect,
};
use big_code_analysis_ast::{LANG, tree_sitter};

use super::fixtures::Operation;
use super::{HarnessError, SourceProbe, failure, require};

/// Owned parse storage. Restricted cells retain their exact tree and source.
pub enum Parsed {
    /// Ordinary inputs are parsed once through the root facade.
    Ordinary(Ast),
    /// Deliberately restricted parse; never cloned, adopted or reparsed.
    Restricted {
        /// Exact parse buffer.
        source: Vec<u8>,
        /// Tree carrying the explicit included ranges.
        tree: tree_sitter::Tree,
        /// Selected grammar.
        language: LANG,
    },
}

impl Parsed {
    /// Parse a registry fixture once, outside measurement.
    ///
    /// # Errors
    /// Rejects invalid fixture generation, grammar setup and incomplete parses.
    pub fn new(probe: &SourceProbe, size: usize) -> Result<Self, HarnessError> {
        let source = probe.source(size)?;
        if !matches!(probe.key, "ranges" | "multiline-adjacent-ranges") {
            return Ast::parse(Source::new(probe.language, &source))
                .map(Self::Ordinary)
                .map_err(failure);
        }
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&probe.language.tree_sitter_language().map_err(failure)?)
            .map_err(failure)?;
        let ranges = included_ranges(&source, probe.key == "ranges");
        parser.set_included_ranges(&ranges).map_err(failure)?;
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| HarnessError("incomplete restricted parse".into()))?;
        require(
            tree.included_ranges() == ranges,
            "restricted range membership changed",
        )?;
        Ok(Self::Restricted {
            source,
            tree,
            language: probe.language,
        })
    }

    pub(super) fn language(&self) -> LANG {
        match self {
            Self::Ordinary(ast) => ast.language(),
            Self::Restricted { language, .. } => *language,
        }
    }

    /// The actual held parse buffer, including any facade preprocessing.
    #[must_use]
    pub fn source(&self) -> &[u8] {
        match self {
            Self::Ordinary(ast) => ast.source(),
            Self::Restricted { source, .. } => source,
        }
    }

    pub(super) fn tree(&self) -> &tree_sitter::Tree {
        match self {
            Self::Ordinary(ast) => ast.as_tree_sitter(),
            Self::Restricted { tree, .. } => tree,
        }
    }

    pub(super) fn nodes(&self) -> Vec<(tree_sitter::Node<'_>, usize)> {
        let mut nodes = Vec::new();
        let mut cursor = self.tree().walk();
        let mut depth = 0;
        loop {
            nodes.push((cursor.node(), depth));
            if cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return nodes;
                }
                depth -= 1;
            }
        }
    }

    /// Collect from exact borrowed language/tree/source accessors.
    ///
    /// # Errors
    /// Propagates every collection failure as a harness error.
    pub fn collect(&self, probe: &SourceProbe) -> Result<DocumentationIndex<'_>, HarnessError> {
        self.collect_with(&DocumentationContext::default(), &probe.options())
    }

    #[allow(clippy::trivially_copy_pass_by_ref)] // Preserve the borrowed production options boundary inside timing.
    fn collect_with(
        &self,
        context: &DocumentationContext,
        options: &DocumentationOptions,
    ) -> Result<DocumentationIndex<'_>, HarnessError> {
        collect(
            black_box(self.language()),
            black_box(self.tree()),
            black_box(self.source()),
            black_box(context),
            black_box(options),
        )
        .map_err(failure)
    }
}

fn included_ranges(source: &[u8], rows: bool) -> Vec<tree_sitter::Range> {
    let mut point = tree_sitter::Point::new(0, 0);
    let mut start_point = point;
    let mut start_byte = 0;
    let mut ranges = Vec::new();
    for (at, byte) in source.iter().enumerate() {
        if *byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
        if !rows || (at + 1).is_multiple_of(19) {
            ranges.push(tree_sitter::Range {
                start_byte,
                end_byte: at + 1,
                start_point,
                end_point: point,
            });
            start_byte = at + 1;
            start_point = point;
        }
    }
    ranges
}

/// Borrowed state constructed only after the owned parses and indexes are stable.
pub struct Prepared<'i, 's> {
    probe: &'i SourceProbe,
    parsed: &'s Parsed,
    index: &'i DocumentationIndex<'s>,
    document: Option<FactRef<'i, Document<'s>>>,
    node: Option<tree_sitter::Node<'s>>,
    owner: Option<FactRef<'i, Owner<'s>>>,
    text: Option<CompleteText<'i, 's>>,
    render: Option<PreparedRender<'i, 's>>,
    counts: SourceCounts,
    context: DocumentationContext,
    options: DocumentationOptions,
    off_options: DocumentationOptions,
}

/// Untimed input/output dimensions reported independently from elapsed time.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceCounts {
    /// Visible parse nodes.
    pub nodes: usize,
    /// Maximum visible tree depth, root at zero.
    pub depth: usize,
    /// Maximum direct child count of a visible node.
    pub width: usize,
    /// Physical retained fragments.
    pub fragments: usize,
    /// Logical documents.
    pub documents: usize,
    /// Association-owner edges, including legitimate fanout.
    pub edges: usize,
    /// Candidate/reference qualification records.
    pub qualifications: usize,
    /// Complete text bytes stored once per document.
    pub rendered_bytes: usize,
    /// Complete map segments stored once per document.
    pub map_segments: usize,
    /// Retained included ranges.
    pub ranges: usize,
    /// Retained fragment/source intersections.
    pub intersections: usize,
    /// Records consumed by one selected query; zero for construction.
    pub query_output: usize,
}

impl SourceCounts {
    fn new(parsed: &Parsed, index: &DocumentationIndex<'_>) -> Result<Self, HarnessError> {
        let nodes = parsed.nodes();
        let mut counts = Self {
            nodes: nodes.len(),
            depth: nodes.iter().map(|(_, depth)| *depth).max().unwrap_or(0),
            width: nodes
                .iter()
                .map(|(node, _)| node.child_count())
                .max()
                .unwrap_or(0),
            fragments: index.facts().fragments().len(),
            documents: index.documents().len(),
            edges: index.associations().map(|a| a.get().owner_count()).sum(),
            qualifications: index.qualifications().len(),
            ranges: parsed.tree().included_ranges().len(),
            intersections: index
                .facts()
                .fragments()
                .map(|f| f.get().source().pieces().len())
                .sum(),
            ..Self::default()
        };
        for doc in index.documents() {
            if let Some(text) = index.complete_text(doc).map_err(failure)? {
                counts.rendered_bytes += text.text().len();
                counts.map_segments += text.mappings().len();
            }
        }
        Ok(counts)
    }
}

impl<'i, 's> Prepared<'i, 's> {
    /// Validate every actual rung, then prepare the selected query/render handles.
    ///
    /// # Errors
    /// Rejects shape/output drift or an operation with unavailable prepared state.
    pub fn new(
        probe: &'i SourceProbe,
        size: usize,
        parsed: &'s Parsed,
        index: &'i DocumentationIndex<'s>,
    ) -> Result<Self, HarnessError> {
        super::validation::fixture(probe, size, parsed, index)?;
        let document = index.documents().last();
        let node = parsed
            .nodes()
            .into_iter()
            .rev()
            .map(|(node, _)| node)
            .find(|node| node.kind() == "function_item");
        let owner = if probe.operation == Operation::OwnerQualifications {
            let association = index
                .associations()
                .last()
                .ok_or_else(|| HarnessError("qualification association absent".into()))?;
            Some(
                index
                    .facts()
                    .associated_owner(association, 0)
                    .map_err(failure)?,
            )
        } else {
            None
        };
        let text = document
            .map(|doc| index.complete_text(doc))
            .transpose()
            .map_err(failure)?
            .flatten();
        let render = if probe.operation == Operation::Render {
            Some(
                PreparedRender::new(
                    index,
                    document.ok_or_else(|| HarnessError("render document absent".into()))?,
                )
                .map_err(failure)?,
            )
        } else {
            None
        };
        let mut prepared = Self {
            probe,
            parsed,
            index,
            document,
            node,
            owner,
            text,
            render,
            counts: SourceCounts::new(parsed, index)?,
            context: DocumentationContext::default(),
            options: probe.options(),
            off_options: DocumentationOptions::default(),
        };
        prepared.validate_operation(size)?;
        prepared.counts.query_output = match probe.operation {
            Operation::Owner
            | Operation::Retained
            | Operation::Map
            | Operation::OwnerQualifications
            | Operation::DocumentQualifications => 1,
            Operation::Pending => {
                if probe.key == "pending-ancestry" {
                    size * 2
                } else {
                    2
                }
            }
            _ => 0,
        };
        Ok(prepared)
    }

    #[allow(clippy::too_many_lines)] // Each operation has an independent untimed output control.
    fn validate_operation(&self, size: usize) -> Result<(), HarnessError> {
        match self.probe.operation {
            Operation::Collect | Operation::EvidencePair => {}
            Operation::Owner => {
                let node = self
                    .node
                    .ok_or_else(|| HarnessError("owner node absent".into()))?;
                let expected = if self.probe.key == "width" {
                    (size - 1) * 19 + 9
                } else {
                    5 * size
                };
                require(
                    node.start_byte() == expected,
                    "prepared owner source anchor",
                )?;
                require(
                    self.index
                        .node_associations(node)
                        .map_err(failure)?
                        .associations()
                        .count()
                        == 1,
                    "prepared owner K=1",
                )?;
            }
            Operation::Render => {
                let rendered = self
                    .render
                    .as_ref()
                    .ok_or_else(|| HarnessError("render not prepared".into()))?
                    .render()
                    .map_err(failure)?;
                let Content::Complete { text, mappings, .. } = rendered else {
                    return Err(HarnessError("fresh render incomplete".into()));
                };
                let retained = self
                    .text
                    .ok_or_else(|| HarnessError("retained render control missing".into()))?;
                require(
                    text == retained.text() && mappings == retained.mappings(),
                    "fresh render text/map mismatch",
                )?;
            }
            Operation::Pending => {
                let node = self
                    .node
                    .ok_or_else(|| HarnessError("pending node absent".into()))?;
                let query = self.index.node_associations(node).map_err(failure)?;
                let expected = if self.probe.key == "pending-ancestry" {
                    size * 2
                } else {
                    2
                };
                require(query.assessments().count() == expected, "pending query K")?;
            }
            Operation::OwnerQualifications => {
                let owner = self
                    .owner
                    .ok_or_else(|| HarnessError("qualification owner absent".into()))?;
                require(
                    self.index
                        .owner_associations(owner)
                        .map_err(failure)?
                        .qualifications()
                        .count()
                        == 1,
                    "owner qualification K=1",
                )?;
            }
            Operation::DocumentQualifications => {
                let document = self
                    .document
                    .ok_or_else(|| HarnessError("qualification document absent".into()))?;
                require(
                    self.index
                        .document_qualifications(document)
                        .map_err(failure)?
                        .count()
                        == 1,
                    "document qualification K=1",
                )?;
            }
            Operation::Map | Operation::Retained => {
                require(self.text.is_some(), "complete prepared text absent")?;
                self.execute()?;
            }
        }
        Ok(())
    }

    /// Run exactly one operation, consuming lazy results and dropping owned output.
    ///
    /// # Errors
    /// Returns missing-state, collection, query and rendering failures.
    pub fn execute(&self) -> Result<(), HarnessError> {
        match self.probe.operation {
            Operation::EvidencePair => {
                drop(black_box(
                    self.parsed.collect_with(&self.context, &self.off_options)?,
                ));
                drop(black_box(
                    self.parsed.collect_with(&self.context, &self.options)?,
                ));
            }
            Operation::Collect => {
                drop(black_box(
                    self.parsed.collect_with(&self.context, &self.options)?,
                ));
            }
            Operation::Render => {
                drop(black_box(
                    black_box(
                        self.render
                            .as_ref()
                            .ok_or_else(|| HarnessError("render not prepared".into()))?,
                    )
                    .render()
                    .map_err(failure)?,
                ));
            }
            Operation::Owner | Operation::Pending => {
                let node = self
                    .node
                    .ok_or_else(|| HarnessError("query node absent".into()))?;
                let query = black_box(self.index)
                    .node_associations(black_box(node))
                    .map_err(failure)?;
                if self.probe.operation == Operation::Pending {
                    for assessment in query.assessments() {
                        black_box(assessment.get());
                    }
                } else {
                    for association in query.associations() {
                        black_box(association.get());
                    }
                }
            }
            Operation::OwnerQualifications => {
                let owner = self
                    .owner
                    .ok_or_else(|| HarnessError("qualification owner absent".into()))?;
                let query = black_box(self.index)
                    .owner_associations(black_box(owner))
                    .map_err(failure)?;
                for qualification in query.qualifications() {
                    black_box(qualification.get());
                }
            }
            Operation::DocumentQualifications => {
                let doc = self
                    .document
                    .ok_or_else(|| HarnessError("qualification document absent".into()))?;
                for qualification in black_box(self.index)
                    .document_qualifications(black_box(doc))
                    .map_err(failure)?
                {
                    black_box(qualification.get());
                }
            }
            Operation::Map => {
                let text = black_box(
                    self.text
                        .ok_or_else(|| HarnessError("map text absent".into()))?,
                );
                let end = text.text().len();
                require(end > 0, "late map query has no text")?;
                for portion in text.origins(black_box(end - 1..end)).map_err(failure)? {
                    black_box(portion);
                }
            }
            Operation::Retained => {
                let doc = self
                    .document
                    .ok_or_else(|| HarnessError("retained document absent".into()))?;
                black_box(
                    black_box(self.index)
                        .complete_text(black_box(doc))
                        .map_err(failure)?,
                );
            }
        }
        Ok(())
    }

    /// Number of retained output-map segments, for the declared logarithmic axis.
    #[must_use]
    pub fn segments(&self) -> usize {
        self.text.map_or(0, |text| text.mappings().len())
    }

    /// Independently observed input/output dimensions, collected before timing.
    #[must_use]
    pub fn counts(&self) -> SourceCounts {
        self.counts
    }
}
