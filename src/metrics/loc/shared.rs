//! Cross-language `Loc` helper functions shared by the per-language
//! submodules.
//!
//! The public metric `Stats` (and `Sloc` / `Ploc` / `Cloc` / `Lloc`),
//! the `Loc` trait, and `min_or_zero` stay in the parent module; only
//! the cross-language line-counting helpers live here so the parent
//! clears the 800-SLOC self-scan limit (#976).
//!
//! # The `Loc` catch-all arm is fail-*open*, unlike every sibling metric
//!
//! `Cognitive`, `Cyclomatic`, and `Abc` end their per-language matches
//! with `_ => {}`, so a token the author never considered costs
//! nothing. Every `Loc` implementation instead ends with
//!
//! ```ignore
//! _ => {
//!     check_comment_ends_on_code_line(stats, start);
//!     stats.ploc.lines.insert(start);
//! }
//! ```
//!
//! which asserts *this row is code*. The default is inverted from the
//! one the neighbouring metric modules train you to expect: an
//! unrecognised token does not fall through harmlessly, it adds a
//! physical line of code — and via [`check_comment_ends_on_code_line`]
//! can reclassify a comment-only row as code-and-comment.
//!
//! Two shipped defects came from exactly this. Tcl and iRules are the
//! only grammars that surface the row terminator as a token child of
//! the root; its start row is the row it *terminates*, so every
//! comment-only and whitespace-only row was credited to PLOC — a
//! realistic fourteen-row Tcl file reported `ploc 13` against a true
//! `7` (#1135). Perl's `#` sits *inside* its `comments` node and did
//! the same to every Perl file carrying a comment row (#1137).
//!
//! When adding a language, enumerate the tokens its grammar emits that
//! are not code — row terminators, comment punctuation, string-internal
//! tokens — and give them an explicit no-op arm. `bca dump` over a
//! small fixture is the fastest way to see them. The parent module's
//! `a_comment_row_is_never_counted_as_code` and
//! `whitespace_only_input_is_uniform_across_grammars` sweep every
//! language for both shapes.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::{
    bound_doc_run, documents_unbound, documents_where_it_sits, leading_doc_run,
};
use crate::node::Cursor;

#[inline]
pub(crate) fn init(node: &Node, stats: &mut Stats, is_func_space: bool) -> usize {
    let start = node.start_row();

    if is_func_space {
        stats.sloc.start = start;
        // `Node::end_line`, not the raw end row, and not a unit/non-unit
        // flag: the end column decides whether the final row belongs to
        // this span, and that rule lives in one place (#1067, #1163).
        stats.sloc.end_line = node.end_line();
    }
    // The span written above is the *measured* one, and for the file-level
    // unit that is wrong whenever the file opens with blank lines — the
    // root node starts at the first token, not at line 1. Nothing is done
    // about it here because this helper cannot tell a unit from any other
    // func-space without re-widening the `Loc` trait, which #1067 removed
    // for perf. `spaces::compute::anchor_unit_sloc_span` overwrites the
    // unit's span at finalization instead (#1247).
    start
}

#[inline]
// Discriminates among the comments that are *after* a code line and
// the ones that are on an independent line.
// This difference is necessary in order to avoid having
// a wrong count for the blank metric.
//
// Requires `end >= start`. Every caller passes the row span returned by
// `init`, and tree-sitter guarantees a node's end row is never before its
// start row — so the only way to violate this is for a caller to adjust
// `end` downwards itself, as Rust's `LineComment` arm does for doc
// comments (#1051).
//
// Scope of the assert, deliberately narrow: it fires only for
// `end < start`, and only in debug — the release profile disables both
// `debug-assertions` and `overflow-checks`. It cannot catch a caller
// whose own arithmetic already wrapped, because `(0, usize::MAX)`
// satisfies `end >= start`. The real guard against that is refusing to
// underflow in the first place, at the site that adjusts the span.
//
// The `saturating_sub` below narrows what release does with an
// inverted span, and the effect is smaller than it looks: `comment_diff`
// feeds only the branch tests, never a row index. `0` routes to the
// `== 0` arm rather than the block-comment arm a wrapped `usize::MAX`
// selected, but the two differ only by an `add_only_comment_lines(start
// + 1, end)` that `LineSet::insert_range` already rejects as inverted —
// so this buys a truthful classification, not an observable metric
// change (#1152).
fn add_cloc_lines(stats: &mut Stats, start: usize, end: usize) {
    debug_assert!(end >= start, "add_cloc_lines: end {end} < start {start}");
    let comment_diff = end.saturating_sub(start);
    let is_comment_after_code_line = stats.ploc.lines.contains(start);
    if is_comment_after_code_line && comment_diff == 0 {
        // A comment is *entirely* next to a code line
        add_code_comment_line(stats, start);
    } else if is_comment_after_code_line && comment_diff > 0 {
        // A block comment that starts next to a code line and ends on
        // independent lines.
        add_code_comment_line(stats, start);
        add_only_comment_lines(stats, start.saturating_add(1), end);
    } else {
        // A comment on an independent line AND
        // a block comment on independent lines OR
        // a comment *before* a code line.
        //
        // Insert each spanned row into the comment-only set rather than
        // bumping a counter: a multi-line block still contributes one
        // line per distinct row, but two standalone comments on a
        // single physical line (`/*a*/ /*b*/`) share the row and count
        // once (issue #461 follow-up).
        add_only_comment_lines(stats, start, end);
        // Save line end of a comment to check whether
        // a comment *before* a code line is considered
        stats.cloc.comment_line_end = Some(end);
    }
}

