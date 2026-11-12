//! Sibling lookups on an [`Ancestors`] chain, and the [`SiblingCache`] a
//! walk records its wide nodes' children in, so that a lookup under one
//! is a search rather than a seek from the parent's first child (#1584).

use super::Ancestors;
use crate::node::Node;

impl<'tree, 'chain> Ancestors<'tree, 'chain> {
    /// Whether the sibling immediately before `node` has `kind`: the
    /// `ElseIf` gate of the grammars that spell `else if` as a bare
    /// `else` token followed by a nested `if` with no clause wrapper
    /// (Java, Groovy, C#, Kotlin — #1697), through [`previous_sibling`]
    /// and so at its cost. `false` when `node` is its parent's first
    /// child or has no parent: a root's `if` continues nothing.
    ///
    /// [`previous_sibling`]: Self::previous_sibling
    #[inline]
    #[must_use]
    pub(crate) fn previous_sibling_has_kind(self, node: &Node<'tree>, kind: u16) -> bool {
        self.previous_sibling(node)
            .is_some_and(|prev| prev.kind_id() == kind)
    }

    /// The sibling immediately before `node`, or `None` when `node` is
    /// its parent's first child or has no parent.
    ///
    /// `tree_sitter`'s own `prev_sibling` resolves the parent first
    /// (`ts_node__prev_sibling` opens with `ts_node_parent`), so it
    /// carries the same `O(depth)` cost [`Node::parent`] does. With a
    /// known chain the parent is free and what remains is a cursor walk
    /// over the siblings — or, for a wide parent the walk recorded in its
    /// [`SiblingCache`], a binary search, since a walk over every child
    /// of a Kotlin string's `$name` run was quadratic in it (#1584).
    /// A debug build rejects that cursor walk over a parent wider than
    /// [`WIDE_PARENT`], which only a chain lending no cache can reach
    /// (#1785). The metric, ops and `find` / `count` walks call this and
    /// lend one; on the suppression, dump and comment-removal walks,
    /// which record no siblings, the assertion in `lookup` fires first
    /// (#1601).
    #[inline]
    #[must_use]
    pub fn previous_sibling(self, node: &Node<'tree>) -> Option<Node<'tree>> {
        let Some(chain) = self.chain else {
            return node.previous_sibling();
        };
        let parent = chain.last()?;
        if let Lookup::Wide(children, index) = self.lookup(node) {
            return match index {
                Some(index) => children[..index].last().copied(),
                None => node.previous_sibling(),
            };
        }
        let mut previous = None;
        for child in parent.children() {
            if child.id() == node.id() {
                // The scan is `O(index)`, so only a chain that lends no
                // cache can reach it under a wide parent, and no caller
                // does (#1785). One that starts to would be quadratic
                // over the parent's children and answer the same, so
                // make it loud. Asserted once the scan has found `node`:
                // a chain that does not describe it is a different
                // caller error, the one the fallback below answers.
                debug_assert!(
                    parent.child_count() <= WIDE_PARENT,
                    "previous_sibling scanned a {}-child {} with no SiblingCache (#1785)",
                    parent.child_count(),
                    parent.kind()
                );
                return previous;
            }
            previous = Some(child);
        }
        // `node` is not among `parent.children()`, so this chain does
        // not describe `node`. Answering `None` would claim "no
        // previous sibling", which is a different (and wrong) answer;
        // fall back to the authoritative lookup instead.
        node.previous_sibling()
    }

    /// The siblings after `node`, nearest first: what
    /// [`Node::children_after`] yields on `node`'s parent, and nothing
    /// for the root. `self` is `node`'s chain.
    ///
    /// The seek [`Node::children_after`] runs is `O(index)` over a flat
    /// child array, so a walk that asks of every child of a wide
    /// `ERROR` is quadratic in it (#1584). A walk that lends a
    /// [`SiblingCache`] has already collected a wide parent's children,
    /// and the lookup is a binary search among them.
    pub fn siblings_after(
        self,
        node: &Node<'tree>,
    ) -> impl Iterator<Item = Node<'tree>> + use<'tree, 'chain> {
        let (wide, narrow) = match self.lookup(node) {
            Lookup::Wide(children, index) => (index.map(|index| &children[index + 1..]), None),
            Lookup::Narrow(parent) => (None, parent.map(|parent| parent.children_after(node))),
        };
        let wide = wide.into_iter().flatten().copied();
        wide.chain(narrow.into_iter().flatten())
    }

