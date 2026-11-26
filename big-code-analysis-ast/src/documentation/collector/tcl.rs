//! Static Ruff! documentation of a Tcl `proc`: the lead block of its body and
//! the later `#ruff` blocks, read once when the walk leaves the procedure.
//!
//! The free functions read the canonical tree and the source. The `Builder`
//! methods record what they found.

use super::*;
use crate::Tcl;
use crate::lang_helpers::tcl::{BODY_ROW_KINDS, ruff_row};
use crate::lang_helpers::tcl_family::{BodyRow, BodyRows, braced_word_end};
use crate::lang_helpers::tcl_scan::{Command, Listed};

pub(super) use crate::lang_helpers::tcl_scan::CommandScan;

const OBSCURED: &str = "Recovered Tcl body rows may hold Ruff documentation";

// A `proc` whose own words can be read, and its four children.
pub(super) struct Slots {
    procedure: NodeId,
    keyword: NodeId,
    name: NodeId,
    arguments: NodeId,
    pub(super) body: NodeId,
}

// The documentation rows among a body's direct children, each with the end
// of its spelling.
#[derive(Default)]
pub(super) struct Rows {
    pub(super) members: Vec<(NodeId, usize)>,
    pub(super) directives: Vec<(NodeId, usize)>,
    pub(super) obscured: Option<NodeId>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Standing {
    Owner,
    Pending,
    Uncertain(UnresolvedReason),
    // Tcl ends a braced word of the procedure elsewhere than the grammar
    // does: no owner, and the rows are not known to be rows of Tcl's body.
    Parted,
}

fn node<'s>(canonical: &CanonicalNodes<'s>, id: NodeId) -> tree_sitter::Node<'s> {
    canonical.nodes[id.0].evidence.node()
}

fn children<'c>(canonical: &'c CanonicalNodes<'_>, id: NodeId) -> &'c [NodeId] {
    &canonical.children[canonical.nodes[id.0].children.clone()]
}

// A token of that kind that the parser read and did not invent.
fn read(canonical: &CanonicalNodes<'_>, id: NodeId, kind: u16) -> bool {
    let node = node(canonical, id);
    node.kind_id() == kind && node.start_byte() < node.end_byte()
}

// The words of a `proc` that can be read: the name and the parameter list
// hold no recovery node, and the body is a braced word. With four children
// the grammar leaves no other order than `proc`, one name word, `arguments`
// and one body word, and only a `braced_word` starts and ends with a brace.
// A recovery node deeper in the body does not count: `has_error()` of the
// procedure is not asked.
fn procedure_slots(canonical: &CanonicalNodes<'_>, procedure: NodeId) -> Option<Slots> {
    let &[keyword, name, arguments, body] = children(canonical, procedure) else {
        return None;
    };
    let braces = children(canonical, body);
    let (&open, &close) = braces.first().zip(braces.last())?;
    (!node(canonical, name).has_error()
        && !node(canonical, arguments).has_error()
        && read(canonical, open, Tcl::LBRACE as u16)
        && read(canonical, close, Tcl::RBRACE as u16))
    .then_some(Slots {
        procedure,
        keyword,
        name,
        arguments,
        body,
    })
}

pub(super) fn body_rows<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    body: NodeId,
    budget: &B,
) -> Result<Rows, B::Error> {
    let between = children(&index.canonical, body);
    let mut reader = BodyRows::new(index.source.bytes(), BODY_ROW_KINDS, budget);
    let mut rows = Rows::default();
    // `procedure_slots` has shown that the first and the last child are the braces.
    for &child in &between[1..between.len() - 1] {
        budget.step()?;
        match reader.read(node(&index.canonical, child), ruff_row)? {
            BodyRow::Outside => {}
            BodyRow::Member(end) => rows.members.push((child, end)),
            BodyRow::Directive(end) => rows.directives.push((child, end)),
            BodyRow::Obscured => {
                rows.obscured = Some(child);
                break;
            }
        }
    }
    Ok(rows)
}

// Whether Tcl ends a braced word of the procedure elsewhere than its node
// ends. Tcl counts braces and nothing else; the grammar reads a comment, a
// quoted word and a bracket first, and can end the node before or after.
// The count stops at the end of the node: an end behind it is elsewhere too.
fn parted<B: WorkBudget>(
    index: &DocumentationIndex<'_>,
    slots: &Slots,
    budget: &B,
) -> Result<bool, B::Error> {
    let canonical = &index.canonical;
    for word in [slots.arguments, slots.body] {
        budget.step()?;
        let (start, end) = (
            node(canonical, word).start_byte(),
            node(canonical, word).end_byte(),
        );
        let braced = children(canonical, word)
            .first()
            .is_some_and(|&open| read(canonical, open, Tcl::LBRACE as u16));
        if braced && braced_word_end(&index.source.bytes()[..end], start, budget)? != Some(end) {
            return Ok(true);
        }
    }
    Ok(false)
}

// Why the procedure is not the `proc` command Tcl would run, if it is not.
// It is that command if the scan of the file lists a command of the file's
// own level that starts where the node starts and has the node's four
// children as its four words, none of them expanded. Excluded bytes that
// hide the answer are a reason of their own.
fn unlisted<'s, B: WorkBudget>(
    index: &DocumentationIndex<'s>,
    slots: &Slots,
    commands: &mut CommandScan<'s>,
    budget: &B,
) -> Result<Option<UnresolvedReason>, SourceError> {
    let canonical = &index.canonical;
    let span = |id: NodeId| {
        (
            node(canonical, id).start_byte(),
            node(canonical, id).end_byte(),
        )
    };
    let command = Command {
        start: span(slots.procedure).0,
        words: [slots.keyword, slots.name, slots.arguments, slots.body].map(span),
        count: 4,
        expands: false,
    };
    Ok(
        match commands.command_at(&index.source, command.start, budget)? {
            Listed::Unknown => Some(UnresolvedReason::ExcludedGap),
            Listed::Command(listed) if listed == command => None,
            Listed::Command(_) | Listed::Nothing => Some(UnresolvedReason::GrammarBoundary),
        },
    )
}