#[inline]
// Records a span of comment rows exactly as `add_cloc_lines` does and,
// when they are documentation, the same rows in the `dloc` set that
// `loc.dloc` reports, so `dloc() <= cloc()` holds by construction
// (#1597). The callers are `add_comment_lines` below, Rust's
// line-comment arm and Python's bare-string arms. The `dloc` set's two
// other writers, `add_leading_doc_run` and Python's documentation-string
// and Sphinx-comment runs, only re-bill rows already billed here.
pub(crate) fn add_comment_rows(stats: &mut Stats, start: usize, end: usize, documentation: bool) {
    add_cloc_lines(stats, start, end);
    if documentation {
        stats.dloc.lines.insert_range(start, end);
    }
}

#[inline]
// Documentation by position rather than by marker (Go, Ruby, Lua): bills
// the comment run `node` ends as `dloc` when it sits directly above a
// declaration the language documents, or its first member is a language's
// run opener (LDoc `---`, RDoc `##`, YARD `@!`). Called
// from the comment arm after `add_comment_lines`, so every row it credits
// is one a run member's own arm already billed to this space's cloc sets,
// and `dloc() <= cloc()` still holds (#1597).
pub(crate) fn add_leading_doc_run<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    leading_doc_run::<T>(node, code, ancestors, |first, last| {
        stats.dloc.lines.insert_range(first, last);
    });
}

#[inline]
// Bills a comment node's rows, as documentation when `is_documentation`
// says so. Every language's comment arm routes here — Rust's comment
// arm bills through `add_comment_rows` itself, since its
// `Checker::is_doc_comment` answer also sizes its row span — so "which
// comments are documentation" is answered in one place per language, in
// the `Checker` impl, and never drifts from `is_comment` (#1597).
//
// The rows are the comment node's own, exactly as `init` hands them to
// every comment arm; reading them here keeps each call site to the node
// and its context.
pub(crate) fn add_comment_lines<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    let documentation = is_documentation::<T>(node, code, ancestors);
    add_comment_rows(stats, node.start_row(), node.end_row(), documentation);
    add_bound_doc_run::<T>(node, code, ancestors, stats);
}

// Whether a comment is documentation by where it sits: spelled as a doc
// comment (`Checker::is_doc_comment`), not one a `Checker::DOC_BINDING`
// attaches forward (`documents_unbound`; `add_bound_doc_run` bills
// those), and, per `documents_where_it_sits`, not a working note in a
// function body (#1597, #1613).
fn is_documentation<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    T::is_doc_comment(node, code) && documents_by_placement::<T>(node, code, ancestors)
}

// The placement half of `is_documentation`, for a caller that already
// holds the `Checker::is_doc_comment` answer (Rust's comment arm).
pub(crate) fn documents_by_placement<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    documents_unbound::<T>(node, code, ancestors)
        && documents_where_it_sits::<T>(node, code, ancestors)
}

#[inline]
// Bills as `dloc` the doc comments a bound language's tool attaches to
// the declaration after them, once `node` ends their stretch
// (`bound_doc_run`, #1613). Each member's rows are the ones its own
// comment arm billed to `cloc`, so `dloc() <= cloc()` still holds; every
// member is a sibling of `node`, so they were billed to this space's
// `stats`. Rust's arm, whose `///` rows are not its node's, calls
// `bound_doc_run` itself.
fn add_bound_doc_run<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    bound_doc_run::<T>(node, code, ancestors, |member| {
        stats
            .dloc
            .lines
            .insert_range(member.start_row(), member.end_row());
    });
}

#[inline]
// Detects the comments that are on a code line but *before* the code part.
// This difference is necessary in order to avoid having
// a wrong count for the blank metric.
pub(crate) fn check_comment_ends_on_code_line(stats: &mut Stats, start_code_line: usize) {
    if let Some(end) = stats.cloc.comment_line_end
        && end == start_code_line
        && !stats.ploc.lines.contains(start_code_line)
    {
        // Comment entirely *before* a code line: reclassify the line
        // from comment-only to code-comment. Remove it from the
        // comment-only set (so `blank` no longer credits it) and add it
        // to the code-comment set. Both operations are idempotent, so a
        // line already reclassified stays correct (issue #461).
        stats.cloc.only_comment_line_starts.remove(start_code_line);
        add_code_comment_line(stats, start_code_line);
    }
}

