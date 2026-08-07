//! Set of physical source rows, backed by a word array.
//!
//! `Loc` keeps three of these per space — `Ploc::lines` and `Cloc`'s two
//! comment-line sets — and folds every child space into its parent, so a
//! row inside `D` nested spaces is recorded `D` times on the way up. As
//! a hash set that cost one probe per element per level; here it is a
//! word-wise `|=` over `ceil(rows / 64)` words, and the sets are
//! retained in the returned `FuncSpace` tree at about one bit per
//! spanned row instead of the ~10 bytes per row a `HashSet<usize>`
//! occupies at hashbrown's load factor (#1109).
//!
//! # Why the offset is derived from the data
//!
//! `words[0]` covers rows `first_word * 64 ..`, and the array grows in
//! *either* direction on demand. Anchoring it to the space's declared
//! `[start_row, end_row]` span instead would need one fewer field, and is
//! rejected for two reasons.
//!
//! The span is not known when the rows arrive: a space's `Stats` is
//! default-constructed and `shared::init` records the span from the
//! space's own node, so any anchor would have to be patched in
//! afterwards. And containment is a property of the *callers*, not of
//! this type — `Stats::with_cloc_sloc` already records rows past the
//! span's end on purpose, and a bitset that assumed containment would
//! silently drop an out-of-span row rather than fail. Measured over the
//! `pdf.js`, `DeepSpeech` and `serde` corpora, no parsed space records a
//! row outside its span today; a data-derived offset means that stays a
//! fact about the walk rather than a correctness precondition here.
//!
//! # Density
//!
//! The trade against a hash set is the gap between the lowest and
//! highest row a single space records: the array covers that whole
//! interval whether or not the rows inside it are set. One word per 64
//! rows of *span*, against roughly 10 bytes per *recorded* row for a
//! `HashSet<usize>` at hashbrown's load factor — so the ratio argument
//! ("a win above about one row in 80") holds for `Ploc::lines`, whose
//! rows are the space's own code lines, but not for `Cloc`'s two sets:
//! a 200-line body carrying one comment is one row in 200.
//!
//! What survives regardless of density is the absolute bound — a space
//! costs `span / 8` bytes whatever it records, so the loss is capped by
//! the file's line count rather than unbounded. A comment-sparse,
//! blank-line-heavy file is the honest counterexample: two rows spread
//! over 100 000 cost 12 504 bytes where the hash set costs about 36,
//! and a synthetic 1.9 MB Rust file of two million blank lines measured
//! 12.6 MB peak before #1109 against 13.5 MB after. Real sources are
//! not that shape, and the walk-time win (a word-wise `|=` per merge in
//! place of a probe per row per nesting level) is paid for by every
//! space.

// The enclosing module warns on `arithmetic_side_effects` because its
// arithmetic is on tree-sitter span coordinates (#1051, #1152). This
// file is the deliberate carve-out: its arithmetic is on *word indices
// into `words`*, and every site is bounds-established at the point of
// use rather than by a property of the input — `reserve` runs before
// each subtraction in `insert`/`insert_range`/`union_with`, `slot` and
// `word` already use `checked_sub`, and `insert_range` returns early on
// an inverted span.
//
// Making these saturating would be actively worse than leaving them
// checked. `self.words[word - self.first_word]` saturating to index 0
// reads or writes *the wrong row's word* and silently miscounts the
// metric; the current form panics on a corrupt offset, which is how
// `intersection_len`'s comment says it is meant to fail. Prevention and
// masking point in opposite directions here, so the lint is off rather
// than satisfied.
#![allow(clippy::arithmetic_side_effects)]

use std::fmt;

/// Bits per element of [`LineSet::words`].
const BITS_PER_WORD: usize = u64::BITS as usize;

/// Index of the word holding `row`.
#[inline]
const fn word_of(row: usize) -> usize {
    row / BITS_PER_WORD
}

/// Mask selecting `row` within its word.
#[inline]
const fn bit_of(row: usize) -> u64 {
    1 << (row % BITS_PER_WORD)
}

/// A set of 0-based physical source row numbers.
///
/// Cardinality is a popcount sum rather than a stored counter, so no
/// invariant has to be maintained across `insert` / `remove` /
/// [`union_with`](LineSet::union_with).
#[derive(Clone, Default)]
pub(super) struct LineSet {
    /// Index of the word `words[0]` holds. Meaningless while `words` is
    /// empty, and reset by the first insertion.
    first_word: usize,
    /// Bit `b` of `words[i]` is row `(first_word + i) * BITS_PER_WORD + b`.
    words: Vec<u64>,
}

