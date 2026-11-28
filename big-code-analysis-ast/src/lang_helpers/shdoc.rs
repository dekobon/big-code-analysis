//! shdoc source rules for Bash: which function a tagged comment run
//! documents, which rows of a run are not prose, and which nodes of the
//! grammar stand where Bash reads a comment or a command. Only the
//! documentation collector reads this module; no role row and no legacy
//! `Checker` answer does.

pub(crate) mod scan;

use crate::documentation::budget::WorkBudget;
use crate::documentation::streaming::positional::{
    OpenerReason, RunContext, RunSelection, SourceRun,
};
use crate::documentation::{SourceContext, SourceError, UnresolvedReason};
use crate::lang_helpers::doc_comment::node_bytes;
use crate::lang_helpers::doc_text::{
    ShdocRow, shdoc_row_with_budget, shellcheck_directive_with_budget,
};
use crate::node::Node;
use scan::{Place, Scan};

/// A function definition, in either spelling.
pub(crate) fn target(node: &Node<'_>) -> bool {
    crate::roles::bash::func_opener::OWNERS.contains(&node.kind_id())
}

/// A row that is not prose: the `#!` row at the first byte of the source,
/// or a ShellCheck directive. `comment` holds the bytes of `node`.
pub(crate) fn directive_with_budget<B: WorkBudget>(
    node: &Node<'_>,
    comment: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    Ok((node.start_byte() == 0 && comment.starts_with(b"#!"))
        || shellcheck_directive_with_budget(comment, budget)?)
}

/// What shdoc reads in the rows of a run, its directive rows left out: the
/// class of the first row, whether a row is a tag line, and whether one
/// is a file tag line.
#[derive(Default)]
struct RunRows {
    opening: Option<ShdocRow>,
    tagged: bool,
    file: bool,
}

fn run_rows<B: WorkBudget>(run: &RunContext<'_, '_>, budget: &B) -> Result<RunRows, B::Error> {
    let mut rows = RunRows::default();
    for member in run.members {
        budget.step()?;
        let comment = node_bytes(member, run.code);
        if !directive_with_budget(member, comment, budget)? {
            let row = shdoc_row_with_budget(comment, budget)?;
            rows.opening.get_or_insert(row);
            rows.tagged |= row != ShdocRow::Text;
            rows.file |= row == ShdocRow::FileTag;
        }
    }
    Ok(rows)
}

/// What the run `run` is, read as shdoc reads its rows: nothing, the
/// documentation of the function directly below it, or a document whose
/// owner this profile does not decide yet. Whether its rows and the
/// function stand where Bash reads them is `Reading::confirm`'s question.
pub(crate) fn opener_with_budget<B: WorkBudget>(
    run: &RunContext<'_, '_>,
    budget: &B,
) -> Result<Option<OpenerReason>, B::Error> {
    let rows = run_rows(run, budget)?;
    if !rows.tagged {
        return Ok(None);
    }
    if rows.file || rows.opening == Some(ShdocRow::Text) {
        return Ok(Some(OpenerReason::Explicit));
    }
    Ok(match &run.below {
        Some(below) if target(below) => Some(OpenerReason::FollowingItem),
        // A node with a syntax error may be a function the grammar could
        // not read (`f() for …`): the document stays, without an owner.
        Some(below) if below.has_error() || below.is_missing() => Some(OpenerReason::Explicit),
        _ => None,
    })
}

/// What the walk of the collector reads of a Bash source. The scan runs
/// with the walk: before the walk reads a node, the scan has read every
/// byte before it, and it says what Bash reads at the node's first byte.
/// The walk visits nodes in source order, so the scan reads each byte once.
pub(crate) struct Reading<'s> {
    scan: Option<Scan<'s>>,
    /// What the scan reads at the first byte of the node the walk entered.
    here: Place,
    /// The comment nodes entered since the last node of another kind, by
    /// first byte: the members of the run that the last of them ends.
    rows: Vec<(usize, Place)>,
}

impl Default for Reading<'_> {
    fn default() -> Self {
        Self {
            scan: None,
            here: Place::Unknown,
            rows: Vec::new(),
        }
    }
}