#[inline]
// Records a physical line carrying both code and comment. Backed by a
// per-line set so several inline block comments on one code line
// (`f(int /*a*/, int /*b*/)`) yield a single comment line, not one per
// comment node (issue #461). Mirrors `Ploc`'s per-line de-duplication.
pub(crate) fn add_code_comment_line(stats: &mut Stats, line: usize) {
    stats.cloc.code_comment_line_starts.insert(line);
}

#[inline]
// Records the inclusive row range `start..=end` as comment-only lines.
// Backed by a per-line set so two standalone block comments on one
// physical line (`/*a*/ /*b*/`) count once, while each distinct row of
// a genuine multi-line block comment still counts (issue #461).
pub(crate) fn add_only_comment_lines(stats: &mut Stats, start: usize, end: usize) {
    stats.cloc.only_comment_line_starts.insert_range(start, end);
}

// Adds every physical row spanned by a multi-line string literal to PLOC.
//
// Interior rows of a multi-line string hold real source text, not blank
// lines: classifying them as blank (which happens whenever a language
// no-ops its string nodes, so the rows reach neither PLOC nor CLOC and
// `blank = sloc - ploc - cloc` mislabels them) diverges from Python's
// established behaviour, where a non-docstring multi-line string credits
// all of its rows to PLOC (#415). This helper makes every other language
// agree with that decision (#778).
//
// Mirrors Python's `String` arm: the opening row is inserted only when the
// enclosing statement begins on an earlier row — otherwise that row is
// already attributed to the parent — and the interior and closing rows are
// always inserted.
//
// **That parent gate assumes some other node credits the opening row.**
// It is a skip, and it is only safe where the language's catch-all
// credits every node's start row, as it does in all eighteen languages
// #778 covered. Where the catch-all is leaf-gated
// (`if node.child_count() == 0`, as in `bash.rs` and `elixir.rs`) a
// container parent contributes nothing, so a *childless* literal is the
// only node covering its own row and the gate deletes it: `'ls'` alone
// in a Bash file reported `ploc 0, blank 1` (#1260). Elixir is
// unaffected because a `quoted_content`'s parent always carries its
// opening delimiter as a leaf token on that row. Bash's arm owns its
// opening row outright and calls
// [`add_string_interior_ploc`] instead — do not reach for this one
// there, where the gate can only be wrong.
#[inline]
pub(crate) fn add_multiline_string_ploc(
    node: &Node,
    ancestors: Ancestors<'_, '_>,
    stats: &mut Stats,
    start: usize,
) {
    if ancestors
        .parent(node)
        .is_none_or(|parent| parent.start_row() != start)
    {
        check_comment_ends_on_code_line(stats, start);
        stats.ploc.lines.insert(start);
    }
    add_string_interior_ploc(node, stats, start);
}

// Adds a multi-row literal's interior and closing rows to PLOC, leaving
// its opening row to the caller. The half of
// [`add_multiline_string_ploc`] a language whose catch-all is leaf-gated
// wants on its own: those callers must credit the opening row
// unconditionally, so the parent gate above is dead weight for them and
// re-running its two statements after the fact reads as a second rule.
//
// The last row is derived from [`Node::end_line`] rather than the raw
// end row: a node whose end column is 0 finished at the *start* of the
// row below its last content row, so that row is not part of its span,
// and crediting it claims a row the literal does not cover.
//
// `insert_range` rather than a row-at-a-time loop: the range is one
// bitmap span, and Bash's heredoc bodies push thousands of rows through
// here where the loop paid a reserve and a bounds check per row.
//
// The name records where the rule came from, not the only node it
// applies to: #1423 routed the four C-family `PreprocArg` arms here
// too, since an unexpanded macro body is the same question — which rows
// does this multi-row node cover — and had the same off-by-one bound
// open-coded.
#[inline]
pub(crate) fn add_string_interior_ploc(node: &Node, stats: &mut Stats, start: usize) {
    // Inclusive, and `insert_range` no-ops on an inverted span, so a
    // single-row literal inserts nothing here.
    stats
        .ploc
        .lines
        .insert_range(start.saturating_add(1), node.end_line().saturating_sub(1));
}

// A node's start row, the bare row and its end row: a node spanning fewer
// rows has no complete row strictly between its children to find.
const ROWS_AROUND_A_GAP_ROW: usize = 2;