impl LineSet {
    /// Adds `row`.
    #[inline]
    pub(super) fn insert(&mut self, row: usize) {
        let word = word_of(row);
        self.reserve(word);
        self.words[word - self.first_word] |= bit_of(row);
    }

    /// Adds every row in the inclusive range `start..=end`.
    ///
    /// A no-op when `end < start`. No caller passes an inverted span
    /// today, and `add_cloc_lines` debug-asserts that it does not — but
    /// that assertion is compiled out in release, which is exactly where
    /// the guard earns its keep: without it an inverted span reaches the
    /// slice fill below with `last < first` and panics mid-walk.
    /// Returning is a better way to survive a span adjustment that
    /// started underflowing (such as the Rust doc-comment one in #1051)
    /// than an index panic in a release build.
    pub(super) fn insert_range(&mut self, start: usize, end: usize) {
        if end < start {
            return;
        }
        // Reserve both ends before taking either slot index: the first
        // call can prepend words and shift `first_word`, which would
        // invalidate an index captured beforehand.
        self.reserve(word_of(start));
        self.reserve(word_of(end));
        let first = word_of(start) - self.first_word;
        let last = word_of(end) - self.first_word;

        // Bits at or above `start`'s position, and bits at or below
        // `end`'s. Both shift amounts are in `0..BITS_PER_WORD`.
        let from_start = u64::MAX << (start % BITS_PER_WORD);
        let through_end = u64::MAX >> (BITS_PER_WORD - 1 - end % BITS_PER_WORD);

        if first == last {
            self.words[first] |= from_start & through_end;
        } else {
            self.words[first] |= from_start;
            self.words[first + 1..last].fill(u64::MAX);
            self.words[last] |= through_end;
        }
    }

    /// Removes `row`. A no-op when it is absent.
    #[inline]
    pub(super) fn remove(&mut self, row: usize) {
        if let Some(slot) = self.slot(row) {
            self.words[slot] &= !bit_of(row);
        }
    }

    /// Whether `row` is present.
    #[inline]
    pub(super) fn contains(&self, row: usize) -> bool {
        self.slot(row)
            .is_some_and(|slot| self.words[slot] & bit_of(row) != 0)
    }

    /// Number of rows in the set.
    ///
    /// O(words), where `HashSet::len` was O(1) — this popcounts the
    /// whole array. That is a new cliff: a caller asking for it once
    /// per *node* would be quadratic in the span. The walk asks once
    /// per space, from `compute_minmax` and the `cloc` / `blank`
    /// readers.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Number of rows in `self ∪ other`, without materialising the union.
    ///
    /// Inclusion-exclusion over the two cardinalities and the
    /// intersection, so only the words the two sets share are visited
    /// twice.
    pub(super) fn union_len(&self, other: &Self) -> usize {
        self.len() + other.len() - self.intersection_len(other)
    }

    /// Adds every row of `other` to `self`.
    pub(super) fn union_with(&mut self, other: &Self) {
        let Some(last_word) = other.last_word() else {
            return;
        };
        self.reserve(other.first_word);
        self.reserve(last_word);
        // Both reserves have run, so `self` now spans at least
        // `other.first_word..=last_word` and the subtraction cannot
        // underflow.
        let base = other.first_word - self.first_word;
        for (dst, src) in self.words[base..].iter_mut().zip(&other.words) {
            *dst |= src;
        }
    }

    /// The rows in the set, ascending.
    ///
    /// Scans a full word per populated word, so it is for [`fmt::Debug`]
    /// and tests rather than for the metric walk.
    fn rows(&self) -> impl Iterator<Item = usize> {
        self.words.iter().enumerate().flat_map(|(index, &word)| {
            let base = (self.first_word + index) * BITS_PER_WORD;
            (0..BITS_PER_WORD)
                .filter(move |bit| word >> bit & 1 == 1)
                .map(move |bit| base + bit)
        })
    }

    /// Index of the last word held, or `None` when the set has never
    /// been written to.
    #[inline]
    fn last_word(&self) -> Option<usize> {
        (!self.words.is_empty()).then(|| self.first_word + self.words.len() - 1)
    }

    /// Position of `row`'s word within [`LineSet::words`], if covered.
    #[inline]
    fn slot(&self, row: usize) -> Option<usize> {
        word_of(row)
            .checked_sub(self.first_word)
            .filter(|slot| *slot < self.words.len())
    }

    /// The word at absolute index `word`, or zero when uncovered.
    #[inline]
    fn word(&self, word: usize) -> u64 {
        word.checked_sub(self.first_word)
            .and_then(|slot| self.words.get(slot))
            .copied()
            .unwrap_or(0)
    }