    /// The siblings before `node`, nearest first: what
    /// [`Node::children_before`] yields on `node`'s parent, and nothing
    /// for the root. `self` is `node`'s chain; see
    /// [`siblings_after`](Self::siblings_after).
    ///
    /// When the chain does not describe `node`, so `node` is not among the
    /// parent's children, both this and `siblings_after` yield nothing on
    /// either lookup, where [`previous_sibling`](Self::previous_sibling)
    /// falls back to [`Node::previous_sibling`]. The asymmetry is
    /// deliberate: the mismatch is a caller error the walk constructors
    /// report (#1122), and "no siblings" is the conservative answer for
    /// the `exclude_tests` prune that reads these (nothing is pruned).
    pub fn siblings_before(
        self,
        node: &Node<'tree>,
    ) -> impl Iterator<Item = Node<'tree>> + use<'tree, 'chain> {
        let (wide, narrow) = match self.lookup(node) {
            Lookup::Wide(children, index) => (index.map(|index| &children[..index]), None),
            Lookup::Narrow(parent) => (None, parent.map(|parent| parent.children_before(node))),
        };
        let wide = wide.into_iter().flatten().rev().copied();
        wide.chain(narrow.into_iter().flatten())
    }

    // Where `node`'s siblings are read from: its parent's children as the
    // walk recorded them and `node`'s index among them, or the parent to
    // seek in when the walk recorded none.
    pub(super) fn lookup(self, node: &Node<'tree>) -> Lookup<'tree, 'chain> {
        let parent = self.parent(node);
        let recorded = match (self.siblings, self.chain, parent) {
            (Some(cache), Some(chain), Some(parent)) => {
                let recorded = cache.children(&parent, chain.len() - 1);
                // A walk that stopped recording, or one that records
                // nothing and started asking, would answer the same, only
                // quadratically, which no metric value shows.
                debug_assert!(
                    recorded.is_some() || parent.child_count() <= WIDE_PARENT,
                    "a walk must record every wide node on its chain before a \
                     sibling is asked for under it (#1584), and lend that record \
                     (#1601), but not this {}-child {}",
                    parent.child_count(),
                    parent.kind()
                );
                recorded
            }
            _ => None,
        };
        if let Some(children) = recorded {
            sibling_cache_reads::record();
            return Lookup::Wide(children, position_by_start(children, node));
        }
        if self.chain.is_some() && parent.is_some() {
            sibling_seeks::record();
        }
        Lookup::Narrow(parent)
    }
}

pub(super) enum Lookup<'tree, 'chain> {
    Wide(&'chain [Node<'tree>], Option<usize>),
    Narrow(Option<Node<'tree>>),
}

/// Children past which a walk records a node's children for sibling
/// lookups. A seek over this many is a short scan however the grammar
/// stores them, so recording a narrower node would copy it for nothing.
///
/// `pub` and `#[doc(hidden)]` only so the wide-parent fixtures in this
/// crate's and `big-code-analysis`'s tests size themselves from it: a
/// hand-copied width would silently go narrow if this were raised.
#[doc(hidden)]
pub const WIDE_PARENT: usize = 64;