// Bare-row scans actually run, after every bail in
// `credit_bare_continuation_rows`: the one count that tells a file whose
// scan was skipped (#1821) from one whose scan credited nothing.
crate::observation::counter!(bare_row_scans);

// Whether `code` can hold a row `credit_bare_continuation_rows` would
// credit, so a file with none skips that scan at every node (#1821). The
// scan credits a row of the form `[ \t\v\f]* \ \r? \n`, and at EOF one
// with no newline, so every such row ends in `\` + newline, `\` + CRLF,
// or a `\` or `\` + CR that ends the file. Leading padding and the `\`'s
// position are not checked: a superset is safe, since the scan still
// decides each row, and a subset silently loses credits. `\` + space +
// newline is matched by none of the four, as the scan reads it as
// `Other`.
pub(crate) fn may_hold_bare_rows(code: &[u8]) -> bool {
    code.iter().enumerate().any(|(at, &byte)| {
        byte == b'\\'
            && matches!(
                code.get(at.saturating_add(1)..).unwrap_or_default(),
                [b'\n', ..] | [b'\r', b'\n', ..] | [] | [b'\r']
            )
    })
}

// Credits each `\`-continuation row that holds nothing but the backslash
// as the code it continues (#1445).
//
// Bash, Tcl and iRules splice `\` + newline away in the lexer — each
// grammar lists `/\\\r?\n/` among its extras — so a row spelled only `\`
// surfaces no node at all. Their `Loc` arms credit nodes, and a row no
// node covers reaches neither PLOC nor CLOC, so
// `blank = sloc - ploc - cloc` claimed it: `echo a | \` + `\` +
// `  grep b` reported `ploc 2, blank 1` for one three-row command.
//
// C, C++, Mozcpp, Objective-C, Ruby and Elixir splice it the same way,
// some inside a wider extras pattern, and call this too (#1801). That
// covers `#define`: a bare row between the name or parameters and the
// body is padding in the directive, before `preproc_arg` starts, not
// macro text the `PreprocArg` range credits, and so is one after a
// `/* … */` that ends a `preproc_arg`, where the grammar recovers with
// an `ERROR` around the rest of the body.
//
// A bare row that continues no row before it starts a logical line, and
// is what that line becomes once spliced: a comment where the rest of the
// line holds only comment text (`\` + `# note` is one comment line to bash
// and tclsh, `\` + `/* a */ /* b */` to cc), blank where the row it joins
// is empty or EOF (`\` + an empty row), and code otherwise, a C-family
// `/* c */ int z;` included. The joined row is the first after the run of
// bare rows, and since only padding precedes its first token, that token
// is the gap's next child.
//
// A bare row that continues a row is the code it continues, unless that
// row holds only a C-family `/* … */` before its `\`: the logical line
// then has no code yet and is what the rows after it make it, a comment
// where they add none.
//
// It reads only a node's *gaps*, the padding between its children and
// after the last of them. A bare row carries no token, so it lies in
// exactly one gap: that of the deepest node spanning it. That node's
// `stats` belongs to the innermost space containing the row, which is
// the space whose own `ploc` needs it, and every enclosing space receives
// it through `Stats::merge`. A per-space scan of each space's whole byte
// range would also be right and costs bytes × nesting depth, the class
// `loc/nested-fn-rows` exists to keep linear
// (`loc/bash-nested-continued-fn` measures it); each padding byte here
// sits in one node's gaps, so the scan is linear in the file. Iterating a
// node's children is the cost every node would pay, so a node too short
// to hold a gap row returns before it.
//
// The root alone also owns the padding before its first child, and its
// trailing padding runs on to EOF: its span begins at its first token, so
// a leading `\` + `echo a` has its bare row outside every node. Any other
// node's trailing padding ends with it and can still hold a bare row:
// Ruby's `then` and Elixir's stab-clause `body` run on past their last
// child to a hidden line break, as does a Bash `else` clause.
//
// `comments` names the grammar's comment row and whether a `\` ending a
// comment row continues the comment. `tclsh` reads `# c \` + `\` +
// `puts x` as one comment, but the Tcl and iRules `comment` token is
// `#[^\n]*` and stops at the row's end, so a bare row continuing one is
// comment text the comment node does not cover; it is credited to CLOC.
// C and its siblings continue a comment as well (translation phase 2),
// but their `//` token takes the continued rows itself, so no bare row
// after one is padding. Bash, Ruby and Elixir never continue a comment
// — `echo a # c \` + `\` + `echo b` runs `echo b` — so there a bare row
// after a comment starts a logical line.
//
// A grammar that keeps the `\` as a node applies the same rule per node
// instead: Python's `LineContinuation` arm (`python.rs`, #1819). A `\`
// the external scanner takes as padding before a string start has no
// node and never reaches this (#1825).
//
// A file with no row the scan could credit skips it at every node: the
// walk seeds `Stats::bare_rows_possible` from `may_hold_bare_rows` over
// the whole source once (#1821). The exit changes no output — `GapScan`
// writes `stats` only in `credit`, which runs only for a bare row — so
// `bare_row_scans` is what a test observes.
#[inline]
pub(crate) fn credit_bare_continuation_rows<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
    comments: BareRowComments,
) {
    if !stats.bare_rows_possible {
        return;
    }
    let parent = ancestors.parent(node);
    let is_root = parent.is_none();
    if !is_root
        && (node.child_count() == 0
            || node.end_row() < node.start_row().saturating_add(ROWS_AROUND_A_GAP_ROW))
    {
        return;
    }
    bare_row_scans::record();
    let mut scan = GapScan {
        node: node.as_tree_sitter(),
        parent: parent.map(|parent| parent.as_tree_sitter()),
        before: [None; CHILDREN_BEFORE_A_GAP],
        code,
        stats,
        comments,
        previous_bare: None,
    };
    scan.scan(node);
}

