//! Places where perl may read a line as literal text and the grammar reads
//! a POD block: after a syntax error, in the body of a `format`, beside a
//! here-document, and after a quote-like literal that perl reads
//! differently.
//!
//! Where a construct starts and ends is a claim of the grammar. The rules
//! here read the bytes by perl's own rule and compare: a literal in
//! `pod_literal`, a format in `pod_format`, and here what opens a
//! here-document.
//!
//! The walk hands every node to [`Doubt::enter`], in tree order. The state
//! only moves forward: once every later block is doubtful, nothing more is
//! read, and a format's body is scanned once, whatever it holds.

use std::ops::Range;

use super::pod_format::{format_body, format_end};
use super::pod_literal::{Quote, Reading, operator};
use crate::documentation::SourceContext;
use crate::documentation::budget::WorkBudget;
use crate::documentation::byte_scan as scan;

const RECOVERY: &str = "POD block follows a syntax error, where perl may read literal text";
const FORMAT: &str = "POD block lies in the body of a format, or after one whose end is not known";
const UNREAD_OPENER: &str =
    "POD block follows a here-document opener that the grammar did not read";
const HEREDOC: &str = "POD block lies beside a here-document that perl may read differently";
const LITERAL: &str = "POD block follows a quote-like literal that perl may read differently";

/// What the walk has entered so far that makes a later block doubtful.
#[derive(Debug, Default)]
pub(super) struct Doubt {
    /// Why every later block is doubtful: the first such place in the tree.
    settled: Option<&'static str>,
    /// The last format, from its keyword to the end of its last line.
    format: Range<usize>,
    /// The `.` that ends that format, until the walk enters a node there.
    last_line: Option<usize>,
    /// The `<<` of the last opener that the grammar did read.
    read_opener: Option<usize>,
    /// Openers whose body the walk has not entered.
    open_bodies: usize,
    /// The row on which the next body must start.
    body_row: usize,
}

impl Doubt {
    /// Why a block that starts at `start` is not read.
    pub(super) fn reason(&self, start: usize) -> Option<&'static str> {
        if self.settled.is_some() {
            self.settled
        } else if start < self.format.end {
            Some(FORMAT)
        } else {
            (self.open_bodies > 0).then_some(HEREDOC)
        }
    }

    pub(super) fn enter<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'_>,
        node: tree_sitter::Node<'_>,
        budget: &B,
    ) -> Result<(), B::Error> {
        let start = node.start_byte();
        if self.settled.is_some() {
            // Every later block is withheld: nothing more is read.
        } else if node.is_error() || node.is_missing() {
            self.settled = Some(RECOVERY);
        } else if start < self.format.end {
            // Picture text: the grammar's reading of it says nothing, except
            // whether a node starts at the dot of the format's last line.
            if self.last_line == Some(start) {
                self.last_line = None;
            }
        } else if self.last_line.take().is_some() {
            self.settled = Some(FORMAT);
        } else {
            self.settled = self.code(source, node, budget)?;
        }
        Ok(())
    }

    /// A node outside every format. Returns why later blocks are doubtful.
    fn code<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'_>,
        node: tree_sitter::Node<'_>,
        budget: &B,
    ) -> Result<Option<&'static str>, B::Error> {
        let spelling = node.start_byte()..node.end_byte();
        match Quote::of(node.kind()) {
            Some(quote) => Ok(match quote.reading(source, spelling, budget)? {
                Reading::Same => None,
                Reading::Opener => Some(UNREAD_OPENER),
                Reading::Different => Some(LITERAL),
            }),
            None if node.kind() == "identifier" => self.word(source, spelling, budget),
            None => self.heredoc(source, node, budget),
        }
    }

    /// An identifier that perl may read as a keyword: `format`, or a
    /// quote-like operator behind `CORE::`, which the grammar reads as a
    /// name. An identifier with an excluded byte is not read and counts as
    /// the keyword it may be.
    fn word<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'_>,
        word: Range<usize>,
        budget: &B,
    ) -> Result<Option<&'static str>, B::Error> {
        if word.len() >= 6 {
            self.format_keyword(source, word, budget)?;
        } else if word.len() <= 2 && word.start >= 6 {
            return Ok(core_operator(source, word, budget)?.then_some(LITERAL));
        }
        Ok(None)
    }

    /// An identifier that may be the keyword `format`: where the bytes
    /// behind it open a format, its body is read to the line that ends it.
    fn format_keyword<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'_>,
        word: Range<usize>,
        budget: &B,
    ) -> Result<(), B::Error> {
        let code = source.bytes();
        let read = source.covers_with_budget(word.clone(), budget)?;
        if (!read || code[word.clone()] == *b"format")
            && let Some(body) = format_start(source, word.end, budget)?
        {
            let last = format_end(&code[body..], budget)?;
            self.last_line = last.as_ref().map(|line| body + line.start);
            self.format = word.start..last.map_or(code.len(), |line| body + line.end);
        }
        Ok(())
    }

    /// The nodes of a here-document. Returns why later blocks are doubtful.
    fn heredoc<B: WorkBudget>(
        &mut self,
        source: &SourceContext<'_>,
        node: tree_sitter::Node<'_>,
        budget: &B,
    ) -> Result<Option<&'static str>, B::Error> {
        let (start, end) = (node.start_byte(), node.end_byte());
        let misread = match node.kind() {
            "heredoc_initializer" => {
                self.read_opener = Some(start);
                self.open_bodies += 1;
                self.body_row = node.end_position().row;
                false
            }
            "<<" if self.read_opener != Some(start) => {
                return Ok(opens_heredoc(source, end, budget)?.then_some(UNREAD_OPENER));
            }
            "heredoc_body_statement" => {
                let late = node.start_position().row != self.body_row;
                self.open_bodies = self.open_bodies.saturating_sub(1);
                self.body_row = node.end_position().row;
                late
            }
            "heredoc_end_identifier" => {
                let code = source.bytes();
                let after = end..code.len().min(end + 2);
                !source.covers_with_budget(after.clone(), budget)?
                    || !matches!(code[after], [b'\n', ..] | [b'\r', b'\n'])
            }
            _ => false,
        };
        Ok(misread.then_some(HEREDOC))
    }
}