    /// Number of rows in `self ∩ other`.
    fn intersection_len(&self, other: &Self) -> usize {
        let start = self.first_word.max(other.first_word);
        let end = (self.first_word + self.words.len()).min(other.first_word + other.words.len());
        // Empty when the two spans are disjoint: `start > end` yields an
        // empty range rather than panicking. Inside it both indices are
        // in bounds by construction, so a bound that drifted would panic
        // here rather than silently miscount.
        (start..end)
            .map(|word| {
                (self.words[word - self.first_word] & other.words[word - other.first_word])
                    .count_ones() as usize
            })
            .sum()
    }

    /// Grows the array so absolute index `word` is covered, in whichever
    /// direction is needed.
    ///
    /// The two directions do not cost the same. Upward is a
    /// `Vec::resize`, amortised O(1); downward is a `splice(0..0, …)`
    /// that shifts every existing word, O(len) per call — so a strictly
    /// descending insertion order would be quadratic in words. Not
    /// reachable today: the walk visits rows roughly ascending, and
    /// merges fold child into parent rather than the reverse. It is the
    /// shape to check first if a caller ever starts feeding rows
    /// backwards.
    fn reserve(&mut self, word: usize) {
        if self.words.is_empty() {
            self.first_word = word;
            self.words.push(0);
        } else if word < self.first_word {
            let below = self.first_word - word;
            self.words.splice(0..0, std::iter::repeat_n(0, below));
            self.first_word = word;
        } else if word - self.first_word >= self.words.len() {
            self.words.resize(word - self.first_word + 1, 0);
        }
    }
}

/// Equality is over the rows, not the representation: two sets holding
/// the same rows compare equal whatever offsets and zero padding they
/// reached them through. `Loc`'s `Stats` derives `PartialEq`, so a
/// representational comparison would make a metric equality assertion
/// depend on insertion order.
impl PartialEq for LineSet {
    fn eq(&self, other: &Self) -> bool {
        let start = self.first_word.min(other.first_word);
        let end = (self.first_word + self.words.len()).max(other.first_word + other.words.len());
        (start..end).all(|word| self.word(word) == other.word(word))
    }
}