// The grammar's comment row, as its `lists_kind`, and whether a `\` ending
// a comment row carries the comment onto the next row (Tcl, iRules and
// the C family) or not (Bash, Ruby, Elixir, Python).
#[derive(Clone, Copy)]
pub(crate) struct BareRowComments {
    pub(crate) is_comment: fn(u16) -> bool,
    pub(crate) continued: bool,
}

impl BareRowComments {
    pub(super) fn lists(self, node: tree_sitter::Node<'_>) -> bool {
        (self.is_comment)(node.kind_id())
    }
}

// What a bare row is credited as.
#[derive(Clone, Copy)]
pub(super) enum BareRow {
    Code,
    Comment,
    Blank,
}

// What the row read so far holds, for the bare-row test: optional
// horizontal whitespace, one `\`, and an optional `\r` before the newline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowShape {
    Blank,
    Backslash,
    BackslashCr,
    Other,
}

impl RowShape {
    fn step(self, byte: u8) -> Self {
        match (self, byte) {
            (Self::Blank, b'\\') => Self::Backslash,
            (Self::Blank, _) if is_horizontal_space(byte) => Self::Blank,
            (Self::Backslash, b'\r') => Self::BackslashCr,
            _ => Self::Other,
        }
    }

    fn is_bare(self) -> bool {
        matches!(self, Self::Backslash | Self::BackslashCr)
    }
}

// The control characters every splicing grammar skips within a row
// alongside space and tab: the C family's and Ruby's `\s`, and Bash's and
// Tcl's, are ASCII and take both. Elixir skips neither, but lexes each as
// a token of its own, which no gap holds. bash itself does not skip them:
// its blanks are space and tab, so either is a word character. A row
// holding one before its `\` is code to bash as it is here. A bare row
// joining a row that one starts reads here as what the rest of that row
// makes it, but to bash it joins a command named by the control character,
// which no working script runs, so the difference is left alone.
const VERTICAL_TAB: u8 = 0x0b;
const FORM_FEED: u8 = 0x0c;

// Whether `byte` may indent a bare row, or stand between a row's last
// token and its newline, as the grammars skip it.
fn is_horizontal_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | VERTICAL_TAB | FORM_FEED)
}

// A CR is not skipped: the row-end patterns read it as the start of CRLF.
pub(super) fn past_horizontal_space(code: &[u8], at: usize) -> usize {
    let rest = code.get(at..).unwrap_or_default();
    at.saturating_add(
        rest.iter()
            .take_while(|&&byte| is_horizontal_space(byte))
            .count(),
    )
}

pub(super) fn at_row_end(code: &[u8], at: usize) -> bool {
    matches!(
        code.get(at..).unwrap_or_default(),
        [] | [b'\n', ..] | [b'\r', b'\n', ..]
    )
}

// The first byte at or after `at` that is neither horizontal space nor a
// `\` splice: where a logical line's next token or its end begins, once
// translation phase 2 (or the shell's lexer, or CPython's tokenizer) has
// joined its rows.
pub(super) fn past_line_padding(code: &[u8], mut at: usize) -> usize {
    loop {
        let first = past_horizontal_space(code, at);
        match code.get(first..).unwrap_or_default() {
            [b'\\', b'\n', ..] => at = first.saturating_add(2),
            [b'\\', b'\r', b'\n', ..] => at = first.saturating_add(3),
            _ => return first,
        }
    }
}

// How many of a node's children before a gap `GapScan` keeps for the
// comment lookup: the one ending at the gap, the `\n` token a grammar may
// put between it and the gap, and one spare for a zero-width recovery
// token.
const CHILDREN_BEFORE_A_GAP: usize = 3;