impl<'s> Reading<'s> {
    /// The scan of `source`, at its first byte, over its included bytes.
    fn open<B: WorkBudget>(
        source: &SourceContext<'s>,
        budget: &B,
    ) -> Result<Scan<'s>, SourceError> {
        let code = source.bytes();
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
        match source.pieces_with_budget(0..code.len(), budget) {
            Ok(included) => {
                for piece in included.pieces() {
                    budget.step().map_err(Into::into)?;
                    match ranges.last_mut() {
                        Some(last) if last.end == piece.range().start => {
                            last.end = piece.range().end;
                        }
                        _ => ranges.push(piece.range()),
                    }
                }
            }
            Err(SourceError::Cancelled) => return Err(SourceError::Cancelled),
            // An empty source, or one without an included byte.
            Err(_) => {}
        }
        Ok(Scan::new(code, ranges))
    }

    /// Takes the next node of the walk, in source order. The first call
    /// starts the scan of `source`.
    pub(crate) fn enter<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'s>,
        node: &Node<'_>,
        comment: bool,
        budget: &B,
    ) -> Result<(), SourceError> {
        let scan = match &mut self.scan {
            Some(scan) => scan,
            unopened => unopened.insert(Self::open(source, budget)?),
        };
        let start = node.start_byte();
        if comment {
            scan.read(start, budget).map_err(Into::into)?;
        } else {
            scan.advance(start, budget).map_err(Into::into)?;
        }
        if node.is_error() || node.is_missing() {
            // A recovery node may stand at the opener of a literal.
            scan.stop_at(start);
        }
        self.here = scan.place(start);
        if comment {
            // The node is a row only where the scan reads the same comment.
            if matches!(self.here, Place::Comment { end, .. } if end != node.end_byte()) {
                self.here = Place::Text;
            }
            self.rows.push((start, self.here));
        } else {
            self.rows.clear();
        }
        Ok(())
    }

    /// Leaves a node. The answer is why the end of a function is not
    /// confirmed: at the byte where the grammar ends it, the scan stands
    /// inside a string, a text or a form, or it stopped before that byte.
    pub(crate) fn leave<B: WorkBudget>(
        &mut self,
        node: &Node<'_>,
        budget: &B,
    ) -> Result<Option<UnresolvedReason>, B::Error> {
        let Some(scan) = self.scan.as_mut() else {
            return Ok(None);
        };
        scan.advance(node.end_byte(), budget)?;
        let open = target(node) && scan.inside(node.end_byte());
        Ok(open.then(|| scan.stopped().unwrap_or(UnresolvedReason::GrammarBoundary)))
    }

    /// Whether the node the walk entered stands inside a comment, a quoted
    /// string or the text of a here-document: it is not read.
    pub(crate) fn text(&self) -> bool {
        self.here == Place::Text
    }

    /// Whether the comment node the walk entered is no comment for Bash.
    pub(crate) fn unread(&self) -> bool {
        matches!(self.here, Place::Text | Place::Form | Place::Code { .. })
    }

    /// Whether the node the walk entered stands at or behind the place
    /// where the scan stopped.
    pub(crate) fn withheld(&self) -> bool {
        matches!(self.here, Place::Behind(_))
    }

    /// Whether a function the walk entered waits for a later decision: it
    /// stands in a substitution or another form that the scan read on its
    /// row, in back quotes, behind the place where the scan stopped, or
    /// where Bash starts no word.
    pub(crate) fn waits(&self) -> bool {
        matches!(
            self.here,
            Place::Form | Place::Unknown | Place::Behind(_) | Place::Code { word: false, .. }
        )
    }

    /// What the scan read at the comment node that starts at `start`.
    fn row<B: WorkBudget>(&self, start: usize, budget: &B) -> Result<Place, B::Error> {
        let at = crate::documentation::budget::binary_search_end(
            self.rows.len(),
            |at| self.rows[at].0 < start,
            budget,
        )?;
        Ok(match self.rows.get(at) {
            Some((found, place)) if *found == start => *place,
            _ => Place::Text,
        })
    }

    /// Keeps the members of `run` that are comments for Bash. The answer
    /// is whether a row of the document does not open a line of its own
    /// or stands behind excluded bytes; `None`: a member among the rows of
    /// the document is no comment for Bash.
    fn rows<B: WorkBudget>(
        &self,
        run: &mut SourceRun<'s>,
        budget: &B,
    ) -> Result<Option<bool>, B::Error> {
        let (mut started, mut loose) = (false, false);
        let mut members = Vec::new();
        for member in std::mem::take(&mut run.members) {
            budget.step()?;
            started |= member.selected;
            match self.row(member.node.node().start_byte(), budget)? {
                Place::Comment { line, .. } => loose |= member.selected && !line,
                Place::Unknown => loose |= member.selected,
                _ if started => return Ok(None),
                _ => continue,
            }
            members.push(member);
        }
        run.members = members;
        Ok(Some(loose))
    }

    /// Whether Bash starts a command line at `start`. Behind the place
    /// where the scan stopped the answer is yes: there the function's own
    /// qualification says that its reading is not confirmed.
    fn opens_line<B: WorkBudget>(&mut self, start: usize, budget: &B) -> Result<bool, B::Error> {
        let Some(scan) = self.scan.as_mut() else {
            return Ok(true);
        };
        scan.advance(start, budget)?;
        let (word, line) = (true, true);
        let place = scan.place(start);
        Ok(matches!(place, Place::Behind(_)) || place == Place::Code { word, line })
    }

    /// Confirms the run that the shared path decided at the comment node
    /// the walk entered. A member that is no comment for Bash is left
    /// out; if it stands among the rows of the document, the run is not
    /// read. If a row of the document or the function does not open a
    /// line of its own, or stands behind excluded bytes, the run is not
    /// selected.
    pub(crate) fn confirm<B: WorkBudget>(
        &mut self,
        mut run: SourceRun<'s>,
        retain_unbound: bool,
        budget: &B,
    ) -> Result<Option<SourceRun<'s>>, B::Error> {
        if self.scan.is_none() {
            return Ok(Some(run));
        }
        let Some(mut loose) = self.rows(&mut run, budget)? else {
            return Ok(None);
        };
        if let (RunSelection::Opener(OpenerReason::FollowingItem), Some(function)) =
            (run.selection, run.target)
        {
            loose |= !self.opens_line(function.node().start_byte(), budget)?;
        }
        if loose {
            if !retain_unbound {
                return Ok(None);
            }
            // Without a selected member the run gives no document; its
            // rows are candidates.
            for member in &mut run.members {
                member.selected = false;
            }
        }
        Ok(Some(run))
    }
}