// `node`'s index in `children`, a parent's children in order. Their start
// bytes never decrease, so a binary search lands on the first child
// starting where `node` does; only zero-width children, which recovery
// inserts, can share that byte with it.
pub(super) fn position_by_start(children: &[Node], node: &Node) -> Option<usize> {
    let first = children.partition_point(|child| child.start_byte() < node.start_byte());
    children[first..]
        .iter()
        .take_while(|child| child.start_byte() == node.start_byte())
        .position(|child| child.id() == node.id())
        .map(|offset| first + offset)
}

/// The children of each wide node on a walk's chain, recorded as the
/// walk pushes them, so that [`Ancestors::siblings_after`] and
/// [`Ancestors::siblings_before`] find a sibling in a flat `ERROR`
/// without a scan from its start (#1584).
///
/// One slot per depth, keyed by the node's id. A walk descends into a
/// node only after pushing its children, so every wide node on the
/// chain has its slot; a slot left behind by a node the walk has moved
/// past holds another id and is not read. Recording copies what the
/// walk already holds, once per wide node.
///
/// A walk lends it through [`Ancestors::for_walk`] (#1601).
#[derive(Debug, Default)]
pub struct SiblingCache<'tree>(pub(super) Vec<(usize, Vec<Node<'tree>>)>);

/// The record a walk that records no siblings lends: empty, so a sibling
/// asked under a wide parent finds no record and trips the walk
/// assertion in [`Ancestors::lookup`], while one under a narrow parent
/// seeks as it would through no cache at all. Nothing can write to it:
/// recording takes `&mut`.
pub(super) const NOTHING_RECORDED: &SiblingCache<'static> = &SiblingCache(Vec::new());

impl<'tree> SiblingCache<'tree> {
    /// Records `children`, in order, as the children of `node`, which
    /// sits at `depth` in the walk's chain. A node of no more than a
    /// short scan's children is not recorded, and its batch is not read.
    ///
    /// `children` is every child of `node`: a lookup indexes into the
    /// record, so a batch that left one out would answer with the wrong
    /// sibling. A debug build rejects a batch of any other length.
    pub fn record(
        &mut self,
        node: &Node<'tree>,
        depth: usize,
        children: impl IntoIterator<Item = Node<'tree>>,
    ) {
        if node.child_count() <= WIDE_PARENT {
            return;
        }
        if self.0.len() <= depth {
            self.0.resize_with(depth + 1, Default::default);
        }
        // An unused slot's id is 0, which no node has: tree-sitter's id
        // is the address of the node's subtree.
        let (id, recorded) = &mut self.0[depth];
        *id = node.id();
        recorded.clear();
        recorded.extend(children);
        debug_assert_eq!(
            recorded.len(),
            node.child_count(),
            "a walk must record every child of a wide {}, in one batch (#1871)",
            node.kind()
        );
    }

    // `parent`'s children, when the walk recorded them at `depth`.
    fn children(&self, parent: &Node<'tree>, depth: usize) -> Option<&[Node<'tree>]> {
        self.0
            .get(depth)
            .filter(|(id, _)| *id == parent.id())
            .map(|(_, children)| children.as_slice())
    }
}

// Sibling lookups answered from a walk's `SiblingCache` rather than by a
// seek. The two answer identically, so only this tells a walk that lends
// the cache from one that dropped it (#1447 review); a seek on a wide
// flat parent is the quadratic #1584 fixed.
crate::observation::counter!(
    sibling_cache_reads,
    cfg(any(test, feature = "test-support"))
);

// Sibling lookups on a known chain that take the uncached path: no
// record of the parent, so the lookup seeks (or, for `previous_sibling`,
// scans) it. A walk that records no siblings must make none under a wide
// parent, which the walk assertion in `Ancestors::lookup` catches in a
// debug build; this counts them in every build and under narrow parents
// too, so a test can pin that such a walk asks for no sibling at all
// (#1601). It does not see a chainless `Ancestors::unknown()`, nor a scan
// of `parent.children()` or a cursor seek made outside `Ancestors`.
crate::observation::counter!(sibling_seeks, cfg(any(test, feature = "test-support")));