struct GapScan<'s, 'a> {
    // The node whose gaps are scanned, and so the one holding any comment
    // a bare row in them continues.
    node: tree_sitter::Node<'a>,
    // The node's parent, `None` at the root, whose padding alone runs from
    // byte 0. It holds the `\` before a bare row when the node starts
    // mid-row after a sibling's token: Ruby's `then` and `do` begin where
    // the header's trailing comment, a child of their parent, ends.
    parent: Option<tree_sitter::Node<'a>>,
    // The node's children before the gap being scanned, nearest first.
    before: [Option<tree_sitter::Node<'a>>; CHILDREN_BEFORE_A_GAP],
    code: &'s [u8],
    stats: &'s mut Stats,
    comments: BareRowComments,
    // The last bare row credited and what as: a run of bare rows is
    // whatever its first one is.
    previous_bare: Option<(usize, BareRow)>,
}

impl<'a> GapScan<'_, 'a> {
    // Scans each of `node`'s gaps in turn: before its first child, between
    // its children and after its last, keeping the window of children
    // before the gap that the comment lookups read.
    fn scan(&mut self, node: &Node<'a>) {
        let (mut gap_start, mut gap_row) = if self.parent.is_none() {
            (0, 0)
        } else {
            (node.start_byte(), node.start_row())
        };
        // A cursor rather than `children()`: seated on the gap's next
        // child while the gap is scanned, a copy of it steps to the
        // comments after that child at one sibling each
        // (`only_comments_after`).
        let mut cursor = node.cursor();
        let mut more = cursor.goto_first_child();
        while more {
            let child = cursor.node();
            self.gap(gap_start, gap_row, child.start_byte(), Some(&cursor));
            gap_start = child.end_byte();
            gap_row = child.end_row();
            self.before.rotate_right(1);
            self.before[0] = Some(child.as_tree_sitter());
            more = cursor.goto_next_sibling();
        }
        // The padding after the last child: only the root's runs on to
        // EOF, the one place a bare row can lack its newline.
        let end = if self.parent.is_none() {
            self.code.len()
        } else {
            node.end_byte()
        };
        self.gap(gap_start, gap_row, end, None);
    }

    // Scans `start` up to `end`, where `next` (a cursor on the child ending
    // the gap) starts, or the node or the file ends for `None`; the gap's
    // first byte sits on `row`.
    fn gap(&mut self, start: usize, mut row: usize, end: usize, next: Option<&Cursor<'a>>) {
        let Some(bytes) = self.code.get(start..end) else {
            return;
        };
        // `None` while the row being read began inside a child, whose
        // token it then carries.
        let mut row_start =
            (start == 0 || self.code.get(start.wrapping_sub(1)) == Some(&b'\n')).then_some(start);
        let mut shape = RowShape::Blank;
        for (offset, &byte) in (start..).zip(bytes) {
            if byte == b'\n' {
                if let Some(first) = row_start
                    && shape.is_bare()
                {
                    self.credit(row, first, start, next);
                }
                row = row.saturating_add(1);
                row_start = Some(offset.saturating_add(1));
                shape = RowShape::Blank;
            } else {
                shape = shape.step(byte);
            }
        }
        if end == self.code.len()
            && let Some(first) = row_start
            && shape.is_bare()
        {
            self.credit(row, first, start, next);
        }
    }

    // Credits the bare `row`, which starts at byte `row_start` in the gap
    // that starts at `gap_start` and ends at `next`.
    fn credit(
        &mut self,
        row: usize,
        row_start: usize,
        gap_start: usize,
        next: Option<&Cursor<'a>>,
    ) {
        let class = self.classify(row, row_start, gap_start, next);
        match class {
            // A range arm already made the row code — a Tcl quoted word
            // around the command substitution holding the comment — so it
            // is a comment on a code line, as `add_cloc_lines` files one;
            // filing it comment-only too would subtract it from `blank`
            // twice.
            BareRow::Comment if self.stats.ploc.lines.contains(row) => {
                add_code_comment_line(self.stats, row);
            }
            BareRow::Comment => add_only_comment_lines(self.stats, row, row),
            // No `check_comment_ends_on_code_line`: that reclassifies a
            // comment ending on this row, and nothing but the `\` is on it.
            BareRow::Code => {
                self.stats.ploc.lines.insert(row);
            }
            BareRow::Blank => {}
        }
        self.previous_bare = Some((row, class));
    }

