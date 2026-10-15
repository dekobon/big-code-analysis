//! [`Ancestors`], the chain of a node's ancestors a walker threads down
//! to its predicates, and the [`SiblingCache`] it can lend with it.

use super::Node;

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
#[derive(Clone, Copy, Debug)]
pub struct Ancestors<'tree, 'chain> {
    chain: Option<&'chain [Node<'tree>]>,
    siblings: Option<&'chain SiblingCache<'tree>>,
}

impl<'tree, 'chain> Ancestors<'tree, 'chain> {
    /// No chain is available; every query climbs with [`Node::parent`].
    #[inline]
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            chain: None,
            siblings: None,
        }
    }

    /// `chain` lists every ancestor of the node about to be queried,
    /// root first — so `chain.last()` is its parent and an empty chain
    /// means the node is the root.
    #[inline]
    #[must_use]
    pub const fn known(chain: &'chain [Node<'tree>]) -> Self {
        Self {
            chain: Some(chain),
            siblings: None,
        }
    }

    /// This chain, answering [`siblings_after`](Self::siblings_after) and
    /// [`siblings_before`](Self::siblings_before) through `cache`, which
    /// the walker that keeps the chain owns for the whole walk.
    #[inline]
    #[must_use]
    pub const fn with_siblings(self, cache: &'chain SiblingCache<'tree>) -> Self {
        Self {
            siblings: Some(cache),
            ..self
        }
    }

    /// [`Ancestors::known`], but first checks that `chain` really is
    /// `node`'s ancestry.
    ///
    /// The parity test below proves the walker's truncate/push rule on a
    /// *replica* walker, so it cannot see `metrics_inner` itself
    /// desynchronising — a `chain.push` moved ahead of the per-node
    /// computes, say, or a `continue` inserted above the truncate.
    /// [`Ancestors::parent`] trusts `chain.last()` unvalidated, so such a
    /// drift would feed every predicate a wrong ancestor silently rather
    /// than fail. Walkers that maintain a chain should construct through
    /// here; [`Ancestors::known`] stays unchecked for the callers that
    /// deliberately pair a chain with a foreign node.
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
    #[must_use]
    pub fn checked(chain: &'chain [Node<'tree>], node: &Node<'tree>) -> Self {
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
        Self::known(chain)
    }

    /// How far the node this chain describes sits from the root, or
    /// `None` when no chain is known — deriving it then would cost the
    /// [`Node::parent`] climb the chain exists to remove.
    #[inline]
    pub fn depth(self) -> Option<usize> {
        self.chain.map(<[Node<'tree>]>::len)
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
    /// The compound-leaf guard of `.claude/rules/grammar-dispatch.md`
    /// section 5 asks this and nothing else: a delimiter or keyword
    /// token is suppressed *only* directly under the construct that
    /// owns it, never under an arbitrary ancestor. Eleven dispatch
    /// arms across `getter`, `checker` and the metric walkers spelled
    /// it out as `.parent(node).is_some_and(|p| p.kind_id() == X as
    /// u16)`, which wraps onto four rustfmt lines inside a match guard
    /// and buries the question under the plumbing; #1314 added six more
    /// and folded all seventeen onto this. Three sites spelling their
    /// binding `|parent|` were left alone and would each fit:
    /// `impl_is_else_if_parent_clause!` (`src/checker.rs`),
    /// `is_stabby_lambda_body` (`src/checker/ruby.rs`) and
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

    /// `node`'s ancestors, nearest first, each paired with *its* own
    /// ancestry so a predicate applied to an ancestor stays as cheap as
    /// one applied to `node`.
    #[inline]
    #[must_use]
    pub fn iter(self, node: &Node<'tree>) -> AncestorIter<'tree, 'chain> {
        match self.chain {
            Some(chain) => AncestorIter::Chain(chain, self.siblings),
            None => AncestorIter::Climb(node.parent()),
        }
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
    fn lookup(self, node: &Node<'tree>) -> Lookup<'tree, 'chain> {
        let parent = self.parent(node);
        let recorded = match (self.siblings, self.chain, parent) {
            (Some(cache), Some(chain), Some(parent)) => {
                let recorded = cache.children(&parent, chain.len() - 1);
                // A walk that stopped recording would answer the same,
                // only quadratically, which no metric value shows.
                debug_assert!(
                    recorded.is_some() || parent.child_count() <= WIDE_PARENT,
                    "a walk lending a SiblingCache must record every wide node on its \
                     chain (#1584), but not this {}",
                    parent.kind()
                );
                recorded
            }
            _ => None,
        };
        match recorded {
            Some(children) => Lookup::Wide(children, position_by_start(children, node)),
            None => Lookup::Narrow(parent),
        }
    }
}

enum Lookup<'tree, 'chain> {
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
fn position_by_start(children: &[Node], node: &Node) -> Option<usize> {
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
#[derive(Debug, Default)]
pub struct SiblingCache<'tree>(Vec<(usize, Vec<Node<'tree>>)>);

impl<'tree> SiblingCache<'tree> {
    /// Records `children`, in order, as the children of `node`, which
    /// sits at `depth` in the walk's chain. A node of no more than a
    /// short scan's children is not recorded.
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
    }

    // `parent`'s children, when the walk recorded them at `depth`.
    fn children(&self, parent: &Node<'tree>, depth: usize) -> Option<&[Node<'tree>]> {
        self.0
            .get(depth)
            .filter(|(id, _)| *id == parent.id())
            .map(|(_, children)| children.as_slice())
    }
}

/// Ancestor iterator returned by [`Ancestors::iter`], nearest first.
pub enum AncestorIter<'tree, 'chain> {
    /// The not-yet-yielded prefix of a known chain, and the walk's
    /// [`SiblingCache`] if it lent one. The prefix's last element is
    /// the next ancestor, and the prefix before it is that ancestor's
    /// own chain — so splitting from the back hands out both at once.
    Chain(&'chain [Node<'tree>], Option<&'chain SiblingCache<'tree>>),
    /// The next ancestor to yield, reached by climbing.
    Climb(Option<Node<'tree>>),
}

impl<'tree, 'chain> Iterator for AncestorIter<'tree, 'chain> {
    type Item = (Node<'tree>, Ancestors<'tree, 'chain>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Chain(remaining, siblings) => {
                let (&nearest, above) = remaining.split_last()?;
                *remaining = above;
                let ancestors = Ancestors {
                    chain: Some(above),
                    siblings: *siblings,
                };
                Some((nearest, ancestors))
            }
            Self::Climb(next) => {
                let nearest = (*next)?;
                *next = nearest.parent();
                Some((nearest, Ancestors::unknown()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Tree;

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
        assert!(cache.0.is_empty(), "a narrow node must not be recorded");

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
}