impl fmt::Debug for LineSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.rows()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{BITS_PER_WORD, LineSet};

    /// Builds a set from an explicit row list, so a test's fixture is
    /// visible at its use site.
    fn set_of(rows: &[usize]) -> LineSet {
        let mut set = LineSet::default();
        for row in rows {
            set.insert(*row);
        }
        set
    }

    fn rows_of(set: &LineSet) -> Vec<usize> {
        set.rows().collect()
    }

    /// A never-written set is empty and allocates nothing, which is the
    /// state most spaces stay in for `Cloc`'s two sets.
    #[test]
    fn default_is_empty_and_unallocated() {
        let set = LineSet::default();
        assert_eq!(set.len(), 0);
        assert!(!set.contains(0));
        assert_eq!(set.words.capacity(), 0, "an unused set must not allocate");
    }

    /// Insertion is de-duplicating and order-independent, the property
    /// `Ploc` relies on to count a physical line once however many nodes
    /// start on it.
    #[test]
    fn insert_is_idempotent_and_order_independent() {
        let ascending = set_of(&[3, 64, 65, 4_000]);
        let descending = set_of(&[4_000, 65, 64, 3]);
        let repeated = set_of(&[64, 3, 64, 4_000, 65, 3]);

        assert_eq!(rows_of(&ascending), vec![3, 64, 65, 4_000]);
        assert_eq!(ascending.len(), 4);
        assert_eq!(ascending, descending);
        assert_eq!(ascending, repeated);
    }

    /// Inserting below the current offset must prepend words rather than
    /// wrap the subtraction or drop the row. Rows arrive in walk order,
    /// which is mostly ascending but not guaranteed to be.
    #[test]
    fn insert_below_offset_grows_downward() {
        let mut set = set_of(&[300]);
        set.insert(1);
        set.insert(299);

        assert_eq!(rows_of(&set), vec![1, 299, 300]);
        assert!(set.contains(1));
        assert!(!set.contains(0));
        assert!(!set.contains(2));
    }

    /// `insert_range` must set exactly the inclusive range, including
    /// when it starts and ends mid-word, spans whole interior words, or
    /// sits inside one word. These are the block-comment shapes
    /// `add_only_comment_lines` feeds it.
    #[test]
    fn insert_range_covers_exactly_the_inclusive_span() {
        let mut single = LineSet::default();
        single.insert_range(7, 7);
        assert_eq!(rows_of(&single), vec![7]);

        let mut within_word = LineSet::default();
        within_word.insert_range(5, 9);
        assert_eq!(rows_of(&within_word), vec![5, 6, 7, 8, 9]);

        let mut across_words = LineSet::default();
        across_words.insert_range(63, 129);
        assert_eq!(across_words.len(), 67);
        assert!(!across_words.contains(62));
        assert!(across_words.contains(63));
        assert!(across_words.contains(64));
        assert!(across_words.contains(129));
        assert!(!across_words.contains(130));

        // Whole interior words: rows 64..=191 is exactly words 1..=2.
        let mut whole_words = LineSet::default();
        whole_words.insert_range(64, 191);
        assert_eq!(whole_words.len(), 128);
        assert!(!whole_words.contains(63));
        assert!(!whole_words.contains(192));
    }

    /// An inverted span is a no-op, not a wrap into a full fill.
    ///
    /// The cross-word case is the one that pins the guard. Inverted
    /// *within* a word the mask arithmetic already yields zero, so
    /// deleting `if end < start` leaves that case passing; across a word
    /// boundary `first` exceeds `last` and the interior fill indexes
    /// `words[2..0]`, which panics.
    #[test]
    fn insert_range_ignores_an_inverted_span() {
        let mut within_word = set_of(&[10]);
        within_word.insert_range(9, 8);
        assert_eq!(rows_of(&within_word), vec![10]);

        let mut across_words = set_of(&[10]);
        across_words.insert_range(BITS_PER_WORD, BITS_PER_WORD - 1);
        assert_eq!(rows_of(&across_words), vec![10]);
    }

    /// `insert_range` must grow a set that has already been written to,
    /// in either direction. Every other range case starts from a default
    /// set, whose first `reserve` simply anchors the offset, so neither
    /// growth branch is reached from `insert_range` at all.
    ///
    /// The first range starts below the current offset and ends inside
    /// it: that `reserve` prepends and shifts `first_word`, which is
    /// what the "reserve both ends before taking either slot index"
    /// comment defends — an index captured beforehand would underflow
    /// (`word_of(70) - 3`). The second range sits past the last word
    /// held, taking the upward branch.
    #[test]
    fn insert_range_grows_a_seeded_set_in_both_directions() {
        // Both seeds live in word 3 (rows 192..=255).
        let mut set = set_of(&[200, 205]);

        set.insert_range(70, 202);
        set.insert_range(300, 301);

        assert!(set.contains(70), "the prepended low end must be set");
        assert!(set.contains(202));
        assert!(set.contains(205), "the seeded rows must survive the growth");
        assert!(set.contains(300));
        assert!(!set.contains(69));
        assert!(!set.contains(203));
        assert!(!set.contains(204));
        assert!(!set.contains(299));
        assert!(!set.contains(302));
        // 70..=202 is 133 rows, plus the seed at 205 and the pair at
        // 300..=301; the seed at 200 falls inside the first range.
        assert_eq!(set.len(), 136);
    }

    /// `check_comment_ends_on_code_line` reclassifies a row by removing
    /// it from the comment-only set; removal must clear only that row and
    /// must tolerate a row the set never covered.
    #[test]
    fn remove_clears_one_row_and_tolerates_absent_rows() {
        let mut set = set_of(&[64, 65, 66]);
        set.remove(65);
        assert_eq!(rows_of(&set), vec![64, 66]);

        // Below the offset, above the last word, and inside it but unset.
        set.remove(0);
        set.remove(10_000);
        set.remove(65);
        assert_eq!(rows_of(&set), vec![64, 66]);
        assert_eq!(set.len(), 2);
    }

    /// Merging a child space into its parent is a union: rows the two
    /// share count once, and the parent keeps rows the child does not
    /// have. Every row distinguishes a case — 5 is parent-only, 6 is
    /// child-only *in a word the parent has already written* (so an
    /// assignment in place of the `|=` drops row 5), 70 is shared, 130 is
    /// child-only past the parent's last word. A fixture without an
    /// overlap would pass equally against a sum.
    #[test]
    fn union_with_deduplicates_shared_rows() {
        let mut parent = set_of(&[5, 70]);
        let child = set_of(&[6, 70, 130]);

        parent.union_with(&child);

        assert_eq!(rows_of(&parent), vec![5, 6, 70, 130]);
        assert_eq!(
            parent.len(),
            4,
            "the shared row 70 must not be counted twice"
        );
    }

    /// A child whose rows sit entirely below or entirely above the
    /// parent's must extend the parent in that direction. The downward
    /// case is what a naive `words[i] |= other.words[i]` would corrupt.
    #[test]
    fn union_with_extends_in_both_directions() {
        let mut below = set_of(&[500]);
        below.union_with(&set_of(&[1]));
        assert_eq!(rows_of(&below), vec![1, 500]);

        let mut above = set_of(&[1]);
        above.union_with(&set_of(&[500]));
        assert_eq!(rows_of(&above), vec![1, 500]);
    }

    /// An empty child leaves the parent untouched, and merging into an
    /// empty parent adopts the child wholesale. Both happen constantly:
    /// most spaces have no comments at all.
    #[test]
    fn union_with_handles_an_empty_side() {
        let mut parent = set_of(&[9, 200]);
        parent.union_with(&LineSet::default());
        assert_eq!(rows_of(&parent), vec![9, 200]);

        let mut empty = LineSet::default();
        empty.union_with(&set_of(&[9, 200]));
        assert_eq!(rows_of(&empty), vec![9, 200]);
    }

    /// `Cloc::cloc()` counts the union of the comment-only and
    /// code-comment sets. Seeded with an overlap, a parent-only row and a
    /// child-only row so the inclusion-exclusion terms are all non-zero.
    #[test]
    fn union_len_counts_each_row_once() {
        let only_comments = set_of(&[2, 64, 300]);
        let code_comments = set_of(&[64, 301]);

        assert_eq!(only_comments.union_len(&code_comments), 4);
        assert_eq!(
            only_comments.union_len(&code_comments),
            code_comments.union_len(&only_comments),
            "union cardinality is symmetric"
        );
        // Disjoint word spans exercise the empty-intersection path.
        assert_eq!(set_of(&[1]).union_len(&set_of(&[1_000])), 2);
        assert_eq!(only_comments.union_len(&LineSet::default()), 3);
    }

    /// Equality is over rows, so two sets that reached the same rows
    /// through different offsets and different amounts of zero padding
    /// compare equal. `Loc::Stats` derives `PartialEq`.
    #[test]
    fn equality_ignores_offset_and_padding() {
        let direct = set_of(&[500]);

        let mut padded = set_of(&[1, 500]);
        padded.remove(1);

        assert_eq!(direct, padded);
        assert_eq!(padded, direct);
        assert_eq!(LineSet::default(), padded_but_cleared());
        assert_ne!(direct, set_of(&[499]));
    }

    /// A set whose only row was removed: non-empty backing array, no rows.
    fn padded_but_cleared() -> LineSet {
        let mut set = set_of(&[4_096]);
        set.remove(4_096);
        set
    }

    /// The representation is the point of #1109: a dense run of rows
    /// costs one word per 64 rows, not one entry per row. A revert to a
    /// hash set cannot satisfy this.
    #[test]
    fn dense_rows_cost_one_word_per_sixty_four() {
        let mut set = LineSet::default();
        set.insert_range(0, 10_239);

        assert_eq!(set.len(), 10_240);
        assert_eq!(set.words.len(), 10_240 / BITS_PER_WORD);
        assert_eq!(set.words.len(), 160);
    }

    /// The largest span a real file reaches, merged the way the space
    /// stack merges it, still counts every row exactly once.
    #[test]
    fn large_file_span_merges_without_loss() {
        const ROWS: usize = 100_000;

        let mut unit = LineSet::default();
        // Ten "child spaces" of ten thousand rows each, adjacent so each
        // shares its boundary row with its neighbour.
        for chunk in 0..10 {
            let start = chunk * (ROWS / 10);
            let mut child = LineSet::default();
            child.insert_range(start, start + ROWS / 10);
            unit.union_with(&child);
        }

        // 100_001 rows: the ten chunks cover 0..=100_000, and the nine
        // shared boundary rows are counted once.
        assert_eq!(unit.len(), ROWS + 1);
        assert!(unit.contains(0));
        assert!(unit.contains(ROWS));
        assert!(!unit.contains(ROWS + 1));
    }

    /// `LineSet` is a bitset, so its derived-looking `Debug` is
    /// hand-written to print rows rather than words. Sparse, non-adjacent
    /// rows spanning more than one word make a word-order or
    /// offset mistake visible; a set of `{0, 1}` would not.
    #[test]
    fn debug_renders_rows_not_words() {
        let rendered = format!("{:?}", set_of(&[3, BITS_PER_WORD + 1, 200]));
        assert_eq!(rendered, format!("{{3, {}, 200}}", BITS_PER_WORD + 1));
        assert_eq!(format!("{:?}", LineSet::default()), "{}");
    }
}