    // What the bare `row` is: what the row before it continues onto it,
    // or, when that row continues nothing, what the logical line it starts
    // becomes.
    fn classify(
        &self,
        row: usize,
        row_start: usize,
        gap_start: usize,
        next: Option<&Cursor<'a>>,
    ) -> BareRow {
        if let Some((bare_row, class)) = self.previous_bare
            && bare_row.saturating_add(1) == row
        {
            return class;
        }
        let Some(last) = continuing_backslash(self.code, row_start) else {
            return self.spliced_into(row_start, next);
        };
        // A `\` in padding follows code, since a `#` comment swallows its
        // row's `\`, unless a C-family `/* … */` left it on a row of
        // comments alone. Those comments are then the line's text however
        // empty the rows after them are.
        if last >= gap_start {
            return if self.only_comments_before(last) {
                match self.spliced_into(row_start, next) {
                    BareRow::Blank => BareRow::Comment,
                    class => class,
                }
            } else {
                BareRow::Code
            };
        }
        let in_comment = self.holder_of(last).is_some_and(|holder| {
            holder
                .descendant_for_byte_range(last, last.saturating_add(1))
                .is_some_and(|covering| self.comments.lists(covering))
        });
        match (in_comment, self.comments.continued) {
            (false, _) => BareRow::Code,
            (true, true) => BareRow::Comment,
            // A Bash comment's `\` is comment text, so the bare row starts
            // a logical line of its own.
            (true, false) => self.spliced_into(row_start, next),
        }
    }

    // Whether the row ending in the padding `\` at byte `last` holds only
    // comments before it and starts its logical line. Its tokens are the
    // children before the gap, nearest first. A row that continues an
    // earlier one, or holds more tokens than the window keeps, counts as
    // code: the rows before it are not walked back.
    fn only_comments_before(&self, last: usize) -> bool {
        let line_start = self
            .code
            .get(..last)
            .and_then(|row| row.iter().rposition(|&byte| byte == b'\n'))
            .map_or(0, |newline| newline.saturating_add(1));
        if continuing_backslash(self.code, line_start).is_some() {
            return false;
        }
        // The first child that settles it, nearest first: one ending before
        // the row leaves only the comments already passed, a comment
        // covering the row's start makes all of it comment text, and any
        // other token is code.
        self.before
            .iter()
            .find_map(|&child| match child {
                // Fewer children than slots: only padding precedes the
                // root's first child, but any other node may follow a token
                // of an ancestor's on the row.
                None => Some(self.parent.is_none() || line_start >= self.node.start_byte()),
                Some(child) if child.end_byte() <= line_start => Some(true),
                Some(child) if !self.comments.lists(child) => Some(false),
                Some(child) => (child.start_byte() <= line_start).then_some(true),
            })
            .unwrap_or(false)
    }

    // What the logical line a run of bare rows starts at byte `row_start`
    // becomes, read off the first row after the run: blank when that row
    // is empty or the file ends, a comment when the line holds only
    // comment text, code otherwise.
    fn spliced_into(&self, row_start: usize, next: Option<&Cursor<'a>>) -> BareRow {
        // Only padding precedes `next`, so the skip stops there: Elixir
        // lexes a form feed as a token, which must not read as space.
        let padding = next
            .and_then(|cursor| self.code.get(..cursor.node().start_byte()))
            .unwrap_or(self.code);
        let first = past_line_padding(padding, row_start);
        if at_row_end(self.code, first) {
            BareRow::Blank
        } else if next
            .is_some_and(|cursor| opens_comment_only_line(self.code, self.comments, cursor, first))
        {
            BareRow::Comment
        } else {
            BareRow::Code
        }
    }

    // The node to look byte `last` up from, the `\` ending the row before
    // a bare one, when that `\` is not padding in the gap itself: the
    // child before the gap that holds it.
    //
    // Only the newline separates `last` from the gap's first row, so the
    // child holding it is the one ending at the gap or, past a `\n`
    // token, the one before. Looking up from the gap's owner instead
    // costs that owner's width per bare row: tree-sitter scans each
    // level's children from the first, and an unclosed opener per unit
    // leaves every unit a direct child of one flat `ERROR`, which it
    // never balances (`loc/tcl-unclosed-continued-comments`). The child
    // must contain `last`: looked up from a child that does not,
    // `descendant_for_byte_range` answers the child itself, a comment
    // that ended a row earlier among them.
    fn holder_of(&self, last: usize) -> Option<tree_sitter::Node<'a>> {
        // Of the fallbacks below, only the parent's is reached by valid
        // input (Ruby's `then` and `do`); otherwise the `\` is inside the
        // child ending at the gap or the one before. Each returns the safe
        // answer rather than asserting, because a zero-width recovery
        // token landing between the `\` and the gap would reach one, and
        // there the answer below is the right one.
        for child in self.before {
            // Fewer children than slots: `last` is padding before the
            // first, where no comment is, unless it precedes the node
            // itself, in a token of the parent's (`GapScan::parent`).
            let Some(child) = child else {
                return self
                    .parent
                    .filter(|parent| parent.start_byte() <= last && last < self.node.start_byte());
            };
            if child.start_byte() <= last {
                return (last < child.end_byte()).then_some(child);
            }
        }
        // More zero-width children than slots stand between: the owner
        // still holds `last`, at the owner's width.
        Some(self.node)
    }
}