// Whether the procedure is an owner this unit can name. Scopes other than
// the file are a later unit's.
fn standing<'s, B: WorkBudget>(
    index: &DocumentationIndex<'s>,
    slots: &Slots,
    commands: &mut CommandScan<'s>,
    budget: &B,
) -> Result<Standing, SourceError> {
    let canonical = &index.canonical;
    Ok(
        if canonical.nodes[slots.procedure.0].parent != Some(NodeId(0)) {
            Standing::Pending
        } else if parted(index, slots, budget).map_err(Into::into)? {
            Standing::Parted
        } else if let Some(reason) = unlisted(index, slots, commands, budget)? {
            Standing::Uncertain(reason)
        } else {
            match Tcl::from(node(canonical, slots.name).kind_id()) {
                Tcl::SimpleWord => Standing::Owner,
                Tcl::VariableSubstitution | Tcl::CommandSubstitution => {
                    Standing::Uncertain(UnresolvedReason::Dynamic)
                }
                _ => Standing::Pending,
            }
        },
    )
}

// A procedure whose bytes are all included and whose words can be read:
// its words, and whether it is an owner.
pub(super) fn read_procedure<'s, B: WorkBudget>(
    index: &DocumentationIndex<'s>,
    procedure: NodeId,
    commands: &mut CommandScan<'s>,
    budget: &B,
) -> Result<Option<(Slots, Standing)>, SourceError> {
    let span = node(&index.canonical, procedure).byte_range();
    let included = index
        .source
        .covers_with_budget(span, budget)
        .map_err(Into::into)?;
    let Some(slots) = procedure_slots(&index.canonical, procedure).filter(|_| included) else {
        return Ok(None);
    };
    let standing = standing(index, &slots, commands, budget)?;
    Ok(Some((slots, standing)))
}

impl Builder<'_> {
    pub(super) fn ruff_procedure(
        &mut self,
        id: NodeId,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        if node(&self.index.canonical, id).kind_id() != Tcl::Procedure as u16 {
            return Ok(());
        }
        // Pending until the procedure proves to be an owner.
        self.index.canonical.nodes[id.0].projection = Projection::Pending;
        let Some((slots, standing)) = read_procedure(&self.index, id, &mut self.tcl_scan, budget)?
        else {
            return Ok(());
        };
        let unresolved = match standing {
            Standing::Owner => {
                self.add_owner(id, Owner::Node(self.index.canonical.nodes[id.0].evidence));
                None
            }
            Standing::Pending => None,
            Standing::Uncertain(reason) => Some(reason),
            Standing::Parted => Some(UnresolvedReason::GrammarBoundary),
        };
        // In a body that Tcl ends elsewhere, a comment node is no evidence
        // of a comment: the body word carries the rows, and they get no text.
        let carrier = (standing == Standing::Parted).then_some(slots.body);
        self.ruff_document(&slots, unresolved, carrier, budget)
    }

    // Reads the body, and files one document for its rows if it has any.
    fn ruff_document(
        &mut self,
        slots: &Slots,
        unresolved: Option<UnresolvedReason>,
        carrier: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        let (procedure, body) = (slots.procedure, slots.body);
        let rows = body_rows(&self.index, body, budget)?;
        if let Some(recovery) = rows.obscured {
            let assessment = self.index.assessments.len();
            self.index.assessments.push(Assessment {
                axis: AssessmentAxis::Selection,
                reason: OBSCURED,
                node: Some(self.index.canonical.nodes[recovery.0].evidence),
                document: None,
            });
            self.index.canonical.nodes[procedure.0]
                .pending
                .push(assessment);
            return Ok(());
        }
        if self.index.options.retain_directives {
            self.ruff_directives(&rows.directives, carrier, budget)?;
        }
        let fragments = self.ruff_members(&rows.members, carrier, budget)?;
        if !fragments.is_empty() {
            self.drafts.push(Draft {
                fragments,
                target: Some(procedure),
                scope: body,
                binding: None,
                role: DocumentRole::BodyComment,
                unresolved,
                pending: false,
                gap: false,
            });
        }
        Ok(())
    }

    fn ruff_fragment(
        &mut self,
        (comment, end): (NodeId, usize),
        carrier: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<FragmentId, CollectionError> {
        let nodes = &self.index.canonical.nodes;
        let start = nodes[comment.0].evidence.node().start_byte();
        let evidence = nodes[carrier.unwrap_or(comment).0].evidence;
        let source = self.index.source.pieces_with_budget(start..end, budget)?;
        Ok(self.fragment(source, evidence, FragmentForm::Comment))
    }

    fn ruff_directives(
        &mut self,
        rows: &[(NodeId, usize)],
        carrier: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<(), CollectionError> {
        for &row in rows {
            budget.step()?;
            let fragment = self.ruff_fragment(row, carrier, budget)?;
            self.index.evidence.push(CandidateEvidence {
                fragment,
                disposition: CandidateDisposition::Directive,
            });
        }
        Ok(())
    }

    fn ruff_members(
        &mut self,
        rows: &[(NodeId, usize)],
        carrier: Option<NodeId>,
        budget: &CollectionBudget,
    ) -> Result<Vec<FragmentId>, CollectionError> {
        let mut fragments = Vec::new();
        for &row in rows {
            budget.step()?;
            fragments.push(self.ruff_fragment(row, carrier, budget)?);
        }
        Ok(fragments)
    }
}
