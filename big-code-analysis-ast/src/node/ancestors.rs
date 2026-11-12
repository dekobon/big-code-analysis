//! [`Ancestors`], the chain of a node's ancestors a walker threads down
//! to its predicates, and the [`SiblingCache`] and [`FoldCache`] it can
//! lend with it.
//!
//! The chain, its constructors and its plain reads (`parent`, `iter`,
//! `of_children`) are here, with the tests of the whole module. Three
//! submodules hold the rest: `chain`, the two runs a chain reads as one;
//! `siblings`, the sibling lookups and the record a walk keeps for them;
//! `folds`, the folds and memos and their cache.

use super::Node;

mod chain;
mod folds;
mod siblings;

use chain::{Chain, HeldChain};
pub use folds::{
    FoldCache, GateLane, KindLane, LevelQuestion, NodeQuestion, fold_slot_probes, fold_steps,
};
pub(crate) use folds::{FoldLane, MemoQuestion, NodeMemo, WordSlot};
use siblings::NOTHING_RECORDED;
pub use siblings::{SiblingCache, WIDE_PARENT, sibling_cache_reads, sibling_seeks};

/// The chain of a node's children, held by a consumer that reads them
/// without descending: what [`Ancestors::of_children`] builds. A
/// consumer that does descend, into wrappers whose statements it reads
/// as the body's own, moves it with [`descend`](Self::descend) and
/// [`ascend`](Self::ascend) rather than building one per level.
///
/// It borrows the walk's chain and owns only the entries below it, so
/// it holds the walk's `Vec` immutably for as long as it lives (#1805).
#[derive(Debug)]
pub struct ChildAncestors<'tree, 'chain> {
    /// `None` without a known chain, where every answer climbs.
    chain: Option<HeldChain<'tree, 'chain>>,
    folds: Option<&'chain FoldCache>,
}

impl<'tree> ChildAncestors<'tree, '_> {
    /// The children's [`Ancestors`], with the walk's fold cache carried
    /// — and with it the visited node its word slots serve — and its
    /// sibling cache deliberately not: the children are nodes the walk
    /// has not recorded. Neither does an [`iter`](Ancestors::iter) from
    /// it regain the sibling cache once it climbs back into the walk's
    /// own chain. Forwarding the record for that climb was measured and
    /// left out: no walk asks for a sibling there, in the suite or the
    /// repository corpora (#1871).
    #[inline]
    #[must_use]
    pub fn get(&self) -> Ancestors<'tree, '_> {
        match &self.chain {
            Some(chain) => Ancestors {
                chain: Some(chain.view()),
                siblings: None,
                folds: self.folds,
            },
            None => Ancestors::unknown(),
        }
    }

    /// Descends into `child`, one of the nodes this chain describes, so
    /// that [`get`](Self::get) answers for *its* children; undone by
    /// [`ascend`](Self::ascend). A consumer that reads a body's
    /// statements through the wrappers they sit in — Ruby's `npm` under
    /// an `if`, a `begin`, a call's argument list — keeps one chain and
    /// moves it, where deriving a fresh [`Ancestors::of_children`] from
    /// the last at each level copies every level derived so far, all of
    /// them live at once (#1401 review). Without a known chain there is
    /// nothing to move and the answers keep climbing.
    #[inline]
    pub fn descend(&mut self, child: &Node<'tree>) {
        if let Some(chain) = &mut self.chain {
            chain.descend(child);
        }
    }

    /// Undoes one [`descend`](Self::descend). The chain never shrinks
    /// below the node it was built for, so a consumer that ascends once
    /// too often reads the body's own chain rather than its parent's —
    /// and trips the assertion in a debug build.
    #[inline]
    pub fn ascend(&mut self) {
        if let Some(chain) = &mut self.chain {
            chain.ascend();
        }
    }
}

/// The chain of a node's ancestors, root first, as recorded by a walker
/// that descended to that node.
///
/// A predicate that asks a node for its parent pays [`Node::parent`]'s
/// `O(depth)` once per node, so `O(depth²)` over a deeply nested file,
/// however few parent steps it takes (#1084). A walker that visits
/// parents before children already holds the chain, and handing it down
/// turns each step into a slice index — the upward counterpart of the
/// downward flag propagation #1052 and #1062 used.
///
/// Callers that reached a node some other way pass
/// [`Ancestors::unknown`], which climbs with [`Node::parent`]: the same
/// answers at the original cost.
///
/// A chain derived below the walk's ([`of_children`](Self::of_children))
/// reads the walk's chain in place and the entries it adds after it, as
/// one chain at the same depths (#1805).
///
/// A walk's own chain states whether the walk records siblings: it is
/// built with [`for_walk`](Self::for_walk), which requires the walk's
/// [`SiblingCache`], or with
/// [`for_walk_without_siblings`](Self::for_walk_without_siblings) or,
/// inside this crate, `for_walk_chain_only`, which say that it records
/// none. A walk can no longer leave the cache out by forgetting a
/// builder call (#1601).
#[derive(Clone, Copy, Debug)]
pub struct Ancestors<'tree, 'chain> {
    chain: Option<Chain<'tree, 'chain>>,
    /// Where a sibling lookup reads from. The constructor decides, and
    /// [`iter`](Self::iter) and a fold step's `outer` chain keep what
    /// they were derived from:
    ///
    /// - [`for_walk`](Self::for_walk): the walk's record of its wide
    ///   parents. A lookup under one is a binary search, and a wide
    ///   parent the record lacks trips the assertion in
    ///   [`lookup`](Self::lookup).
    /// - [`for_walk_without_siblings`](Self::for_walk_without_siblings)
    ///   and [`for_walk_chain_only`](Self::for_walk_chain_only):
    ///   [`NOTHING_RECORDED`], the record of a walk that records none,
    ///   so every wide parent trips that assertion.
    /// - [`known`](Self::known), [`below`](Self::below) and
    ///   [`of_children`](Self::of_children): `None`, a chain that is
    ///   not a walk's own. `siblings_before` and `siblings_after` seek a
    ///   wide parent through it, and `previous_sibling` has its own
    ///   assertion there (#1785). A chain iterated from a derived one
    ///   stays `None` even where it climbs back into the walk's chain.
    /// - [`unknown`](Self::unknown): `None` and no chain, so every
    ///   lookup resolves the parent from the node and asserts nothing.
    ///
    /// Under a parent of at most [`WIDE_PARENT`] children every chain
    /// seeks. `Some` therefore does not mean that the walk records
    /// siblings.
    siblings: Option<&'chain SiblingCache<'tree>>,
    folds: Option<&'chain FoldCache>,
}