// Whether the logical line from byte `first`, the joined row's first
// token, holds only comment text: the gap's next child (`next`) — or the
// first sibling that is not a `line_continuation` after a Python run — or
// a first descendant of it, is a comment starting at `first`, and only padding,
// `\` splices and further comments follow it. A `#` comment runs to its
// row's end unless a NUL ends its token, where the tree recovers with an
// `ERROR` that makes that row code, as the bare row joining it then is; a
// C-family block comment can leave code there (`\` + `/* c */ int z = 1;`
// is one code line once spliced). Each node is asked for one gap's run at
// most, so the descent stays linear in the file. The comment is `child`
// itself in every tree seen so far but that NUL's, a child of the `ERROR`
// in Tcl and iRules; the descent (which Tcl's `puts a` + `\` + `# c`,
// #1808, runs into a word) is kept because a grammar may wrap one.
pub(super) fn opens_comment_only_line(
    code: &[u8],
    comments: BareRowComments,
    next: &Cursor<'_>,
    first: usize,
) -> bool {
    let child = next.node().as_tree_sitter();
    let mut node = Some(child);
    while let Some(current) = node
        && current.start_byte() == first
    {
        if comments.lists(current) {
            // Only `child`'s own siblings are at hand; a comment below it
            // is the line's last token or the line is code.
            let siblings = (current == child).then_some(next);
            return only_comments_after(code, comments, current, siblings);
        }
        node = current.child(0);
    }
    false
}

// Whether only padding, `\` splices and comments follow `comment` to the
// end of its logical line. The comments after it are its siblings, which
// a copy of `siblings` (a cursor on `comment`) steps to, made only once
// the line runs on past it: a sibling step is amortised O(1), where
// `next_sibling` or a lookup from the gap's owner scans a flat `ERROR`
// from its first child.
fn only_comments_after<'a>(
    code: &[u8],
    comments: BareRowComments,
    mut comment: tree_sitter::Node<'a>,
    siblings: Option<&Cursor<'a>>,
) -> bool {
    let mut copy: Option<Cursor<'a>> = None;
    loop {
        // A comment running past its row is the rest of that row.
        if comment.end_position().row > comment.start_position().row {
            return true;
        }
        let after = past_line_padding(code, comment.end_byte());
        if at_row_end(code, after) {
            return true;
        }
        // The line runs on, and stays comment text only if the next token,
        // where the padding ends, is a comment too.
        let next = siblings.and_then(|siblings| {
            let cursor = copy.get_or_insert_with(|| siblings.clone());
            cursor
                .goto_next_sibling()
                .then(|| cursor.node().as_tree_sitter())
        });
        match next {
            Some(sibling) if sibling.start_byte() == after && comments.lists(sibling) => {
                comment = sibling;
            }
            _ => return false,
        }
    }
}

// The offset of the `\` that ends the row before the one starting at byte
// `row_start`, if that `\` continues the row: a trailing `\r` is stepped
// over, and an even run is escaped backslashes rather than a continuation
// (`tclsh` runs the row after `# c \\`).
pub(super) fn continuing_backslash(code: &[u8], row_start: usize) -> Option<usize> {
    // `row_start - 1` is the previous row's newline.
    let newline = row_start.checked_sub(1)?;
    let before = code.get(..newline)?;
    // Reads the row the way `RowShape` reads the bare one. It decides
    // input whose comment row ends in CRLF while the bare row after it
    // ends in LF: the Tcl and iRules `comment` token takes the `\r`, the
    // bare row is padding, and tclsh continues the comment through it
    // (`tcl_bare_continuation_rows_are_code_or_comment`). Only the raw
    // library and Python `Ast.parse` paths keep a CR at all; the CLI
    // normalises line endings first. An all-CRLF bare row never gets
    // here: those grammars lex its `\` + `\r` as a word (#1802).
    let content = before.strip_suffix(b"\r").unwrap_or(before);
    let run = content
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count();
    (run % 2 == 1).then(|| content.len().saturating_sub(1))
}

// The first byte of the row holding byte `at`, when only horizontal
// space precedes `at` on it: the bare-row test of a node's own row, for
// a grammar that keeps the `\` as a node (Python). The row-start scan is
// `GapScan::only_comments_before`'s too (#1818 owns that region).
pub(super) fn bare_row_start(code: &[u8], at: usize) -> Option<usize> {
    let row_start = code
        .get(..at)
        .and_then(|before| before.iter().rposition(|&byte| byte == b'\n'))
        .map_or(0, |newline| newline.saturating_add(1));
    (past_horizontal_space(code, row_start) == at).then_some(row_start)
}