/// Whether the `<<` operator that ends at `shift`, and that the grammar
/// reads as a shift, may open a here-document for perl: `~`, `\`, a letter
/// or `_` stands directly behind it; or a quote, directly or behind blanks
/// and tabs; or a digit where white space stands before the operator, as
/// in `print $fh <<1`.
fn opens_heredoc<B: WorkBudget>(
    source: &SourceContext<'_>,
    shift: usize,
    budget: &B,
) -> Result<bool, B::Error> {
    let code = source.bytes();
    let read = shift.saturating_sub(3)..code.len().min(shift + 1);
    if !source.covers_with_budget(read.clone(), budget)? {
        return Ok(true);
    }
    Ok(match code[read] {
        [.., b'"' | b'\'' | b'`' | b'~' | b'\\' | b'_'] => true,
        [.., tag] if tag.is_ascii_alphabetic() => true,
        [.., b' ' | b'\t'] => quoted_tag(source, shift + 1, budget)?,
        [before, _, _, tag] => before.is_ascii_whitespace() && tag.is_ascii_digit(),
        _ => false,
    })
}

/// Whether a quote stands at `from` or behind the blanks and tabs there.
/// With an excluded byte behind `from` nothing is read and the worst case
/// is taken.
fn quoted_tag<B: WorkBudget>(
    source: &SourceContext<'_>,
    from: usize,
    budget: &B,
) -> Result<bool, B::Error> {
    let code = source.bytes();
    if !source.covers_with_budget(from..code.len(), budget)? {
        return Ok(true);
    }
    let rest = &code[from..];
    let tag = scan::position(rest, |byte| !matches!(byte, b' ' | b'\t'), budget)?;
    Ok(tag.is_some_and(|at| matches!(rest[at], b'"' | b'\'' | b'`')))
}

/// Whether the identifier `word`, of one or two bytes, spells a quote-like
/// operator directly behind `CORE::`: the operator for perl, a name for the
/// grammar. The text of that literal is code for the grammar.
fn core_operator<B: WorkBudget>(
    source: &SourceContext<'_>,
    word: Range<usize>,
    budget: &B,
) -> Result<bool, B::Error> {
    let code = source.bytes();
    let read = word.start - 6..word.end;
    Ok(!source.covers_with_budget(read.clone(), budget)?
        || (code[read].starts_with(b"CORE::") && operator(&code[word])))
}

/// Where the body of the format starts whose keyword ends at `keyword`, if
/// the bytes behind it open one. With an excluded byte behind the keyword
/// nothing is read and the worst case is taken: a body without a last line.
fn format_start<B: WorkBudget>(
    source: &SourceContext<'_>,
    keyword: usize,
    budget: &B,
) -> Result<Option<usize>, B::Error> {
    let code = source.bytes();
    Ok(if source.covers_with_budget(keyword..code.len(), budget)? {
        format_body(&code[keyword..], budget)?.map(|body| keyword + body)
    } else {
        Some(code.len())
    })
}
