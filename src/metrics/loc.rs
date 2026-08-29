// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
// `Loc` is the one metric that computes on tree-sitter *span
// coordinates* rather than on its own accumulators, and a row index
// arriving from a parse is attacker-controlled through the source
// layout. #1051 was a `usize` underflow of exactly this shape — a Rust
// doc comment at EOF drove `end - 1` below zero, panicking in debug and
// wrapping to `usize::MAX` in release, from an input as small as
// `/// x`. Warning here forces every span adjustment to be explicitly
// saturating, checked, or annotated, rather than relying on a bound
// that holds only until a grammar changes shape.
//
// Deliberately scoped to `loc` rather than to `src/metrics/`: the other
// metrics contribute 244 hits, all of them `+=` on their own counters,
// which is not this bug class and would bury it (#1152).
#![warn(clippy::arithmetic_side_effects)]

use crate::checker::Checker;
use crate::metrics::npa::python_is_block;
use std::fmt;

use crate::macros::implement_metric_trait;
use crate::*;

// Collapse the `usize::MAX` sentinel that `*_min` fields are
// initialised to on `Default` into `0`, so a never-observed space
// serializes to a meaningful number rather than `18446744073709551615`.
// Mirrors `tokens::Stats::tokens_min`'s guard.
#[inline]
fn min_or_zero(v: usize) -> u64 {
    if v == usize::MAX { 0 } else { v as u64 }
}

/// Number of physical source rows covered by a span running from the
/// 0-based `start_row` to the 1-based inclusive `end_line`.
///
/// The subtlety is not here — it is in which row counts as the last
/// one, and that rule lives once, in [`Node::end_line`]: a span whose
/// end position sits at column 0 stops *before* that row contributes a
/// single character, so the row belongs to whatever follows. Both
/// callers take `end_line` straight from the node, so this is plain
/// subtraction rather than a second copy of the rule.
///
/// Keying on the end column rather than on "is this the unit?" is what
/// fixed issue #1067 here and #1163 in `Node`: the unit/non-unit split
/// assumed the unit always ends at column 0 (false for un-newline-
/// terminated input, which then lost a row) and that nothing else ever
/// does (false for Perl, whose last `sub` gained one).
///
/// Requires `end_line >= start_row`, which tree-sitter guarantees for a
/// single node's own span. The `debug_assert` pins that in tests; the
/// `saturating_sub` decides what release does if it is ever violated
/// anyway. Zero is the right answer there — an inverted span covers no
/// rows. Note what this does *not* buy: `sloc()` already clamps with
/// `saturating_sub`, so a wrapped value could not have escaped as
/// `usize::MAX` either. It would have escaped as `sloc: 0` for a
/// non-empty file, and on into MI's SLOC term — a wrong number surfacing
/// far from its cause, which is how #1051 was reported.
#[inline]
fn span_rows(start_row: usize, end_line: usize) -> usize {
    debug_assert!(
        end_line >= start_row,
        "span_rows: end_line {end_line} < start_row {start_row}"
    );
    end_line.saturating_sub(start_row)
}

mod line_set;
use line_set::LineSet;

/// The `SLoc` metric suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Sloc {
    start: usize,
    // 1-based inclusive last line of the span, from `Node::end_line`.
    // Storing the resolved line rather than the raw end row plus its
    // column keeps the "does the final row count" rule in one place.
    end_line: usize,
    // Physical lines removed from this space's span by `exclude_tests`
    // pruning. `sloc` is the lone loc sub-metric computed by span
    // subtraction rather than node-by-node accumulation, so a pruned
    // subtree (which a `continue` in the walk suppresses for every
    // accumulated metric) leaves the span untouched. We accumulate the
    // inclusive row count of each pruned subtree here and subtract it
    // in `sloc()` so SLOC drops in step with `ploc`/`cloc`/`lloc`
    // (issue #722).
    excluded_lines: usize,
    sloc_min: usize,
    sloc_max: usize,
}

impl Default for Sloc {
    fn default() -> Self {
        Self {
            start: 0,
            end_line: 0,
            excluded_lines: 0,
            sloc_min: usize::MAX,
            sloc_max: 0,
        }
    }
}

impl Sloc {
    /// The `SLOC` metric value for this space (source lines, including blanks and comments).
    #[inline]
    #[must_use]
    pub fn sloc(&self) -> u64 {
        // This metric counts the number of physical lines this space
        // occupies, including blanks and comments.
        let span = span_rows(self.start, self.end_line);
        // Subtract the lines belonging to `exclude_tests`-pruned subtrees
        // (issue #722). `saturating_sub` is defensive: `excluded_lines`
        // can never exceed the span (each pruned subtree is contained in
        // it), but a future caller that double-records a span must not
        // wrap to `u64::MAX`.
        span.saturating_sub(self.excluded_lines) as u64
    }

    /// Records a pruned (`exclude_tests`) subtree's row span so that
    /// `sloc()` drops in step with the node-accumulated loc sub-metrics.
    /// The arguments are the pruned node's own start row and
    /// `Node::end_line`; its row count follows the same rule the
    /// enclosing span was measured with, so the subtraction cannot
    /// overshoot.
    ///
    /// Pruned subtrees are whole Rust items (`mod`/`fn`/`impl`/…) that
    /// rustfmt places on dedicated rows, so they share no physical line
    /// with a retained sibling and their spans are pairwise disjoint (the
    /// walk `continue`s on a pruned node, never descending, so a nested
    /// pruned item is never recorded twice). The counts therefore add
    /// without an interval merge (issue #722).
    #[inline]
    pub(crate) fn exclude_span(&mut self, start_row: usize, end_line: usize) {
        self.excluded_lines = self
            .excluded_lines
            .saturating_add(span_rows(start_row, end_line));
    }

    /// The `Sloc` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub fn sloc_min(&self) -> u64 {
        min_or_zero(self.sloc_min)
    }

    /// The `Sloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn sloc_max(&self) -> u64 {
        self.sloc_max as u64
    }

    /// Folds `other` into `self`, updating the min/max accumulators and
    /// accumulating the child's `exclude_tests`-pruned line count.
    #[inline]
    pub fn merge(&mut self, other: &Sloc) {
        // Fold the child's own min/max (not its aggregate `sloc()`), so the
        // granularity of deeply nested function spaces propagates to the
        // root. This matches every sibling metric (cyclomatic, cognitive,
        // exit, nargs, nom, tokens, abc) and fixed issue #437.
        self.sloc_min = self.sloc_min.min(other.sloc_min);
        self.sloc_max = self.sloc_max.max(other.sloc_max);

        // Propagate the child's pruned line count upward so an ancestor's
        // span-based `sloc()` drops by the same lines, mirroring how `Ploc`
        // unions its line-set upward (`Ploc::merge`). The prune hook records
        // each pruned subtree's span only on its innermost enclosing
        // func-space; without this fold a `#[test] fn` inside a retained
        // `impl`/`trait`/closure would shrink only that space's `sloc`,
        // leaving every enclosing space (including the unit, which feeds
        // MI's SLOC term) inflated (issue #741, #722 follow-up). Each
        // ancestor's span already includes the pruned rows exactly once, so
        // subtracting the accumulated count once per level cannot
        // double-count: pruned subtrees never descend, so a nested pruned
        // item is recorded on a single space and folded up one altitude at
        // a time.
        self.excluded_lines = self.excluded_lines.saturating_add(other.excluded_lines);
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        // Fold this space's own span unconditionally so containers (Unit,
        // classes) participate in min/max, matching the sibling metrics'
        // convention. Each space runs this before being merged upward, so
        // the guarded form previously here dropped nested leaves (#437).
        self.sloc_min = self.sloc_min.min(self.sloc() as usize);
        self.sloc_max = self.sloc_max.max(self.sloc() as usize);
    }
}

/// The `PLoc` metric suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Ploc {
    lines: LineSet,
    ploc_min: usize,
    ploc_max: usize,
}

impl Default for Ploc {
    fn default() -> Self {
        Self {
            lines: LineSet::default(),
            ploc_min: usize::MAX,
            ploc_max: 0,
        }
    }
}

impl Ploc {
    /// The `PLOC` metric value for this space (physical lines of code, excluding blanks and comments).
    #[inline]
    #[must_use]
    pub fn ploc(&self) -> u64 {
        // This metric counts the number of instruction lines in a code
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        self.lines.len() as u64
    }

    /// The `Ploc` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub fn ploc_min(&self) -> u64 {
        min_or_zero(self.ploc_min)
    }

    /// The `Ploc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn ploc_max(&self) -> u64 {
        self.ploc_max as u64
    }

    /// Folds `other` into `self`, unioning the line set and updating min/max.
    #[inline]
    pub fn merge(&mut self, other: &Ploc) {
        // Union the child's physical lines in, so a line shared with a
        // sibling space is counted once. A word-wise OR rather than an
        // insert per row: a line inside D nested spaces is folded upward
        // D times (#1109).
        self.lines.union_with(&other.lines);

        // Fold the child's own min/max so nested spaces propagate (#437).
        self.ploc_min = self.ploc_min.min(other.ploc_min);
        self.ploc_max = self.ploc_max.max(other.ploc_max);
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        // Fold this space's own value unconditionally so containers
        // participate, matching the sibling metrics' convention (#437).
        // Bound once: `ploc()` is a popcount over the whole word array
        // since #1109, not the O(1) `HashSet::len` it used to be.
        let ploc = self.ploc() as usize;
        self.ploc_min = self.ploc_min.min(ploc);
        self.ploc_max = self.ploc_max.max(ploc);
    }
}

/// The `CLoc` metric suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Cloc {
    // Physical lines that are comment-only (no code). Feeds both
    // `cloc()` and the `blank` metric (`sloc - ploc - only.len()`).
    // A set rather than a counter so two standalone block comments on
    // one physical line (`/*a*/ /*b*/`) contribute a single comment
    // line, not one per node (issue #461 follow-up). Each spanned row
    // of a genuine multi-line block comment is a distinct key, so it
    // still counts once per line.
    only_comment_line_starts: LineSet,
    // Physical lines carrying both code and comment (`int x; /*c*/`).
    // A line with several inline block comments (`f(int /*a*/, int
    // /*b*/)`) must contribute a single comment line, not one per
    // node, otherwise cloc can exceed sloc/ploc and push the MI
    // comments_percentage above 100% (issue #461). Mirrors `Ploc`'s
    // per-line de-dup via `Ploc::lines`; disjoint from
    // `only_comment_line_starts` by construction.
    code_comment_line_starts: LineSet,
    comment_line_end: Option<usize>,
    cloc_min: usize,
    cloc_max: usize,
}

impl Default for Cloc {
    fn default() -> Self {
        Self {
            only_comment_line_starts: LineSet::default(),
            code_comment_line_starts: LineSet::default(),
            comment_line_end: Option::default(),
            cloc_min: usize::MAX,
            cloc_max: 0,
        }
    }
}

impl Cloc {
    /// The `CLOC` metric value for this space (comment lines, standalone + trailing).
    #[inline]
    #[must_use]
    pub fn cloc(&self) -> u64 {
        // Comments are counted regardless of their placement
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        //
        // Derive from the per-physical-line sets rather than summed
        // counters so co-located comments (standalone or inline) count
        // their shared line once and a comment line shared across
        // merged spaces is not double-counted (issue #461). The two
        // sets are disjoint by construction, but a union is used
        // defensively so a stray overlap cannot inflate the count.
        self.only_comment_line_starts
            .union_len(&self.code_comment_line_starts) as u64
    }

    /// The `Cloc` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub fn cloc_min(&self) -> u64 {
        min_or_zero(self.cloc_min)
    }

    /// The `Cloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn cloc_max(&self) -> u64 {
        self.cloc_max as u64
    }

    /// Folds `other` into `self`, summing comment counts and updating min/max.
    #[inline]
    pub fn merge(&mut self, other: &Cloc) {
        // Union both per-line sets so a comment line shared across
        // merged spaces is counted once (mirrors `Ploc`'s line union).
        self.only_comment_line_starts
            .union_with(&other.only_comment_line_starts);
        self.code_comment_line_starts
            .union_with(&other.code_comment_line_starts);

        // Fold the child's own min/max so nested spaces propagate (#437).
        self.cloc_min = self.cloc_min.min(other.cloc_min);
        self.cloc_max = self.cloc_max.max(other.cloc_max);
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        // Fold this space's own value unconditionally so containers
        // participate, matching the sibling metrics' convention (#437).
        // Bound once: `cloc()` is a `union_len` over both word arrays
        // since #1109, so calling it twice is six array scans per space.
        let cloc = self.cloc() as usize;
        self.cloc_min = self.cloc_min.min(cloc);
        self.cloc_max = self.cloc_max.max(cloc);
    }
}

/// The `LLoc` metric suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Lloc {
    logical_lines: usize,
    lloc_min: usize,
    lloc_max: usize,
}

impl Default for Lloc {
    fn default() -> Self {
        Self {
            logical_lines: 0,
            lloc_min: usize::MAX,
            lloc_max: 0,
        }
    }
}

impl Lloc {
    /// The `LLOC` metric value for this space (logical statements).
    #[inline]
    #[must_use]
    pub fn lloc(&self) -> u64 {
        // This metric counts the number of statements in a code
        // https://en.wikipedia.org/wiki/Source_lines_of_code
        self.logical_lines as u64
    }

    /// The `Lloc` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub fn lloc_min(&self) -> u64 {
        min_or_zero(self.lloc_min)
    }

    /// The `Lloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn lloc_max(&self) -> u64 {
        self.lloc_max as u64
    }

    /// Records one logical statement.
    ///
    /// Exists so the 23 per-language `Loc` impls name the operation
    /// instead of each reaching into a private field, which also keeps
    /// the module's `arithmetic_side_effects` carve-out to this one
    /// line rather than 36 of them (#1152). Saturating is unreachable —
    /// the count is bounded by the AST's node count — and is the right
    /// answer if it ever were: a pinned `usize::MAX` is a visibly broken
    /// LLOC, where a wrap to 0 reads as a legitimately empty space.
    #[inline]
    fn count_logical_line(&mut self) {
        self.logical_lines = self.logical_lines.saturating_add(1);
    }

    /// Folds `other` into `self`, summing statement counts and updating min/max.
    #[inline]
    pub fn merge(&mut self, other: &Lloc) {
        // Merge lloc lines
        self.logical_lines = self.logical_lines.saturating_add(other.logical_lines);
        // Fold the child's own min/max so nested spaces propagate (#437).
        self.lloc_min = self.lloc_min.min(other.lloc_min);
        self.lloc_max = self.lloc_max.max(other.lloc_max);
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        // Fold this space's own value unconditionally so containers
        // participate, matching the sibling metrics' convention (#437).
        self.lloc_min = self.lloc_min.min(self.lloc() as usize);
        self.lloc_max = self.lloc_max.max(self.lloc() as usize);
    }
}

/// The `Loc` metric suite.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    sloc: Sloc,
    ploc: Ploc,
    cloc: Cloc,
    lloc: Lloc,
    space_count: usize,
    blank_min: usize,
    blank_max: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            sloc: Sloc::default(),
            ploc: Ploc::default(),
            cloc: Cloc::default(),
            lloc: Lloc::default(),
            space_count: 1,
            blank_min: usize::MAX,
            blank_max: 0,
        }
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sloc: {}, ploc: {}, lloc: {}, cloc: {}, blank: {}, sloc_average: {}, ploc_average: {}, lloc_average: {}, cloc_average: {}, blank_average: {}, sloc_min: {}, sloc_max: {}, cloc_min: {}, cloc_max: {}, ploc_min: {}, ploc_max: {}, lloc_min: {}, lloc_max: {}, blank_min: {}, blank_max: {}",
            self.sloc(),
            self.ploc(),
            self.lloc(),
            self.cloc(),
            self.blank(),
            self.sloc_average(),
            self.ploc_average(),
            self.lloc_average(),
            self.cloc_average(),
            self.blank_average(),
            self.sloc_min(),
            self.sloc_max(),
            self.cloc_min(),
            self.cloc_max(),
            self.ploc_min(),
            self.ploc_max(),
            self.lloc_min(),
            self.lloc_max(),
            self.blank_min(),
            self.blank_max(),
        )
    }
}

impl Stats {
    /// Test-only constructor that forces a degenerate `cloc`/`sloc`
    /// pair (here `cloc > sloc`) so callers in other metric modules can
    /// exercise downstream clamps without depending on the parsing
    /// pipeline. `cloc <= sloc` always holds for parsed input after
    /// issue #461, so this state is unreachable through normal use.
    #[cfg(test)]
    pub(crate) fn with_cloc_sloc(code_comment_lines: usize, sloc_end_row: usize) -> Self {
        let mut stats = Stats::default();
        stats.sloc.start = 0;
        // `end_row + 1`: the synthetic span models a real one ending
        // mid-line, so the final row counts and `sloc == sloc_end_row + 1`.
        stats.sloc.end_line = sloc_end_row.saturating_add(1);
        // Inject `code_comment_lines` distinct synthetic code-comment
        // rows. An offset past `sloc_end_row` keeps them disjoint from
        // any real span row, so `cloc()` (the set's cardinality) equals
        // the requested count without colliding with sloc attribution.
        if code_comment_lines > 0 {
            let synthetic_base = sloc_end_row.saturating_add(1);
            // Explicit rather than leaning on `insert_range`'s inverted-span
            // guard: that guard exists to survive a bug, not to serve as a
            // caller's empty case. The `- 1` is exact under the `> 0` test
            // above, which is also what makes the inclusive end well-formed.
            let synthetic_end = synthetic_base
                .saturating_add(code_comment_lines)
                .saturating_sub(1);
            stats
                .cloc
                .code_comment_line_starts
                .insert_range(synthetic_base, synthetic_end);
        }
        stats
    }

    /// Merges a second `Loc` metric suite into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.sloc.merge(&other.sloc);
        self.ploc.merge(&other.ploc);
        self.cloc.merge(&other.cloc);
        self.lloc.merge(&other.lloc);

        // Count spaces
        self.space_count = self.space_count.saturating_add(other.space_count);

        // Fold the child's own min/max so nested spaces propagate (#437).
        self.blank_min = self.blank_min.min(other.blank_min);
        self.blank_max = self.blank_max.max(other.blank_max);
    }

    /// Records an `exclude_tests`-pruned subtree's span so this space's
    /// `sloc()` excludes those physical lines, matching the
    /// node-accumulated loc sub-metrics that the pruning already drops
    /// (issue #722). Called from the walker for the space enclosing each
    /// skipped subtree.
    #[inline]
    pub(crate) fn exclude_test_span(&mut self, start_row: usize, end_line: usize) {
        self.sloc.exclude_span(start_row, end_line);
    }

    /// The `Sloc` metric.
    ///
    /// Counts the number of lines in a scope
    #[inline]
    #[must_use]
    pub fn sloc(&self) -> u64 {
        self.sloc.sloc()
    }

    /// The `Ploc` metric.
    ///
    /// Counts the number of instruction lines in a scope
    #[inline]
    #[must_use]
    pub fn ploc(&self) -> u64 {
        self.ploc.ploc()
    }

    /// The `Lloc` metric.
    ///
    /// Counts the number of statements in a scope
    #[inline]
    #[must_use]
    pub fn lloc(&self) -> u64 {
        self.lloc.lloc()
    }

    /// The `Cloc` metric.
    ///
    /// Counts the number of comments in a scope
    #[inline]
    #[must_use]
    pub fn cloc(&self) -> u64 {
        self.cloc.cloc()
    }

    /// The `Blank` metric.
    ///
    /// Counts the number of blank lines in a scope
    #[inline]
    #[must_use]
    pub fn blank(&self) -> u64 {
        // `sloc - ploc - only_comment_lines` can go negative when a space's
        // physical and comment line attribution overlaps the span row count
        // (e.g. single-line bodies). `saturating_sub` clamps at 0 (matching
        // the prior `.max(0.0)` on the f64 form) so the serialized value is
        // never negative (#437).
        self.sloc()
            .saturating_sub(self.ploc())
            .saturating_sub(self.cloc.only_comment_line_starts.len() as u64)
    }

    /// The `Sloc` metric average value.
    ///
    /// This value is computed dividing the `Sloc` value for the number of spaces
    #[inline]
    #[must_use]
    pub fn sloc_average(&self) -> f64 {
        crate::metrics::average(self.sloc() as f64, self.space_count)
    }

    /// The `Ploc` metric average value.
    ///
    /// This value is computed dividing the `Ploc` value for the number of spaces
    #[inline]
    #[must_use]
    pub fn ploc_average(&self) -> f64 {
        crate::metrics::average(self.ploc() as f64, self.space_count)
    }

    /// The `Lloc` metric average value.
    ///
    /// This value is computed dividing the `Lloc` value for the number of spaces
    #[inline]
    #[must_use]
    pub fn lloc_average(&self) -> f64 {
        crate::metrics::average(self.lloc() as f64, self.space_count)
    }

    /// The `Cloc` metric average value.
    ///
    /// This value is computed dividing the `Cloc` value for the number of spaces
    #[inline]
    #[must_use]
    pub fn cloc_average(&self) -> f64 {
        crate::metrics::average(self.cloc() as f64, self.space_count)
    }

    /// The `Blank` metric average value.
    ///
    /// This value is computed dividing the `Blank` value for the number of spaces
    #[inline]
    #[must_use]
    pub fn blank_average(&self) -> f64 {
        crate::metrics::average(self.blank() as f64, self.space_count)
    }

    /// The `Sloc` metric minimum value.
    #[inline]
    #[must_use]
    pub fn sloc_min(&self) -> u64 {
        self.sloc.sloc_min()
    }

    /// The `Sloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn sloc_max(&self) -> u64 {
        self.sloc.sloc_max()
    }

    /// The `Cloc` metric minimum value.
    #[inline]
    #[must_use]
    pub fn cloc_min(&self) -> u64 {
        self.cloc.cloc_min()
    }

    /// The `Cloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn cloc_max(&self) -> u64 {
        self.cloc.cloc_max()
    }

    /// The `Ploc` metric minimum value.
    #[inline]
    #[must_use]
    pub fn ploc_min(&self) -> u64 {
        self.ploc.ploc_min()
    }

    /// The `Ploc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn ploc_max(&self) -> u64 {
        self.ploc.ploc_max()
    }

    /// The `Lloc` metric minimum value.
    #[inline]
    #[must_use]
    pub fn lloc_min(&self) -> u64 {
        self.lloc.lloc_min()
    }

    /// The `Lloc` metric maximum value.
    #[inline]
    #[must_use]
    pub fn lloc_max(&self) -> u64 {
        self.lloc.lloc_max()
    }

    /// The `Blank` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub fn blank_min(&self) -> u64 {
        min_or_zero(self.blank_min)
    }

    /// The `Blank` metric maximum value.
    #[inline]
    #[must_use]
    pub fn blank_max(&self) -> u64 {
        self.blank_max as u64
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.sloc.compute_minmax();
        self.ploc.compute_minmax();
        self.cloc.compute_minmax();
        self.lloc.compute_minmax();

        // Fold this space's own blank value unconditionally so containers
        // participate, matching the sibling metrics' convention (#437).
        // `blank()` returns a `u64` already clamped at 0 by `saturating_sub`,
        // so the widening `as usize` cast is lossless (64-bit) and cannot
        // introduce a spurious value here. Bound once: `blank()` popcounts
        // two word arrays since #1109.
        let blank = self.blank() as usize;
        self.blank_min = self.blank_min.min(blank);
        self.blank_max = self.blank_max.max(blank);
    }

    pub(crate) fn init_unit_span(&mut self, start: usize, end_line: usize) {
        self.sloc.start = start;
        self.sloc.end_line = end_line;
    }
}

#[doc(hidden)]
/// Per-language computation of the lines-of-code metrics.
pub(crate) trait Loc
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    /// `ancestors` is the chain the walker descended through: the
    /// C-family and JVM-family arms read it to tell a declaration in a
    /// loop header from one in the loop body (#1084).
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool);
}

mod shared;
pub(crate) use shared::*;

// Real defaults — Loc counts on these "languages" would conflate
// comments / preproc directives with executable code; treating them
// as 0 is the documented behaviour. Audited in #188.
implement_metric_trait!(Loc, PreprocCode, CcommentCode);