impl<'tree, 'chain> Ancestors<'tree, 'chain> {
    /// No chain is available; every query climbs with [`Node::parent`].
    #[inline]
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            chain: None,
            siblings: None,
            folds: None,
        }
    }

    /// `chain` lists every ancestor of the node about to be queried,
    /// root first — so `chain.last()` is its parent and an empty chain
    /// means the node is the root.
    ///
    /// Not for the chain a walk keeps: a walk constructs through
    /// [`for_walk`](Self::for_walk) or
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings)
    /// (or, inside this crate, `for_walk_chain_only`), which check the
    /// chain against the node and state whether the walk records
    /// siblings. A chain built here seeks for a sibling under a wide
    /// parent with no such check (#1601).
    #[inline]
    #[must_use]
    pub const fn known(chain: &'chain [Node<'tree>]) -> Self {
        Self {
            chain: Some(Chain::flat(chain)),
            siblings: None,
            folds: None,
        }
    }

    /// This chain, answering [`siblings_after`](Self::siblings_after) and
    /// [`siblings_before`](Self::siblings_before) through `cache`.
    /// Test-only, for a test that drives a lookup directly: a walk lends
    /// its cache through [`for_walk`](Self::for_walk), which requires it,
    /// so no walk can leave the cache out by omission (#1601).
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn with_siblings(self, cache: &'chain SiblingCache<'tree>) -> Self {
        Self {
            siblings: Some(cache),
            ..self
        }
    }

    /// This chain, memoising `Ancestors::fold_down` and the per-node
    /// memos through `cache`, without recording a visit: the word slots
    /// serve only the node the cache was told is being visited, and
    /// nothing tells it here. Test-only, for a test that drives a fold
    /// or a memo directly: a walk lends its cache through
    /// [`for_walk`](Self::for_walk) or
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings),
    /// the only non-test constructors that set `folds`, and both record
    /// the visit, so no lender can lend the cache and skip it (#1737).
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn with_folds(self, cache: &'chain FoldCache) -> Self {
        Self {
            folds: Some(cache),
            ..self
        }
    }

    /// This chain with the walk's [`FoldCache`] withheld and its
    /// [`SiblingCache`] kept. Test-only, for a test that asks one
    /// question twice with different inputs: a memo is keyed by node,
    /// so through the cache the second ask would be served the first
    /// one's answer.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn without_folds(self) -> Self {
        Self {
            folds: None,
            ..self
        }
    }

    /// The chain a walk hands its predicates for `node`, the node it is
    /// visiting: `chain`, checked against `node` as
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings)
    /// checks it, lending `folds` with the visit recorded on it (#1737)
    /// and answering sibling lookups from `siblings`, the children the
    /// walk recorded of every wide node on `chain` (#1584). Lending the
    /// caches and naming the
    /// visited node are one step that no walk can split: outside the
    /// walk constructors, only the test-only `with_folds` and
    /// `with_siblings` set either cache (#1601). A walk that records no
    /// siblings says so with
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings).
    ///
    /// The visit is what `Ancestors::classified` keys the word slots
    /// on: an ask about `node` is served and stored, an ask about any
    /// other node — an enclosing call read from a descendant, a module's
    /// statements read while the module is visited — is computed and
    /// not stored, so it cannot evict the visited node's answer. A
    /// derived chain ([`of_children`](Self::of_children), `below`,
    /// [`iter`](Self::iter), the `outer` a fold step is handed) carries
    /// the same cache and so the same visit.
    #[inline]
    #[must_use]
    pub fn for_walk(
        chain: &'chain [Node<'tree>],
        node: &Node<'tree>,
        folds: &'chain FoldCache,
        siblings: &'chain SiblingCache<'tree>,
    ) -> Self {
        Self {
            siblings: Some(siblings),
            ..Self::for_walk_without_siblings(chain, node, folds)
        }
    }

    /// [`for_walk`](Self::for_walk) for a walk that records no siblings:
    /// the suppression scan and the dump. No predicate those walks call
    /// asks for a sibling under any parent, which
    /// `the_walks_that_record_no_siblings_ask_for_none` pins in the root
    /// crate, since a predicate cannot know that its parent is narrow.
    /// This chain enforces the costly half: a sibling asked under a
    /// parent wider than [`WIDE_PARENT`] trips the assertion in `lookup`
    /// in a debug build, because the seek is `O(index)` and a walk that
    /// asks it of every child of a flat `ERROR` is quadratic (#1584).
    /// When a predicate of such a walk needs a sibling, the walk records
    /// each wide node's children as it pushes them and constructs
    /// through [`for_walk`](Self::for_walk) instead (#1601).
    ///
    /// # Panics
    ///
    /// When `chain` is not `node`'s ancestry, as a walk's chain is after
    /// a slip in its truncate/push bookkeeping. A debug build checks an
    /// `O(1)` consequence: `chain.last()` is not `node` itself, and its
    /// byte span contains `node`'s. That catches a `push` moved ahead of
    /// the per-node work and a dropped `truncate`, and misses a chain
    /// short by exactly one entry. Under `--cfg chain_audit`, which
    /// `make chain-audit` sets, every profile asserts the exact
    /// `chain.last() == node.parent()` instead, at [`Node::parent`]'s
    /// `O(depth)` per node (#1122). [`for_walk`](Self::for_walk) builds
    /// on this constructor and panics the same way.
    #[inline]
    #[must_use]
    pub fn for_walk_without_siblings(
        chain: &'chain [Node<'tree>],
        node: &Node<'tree>,
        folds: &'chain FoldCache,
    ) -> Self {
        folds.record_visit(node);
        Self {
            folds: Some(folds),
            ..Self::for_walk_chain_only(chain, node)
        }
    }

    /// The chain of a walk that lends no cache, the comment-removal
    /// walk: [`Ancestors::known`] for the chain a walk keeps. It first
    /// checks that `chain` really is `node`'s ancestry, and it lends the
    /// empty sibling record of a walk that records none. Every
    /// `for_walk` constructor makes the check, and
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings)
    /// lends the same record; this one adds nothing to them, which is
    /// its name.
    ///
    /// The parity test (`a_known_chain_answers_exactly_what_climbing_answers`
    /// in `node.rs`) proves the walker's truncate/push rule on a
    /// *replica* walker, so it cannot see `metrics_inner` itself
    /// desynchronising — a `chain.push` moved ahead of the per-node
    /// computes, say, or a `continue` inserted above the truncate.
    /// [`Ancestors::parent`] trusts `chain.last()` unvalidated, so such a
    /// drift would feed every predicate a wrong ancestor silently rather
    /// than fail. Walkers that maintain a chain should construct through
    /// here — through [`for_walk`](Self::for_walk) or
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings),
    /// which build on it, when they lend a [`FoldCache`];
    /// [`Ancestors::known`] stays unchecked for the callers that
    /// deliberately pair a chain with a foreign node.
    ///
    /// A checked chain is a walk's own, so a sibling asked through it
    /// under a parent wider than [`WIDE_PARENT`] trips the assertion in
    /// `lookup` (#1601).
    ///
    /// Crate-private: a walk outside this crate lends a [`FoldCache`]
    /// and says so with [`for_walk`](Self::for_walk) or
    /// [`for_walk_without_siblings`](Self::for_walk_without_siblings).
    /// It was the public `checked` until #1871, a name that said only
    /// the first of the two things it does.
    ///
    /// # Two checks, because the exact one is not affordable by default
    ///
    /// The invariant is "`chain.last()` **is** `node.parent()`", and
    /// asking that outright costs [`Node::parent`]'s `O(depth)` — the
    /// very lookup #1084 exists to remove. Per node, on every walk that
    /// constructs a checked chain, it made every debug-build walk
    /// `O(nodes × depth)` while the shipped walk is `O(nodes)`: a tax on
    /// every `cargo test`, worst on the deep-nesting regression tests
    /// that exist to pin the shipped walk's linearity (#1122). It now
    /// runs only under `--cfg chain_audit`, which `make chain-audit` and
    /// the CI lane of the same name set — and there as a plain
    /// `assert_eq!`, so the audit has teeth in a release profile too.
    ///
    /// What stays on in every debug build is an `O(1)` *consequence* of
    /// that invariant: a parent's byte span contains its child's, and no
    /// node is its own parent. Strictly weaker — a grandparent contains
    /// the node as well — but it is shaped to the two ways a walker
    /// desynchronises. A `push` moved ahead of the per-node computes
    /// leaves `chain.last() == node`; a dropped `truncate` leaves the
    /// previous subtree's path, whose last entry is disjoint from the
    /// node that follows it. Both trip here, on the first node that
    /// shows them, at four integer comparisons.
    #[inline]
    #[must_use]
    pub(crate) fn for_walk_chain_only(chain: &'chain [Node<'tree>], node: &Node<'tree>) -> Self {
        // Opt-in only: `Node::parent` restarts at the root, so this is
        // the `O(nodes × depth)` walk described above.
        #[cfg(chain_audit)]
        assert_eq!(
            chain.last().map(Node::id),
            node.parent().map(|parent| parent.id()),
            "ancestor chain desynchronised on a {} node",
            node.kind()
        );
        debug_assert!(
            chain.last().is_none_or(|parent| {
                parent.id() != node.id()
                    && parent.start_byte() <= node.start_byte()
                    && node.end_byte() <= parent.end_byte()
            }),
            "ancestor chain desynchronised on a {} node: chain.last() neither \
             contains it nor differs from it",
            node.kind()
        );
        Self {
            siblings: Some(NOTHING_RECORDED),
            ..Self::known(chain)
        }
    }

    /// Whether the chain is known — a walk handed it in — rather than
    /// the climbing [`Ancestors::unknown`]. A caller about to replace the
    /// chain with [`below`](Self::below) checks it first: the climb is
    /// already exact, where a replacing chain is exact only for what it
    /// holds. [`of_children`](Self::of_children) needs no check, since it
    /// derives an unknown chain from an unknown one.
    pub(crate) const fn is_known(self) -> bool {
        self.chain.is_some()
    }

    /// The chain of the ancestor at `depth` on this one: this chain cut
    /// above it, with the same caches. What a fold step is handed for
    /// the ancestor it judges, built here so that what a derived chain
    /// keeps is decided in this file, as it is for [`iter`](Self::iter).
    fn above(self, depth: usize) -> Self {
        Self {
            chain: self.chain.map(|chain| chain.above(depth)),
            ..self
        }
    }

    /// How far the node this chain describes sits from the root, or
    /// `None` when no chain is known — deriving it then would cost the
    /// [`Node::parent`] climb the chain exists to remove.
    #[inline]
    pub fn depth(self) -> Option<usize> {
        self.chain.map(Chain::len)
    }

    /// The chain of `node`'s children, held: `self` extended by `node`,
    /// for a consumer that holds a body's chain and asks a row about the
    /// body's *members* — `npa` and `npm` over a class body, a parse
    /// error's children asked whether they are doc targets — and, where
    /// it reads through a member's own children, moves the chain with
    /// [`ChildAncestors::descend`] rather than building another. A row's
    /// gate reads the chain it is handed as the queried node's own, so
    /// handing it the body's instead answers from the wrong parent the
    /// moment a `Member` or `FuncOpener` row grows a gate; the comment
    /// "the row reads no chain" was the only guard before this. Without a
    /// known chain the result climbs, as [`Ancestors::unknown`] does.
    /// Read it with [`ChildAncestors::get`].
    ///
    /// The walk's chain is borrowed, not copied: from it this is one
    /// allocation of one entry, whatever the depth. From a chain itself
    /// derived it copies the entries that one added below the walk's,
    /// which every caller bounds by a fixed structural nesting. A copy of
    /// the whole chain per container was quadratic in nested containers
    /// (#1805).
    #[must_use]
    pub fn of_children(self, node: &Node<'tree>) -> ChildAncestors<'tree, 'chain> {
        child_chains_derived::record();
        ChildAncestors {
            chain: self.chain.map(|chain| HeldChain::below(chain, *node)),
            folds: self.folds,
        }
    }

    /// `self` re-rooted on `chain`, which replaces its chain rather than
    /// extending it, with the walk's [`FoldCache`] carried over and its
    /// [`SiblingCache`] deliberately not. Doxygen hands a conditional's
    /// children a chain of that one entry this way (`chain_below`).
    ///
    /// The fold cache is per walk, not per chain, so a derived chain
    /// reads and stores through the same one. Rebuilding with
    /// [`Ancestors::known`] instead dropped it silently — the shape
    /// lesson 101 is about — and left Doxygen's conditionals inside an
    /// `ERROR` child unmemoised (#1686 review). Sharing it is sound for
    /// a fold lane, whose slots are keyed by depth and id, and for a
    /// memo of a node's own subtree. It is not for
    /// [`nearest_level_answer`](Self::nearest_level_answer), whose memo
    /// holds an answer about the ancestry this chain cuts short: do not
    /// ask one from a re-rooted chain.
    ///
    /// The sibling cache is keyed by depth and holds only the nodes the
    /// walk has pushed; the node this chain descends into is one the walk
    /// has *not* reached, so its slot is empty and a lookup through it
    /// would trip the walk-must-record assertion in [`lookup`](Self::lookup)
    /// on a wide one. Java's `is_doc_target` asks for a sibling, and a
    /// `class {` with a hundred stray `@` after it made exactly that
    /// `ERROR` (#1686 review). Without the cache the lookup seeks, which
    /// is what it did before and all an unrecorded node can offer.
    /// [`ChildAncestors::get`] drops it for the same reason.
    #[inline]
    #[must_use]
    pub(crate) const fn below<'below>(
        self,
        chain: &'below [Node<'tree>],
    ) -> Ancestors<'tree, 'below>
    where
        'chain: 'below,
    {
        Ancestors {
            chain: Some(Chain::flat(chain)),
            siblings: None,
            folds: self.folds,
        }
    }

    /// `node`'s parent.
    #[inline]
    #[must_use]
    pub fn parent(self, node: &Node<'tree>) -> Option<Node<'tree>> {
        match self.chain {
            Some(chain) => chain.last().copied(),
            None => node.parent(),
        }
    }

    /// Whether `node`'s parent has kind id `kind`.
    ///
    /// The compound-leaf guard of `docs/development/grammar_dispatch.md`
    /// section 5 asks this and nothing else: a delimiter or keyword
    /// token is suppressed *only* directly under the construct that
    /// owns it, never under an arbitrary ancestor. Eleven dispatch
    /// arms across `getter`, `checker` and the metric walkers spelled
    /// it out as `.parent(node).is_some_and(|p| p.kind_id() == X as
    /// u16)`, which wraps onto four rustfmt lines inside a match guard
    /// and buries the question under the plumbing; #1314 added six more
    /// and folded all seventeen onto this, and #1697 the `ElseIf`
    /// gates. Two sites spelling their binding `|parent|` were left
    /// alone and would each fit:
    /// `is_stabby_lambda_body` (`src/lang_helpers/ruby.rs`) and
    /// `is_useful_comment` (`src/checker/rust.rs`, a let-chain).
    ///
    /// A `false` when `node` has no parent is the answer every one of
    /// those call sites wants: a root node's token is not inside the
    /// construct, so it is not suppressed.
    #[inline]
    #[must_use]
    pub fn parent_has_kind(self, node: &Node<'tree>, kind: u16) -> bool {
        self.parent(node).is_some_and(|p| p.kind_id() == kind)
    }

    /// `node`'s ancestors, nearest first, each paired with *its* own
    /// ancestry so a predicate applied to an ancestor stays as cheap as
    /// one applied to `node`.
    #[inline]
    #[must_use]
    pub fn iter(self, node: &Node<'tree>) -> AncestorIter<'tree, 'chain> {
        AncestorIter(match self.chain {
            Some(chain) => AncestorSource::Chain(chain, self.siblings, self.folds),
            None => AncestorSource::Climb(node.parent()),
        })
    }
}

