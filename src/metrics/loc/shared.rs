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

#[inline]
pub(crate) fn init(node: &Node, stats: &mut Stats, is_func_space: bool) -> (usize, usize) {
    let (start, _) = node.start_position();
    let (end, _) = node.end_position();

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
    (start, end)
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
pub(crate) fn add_cloc_lines(stats: &mut Stats, start: usize, end: usize) {
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
// Mirrors Python's `String` arm exactly: the opening row is inserted only
// when the enclosing statement begins on an earlier row — otherwise that
// row is already attributed to the parent — and rows `start + 1..=end`
// (the interior and closing rows) are always inserted.
#[inline]
pub(crate) fn add_multiline_string_ploc(
    node: &Node,
    ancestors: Ancestors<'_, '_>,
    stats: &mut Stats,
    start: usize,
    end: usize,
) {
    if ancestors
        .parent(node)
        .is_none_or(|parent| parent.start_row() != start)
    {
        check_comment_ends_on_code_line(stats, start);
        stats.ploc.lines.insert(start);
    }
    (start.saturating_add(1)..=end).for_each(|line| {
        stats.ploc.lines.insert(line);
    });
}