mod bash;
mod c;
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod irules;
mod java;
mod javascript;
mod kotlin;
mod lua;
mod mozcpp;
mod mozjs;
mod objc;
mod perl;
mod php;
mod python;
mod ruby;
mod rust;
mod tcl;
mod tsx;
mod typescript;

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use crate::test_support::{check_metrics_only_shim, metrics_verbatim, space_verbatim};

    use super::*;

    check_metrics_only_shim!(check_metrics, Loc);

    /// A `Stats::default()` that never sees an observation must not leak
    /// the `usize::MAX` sentinel for any of the LOC `_min` accumulators
    /// (`sloc_min`, `ploc_min`, `lloc_min`, `cloc_min`, `blank_min`).
    /// The getters collapse the sentinel to `0.0` so JSON never emits
    /// `1.8446744e19`.
    #[test]
    fn loc_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.sloc_min(), 0);
        assert_eq!(stats.ploc_min(), 0);
        assert_eq!(stats.lloc_min(), 0);
        assert_eq!(stats.cloc_min(), 0);
        assert_eq!(stats.blank_min(), 0);
    }

    /// Parses `source` with `PerlParser` and asserts the resulting tree has
    /// no `ERROR` nodes. Use alongside metric assertions whose expected
    /// values would happen to match what an error tree produces — a parse
    /// regression in tree-sitter-perl could otherwise leave such tests
    /// silently green.
    #[cfg(test)]
    fn assert_perl_parses_cleanly(source: &str) {
        use crate::traits::ParserTrait;
        // Mirror the trailing-newline normalisation `check_func_space` does
        // before handing input to the parser, so this helper sees the same
        // bytes the metric tests do.
        let path = std::path::PathBuf::from("foo.pl");
        let mut bytes = source.trim_end_matches('\n').as_bytes().to_vec();
        bytes.push(b'\n');
        let parser = PerlParser::new(bytes, &path, None);
        assert!(
            !parser.root().has_error(),
            "tree-sitter-perl returned an error tree for snippet:\n{source}"
        );
    }

    #[test]
    fn python_sloc() {
        check_metrics::<PythonParser>(
            "

            a = 42

            ",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 1,
                  "ploc": 1,
                  "lloc": 1,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 1.0,
                  "ploc_average": 1.0,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 1,
                  "sloc_max": 1,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_blank() {
        check_metrics::<PythonParser>(
            "
            a = 42

            b = 43

            ",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_blank() {
        check_metrics::<RustParser>(
            "

            let a = 42;

            let b = 43;

            ",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>("fn func() { /* comment */ }", "foo.rs", |metric| {
            // Spaces: 2
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 0,
              "cloc": 1,
              "blank": 0,
              "sloc_average": 0.5,
              "ploc_average": 0.5,
              "lloc_average": 0.0,
              "cloc_average": 0.5,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 1,
              "cloc_max": 1,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 0,
              "lloc_max": 0,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn c_blank() {
        check_metrics::<CParser>(
            "

            int a = 42;

            int b = 43;

            ",
            "foo.c",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4

                 updateServer = -42
                 isConnected = False
                 currTry = 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 10,
                  "ploc": 7,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 1,
                  "sloc_average": 5.0,
                  "ploc_average": 3.5,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.5,
                  "sloc_min": 10,
                  "sloc_max": 10,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 7,
                  "ploc_max": 7,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_no_blank() {
        // Checks that the blank metric is equal to 0 when there are no blank
        // lines and there are comments next to code lines.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4
                 updateServer = -42
                 isConnected = False
                 currTry = 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 9,
                  "ploc": 7,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 4.5,
                  "ploc_average": 3.5,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 9,
                  "sloc_max": 9,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 7,
                  "ploc_max": 7,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_no_zero_blank_more_comments() {
        // Checks that the blank metric is not equal to 0 when there are more
        // comments next to code lines compared to the previous tests.
        check_metrics::<PythonParser>(
            "def ConnectToUpdateServer():
                 pool = 4

                 updateServer = -42
                 isConnected = False
                 currTry = 0 # Set this variable to 0
                 numRetries = 10 # Number of IPC connection retries before
                                 # giving up.
                 numTries = 20 # Number of IPC connection tries before
                               # giving up.",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 10,
                  "ploc": 7,
                  "lloc": 6,
                  "cloc": 5,
                  "blank": 1,
                  "sloc_average": 5.0,
                  "ploc_average": 3.5,
                  "lloc_average": 3.0,
                  "cloc_average": 2.5,
                  "blank_average": 0.5,
                  "sloc_min": 10,
                  "sloc_max": 10,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 7,
                  "ploc_max": 7,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<RustParser>(
            "fn ConnectToUpdateServer() {
              let pool = 0;

              let updateServer = -42;
              let isConnected = false;
              let currTry = 0;
              let numRetries = 10;  // Number of IPC connection retries before
                                    // giving up.
              let numTries = 20;    // Number of IPC connection tries before
                                    // giving up.
            }",
            "foo.rs",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 1,
                  "sloc_average": 5.5,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.5,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<JavascriptParser>(
            "function ConnectToUpdateServer() {
              var pool = 0;

              var updateServer = -42;
              var isConnected = false;
              var currTry = 0;
              var numRetries = 10;  // Number of IPC connection retries before
                                    // giving up.
              var numTries = 20;    // Number of IPC connection tries before
                                    // giving up.
            }",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 1,
                  "sloc_average": 5.5,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.5,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              const int numRetries = 10; // Number of IPC connection retries before
                                         // giving up.
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 1,
                  "sloc_average": 5.5,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.5,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_start_block_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments starting next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              const int numRetries = 10; /* Number of IPC connection retries
              before
              giving up. */
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 12,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 5,
                  "blank": 1,
                  "sloc_average": 6.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.5,
                  "blank_average": 0.5,
                  "sloc_min": 12,
                  "sloc_max": 12,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_block_comment_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments on independent lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries
              before
              giving up. */
              const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 13,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 5,
                  "blank": 1,
                  "sloc_average": 6.5,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.5,
                  "blank_average": 0.5,
                  "sloc_min": 13,
                  "sloc_max": 13,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_block_one_line_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments before the same code line.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries before giving up. */ const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 10,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 5.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 1.5,
                  "blank_average": 0.5,
                  "sloc_min": 10,
                  "sloc_max": 10,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_code_line_end_block_blank() {
        // Checks that the blank metric is equal to 1 when there are
        // block comments ending next to code lines.
        check_metrics::<CppParser>(
            "void ConnectToUpdateServer() {
              int pool;

              int updateServer = -42;
              bool isConnected = false;
              int currTry = 0;
              /* Number of IPC connection retries
              before
              giving up. */ const int numRetries = 10;
              const int numTries = 20; // Number of IPC connection tries before
                                       // giving up.
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 12,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 5,
                  "blank": 1,
                  "sloc_average": 6.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.5,
                  "blank_average": 0.5,
                  "sloc_min": 12,
                  "sloc_max": 12,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_cloc() {
        check_metrics::<PythonParser>(
            "\"\"\"Block comment
            Block comment
            \"\"\"
            # Line Comment
            a = 42 # Line Comment",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 1,
                  "lloc": 2,
                  "cloc": 5,
                  "blank": 0,
                  "sloc_average": 5.0,
                  "ploc_average": 1.0,
                  "lloc_average": 2.0,
                  "cloc_average": 5.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_cloc() {
        check_metrics::<RustParser>(
            "/*Block comment
            Block Comment*/
            //Line Comment
            /*Block Comment*/ let a = 42; // Line Comment",
            "foo.rs",
            |metric| {
                // Spaces: 1
                // expected: cloc = 4 — the 2-line block (lines 1-2) and
                // the standalone `//Line Comment` (line 3) give 3
                // only-comment lines; line 4 carries a leading block
                // comment AND a trailing line comment but is one
                // physical code line, so it adds a single code-comment
                // line, not two (issue #461). Pre-fix this reported 5,
                // violating cloc <= sloc (sloc = 4).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 1,
                  "lloc": 1,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 1.0,
                  "lloc_average": 1.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_cloc() {
        check_metrics::<CParser>(
            "/*Block comment
            Block Comment*/
            //Line Comment
            /*Block Comment*/ int a = 42; // Line Comment",
            "foo.c",
            |metric| {
                // Spaces: 1
                // expected: cloc = 4 — see `rust_cloc`; line 4's leading
                // block comment and trailing line comment share one
                // physical code line and add a single code-comment line
                // (issue #461). Pre-fix reported 5 (cloc > sloc = 4).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 1,
                  "lloc": 1,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 1.0,
                  "lloc_average": 1.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_lloc() {
        check_metrics::<PythonParser>(
            "for x in range(0,42):
                if x % 2 == 0:
                    print(x)",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_lloc() {
        check_metrics::<RustParser>(
            "for x in 0..42 {
                if x % 2 == 0 {
                    println!(\"{}\", x);
                }
             }",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 5.0,
                  "ploc_average": 5.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );

        // LLOC returns three because there is an empty Rust statement
        check_metrics::<RustParser>(
            "let a = 42;
             if true {
                42
             } else {
                43
             };",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 6,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 6.0,
                  "ploc_average": 6.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 6,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn c_lloc() {
        check_metrics::<CParser>(
            "for (;;)
                break;",
            "foo.c",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 2,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 2,
                  "sloc_max": 2,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_lloc() {
        check_metrics::<CppParser>(
            "nsTArray<xpcGCCallback> callbacks(extraGCCallbacks.Clone());
             for (uint32_t i = 0; i < callbacks.Length(); ++i) {
                 callbacks[i](status);
             }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: nsTArray, for, callbacks
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_return_lloc() {
        check_metrics::<CppParser>(
            "uint8_t* pixel_data = frame.GetFrameDataAtPos(DesktopVector(x, y));
             return RgbaColor(pixel_data) == blank_pixel_;",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: pixel_data, return
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 2,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 2,
                  "sloc_max": 2,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_for_lloc() {
        check_metrics::<CppParser>(
            "for (; start != end; ++start) {
                 const unsigned char idx = *start;
                 if (idx > 127 || !kValidTokenMap[idx]) return false;
             }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: for, idx, if, return
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 4,
                  "lloc": 4,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 4.0,
                  "lloc_average": 4.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_while_lloc() {
        check_metrics::<CppParser>(
            "while (sHeapAtoms) {
                 HttpHeapAtom* next = sHeapAtoms->next;
                 free(sHeapAtoms);
            }",
            "foo.cpp",
            |metric| {
                // Spaces: 1
                // lloc: while, next, free
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_string_on_new_line() {
        // More lines of the same instruction were counted as blank lines
        check_metrics::<PythonParser>(
            "capabilities[\"goog:chromeOptions\"][\"androidPackage\"] = \\
                \"org.chromium.weblayer.shell\"",
            "foo.py",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 2,
                  "ploc": 2,
                  "lloc": 1,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 2,
                  "sloc_max": 2,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_multiline_string_assignment_ploc() {
        // Regression test for issue #415: interior rows of a multi-line,
        // non-docstring string literal were mis-counted as blank lines.
        check_metrics::<PythonParser>(
            "QUERY = \"\"\"
SELECT id, name
FROM users
WHERE active = 1
ORDER BY name
\"\"\"",
            "foo.py",
            |metric| {
                // Spaces: 1. Six physical lines, all code, no blanks.
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 6,
                  "lloc": 1,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 6.0,
                  "ploc_average": 6.0,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 6,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_multiline_string_argument_ploc() {
        // Regression test for issue #415: a multi-line string passed as a call
        // argument must have all its rows counted as code, not blank.
        check_metrics::<PythonParser>(
            "print(\"\"\"
line one
line two
\"\"\")",
            "foo.py",
            |metric| {
                // Spaces: 1. Four physical lines, all code, no blanks.
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn python_single_line_string_assignment_ploc() {
        // Single-line, non-docstring string: behaviour must be unchanged by
        // the issue #415 fix (start == end means no extra rows are inserted).
        check_metrics::<PythonParser>("QUERY = \"SELECT 1\"", "foo.py", |metric| {
            // Spaces: 1.
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
        });
    }

    #[test]
    fn python_multiline_docstring_still_cloc() {
        // The fix for issue #415 must leave docstring classification unchanged:
        // a bare triple-quoted string statement is still counted as comments.
        check_metrics::<PythonParser>(
            "def f():
    \"\"\"Docstring line one
    Docstring line two
    \"\"\"
    return 1",
            "foo.py",
            |metric| {
                // Spaces: 2 (module + function). The three docstring rows are
                // comment lines, not code or blank lines.
                assert_eq!(metric.loc.cloc(), 3);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn rust_no_field_expression_lloc() {
        check_metrics::<RustParser>(
            "struct Foo {
                field: usize,
             }
             let foo = Foo { 42 };
             foo.field;",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 5.0,
                  "ploc_average": 5.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_no_parenthesized_expression_lloc() {
        check_metrics::<RustParser>("let a = (42 + 0);", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_array_expression_lloc() {
        check_metrics::<RustParser>("let a = [0; 42];", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_tuple_expression_lloc() {
        check_metrics::<RustParser>("let a = (0, 42);", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_unit_expression_lloc() {
        check_metrics::<RustParser>("let a = ();", "foo.rs", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_call_function_lloc() {
        check_metrics::<RustParser>(
            "let a = foo(); // +1
             foo(); // +1
             k!(foo()); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 3.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_macro_invocation_lloc() {
        check_metrics::<RustParser>(
            "let a = foo!(); // +1
             foo!(); // +1
             k(foo!()); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 3.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_function_in_loop_lloc() {
        check_metrics::<RustParser>(
            "for (a, b) in c.iter().enumerate() {} // +1
             while (a, b) in c.iter().enumerate() {} // +1
             while let Some(a) = c.strip_prefix(\"hi\") {} // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 3.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_function_in_if_lloc() {
        check_metrics::<RustParser>(
            "if foo() {} // +1
             if let Some(a) = foo() {} // +1",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 2,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 2,
                  "sloc_max": 2,
                  "cloc_min": 2,
                  "cloc_max": 2,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_function_in_return_lloc() {
        check_metrics::<RustParser>(
            "return foo();
             await foo();",
            "foo.rs",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 2,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 2,
                  "sloc_max": 2,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_closure_expression_lloc() {
        check_metrics::<RustParser>(
            "let a = |i: i32| -> i32 { i + 1 }; // +1
             a(42); // +1
             k(b.iter().map(|n| n.parse.ok().unwrap_or(42))); // +1",
            "foo.rs",
            |metric| {
                // Spaces: 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 0,
                  "sloc_average": 1.0,
                  "ploc_average": 1.0,
                  "lloc_average": 1.0,
                  "cloc_average": 1.0,
                  "blank_average": 0.0,
                  "sloc_min": 1,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 3,
                  "ploc_min": 1,
                  "ploc_max": 3,
                  "lloc_min": 0,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_general_loc() {
        check_metrics::<PythonParser>(
            "def func(a,
                      b,
                      c):
                 print(a)
                 print(b)
                 print(c)",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 6,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 1.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 6,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn python_real_loc() {
        check_metrics::<PythonParser>(
            "def web_socket_transfer_data(request):
                while True:
                    line = request.ws_stream.receive_message()
                    if line is None:
                        return
                    code, reason = line.split(' ', 1)
                    if code is None or reason is None:
                        return
                    request.ws_stream.close_connection(int(code), reason)
                    # close_connection() initiates closing handshake. It validates code
                    # and reason. If you want to send a broken close frame for a test,
                    # following code will be useful.
                    # > data = struct.pack('!H', int(code)) + reason.encode('UTF-8')
                    # > request.connection.write(stream.create_close_frame(data))
                    # > # Suppress to re-respond client responding close frame.
                    # > raise Exception(\"customized server initiated closing handshake\")",
            "foo.py",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 16,
                  "ploc": 9,
                  "lloc": 8,
                  "cloc": 7,
                  "blank": 0,
                  "sloc_average": 8.0,
                  "ploc_average": 4.5,
                  "lloc_average": 4.0,
                  "cloc_average": 3.5,
                  "blank_average": 0.0,
                  "sloc_min": 16,
                  "sloc_max": 16,
                  "cloc_min": 7,
                  "cloc_max": 7,
                  "ploc_min": 9,
                  "ploc_max": 9,
                  "lloc_min": 8,
                  "lloc_max": 8,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_real_loc() {
        check_metrics::<JavascriptParser>(
            "assert.throws(Test262Error, function() {
               for (let { poisoned: x = ++initEvalCount } = poisonedProperty; ; ) {
                 return;
               }
             });",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 4,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.5,
                  "ploc_average": 2.5,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 3,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_real_loc() {
        check_metrics::<MozjsParser>(
            "assert.throws(Test262Error, function() {
               for (let { poisoned: x = ++initEvalCount } = poisonedProperty; ; ) {
                 return;
               }
             });",
            "foo.js",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 4,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.5,
                  "ploc_average": 2.5,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 3,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_blank_and_comment_loc() {
        check_metrics::<MozjsParser>(
            "// a comment
             function f() {

                 var x = 1;

             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 3,
                  "lloc": 1,
                  "cloc": 1,
                  "blank": 2,
                  "sloc_average": 3.0,
                  "ploc_average": 1.5,
                  "lloc_average": 0.5,
                  "cloc_average": 0.5,
                  "blank_average": 1.0,
                  "sloc_min": 5,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 1,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 2,
                  "blank_max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn cpp_namespace_loc() {
        check_metrics::<CppParser>(
            "namespace mozilla::dom::quota {} // namespace mozilla::dom::quota",
            "foo.cpp",
            |metric| {
                // Spaces: 2
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 1,
                  "ploc": 1,
                  "lloc": 0,
                  "cloc": 1,
                  "blank": 0,
                  "sloc_average": 0.5,
                  "ploc_average": 0.5,
                  "lloc_average": 0.0,
                  "cloc_average": 0.5,
                  "blank_average": 0.0,
                  "sloc_min": 1,
                  "sloc_max": 1,
                  "cloc_min": 0,
                  "cloc_max": 1,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 0,
                  "lloc_max": 0,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_comments() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { \
               // Print hello
               System.out.println(\"hello\"); \
               // Print world
               System.out.println(\"hello\"); \
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 2,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 2,
                  "cloc_max": 2,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_blank() {
        check_metrics::<JavaParser>(
            "int x = 1;


            int y = 2;",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 2,
                  "sloc_average": 4.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 2.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 2,
                  "blank_max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_sloc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_module_sloc() {
        check_metrics::<JavaParser>(
            "module helloworld{
              exports com.test;
            }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 0,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 0.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 0,
                  "lloc_max": 0,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_single_ploc() {
        check_metrics::<JavaParser>("int x = 1;", "foo.java", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn java_simple_ploc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i = i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_multi_ploc() {
        check_metrics::<JavaParser>(
            "int x = 1;
            for (int i = 0; i < 100; i++) {
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_single_statement_lloc() {
        check_metrics::<JavaParser>("int max = 10;", "foo.java", |metric| {
            // Spaces: 1
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
            {
              "sloc": 1,
              "ploc": 1,
              "lloc": 1,
              "cloc": 0,
              "blank": 0,
              "sloc_average": 1.0,
              "ploc_average": 1.0,
              "lloc_average": 1.0,
              "cloc_average": 0.0,
              "blank_average": 0.0,
              "sloc_min": 1,
              "sloc_max": 1,
              "cloc_min": 0,
              "cloc_max": 0,
              "ploc_min": 1,
              "ploc_max": 1,
              "lloc_min": 1,
              "lloc_max": 1,
              "blank_min": 0,
              "blank_max": 0
            }
            "#
            );
        });
    }

    #[test]
    fn java_for_lloc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { // + 1
               System.out.println(i); // + 1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 2.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 2,
                  "cloc_max": 2,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_foreach_lloc() {
        check_metrics::<JavaParser>(
            "
            int arr[]={12,13,14,44}; // +1
            for (int i:arr) { // +1
               System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 4,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 3.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_while_lloc() {
        check_metrics::<JavaParser>(
            "
            int i=0; // +1
            while(i < 10) { // +1
                i++; // +1
                System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 4,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 5.0,
                  "ploc_average": 5.0,
                  "lloc_average": 4.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_do_while_lloc() {
        check_metrics::<JavaParser>(
            "
            int i=0; // +1
            do { // +1
                i++; // +1
                System.out.println(i); // +1
             } while(i < 10)",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 4,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 5.0,
                  "ploc_average": 5.0,
                  "lloc_average": 4.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_switch_lloc() {
        check_metrics::<JavaParser>(
            "switch(grade) { // +1
                case 'A' :
                   System.out.println(\"Pass with distinction\"); // +1
                   break; // +1
                case 'B' :
                case 'C' :
                   System.out.println(\"Pass\"); // +1
                   break; // +1
                case 'D' :
                   System.out.println(\"At risk\"); // +1
                case 'F' :
                   System.out.println(\"Fail\"); // +1
                   break; // +1
                default :
                   System.out.println(\"Invalid grade\"); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 16,
                  "ploc": 16,
                  "lloc": 9,
                  "cloc": 9,
                  "blank": 0,
                  "sloc_average": 16.0,
                  "ploc_average": 16.0,
                  "lloc_average": 9.0,
                  "cloc_average": 9.0,
                  "blank_average": 0.0,
                  "sloc_min": 16,
                  "sloc_max": 16,
                  "cloc_min": 9,
                  "cloc_max": 9,
                  "ploc_min": 16,
                  "ploc_max": 16,
                  "lloc_min": 9,
                  "lloc_max": 9,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_continue_lloc() {
        check_metrics::<JavaParser>(
            "int max = 10; // +1

            for (int i = 0; i < max; i++) { // +1
                if(i % 2 == 0) { continue;} + 2
                System.out.println(i); // +1
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 5,
                  "lloc": 5,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 6.0,
                  "ploc_average": 5.0,
                  "lloc_average": 5.0,
                  "cloc_average": 3.0,
                  "blank_average": 1.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 3,
                  "cloc_max": 3,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 5,
                  "lloc_max": 5,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_try_lloc() {
        check_metrics::<JavaParser>(
            "try { // +1
                int[] myNumbers = {1, 2, 3}; // +1
                System.out.println(myNumbers[10]); // +1
              } catch (Exception e) {
                System.out.println(e.getMessage()); // +1
                throw e; // +1
              }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 7,
                  "ploc": 7,
                  "lloc": 5,
                  "cloc": 5,
                  "blank": 0,
                  "sloc_average": 7.0,
                  "ploc_average": 7.0,
                  "lloc_average": 5.0,
                  "cloc_average": 5.0,
                  "blank_average": 0.0,
                  "sloc_min": 7,
                  "sloc_max": 7,
                  "cloc_min": 5,
                  "cloc_max": 5,
                  "ploc_min": 7,
                  "ploc_max": 7,
                  "lloc_min": 5,
                  "lloc_max": 5,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_class_loc() {
        check_metrics::<JavaParser>(
            "
            public class Person {
              private String name;
              public Person(String name){
                this.name = name; // +1
              }
              public String getName() {
                return name; // +1
              }
            }",
            "foo.java",
            |metric| {
                // Spaces: 4
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 9,
                  "ploc": 9,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 0,
                  "sloc_average": 2.25,
                  "ploc_average": 2.25,
                  "lloc_average": 0.5,
                  "cloc_average": 0.5,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 9,
                  "cloc_min": 1,
                  "cloc_max": 2,
                  "ploc_min": 3,
                  "ploc_max": 9,
                  "lloc_min": 1,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_expressions_lloc() {
        check_metrics::<JavaParser>(
            "int x = 10;                                                            // +1 local var declaration
            x=+89;                                                                  // +1 expression statement
            int y = x * 2;                                                          // +1 local var declaration
            IntFunction double = (n) -> n*2;                                        // +1 local var declaration
            int y2 = double(x);                                                     // +1 local var declaration
            System.out.println(\"double \" + x + \" = \" + y2);                     // +1 expression statement
            String message = (x % 2) == 0 ? \"Evenly done.\" : \"Oddly done.\";     // +1 local var declaration
            Object done = (Runnable) () -> { System.out.println(\"Done!\"); };      // +2 local var declaration + expression statement
            String s = \"string\";                                                  // +1 local var declaration
            boolean isS = (s instanceof String);                                    // +1 local var declaration
            done.run();                                                             // +1 expression statement
            ",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 11,
                  "lloc": 12,
                  "cloc": 11,
                  "blank": 0,
                  "sloc_average": 11.0,
                  "ploc_average": 11.0,
                  "lloc_average": 12.0,
                  "cloc_average": 11.0,
                  "blank_average": 0.0,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 11,
                  "cloc_max": 11,
                  "ploc_min": 11,
                  "ploc_max": 11,
                  "lloc_min": 12,
                  "lloc_max": 12,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_statement_inline_loc() {
        check_metrics::<JavaParser>(
            "for (int i = 0; i < 100; i++) { System.out.println(\"hello\"); }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 1,
                  "ploc": 1,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 1.0,
                  "ploc_average": 1.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 1,
                  "sloc_max": 1,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_general_loc() {
        check_metrics::<JavaParser>(
            "int max = 100;

            /*
              Loop through and print
                from: 0
                to: max
            */
            for (int i = 0; i < max; i++) {
               // Print the value
               System.out.println(i);
             }",
            "foo.java",
            |metric| {
                // Spaces: 1
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 6,
                  "blank": 1,
                  "sloc_average": 11.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 6.0,
                  "blank_average": 1.0,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 6,
                  "cloc_max": 6,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_main_class_loc() {
        check_metrics::<JavaParser>(
            "package com.company;
             /**
             * The HelloWorldApp class implements an application that
             * simply prints \"Hello World!\" to standard output.
             */

            class HelloWorldApp {
              public void main(String[] args) {
                String message = args.length == 0 ? \"Hello empty world\" : \"Hello world\"; // +1 lloc : 1 var assignment
                System.out.println(message); // Display the string. +1 lloc
              }
            }",
            "foo.java",
            |metric| {
                // Spaces: 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 12,
                  "ploc": 7,
                  "lloc": 2,
                  "cloc": 6,
                  "blank": 1,
                  "sloc_average": 4.0,
                  "ploc_average": 2.3333333333333335,
                  "lloc_average": 0.6666666666666666,
                  "cloc_average": 2.0,
                  "blank_average": 0.3333333333333333,
                  "sloc_min": 4,
                  "sloc_max": 12,
                  "cloc_min": 2,
                  "cloc_max": 6,
                  "ploc_min": 4,
                  "ploc_max": 7,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_general_loc() {
        check_metrics::<GoParser>(
            "package main

            // entrypoint
            func main() {
                /* loop body */
                for i := 0; i < 10; i++ {
                    fmt.Println(i)
                }
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + main).
                // lloc: for_statement (+1), fmt.Println expression (+1).
                //       `i := 0` and `i++` inside the for-clause are gated.
                // cloc: 2 comments (line + block).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 9,
                  "ploc": 6,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 1,
                  "sloc_average": 4.5,
                  "ploc_average": 3.0,
                  "lloc_average": 1.0,
                  "cloc_average": 1.0,
                  "blank_average": 0.5,
                  "sloc_min": 6,
                  "sloc_max": 9,
                  "cloc_min": 1,
                  "cloc_max": 2,
                  "ploc_min": 5,
                  "ploc_max": 6,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_for_clause_does_not_double_count_lloc() {
        // Bare `for` body has only a return; the `for_statement` itself is the
        // single logical line. Confirms ShortVarDeclaration in a for-clause
        // does not add an extra lloc.
        check_metrics::<GoParser>(
            "package main
            func f(n int) int {
                for i := 0; i < n; i++ {
                    return i
                }
                return 0
            }",
            "foo.go",
            |metric| {
                // Expected lloc: for (+1), return (+1), return (+1) = 3.
                // Without the gate, ShortVarDeclaration would add an extra (+1).
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn go_blank() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                x := 1

                y := 2
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // blank: 2 (lines 2 and 5 are empty).
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 7,
                  "ploc": 5,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 2,
                  "sloc_average": 3.5,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 5,
                  "sloc_max": 7,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 5,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_cloc_line_comments() {
        check_metrics::<GoParser>(
            "package main

            // helper adds two numbers.
            // It returns their sum.
            func add(a, b int) int {
                // compute the result
                return a + b
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + add).
                // cloc: 3 lines with `//` comments.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8,
                  "ploc": 4,
                  "lloc": 1,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 4.0,
                  "ploc_average": 2.0,
                  "lloc_average": 0.5,
                  "cloc_average": 1.5,
                  "blank_average": 0.5,
                  "sloc_min": 4,
                  "sloc_max": 8,
                  "cloc_min": 1,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 4,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_cloc_block_comments() {
        check_metrics::<GoParser>(
            "package main

            /* block comment
               spanning two lines */
            func foo() {
                x := 1 /* inline block */
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // cloc: 2-line block comment + inline block = 3 comment lines.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 7,
                  "ploc": 4,
                  "lloc": 1,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 3.5,
                  "ploc_average": 2.0,
                  "lloc_average": 0.5,
                  "cloc_average": 1.5,
                  "blank_average": 0.5,
                  "sloc_min": 3,
                  "sloc_max": 7,
                  "cloc_min": 1,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 4,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_if_for_switch() {
        check_metrics::<GoParser>(
            "package main

            func foo(n int) int {
                if n > 0 {
                    for i := 0; i < n; i++ {
                        switch i {
                        }
                    }
                }
                return n
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: if (+1), for (+1), switch (+1), return (+1) = 4.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 10,
                  "lloc": 4,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 5.5,
                  "ploc_average": 5.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 9,
                  "sloc_max": 11,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 9,
                  "ploc_max": 10,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_go_defer() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                go run()
                defer cleanup()
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: go (+1), defer (+1) = 2.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 5,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 4,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 4,
                  "ploc_max": 5,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_var_const_declarations() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                var x int
                var y = 10
                const z = 42
                a := 3
                a = 4
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: var (+1), var (+1), const (+1),
                //       short_var_decl (+1), assignment (+1) = 5.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 9,
                  "ploc": 8,
                  "lloc": 5,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 4.5,
                  "ploc_average": 4.0,
                  "lloc_average": 2.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 7,
                  "sloc_max": 9,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 7,
                  "ploc_max": 8,
                  "lloc_min": 5,
                  "lloc_max": 5,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_lloc_select() {
        check_metrics::<GoParser>(
            "package main

            func foo(ch chan int) {
                select {
                case v := <-ch:
                    _ = v
                }
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // lloc: select (+1), assignment `_ = v` (+1) = 2.
                // `case v := <-ch:` is a receive_statement inside a
                // communication_case, not a ShortVarDeclaration.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8,
                  "ploc": 7,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 4.0,
                  "ploc_average": 3.5,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 6,
                  "sloc_max": 8,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 7,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_sloc_multiline_function() {
        check_metrics::<GoParser>(
            "package main

            func add(
                a int,
                b int,
            ) int {
                return a + b
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + add).
                // The multi-line signature should count each line as sloc.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8,
                  "ploc": 7,
                  "lloc": 1,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 4.0,
                  "ploc_average": 3.5,
                  "lloc_average": 0.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 6,
                  "sloc_max": 8,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 7,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn go_code_comment_same_line() {
        check_metrics::<GoParser>(
            "package main

            func foo() {
                x := 1 // initialize x
                y := 2 // initialize y
            }",
            "foo.go",
            |metric| {
                // Spaces: 2 (unit + foo).
                // cloc: 2 (inline comments on code lines).
                // blank: 1 (line between package and func).
                // The code+comment lines should count for both ploc and cloc.
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 5,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.5,
                  "lloc_average": 1.0,
                  "cloc_average": 1.0,
                  "blank_average": 0.5,
                  "sloc_min": 4,
                  "sloc_max": 6,
                  "cloc_min": 2,
                  "cloc_max": 2,
                  "ploc_min": 4,
                  "ploc_max": 5,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_grammar_smoke() {
        // Pin the contract that tree-sitter-perl 1.1.2 cleanly parses every
        // Perl construct exercised by the rest of the `perl_*` test suite.
        // If a future grammar bump turns one of these into an error tree,
        // the metric assertions might still pass numerically by coincidence;
        // this test fails loudly instead.
        assert_perl_parses_cleanly(
            "use strict;
use warnings;

# line comment

=pod
multi-line POD
=cut

sub factorial {
    my ($n) = @_;
    return 1 if $n <= 1;
    return $n * factorial($n - 1);
}

my @arr = (1, 2, 3);
my %hash = (a => 1, b => 2);
my $closure = sub { return $_[0] + 1; };

for my $i (1..3) {
    if ($i % 2 == 0) {
        print \"even\\n\";
    } elsif ($i == 1) {
        print \"one\\n\";
    } else {
        print \"odd\\n\";
    }
}

while ($x > 0) {
    last if $x == 0;
    $x--;
}

unless ($done) {
    next;
}

my $heredoc = <<END;
hello
END
",
        );
    }

    #[test]
    fn perl_blank() {
        check_metrics::<PerlParser>(
            "

my $a = 42;

my $b = 43;

",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<PerlParser>(
            "my $a = 1;
my $b = 2;

my $c = 3; # trailing
my $d = 4; # trailing
my $e = 5;",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn perl_blank_zero_sanity() {
        // Sanity check: blank must report 0, never go negative, when the
        // input has no blank lines.
        check_metrics::<PerlParser>(
            "my $a = 1;
my $b = 2;",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// expected: row 0 is comment-only, row 1 is code carrying a trailing
    /// comment, row 2 is code — so `ploc 2` and `cloc 2`, with row 1 in
    /// both tallies. This pinned `ploc 3` until #1137: the `#` token
    /// inside the `comments` node reached the PLOC catch-all, which also
    /// reclassified row 0 from comment-only to code-and-comment.
    #[test]
    fn perl_cloc_line_comments() {
        check_metrics::<PerlParser>(
            "# top comment
my $a = 1; # trailing
my $b = 2;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 2,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 2,
                  "cloc_max": 2,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_cloc_pod_block() {
        check_metrics::<PerlParser>(
            "my $x = 1;
=pod
multi-line
pod block
=cut
my $y = 2;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 6,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 6.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_simple_statements() {
        check_metrics::<PerlParser>(
            "my $a = 1;
my $b = 2;
my $c = 3;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_compound_statements() {
        check_metrics::<PerlParser>(
            "if ($x) {
    print 'a';
}
while ($n > 0) {
    $n--;
}",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 6,
                  "ploc": 6,
                  "lloc": 4,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 6.0,
                  "ploc_average": 6.0,
                  "lloc_average": 4.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 6,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 6,
                  "ploc_max": 6,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_postfix_form_counts_once() {
        // `do_thing() if cond;` is one logical line — wrapped in
        // single_line_statement; the inner if_simple_statement does not
        // add a second LLOC.
        check_metrics::<PerlParser>(
            "sub f {
    return 1 if $_[0];
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn perl_lloc_use_statement() {
        check_metrics::<PerlParser>(
            "use strict;
use warnings;
my $x = 1;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_lloc_for_loop() {
        check_metrics::<PerlParser>(
            "for my $i (1..3) {
    print $i;
}",
            "foo.pl",
            |metric| {
                // `for_statement_2` (+1) and `print …;` SEMI in block (+1) → 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn perl_lloc_loop_control_statement() {
        check_metrics::<PerlParser>(
            "while (1) {
    last if $done;
}",
            "foo.pl",
            |metric| {
                // while_statement (+1) + loop_control_statement (+1) = 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn perl_lloc_no_double_count_inside_single_line_statement() {
        // SEMI inside a single_line_statement (postfix form) is a child of
        // if_simple_statement, not Block — so it must not add a second LLOC.
        check_metrics::<PerlParser>(
            "sub f {
    print 'a' unless $_[0];
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn perl_lloc_function_definition_not_counted() {
        // `sub f { ... }` itself is a function space, not an LLOC; only its
        // body statements count.
        check_metrics::<PerlParser>(
            "sub f {
    my $x = 1;
}",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn perl_lloc_anonymous_function() {
        // `my $f = sub { return 1; };` — the assignment is one LLOC at the
        // top level (the SEMI after `};`); the `return 1;` inside the
        // anonymous function block is a second LLOC inside the closure.
        check_metrics::<PerlParser>("my $f = sub { return 1; };", "foo.pl", |metric| {
            assert_eq!(metric.loc.lloc(), 2);
        });
    }

    #[test]
    fn perl_multiline_string_assignment_ploc() {
        // Regression test for issue #778: interior rows of a multi-line string
        // literal are real code, not blank lines, and must be credited to PLOC
        // exactly as Python does (#415). Previously Perl no-op'd its string
        // kinds, so row 1 reached neither PLOC nor CLOC and `blank =
        // sloc - ploc - cloc` mislabelled it as blank (ploc was 2, blank 1).
        // Row 0 holds `my $s = "line1`, row 1 `line2`, row 2 `line3";`.
        check_metrics::<PerlParser>(
            "my $s = \"line1
line2
line3\";",
            "foo.pl",
            |metric| {
                // Three physical rows, all code, no blanks — matching Python.
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn multiline_string_ploc_consistent_across_languages() {
        // Cross-language parity for issue #778: the SAME 3-line string
        // assignment must report identical ploc / blank in every language
        // that has a multi-line string literal. The canonical value is
        // Python's #415 decision: all three rows are code, none are blank.
        // `check_metrics` takes a plain `fn(CodeMetrics)`, so the shared
        // assertion is a named function rather than a capturing closure and
        // must take its argument by value to match that pointer type.
        #[allow(clippy::needless_pass_by_value)]
        fn assert_three_code_rows(metric: crate::CodeMetrics) {
            assert_eq!(metric.loc.sloc(), 3);
            assert_eq!(metric.loc.ploc(), 3);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
        }
        check_metrics::<PythonParser>(
            "s = \"\"\"line1\nline2\nline3\"\"\"",
            "foo.py",
            assert_three_code_rows,
        );
        check_metrics::<PerlParser>(
            "my $s = \"line1\nline2\nline3\";",
            "foo.pl",
            assert_three_code_rows,
        );
        check_metrics::<RubyParser>(
            "s = \"line1\nline2\nline3\"",
            "foo.rb",
            assert_three_code_rows,
        );
        // Go, Kotlin, and Mozilla-C++ reach the same shared
        // `add_multiline_string_ploc` helper through their own
        // raw-string kinds (`raw_string_literal`,
        // `multiline_string_literal`, `raw_string_literal`), and were
        // the three call sites of it that no test exercised. Each needs
        // its own syntax, so they cannot reuse the quoted form above.
        check_metrics::<GoParser>(
            "package p\n\nvar s = `line1\nline2\nline3`",
            "foo.go",
            |metric| {
                // Two extra code rows for `package p` and the blank
                // between it and the declaration, which Go requires.
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
            },
        );
        check_metrics::<KotlinParser>(
            "val s = \"\"\"line1\nline2\nline3\"\"\"",
            "foo.kt",
            assert_three_code_rows,
        );
        check_metrics::<MozcppParser>(
            "const char* s = R\"(line1\nline2\nline3)\";",
            "foo.cpp",
            assert_three_code_rows,
        );
    }

    #[test]
    fn perl_lloc_unless_until() {
        check_metrics::<PerlParser>(
            "unless ($x) {
    print 'a';
}
until ($n == 0) {
    $n--;
}",
            "foo.pl",
            |metric| {
                // unless_statement (+1) + print SEMI (+1) + until_statement (+1)
                // + $n-- SEMI (+1) = 4
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    #[test]
    fn perl_lloc_heredoc_body_not_counted() {
        // Heredoc body content is data, not code: the body lines should not
        // contribute LLOC or PLOC.
        check_metrics::<PerlParser>(
            "my $s = <<END;
line1
line2
END
my $x = 1;",
            "foo.pl",
            |metric| {
                // Two top-level statements: the heredoc-using `my $s = …;`
                // and `my $x = 1;`.
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
        // Independent confirmation that the snippet is a valid heredoc and
        // not silently parsed as an error tree (which could otherwise yield
        // the same `lloc == 2.0` and mask a grammar regression).
        assert_perl_parses_cleanly(
            "my $s = <<END;
line1
line2
END
my $x = 1;",
        );
    }

    #[test]
    fn perl_lloc_package_and_require() {
        check_metrics::<PerlParser>(
            "package Foo;
require 5.010;
my $x = 1;",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 3,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.0,
                  "ploc_average": 3.0,
                  "lloc_average": 3.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 3,
                  "ploc_max": 3,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn lua_blank() {
        check_metrics::<LuaParser>(
            "local x = 1

local y = 2",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<LuaParser>(
            "local a = 1
local b = 2

local c = 3 -- trailing
local d = 4 -- trailing
local e = 5",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_blank_zero_sanity() {
        // Sanity check: blank must report 0, never go negative, when the
        // input has no blank lines.
        check_metrics::<LuaParser>(
            "local x = 1
local y = 2",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn lua_cloc() {
        check_metrics::<LuaParser>(
            "-- single line comment
local x = 1
--[[
  block comment
  second line
]]",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 1);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 5);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_lloc() {
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    local y = x + 1
    return y
  end
  return 0
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_string_lloc() {
        // Long strings spanning multiple lines must not inflate lloc.
        check_metrics::<LuaParser>(
            "local s = [[
  line one
  line two
]]",
            "foo.lua",
            |metric| {
                // #778: a multi-line long-bracket string credits every spanned
                // row to PLOC (matching Python's #415 decision), so all four
                // rows are code and none are blank. It still contributes a
                // single lloc — the assignment statement.
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_functiondefinition_lloc() {
        // Anonymous function definition is an expression, not a statement.
        // The containing variable_declaration counts as lloc; FunctionDefinition must not.
        check_metrics::<LuaParser>(
            "local f = function(x)
  return x + 1
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_elseif_lloc() {
        // elseif_statement must not add lloc; only if_statement does.
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    return 1
  elseif x < 0 then
    return -1
  else
    return 0
  end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9);
                assert_eq!(metric.loc.ploc(), 9);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_no_else_lloc() {
        // else_statement must not add lloc.
        check_metrics::<LuaParser>(
            "local function f(x)
  if x > 0 then
    return 1
  else
    return 0
  end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_functiondeclaration_lloc() {
        // Named function declaration counts as one lloc.
        check_metrics::<LuaParser>(
            "function f()
  return 1
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_local_function_lloc() {
        // local function declaration is also a function_declaration node → one lloc.
        check_metrics::<LuaParser>(
            "local function g()
  return 2
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_for_numeric_lloc() {
        check_metrics::<LuaParser>(
            "for i = 1, 10 do
  print(i)
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_for_generic_lloc() {
        check_metrics::<LuaParser>(
            "for k, v in pairs(t) do
  print(k, v)
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_repeat_lloc() {
        check_metrics::<LuaParser>(
            "local i = 0
repeat
  i = i + 1
until i >= 10",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_local_decl_lloc() {
        check_metrics::<LuaParser>(
            "local x = 1
local y, z = 2, 3",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_function_call_lloc() {
        // Standalone function calls have no expression_statement wrapper in Lua.
        // They fall to the `_` branch → counted as ploc, not lloc.
        check_metrics::<LuaParser>(
            "print(\"hello\")
local x = 1",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_toplevel_assignment_lloc() {
        // Bare `x = 1` at chunk level: parent is Chunk, not VariableDeclaration,
        // so the parent-guard correctly counts it as 1 lloc.
        check_metrics::<LuaParser>(
            "x = 1
y, z = 2, 3",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_basic_loc() {
        check_metrics::<TsxParser>(
            "// A simple utility function
            function add(a: number, b: number): number {
                /* multi-line
                   comment */
                return a + b;
            }

            const greet = (name: string) => {
                return `Hello, ${name}`;
            };",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 10,
                  "ploc": 6,
                  "lloc": 3,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 3.3333333333333335,
                  "ploc_average": 2.0,
                  "lloc_average": 1.0,
                  "cloc_average": 1.0,
                  "blank_average": 0.3333333333333333,
                  "sloc_min": 3,
                  "sloc_max": 10,
                  "cloc_min": 0,
                  "cloc_max": 3,
                  "ploc_min": 3,
                  "ploc_max": 6,
                  "lloc_min": 1,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_basic_loc() {
        check_metrics::<TypescriptParser>(
            "// Line comment
            /* Block
               comment */
            function greet(name: string): string {
                return `Hello, ${name}`;
            }

            const add = (a: number, b: number): number => a + b;",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8,
                  "ploc": 4,
                  "lloc": 2,
                  "cloc": 3,
                  "blank": 1,
                  "sloc_average": 2.6666666666666665,
                  "ploc_average": 1.3333333333333333,
                  "lloc_average": 0.6666666666666666,
                  "cloc_average": 1.0,
                  "blank_average": 0.3333333333333333,
                  "sloc_min": 1,
                  "sloc_max": 8,
                  "cloc_min": 0,
                  "cloc_max": 3,
                  "ploc_min": 1,
                  "ploc_max": 4,
                  "lloc_min": 0,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn csharp_comments() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               // Print hello
               System.Console.WriteLine(\"hello\");
               /// XML doc comment
               System.Console.WriteLine(\"hello\");
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_blank() {
        check_metrics::<CsharpParser>(
            "int x = 1;


            int y = 2;",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_sloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_module_sloc() {
        check_metrics::<CsharpParser>(
            "namespace HelloWorld {
              class Program { }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 0);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_single_ploc() {
        check_metrics::<CsharpParser>("int x = 1;", "foo.cs", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn csharp_simple_ploc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_multi_ploc() {
        check_metrics::<CsharpParser>(
            "int x = 1;
            for (int i = 0; i < 100; i++) {
               System.Console.WriteLine(i);
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_single_statement_lloc() {
        check_metrics::<CsharpParser>("int max = 10;", "foo.cs", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn csharp_for_lloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 10; i++) {
                System.Console.WriteLine(i);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_foreach_lloc() {
        check_metrics::<CsharpParser>(
            "foreach (var item in items) {
                System.Console.WriteLine(item);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_while_lloc() {
        check_metrics::<CsharpParser>(
            "int i = 0;
            while (i < 10) {
                i++;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_do_while_lloc() {
        check_metrics::<CsharpParser>(
            "int i = 0;
            do {
                i++;
            } while (i < 10);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_switch_lloc() {
        check_metrics::<CsharpParser>(
            "switch (x) {
                case 1: System.Console.WriteLine(1); break;
                case 2: System.Console.WriteLine(2); break;
                default: System.Console.WriteLine(0); break;
            }
            string s = x switch { 1 => \"one\", _ => \"other\" };",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 8);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_continue_lloc() {
        check_metrics::<CsharpParser>(
            "for (int i = 0; i < 10; i++) {
                if (i == 5) continue;
                System.Console.WriteLine(i);
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_try_lloc() {
        check_metrics::<CsharpParser>(
            "try {
                System.Console.WriteLine(\"try\");
            } catch (System.Exception e) {
                throw new System.Exception(\"caught\");
            } finally {
                System.Console.WriteLine(\"done\");
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_class_loc() {
        check_metrics::<CsharpParser>(
            "class A {
                int x;
                public void M() {
                    System.Console.WriteLine(x);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_expressions_lloc() {
        check_metrics::<CsharpParser>(
            "int a = 1;
            int b = 2;
            int c = a + b;
            System.Console.WriteLine(c);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_statement_inline_loc() {
        check_metrics::<CsharpParser>(
            "if (x > 0) System.Console.WriteLine(x);",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.ploc(), 1);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_general_loc() {
        check_metrics::<CsharpParser>(
            "using System;
            namespace Demo {
                class A {
                    public void M() {
                        Console.WriteLine(\"hi\");
                    }
                }
                class B {
                    public int N() { return 0; }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11);
                assert_eq!(metric.loc.ploc(), 11);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_using_lloc() {
        // EC11 — `using_directive` does not bump LLOC; `using_statement`
        // (block form) and the C# 8 simple-using local-declaration
        // (`using var x = ...;`) both do, the latter via the standard
        // `LocalDeclarationStatement` path.
        check_metrics::<CsharpParser>(
            "using System;
            using System.IO;
            class A {
                public void M() {
                    using (var s = File.OpenRead(\"x\")) {
                        Console.WriteLine(s);
                    }
                    using var t = File.OpenRead(\"y\");
                    Console.WriteLine(t);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11);
                assert_eq!(metric.loc.ploc(), 11);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_basic() {
        check_metrics::<KotlinParser>(
            "// A simple function
            fun greet(name: String): String {
                val greeting = \"Hello, \" + name
                if (name.isEmpty()) {
                    return \"Hello, World!\"
                }
                return greeting
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 8,
                  "ploc": 7,
                  "lloc": 4,
                  "cloc": 1,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 3.5,
                  "lloc_average": 2.0,
                  "cloc_average": 0.5,
                  "blank_average": 0.0,
                  "sloc_min": 7,
                  "sloc_max": 8,
                  "cloc_min": 0,
                  "cloc_max": 1,
                  "ploc_min": 7,
                  "ploc_max": 7,
                  "lloc_min": 4,
                  "lloc_max": 4,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_loc_bare_expression() {
        check_metrics::<KotlinParser>(
            "fun main() {
                val x = 42
                println(x)
                listOf(1, 2, 3).forEach { println(it) }
            }",
            "foo.kt",
            |metric| {
                // lloc should count: val x = 42 (PropertyDeclaration, +1)
                // + println(x) (CallExpression, parent=Block, +1)
                // + listOf(1, 2, 3).forEach { ... } (CallExpression, parent=Block, +1) = 3
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 5,
                  "ploc": 5,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.5,
                  "ploc_average": 2.5,
                  "lloc_average": 1.5,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 5,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_loc() {
        check_metrics::<BashParser>(
            "#!/bin/bash
# This is a comment
f() {
    echo 'hello'
}

# Another comment
f",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 3);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    // CRLF regression tests: metrics must be identical regardless of line ending style.
    // These also serve as canaries for tree-sitter row-counting behaviour with \r bytes.

    #[test]
    fn python_cloc_crlf_matches_lf() {
        check_metrics::<PythonParser>("# comment\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1);
            assert_eq!(m.loc.ploc(), 1);
            assert_eq!(m.loc.sloc(), 2);
            assert_eq!(m.loc.blank(), 0);
        });
        check_metrics::<PythonParser>("# comment\r\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1);
            assert_eq!(m.loc.ploc(), 1);
            assert_eq!(m.loc.sloc(), 2);
            assert_eq!(m.loc.blank(), 0);
        });
        // Lone-CR (old Mac line endings) is the true canary: without CR normalisation,
        // tree-sitter 0.26.8 only advances its row counter on \n, collapsing all content
        // onto row 0 and producing wrong sloc/cloc metrics.
        check_metrics::<PythonParser>("# comment\rx = 1", "foo.py", |m| {
            assert_eq!(m.loc.cloc(), 1);
            assert_eq!(m.loc.ploc(), 1);
            assert_eq!(m.loc.sloc(), 2);
            assert_eq!(m.loc.blank(), 0);
        });
    }

    #[test]
    fn python_blank_crlf_matches_lf() {
        check_metrics::<PythonParser>("# comment\n\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1);
        });
        check_metrics::<PythonParser>("# comment\r\n\r\nx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1);
        });
        // Lone-CR: without normalisation the blank \r line stays on row 0 and is not counted.
        check_metrics::<PythonParser>("# comment\r\rx = 1", "foo.py", |m| {
            assert_eq!(m.loc.blank(), 1);
        });
    }

    #[test]
    fn rust_cloc_crlf_matches_lf() {
        check_metrics::<RustParser>(
            "fn f() {\n    // comment\n    let x = 1;\n}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1);
                assert_eq!(m.loc.sloc(), 4);
            },
        );
        check_metrics::<RustParser>(
            "fn f() {\r\n    // comment\r\n    let x = 1;\r\n}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1);
                assert_eq!(m.loc.sloc(), 4);
            },
        );
        // Lone-CR: without normalisation, tree-sitter 0.26.8 only advances its row counter on
        // \n, so all content collapses onto row 0 and sloc becomes 1 instead of 4.
        check_metrics::<RustParser>(
            "fn f() {\r    // comment\r    let x = 1;\r}",
            "foo.rs",
            |m| {
                assert_eq!(m.loc.cloc(), 1);
                assert_eq!(m.loc.sloc(), 4);
            },
        );
    }

    #[test]
    fn tcl_blank() {
        check_metrics::<TclParser>("set x 1\n\nset y 2", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3);
            assert_eq!(metric.loc.ploc(), 2);
            assert_eq!(metric.loc.lloc(), 2);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 1);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // ensures the `blank = sloc - (ploc ∪ cloc lines)` union math holds
        // when code and comment lines coincide.
        check_metrics::<TclParser>(
            "set a 1\nset b 2\n\nset c 3 ;# trailing\nset d 4 ;# trailing\nset e 5",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
            },
        );
    }

    /// expected: row 0 is comment-only, row 1 is the sole code row — so
    /// `cloc 1 + ploc 1 == sloc 2`. This test pinned `ploc == 2` until
    /// #1135: the `LF` token terminating the comment row landed in the
    /// `_` catch-all and inserted that row into PLOC, which also drove
    /// `cloc + ploc` past `sloc`.
    #[test]
    fn tcl_cloc() {
        check_metrics::<TclParser>("# This is a comment\nset x 1", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 2);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 1);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_lloc() {
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        if {$x > 10} {
            set x [expr {$x - 1}]
        }
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_no_command_substitution_lloc() {
        // `string toupper` inside [...] is a sub-expression; only `puts` is top-level.
        check_metrics::<TclParser>("puts [string toupper x]", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_procedure_lloc() {
        check_metrics::<TclParser>("proc foo {} {\n    puts hello\n}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3);
            assert_eq!(metric.loc.ploc(), 3);
            assert_eq!(metric.loc.lloc(), 2);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_if_lloc() {
        check_metrics::<TclParser>("if {1} {\n    puts hello\n}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 3);
            assert_eq!(metric.loc.ploc(), 3);
            assert_eq!(metric.loc.lloc(), 2);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_elseif_lloc() {
        // if=1 lloc, elseif=1 lloc, else adds 0 lloc
        check_metrics::<TclParser>(
            "if {$x > 10} {
    puts big
} elseif {$x > 5} {
    puts medium
} else {
    puts small
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_while_lloc() {
        check_metrics::<TclParser>(
            "while {$x > 0} {\n    set x [expr {$x - 1}]\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_foreach_lloc() {
        check_metrics::<TclParser>(
            "foreach item {a b c} {\n    puts $item\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_set_lloc() {
        check_metrics::<TclParser>("set x 42", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_global_lloc() {
        check_metrics::<TclParser>("global x", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_try_catch_lloc() {
        // try=1 lloc; catch command=1 lloc; commands inside bodies count separately
        check_metrics::<TclParser>(
            "catch {
    set x 1
} result
try {
    set y 2
} on error {msg} {
    puts $msg
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_namespace_lloc() {
        check_metrics::<TclParser>(
            "namespace eval myns {\n    set x 1\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_regexp_lloc() {
        check_metrics::<TclParser>("regexp {^[0-9]+$} $x", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_expr_cmd_lloc() {
        check_metrics::<TclParser>("expr {1 + 2}", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_expr_cmd_substitution_lloc() {
        // `expr` inside [...] is a sub-expression, not a statement; only `set` counts.
        check_metrics::<TclParser>("set x [expr {1 + 2}]", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_nested_commands_lloc() {
        // Commands inside proc body are recursively parsed; verify each counts.
        check_metrics::<TclParser>(
            "proc f {x} {
    set y [expr {$x * 2}]
    puts $y
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_command_lloc() {
        check_metrics::<TclParser>("puts hello", "foo.tcl", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn tcl_no_else_lloc() {
        // `else` block does not add a logical line.
        check_metrics::<TclParser>(
            "if {1} {\n    puts yes\n} else {\n    puts no\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_no_finally_lloc() {
        // `finally` block, like `else`, does not add a logical line.
        // proc(1) + try(1) + puts_hi(1) + puts_done(1) + finally(0) = 4.
        check_metrics::<TclParser>(
            "proc f {} {\n    try {\n        puts hi\n    } finally {\n        puts done\n    }\n}",
            "foo.tcl",
            |metric| {
                assert_eq!(
                    metric.loc.lloc(),
                    4,
                    "finally adds 0 lloc; would be 5 if finally counted"
                );
            },
        );
    }

    #[test]
    fn tcl_multiline_block() {
        check_metrics::<TclParser>(
            "proc f {x} {
    set a 1

    set b 2
    return [expr {$a + $b}]
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_no_string_lloc() {
        // Multi-line double-quoted strings must not inflate lloc — only the
        // surrounding command should count. Mirrors lua_no_string_lloc and
        // elixir_no_string_content_lloc; pins the heredoc-shaped invariant
        // for Tcl quoted_word bodies.
        check_metrics::<TclParser>(
            "set s \"line one\nline two\nline three\"",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_blank() {
        check_metrics::<JavascriptParser>(
            "// header comment
        function f() {

            var x = 1;

            var y = 2;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 1);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_cloc() {
        check_metrics::<JavascriptParser>(
            "// line comment
        /* block
           comment */
        function f() {
            return 1; // inline
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_cloc_html_comment() {
        // The Annex-B `<!-- -->` `html_comment` must count as CLOC, not
        // fall to the `_` arm and inflate PLOC (#697). Pre-fix this
        // fixture measured cloc 0 / ploc 4.
        check_metrics::<JavascriptParser>(
            "<!-- header comment -->
function f() {
  return 1;
}",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 1);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn mozjs_blank() {
        check_metrics::<MozjsParser>(
            "function f() {

            var x = 1;

        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_cloc() {
        check_metrics::<MozjsParser>(
            "// header
        /* block comment */
        function f() {
            return 42;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<MozjsParser>(
            "function f() {
  var a = 1;

  var b = 2; // trailing
  var c = 3; // trailing
}",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_arrow_function_loc() {
        check_metrics::<MozjsParser>(
            "const add = (a, b) => a + b;
        const greet = name => {
            return 'Hello ' + name;
        };",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_multiple_functions_loc() {
        check_metrics::<MozjsParser>(
            "function f() {
            return 1;
        }
        function g() {
            return 2;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_nested_function_loc() {
        check_metrics::<MozjsParser>(
            "function outer() {
            function inner() {
                return 1;
            }
            return inner();
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn mozjs_if_lloc() {
        check_metrics::<MozjsParser>(
            "function f(x) {
            if (x > 0) {
                return 1;
            } else {
                return -1;
            }
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                // lloc = 3: the `if` statement plus the two `return`
                // statements. The three `{ … }` brace blocks (function
                // body, `if` consequent, `else` alternative) are syntactic
                // groupings, not logical statements, and contribute 0 —
                // matching C/Rust/Java for the equivalent code (#777).
                // Pre-#777 this asserted 6 (every StatementBlock counted).
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    // Cross-language parity (#777): the same if/else function body yields
    // identical lloc across the JS family and the C-family / Rust baselines.
    // Removing `StatementBlock` from the JS-family lloc arms restored this
    // invariant — every brace block now contributes 0 lloc, as it always
    // had elsewhere. Pre-#777 the JS variants reported lloc 6 (three brace
    // blocks over-counted) against C's and Rust's 3.
    #[test]
    fn js_family_if_lloc_matches_c_and_rust() {
        const JS_SRC: &str = "function f(x) {
            if (x > 0) {
                return 1;
            } else {
                return -1;
            }
        }";
        const C_SRC: &str = "int f(int x) {
            if (x > 0) {
                return 1;
            } else {
                return -1;
            }
        }";
        const RUST_SRC: &str = "fn f(x: i32) -> i32 {
            if x > 0 {
                return 1;
            } else {
                return -1;
            }
        }";

        // The logical-statement count is grammar-independent: one `if`
        // plus two `return`s, regardless of brace style or language.
        const EXPECTED_LLOC: usize = 3;

        check_metrics::<CppParser>(C_SRC, "f.c", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
        check_metrics::<RustParser>(RUST_SRC, "f.rs", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
        check_metrics::<MozjsParser>(JS_SRC, "f.js", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
        check_metrics::<JavascriptParser>(JS_SRC, "f.js", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
        check_metrics::<TypescriptParser>(JS_SRC, "f.ts", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
        check_metrics::<TsxParser>(JS_SRC, "f.tsx", |m| {
            assert_eq!(m.loc.lloc() as usize, EXPECTED_LLOC);
        });
    }

    #[test]
    fn mozjs_for_lloc() {
        check_metrics::<MozjsParser>(
            "function f(n) {
            var s = 0;
            for (var i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_blank() {
        check_metrics::<BashParser>(
            "#!/bin/bash

        f() {

            echo hello

        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 1);
                assert_eq!(metric.loc.blank(), 3);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_cloc() {
        check_metrics::<BashParser>(
            "# header comment
        f() {
            # body comment
            echo hello
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<BashParser>(
            "f() {
  echo a

  echo b # trailing
  echo c # trailing
}",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_comment_before_code_line_reclassified() {
        // Regression for #547: a standalone `#` comment sitting on a line
        // that the grammar *also* anchors a zero-width code leaf to (the
        // empty `word` tree-sitter-bash emits inside a `$(...)` command
        // substitution that contains only a comment) must reclassify that
        // row from comment-only to code-comment. The Bash `Loc` leaf arm
        // previously omitted `check_comment_ends_on_code_line` (unlike
        // Elixir and every other impl), so the row was credited to BOTH
        // `ploc` and the comment-only set and `blank` was undercounted by
        // one.
        //
        // Source rows: 0 `echo a`, 1 blank, 2 `echo "$(`, 3 `  # c`,
        // 4 `)"`. expected: sloc=5 (every physical row),
        // ploc=4 (rows 0/2/3/4 — row 3 carries the phantom code leaf),
        // lloc=3, cloc=1 (row 3, now a code-comment line, not comment-only),
        // blank=1 (row 1). Without the fix `blank` collapses to 0 because
        // row 3 is double-counted. Verified fail-on-revert per
        // .claude/rules/testing.md.
        check_metrics::<BashParser>("echo a\n\necho \"$(\n  # c\n)\"\n", "foo.sh", |metric| {
            assert_eq!(metric.loc.sloc(), 5);
            assert_eq!(metric.loc.ploc(), 4);
            assert_eq!(metric.loc.lloc(), 3);
            assert_eq!(metric.loc.cloc(), 1);
            assert_eq!(metric.loc.blank(), 1);
            insta::assert_json_snapshot!(
                metric.loc,
                @r#"
                {
                  "sloc": 5,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 1,
                  "blank": 1,
                  "sloc_average": 5.0,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 1.0,
                  "blank_average": 1.0,
                  "sloc_min": 5,
                  "sloc_max": 5,
                  "cloc_min": 1,
                  "cloc_max": 1,
                  "ploc_min": 4,
                  "ploc_max": 4,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
            );
        });
    }

    #[test]
    fn bash_if_lloc() {
        check_metrics::<BashParser>(
            "f() {
            if [ $1 -gt 0 ]; then
                echo positive
            else
                echo negative
            fi
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_for_lloc() {
        check_metrics::<BashParser>(
            "f() {
            for i in 1 2 3; do
                echo $i
            done
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_while_lloc() {
        check_metrics::<BashParser>(
            "f() {
            local n=5
            while [ $n -gt 0 ]; do
                echo $n
                n=$((n - 1))
            done
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_case_lloc() {
        check_metrics::<BashParser>(
            "f() {
            case $1 in
                start) echo starting ;;
                stop)  echo stopping ;;
                *)     echo unknown  ;;
            esac
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_multiple_functions_loc() {
        check_metrics::<BashParser>(
            "f() {
            echo hello
        }
        g() {
            echo world
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_nested_function_loc() {
        check_metrics::<BashParser>(
            "outer() {
            inner() {
                echo inner
            }
            inner
            echo outer
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn bash_heredoc_loc() {
        check_metrics::<BashParser>(
            "f() {
            cat <<EOF
line1
line2
EOF
        }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_blank() {
        check_metrics::<KotlinParser>(
            "fun f(): Int {

            val x = 1

            return x
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_cloc() {
        check_metrics::<KotlinParser>(
            "// header comment
        /* block
           comment */
        fun f(): Int {
            return 42 // inline
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_no_zero_blank() {
        // Checks that the blank metric is not equal to 0 when there are some
        // comments next to code lines. Mirrors rust_no_zero_blank.
        check_metrics::<KotlinParser>(
            "fun connectToUpdateServer() {
              val pool = 0

              val updateServer = -42
              val isConnected = false
              val currTry = 0
              val numRetries = 10  // Number of IPC connection retries before
                                    // giving up.
              val numTries = 20    // Number of IPC connection tries before
                                    // giving up.
            }",
            "foo.kt",
            |metric| {
                // Anchor the headline integer values; in particular
                // `blank() > 0` is the contract this test's name advertises.
                assert_eq!(metric.loc.sloc(), 11);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 11,
                  "ploc": 8,
                  "lloc": 6,
                  "cloc": 4,
                  "blank": 1,
                  "sloc_average": 5.5,
                  "ploc_average": 4.0,
                  "lloc_average": 3.0,
                  "cloc_average": 2.0,
                  "blank_average": 0.5,
                  "sloc_min": 11,
                  "sloc_max": 11,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 8,
                  "ploc_max": 8,
                  "lloc_min": 6,
                  "lloc_max": 6,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_loc_blank_zero_sanity() {
        // Sanity: when the source has no blank lines, blank() must be 0.
        // Preserves the no-blank coverage previously held by
        // kotlin_loc_no_zero_blank before it was rewritten to assert the
        // positive case its name advertises.
        check_metrics::<KotlinParser>(
            "fun f(): Int {
            val x = 1 // x
            val y = 2 // y
            return x + y
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn kotlin_loc_if_lloc() {
        check_metrics::<KotlinParser>(
            "fun classify(n: Int): String {
            if (n > 0) {
                return \"positive\"
            } else if (n < 0) {
                return \"negative\"
            }
            return \"zero\"
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_for_lloc() {
        check_metrics::<KotlinParser>(
            "fun sum(n: Int): Int {
            var s = 0
            for (i in 1..n) {
                s += i
            }
            return s
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_when_lloc() {
        check_metrics::<KotlinParser>(
            "fun describe(x: Int): String {
            return when (x) {
                1 -> \"one\"
                2 -> \"two\"
                else -> \"other\"
            }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_lambda_lloc() {
        check_metrics::<KotlinParser>(
            "fun f(list: List<Int>): List<Int> {
            return list.filter { it > 0 }
                       .map { it * 2 }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_class_loc() {
        check_metrics::<KotlinParser>(
            "class Counter {
            private var count = 0
            fun increment() { count++ }
            fun get(): Int = count
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_multiple_functions_loc() {
        check_metrics::<KotlinParser>(
            "fun f(): Int {
            return 1
        }
        fun g(): Int {
            return 2
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_loc_while_lloc() {
        check_metrics::<KotlinParser>(
            "fun countdown(n: Int) {
            var i = n
            while (i > 0) {
                println(i)
                i--
            }
        }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_blank() {
        check_metrics::<TypescriptParser>(
            "function f(): void {

            const x = 1;

        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_cloc() {
        check_metrics::<TypescriptParser>(
            "// header
        /* block
           comment */
        function f(): number {
            return 42; // inline
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<TypescriptParser>(
            "function f(): void {
  const a = 1;

  const b = 2; // trailing
  const c = 3; // trailing
}",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_if_lloc() {
        check_metrics::<TypescriptParser>(
            "function classify(n: number): string {
            if (n > 0) {
                return 'positive';
            } else {
                return 'non-positive';
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_for_lloc() {
        check_metrics::<TypescriptParser>(
            "function sum(n: number): number {
            let s = 0;
            for (let i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_while_lloc() {
        check_metrics::<TypescriptParser>(
            "function countdown(n: number): void {
            let i = n;
            while (i > 0) {
                console.log(i);
                i--;
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_switch_lloc() {
        check_metrics::<TypescriptParser>(
            "function describe(x: number): string {
            switch (x) {
                case 1: return 'one';
                case 2: return 'two';
                default: return 'other';
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_class_loc() {
        check_metrics::<TypescriptParser>(
            "class Counter {
            private count: number = 0;
            increment(): void { this.count++; }
            get(): number { return this.count; }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_arrow_function_loc() {
        check_metrics::<TypescriptParser>(
            "const add = (a: number, b: number): number => a + b;
        const greet = (name: string): string => {
            return `Hello, ${name}`;
        };",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_interface_loc() {
        check_metrics::<TypescriptParser>(
            "interface Shape {
            area(): number;
            perimeter(): number;
        }
        function describe(s: Shape): string {
            return `area=${s.area()}`;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_multiple_functions_loc() {
        check_metrics::<TypescriptParser>(
            "function f(): number {
            return 1;
        }
        function g(): number {
            return 2;
        }
        function h(): number {
            return 3;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9);
                assert_eq!(metric.loc.ploc(), 9);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_try_catch_lloc() {
        check_metrics::<TypescriptParser>(
            "function safe(x: number): number {
            try {
                return 1 / x;
            } catch (e) {
                return 0;
            }
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_nested_functions_loc() {
        check_metrics::<TypescriptParser>(
            "function outer(x: number): number {
            function inner(y: number): number {
                return y * 2;
            }
            return inner(x) + 1;
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn typescript_generic_function_loc() {
        check_metrics::<TypescriptParser>(
            "function identity<T>(value: T): T {
            return value;
        }
        function first<T>(arr: T[]): T | undefined {
            return arr[0];
        }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_blank() {
        check_metrics::<TsxParser>(
            "function f(): void {

            const x = 1;

        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_cloc() {
        check_metrics::<TsxParser>(
            "// header
        /* block
           comment */
        function f(): number {
            return 42; // inline
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<TsxParser>(
            "function f(): void {
  const a = 1;

  const b = 2; // trailing
  const c = 3; // trailing
}",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_if_lloc() {
        check_metrics::<TsxParser>(
            "function classify(n: number): string {
            if (n > 0) {
                return 'positive';
            } else {
                return 'non-positive';
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_for_lloc() {
        check_metrics::<TsxParser>(
            "function sum(n: number): number {
            let s = 0;
            for (let i = 0; i < n; i++) {
                s += i;
            }
            return s;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_while_lloc() {
        check_metrics::<TsxParser>(
            "function countdown(n: number): void {
            let i = n;
            while (i > 0) {
                console.log(i);
                i--;
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_switch_lloc() {
        check_metrics::<TsxParser>(
            "function describe(x: number): string {
            switch (x) {
                case 1: return 'one';
                case 2: return 'two';
                default: return 'other';
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_class_loc() {
        check_metrics::<TsxParser>(
            "class Counter {
            private count: number = 0;
            increment(): void { this.count++; }
            get(): number { return this.count; }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_arrow_function_loc() {
        check_metrics::<TsxParser>(
            "const add = (a: number, b: number): number => a + b;
        const greet = (name: string): string => {
            return `Hello, ${name}`;
        };",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_multiple_functions_loc() {
        check_metrics::<TsxParser>(
            "function f(): number {
            return 1;
        }
        function g(): number {
            return 2;
        }
        function h(): number {
            return 3;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 9);
                assert_eq!(metric.loc.ploc(), 9);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_try_catch_lloc() {
        check_metrics::<TsxParser>(
            "function safe(x: number): number {
            try {
                return 1 / x;
            } catch (e) {
                return 0;
            }
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_nested_functions_loc() {
        check_metrics::<TsxParser>(
            "function outer(x: number): number {
            function inner(y: number): number {
                return y * 2;
            }
            return inner(x) + 1;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_interface_loc() {
        check_metrics::<TsxParser>(
            "interface Shape {
            area(): number;
            perimeter(): number;
        }
        function describe(s: Shape): string {
            return `area=${s.area()}`;
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tsx_generic_function_loc() {
        check_metrics::<TsxParser>(
            "function identity<T>(value: T): T {
            return value;
        }
        function first<T>(arr: T[]): T | undefined {
            return arr[0];
        }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_blank() {
        check_metrics::<PhpParser>(
            "<?php

$a = 1;

$b = 2;

",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<PhpParser>(
            "<?php
$a = 1;

$b = 2; // trailing
$c = 3; // trailing
",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_double_slash() {
        check_metrics::<PhpParser>(
            "<?php
// first
// second
$a = 1; // trailing",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 3);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_hash() {
        check_metrics::<PhpParser>(
            "<?php
# first
# second
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_cloc_block() {
        check_metrics::<PhpParser>(
            "<?php
/*
 * block
 * comment
 */
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 4);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_lloc() {
        // Three statements: assignment, if (with body), echo.
        check_metrics::<PhpParser>(
            "<?php
$a = 1;
if ($a > 0) {
    echo $a;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_parenthesized_expression_lloc() {
        // Parenthesized expression should not add an extra LLOC over the
        // surrounding expression_statement.
        check_metrics::<PhpParser>(
            "<?php
$a = (1 + 2);",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_compound_statement_lloc() {
        // Block wrappers (`{ … }`) are not LLOC themselves.
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    $a = 1;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_colon_block_lloc() {
        // Alternative syntax (`if: … endif;`) uses ColonBlock instead of
        // CompoundStatement; it is also not LLOC.
        check_metrics::<PhpParser>(
            "<?php
if (true):
    $a = 1;
endif;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_else_clause_lloc() {
        // ElseClause and ElseIfClause are sub-parts of IfStatement.
        check_metrics::<PhpParser>(
            "<?php
if ($x) {
    $a = 1;
} elseif ($y) {
    $a = 2;
} else {
    $a = 3;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_case_statement_lloc() {
        // CaseStatement / DefaultStatement are switch arms, not separate
        // statements.
        check_metrics::<PhpParser>(
            "<?php
switch ($x) {
    case 1:
        $a = 1;
        break;
    case 2:
        $a = 2;
        break;
    default:
        $a = 0;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 11);
                assert_eq!(metric.loc.ploc(), 11);
                assert_eq!(metric.loc.lloc(), 6);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_match_arm_lloc() {
        // MatchConditionalExpression / MatchDefaultExpression are arms;
        // only the surrounding expression_statement counts.
        check_metrics::<PhpParser>(
            "<?php
$a = match ($x) {
    1 => 'one',
    2 => 'two',
    default => 'other',
};",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_throw_in_expression_lloc() {
        // PHP 8 `throw` as expression: only the surrounding statement
        // counts (the `??` in this example), not the throw_expression.
        check_metrics::<PhpParser>(
            "<?php
$x = $y ?? throw new \\Exception('nope');",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_no_closure_in_assignment_lloc() {
        // Anonymous function as RHS does not add an LLOC; only the
        // expression_statement counts. The closure body's statements are
        // counted in its own FuncSpace.
        check_metrics::<PhpParser>(
            "<?php
$f = function (): int {
    return 42;
};",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_for_lloc() {
        // The for_statement contributes 1 LLOC; init/cond/update are NOT
        // separate statements in PHP's grammar.
        check_metrics::<PhpParser>(
            "<?php
for ($i = 0; $i < 10; $i++) {
    echo $i;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_foreach_lloc() {
        check_metrics::<PhpParser>(
            "<?php
foreach ($items as $k => $v) {
    echo $v;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_try_lloc() {
        check_metrics::<PhpParser>(
            "<?php
try {
    $a = 1;
} catch (\\Exception $e) {
    $a = 0;
} finally {
    $b = 2;
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_class_loc() {
        check_metrics::<PhpParser>(
            "<?php
class A {
    public int $x = 0;
    private const Y = 1;
    public function f(): int {
        return $this->x;
    }
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_namespace_use_lloc() {
        check_metrics::<PhpParser>(
            "<?php
namespace App;
use App\\Foo;
use App\\Bar;
$a = 1;",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_general_loc() {
        check_metrics::<PhpParser>(
            "<?php
// header
namespace App;
use App\\Foo;

class Bar {
    public int $n = 0;

    public function add(int $x): int {
        if ($x > 0) {
            return $this->n + $x;
        }
        return $this->n;
    }
}",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 15);
                assert_eq!(metric.loc.ploc(), 12);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 1);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_match_in_expression_lloc() {
        // Match inside another expression (e.g. assignment RHS) — the
        // outer expression_statement counts, the inner match arms do not.
        check_metrics::<PhpParser>(
            "<?php
$y = 10 + match ($x) { 1 => 2, default => 0 };",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_html_island_ploc() {
        // Embedded HTML between PHP tags ("text interpolation"). HTML
        // rows must contribute to PLOC (they are not blank and not a
        // PHP comment); this test locks that behavior so a future
        // grammar bump or impl tweak that excludes `text` nodes from
        // the default PLOC branch is caught.
        check_metrics::<PhpParser>(
            "<?php if ($cond): ?>
<div>hello</div>
<p>world</p>
<?php endif; ?>",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_short_echo_tag_ploc() {
        // `<?=` is the same `php_tag` kind as `<?php` per
        // tree-sitter-php 0.24.2. A regression that re-classified `<?=`
        // would shift PLOC; this test pins the current behavior.
        check_metrics::<PhpParser>("<p><?= $name ?></p>", "foo.php", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.lloc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
            assert_eq!(metric.loc.blank(), 0);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn elixir_blank() {
        // Two blank lines separate three top-level expressions.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n\n  def a, do: :a\n\n  def b, do: :b\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
                insta::assert_json_snapshot!(
                    metric.loc,
                    @r#"
                {
                  "sloc": 6,
                  "ploc": 4,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 2,
                  "sloc_average": 1.5,
                  "ploc_average": 1.0,
                  "lloc_average": 0.75,
                  "cloc_average": 0.0,
                  "blank_average": 0.5,
                  "sloc_min": 1,
                  "sloc_max": 6,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 1,
                  "ploc_max": 4,
                  "lloc_min": 1,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 2
                }
                "#
                );
            },
        );
    }

    #[test]
    fn elixir_no_zero_blank() {
        // Blank line interleaved with code that carries trailing comments —
        // stresses the `blank = sloc - (ploc ∪ cloc lines)` union math.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f, do: :ok\n\n  def g, do: :ok # trailing\n  def h, do: :ok # trailing\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.blank(), 1);
            },
        );
    }

    #[test]
    fn elixir_blank_zero_sanity() {
        // Sanity check: blank must report 0, never go negative, when the
        // input has no blank lines.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f, do: :ok\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn elixir_cloc() {
        // Mix of standalone comments and a comment on the same line as
        // code. Elixir has no block comment syntax — only `#` lines.
        check_metrics::<ElixirParser>(
            "# top\ndefmodule Foo do\n  # body\n  def f, do: :ok # trailing\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.cloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_lloc() {
        // Two statements at the top level of the module body — the
        // `defmodule` call itself counts as one statement (since its
        // parent is `Source`), and each `def` inside its `do_block`
        // counts too: 1 + 2 = 3.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def a, do: 1\n  def b, do: 2\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_no_nested_call_lloc() {
        // Calls nested inside another call's arguments are NOT direct
        // children of a statement container, so they do not bump LLOC.
        // Three syntactic calls (`defmodule`, `def`, `IO.puts`) → 3.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    IO.puts(Enum.join([1, 2, 3], \", \"))\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_no_binary_operator_inside_call_lloc() {
        // Binary operators inside call arguments are sub-expressions,
        // not statements. A single `def` body containing `IO.puts(a + b)`
        // produces 3 LLOC (defmodule, def, IO.puts) — the `a + b`
        // binary_operator is not a direct child of any statement
        // container.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(a, b) do\n    IO.puts(a + b)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_stab_clause_counts_lloc() {
        // Each `stab_clause` arm in a `case do ... end` is a direct
        // child of the inner `do_block`, so each one is its own LLOC.
        // defmodule + def + case + 3 arms = 6 logical lines.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    case x do\n      1 -> :a\n      2 -> :b\n      _ -> :c\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 6);
            },
        );
    }

    #[test]
    fn elixir_no_comment_lloc() {
        // Comments are direct children of a statement container but
        // are routed through the dedicated `Comment` arm in `compute`,
        // so they MUST NOT bump LLOC. Only `defmodule` and `def`
        // contribute LLOC here.
        check_metrics::<ElixirParser>(
            "# leading\ndefmodule Foo do\n  # inside\n  def f, do: :ok\n  # trailing\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn elixir_no_do_token_lloc() {
        // The `do` and `end` keyword tokens are unnamed leaves inside a
        // `do_block`; they must not be counted as statements. A body
        // with one expression produces exactly 2 LLOC (defmodule and
        // the inner expression).
        check_metrics::<ElixirParser>("defmodule Foo do\n  :ok\nend\n", "foo.ex", |metric| {
            // `:ok` is an `Atom` whose parent is the module-call's
            // `do_block`; that counts. Plus the `defmodule` call.
            assert_eq!(metric.loc.lloc(), 2);
        });
    }

    #[test]
    fn elixir_no_keyword_pair_lloc() {
        // `key: value` keyword pairs inside an argument list (`def f,
        // do: :ok`) are children of an `arguments` / `keywords` node,
        // not a statement container, so they don't bump LLOC.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def add(a, b), do: a + b\nend\n",
            "foo.ex",
            |metric| {
                // defmodule (1) + def (1) = 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn elixir_no_string_content_lloc() {
        // `quoted_content` chunks inside a heredoc / regular string are
        // structural and don't represent statements. A `@moduledoc`
        // attribute call with a multi-line string contributes exactly
        // one LLOC (the `@moduledoc` call), not one per content line.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  @moduledoc \"\"\"\n  line one\n  line two\n  \"\"\"\n  def f, do: :ok\nend\n",
            "foo.ex",
            |metric| {
                // defmodule + @moduledoc + def = 3
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_rescue_arm_counts_lloc() {
        // Each rescue arm's body has a single expression (e.g. `:bad`)
        // that counts as one LLOC; the `stab_clause` header itself is
        // skipped. The rescue_block named node is also a direct child
        // of try's do_block, so it contributes one LLOC too.
        // Total: defmodule + def + try + do_it() + rescue_block
        //        + 2 arm bodies = 7.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def safe do\n    try do\n      do_it()\n    rescue\n      ArgumentError -> :bad\n      RuntimeError -> :worse\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 7);
            },
        );
    }

    #[test]
    fn elixir_no_arg_punctuation_lloc() {
        // Function-call arguments (`a, b` inside `def add(a, b)`) are
        // children of an `arguments` node, not of a statement container.
        // They MUST NOT inflate LLOC.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def add(a, b, c, d) do\n    a + b + c + d\n  end\nend\n",
            "foo.ex",
            |metric| {
                // defmodule + def + (a+b+c+d) = 3
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_no_list_element_lloc() {
        // List literal elements live under a `list` node, not a
        // statement container — they must not bump LLOC.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    [:a, :b, :c, :d]\n  end\nend\n",
            "foo.ex",
            |metric| {
                // defmodule + def + the list expression = 3
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_no_map_field_lloc() {
        // Map `pair`s live under `map`, not a statement container.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    %{a: 1, b: 2, c: 3}\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn elixir_anonymous_fn_body_lloc() {
        // `lloc()` on the Unit space returns the aggregate (own +
        // nested-space) count. Even though the anonymous_function is
        // its own function space, the merge step pulls its `lloc` back
        // into the parent. Counts:
        //   Unit own: defmodule, def, `add = fn ...`, final `add` = 4
        //   anon-fn:  `x + 1` body expression                       = 1
        //   aggregated total                                        = 5
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    add = fn x -> x + 1 end\n    add\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.lloc(), 5);
            },
        );
    }

    #[test]
    fn ruby_blank() {
        // The parser's root span starts at the first non-blank line, so
        // a blank line must sit BETWEEN code lines to be counted.
        // expected: line 3 is blank → blank = 1.
        check_metrics::<RubyParser>("def foo\n  a = 1\n\n  a + 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.loc.blank(), 1);
        });
    }

    #[test]
    fn ruby_no_zero_blank() {
        // Mirrors `rust_no_zero_blank`: the blank counter must stay
        // non-zero when blank lines sit between code lines that carry
        // trailing comments. Catches regressions in the SLOC −
        // (PLOC ∪ CLOC) union math when PLOC and CLOC line-sets
        // overlap.
        check_metrics::<RubyParser>(
            "def foo  # entry\n  pool = 0\n\n  server = -42  # negative\n\n  ok = false\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.blank(), 2);
            },
        );
    }

    #[test]
    fn ruby_cloc() {
        // 3 comment lines.
        check_metrics::<RubyParser>(
            "# one\n# two\n# three\ndef foo\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.cloc(), 3);
            },
        );
    }

    #[test]
    fn ruby_lloc() {
        // expected: 3 logical lines = `def` (Method) + `if` (If) +
        // `while` (While). Bare expression-statements (assignments,
        // calls) are intentionally NOT counted.
        check_metrics::<RubyParser>(
            "def foo(a)\n  if a\n    a += 1\n  end\n  while a > 0\n    a -= 1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn ruby_no_call_lloc() {
        // expected: 1 logical line (the surrounding `def`). The bare
        // method calls `puts 'hello'` and `puts 'world'` are
        // intentionally NOT counted — there is no expression_statement
        // wrapper to disambiguate them from sub-expressions.
        check_metrics::<RubyParser>(
            "def foo\n  puts 'hello'\n  puts 'world'\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn ruby_no_assignment_lloc() {
        // Same rationale as `ruby_no_call_lloc`. expected: 1 lloc
        // (the `def`); raw assignments aren't counted.
        check_metrics::<RubyParser>(
            "def foo\n  a = 1\n  b = 2\n  c = a + b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn ruby_modifier_lloc() {
        // Postfix modifier forms each count as one logical line. A
        // `return … if …` parses as an `IfModifier` wrapping a `Return`;
        // both fire the LLOC arm so the modifier line contributes +2.
        // expected: def(1) + if_modifier(1) + inner return(1)
        // + while_modifier(1) + rescue_modifier(1) = 5.
        check_metrics::<RubyParser>(
            "def foo(a)\n  return a if a.nil?\n  a -= 1 while a > 0\n  parse(a) rescue nil\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 5);
            },
        );
    }

    #[test]
    fn ruby_class_lloc() {
        // expected: 1 class + 1 module + 2 methods = 4.
        check_metrics::<RubyParser>(
            "module M\n  class C\n    def foo\n    end\n    def bar\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    #[test]
    fn ruby_begin_rescue_lloc() {
        // expected: 1 def + 1 begin = 2. Rescue clauses are part of
        // the begin construct and not separately counted; the bare
        // expression body lines are not statements.
        check_metrics::<RubyParser>(
            "def foo\n  begin\n    risky\n  rescue StandardError\n    nil\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn ruby_nested_defs_lloc() {
        // Each `Method` declaration contributes one logical line.
        // expected: outer `def` + inner `def` = 2.
        check_metrics::<RubyParser>(
            "def outer\n  def inner\n    1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn ruby_no_block_body_lloc() {
        // A top-level `[1,2,3].each do |x| puts x end` produces zero
        // logical lines: the surrounding `.each` is a `Call` (not in
        // the LLOC arm), the `DoBlock` is a closure (also not a
        // statement), and the `puts x` inside is another call. This
        // pins the documented expression-statement exclusion.
        check_metrics::<RubyParser>(
            "[1, 2, 3].each do |x|\n  puts x\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 0);
            },
        );
    }

    #[test]
    fn ruby_no_lambda_body_lloc() {
        // `add = ->(a, b) { a + b }` produces zero logical lines for
        // the same reason as `ruby_no_block_body_lloc`: assignments,
        // calls, and lambda bodies are intentionally not statements
        // in this impl.
        check_metrics::<RubyParser>("add = ->(a, b) {\n  a + b\n}\n", "foo.rb", |metric| {
            assert_eq!(metric.loc.lloc(), 0);
        });
    }

    #[test]
    fn ruby_heredoc_lloc_and_blank() {
        // A `<<~TXT` heredoc contributes: SLOC = every line in the file
        // (including the heredoc body); LLOC = just the surrounding `def`.
        // #778: the heredoc-body rows (`one`, `two`) hold real string text,
        // so they are credited to PLOC like Python's multi-line strings
        // (#415) rather than mislabelled as blank. Every row is now code.
        // expected: sloc = 7, ploc = 7, lloc = 1, blank = 0.
        check_metrics::<RubyParser>(
            "def foo\n  msg = <<~TXT\n    one\n    two\n  TXT\n  msg\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    #[test]
    fn ruby_semicolon_multistatement_lloc_undercount() {
        // Documented limitation: Ruby has no `expression_statement`
        // wrapper, so `;`-separated multi-statement lines collapse to
        // a single LLOC bump (the surrounding `def`). A future
        // statement-counter that walks BlockBody children would
        // change this — pin the current behaviour so the regression
        // is visible.
        check_metrics::<RubyParser>(
            "def foo\n  a = 1; b = 2; a + b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn ruby_ploc_skips_comments_and_blanks() {
        // PLOC counts physical instruction lines: code-bearing lines
        // only. Comments and blanks are excluded.
        check_metrics::<RubyParser>("# header\n\ndef foo\n  a = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.loc.ploc(), 3);
            assert_eq!(metric.loc.cloc(), 1);
            assert_eq!(metric.loc.blank(), 1);
        });
    }

    // -----------------------------------------------------------------
    // Issue #195: nested-function/closure LLOC tests across languages.
    // Mirrors the prior art for Rust (`rust_function_in_loop_lloc`,
    // `rust_closure_expression_lloc`), Mozjs (`mozjs_nested_function_loc`),
    // Bash (`bash_nested_function_loc`), and TypeScript
    // (`typescript_nested_functions_loc`, `tsx_nested_functions_loc`).
    // -----------------------------------------------------------------

    #[test]
    fn python_nested_def_lloc() {
        // Nested `def`: the inner function declaration plus the outer
        // body's `return inner()` are both LLOC; the outer `def` header
        // and the inner `return 1` belong to their own function spaces.
        check_metrics::<PythonParser>(
            "def outer():\n    def inner():\n        return 1\n    return inner()\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn python_lambda_in_def_lloc() {
        // `lambda x: x + 1` is an expression, not a Python `function_definition`,
        // so it does not start a new function space. The two LLOC come from
        // the assignment `f = lambda ...` and the `return f(2)` statement.
        check_metrics::<PythonParser>(
            "def outer():\n    f = lambda x: x + 1\n    return f(2)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.loc.sloc(), 3);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn python_match_statement_lloc() {
        // `match` (PEP 634) is a control-flow statement that must add one
        // LLOC like `if`/`try`, plus each `return` in a case body (#462).
        // Its `case_clause` children add nothing, mirroring how
        // `elif_clause`/`else_clause` are absent from the LLOC arm: the
        // construct counts once and the statements inside count via their
        // own arms. Here: match(1) + two `return`s = 3.
        check_metrics::<PythonParser>(
            "def f(x):\n    match x:\n        case 1: return 1\n        case _: return 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn python_match_lloc_matches_if_else() {
        // Parity with the equivalent two-branch `if`/`else`: both have the
        // construct keyword (1) plus two `return` bodies (2), so each must
        // report an identical LLOC of 3. The match form previously
        // undercounted (2) because `match_statement` was absent from the
        // LLOC arm (#462). `check_metrics` takes a non-capturing `fn`, so
        // the shared expectation is pinned to the same literal in both
        // closures rather than threaded through a captured variable.
        check_metrics::<PythonParser>(
            "def f(x):\n    if x == 1: return 1\n    else: return 0\n",
            "foo.py",
            |metric| assert_eq!(metric.loc.lloc(), 3),
        );
        check_metrics::<PythonParser>(
            "def f(x):\n    match x:\n        case 1: return 1\n        case _: return 0\n",
            "foo.py",
            |metric| assert_eq!(metric.loc.lloc(), 3),
        );
    }

    #[test]
    fn python_type_alias_lloc() {
        // A `type` alias (PEP 695) is a leaf statement, counted like an
        // assignment. `type Alias = int` followed by `x = 1` is two LLOC;
        // before #462 the alias fell through to the `_` arm and the file
        // reported only 1.
        check_metrics::<PythonParser>("type Alias = int\nx = 1\n", "foo.py", |metric| {
            assert_eq!(metric.loc.lloc(), 2);
            insta::assert_json_snapshot!(metric.loc);
        });
    }

    #[test]
    fn java_local_class_in_method_lloc() {
        // A `class` declared inside a method body produces its own function
        // space, so the outer method's LLOC only sees `return new Local().v();`
        // and the body of `v()` contributes the second LLOC.
        check_metrics::<JavaParser>(
            "class Foo {\n    int bar() {\n        class Local {\n            int v() { return 1; }\n        }\n        return new Local().v();\n    }\n}\n",
            "foo.java",
            |metric| {
                assert_eq!(metric.loc.sloc(), 8);
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn java_lambda_in_method_lloc() {
        // Java lambdas are expressions; the two LLOC come from the
        // `IntUnaryOperator f = x -> x + 1;` declaration and the
        // `f.applyAsInt(3);` expression statement.
        check_metrics::<JavaParser>(
            "class Foo {\n    void bar() {\n        java.util.function.IntUnaryOperator f = x -> x + 1;\n        f.applyAsInt(3);\n    }\n}\n",
            "foo.java",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn groovy_blank() {
        // Blank lines + simple statements. Newlines act as the
        // statement terminator; PLOC counts the two declaration lines.
        check_metrics::<GroovyParser>("int x = 1\n\n\nint y = 2", "foo.groovy", |metric| {
            assert_eq!(metric.loc.sloc(), 4);
            assert_eq!(metric.loc.ploc(), 2);
            assert_eq!(metric.loc.lloc(), 2);
            assert_eq!(metric.loc.blank(), 2);
        });
    }

    #[test]
    fn groovy_no_zero_blank() {
        // A single line with no blanks: blank() == 0.
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.blank(), 0);
        });
    }

    #[test]
    fn groovy_cloc_line_comments() {
        check_metrics::<GroovyParser>(
            "// first comment
            int x = 1
            // second comment
            int y = 2",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.cloc(), 2);
                assert_eq!(metric.loc.ploc(), 2);
            },
        );
    }

    #[test]
    fn groovy_cloc_block_comment() {
        check_metrics::<GroovyParser>(
            "/* multi
               line
               comment */
            int x = 1",
            "foo.groovy",
            |metric| {
                // Block comment spans 3 lines → cloc == 3.
                assert_eq!(metric.loc.cloc(), 3);
            },
        );
    }

    #[test]
    fn groovy_cloc_groovydoc_comment() {
        // Groovy `/** … */` `groovydoc_comment` counts as CLOC. The
        // `Loc` arm already handled it; this pins it alongside the
        // restored `is_comment` parity (#697).
        check_metrics::<GroovyParser>(
            "/** groovydoc */
class A {
  int x = 1
}",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.cloc(), 1);
                assert_eq!(metric.loc.ploc(), 3);
            },
        );
    }

    #[test]
    fn groovy_simple_lloc() {
        // One LLOC per simple expression statement.
        check_metrics::<GroovyParser>(
            "int a = 1
            int b = 2
            int c = 3",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn groovy_no_local_variable_declaration_in_for_lloc() {
        // The variable declaration inside a classic `for` init slot
        // does NOT count as an LLOC (it's an expression part of the
        // for-loop). Same gating as Java's `java_for_lloc`.
        check_metrics::<GroovyParser>(
            "for (int i = 0; i < 10; i++) {
                println(i)
            }",
            "foo.groovy",
            |metric| {
                // for-statement (1) + expression-statement `println(i)` (1) = 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn groovy_lambda_in_method_lloc() {
        // Closures contain a statement list — the dekobon grammar wraps
        // a single-expression body in `expression_statement` rather than
        // emitting the expression directly (as Java's `lambda_expression`
        // does), so a one-line closure body counts as its own LLOC.
        // Declaration `def f = …` (1) + closure body `x + 1` (1) +
        // call `f(3)` (1) = 3.
        check_metrics::<GroovyParser>(
            "class Foo {
                void bar() {
                    def f = { x -> x + 1 }
                    f(3)
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn groovy_try_lloc() {
        // try-statement counts as one LLOC; the catch body's
        // statements count separately.
        check_metrics::<GroovyParser>(
            "void f() {
                try {
                    risky()
                } catch (Exception e) {
                    handle(e)
                }
            }",
            "foo.groovy",
            |metric| {
                // try(1) + risky() expr-stmt(1) + handle() expr-stmt(1) = 3
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn groovy_class_loc() {
        // Source-file-level totals across multiple methods.
        check_metrics::<GroovyParser>(
            "class A {
                void f() {
                    int x = 1
                }
                void g() {
                    int y = 2
                }
            }",
            "foo.groovy",
            |metric| {
                // 8 lines of non-comment content: `class A {`, two
                // `void` headers, two `int … = …` body statements,
                // three closing braces.
                assert_eq!(metric.loc.ploc(), 8);
                assert_eq!(metric.loc.cloc(), 0);
                // Two expression-statement LLOCs (`int x = 1`,
                // `int y = 2`).
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn groovy_partial_parse_recovers_unit() {
        // Malformed input parses with ERROR but still emits a Unit
        // root via `spaces.rs` fallback (lesson 9). The single
        // source line is counted as SLOC even when the parse fails
        // mid-expression.
        check_metrics::<GroovyParser>("def x = (((", "foo.groovy", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.blank(), 0);
        });
    }

    #[test]
    fn groovy_sloc() {
        // Mirrors `java_sloc`: basic per-line count across a mix of
        // statements and a blank line.
        check_metrics::<GroovyParser>(
            "int a = 1
            int b = 2

            int c = 3",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 3);
                assert_eq!(metric.loc.blank(), 1);
            },
        );
    }

    #[test]
    fn groovy_single_ploc() {
        // Mirrors `java_single_ploc`: one non-blank, non-comment
        // line of code => ploc == 1.
        check_metrics::<GroovyParser>("int x = 42", "foo.groovy", |metric| {
            assert_eq!(metric.loc.ploc(), 1);
            assert_eq!(metric.loc.cloc(), 0);
        });
    }

    #[test]
    fn groovy_multi_ploc() {
        // Multiple statements on separate lines all contribute to
        // PLOC. Mirrors `java_multi_ploc`.
        check_metrics::<GroovyParser>(
            "int a = 1
            int b = 2
            int c = 3
            int d = 4",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    #[test]
    fn groovy_single_statement_lloc() {
        // A single expression statement contributes one LLOC.
        // Mirrors `java_single_statement_lloc`.
        check_metrics::<GroovyParser>("println 'hi'", "foo.groovy", |metric| {
            assert_eq!(metric.loc.lloc(), 1);
        });
    }

    #[test]
    fn groovy_for_lloc() {
        // The classical `for` statement itself counts as one LLOC;
        // the body's `println(i)` adds another. The init-slot
        // var-decl is suppressed by the LocalVariableDeclaration
        // ancestor-check (same rule as `java_for_lloc`).
        check_metrics::<GroovyParser>(
            "for (int i = 0; i < 100; i++) {
                println(i)
            }",
            "foo.groovy",
            |metric| {
                // ForStatement(1) + println-expr(1) = 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn groovy_foreach_lloc() {
        // `for (item in list)` parses as `enhanced_for_statement` —
        // counts as one LLOC.
        check_metrics::<GroovyParser>(
            "for (item in items) {
                println(item)
            }",
            "foo.groovy",
            |metric| {
                // EnhancedForStatement(1) + println(1) = 2
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    #[test]
    fn groovy_while_lloc() {
        // `while` itself is one LLOC; each body statement adds
        // another. Mirrors `java_while_lloc`.
        check_metrics::<GroovyParser>(
            "int i = 0
            while (i < 10) {
                i++
                println(i)
            }",
            "foo.groovy",
            |metric| {
                // int i = 0 (1) + while (1) + i++ (1) + println (1) = 4
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    #[test]
    fn groovy_do_while_lloc() {
        // `do…while` is one LLOC plus its body. Mirrors
        // `java_do_while_lloc`.
        check_metrics::<GroovyParser>(
            "int i = 0
            do {
                i++
            } while (i < 5)",
            "foo.groovy",
            |metric| {
                // int i = 0 (1) + do (1) + i++ (1) = 3
                assert_eq!(metric.loc.lloc(), 3);
            },
        );
    }

    #[test]
    fn groovy_continue_lloc() {
        // `continue` is an LLOC. Same gating as `java_continue_lloc`.
        check_metrics::<GroovyParser>(
            "for (int i = 0; i < 10; i++) {
                if (i == 5) {
                    continue
                }
                println(i)
            }",
            "foo.groovy",
            |metric| {
                // for(1) + if(1) + continue(1) + println(1) = 4
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    #[test]
    fn groovy_expressions_lloc() {
        // A bag of expression statements: each independent
        // expr-stmt is one LLOC. Mirrors `java_expressions_lloc`.
        check_metrics::<GroovyParser>(
            "int a = 1
            a = 2
            a += 3
            println(a)
            doSomething()",
            "foo.groovy",
            |metric| {
                // 5 expression-statement lines.
                assert_eq!(metric.loc.lloc(), 5);
            },
        );
    }

    #[test]
    fn groovy_throw_lloc() {
        // `throw` is one LLOC via the `ThrowStatement` arm.
        check_metrics::<GroovyParser>(
            "throw new RuntimeException('bad')",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.lloc(), 1);
            },
        );
    }

    #[test]
    fn groovy_general_loc() {
        // Comprehensive mix: class + method + control flow.
        // Mirrors `java_general_loc`'s coverage shape.
        //
        // LLOC = 4, fully attributable:
        //   IfStatement (the outer if/else):     +1
        //   `println(x)`     (JuxtFunctionCall):  +1
        //   `println 'neg'` (JuxtFunctionCall):  +1
        //   `return`        (ReturnStatement):   +1
        // The else-branch's `expression_statement (closure)`
        // wrapper does NOT count — see the bare-Closure carve-out
        // in `impl Loc for GroovyCode::compute`.
        check_metrics::<GroovyParser>(
            "class A {
                void f(int x) {
                    if (x > 0) {
                        println(x)
                    } else {
                        println 'neg'
                    }
                    return
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
            },
        );
    }

    #[test]
    fn csharp_local_function_in_method_lloc() {
        // C# local functions (`int Inner(int x) { ... }` inside `Bar()`)
        // open their own function space, so the outer method sees only
        // `return Inner(2);` plus the inner body's `return x + 1;`.
        check_metrics::<CsharpParser>(
            "class Foo {\n    int Bar() {\n        int Inner(int x) { return x + 1; }\n        return Inner(2);\n    }\n}\n",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn csharp_lambda_in_method_lloc() {
        // C# lambdas are expressions: the two LLOC come from the
        // `Func<int,int> f = x => x + 1;` declaration and the `f(3);` call.
        check_metrics::<CsharpParser>(
            "class Foo {\n    void Bar() {\n        System.Func<int, int> f = x => x + 1;\n        f(3);\n    }\n}\n",
            "foo.cs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn cpp_lambda_in_function_lloc() {
        // C++11 lambdas are expressions. The outer function `bar()` produces
        // two LLOC for the body: `auto f = [](int x) { return x + 1; };` and
        // `return f(2);`. The lambda's inner `return x + 1;` is part of the
        // lambda body inside the same function space (lambdas do not open a
        // new FuncSpace in this implementation), so it adds a third LLOC.
        // Closes the parity gap with #195 (which covered 11 other
        // languages but omitted C++).
        check_metrics::<CppParser>(
            "int bar() {\n    auto f = [](int x) { return x + 1; };\n    return f(2);\n}\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_nested_function_lloc() {
        // Nested function_declaration: 4 LLOC = outer's `return inner();`,
        // inner's `return 1;`, plus the two function declarations
        // themselves (the JS Checker counts function declarations as LLOC).
        check_metrics::<JavascriptParser>(
            "function outer() {\n    function inner() {\n        return 1;\n    }\n    return inner();\n}\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn javascript_arrow_function_lloc() {
        // The arrow function `(x) => x + 1` is an expression: the LLOC
        // come from `const inner = ...;` and `return inner(2);`.
        check_metrics::<JavascriptParser>(
            "function outer() {\n    const inner = (x) => x + 1;\n    return inner(2);\n}\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_lambda_literal_in_fun_lloc() {
        // A lambda literal (`{ x -> x + 1 }`) assigned to a `val` plus the
        // following call yields two LLOC at the outer function.
        check_metrics::<KotlinParser>(
            "fun outer() {\n    val f: (Int) -> Int = { x -> x + 1 }\n    f(3)\n}\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_local_fun_in_fun_lloc() {
        // Kotlin's local `fun inner(...)` is also a function_declaration,
        // so it opens its own space; the outer LLOC reduces to `inner(3)`,
        // and the inner body contributes the second LLOC.
        check_metrics::<KotlinParser>(
            "fun outer() {\n    fun inner(x: Int): Int { return x + 1 }\n    inner(3)\n}\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn kotlin_object_expression_in_fun_lloc() {
        // An `object : Runnable { ... }` expression with an overridden
        // method whose body invokes `println("hi")`. LLOC: `val r = ...`,
        // the override's body call, and the outer `r.run()` call = 3.
        check_metrics::<KotlinParser>(
            "fun outer() {\n    val r = object : Runnable { override fun run() { println(\"hi\") } }\n    r.run()\n}\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn go_function_literal_initializer_lloc() {
        // `inner := func(x int) int { return x + 1 }` — the function
        // literal opens its own space; LLOC visible on the outer space:
        // the assignment + `return inner(2)` = 2, plus the literal's
        // `return x + 1` body = 3 aggregated.
        check_metrics::<GoParser>(
            "package main\nfunc outer() int {\n    inner := func(x int) int { return x + 1 }\n    return inner(2)\n}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_anonymous_function_in_function_lloc() {
        // Anonymous function `function ($x) { return $x + 1; }`: outer
        // sees the assignment + `return $f(2);`, the closure body adds
        // `return $x + 1;` for 3 LLOC aggregated.
        check_metrics::<PhpParser>(
            "<?php\nfunction outer() {\n    $f = function ($x) { return $x + 1; };\n    return $f(2);\n}\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn php_arrow_function_in_function_lloc() {
        // The `fn ($x) => $x + 1` arrow function is an expression; the
        // outer function sees only its assignment and the `return $f(2);`.
        check_metrics::<PhpParser>(
            "<?php\nfunction outer() {\n    $f = fn ($x) => $x + 1;\n    return $f(2);\n}\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_nested_local_function_lloc() {
        // Two nested `local function` declarations: outer + inner both
        // count as `function_declaration` LLOC, plus the two `return`
        // statements = 4 aggregated.
        check_metrics::<LuaParser>(
            "local function outer()\n    local function inner()\n        return 1\n    end\n    return inner()\nend\n",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn lua_function_expression_in_local_decl_lloc() {
        // `local f = function (x) return x + 1 end` — the function
        // expression is its own space; aggregated LLOC: outer
        // declaration, the inner expression's declaration, the inner
        // `return x + 1`, and the outer `return f(2)` = 4.
        check_metrics::<LuaParser>(
            "local function outer()\n    local f = function (x) return x + 1 end\n    return f(2)\nend\n",
            "foo.lua",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn tcl_apply_closure_lloc() {
        // `apply $f 2` is a regular Tcl command, not a separate function
        // space — tree-sitter-tcl does not model `apply { ... }` as a
        // closure construct distinct from any other command. We assert
        // the observed LLOC (proc, set, apply, plus the nested `expr`
        // command substitution inside the lambda body) so any future
        // change to lambda-body counting is caught here.
        check_metrics::<TclParser>(
            "proc outer {} {\n    set f [list x {return [expr {$x + 1}]}]\n    apply $f 2\n}\n",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn perl_anonymous_sub_in_sub_lloc() {
        // Anonymous sub `sub { ... }` opens its own function space; the
        // outer LLOC counts the `my $f = ...;` declaration plus
        // `return $f->(2);`, and the anonymous sub contributes
        // `return $_[0] + 1;` for 2 LLOC.
        //
        // NOTE: a prior LLOC for this construct exists as
        // `perl_lloc_anonymous_function` (top-level form) — this test
        // asserts the same shape *inside* another sub, exercising space
        // nesting.
        check_metrics::<PerlParser>(
            "sub outer {\n    my $f = sub { return $_[0] + 1 };\n    return $f->(2);\n}\n",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 2);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn perl_named_sub_in_sub_lloc() {
        // Perl `sub` declarations are not LLOC (see
        // `perl_lloc_function_definition_not_counted`); inside `outer`,
        // only `return inner();` is LLOC, and `inner`'s `return 1` is in
        // its own space contributing one more aggregated LLOC.
        // Total aggregated LLOC: 1.
        //
        // Observation: lloc=1, not 2. Perl LLOC is anchored on `;`
        // tokens whose parent is `SourceFile` or `Block` (see
        // `PerlCode::compute` in this file). The bare `return 1` inside
        // `sub inner { ... }` has no trailing `;`, so it does not bump
        // LLOC. The outer `return inner();` carries the only SEMI.
        // This is intentional Perl behaviour and not a bug — Perl
        // requires `;` between statements; a single trailing statement
        // before `}` is syntactically optional. Asserted as-is.
        check_metrics::<PerlParser>(
            "sub outer {\n    sub inner { return 1 }\n    return inner();\n}\n",
            "foo.pl",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 1);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    #[test]
    fn elixir_fn_inside_def_lloc() {
        // `fn x -> x + 1 end` inside a `def`: defmodule + def +
        // `f = fn ...` + `f.(2)` = 4 own LLOC for the Unit space, plus
        // the anonymous fn body `x + 1` = 1 nested, aggregated 5.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def outer do\n    f = fn x -> x + 1 end\n    f.(2)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 6);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
                insta::assert_json_snapshot!(metric.loc);
            },
        );
    }

    /// Regression for #437: `Loc` min/max must fold each nested function
    /// space's *own* min/max, not its aggregate value, so the smallest
    /// (and largest) leaf function propagates to the root. Before the fix,
    /// merge folded `other.sloc()` and a guarded `compute_minmax` skipped
    /// containers, so `sloc_min` reflected only top-level spaces.
    ///
    /// Layout: Unit (whole file) -> class C -> two methods of *different*
    /// sizes. The smaller method is the global minimum; the file/class
    /// spans are larger; `sloc_min` must be the small leaf, not the class
    /// or unit span. Verified against the pre-fix code by reverting the
    /// merge/compute_minmax change (it reports the unit span instead).
    #[test]
    fn rust_nested_min_max_propagates() {
        check_metrics::<RustParser>(
            "struct C;\nimpl C {\n    fn small(&self) {\n        let _ = 1;\n    }\n    fn big(&self) {\n        let _ = 1;\n        let _ = 2;\n        let _ = 3;\n    }\n}\n",
            "c.rs",
            |metric| {
                // Spaces: Unit + impl C + small() + big().
                // small() spans 3 rows (signature .. closing brace),
                // big() spans 5 rows. The Unit/impl spans are larger
                // still. sloc_min must be the smallest leaf (3), not the
                // top-level span; sloc_max must be the largest space.
                let loc = &metric.loc;
                assert!(
                    loc.sloc_min() <= loc.sloc(),
                    "sloc_min {} must not exceed unit sloc {}",
                    loc.sloc_min(),
                    loc.sloc()
                );
                assert_eq!(loc.sloc_min(), 3, "smallest leaf method span");
                assert_eq!(loc.sloc_max(), loc.sloc(), "largest space is the unit");
                // The smallest leaf has one statement; min must reflect it.
                assert_eq!(loc.lloc_min(), 1);
            },
        );
    }

    /// Java sibling of `rust_nested_min_max_propagates` (#437). The
    /// `java_class_loc` snapshot above showed the bug directly: a class
    /// with methods reported `sloc_min == sloc` (the unit span). Here we
    /// assert the smallest method propagates.
    #[test]
    fn java_nested_min_max_propagates() {
        check_metrics::<JavaParser>(
            "public class C {\n  void small() {\n    int x = 1;\n  }\n  void big() {\n    int a = 1;\n    int b = 2;\n    int c = 3;\n  }\n}\n",
            "C.java",
            |metric| {
                // Spaces: Unit + class C + small() + big().
                let loc = &metric.loc;
                assert!(loc.sloc_min() <= loc.sloc());
                assert_eq!(loc.sloc_min(), 3, "smallest leaf method span");
                assert_eq!(loc.sloc_max(), loc.sloc());
                assert_eq!(loc.lloc_min(), 1);
            },
        );
    }

    /// Python sibling of `rust_nested_min_max_propagates` (#437). Python
    /// nesting is class -> method, mirroring the worked example in the
    /// issue (file -> class C -> method m).
    #[test]
    fn python_nested_min_max_propagates() {
        check_metrics::<PythonParser>(
            "class C:\n    def small(self):\n        x = 1\n    def big(self):\n        a = 1\n        b = 2\n        c = 3\n",
            "c.py",
            |metric| {
                // Spaces: Unit + class C + small() + big().
                let loc = &metric.loc;
                assert!(loc.sloc_min() <= loc.sloc());
                assert_eq!(loc.sloc_min(), 2, "smallest leaf method span");
                assert_eq!(loc.sloc_max(), loc.sloc());
                assert_eq!(loc.lloc_min(), 1);
            },
        );
    }

    /// `blank()` is `sloc - ploc - only_comment_lines`, an f64 subtraction
    /// that can go negative when a space's physical and comment line
    /// attribution overlaps its span row count. It must clamp at 0 so the
    /// serialized value is never negative (#437).
    ///
    /// Built from a synthetic [`Stats`] rather than a parsed fixture: the
    /// current grammars do not emit a parsed space whose root `sloc` is
    /// smaller than `ploc + only_comment_lines`, so a source fixture cannot
    /// drive the subtraction negative and would pass against the pre-clamp
    /// code (proving nothing). Setting `sloc = ploc = only_comment_lines = 1`
    /// yields a pre-clamp `blank` of `1 - 1 - 1 = -1`; reverting the
    /// `.max(0.0)` makes this assertion fail with `-1`.
    #[test]
    fn blank_clamps_negative_to_zero() {
        let mut stats = Stats::default();
        // A single-row span ending mid-line => sloc() of 1 row.
        stats.sloc.start = 0;
        // End row 0 ending mid-line, so that row counts: sloc() == 1.
        stats.sloc.end_line = 1;
        // ploc() is the cardinality of the physical-line set => 1.
        stats.ploc.lines.insert(0);
        // One comment-only line on the same single row.
        stats.cloc.only_comment_line_starts.insert(0);

        // Pre-clamp this is 1 - 1 - 1 = -1.
        assert_eq!(stats.sloc(), 1);
        assert_eq!(stats.ploc(), 1);
        assert_eq!(
            stats.blank(),
            0,
            "blank() must clamp the negative subtraction to 0"
        );
    }

    /// A physical line shared by two *sibling* function spaces must
    /// count once in the parent that merges them.
    ///
    /// This is the semantics `Ploc::merge` / `Cloc::merge` encode, and
    /// the reason the per-space line stores are sets rather than
    /// counters. It is asserted here across four language families —
    /// Rust, the C family, the JS family, and Python's indentation-based
    /// spaces — because the merge is shared by all of them and each
    /// family reaches it through a different `Loc::compute` body.
    ///
    /// Every fixture puts both spaces on one row, so a merge that summed
    /// instead of unioning would report `ploc`/`cloc` of 2 against an
    /// `sloc` of 1 — an impossible reading that also drives `blank`
    /// negative.
    #[test]
    fn sibling_spaces_sharing_a_line_count_it_once() {
        check_metrics::<RustParser>(
            "fn a() { let x = 1; } fn b() { let y = 2; }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.ploc(), 1, "one physical row, two spaces");
                assert_eq!(metric.loc.blank(), 0);
            },
        );

        check_metrics::<CppParser>(
            "int a() { return 1; } int b() { return 2; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.ploc(), 1, "one physical row, two spaces");
                assert_eq!(metric.loc.blank(), 0);
            },
        );

        check_metrics::<JavascriptParser>(
            "function a() { let x = 1; } function b() { let y = 2; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.ploc(), 1, "one physical row, two spaces");
                assert_eq!(metric.loc.blank(), 0);
            },
        );

        // Python cannot open two spaces on one row, so the shared row is
        // the nested `def`'s: it belongs to the inner space's span and to
        // the outer space's, and the unit merges both.
        check_metrics::<PythonParser>(
            "def a():
    def b(): return 1",
            "foo.py",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert_eq!(metric.loc.ploc(), 2, "two physical rows, three spaces");
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// The same union property for comment lines: two sibling spaces that
    /// each carry a comment on the one shared row must yield `cloc == 1`.
    ///
    /// Summing rather than unioning would report `cloc == 2` against
    /// `sloc == 1`, the `cloc > sloc` state that pushes MI's
    /// comments_percentage above 100% (the failure mode of issue #461,
    /// here across the space merge rather than within one space).
    #[test]
    fn sibling_spaces_sharing_a_comment_line_count_it_once() {
        check_metrics::<CppParser>(
            "int a() { /*x*/ return 1; } int b() { /*y*/ return 2; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.cloc(), 1, "one comment row, two spaces");
                assert!(
                    metric.loc.cloc() <= metric.loc.sloc(),
                    "cloc must never exceed sloc"
                );
            },
        );

        check_metrics::<RustParser>(
            "fn a() { /*x*/ let p = 1; } fn b() { /*y*/ let q = 2; }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.loc.sloc(), 1);
                assert_eq!(metric.loc.cloc(), 1, "one comment row, two spaces");
                assert!(
                    metric.loc.cloc() <= metric.loc.sloc(),
                    "cloc must never exceed sloc"
                );
            },
        );
    }

    /// A row inside a chain of nested spaces is folded upward once per
    /// level, and must still count once at the top.
    ///
    /// Fifteen levels rather than two: at one level a union and a sum
    /// agree whenever the sets happen to be disjoint, and the point of
    /// #1109 is the repeated fold. The body row belongs to every level's
    /// span, so a merge that accumulated would report `ploc == 15`.
    #[test]
    fn a_row_folded_through_nested_spaces_counts_once() {
        const DEPTH: usize = 15;
        let source = format!(
            "{}let x = 1;{}",
            "fn f() { ".repeat(DEPTH),
            "} ".repeat(DEPTH)
        );

        check_metrics::<RustParser>(&source, "foo.rs", |metric| {
            assert_eq!(metric.loc.sloc(), 1);
            assert_eq!(metric.loc.ploc(), 1, "one physical row, {DEPTH} spaces");
            assert_eq!(metric.loc.ploc_max(), 1);
            assert_eq!(metric.loc.blank(), 0);
        });
    }

    /// Two inline block comments on a single code line must count as a
    /// single comment line, not one per comment node. Pre-fix this
    /// reported `cloc = 2` (one increment per node) for a one-line
    /// construct, violating `cloc <= sloc`/`cloc <= ploc` and pushing
    /// the MI comments_percentage above 100% (issue #461). Reverting
    /// the per-line de-dup in `add_code_comment_line` makes the
    /// `cloc == 1` assertions fail with `2`.
    #[test]
    fn cloc_multiple_block_comments_one_line_cpp() {
        check_metrics::<CppParser>(
            "int f(int /*a*/, int /*b*/) { return 1; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.loc.cloc(), 1, "two inline comments => 1 cloc");
                assert!(
                    metric.loc.cloc() <= metric.loc.sloc(),
                    "cloc must not exceed sloc"
                );
                assert!(
                    metric.loc.cloc() <= metric.loc.ploc(),
                    "cloc must not exceed ploc for a single-line construct"
                );
            },
        );
    }

    /// Sibling-language coverage: `add_cloc_lines` is shared across
    /// every block-comment language, so the Rust path must behave
    /// identically to C++ (issue #461).
    #[test]
    fn cloc_multiple_block_comments_one_line_rust() {
        check_metrics::<RustParser>(
            "fn f(/*a*/ x: i32, /*b*/ y: i32) -> i32 { 1 }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.loc.cloc(), 1, "two inline comments => 1 cloc");
                assert!(
                    metric.loc.cloc() <= metric.loc.sloc(),
                    "cloc must not exceed sloc"
                );
            },
        );
    }

    /// Guard against over-de-dup: a single multi-line block comment
    /// must still contribute one comment line per physical line it
    /// spans. The de-dup keys on the start row only, so the three
    /// independent continuation lines are unaffected (issue #461).
    #[test]
    fn cloc_multiline_block_comment_counts_each_line() {
        check_metrics::<CppParser>(
            "int g() {\n  /* l1\n     l2\n     l3 */\n  return 0;\n}",
            "foo.cpp",
            |metric| {
                assert_eq!(
                    metric.loc.cloc(),
                    3,
                    "a 3-line block comment counts 3 comment lines"
                );
            },
        );
    }

    /// Two *standalone* block comments on a single physical line (no
    /// code) must count as one comment line, not one per node. #461
    /// deduped only inline co-located comments via the code-comment
    /// path; the standalone path bumped `only_comment_lines` per node,
    /// so `/*a*/ /*b*/` reported `cloc = 2` for a single line —
    /// violating `cloc <= sloc`. Reverting the per-line set in
    /// `add_only_comment_lines` makes the `cloc == 1` assertion fail
    /// with `2` (verified by reverting to `only_comment_lines += …`).
    #[test]
    fn cloc_multiple_standalone_block_comments_one_line_cpp() {
        check_metrics::<CppParser>("/*a*/ /*b*/", "foo.cpp", |metric| {
            assert_eq!(
                metric.loc.cloc(),
                1,
                "two standalone comments on one line => 1 cloc"
            );
            assert_eq!(metric.loc.sloc(), 1);
            assert!(
                metric.loc.cloc() <= metric.loc.sloc(),
                "cloc must not exceed sloc"
            );
        });
    }

    /// Sibling-language coverage: the standalone de-dup lives in the
    /// shared `add_only_comment_lines` helper, so Rust must match C++
    /// (issue #461 follow-up).
    #[test]
    fn cloc_multiple_standalone_block_comments_one_line_rust() {
        check_metrics::<RustParser>("/*a*/ /*b*/", "foo.rs", |metric| {
            assert_eq!(
                metric.loc.cloc(),
                1,
                "two standalone comments on one line => 1 cloc"
            );
            assert_eq!(metric.loc.sloc(), 1);
            assert!(
                metric.loc.cloc() <= metric.loc.sloc(),
                "cloc must not exceed sloc"
            );
        });
    }

    /// File-level guard: a comment-only physical line followed by a
    /// real code line still counts the comment line once and never
    /// exceeds sloc, even after the per-space merge that previously
    /// summed `code_comment_lines` (and double-counted a boundary line)
    /// rather than reading the per-line set. Three standalone comments
    /// share line 1, so the whole file has exactly one comment line.
    #[test]
    fn cloc_standalone_comments_then_code_no_double_count() {
        check_metrics::<CppParser>("/*a*/ /*b*/ /*c*/\nint x = 1;\n", "foo.cpp", |metric| {
            assert_eq!(metric.loc.sloc(), 2);
            assert_eq!(
                metric.loc.cloc(),
                1,
                "only line 1 carries comments => 1 cloc"
            );
            assert!(
                metric.loc.cloc() <= metric.loc.sloc(),
                "cloc must not exceed sloc"
            );
        });
    }

    /// Interior blank lines are counted as BLANK and excluded from PLOC.
    /// sloc 6 (every line) / ploc 4 (4 code lines) / lloc 3 (handler + 2
    /// `set`s) / cloc 0 / blank 2.
    #[test]
    fn irules_blank() {
        check_metrics::<IrulesParser>(
            "when X {\n\n    set x 1\n\n    set y 2\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.sloc(), 6);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 2);
            },
        );
    }

    /// A handler body with no blank lines reports zero BLANK. lloc 3 =
    /// handler + `set` + `log` command.
    #[test]
    fn irules_no_zero_blank() {
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {\n    set x 1\n    log local0. $x\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 3);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// `#`-prefixed comment lines are counted as CLOC (iRules has no block
    /// comments, so each comment node spans exactly one line).
    ///
    /// expected: rows 0-2 are comment-only, row 3 is the sole code row.
    /// `ploc` and `sloc` are asserted, not just `cloc`/`blank`: without
    /// them this test passed all the way through #1135, which credited
    /// each of the three comment rows to PLOC as well (`ploc == 4`,
    /// `cloc + ploc == 7` against `sloc == 4`).
    #[test]
    fn irules_cloc() {
        check_metrics::<IrulesParser>(
            "# a\n# b\n# c\nwhen X { set x 1 }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 1);
                assert_eq!(metric.loc.cloc(), 3);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// LLOC counts each statement once: handler header, `if`, `set`, and the
    /// generic `log` command = 4. The `switch_arm` headers are not counted
    /// (their bodies' commands are), verified in `irules_switch_lloc`.
    #[test]
    fn irules_lloc() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { $a } {\n        set x 1\n    }\n    log local0. done\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    /// A command inside `[...]` (`command_substitution`) is a sub-expression,
    /// not a top-level statement, so it does not add to LLOC. Here lloc 2 =
    /// handler + `set`; the inner `expr` is NOT counted. Removing the
    /// `CommandSubstitution` guard would push lloc to 3 — this is the loc
    /// gating-decision regression test.
    #[test]
    fn irules_no_command_substitution_lloc() {
        check_metrics::<IrulesParser>(
            "when X {\n    set y [expr { 1 + 2 }]\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    /// `switch` counts once; each arm's *body* command counts, but the
    /// `switch_arm` pattern/body pair itself is not a logical line. lloc 4 =
    /// handler + `switch` + two `set`s (one per arm body).
    #[test]
    fn irules_switch_lloc() {
        check_metrics::<IrulesParser>(
            "when X {\n    switch $h {\n        a { set r 1 }\n        b { set r 2 }\n    }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.lloc(), 4);
            },
        );
    }

    /// A `proc` definition and its `return` command are each one logical
    /// line: lloc 2.
    #[test]
    fn irules_proc_lloc() {
        check_metrics::<IrulesParser>(
            "proc f { a } {\n    return $a\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.loc.lloc(), 2);
            },
        );
    }

    /// Objective-C blank-line accounting: two code lines separated by
    /// blank lines.
    #[test]
    fn objc_blank() {
        check_metrics::<ObjcParser>(
            "

            int a = 42;

            int b = 43;

            ",
            "foo.m",
            |metric| {
                assert_eq!(metric.loc.blank(), 1);
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 3,
                  "ploc": 2,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 1,
                  "sloc_average": 3.0,
                  "ploc_average": 2.0,
                  "lloc_average": 2.0,
                  "cloc_average": 0.0,
                  "blank_average": 1.0,
                  "sloc_min": 3,
                  "sloc_max": 3,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 2,
                  "ploc_max": 2,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 1,
                  "blank_max": 1
                }
                "#);
            },
        );
    }

    /// Objective-C comment accounting: a block comment and a line
    /// comment each contribute to `cloc`.
    #[test]
    fn objc_cloc() {
        check_metrics::<ObjcParser>(
            "/* Block comment
            still the block */
            // Line comment
            int a = 42; // trailing",
            "foo.m",
            |metric| {
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 4,
                  "ploc": 1,
                  "lloc": 1,
                  "cloc": 4,
                  "blank": 0,
                  "sloc_average": 4.0,
                  "ploc_average": 1.0,
                  "lloc_average": 1.0,
                  "cloc_average": 4.0,
                  "blank_average": 0.0,
                  "sloc_min": 4,
                  "sloc_max": 4,
                  "cloc_min": 4,
                  "cloc_max": 4,
                  "ploc_min": 1,
                  "ploc_max": 1,
                  "lloc_min": 1,
                  "lloc_max": 1,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C logical-line accounting: a method whose body has three
    /// statements. The `method_definition` opens a function space but is
    /// not itself a logical line; each statement adds one.
    #[test]
    fn objc_lloc() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (int)bar {
    int a = 1;
    int b = 2;
    return a + b;
}
@end
",
            "foo.m",
            |metric| {
                // expected: decl `int a` (1) + decl `int b` (1) +
                // `return` (1) = 3.
                assert_eq!(metric.loc.lloc(), 3);
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 7,
                  "ploc": 7,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.3333333333333335,
                  "ploc_average": 2.3333333333333335,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 7,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 7,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C for-header gating: the `int i = 0` declaration in a
    /// classic `for` init slot is part of the `for` statement's logical
    /// line and must NOT add a second one (mirrors the C / C++ gate).
    /// Reverting the `count_specific_ancestors` gate would push lloc from
    /// 2 to 3.
    #[test]
    fn objc_no_declaration_in_for_header_lloc() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar {
    for (int i = 0; i < 10; ++i) {
        [self use:i];
    }
}
@end
",
            "foo.m",
            |metric| {
                // expected: for-statement (1) + body expression
                // `[self use:i]` (1) = 2. The header `int i = 0`
                // declaration is gated out; without the gate this is 3.
                assert_eq!(metric.loc.lloc(), 2);
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 7,
                  "ploc": 7,
                  "lloc": 2,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 2.3333333333333335,
                  "ploc_average": 2.3333333333333335,
                  "lloc_average": 0.6666666666666666,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 5,
                  "sloc_max": 7,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 5,
                  "ploc_max": 7,
                  "lloc_min": 2,
                  "lloc_max": 2,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    #[test]
    fn objc_at_directives_lloc() {
        // The only ObjC-specific LLOC work the impl does beyond the C
        // inheritance: `@synchronized` is a dedicated `synchronized_statement`
        // node (counts as a logical line), but `@autoreleasepool` emits
        // only a keyword token with no wrapping node, so its *header* adds
        // nothing and only its inner statements count.
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar {
    @synchronized (self) {
        [self use];
    }
    @autoreleasepool {
        [self use];
    }
}
@end
",
            "foo.m",
            |metric| {
                // expected: @synchronized statement (1) + its body
                // `[self use]` (1) + the @autoreleasepool body `[self use]`
                // (1) = 3. The `@autoreleasepool` header contributes no
                // logical line (it has no node); if it did, this would be 4.
                assert_eq!(metric.loc.lloc(), 3);
                insta::assert_json_snapshot!(metric.loc, @r#"
                {
                  "sloc": 10,
                  "ploc": 10,
                  "lloc": 3,
                  "cloc": 0,
                  "blank": 0,
                  "sloc_average": 3.3333333333333335,
                  "ploc_average": 3.3333333333333335,
                  "lloc_average": 1.0,
                  "cloc_average": 0.0,
                  "blank_average": 0.0,
                  "sloc_min": 8,
                  "sloc_max": 10,
                  "cloc_min": 0,
                  "cloc_max": 0,
                  "ploc_min": 8,
                  "ploc_max": 10,
                  "lloc_min": 3,
                  "lloc_max": 3,
                  "blank_min": 0,
                  "blank_max": 0
                }
                "#);
            },
        );
    }

    /// Analyses `source` byte-for-byte as Rust.
    ///
    /// Goes through `metrics_verbatim` rather than `check_metrics`
    /// because the #1051 cases end at EOF; see that helper for why.
    fn rust_loc(source: &[u8]) -> Stats {
        metrics_verbatim(crate::LANG::Rust, source, crate::MetricsOptions::default()).loc
    }

    /// #1051: a Rust doc comment ending at EOF has no trailing newline for
    /// the scanner to consume, so its `LineComment` node ends on its own
    /// start row and discounting a row underflowed. On row 0 that panicked
    /// (debug: the subtraction; release: a hash-table capacity overflow in
    /// `add_only_comment_lines`). On any later row release did not crash —
    /// it silently reported one `cloc` too few.
    #[test]
    fn rust_doc_comment_at_eof_does_not_underflow() {
        // `end == start == 0` — underflowed at the subtraction itself.
        // expected: the sole row is one comment-only line, no code.
        let outer = rust_loc(b"/// x");
        assert_eq!(outer.cloc(), 1);
        assert_eq!(outer.ploc(), 0);

        let inner = rust_loc(b"//! x");
        assert_eq!(inner.cloc(), 1);
        assert_eq!(inner.ploc(), 0);

        // `end == start > 0` — the subtraction succeeded but drove `end`
        // below `start`. Release silently counted `cloc == 0` here.
        // expected: row 0 is code, row 1 is comment-only.
        let after_code = rust_loc(b"fn f(){}\n/// x");
        assert_eq!(after_code.cloc(), 1);
        assert_eq!(after_code.ploc(), 1);

        // Two doc comments, the second at EOF: both rows are comment-only.
        // Pre-fix release reported 1 here, not 2.
        assert_eq!(rust_loc(b"/// a\n/// b").cloc(), 2);

        // A doc comment sharing its row with code. `let` is not valid at
        // file scope, but tree-sitter parses it as a clean `let_declaration`
        // + `line_comment`, which is the only way to reach the
        // comment-after-code branch (no *valid* Rust puts `///` after code).
        let trailing = rust_loc(b"let x = 1; /// d");
        assert_eq!(trailing.cloc(), 1);
        assert_eq!(trailing.ploc(), 1);
    }

    /// A doc comment at EOF must count exactly like a plain line comment at
    /// EOF. The `DocComment` adjustment exists only to discount the newline
    /// the scanner consumes; at EOF there is none to discount, so the two
    /// shapes are indistinguishable for LOC purposes.
    #[test]
    fn rust_doc_comment_at_eof_matches_plain_comment() {
        let plain = rust_loc(b"// x");
        // Pin the baseline absolutely too: parity alone would still hold if
        // both sides moved together, and would then read as a regression
        // when the un-newline-terminated `sloc` accounting is corrected.
        assert_eq!(plain.cloc(), 1);
        assert_eq!(plain.ploc(), 0);

        for doc in [&b"/// x"[..], &b"//! x"[..]] {
            let doc = rust_loc(doc);
            assert_eq!(doc.cloc(), plain.cloc());
            assert_eq!(doc.ploc(), plain.ploc());
            assert_eq!(doc.sloc(), plain.sloc());
            assert_eq!(doc.blank(), plain.blank());
        }
    }

    /// The newline-terminated path must stay unchanged by the #1051 guard.
    /// A `DocComment` node really does span one row more than it renders
    /// whenever the scanner consumed a newline, and that row must still be
    /// excluded — otherwise the guard would silently become a no-op and
    /// inflate CLOC for every doc-commented Rust file.
    #[test]
    fn rust_doc_comment_with_trailing_newline_still_discounts_the_row() {
        // expected: one rendered comment row, not two.
        assert_eq!(rust_loc(b"/// x\n").cloc(), 1);
        // expected: two consecutive doc comments are two rows, not four.
        assert_eq!(rust_loc(b"/// a\n/// b\n").cloc(), 2);

        // The common real-world shape: doc comment attached to an item.
        // expected: row 0 comment-only, row 1 code.
        let documented = rust_loc(b"/// doc\nfn f() {}\n");
        assert_eq!(documented.cloc(), 1);
        assert_eq!(documented.ploc(), 1);
    }

    /// CRLF is the boundary the guard must *not* fire on. `\r` is ordinary
    /// content to `process_line_doc_content`, so it consumes the following
    /// `\n` and the node does span an extra row — the discount is still
    /// owed. A lone trailing `\r` at EOF is the opposite case. Without this,
    /// a future grammar bump that stops consuming the newline would leave
    /// every LF test passing while the discount silently became dead code.
    #[test]
    fn rust_doc_comment_crlf_still_discounts_the_row() {
        // Newline consumed despite the `\r`: discount applies.
        assert_eq!(rust_loc(b"/// x\r\n").cloc(), 1);
        // expected: row 0 code, row 1 comment-only.
        let after_code = rust_loc(b"fn f(){}\r\n/// x");
        assert_eq!(after_code.cloc(), 1);
        assert_eq!(after_code.ploc(), 1);
        // Lone `\r`, then EOF: no newline consumed, so no discount is owed.
        assert_eq!(rust_loc(b"/// x\r").cloc(), 1);
    }

    /// One-line, deliberately un-newline-terminated source, one entry per
    /// language whose `Loc` implementation is not a documented no-op.
    ///
    /// `sloc` is computed once, in the shared [`Sloc`], from the unit
    /// span the grammar hands us — so the only per-language variable is
    /// where each grammar puts the root node's end position. #1067 was
    /// possible precisely because that position was assumed rather than
    /// read, so the sweep is exhaustive rather than a sample: a future
    /// grammar whose root does *not* run to end-of-input has to show up
    /// here rather than silently lose a row.
    ///
    /// `Preproc` and `Ccomment` are excluded, but not because they are
    /// exempt from the rule. Their `Loc` impls are no-ops
    /// (`implement_metric_trait!(Loc, PreprocCode, CcommentCode)`, #188),
    /// so the *node-accumulated* sub-metrics are 0 by design — yet
    /// `sloc` is not node-accumulated: the walker anchors every Unit's
    /// row span at finalization, their synthetic Unit root included, so
    /// they carry a real span and drift with #1067 exactly as the
    /// languages below do. What they cannot join is the second sweep, whose final
    /// `mi != 0` assertion is unreachable with `ploc == 0`. They get
    /// their own check in
    /// `no_op_loc_grammars_still_count_their_unterminated_row`.
    const UNTERMINATED_ONE_LINERS: &[(crate::LANG, &[u8])] = &[
        (crate::LANG::Rust, b"fn main() {}"),
        (crate::LANG::C, b"int main(void) { return 0; }"),
        (crate::LANG::Cpp, b"int main() { return 0; }"),
        (crate::LANG::Mozcpp, b"int main() { return 0; }"),
        (crate::LANG::Objc, b"int main(void) { return 0; }"),
        (crate::LANG::Csharp, b"class C { void M() {} }"),
        (crate::LANG::Java, b"class C { void m() {} }"),
        (crate::LANG::Kotlin, b"fun main() {}"),
        (crate::LANG::Groovy, b"def f() {}"),
        (crate::LANG::Go, b"package main"),
        (crate::LANG::Javascript, b"function f() {}"),
        (crate::LANG::Mozjs, b"function f() {}"),
        (crate::LANG::Typescript, b"function f(): void {}"),
        (crate::LANG::Tsx, b"function f() {}"),
        (crate::LANG::Python, b"def f(): pass"),
        (crate::LANG::Ruby, b"def f; end"),
        (crate::LANG::Php, b"<?php function f() {}"),
        (crate::LANG::Perl, b"sub f { return 1; }"),
        (crate::LANG::Bash, b"f() { echo hi; }"),
        (crate::LANG::Lua, b"function f() end"),
        (crate::LANG::Tcl, b"proc f {} {}"),
        (crate::LANG::Irules, b"proc f {} {}"),
        (crate::LANG::Elixir, b"defmodule M do end"),
    ];

    /// #1067: `Sloc::sloc()` derived the unit's row count as `end - start`,
    /// which is only right when a trailing newline pushes the root node's
    /// end onto a phantom extra row. Source that stops mid-line — anything
    /// not newline-terminated — lost its final row, so a one-line file
    /// reported `sloc == 0`.
    ///
    /// Uses [`metrics_verbatim`], not `check_metrics`: the latter trims and
    /// re-appends a trailing newline, which makes this entire input class
    /// unreachable and the test vacuous (the same blind spot that hid
    /// #1051).
    #[test]
    fn unterminated_one_line_file_reports_one_source_line() {
        for (lang, source) in UNTERMINATED_ONE_LINERS {
            let text = String::from_utf8_lossy(source);
            let loc = metrics_verbatim(*lang, source, MetricsOptions::default()).loc;
            assert_eq!(loc.sloc(), 1, "{lang:?} sloc for {text:?}");
            // The largest space can never be bigger than the file itself.
            assert_eq!(loc.sloc_max(), 1, "{lang:?} sloc_max for {text:?}");
            // The invariant documented on `Stats::with_cloc_sloc`: every
            // physical line is code, comment, both, or blank, so the code
            // and comment-only tallies cannot together exceed the row
            // count. `sloc == 0` broke it for any unterminated file whose
            // last line carried content.
            assert!(
                loc.cloc() + loc.ploc() <= loc.sloc(),
                "{lang:?}: cloc {} + ploc {} exceeds sloc {} for {text:?}",
                loc.cloc(),
                loc.ploc(),
                loc.sloc(),
            );
        }
    }

    /// The two grammars whose `Loc` impl is the macro's no-op still get a
    /// `sloc`, so they drift with #1067 like everything else.
    ///
    /// Their root is not a `SpaceKind::Unit`, so `metrics_inner` pushes a
    /// synthetic Unit, whose row span `anchor_unit_sloc_span` fills in at
    /// finalization — a span the no-op `compute` never touches but
    /// `Sloc::sloc()` still measures.
    /// Before #1067 an unterminated one-liner measured `0` rows here too.
    /// Kept apart from [`UNTERMINATED_ONE_LINERS`] only because the
    /// `mi != 0` half of the sweep below cannot hold with `ploc == 0`.
    #[test]
    fn no_op_loc_grammars_still_count_their_unterminated_row() {
        for (lang, source) in [
            (crate::LANG::Preproc, &b"#define A 1"[..]),
            (crate::LANG::Ccomment, &b"/* c */"[..]),
        ] {
            let bare = metrics_verbatim(lang, source, MetricsOptions::default()).loc;
            assert_eq!(bare.sloc(), 1, "{lang:?} unterminated sloc");
            let mut terminated = source.to_vec();
            terminated.push(b'\n');
            let terminated = metrics_verbatim(lang, &terminated, MetricsOptions::default()).loc;
            assert_eq!(
                bare.sloc(),
                terminated.sloc(),
                "{lang:?} sloc must not depend on the trailing newline"
            );
            // The node-accumulated sub-metrics are the ones #188 zeroes.
            assert_eq!((bare.ploc(), bare.cloc(), bare.lloc()), (0, 0, 0));
        }
    }

    /// A backslash-continued `#define` body is one `PreprocArg` node
    /// spanning every continuation row, so each of those rows is PLOC.
    ///
    /// The four C-family `Loc` impls carry an identical arm for this
    /// (`tree-sitter-cpp` does not expand macros — see the comment at
    /// each site), and until #1229 only C++'s copy was exercised: the
    /// other three were the sole uncovered lines in that PR. They are
    /// deliberate clones, so a fixture for one is a fixture for all
    /// four, and `Mozcpp` in particular owns no file extension and can
    /// only be reached by naming the language.
    ///
    /// Measured, and confirmed discriminating by deleting the arm from
    /// all four modules: `ploc` is 4 with it and 3 without, the lost row
    /// being the macro's last continuation line. `sloc` is 5 (three
    /// macro rows, one blank, one `main`) and `lloc` is 1 — the single
    /// `return` statement — since a `#define` declares no statement.
    #[test]
    fn a_continued_macro_body_counts_every_row_it_spans() {
        // Rows: 0-2 are the macro, 3 is blank, 4 is `main`.
        const CONTINUED_MACRO: &[u8] =
            b"#define SUM(a, b) \\\n    ((a) + \\\n     (b))\n\nint main(void) { return SUM(1, 2); }\n";

        for lang in [
            crate::LANG::C,
            crate::LANG::Cpp,
            crate::LANG::Mozcpp,
            crate::LANG::Objc,
        ] {
            let loc = metrics_verbatim(lang, CONTINUED_MACRO, MetricsOptions::default()).loc;
            assert_eq!(
                loc.ploc(),
                4,
                "{lang:?}: every continuation row of the macro body is code"
            );
            assert_eq!(loc.sloc(), 5, "{lang:?} sloc");
            assert_eq!(loc.lloc(), 1, "{lang:?} lloc");
            assert_eq!(loc.cloc(), 0, "{lang:?} cloc");
            assert_eq!(loc.blank(), 1, "{lang:?} blank");
        }
    }

    /// Whether the last line ends in a newline is a formatting detail, not
    /// a property of the code — no LOC sub-metric, and therefore no MI
    /// value, may depend on it. This is the invariant #1067 violated, and
    /// it pins the fix from both sides: the newline-terminated path (the
    /// one every in-tree harness exercises) must not move either.
    ///
    /// **The invariant is now unconditional.** It used to be scoped to
    /// source containing a token: whitespace-only input collapsed most
    /// grammars' roots to a zero-width node at end-of-input, leaving
    /// `sloc` no span to measure, so `b"  "` reported one row and
    /// `b"  \n"` reported none (#1087). #1247 anchored the unit's `sloc`
    /// span to the span the unit reports, which removed that dependence
    /// on where the root node happens to start. The whitespace-only class
    /// is swept separately — it cannot ride this test, whose closing
    /// `assert_ne!` requires a non-zero MI — in
    /// [`whitespace_only_input_is_uniform_across_grammars`].
    #[test]
    fn trailing_newline_does_not_change_loc_or_mi() {
        for (lang, source) in UNTERMINATED_ONE_LINERS {
            let text = String::from_utf8_lossy(source);
            let bare = metrics_verbatim(*lang, source, MetricsOptions::default());
            let mut newline_terminated = source.to_vec();
            newline_terminated.push(b'\n');
            let terminated =
                metrics_verbatim(*lang, &newline_terminated, MetricsOptions::default());

            assert_eq!(
                bare.loc.sloc(),
                terminated.loc.sloc(),
                "{lang:?} sloc {text:?}"
            );
            assert_eq!(
                bare.loc.ploc(),
                terminated.loc.ploc(),
                "{lang:?} ploc {text:?}"
            );
            assert_eq!(
                bare.loc.cloc(),
                terminated.loc.cloc(),
                "{lang:?} cloc {text:?}"
            );
            assert_eq!(
                bare.loc.lloc(),
                terminated.loc.lloc(),
                "{lang:?} lloc {text:?}"
            );
            assert_eq!(
                bare.loc.blank(),
                terminated.loc.blank(),
                "{lang:?} blank {text:?}"
            );
            // The MI knock-on: `mi::inputs_are_empty` short-circuits to
            // 0.0 on `sloc <= 0`, so before the fix every unterminated
            // one-liner reported MI 0 while its newline-terminated twin
            // reported a real score. Inputs are now identical, so the
            // three formulas agree bit-for-bit.
            assert_eq!(
                bare.mi.original(),
                terminated.mi.original(),
                "{lang:?} mi {text:?}"
            );
            assert_eq!(
                bare.mi.sei(),
                terminated.mi.sei(),
                "{lang:?} mi.sei {text:?}"
            );
            assert_eq!(
                bare.mi.visual_studio(),
                terminated.mi.visual_studio(),
                "{lang:?} mi.visual_studio {text:?}",
            );
            assert_ne!(bare.mi.original(), 0.0, "{lang:?} mi must not be zeroed");
        }
    }

    /// The second #1067 symptom: `b"fn f(){}\n/// x"` reported `sloc == 1`
    /// with `ploc == 1` *and* `cloc == 1`, so `cloc + ploc > sloc`. The
    /// file has two rows; only the missing one made the sums disagree.
    #[test]
    fn unterminated_trailing_comment_upholds_the_cloc_ploc_invariant() {
        // expected: row 0 is code, row 1 is comment-only, nothing blank.
        let loc = rust_loc(b"fn f(){}\n/// x");
        assert_eq!(loc.sloc(), 2);
        assert_eq!(loc.ploc(), 1);
        assert_eq!(loc.cloc(), 1);
        assert_eq!(loc.blank(), 0);
    }

    /// Degenerate inputs, pinned so the end-column rule in
    /// `Node::end_line` cannot drift into fabricating rows for files
    /// that have none.
    #[test]
    fn degenerate_inputs_report_their_real_row_count() {
        // No bytes, no rows.
        assert_eq!(rust_loc(b"").sloc(), 0);
        // One row of whitespace, unterminated: the root node is empty but
        // sits at column 3, so the row is real and counts as blank.
        let spaces = rust_loc(b"   ");
        assert_eq!(spaces.sloc(), 1);
        assert_eq!(spaces.blank(), 1);
        // Newline-terminated whitespace was the #1087 carve-out: most
        // grammars collapse the root to a zero-width node at end-of-input
        // (`(1, 0)..(1, 0)` for `"\n"`), so the measured span had no rows
        // left to attribute and `sloc` was 0 for a file that plainly has
        // one. #1247 retired that: the unit's `sloc` span is now anchored
        // to the span the unit *reports*, which #1195 already anchored at
        // line 1, so a collapsed root no longer costs the file its rows.
        let one = rust_loc(b"\n");
        assert_eq!((one.sloc(), one.ploc(), one.blank()), (1, 0, 1));
        let two = rust_loc(b"\n\n");
        assert_eq!((two.sloc(), two.ploc(), two.blank()), (2, 0, 2));
    }

    /// Every grammar whose `Loc` behaviour this module owns, including the
    /// two whose `compute` is the `implement_metric_trait!` no-op.
    ///
    /// [`UNTERMINATED_ONE_LINERS`] carries a fixture per language because
    /// its sweeps need parseable code; the whitespace-only sweep needs
    /// only the language, and must not omit `Preproc`/`Ccomment` — their
    /// synthetic Unit root is anchored like any other, so they carry a
    /// real span and answer the #1087/#1247 question too.
    fn all_loc_grammars() -> impl Iterator<Item = crate::LANG> {
        UNTERMINATED_ONE_LINERS
            .iter()
            .map(|(lang, _)| *lang)
            .chain([crate::LANG::Preproc, crate::LANG::Ccomment])
    }

    /// #1087 accepted whitespace-only source as the one input class where
    /// a trailing newline moved `sloc` — twenty grammars collapse the root
    /// to a zero-width node at end-of-input, so `"  "` reported one row
    /// and `"  \n"` reported none, while five grammars (Elixir, Tcl,
    /// iRules, Preproc, Ccomment) kept the span and were newline-
    /// independent already. This sweep was written to pin both halves.
    ///
    /// #1247 removed the premise. The carve-out was a consequence of
    /// measuring the unit's `sloc` span from the root node's first token;
    /// once that span is anchored to the one the unit *reports* (#1195
    /// anchored the reported span at line 1), a collapsed root no longer
    /// erases the file's rows and every grammar answers alike. The sweep
    /// stays, with the split list retired: it now pins the *absence* of a
    /// per-grammar difference, which is the property a future grammar bump
    /// or walker change could still break.
    ///
    /// The unterminated side is unchanged and was always uniform: every
    /// grammar reports the row, as one blank line.
    #[test]
    fn whitespace_only_input_is_uniform_across_grammars() {
        // Spaces and tabs both, so a grammar that lexes one as extra and
        // the other as an error token cannot hide behind the sweep.
        for bare in [&b"  "[..], b"\t\t"] {
            let mut newline_terminated = bare.to_vec();
            newline_terminated.push(b'\n');
            for lang in all_loc_grammars() {
                let text = String::from_utf8_lossy(bare);
                let unterminated = metrics_verbatim(lang, bare, MetricsOptions::default()).loc;
                let terminated =
                    metrics_verbatim(lang, &newline_terminated, MetricsOptions::default()).loc;

                assert_eq!(
                    (unterminated.sloc(), unterminated.blank()),
                    (1, 1),
                    "{lang:?}: unterminated {text:?} is one blank row for every grammar"
                );

                assert_eq!(
                    (terminated.sloc(), terminated.blank()),
                    (1, 1),
                    "{lang:?}: newline-terminated {text:?} is the same one blank \
                     row — whether the grammar collapses its root at \
                     end-of-input is no longer observable in loc (#1247)"
                );

                // Whitespace is never code and never a comment, whichever
                // side of the carve-out the grammar sits on. This is the
                // assertion #1135 broke for Tcl and iRules, whose row
                // terminator used to land in their PLOC catch-all.
                for loc in [&unterminated, &terminated] {
                    assert_eq!(
                        (loc.ploc(), loc.cloc(), loc.lloc()),
                        (0, 0, 0),
                        "{lang:?}: whitespace is neither code nor comment"
                    );
                }
            }
        }
    }

    /// #1247: the unit anchors its *reported* span at line 1 (#1195) but
    /// measured its `sloc` span from the root node's first token, so blank
    /// rows above that token counted in neither `sloc` nor `blank` — while
    /// byte-identical rows one line lower counted in both.
    ///
    /// Swept across four grammars because the per-language `Loc` impls
    /// mirror each other. Tcl is in the list because it was already
    /// *right*: its row terminator is a token child of the root, so the
    /// root already started at row 0 and Tcl reported `sloc 4` for the
    /// Rust fixture's `sloc 1`. A per-language fix would have had to know
    /// which grammars were which; the walker-level anchor does not, and
    /// Tcl is the case that catches one being applied twice.
    ///
    /// `space_verbatim`, not `check_metrics`: the shim trims leading and
    /// trailing newlines, which deletes this test's entire subject.
    #[test]
    fn leading_blank_rows_count_in_the_units_sloc_and_blank() {
        const LEADING_BLANKS: u64 = 3;
        for (lang, body, ploc) in [
            (crate::LANG::Rust, &b"fn a() {}\n"[..], 1),
            (crate::LANG::Python, b"def a():\n    pass\n", 2),
            (crate::LANG::C, b"int f() { return 0; }\n", 1),
            (crate::LANG::Tcl, b"puts hi\n", 1),
        ] {
            let mut source = vec![b'\n'; LEADING_BLANKS as usize];
            source.extend_from_slice(body);
            let space = space_verbatim(lang, &source, MetricsOptions::default());
            let loc = &space.metrics.loc;
            assert_eq!(
                (loc.sloc(), loc.ploc(), loc.cloc(), loc.blank()),
                (LEADING_BLANKS + ploc, ploc, 0, LEADING_BLANKS),
                "{lang:?}: the leading rows are blank, not absent"
            );
            // The disagreement the issue is named for: the unit's own
            // reported span and its `sloc` are two spellings of one
            // number, and were not before.
            assert_eq!(
                loc.sloc() as usize,
                space.end_line - space.start_line + 1,
                "{lang:?}: sloc equals the rows of the unit's reported span"
            );
        }
    }

    /// The two controls from #1247's evidence table. Both were already
    /// correct, and both are how the inconsistency was visible at all: a
    /// comment on line 1 flipped a byte-identical file from `sloc 1` to
    /// `sloc 4`, because comments are in the tree and blank rows are not.
    /// A fix that reached past the unit would move one of these.
    #[test]
    fn interior_blanks_and_leading_comments_are_unmoved_by_the_anchor() {
        // expected: rows 1 and 3 are code, row 2 is blank.
        let interior = rust_loc(b"fn a() {}\n\nfn b() {}\n");
        assert_eq!(
            (
                interior.sloc(),
                interior.ploc(),
                interior.cloc(),
                interior.blank()
            ),
            (3, 2, 0, 1)
        );
        // expected: row 1 is comment-only, rows 2-3 blank, row 4 code.
        let leading_comment = rust_loc(b"// c\n\n\nfn a() {}\n");
        assert_eq!(
            (
                leading_comment.sloc(),
                leading_comment.ploc(),
                leading_comment.cloc(),
                leading_comment.blank()
            ),
            (4, 1, 1, 2)
        );
    }

    /// The anchor is gated on `SpaceKind::Unit`, and that gate is the
    /// entire separation between "the file starts at line 1" and "every
    /// space starts at line 1". Asserted on the nested space's `sloc` as
    /// well as its span, because only the `sloc` half is new.
    #[test]
    fn the_unit_anchor_does_not_reach_nested_spaces() {
        let space = space_verbatim(
            crate::LANG::Rust,
            b"\n\n\nfn a() {\n    let x = 1;\n}\n",
            MetricsOptions::default(),
        );
        assert_eq!((space.start_line, space.end_line), (1, 6));
        assert_eq!(space.metrics.loc.sloc(), 6, "the file has six rows");
        let nested = &space.spaces[0];
        assert_eq!((nested.start_line, nested.end_line), (4, 6));
        assert_eq!(
            nested.metrics.loc.sloc(),
            3,
            "the function's own three rows, not the file's six"
        );
        assert_eq!(nested.metrics.loc.blank(), 0, "the function has no blanks");
    }

    /// `Sloc::exclude_span` subtracts each pruned subtree's row count from
    /// the enclosing span, so widening that span at the top could in
    /// principle desynchronise the two. It cannot: the rows the anchor
    /// adds are above the first token, and no pruned subtree can overlap
    /// them. Pinned rather than argued, since the failure mode is a
    /// silent `saturating_sub` clamp to 0 rather than a panic (#722,
    /// #1247).
    #[test]
    fn exclude_tests_pruning_composes_with_the_unit_anchor() {
        // Rows 1-3 blank, 4 `fn a`, 5 blank, 6 `#[test]`, 7-9 `fn t`.
        let source = b"\n\n\nfn a() {}\n\n#[test]\nfn t() {\n    assert!(true);\n}\n";
        let kept = metrics_verbatim(
            crate::LANG::Rust,
            source,
            MetricsOptions::default().with_exclude_tests(true),
        )
        .loc;
        // The pruned node is the `fn t` item, rows 7-9; its `#[test]`
        // attribute is a sibling and stays, which is #722's shape and not
        // something the anchor changes. What the anchor decides is the
        // other end: `blank` is 4 rather than 1, because rows 1-3 are now
        // inside the span the pruning subtracts from.
        assert_eq!(
            (kept.sloc(), kept.ploc(), kept.cloc(), kept.blank()),
            (6, 2, 0, 4),
            "the three pruned rows leave; the three leading blanks stay"
        );

        let unpruned = rust_loc(source);
        assert_eq!(
            (unpruned.sloc(), unpruned.ploc(), unpruned.blank()),
            (9, 5, 4),
            "the same file unpruned — the anchor is what makes both blank counts 4"
        );
    }

    /// The non-unit half of the same off-by-one. tree-sitter-perl's
    /// `function_definition` swallows the newline after the closing brace
    /// of a file's **last** `sub`, so that node's span ends at column 0 of
    /// a row it does not occupy. The old unconditional `+ 1` credited that
    /// row, inflating the last sub of every Perl file by one line — here,
    /// reporting a 3-row `sub` as 4.
    #[test]
    fn perl_last_sub_does_not_absorb_the_trailing_newline() {
        // Two identical 3-row subs; only the second hits the quirk.
        let space = space_verbatim(
            crate::LANG::Perl,
            b"sub f {\n    return 1;\n}\nsub g {\n    return 2;\n}\n",
            MetricsOptions::default(),
        );
        assert_eq!(space.metrics.loc.sloc(), 6, "the file has six rows");
        let subs: Vec<u64> = space
            .spaces
            .iter()
            .map(|child| child.metrics.loc.sloc())
            .collect();
        assert_eq!(subs, vec![3, 3], "both subs occupy three rows");
    }

    /// #1135: Tcl and its iRules dialect are the only grammars here that
    /// surface the row terminator as a token child of the root. `LF`'s
    /// start row is the row it *terminates*, so the `_` catch-all in
    /// their `Loc` impls inserted that row into PLOC — turning every
    /// comment-only and whitespace-only row into a line of code.
    ///
    /// A wholly empty row never showed the defect: the `LF` that starts
    /// on it is the one tree-sitter collapses at end-of-input. A row of
    /// *whitespace* does, and trailing whitespace on an otherwise blank
    /// line is ordinary in real source — which is what makes the two
    /// spellings' disagreement the sharpest assertion here.
    ///
    /// Uses [`metrics_verbatim`] so the fixtures reach the parser
    /// byte-for-byte; `check_metrics` rewrites the trailing newline.
    #[test]
    fn tcl_family_does_not_count_terminator_rows_as_code() {
        for lang in [crate::LANG::Tcl, crate::LANG::Irules] {
            // Three rows: code, whitespace-only, code.
            let padded = metrics_verbatim(
                lang,
                b"proc f {} {}\n   \nproc g {} {}\n",
                MetricsOptions::default(),
            )
            .loc;
            assert_eq!(padded.sloc(), 3, "{lang:?} sloc");
            assert_eq!(padded.ploc(), 2, "{lang:?} ploc");
            assert_eq!(padded.blank(), 1, "{lang:?} blank");

            // Whether a blank row carries spaces is not a property of the
            // code, so the empty-row spelling must agree exactly. Pre-fix
            // this side stayed correct while the padded side reported
            // `ploc 3 / blank 0`.
            let empty = metrics_verbatim(
                lang,
                b"proc f {} {}\n\nproc g {} {}\n",
                MetricsOptions::default(),
            )
            .loc;
            assert_eq!(
                (padded.sloc(), padded.ploc(), padded.blank()),
                (empty.sloc(), empty.ploc(), empty.blank()),
                "{lang:?}: trailing whitespace on a blank row must not make it code"
            );

            // The comment-only half of the same defect. It escaped
            // `unterminated_one_line_file_reports_one_source_line`
            // because that sweep's fixtures are one-liners with no
            // comment row. The cross-language version of this case is
            // `a_comment_row_is_never_counted_as_code`; the two rows here
            // stay so the Tcl-family regression reads in one place.
            let commented = metrics_verbatim(
                lang,
                b"# lead-in\nproc f {} {}\n",
                MetricsOptions::default(),
            )
            .loc;
            assert_eq!(commented.sloc(), 2, "{lang:?} commented sloc");
            assert_eq!(commented.ploc(), 1, "{lang:?} commented ploc");
            assert_eq!(commented.cloc(), 1, "{lang:?} commented cloc");
            assert_eq!(commented.blank(), 0, "{lang:?} commented blank");
        }
    }

    /// The comment spellings to sweep for each language: the line form
    /// every language has, then the block and doc forms where one exists.
    ///
    /// Nothing here needs to be exhaustive per language — the defect this
    /// guards against is a stray *token* inside (or terminating) a
    /// comment node reaching a PLOC catch-all, which any one spelling of
    /// a comment exposes. The block and doc entries are there because
    /// those nodes have child tokens the line form does not.
    fn comment_spellings(lang: crate::LANG) -> &'static [&'static str] {
        use crate::LANG::*;
        match lang {
            Python | Ruby | Bash | Elixir | Tcl | Irules | Perl => &["# c"],
            Lua => &["-- c", "--[[ c ]]"],
            Php => &["# c", "/* c */", "/** c */"],
            Rust => &["// c", "/* c */", "/// c"],
            _ => &["// c", "/* c */"],
        }
    }

    /// A comment-only row is never a physical line of code — in any
    /// language, in any comment spelling, on either side of the code.
    ///
    /// Two separate defects broke this, both by letting a token reach the
    /// `_` catch-all that ends `stats.ploc.lines.insert(start)`. In Tcl
    /// and iRules it was the row terminator, whose start row is the row
    /// it terminates (#1135). In Perl it was the `#` *inside* the
    /// `comments` node, which additionally tripped
    /// `check_comment_ends_on_code_line` into reclassifying the row from
    /// comment-only to code-and-comment (#1137).
    ///
    /// Neither was visible to the per-language `*_cloc` tests — several
    /// assert `cloc` and `blank` and leave `ploc` unpinned — nor to
    /// `unterminated_one_line_file_reports_one_source_line`, whose
    /// fixtures carry no comment row at all. Since the failure mode is
    /// structural rather than language-specific, the sweep is per
    /// language rather than a sample.
    #[test]
    fn a_comment_row_is_never_counted_as_code() {
        for (lang, code) in UNTERMINATED_ONE_LINERS {
            // PHP is the one language whose code fixture must open the
            // file: outside `<?php` every row is inline HTML, so a
            // comment placed before it is not a comment at all.
            let must_lead = *lang == crate::LANG::Php;
            for &comment in comment_spellings(*lang) {
                let orders: &[bool] = if must_lead { &[false] } else { &[true, false] };
                for &comment_first in orders {
                    let mut src = Vec::new();
                    let (first, second): (&[u8], &[u8]) = if comment_first {
                        (comment.as_bytes(), code)
                    } else {
                        (code, comment.as_bytes())
                    };
                    src.extend_from_slice(first);
                    src.push(b'\n');
                    src.extend_from_slice(second);
                    src.push(b'\n');

                    let loc = metrics_verbatim(*lang, &src, MetricsOptions::default()).loc;
                    let text = String::from_utf8_lossy(&src);
                    assert_eq!(loc.sloc(), 2, "{lang:?} sloc for {text:?}");
                    assert_eq!(
                        loc.ploc(),
                        1,
                        "{lang:?} ploc for {text:?} — the comment row is not code"
                    );
                    assert_eq!(loc.cloc(), 1, "{lang:?} cloc for {text:?}");
                    assert_eq!(loc.blank(), 0, "{lang:?} blank for {text:?}");
                }
            }
        }
    }

    /// A `var` / `let` / `const` declaration is an executable statement and
    /// counts one LLOC, the same as Java's `LocalVariableDeclaration` and
    /// Rust's `let` (#1283 — before the fix a declarations-only file
    /// reported `lloc 0`). The fourth row is one `variable_declaration`
    /// carrying two declarators, so it counts once, not twice. The fifth row
    /// is a `using_declaration` — the grammar's third executable declaration
    /// kind, which TypeScript and TSX do not have.
    #[test]
    fn javascript_declaration_lloc() {
        check_metrics::<JavascriptParser>(
            "var a = 1;\nlet b = 2;\nconst c = 3;\nvar d = 4, e = 5;\nusing r = open();\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// The classic `for (var i = 0; …)` header is part of the `ForStatement`,
    /// which already counts its own LLOC, so the header declaration must not
    /// add a second one. `for (const x of …)` and `for (var k in …)` need no
    /// carve-out at all: the grammar inlines the `const` / `var` keyword into
    /// `for_in_statement` and emits no declaration node — they are here so a
    /// carve-out wrongly widened to `ForInStatement` still has an input that
    /// notices. `var s = i;` in the loop *body* is a real logical line: the
    /// `StatementBlock` stops the ancestor walk (#1283).
    ///
    /// expected: for-statement 1 + body declaration 1 + for-of 1 + for-in 1 = 4
    #[test]
    fn javascript_for_header_declaration_not_double_counted() {
        check_metrics::<JavascriptParser>(
            "function f(arr, obj) {\n    for (var i = 0; i < 3; i++) {\n        var s = i;\n    }\n    for (const x of arr) {}\n    for (var k in obj) {}\n}\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// `export const a = 1;` is one logical line, not two: the declaration
    /// nests inside the `ExportStatement`, whose arm already counted the row.
    /// The declaration inside the exported function body still counts — the
    /// `StatementBlock` stops the ancestor walk before the `ExportStatement`
    /// is reached (#1283).
    ///
    /// expected: 4 export statements + the `const c = 4;` in `f`'s body = 5
    #[test]
    fn javascript_exported_declaration_counts_once() {
        check_metrics::<JavascriptParser>(
            "export const a = 1;\nexport let b = 2;\nexport default 3;\nexport function f() { const c = 4; }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// A `var` / `let` / `const` declaration is an executable statement and
    /// counts one LLOC, the same as Java's `LocalVariableDeclaration` and
    /// Rust's `let` (#1283 — before the fix a declarations-only file
    /// reported `lloc 0`). The fourth row is one `variable_declaration`
    /// carrying two declarators, so it counts once, not twice. The fifth row
    /// is a `using_declaration` — the grammar's third executable declaration
    /// kind, which TypeScript and TSX do not have.
    #[test]
    fn mozjs_declaration_lloc() {
        check_metrics::<MozjsParser>(
            "var a = 1;\nlet b = 2;\nconst c = 3;\nvar d = 4, e = 5;\nusing r = open();\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 5);
                assert_eq!(metric.loc.ploc(), 5);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// The classic `for (var i = 0; …)` header is part of the `ForStatement`,
    /// which already counts its own LLOC, so the header declaration must not
    /// add a second one. `for (const x of …)` and `for (var k in …)` need no
    /// carve-out at all: the grammar inlines the `const` / `var` keyword into
    /// `for_in_statement` and emits no declaration node — they are here so a
    /// carve-out wrongly widened to `ForInStatement` still has an input that
    /// notices. `var s = i;` in the loop *body* is a real logical line: the
    /// `StatementBlock` stops the ancestor walk (#1283).
    ///
    /// expected: for-statement 1 + body declaration 1 + for-of 1 + for-in 1 = 4
    #[test]
    fn mozjs_for_header_declaration_not_double_counted() {
        check_metrics::<MozjsParser>(
            "function f(arr, obj) {\n    for (var i = 0; i < 3; i++) {\n        var s = i;\n    }\n    for (const x of arr) {}\n    for (var k in obj) {}\n}\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// `export const a = 1;` is one logical line, not two: the declaration
    /// nests inside the `ExportStatement`, whose arm already counted the row.
    /// The declaration inside the exported function body still counts — the
    /// `StatementBlock` stops the ancestor walk before the `ExportStatement`
    /// is reached (#1283).
    ///
    /// expected: 4 export statements + the `const c = 4;` in `f`'s body = 5
    #[test]
    fn mozjs_exported_declaration_counts_once() {
        check_metrics::<MozjsParser>(
            "export const a = 1;\nexport let b = 2;\nexport default 3;\nexport function f() { const c = 4; }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// A `var` / `let` / `const` declaration is an executable statement and
    /// counts one LLOC, the same as Java's `LocalVariableDeclaration` and
    /// Rust's `let` (#1283 — before the fix a declarations-only file
    /// reported `lloc 0`). The fourth row is one `variable_declaration`
    /// carrying two declarators, so it counts once, not twice.
    #[test]
    fn typescript_declaration_lloc() {
        check_metrics::<TypescriptParser>(
            "var a: number = 1;\nlet b = 2;\nconst c = 3;\nvar d = 4, e = 5;\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// The classic `for (var i = 0; …)` header is part of the `ForStatement`,
    /// which already counts its own LLOC, so the header declaration must not
    /// add a second one. `for (const x of …)` and `for (var k in …)` need no
    /// carve-out at all: the grammar inlines the `const` / `var` keyword into
    /// `for_in_statement` and emits no declaration node — they are here so a
    /// carve-out wrongly widened to `ForInStatement` still has an input that
    /// notices. `var s = i;` in the loop *body* is a real logical line: the
    /// `StatementBlock` stops the ancestor walk (#1283).
    ///
    /// expected: for-statement 1 + body declaration 1 + for-of 1 + for-in 1 = 4
    #[test]
    fn typescript_for_header_declaration_not_double_counted() {
        check_metrics::<TypescriptParser>(
            "function f(arr, obj) {\n    for (var i = 0; i < 3; i++) {\n        var s = i;\n    }\n    for (const x of arr) {}\n    for (var k in obj) {}\n}\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// `export const a = 1;` is one logical line, not two: the declaration
    /// nests inside the `ExportStatement`, whose arm already counted the row.
    /// The declaration inside the exported function body still counts — the
    /// `StatementBlock` stops the ancestor walk before the `ExportStatement`
    /// is reached (#1283).
    ///
    /// expected: 4 export statements + the `const c = 4;` in `f`'s body = 5
    ///
    /// The TypeScript spelling also pins `export declare const …`, where an
    /// `ambient_declaration` sits between the export and the declaration: the
    /// carve-out walks the ancestor chain rather than checking the parent, so
    /// it still sees the enclosing `ExportStatement`.
    #[test]
    fn typescript_exported_declaration_counts_once() {
        check_metrics::<TypescriptParser>(
            "export const a: number = 1;\nexport declare const b: string;\nexport default 3;\nexport function f(): void { const c = 4; }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// A `var` / `let` / `const` declaration is an executable statement and
    /// counts one LLOC, the same as Java's `LocalVariableDeclaration` and
    /// Rust's `let` (#1283 — before the fix a declarations-only file
    /// reported `lloc 0`). The fourth row is one `variable_declaration`
    /// carrying two declarators, so it counts once, not twice.
    #[test]
    fn tsx_declaration_lloc() {
        check_metrics::<TsxParser>(
            "var a: number = 1;\nlet b = 2;\nconst c = 3;\nvar d = 4, e = 5;\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// The classic `for (var i = 0; …)` header is part of the `ForStatement`,
    /// which already counts its own LLOC, so the header declaration must not
    /// add a second one. `for (const x of …)` and `for (var k in …)` need no
    /// carve-out at all: the grammar inlines the `const` / `var` keyword into
    /// `for_in_statement` and emits no declaration node — they are here so a
    /// carve-out wrongly widened to `ForInStatement` still has an input that
    /// notices. `var s = i;` in the loop *body* is a real logical line: the
    /// `StatementBlock` stops the ancestor walk (#1283).
    ///
    /// expected: for-statement 1 + body declaration 1 + for-of 1 + for-in 1 = 4
    #[test]
    fn tsx_for_header_declaration_not_double_counted() {
        check_metrics::<TsxParser>(
            "function f(arr, obj) {\n    for (var i = 0; i < 3; i++) {\n        var s = i;\n    }\n    for (const x of arr) {}\n    for (var k in obj) {}\n}\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 7);
                assert_eq!(metric.loc.ploc(), 7);
                assert_eq!(metric.loc.lloc(), 4);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// `export const a = 1;` is one logical line, not two: the declaration
    /// nests inside the `ExportStatement`, whose arm already counted the row.
    /// The declaration inside the exported function body still counts — the
    /// `StatementBlock` stops the ancestor walk before the `ExportStatement`
    /// is reached (#1283).
    ///
    /// expected: 4 export statements + the `const c = 4;` in `f`'s body = 5
    ///
    /// The TypeScript spelling also pins `export declare const …`, where an
    /// `ambient_declaration` sits between the export and the declaration: the
    /// carve-out walks the ancestor chain rather than checking the parent, so
    /// it still sees the enclosing `ExportStatement`.
    #[test]
    fn tsx_exported_declaration_counts_once() {
        check_metrics::<TsxParser>(
            "export const a: number = 1;\nexport declare const b: string;\nexport default 3;\nexport function f(): void { const c = 4; }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.loc.sloc(), 4);
                assert_eq!(metric.loc.ploc(), 4);
                assert_eq!(metric.loc.lloc(), 5);
                assert_eq!(metric.loc.cloc(), 0);
                assert_eq!(metric.loc.blank(), 0);
            },
        );
    }

    /// A brace-less `for` body is a declaration with the *same* parent as
    /// the header, so a carve-out keyed on the enclosing kind alone
    /// dropped it (#1283 review): `for (…) var s = i;` counted one
    /// logical line where the braced spelling and `for (…) x++;` count
    /// two. The header is identified by the `initializer` field instead.
    /// The nested spelling — a `switch` as the brace-less body — is the
    /// same miss one level down.
    ///
    /// expected: for 1 + body declaration 1 = 2; for 1 + switch 1 +
    /// case declaration 1 = 3.
    #[test]
    fn js_family_braceless_for_body_declaration_counts() {
        const BODY: &str = "for (var i = 0; i < 3; i++) var s = i;\n";
        const NESTED: &str = "for (let i = 0; i < 2; i++) switch (i) { case 0: let y = 1; }\n";
        check_metrics::<JavascriptParser>(BODY, "foo.js", |m| assert_eq!(m.loc.lloc(), 2));
        check_metrics::<MozjsParser>(BODY, "foo.js", |m| assert_eq!(m.loc.lloc(), 2));
        check_metrics::<TypescriptParser>(BODY, "foo.ts", |m| assert_eq!(m.loc.lloc(), 2));
        check_metrics::<TsxParser>(BODY, "foo.tsx", |m| assert_eq!(m.loc.lloc(), 2));
        check_metrics::<JavascriptParser>(NESTED, "foo.js", |m| assert_eq!(m.loc.lloc(), 3));
        check_metrics::<MozjsParser>(NESTED, "foo.js", |m| assert_eq!(m.loc.lloc(), 3));
        check_metrics::<TypescriptParser>(NESTED, "foo.ts", |m| assert_eq!(m.loc.lloc(), 3));
        check_metrics::<TsxParser>(NESTED, "foo.tsx", |m| assert_eq!(m.loc.lloc(), 3));
    }

    /// Ambient declarations execute nothing — `declare const x: T;` has
    /// no initializer to run — so they are no logical line, whether the
    /// `declare` is top-level or the declaration sits inside a
    /// `declare namespace` / `declare module` body (#1283 review: only
    /// the `export declare` spelling was carved out, so a `.d.ts` file
    /// reported one LLOC per `declare const`).
    ///
    /// expected: 0 — every row is ambient.
    #[test]
    fn typescript_ambient_declarations_are_not_logical_lines() {
        const SRC: &str = "declare const VERSION: string;\ndeclare let mutable: number;\ndeclare namespace NS { const inner: number; }\ndeclare module \"m\" { let y: string; }\n";
        check_metrics::<TypescriptParser>(SRC, "foo.ts", |m| assert_eq!(m.loc.lloc(), 0));
        check_metrics::<TsxParser>(SRC, "foo.tsx", |m| assert_eq!(m.loc.lloc(), 0));
    }
}