// Chains derived with `Ancestors::of_children`, so a test can see a walk
// derive one per container body rather than one per node it visits: a
// derivation is one allocation since #1805, so a build hoisted ahead of
// the body-kind test (lesson 103, #1696) is a constant factor the scaling
// gate cannot price, and this counter is what tells the two apart.
crate::observation::counter!(
    child_chains_derived,
    cfg(any(test, feature = "test-support"))
);

/// Ancestor iterator returned by [`Ancestors::iter`], nearest first.
///
/// The representation is private so that [`Ancestors::iter`] is its
/// only constructor: every chain it yields carries the caches of the
/// `Ancestors` it was made from, and nothing else can pair a
/// [`FoldCache`] with a chain. That is what lets the walk constructors
/// ([`Ancestors::for_walk`], which builds on
/// [`Ancestors::for_walk_without_siblings`]) be the one place a visit is
/// recorded (#1737) — a public variant holding `Option<&FoldCache>`
/// would have let any caller lend a cache without one.
pub struct AncestorIter<'tree, 'chain>(AncestorSource<'tree, 'chain>);

enum AncestorSource<'tree, 'chain> {
    /// The not-yet-yielded prefix of a known chain, and the walk's
    /// [`SiblingCache`] and [`FoldCache`] if it lent them. The prefix's
    /// last element is the next ancestor, and the prefix before it is
    /// that ancestor's own chain — so splitting from the back hands out
    /// both at once.
    ///
    /// The caches travel with each yielded chain, so a predicate asked
    /// of an ancestor is memoised exactly as one asked of the node
    /// itself. A braced Tcl word's role is asked of its *parent* through
    /// this iterator, and dropping the [`FoldCache`] here left that
    /// question unmemoised on every walk that lent one (#1686).
    Chain(
        Chain<'tree, 'chain>,
        Option<&'chain SiblingCache<'tree>>,
        Option<&'chain FoldCache>,
    ),
    /// The next ancestor to yield, reached by climbing.
    Climb(Option<Node<'tree>>),
}

impl<'tree, 'chain> Iterator for AncestorIter<'tree, 'chain> {
    type Item = (Node<'tree>, Ancestors<'tree, 'chain>);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            AncestorSource::Chain(remaining, siblings, folds) => {
                let (&nearest, above) = remaining.split_last()?;
                *remaining = above;
                let ancestors = Ancestors {
                    chain: Some(above),
                    siblings: *siblings,
                    folds: *folds,
                };
                Some((nearest, ancestors))
            }
            AncestorSource::Climb(next) => {
                let nearest = (*next)?;
                *next = nearest.parent();
                Some((nearest, Ancestors::unknown()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::folds::{climbed_chain, same_lane_reentry};
    use super::siblings::{Lookup, position_by_start};
    use super::*;
    use crate::node::Tree;

    // `node`'s ancestors' ids, nearest first, climbed with `Node::parent`:
    // the reference every chain a holder derives must reproduce.
    #[cfg(any(
        feature = "cpp",
        feature = "java",
        feature = "kotlin",
        feature = "ruby",
        feature = "rust"
    ))]
    fn climbed_ids(node: &Node) -> Vec<usize> {
        std::iter::successors(node.parent(), Node::parent)
            .map(|ancestor| ancestor.id())
            .collect()
    }

