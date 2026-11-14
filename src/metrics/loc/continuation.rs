//! What each bare `\`-continuation row in a C-family file is, read once
//! per walk in source order (#1818).
//!
//! `GapScan` (`shared.rs`) finds the bare rows in a node's gaps and
//! credits each to the innermost space; this module answers *what* a
//! row is for the four grammars that splice a comment row's trailing
//! `\` (translation phase 2). The question has one answer per logical
//! line — the rows joined by `\` + newline — and the scan cannot read
//! it: a node's gaps are scanned before its descendants are visited, so
//! a flag carried through the walk sees a parent's trailing rows before
//! a child's tokens, and the tokens of the logical line a bare row
//! continues may sit in nodes the scan never holds. The bounded lookup
//! it used instead refused a row that itself continued an earlier one,
//! and ran out of its three-child window on a row of four comments, so
//! `/* a */ \` + `/* b */ \` + `\` + an empty row read the bare row as
//! code where `gcc -E -C -P` keeps the chain as one comment line.
//!
//! The index reads the file forward once. A row's class is settled when
//! its logical line ends, from what the line held before that: code
//! anywhere makes every pending bare row code; comments alone make it a
//! comment; nothing makes it blank. Comment text is what the grammar's
//! comment nodes cover, never a `/*` inside a string or an unexpanded
//! macro body, which are code bytes to the count as to the arms. A
//! newline *inside* a comment ends the line for this purpose whether
//! or not a `\` precedes it: the bare row before `/* a` + `b */ int z;`
//! joins the comment's first physical row only, which is the policy
//! `opens_comment_only_line` (`shared.rs`) keeps for the row a bare run
//! joins ("a comment running past its row is the rest of that row"), and
//! what a bare row after `y */ \` joins is that row alone, with the
//! code before the comment's newline forgotten.
//!
//! One pass over the bytes and one cursor walk over the tree, each
//! linear; one byte of class per physical row.

use super::shared::{RowShape, is_horizontal_space};
use super::*;

// Indexes built, for the test that pins one build per walk: the output
// cannot tell one from a build per node, which would be the walk's
// node count times the file.
crate::observation::counter!(bare_row_indexes_built);

/// The class of every bare row in one file, `None` for a row that is
/// not bare. `Option<BareRow>` has a niche, so this is one byte per row.
pub(super) struct BareRowIndex {
    rows: Vec<Option<BareRow>>,
}

impl BareRowIndex {
    /// Reads `code`, taking comment text from the comment nodes under
    /// `root` (the kinds `is_comment` lists).
    pub(super) fn build(root: &Node<'_>, code: &[u8], is_comment: fn(u16) -> bool) -> Self {
        bare_row_indexes_built::record();
        let mut index = Indexer::default();
        let mut comments = comment_spans(root, is_comment).into_iter().peekable();
        for (at, &byte) in code.iter().enumerate() {
            while comments.peek().is_some_and(|&(_, end)| end <= at) {
                comments.next();
            }
            let commented = comments.peek().is_some_and(|&(start, _)| start <= at);
            index.read(code, at, byte, commented);
        }
        // The last row, with no newline: a bare `\` at EOF is the one
        // row the scan credits without one.
        if index.row_start < code.len() {
            index.end_row();
        }
        index.settle();
        Self { rows: index.rows }
    }

    /// What bare `row` is; `None` for a row that is not bare, which the
    /// scan never asks about.
    pub(super) fn class(&self, row: usize) -> Option<BareRow> {
        self.rows.get(row).copied().flatten()
    }
}

// The forward read: the rows classified so far, the bare rows of the
// current logical line awaiting its end, what that line has held, and
// where the current physical row began and what it holds.
#[derive(Default)]
struct Indexer {
    rows: Vec<Option<BareRow>>,
    pending: Vec<usize>,
    code_seen: bool,
    comment_seen: bool,
    row_start: usize,
    shape: RowShape,
}

impl Indexer {
    // Reads the byte at `at`, `commented` when a comment node covers it.
    fn read(&mut self, code: &[u8], at: usize, byte: u8, commented: bool) {
        if byte == b'\n' {
            self.end_row();
            // The `\` before this newline is at `at - 1`, or at `at - 2`
            // past a CR; a comment holding it holds the newline too, so
            // `commented` answers for both. A newline inside a comment
            // ends the line whether or not a `\` precedes it; the
            // comment's next byte marks the line it runs on into.
            if commented || !splices(code, self.row_start, at) {
                self.settle();
            }
            self.row_start = at.saturating_add(1);
            self.shape = RowShape::Blank;
            return;
        }
        self.shape = self.shape.step(byte);
        if commented {
            self.comment_seen = true;
        } else if is_code(code, at, byte) {
            self.code_seen = true;
        }
    }

    // Closes the physical row that ends here: not bare until its
    // logical line settles it, bare ones pending until then.
    fn end_row(&mut self) {
        let row = self.rows.len();
        self.rows.push(None);
        if self.shape.is_bare() {
            self.pending.push(row);
        }
    }

    // Ends the logical line: every pending bare row becomes what the
    // line held.
    fn settle(&mut self) {
        let class = if self.code_seen {
            BareRow::Code
        } else if self.comment_seen {
            BareRow::Comment
        } else {
            BareRow::Blank
        };
        for row in self.pending.drain(..) {
            if let Some(slot) = self.rows.get_mut(row) {
                *slot = Some(class);
            }
        }
        self.code_seen = false;
        self.comment_seen = false;
    }
}

// Whether `byte`, at `at` and outside any comment, is code: anything
// but the padding the grammars skip, a `\r`, and the one `\` a newline
// splices away. A `\` that is not that — mid-row, or the first of an
// even run before a newline — is a stray token, which is code.
fn is_code(code: &[u8], at: usize, byte: u8) -> bool {
    if byte == b'\\' {
        return !matches!(
            code.get(at.saturating_add(1)..).unwrap_or_default(),
            [b'\n', ..] | [b'\r', b'\n', ..]
        );
    }
    !is_horizontal_space(byte) && byte != b'\r'
}

// Whether the newline at `newline` is spliced: the row's last byte
// before it (past a `\r`) is a `\`. Phase 2 removes exactly that one
// backslash, so an even run `\\` + newline splices too, leaving a
// stray `\` that is code — which `is_code` already counted.
fn splices(code: &[u8], row_start: usize, newline: usize) -> bool {
    let row = code.get(row_start..newline).unwrap_or_default();
    let row = row.strip_suffix(b"\r").unwrap_or(row);
    row.last() == Some(&b'\\')
}

// The byte spans of the comment nodes under `root`, in source order.
// A cursor walk rather than `descendant` lookups: each node is visited
// once, so the collection is linear in the tree, under a flat `ERROR`
// included.
fn comment_spans(root: &Node<'_>, is_comment: fn(u16) -> bool) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut cursor = root.cursor();
    loop {
        let node = cursor.node();
        let comment = is_comment(node.kind_id());
        if comment {
            spans.push((node.start_byte(), node.end_byte()));
        }
        // A comment's children, where a grammar gives it any, are its
        // own text; nothing below one is a second comment.
        if !comment && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return spans;
            }
        }
    }
}