    // Asserts that `ancestors` describes `node`'s whole ancestry as the
    // climb does: its depth, its parent, every ancestor `iter` yields
    // and that ancestor's own chain, its previous sibling and both
    // sibling runs.
    #[cfg(any(
        feature = "cpp",
        feature = "java",
        feature = "kotlin",
        feature = "ruby",
        feature = "rust"
    ))]
    fn assert_describes<'t>(ancestors: Ancestors<'t, '_>, node: &Node<'t>) {
        let kind = node.kind();
        let climbed = climbed_ids(node);
        let id = |node: Option<Node>| node.map(|node| node.id());
        assert_eq!(ancestors.depth(), Some(climbed.len()), "{kind}: depth");
        assert_eq!(
            id(ancestors.parent(node)),
            id(node.parent()),
            "{kind}: parent"
        );
        let mut iterated = Vec::new();
        for (ancestor, above) in ancestors.iter(node) {
            assert_eq!(
                (id(above.parent(&ancestor)), above.depth()),
                (
                    id(ancestor.parent()),
                    Some(climbed.len() - 1 - iterated.len())
                ),
                "{kind}: the chain `iter` yields with its {} ancestor",
                ancestor.kind()
            );
            iterated.push(ancestor.id());
        }
        assert_eq!(iterated, climbed, "{kind}: ancestry");
        assert_eq!(
            id(ancestors.previous_sibling(node)),
            id(node.previous_sibling()),
            "{kind}: previous sibling"
        );
        let (before, after): (Vec<usize>, Vec<usize>) =
            node.parent().map_or_else(Default::default, |parent| {
                let siblings: Vec<usize> = parent.children().map(|child| child.id()).collect();
                let at = siblings
                    .iter()
                    .position(|&sibling| sibling == node.id())
                    .expect("a node is among its parent's children");
                (
                    siblings[..at].iter().rev().copied().collect(),
                    siblings[at + 1..].to_vec(),
                )
            });
        let ids = |nodes: &mut dyn Iterator<Item = Node>| nodes.map(|n| n.id()).collect::<Vec<_>>();
        assert_eq!(
            ids(&mut ancestors.siblings_before(node)),
            before,
            "{kind}: before"
        );
        assert_eq!(
            ids(&mut ancestors.siblings_after(node)),
            after,
            "{kind}: after"
        );
    }

    // Every chain a holder derives below the walk's, at every node of
    // `source`, checked against the climb for each node it describes:
    // one level (`of_children` of the walk's chain), two (`of_children`
    // of a derived chain) and moved (`descend` into each child, then
    // `ascend` back to the floor). Returns how many nodes two levels down
    // were checked, so a caller can tell its fixture nests.
    #[cfg(any(
        feature = "cpp",
        feature = "java",
        feature = "kotlin",
        feature = "ruby",
        feature = "rust"
    ))]
    fn check_derived_ancestry<L: crate::traits::LanguageInfo>(
        source: &[u8],
        recovers: bool,
    ) -> usize {
        let mut checked = 0;
        let check = |node: &Node<'_>, chain: &[Node<'_>]| {
            let walk = Ancestors::known(chain);
            let under = walk.of_children(node);
            let mut held = walk.of_children(node);
            for child in node.children() {
                assert_describes(under.get(), &child);
                let nested = under.get().of_children(&child);
                held.descend(&child);
                for grandchild in child.children() {
                    assert_describes(nested.get(), &grandchild);
                    assert_describes(held.get(), &grandchild);
                    checked += 1;
                }
                held.ascend();
                assert_describes(held.get(), &child);
            }
        };
        if recovers {
            crate::test_support::for_each_node_with_chain_tolerating_errors::<L>(source, check);
        } else {
            crate::test_support::for_each_node_with_chain::<L>(source, check);
        }
        checked
    }

    /// Every chain derived below a walk's — one level, two levels, and
    /// moved with `descend` / `ascend` — answers every ancestry question
    /// as climbing with `Node::parent` does: depth, parent, each ancestor
    /// `iter` yields with that ancestor's own chain, the previous
    /// sibling and both sibling runs (#1805). A view that misread the
    /// seam between the walk's borrowed chain and the entries a holder
    /// owns — an index off by one there, an `iter` that dropped or
    /// repeated the entry either side — answers one of them wrong.
    /// Rust, Ruby, C++ and Java nest the containers the member walks
    /// derive chains for.
    #[cfg(feature = "rust")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_in_rust() {
        let source = b"mod a {\n    struct S { x: i32, y: (i32, u8) }\n    impl S {\n        fn f(&self, v: i32) -> i32 { if v > 0 { g(v) } else { h(1, 2) } }\n    }\n}\n";
        let checked = check_derived_ancestry::<crate::langs::RustCode>(source, false);
        assert!(checked > 50, "the fixture must nest: {checked}");
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_in_ruby() {
        let source = b"class A\n  class B\n    attr_reader :x\n    # Doc.\n    def m(a)\n      if a then b end\n    end\n  end\nend\n";
        let checked = check_derived_ancestry::<crate::langs::RubyCode>(source, false);
        assert!(checked > 20, "the fixture must nest: {checked}");
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_in_cpp() {
        let source = b"namespace n { struct A { struct B { int x; void f(); }; }; }\n";
        let checked = check_derived_ancestry::<crate::langs::CppCode>(source, false);
        assert!(checked > 20, "the fixture must nest: {checked}");
    }

    #[cfg(feature = "java")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_in_java() {
        let source = b"class A { class B { int x; void f() { if (x > 0) { g(); } } } }\n";
        let checked = check_derived_ancestry::<crate::langs::JavaCode>(source, false);
        assert!(checked > 20, "the fixture must nest: {checked}");
    }

    // Whether `source` parses into a tree holding an `ERROR`, a zero-width
    // node at its parent's start, and one at its parent's end: the
    // recovery shapes the parity checks below need present. Only a
    // zero-width child at an edge tells `descend`'s non-strict
    // containment check from a strict one, so one inside its parent
    // would not do.
    #[cfg(any(feature = "kotlin", feature = "rust"))]
    fn recovery_shapes<L: crate::traits::LanguageInfo>(source: &[u8]) -> (bool, bool, bool) {
        let tree = Tree::new::<L>(source);
        let nodes: Vec<Node> = tree.get_root().preorder().collect();
        let zero_width_at = |edge: &dyn Fn(&Node, &Node) -> bool| {
            nodes.iter().any(|node| {
                node.start_byte() == node.end_byte()
                    && node.parent().is_some_and(|parent| edge(node, &parent))
            })
        };
        (
            nodes.iter().any(Node::is_error),
            zero_width_at(&|node, parent| node.start_byte() == parent.start_byte()),
            zero_width_at(&|node, parent| node.end_byte() == parent.end_byte()),
        )
    }

    /// The same parity under recovery: an `ERROR` the parser could not
    /// place, and a `;` it inserted as a zero-width node at its parent's
    /// end. `descend`'s containment check compares identity and
    /// non-strict spans, so a zero-width child at either edge is still
    /// contained.
    #[cfg(feature = "rust")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_under_recovery() {
        let source =
            b"fn f() { let x = ; }\nfn g() { let y = 1 }\nmod m { struct S { a: (i32, u8) } }\n";
        let (error, _, at_end) = recovery_shapes::<crate::langs::RustCode>(source);
        assert!(
            error && at_end,
            "the fixture must hold an ERROR and a zero-width node at its parent's end"
        );
        let checked = check_derived_ancestry::<crate::langs::RustCode>(source, true);
        assert!(checked > 20, "the fixture must nest: {checked}");
    }

    /// Kotlin's `vararg: Int` holds the other edge: an identifier the
    /// parser inserted as a zero-width node at its parent's start.
    #[cfg(feature = "kotlin")]
    #[test]
    fn a_derived_chain_describes_the_whole_ancestry_around_a_zero_width_start() {
        let source = b"fun f(vararg: Int, b: Int) {}\n";
        let (error, at_start, _) = recovery_shapes::<crate::langs::KotlinCode>(source);
        assert!(
            !error && at_start,
            "the fixture must hold a zero-width node at its parent's start and no ERROR"
        );
        let checked = check_derived_ancestry::<crate::langs::KotlinCode>(source, true);
        assert!(checked > 10, "the fixture must nest: {checked}");
    }

    /// A chain that lends no `SiblingCache` reaching `previous_sibling`'s
    /// scan under a wide parent trips the debug assertion there, rather
    /// than going quadratic in silence (#1785). No production walk does;
    /// this is what shows the assertion can fire at all, so its silence
    /// across the suite means something.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[should_panic(expected = "with no SiblingCache (#1785)")]
    fn previous_sibling_rejects_a_wide_parent_without_its_cache() {
        let source = "fn a() {}\n".repeat(WIDE_PARENT + 1);
        let tree = Tree::new::<crate::langs::RustCode>(source.as_bytes());
        let root = tree.get_root();
        let last = root.children().last().expect("the file has items");
        let _ = Ancestors::known(&[root]).previous_sibling(&last);
    }

    /// A chain that does not describe the node still falls back to the
    /// tree under a wide parent, as it does under a narrow one
    /// (`previous_sibling_falls_back_on_a_chain_that_is_not_this_nodes`):
    /// the #1785 assertion is about a scan that found the node, and a
    /// mismatched chain is the caller error the fallback answers.
    /// Discriminates only under a debug build, where the assertion is
    /// compiled in.
    #[cfg(feature = "rust")]
    #[test]
    fn previous_sibling_falls_back_on_a_wide_chain_that_is_not_this_nodes() {
        let source = format!(
            "fn a() {{ let x = 1; }}\n{}",
            "fn b() {}\n".repeat(WIDE_PARENT)
        );
        let tree = Tree::new::<crate::langs::RustCode>(source.as_bytes());
        let root = tree.get_root();
        assert!(root.child_count() > WIDE_PARENT, "the root must be wide");
        let statement = root
            .preorder()
            .find(|node| node.kind() == "let_declaration")
            .expect("fixture has a statement");
        let expected = statement
            .previous_sibling()
            .map(|node| node.id())
            .expect("the statement follows the body's `{`");
        // The root is not the statement's parent, which is the body.
        assert_eq!(
            Ancestors::known(&[root])
                .previous_sibling(&statement)
                .map(|node| node.id()),
            Some(expected),
            "a mismatched chain must fall back, not panic or report `None`"
        );
    }

    // The ids of `nodes`, so a failing comparison prints which differ.
    #[cfg(feature = "perl")]
    fn ids<'a>(nodes: impl Iterator<Item = Node<'a>>) -> Vec<usize> {
        nodes.map(|node| node.id()).collect()
    }

    // `parent`'s children through `child(i)`, the raw walk that neither
    // the seek nor the recorded array is built on.
    #[cfg(feature = "perl")]
    fn raw_children(parent: &Node) -> Vec<usize> {
        let raw = parent.as_tree_sitter();
        (0..raw.child_count() as u32)
            .filter_map(|i| raw.child(i))
            .map(|child| child.id())
            .collect()
    }

    // `my @r = (` and a body never closed: recovery makes the root an
    // `ERROR` whose children are the body's tokens, one flat array of
    // well over `WIDE_PARENT` of them.
    #[cfg(feature = "perl")]
    fn flat_error() -> Tree {
        let source = format!("my @r = ({}", "{\nlast,\n$a && $b,\n".repeat(20));
        Tree::new::<crate::langs::PerlCode>(source.as_bytes())
    }

    /// `fold_down` answers the same through all three of its paths: the
    /// walk's memoised [`FoldCache`], a known chain with no cache, and the
    /// chainless climb. Queried in walk order, as the metrics walk queries
    /// it, and twice per node so the second query is answered from the
    /// stored slots (#1597). The step depends on order, on `below` and on
    /// the parent its `outer` chain names, so a slot read at the wrong
    /// depth, a climb in the wrong direction or an `outer` cut at the wrong
    /// place changes the answer. The climb hands `Ancestors::unknown()`,
    /// whose `parent` falls back to `Node::parent`, so all three agree only
    /// when the chains are right.
    #[cfg(feature = "rust")]
    #[test]
    fn fold_down_agrees_across_its_cache_chain_and_climb_paths() {
        let tree = Tree::new::<crate::langs::RustCode>(
            b"mod a {\n    mod b {\n        fn f() { let x = (1 + 2) * 3; }\n    }\n    fn g() {}\n}\nfn h() { if true { 4 } else { 5 }; }\n",
        );
        let step = |state: u8, ancestor: &Node, outer: Ancestors, below: &Node| {
            let parent = outer
                .parent(ancestor)
                .map_or(0, |parent| parent.kind_id().to_le_bytes()[0]);
            state
                .wrapping_mul(31)
                .wrapping_add(ancestor.kind_id().to_le_bytes()[0])
                .wrapping_add(below.kind_id().to_le_bytes()[0].rotate_left(3))
                .wrapping_add(parent.rotate_left(5))
        };
        let cache = FoldCache::default();
        let mut stack = vec![(tree.get_root(), 0_usize)];
        let mut chain: Vec<Node> = Vec::new();
        let mut queried = 0_usize;
        while let Some((node, depth)) = stack.pop() {
            chain.truncate(depth);
            for skip in 0..=3 {
                let cached = |c: &[Node<'_>]| {
                    Ancestors::known(c).with_folds(&cache).fold_down(
                        &node,
                        skip,
                        FoldLane::DocInBody,
                        7,
                        step,
                    )
                };
                let first = cached(&chain);
                let again = cached(&chain);
                let uncached =
                    Ancestors::known(&chain).fold_down(&node, skip, FoldLane::DocInBody, 7, step);
                let climbed =
                    Ancestors::unknown().fold_down(&node, skip, FoldLane::DocInBody, 7, step);
                assert_eq!(
                    (first, again, uncached),
                    (climbed, climbed, climbed),
                    "{} at depth {depth}, skip {skip}",
                    node.kind()
                );
                // The same chain as a derived one holds it, split between
                // the walk's run and the run below it at every depth, so
                // `skip` lands either side of the seam and on it. The
                // cached fold reads the slots the flat folds above stored,
                // so a depth read off by one across the seam changes the
                // answer. The crossing step folds on another lane through
                // its `outer`, a chain cut from the split one.
                let crossing = |state: u8, ancestor: &Node, outer: Ancestors, below: &Node| {
                    step(state, ancestor, outer, below).wrapping_add(outer.fold_down(
                        ancestor,
                        0,
                        FoldLane::DocScope,
                        3,
                        step,
                    ))
                };
                let crossed =
                    Ancestors::unknown().fold_down(&node, skip, FoldLane::InsideQuote, 7, crossing);
                for split in 0..=chain.len() {
                    let (upper, lower) = chain.split_at(split);
                    let segmented = Ancestors {
                        chain: Some(Chain { upper, lower }),
                        siblings: None,
                        folds: Some(&cache),
                    };
                    assert_eq!(
                        (
                            segmented.fold_down(&node, skip, FoldLane::DocInBody, 7, step),
                            segmented.fold_down(&node, skip, FoldLane::InsideQuote, 7, crossing),
                        ),
                        (climbed, crossed),
                        "{} at depth {depth}, skip {skip}, split {split}",
                        node.kind()
                    );
                }
                queried += 1;
            }
            chain.push(node);
            let children: Vec<_> = node.children().collect();
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
        assert!(
            queried > 100,
            "the fixture must exercise a real walk: {queried}"
        );
    }

    /// A node reached through a flat chain and through a derived one
    /// folds through the same depth slots (#1805): a state stored
    /// through either is read back through the other, so neither refolds
    /// an ancestor the other folded. Counted from a cache seeded by a
    /// cold fold of the whole ancestry, so a reused slot shows as steps
    /// not taken rather than as a count that would read zero anyway. The
    /// step at the queried node itself depends on that node and is never
    /// stored, so every query still calls it once; `fold_steps` counts
    /// only the stored ones, which is why the two counts differ by one.
    #[cfg(feature = "rust")]
    #[test]
    fn a_flat_and_a_derived_chain_share_the_fold_slots() {
        use std::cell::Cell;
        let tree = Tree::new::<crate::langs::RustCode>(
            b"mod a { mod b { fn f() { let x = (1 + 2) * 3; } } }\n",
        );
        let node = tree
            .get_root()
            .preorder()
            .find(|node| {
                node.kind() == "binary_expression"
                    && node
                        .child(0)
                        .is_some_and(|lhs| lhs.kind() == "integer_literal")
            })
            .expect("the fixture holds `1 + 2`");
        let child = node.child(0).expect("the `1`");
        let chain = climbed_chain(&node);
        let depth = chain.len();
        assert!(depth > 6, "the fixture must nest: {depth}");
        let calls = Cell::new(0_usize);
        let step = |state: u8, ancestor: &Node, _: Ancestors, below: &Node| {
            calls.set(calls.get() + 1);
            state
                .wrapping_mul(31)
                .wrapping_add(ancestor.kind_id().to_le_bytes()[0])
                .wrapping_add(below.kind_id().to_le_bytes()[0].rotate_left(3))
        };
        let lane = FoldLane::DocInBody;
        let at_node = Ancestors::unknown().fold_down(&node, 0, lane, 7, step);
        let at_child = Ancestors::unknown().fold_down(&child, 0, lane, 7, step);
        // A fold's answer, the steps it stored and the step calls it made.
        let counted = |fold: &dyn Fn() -> u8| {
            let (steps, before) = (fold_steps::observed(), calls.get());
            let answer = fold();
            (answer, fold_steps::observed() - steps, calls.get() - before)
        };

        let cache = FoldCache::default();
        let walk = Ancestors::known(&chain).with_folds(&cache);
        assert_eq!(
            counted(&|| walk.fold_down(&node, 0, lane, 7, step)),
            (at_node, depth - 1, depth),
            "cold, every ancestor below the root is folded and stored"
        );
        for split in 0..=depth {
            let (upper, lower) = chain.split_at(split);
            let segmented = Ancestors {
                chain: Some(Chain { upper, lower }),
                siblings: None,
                folds: Some(&cache),
            };
            assert_eq!(
                counted(&|| segmented.fold_down(&node, 0, lane, 7, step)),
                (at_node, 0, 1),
                "split at {split}, every stored state is read back"
            );
        }
        let under = walk.of_children(&node);
        assert_eq!(
            counted(&|| under.get().fold_down(&child, 0, lane, 7, step)),
            (at_child, 1, 2),
            "one level down, the node's own depth is folded and stored"
        );
        let flat: Vec<Node> = chain.iter().copied().chain([node]).collect();
        let flat_walk = Ancestors::known(&flat).with_folds(&cache);
        assert_eq!(
            counted(&|| flat_walk.fold_down(&child, 0, lane, 7, step)),
            (at_child, 0, 1),
            "and the walk's own chain at the child reads that slot back"
        );
    }

    /// A fold's step may fold on another lane, and that inner fold is
    /// memoised through the same cache: each lane is its own cell, so the
    /// outer fold's borrow does not block it. A step that folds on its
    /// *own* lane finds that lane borrowed and runs unmemoised, answering
    /// what the plain fold does rather than panicking on a second borrow,
    /// at the plain fold's cost.
    ///
    /// The inner fold counts its own steps, so memoisation is observed
    /// rather than inferred: walked in order, each inner fold costs one
    /// step per new depth, so the total stays near the node count, where
    /// an unmemoised inner fold costs the whole depth every time.
    #[cfg(feature = "rust")]
    #[test]
    fn a_step_folds_another_lane_memoised_and_its_own_lane_unmemoised() {
        use crate::test_support::for_each_node_with_chain;
        use std::cell::Cell;
        // Every level opens a block, so depth grows with the fixture and
        // an unmemoised inner fold is visibly quadratic in step count.
        let source = format!("fn f() {{ {} }}\n", "{ ".repeat(60) + &"} ".repeat(60));
        let inner_steps = Cell::new(0_usize);
        let inner = |state: u8, ancestor: &Node, _: Ancestors, _: &Node| {
            inner_steps.set(inner_steps.get() + 1);
            state
                .wrapping_mul(7)
                .wrapping_add(ancestor.kind_id().to_le_bytes()[0])
        };
        let outer_on = |lane: FoldLane| {
            move |state: u8, ancestor: &Node, outer: Ancestors, _: &Node| {
                state.wrapping_add(outer.fold_down(ancestor, 0, lane, 3, inner))
            }
        };
        let fold = |chain: &[Node], node: &Node, cache: Option<&FoldCache>, lane, inner_lane| {
            let ancestors = Ancestors::known(chain);
            let ancestors = cache.map_or(ancestors, |cache| ancestors.with_folds(cache));
            ancestors.fold_down(node, 0, lane, 0, outer_on(inner_lane))
        };

        let cache = FoldCache::default();
        same_lane_reentry::allow(true);
        let nodes =
            for_each_node_with_chain::<crate::langs::RustCode>(source.as_bytes(), |node, chain| {
                let (other, own) = (FoldLane::DocInBody, FoldLane::BareStringClimb);
                assert_eq!(
                    fold(chain, node, Some(&cache), other, FoldLane::DocScope),
                    fold(chain, node, None, other, FoldLane::DocScope),
                    "another lane must answer what the plain fold does"
                );
                let before = inner_steps.get();
                let own_lane = fold(chain, node, Some(&cache), own, own);
                let own_steps = inner_steps.get() - before;
                assert_eq!(
                    own_lane,
                    fold(chain, node, None, own, own),
                    "its own lane must answer what the plain fold does"
                );
                // One plain fold of the prefix per query: `inner` folds
                // its own lane once, at the node, and re-enters nowhere.
                let depth = chain.len();
                assert!(
                    own_steps <= depth * (depth + 1) / 2,
                    "its own lane must cost one plain fold of the prefix"
                );
            });
        same_lane_reentry::allow(false);
        assert!(nodes > 200, "the fixture must nest deeply: {nodes}");

        // The other-lane fold alone on a fresh cache, counting only its
        // inner steps.
        let cache = FoldCache::default();
        inner_steps.set(0);
        for_each_node_with_chain::<crate::langs::RustCode>(source.as_bytes(), |node, chain| {
            fold(
                chain,
                node,
                Some(&cache),
                FoldLane::DocInBody,
                FoldLane::DocScope,
            );
        });
        // Memoised, the inner folds cost a small constant per node;
        // unmemoised, each outer step's inner fold runs the whole prefix,
        // `Σ depth` along the spine — quadratic in it, far past this
        // bound.
        assert!(
            inner_steps.get() <= 4 * nodes,
            "the other lane is not memoised"
        );
    }

    /// A recorded wide parent answers every child's siblings, and its
    /// previous sibling, from the recorded array, and answers exactly
    /// what the raw walk does on either side of it — the `ERROR` whose
    /// flat array made the seek `O(index)` (#1584). A node that is no
    /// child of it gets nothing.
    #[cfg(feature = "perl")]
    #[test]
    fn siblings_of_a_recorded_wide_parent_match_the_child_walk() {
        let tree = flat_error();
        let root = tree.get_root();
        assert!(root.is_error(), "the fixture's tail must fail to parse");
        assert!(root.child_count() > WIDE_PARENT, "the ERROR must be wide");

        let mut cache = SiblingCache::default();
        cache.record(&root, 0, root.children());
        let chain = [root];
        let ancestors = Ancestors::known(&chain).with_siblings(&cache);
        let all = raw_children(&root);

        let (mut observed, mut expected) = (Vec::new(), Vec::new());
        for (index, child) in root.children().enumerate() {
            let recorded_index = match ancestors.lookup(&child) {
                Lookup::Wide(_, index) => index,
                Lookup::Narrow(_) => None,
            };
            observed.push((
                index,
                recorded_index,
                ids(ancestors.siblings_after(&child)),
                ids(ancestors.siblings_before(&child)),
                ancestors.previous_sibling(&child).map(|node| node.id()),
            ));
            expected.push((
                index,
                Some(index),
                all[index + 1..].to_vec(),
                all[..index].iter().rev().copied().collect(),
                index.checked_sub(1).map(|previous| all[previous]),
            ));
        }
        assert_eq!(observed, expected);

        // A grandchild starting where its parent does, so the search
        // reaches it and only the id comparison turns it away.
        let stray = root
            .children()
            .find_map(|child| {
                child
                    .child(0)
                    .filter(|g| g.start_byte() == child.start_byte())
            })
            .expect("the fixture nests a node at its parent's start");
        assert!(matches!(ancestors.lookup(&stray), Lookup::Wide(_, None)));
        assert_eq!(ancestors.siblings_after(&stray).count(), 0);
        assert_eq!(ancestors.siblings_before(&stray).count(), 0);
        // `previous_sibling` falls back to the node's own lookup instead,
        // which finds a stray's real previous sibling: `$b`'s is the `&&`
        // of its own chain, not anything in the root.
        let operand = root
            .children()
            .find_map(|child| child.child(2))
            .expect("the fixture holds `$a && $b`");
        let previous = ancestors
            .previous_sibling(&operand)
            .expect("the fallback finds the `&&`");
        assert_eq!(previous.kind(), "&&");
    }

    /// A narrow parent is never recorded, and its siblings come from
    /// the seek — with or without a cache lent.
    #[cfg(feature = "perl")]
    #[test]
    fn a_narrow_parent_is_sought_not_recorded() {
        let tree = flat_error();
        let root = tree.get_root();
        let chain = [root];
        let narrow = root
            .children()
            .find(|child| (2..=WIDE_PARENT).contains(&child.child_count()))
            .expect("the fixture holds a narrow node with several children");
        let mut cache = SiblingCache::default();
        cache.record(&narrow, 1, narrow.children());
        assert!(cache.is_empty(), "a narrow node must not be recorded");

        let narrow_chain = [root, narrow];
        let all = raw_children(&narrow);
        for ancestors in [
            Ancestors::known(&narrow_chain).with_siblings(&cache),
            Ancestors::known(&narrow_chain),
        ] {
            let first = narrow.child(0).expect("narrow node has children");
            assert!(matches!(ancestors.lookup(&first), Lookup::Narrow(Some(_))));
            assert_eq!(ids(ancestors.siblings_after(&first)), all[1..]);
        }
        // Without a cache a wide parent is sought too.
        let first = root.child(0).expect("root has children");
        assert!(matches!(
            Ancestors::known(&chain).lookup(&first),
            Lookup::Narrow(Some(_))
        ));
    }

    /// A walk that lends a cache but did not record a wide node on its
    /// chain would answer correctly and quadratically; the assertion is
    /// what makes that visible to every test that walks one.
    #[cfg(all(feature = "perl", debug_assertions))]
    #[test]
    #[should_panic(expected = "must record every wide node on its chain")]
    fn an_unrecorded_wide_parent_trips_the_walk_assertion() {
        let tree = flat_error();
        let root = tree.get_root();
        let cache = SiblingCache::default();
        let chain = [root];
        let first = root.child(0).expect("root has children");
        let _ = Ancestors::known(&chain)
            .with_siblings(&cache)
            .lookup(&first);
    }

    /// A walk records every child of a wide node. A lookup indexes into
    /// the record, so a batch that drops a child shifts each answer
    /// after it rather than slowing it, and no cost probe shows that
    /// (#1871). The recording walks pass what `children_with` yields,
    /// whole; one that came to filter at the push would trip this on its
    /// first wide node. The setup cannot raise the message: it parses
    /// and builds an empty record.
    #[cfg(all(feature = "perl", debug_assertions))]
    #[test]
    #[should_panic(expected = "a walk must record every child of a wide ERROR")]
    fn recording_a_short_batch_is_caught() {
        let tree = flat_error();
        let root = tree.get_root();
        let mut cache = SiblingCache::default();
        cache.record(&root, 0, root.children().skip(1));
    }

    /// And a batch that holds more than the node's children, as a slice
    /// of the walk's stack cut one entry too early would.
    #[cfg(all(feature = "perl", debug_assertions))]
    #[test]
    #[should_panic(expected = "a walk must record every child of a wide ERROR")]
    fn recording_a_padded_batch_is_caught() {
        let tree = flat_error();
        let root = tree.get_root();
        let mut cache = SiblingCache::default();
        cache.record(&root, 0, std::iter::once(root).chain(root.children()));
    }

    // The walk's own chain for `node`, built by each constructor of a walk
    // that records no siblings (#1601).
    #[cfg(feature = "perl")]
    fn unrecorded_walks<'t, 'c>(
        chain: &'c [Node<'t>],
        node: &Node<'t>,
        folds: &'c FoldCache,
    ) -> [(&'static str, Ancestors<'t, 'c>); 2] {
        [
            (
                "for_walk_chain_only",
                Ancestors::for_walk_chain_only(chain, node),
            ),
            (
                "for_walk_without_siblings",
                Ancestors::for_walk_without_siblings(chain, node, folds),
            ),
        ]
    }

    // How many siblings follow `target`, an ancestor of `node`, asked
    // through the `outer` chain `fold_down` hands its step at `target`.
    #[cfg(feature = "perl")]
    fn siblings_after_through_fold<'t>(
        walk: Ancestors<'t, '_>,
        node: &Node<'t>,
        target: &Node<'t>,
    ) -> usize {
        let step = |state, ancestor: &Node<'t>, outer: Ancestors<'t, '_>, _: &Node<'t>| {
            if ancestor.id() == target.id() {
                u8::try_from(outer.siblings_after(ancestor).count()).unwrap_or(u8::MAX)
            } else {
                state
            }
        };
        usize::from(walk.fold_down(node, 0, FoldLane::DocInBody, 0, step))
    }

    // Asserts that `ask` panics with the walk assertion of `lookup`.
    fn assert_trips_the_walk_assertion(label: &str, ask: impl FnOnce() -> usize) {
        let message = match catch_unwind(AssertUnwindSafe(ask)) {
            Ok(count) => panic!("{label}: no assertion, {count} siblings"),
            Err(payload) => payload
                .downcast::<String>()
                .expect("a formatted panic message"),
        };
        assert!(
            message.contains("must record every wide node on its chain"),
            "{label}: {message}"
        );
    }

    /// The check above can fail, in both of its ways: it rejects an ask
    /// that returns, and an ask that panics with another message. The
    /// two rejection tests below rest on it, and would pass for a check
    /// that accepted either. The control asks parse nothing, so this
    /// runs in every build.
    #[test]
    fn the_walk_assertion_check_rejects_a_quiet_ask_and_another_panic() {
        let rejection = |ask: fn() -> usize| {
            *catch_unwind(|| assert_trips_the_walk_assertion("control", ask))
                .expect_err("the check must reject this ask")
                .downcast::<String>()
                .expect("a formatted panic message")
        };
        assert_eq!(rejection(|| 0), "control: no assertion, 0 siblings");
        // A `String` payload, as a formatted assertion's is.
        let another = rejection(|| std::panic::panic_any(String::from("another panic")));
        assert_eq!(another, "control: another panic");
    }

    /// A walk that records no siblings cannot ask for one under a wide
    /// parent: the walk assertion fires through each constructor that
    /// says so, as it does for a walk that skipped a node (#1601). Each
    /// ask runs on its own, so one constructor that lent no record — and
    /// so sought quietly — fails here however the other behaves.
    #[cfg(all(feature = "perl", debug_assertions))]
    #[test]
    fn a_walk_that_records_no_siblings_cannot_ask_under_a_wide_parent() {
        let tree = flat_error();
        let root = tree.get_root();
        assert!(root.child_count() > WIDE_PARENT, "the ERROR must be wide");
        let chain = [root];
        let first = root.child(0).expect("root has children");
        let folds = FoldCache::default();
        for (name, walk) in unrecorded_walks(&chain, &first, &folds) {
            assert_trips_the_walk_assertion(name, || walk.siblings_after(&first).count());
        }
    }

    /// The record travels with the chains a predicate derives from the
    /// walk's own: an ancestor's chain from [`Ancestors::iter`] and the
    /// `outer` chain a fold step is handed are the walk's chains too, so
    /// a sibling asked through either under a wide parent fires the walk
    /// assertion. Dropping the record from `AncestorIter::next` or from
    /// `fold_down`'s `outer` fails the matching ask (#1601).
    #[cfg(all(feature = "perl", debug_assertions))]
    #[test]
    fn an_ancestor_or_fold_chain_of_such_a_walk_cannot_ask_either() {
        let tree = flat_error();
        let root = tree.get_root();
        assert!(root.child_count() > WIDE_PARENT, "the ERROR must be wide");
        let parent = root
            .children()
            .find(|child| child.child_count() > 0)
            .expect("the fixture holds a node with children");
        let leaf = parent.child(0).expect("it has a child");
        let chain = [root, parent];
        let folds = FoldCache::default();
        for (name, walk) in unrecorded_walks(&chain, &leaf, &folds) {
            let (ancestor, above) = walk.iter(&leaf).next().expect("leaf has a parent");
            assert_eq!(ancestor.id(), parent.id());
            assert_trips_the_walk_assertion(&format!("{name} through iter"), || {
                above.siblings_after(&ancestor).count()
            });
            assert_trips_the_walk_assertion(&format!("{name} through a fold"), || {
                siblings_after_through_fold(walk, &leaf, &parent)
            });
        }
    }

    /// Under a narrow parent the same walks ask freely: the seek is a
    /// short scan, so the rule bars only the wide one. Each ask takes the
    /// uncached path once and is counted, through the walk's own chain,
    /// an iterated ancestor's and a fold step's `outer`.
    #[cfg(feature = "perl")]
    #[test]
    fn a_walk_that_records_no_siblings_asks_freely_under_a_narrow_parent() {
        let tree = flat_error();
        let root = tree.get_root();
        let parent = root
            .children()
            .find(|child| {
                (3..=WIDE_PARENT).contains(&child.child_count())
                    && child.child(0).is_some_and(|first| first.child_count() > 0)
            })
            .expect("the fixture holds a narrow node whose first child has children");
        let first = parent.child(0).expect("narrow node has children");
        let second = parent.child(1).expect("and at least three of them");
        let grandchild = first.child(0).expect("its first child has children");
        let chain = [root, parent];
        let below_first = [root, parent, first];
        let folds = FoldCache::default();
        let all = raw_children(&parent);
        for (name, walk) in unrecorded_walks(&chain, &first, &folds) {
            let before = sibling_seeks::observed();
            assert_eq!(ids(walk.siblings_after(&first)), all[1..], "{name}");
            assert_eq!(
                walk.previous_sibling(&second).map(|node| node.id()),
                Some(all[0]),
                "{name}"
            );
            // expected: 2, one uncached lookup per ask above.
            assert_eq!(sibling_seeks::observed() - before, 2, "{name}");
        }
        for (name, walk) in unrecorded_walks(&below_first, &grandchild, &folds) {
            let before = sibling_seeks::observed();
            let (_, above) = walk.iter(&grandchild).next().expect("it has a parent");
            assert_eq!(ids(above.siblings_after(&first)), all[1..], "{name}");
            let folded = siblings_after_through_fold(walk, &grandchild, &first);
            assert_eq!(folded, all.len() - 1, "{name}");
            // expected: 2, the iterated ancestor's ask and the fold's.
            assert_eq!(sibling_seeks::observed() - before, 2, "{name}");
        }
    }

    /// A chain that is not a walk's own — derived below it, re-rooted,
    /// or foreign — seeks a wide parent quietly: no walk recorded the
    /// nodes below its own chain, so there is no record to require. Each
    /// of `siblings_before` and `siblings_after` takes the uncached path
    /// once per ask; `previous_sibling` stays loud there (#1785), so it
    /// is not asked.
    #[cfg(feature = "perl")]
    #[test]
    fn a_chain_that_is_not_a_walk_s_own_seeks_a_wide_parent() {
        let tree = flat_error();
        let root = tree.get_root();
        assert!(root.child_count() > WIDE_PARENT, "the ERROR must be wide");
        let all = raw_children(&root);
        let index = all.len() / 2;
        let child = root.child(index).expect("the middle child");
        let chain = [root];
        let folds = FoldCache::default();
        let siblings = SiblingCache::default();
        let walk = Ancestors::for_walk(&[], &root, &folds, &siblings);
        let held = walk.of_children(&root);
        let derived: [(&str, Ancestors<'_, '_>); 3] = [
            ("known", Ancestors::known(&chain)),
            ("below", walk.below(&chain)),
            ("of_children", held.get()),
        ];
        let expected_before: Vec<_> = all[..index].iter().rev().copied().collect();
        for (name, ancestors) in derived {
            let before = sibling_seeks::observed();
            assert_eq!(
                ids(ancestors.siblings_after(&child)),
                all[index + 1..],
                "{name}"
            );
            assert_eq!(
                ids(ancestors.siblings_before(&child)),
                expected_before,
                "{name}"
            );
            // expected: 2, one uncached lookup per ask above.
            assert_eq!(sibling_seeks::observed() - before, 2, "{name}");
        }
        let before = sibling_seeks::observed();
        assert_eq!(
            Ancestors::unknown().siblings_after(&child).count(),
            all.len() - index - 1
        );
        assert_eq!(
            sibling_seeks::observed(),
            before,
            "a chainless lookup is not counted"
        );
    }

    /// The binary search finds every child at its index, including a
    /// zero-width one: recovery inserts it at the byte its successor
    /// starts at, so only the id tells the two apart. The Kotlin
    /// fixture's `vararg: Int` holds one.
    #[cfg(feature = "kotlin")]
    #[test]
    fn position_by_start_finds_a_zero_width_child() {
        let tree = Tree::new::<crate::langs::KotlinCode>(b"fun f(vararg: Int, b: Int) {}");
        let mut zero_width = 0;
        for node in tree.get_root().preorder() {
            let children: Vec<_> = node.children().collect();
            for (index, child) in children.iter().enumerate() {
                zero_width += usize::from(child.start_byte() == child.end_byte());
                assert_eq!(position_by_start(&children, child), Some(index));
            }
        }
        assert!(
            zero_width > 0,
            "fixture must hold an inserted zero-width child"
        );
    }

    /// A chain yielded by [`Ancestors::iter`] carries the walk's
    /// [`FoldCache`], so a memo stored through the node's own chain is
    /// read back through an ancestor's. The Tcl-family role question is
    /// asked of a braced word's *parent* through this iterator, and an
    /// iterator that dropped the cache left it unmemoised on every walk
    /// that lent one (#1686). Restoring `folds: None` in
    /// `AncestorIter::next` fails the `expect` on the iterated ancestor's
    /// memo.
    #[cfg(feature = "rust")]
    #[test]
    fn an_iterated_ancestor_keeps_the_walk_s_memo() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { g(1); }\n");
        let root = tree.get_root();
        // Root first, then the `function_item`, its block, and the
        // expression statement in it: the parent of `g(1)`.
        let mut chain = vec![root];
        for kind in ["function_item", "block", "expression_statement"] {
            let last = *chain.last().expect("the chain starts non-empty");
            let child = last
                .children()
                .find(|child| child.kind() == kind)
                .unwrap_or_else(|| panic!("the fixture nests a {kind}"));
            chain.push(child);
        }
        let leaf = chain
            .last()
            .expect("the chain has a parent")
            .child(0)
            .expect("the statement holds the call");
        assert_eq!(leaf.kind(), "call_expression");

        let cache = FoldCache::default();
        let walk = Ancestors::known(&chain).with_folds(&cache);
        walk.node_memo(MemoQuestion::ArgumentSlotsReadable)
            .expect("the chain lent a cache")
            .set(&leaf, true);
        assert!(
            Ancestors::known(&chain)
                .node_memo(MemoQuestion::ArgumentSlotsReadable)
                .is_none(),
            "a chain lent no cache has no memo"
        );

        let (_, above) = walk.iter(&leaf).next().expect("the leaf has a parent");
        let memo = above
            .node_memo(MemoQuestion::ArgumentSlotsReadable)
            .expect("an iterated ancestor's chain keeps the lent cache");
        assert_eq!(memo.get(&leaf), Some(true));

        // The other derivations: a chain extended *downward* for a node
        // the walk has not reached, and one re-rooted with `below`, read
        // the same memo. Rebuilding with `Ancestors::known` instead
        // answers `None`, which is what `error_holds_target` did before
        // #1686.
        let under = walk.of_children(&leaf);
        let derived = under.get();
        assert_eq!(derived.depth(), Some(chain.len() + 1));
        let memo = derived
            .node_memo(MemoQuestion::ArgumentSlotsReadable)
            .expect("a chain derived with `of_children` keeps the lent cache");
        assert_eq!(memo.get(&leaf), Some(true));
        let memo = walk
            .below(&chain[..1])
            .node_memo(MemoQuestion::ArgumentSlotsReadable)
            .expect("a chain re-rooted with `below` keeps the lent cache");
        assert_eq!(memo.get(&leaf), Some(true));
    }

    /// The word slot serves the node the walk is visiting and no other
    /// (#1737). Through a chain built with `for_walk` for `a`, the first
    /// ask about `a` computes and every later one is served — across
    /// asks about another node `b` through the walk's own chain, a chain
    /// derived from it with `of_children`, `below` and `iter`, and the
    /// fold-free chain `fold_down` hands its step, each of which computes
    /// `b`'s answer and stores nothing. Restoring the last-node-asked
    /// slot, which stored every ask, fails at the second ask about `a`:
    /// the ask about `b` in between evicted it. A chain lent the cache
    /// without a visit computes every ask, and a later `for_walk` for `b`
    /// retires `a`'s entry. Both walk constructors record the visit, so
    /// the same sequence runs through each (#1601).
    #[cfg(feature = "rust")]
    #[test]
    fn the_word_slot_serves_only_the_visited_node() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { g(1); }\n");
        let root = tree.get_root();
        let a = root.child(0).expect("the fn item");
        let body = a
            .children()
            .find(|child| child.kind() == "block")
            .expect("the body");
        let b = body.child(1).expect("the call statement");
        assert_ne!(a.id(), b.id());
        word_slot_serves_only_the_visited_node(root, a, body, b, Some(&SiblingCache::default()));
        word_slot_serves_only_the_visited_node(root, a, body, b, None);
    }

    // The walk's own chain for `node` through either walk constructor:
    // `for_walk` lending `siblings`, or `for_walk_without_siblings`.
    #[cfg(feature = "rust")]
    fn either_walk<'t, 'c>(
        chain: &'c [Node<'t>],
        node: &Node<'t>,
        folds: &'c FoldCache,
        siblings: Option<&'c SiblingCache<'t>>,
    ) -> Ancestors<'t, 'c> {
        match siblings {
            Some(siblings) => Ancestors::for_walk(chain, node, folds, siblings),
            None => Ancestors::for_walk_without_siblings(chain, node, folds),
        }
    }

    // `the_word_slot_serves_only_the_visited_node` through one of the
    // two walk constructors.
    #[cfg(feature = "rust")]
    fn word_slot_serves_only_the_visited_node<'t>(
        root: Node<'t>,
        a: Node<'t>,
        body: Node<'t>,
        b: Node<'t>,
        siblings: Option<&SiblingCache<'t>>,
    ) {
        use std::cell::Cell;
        let runs = Cell::new(0_usize);
        let computes = |answer: u8| {
            let runs = &runs;
            move || {
                runs.set(runs.get() + 1);
                answer
            }
        };
        let cache = FoldCache::default();
        let chain = [root];
        let walk = either_walk(&chain, &a, &cache, siblings);
        let slot = WordSlot::ElixirCallTarget;

        assert_eq!(walk.classified(slot, &a, computes(1)), 1);
        assert_eq!(runs.get(), 1, "the first ask computes");
        assert_eq!(walk.classified(slot, &a, computes(9)), 1);
        assert_eq!(runs.get(), 1, "the second ask is served");

        assert_eq!(walk.classified(slot, &b, computes(2)), 2);
        assert_eq!(walk.classified(slot, &b, computes(2)), 2);
        assert_eq!(
            runs.get(),
            3,
            "an ask about another node computes every time"
        );
        assert_eq!(walk.classified(slot, &a, computes(9)), 1);
        assert_eq!(
            runs.get(),
            3,
            "and does not evict the visited node's answer"
        );

        let under = walk.of_children(&a);
        assert_eq!(under.get().classified(slot, &b, computes(2)), 2);
        let replaced = [root, a];
        assert_eq!(walk.below(&replaced).classified(slot, &b, computes(2)), 2);
        let (_, above) = walk.iter(&b).next().expect("b has a parent");
        assert_eq!(above.classified(slot, &b, computes(2)), 2);
        let folded = walk.fold_down(&a, 0, FoldLane::DocInBody, 0, |_, _, outer, _| {
            outer.classified(slot, &b, computes(2))
        });
        assert_eq!(folded, 2);
        assert_eq!(runs.get(), 7, "each derived chain's ask computes");
        assert_eq!(walk.classified(slot, &a, computes(9)), 1);
        assert_eq!(runs.get(), 7, "and none of them evicts");
        assert_eq!(under.get().classified(slot, &a, computes(9)), 1);
        assert_eq!(runs.get(), 7, "a derived chain serves the visited node too");

        let never_visited = FoldCache::default();
        let unvisited = Ancestors::known(&chain).with_folds(&never_visited);
        assert_eq!(unvisited.classified(slot, &a, computes(1)), 1);
        assert_eq!(unvisited.classified(slot, &a, computes(1)), 1);
        assert_eq!(
            runs.get(),
            9,
            "a cache lent without a visit computes every ask"
        );

        let chain_b = [root, a, body];
        let walk_b = either_walk(&chain_b, &b, &cache, siblings);
        assert_eq!(walk_b.classified(slot, &b, computes(2)), 2);
        assert_eq!(walk_b.classified(slot, &b, computes(9)), 2);
        assert_eq!(
            runs.get(),
            10,
            "the new visit's first ask computes, the next is served"
        );
        assert_eq!(walk_b.classified(slot, &a, computes(1)), 1);
        assert_eq!(runs.get(), 11, "the retired node is computed again");
    }

    /// `of_children` hands a body's members the chain that is their own:
    /// `parent` on a member is the body, where the body's chain would
    /// name the body's parent instead — the shape `npa` / `npm` ask a
    /// row about every member under. The derived chain keeps the walk's
    /// fold cache, as `below` does, and without a known chain it climbs.
    #[cfg(feature = "rust")]
    #[test]
    fn of_children_names_the_body_as_each_member_s_parent() {
        let tree = Tree::new::<crate::langs::RustCode>(b"struct S { a: i32, b: i32 }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the struct item");
        let body = item
            .children()
            .find(|child| child.kind() == "field_declaration_list")
            .expect("the field list");
        let field = body
            .children()
            .find(|child| child.kind() == "field_declaration")
            .expect("a field");

        let chain = [root, item];
        let cache = FoldCache::default();
        let walk = Ancestors::known(&chain).with_folds(&cache);
        assert_eq!(
            walk.parent(&body).map(|p| p.id()),
            Some(item.id()),
            "the body's own chain names the item"
        );
        let under = walk.of_children(&body);
        let members = under.get();
        assert_eq!(
            members.parent(&field).map(|p| p.id()),
            Some(body.id()),
            "a member's chain names the body"
        );
        assert_eq!(members.depth(), Some(3), "root, item, body");
        assert!(
            members.memoises_folds(),
            "the derived chain keeps the walk's fold cache"
        );
        assert_eq!(
            Ancestors::unknown()
                .of_children(&body)
                .get()
                .parent(&field)
                .map(|p| p.id()),
            Some(body.id()),
            "without a chain the derived one climbs to the same parent"
        );
    }

    /// A chain derived with `of_children` borrows the walk's chain in
    /// place — the same slice, not a copy of it — and owns only what it
    /// adds: the body, and below a derived chain the levels that one
    /// added. A copy of the walk's chain per container made `n` nested
    /// containers cost `Σ depth` (#1805); the scaling probes price that
    /// out of band, and this is the per-change guard. Reinstating the
    /// copy fails the first assertion. A root's children derive from the
    /// empty chain the walk holds there, and a leaf's from its own like
    /// any other node, describing nothing.
    #[cfg(feature = "rust")]
    #[test]
    fn of_children_borrows_the_walk_s_chain_and_owns_only_what_it_adds() {
        // Where a slice starts and how long it is: equal for two slices
        // only when one is the other, not a copy of it.
        fn at(nodes: &[Node]) -> (usize, usize) {
            (nodes.as_ptr().addr(), nodes.len())
        }
        // A derived chain's borrowed run, and the ids of the run it owns.
        fn view(under: &ChildAncestors<'_, '_>) -> ((usize, usize), Vec<usize>) {
            let chain = under
                .get()
                .chain
                .expect("a known chain derives a known one");
            (at(chain.upper), chain.lower.iter().map(Node::id).collect())
        }
        let tree = Tree::new::<crate::langs::RustCode>(b"struct S { a: (i32, u8) }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the struct item");
        let body = item
            .children()
            .find(|child| child.kind() == "field_declaration_list")
            .expect("the field list");
        let field = body
            .children()
            .find(|child| child.kind() == "field_declaration")
            .expect("a field");

        let chain = [root, item];
        let walk = Ancestors::known(&chain);
        let under = walk.of_children(&body);
        assert_eq!(
            view(&under),
            (at(&chain), vec![body.id()]),
            "the walk's chain is borrowed in place and only the body is owned"
        );
        let nested = under.get().of_children(&field);
        assert_eq!(
            view(&nested),
            (at(&chain), vec![body.id(), field.id()]),
            "a nested derivation still borrows the walk's chain, copying what the first added"
        );

        let empty: [Node; 0] = [];
        let at_root = Ancestors::known(&empty).of_children(&root);
        assert_eq!(view(&at_root), (at(&empty), vec![root.id()]));
        assert_eq!(at_root.get().parent(&item).map(|p| p.id()), Some(root.id()));
        let leaf = item.child(0).expect("the `struct` keyword");
        assert_eq!(leaf.child_count(), 0, "the keyword is a leaf");
        let under_leaf = walk.of_children(&leaf);
        assert_eq!(view(&under_leaf), (at(&chain), vec![leaf.id()]));
    }

    /// A derived chain carries the walk's fold cache, one level down and
    /// two — its node memos, its folds and the visited node's word slot —
    /// and drops the walk's sibling cache, which never recorded the
    /// nodes it describes. The drop holds on the chains `iter` yields
    /// even once they climb back into the walk's own chain, where the
    /// cache did record the parent: those siblings are sought too. The
    /// root is the one wide parent, so a lookup through the walk's own
    /// chain reads the recorded array.
    #[cfg(feature = "rust")]
    #[test]
    fn a_derived_chain_keeps_the_fold_cache_and_drops_the_sibling_cache() {
        use std::cell::Cell;
        let source = "fn a() { b(); }\n".repeat(WIDE_PARENT + 1);
        let tree = Tree::new::<crate::langs::RustCode>(source.as_bytes());
        let root = tree.get_root();
        let member = root.child(0).expect("the first fn item");
        let block = member
            .children()
            .find(|child| child.kind() == "block")
            .expect("its body");
        let statement = block.child(1).expect("the call statement");
        let mut siblings = SiblingCache::default();
        siblings.record(&root, 0, root.children());
        let folds = FoldCache::default();
        let chain = [root];
        let walk = Ancestors::for_walk(&chain, &member, &folds, &siblings);
        assert!(
            matches!(walk.lookup(&member), Lookup::Wide(_, Some(0))),
            "the walk's own chain reads the recorded root"
        );
        let question = MemoQuestion::ArgumentSlotsReadable;
        walk.node_memo(question)
            .expect("the walk lent a cache")
            .set(&member, true);
        let runs = Cell::new(0_usize);
        let classify = || {
            runs.set(runs.get() + 1);
            4
        };
        assert_eq!(
            walk.classified(WordSlot::TclCommandWord, &member, classify),
            4
        );

        let under = walk.of_children(&member);
        let nested = under.get().of_children(&block);
        for (depth, derived, child) in [(2, under.get(), block), (3, nested.get(), statement)] {
            assert_eq!(derived.depth(), Some(depth));
            assert!(derived.memoises_folds(), "at depth {depth}: folds");
            assert_eq!(
                derived
                    .node_memo(question)
                    .and_then(|memo| memo.get(&member)),
                Some(true),
                "at depth {depth}: the walk's node memo"
            );
            assert_eq!(
                derived.classified(WordSlot::TclCommandWord, &member, classify),
                4
            );
            assert!(
                derived.siblings.is_none(),
                "at depth {depth}: no sibling cache"
            );
            let (_, above) = derived
                .iter(&child)
                .find(|(ancestor, _)| ancestor.id() == member.id())
                .expect("the member is on every derived chain");
            assert_eq!(above.depth(), Some(1), "the walk's own chain, root alone");
            assert!(
                matches!(above.lookup(&member), Lookup::Narrow(Some(_))),
                "at depth {depth}: iterating back into the walk's chain regains no sibling cache"
            );
        }
        assert_eq!(
            runs.get(),
            1,
            "the visited node's word slot served every derived ask"
        );
    }

    /// `descend` / `ascend` move a held chain into a child and back
    /// (#1401): under the child, the child's children read their whole
    /// ancestry from it, one more depth down; after ascending, the
    /// body's own members do again. The descent goes through every level
    /// the tree has, the `if`'s `expression_statement` included: a chain
    /// that skipped one passed the containment check `descend` makes and
    /// the depth-and-parent assertions this made before. A holder derived
    /// from a moved holder moves the same way within a floor of its own,
    /// what it inherited plus its node, and is back on the ancestry it
    /// was built with after one ascend: a floor counted too high would
    /// refuse that pop. Without a known chain there is no chain to move,
    /// so the depth stays unknown and the answers keep climbing. The
    /// floor's debug assertions are `ascending_past_the_floor_is_caught`
    /// and `ascending_past_an_inherited_floor_is_caught`; the
    /// release-build clamp behind them runs only where those assertions
    /// are compiled out, so no test in the gate observes it.
    #[cfg(feature = "rust")]
    #[test]
    fn descend_and_ascend_move_the_held_chain_within_its_floor() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { if a { g(); } }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the fn item");
        let body = item
            .children()
            .find(|child| child.kind() == "block")
            .expect("the body");
        let statement = body
            .children()
            .find(|child| child.kind() == "expression_statement")
            .expect("the if's statement");
        let conditional = statement.child(0).expect("the if");
        let consequence = conditional
            .children()
            .find(|child| child.kind() == "block")
            .expect("the consequence");
        let call = consequence
            .children()
            .find(|child| child.kind() == "expression_statement")
            .expect("the call statement");

        let chain = [root, item];
        let mut under = Ancestors::known(&chain).of_children(&body);
        assert_eq!(under.get().depth(), Some(3), "root, item, body");

        under.descend(&statement);
        under.descend(&conditional);
        under.descend(&consequence);
        assert_describes(under.get(), &call);

        under.ascend();
        under.ascend();
        under.ascend();
        assert_eq!(under.get().depth(), Some(3), "back at the body");
        assert_describes(under.get(), &statement);

        let mut outer = Ancestors::known(&chain[..1]).of_children(&item);
        outer.descend(&body);
        let mut inner = outer.get().of_children(&statement);
        inner.descend(&conditional);
        assert_describes(inner.get(), &consequence);
        inner.ascend();
        assert_describes(inner.get(), &conditional);

        let mut unknown = Ancestors::unknown().of_children(&body);
        unknown.descend(&statement);
        assert_eq!(
            unknown.get().depth(),
            None,
            "without a chain a descent records nothing"
        );
        unknown.ascend();
        assert_eq!(
            unknown.get().parent(&call).map(|p| p.id()),
            Some(consequence.id()),
            "and the answer still climbs"
        );
    }

    /// `descend` takes the chain into a child of its last entry and
    /// nothing else: a walker that descends into a sibling, or into the
    /// node already on top, would have every later gate read a parent
    /// the node does not have. A debug build trips the containment
    /// assertion, which names the offending kind.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[should_panic(
        expected = "descended into a function_item node the chain's last entry does not contain"
    )]
    fn descending_outside_the_last_entry_is_caught() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() {}\nfn g() {}\n");
        let root = tree.get_root();
        let f = root.child(0).expect("the first fn item");
        let g = root.child(1).expect("the second fn item");
        let chain = [root];
        let mut under = Ancestors::known(&chain).of_children(&f);
        under.descend(&g);
    }

    /// `of_children` derives the children's chain of a node the chain
    /// describes and nothing else: asked of a grandchild under the root's
    /// chain — lesson 103's hazard, the derivation hoisted ahead of the
    /// test that names the body — every gate under it would read a parent
    /// the node does not have. A debug build trips the same containment
    /// check `descend` and `for_walk_chain_only` make, naming the kind;
    /// under `--cfg chain_audit` the exact parent check fires first,
    /// with its own message. Only the derivation can raise either;
    /// `Ancestors::known` checks nothing.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[cfg_attr(
        chain_audit,
        should_panic(
            expected = "derived a chain for the children of a block node that is not its own"
        )
    )]
    #[cfg_attr(
        not(chain_audit),
        should_panic(
            expected = "derived a chain for the children of a block node its last entry does not contain"
        )
    )]
    fn deriving_below_a_chain_that_is_not_the_node_s_is_caught() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() {}\nfn g() {}\n");
        let root = tree.get_root();
        let g = root.child(1).expect("the second fn item");
        let f_body = root
            .child(0)
            .expect("the first fn item")
            .children()
            .find(|child| child.kind() == "block")
            .expect("its body");
        let chain = [root, g];
        let _ = Ancestors::known(&chain).of_children(&f_body);
    }

    /// A derivation inherits at most two entries below the walk's chain
    /// (`MAX_INHERITED_RUN`): every production site derives through one
    /// level of fixed nesting, and two is the allowance for a parse
    /// error's children judged through a `then` wrapper. One derived per
    /// wrapper level from the last copies every level so far and is
    /// quadratic in the nest again (#1805); the fourth level trips the
    /// bound. The message carries the count, so only the last derivation
    /// here can raise the one expected.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[should_panic(expected = "derived from a chain 3 levels below the walk's")]
    fn deriving_once_per_level_is_caught() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { if a { g(); } }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the fn item");
        let body = item
            .children()
            .find(|child| child.kind() == "block")
            .expect("the body");
        let statement = body
            .children()
            .find(|child| child.kind() == "expression_statement")
            .expect("the if's statement");
        let conditional = statement.child(0).expect("the if");
        let chain = [root];
        let under_item = Ancestors::known(&chain).of_children(&item);
        let under_body = under_item.get().of_children(&body);
        let under_statement = under_body.get().of_children(&statement);
        let _ = under_statement.get().of_children(&conditional);
    }

    /// In a release build the floor clamps instead of asserting: an
    /// `ascend` with nothing to undo leaves the body's chain in place, so
    /// a consumer that over-ascends still reads the ancestry of the
    /// body's children, never of the body's siblings, and every later
    /// `descend` / `ascend` pair still balances on it. The no-pop outcome
    /// at the floor is reachable only where the debug assertion is
    /// compiled out, so `cargo test --release` is what runs this; the two
    /// `ascending_past_*` tests are its debug-build counterpart, and the
    /// coverage job, a debug run, cannot see this branch by construction.
    #[cfg(feature = "rust")]
    #[cfg(not(debug_assertions))]
    #[test]
    fn ascending_past_the_floor_clamps_in_release() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { if a { g(); } }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the fn item");
        let body = item
            .children()
            .find(|child| child.kind() == "block")
            .expect("the body");
        let statement = body
            .children()
            .find(|child| child.kind() == "expression_statement")
            .expect("the if's statement");

        let chain = [root, item];
        let mut under = Ancestors::known(&chain).of_children(&body);
        under.ascend();
        under.ascend();
        assert_eq!(under.get().depth(), Some(3), "clamped at the body");
        assert_describes(under.get(), &statement);

        under.descend(&statement);
        assert_eq!(under.get().depth(), Some(4), "and still moves");
        under.ascend();
        assert_describes(under.get(), &statement);

        let mut outer = Ancestors::known(&chain[..1]).of_children(&item);
        outer.descend(&body);
        let mut inner = outer.get().of_children(&statement);
        inner.ascend();
        assert_describes(inner.get(), &statement.child(0).expect("the if"));
    }

    /// The floor is the chain as built: an `ascend` with nothing to undo
    /// leaves the body's chain in place rather than popping the body —
    /// a debug build trips the assertion, which is what this pins on a
    /// holder derived from the walk's own chain, the shape every member
    /// walk holds.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[should_panic(expected = "ascended above the node this chain was built for")]
    fn ascending_past_the_floor_is_caught() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() {}\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the fn item");
        let chain = [root];
        let mut under = Ancestors::known(&chain).of_children(&item);
        under.ascend();
    }

    /// A holder derived from a moved holder has a floor of its own: what
    /// it inherited plus its node. At that floor one ascend trips the
    /// assertion rather than popping into the levels it inherited — a
    /// floor of one entry, its node alone, would let that ascend take its
    /// node and the next answer come from the wrong parent. That it moves
    /// within the floor is `descend_and_ascend_move_the_held_chain_within_its_floor`,
    /// so the only ascend here is the one that must trip.
    #[cfg(all(feature = "rust", debug_assertions))]
    #[test]
    #[should_panic(expected = "ascended above the node this chain was built for")]
    fn ascending_past_an_inherited_floor_is_caught() {
        let tree = Tree::new::<crate::langs::RustCode>(b"fn f() { if a { g(); } }\n");
        let root = tree.get_root();
        let item = root.child(0).expect("the fn item");
        let body = item
            .children()
            .find(|child| child.kind() == "block")
            .expect("the body");
        let statement = body
            .children()
            .find(|child| child.kind() == "expression_statement")
            .expect("the if's statement");

        let chain = [root];
        let mut outer = Ancestors::known(&chain).of_children(&item);
        outer.descend(&body);
        let mut inner = outer.get().of_children(&statement);
        inner.ascend();
    }
}
