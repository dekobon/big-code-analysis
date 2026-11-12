//! The chain an [`Ancestors`](super::Ancestors) reads, as two runs: the
//! walk's own chain, borrowed, and the entries a
//! [`ChildAncestors`](super::ChildAncestors) adds below it.
//!
//! A chain derived for a container's children used to be a copy of the
//! walk's chain plus the container: `O(depth)` per container, and every
//! member walk derives one per container, so `n` nested containers cost
//! `Σ depth`, quadratic in the nest (#1805). The walk's chain does not
//! change while a predicate holds a derived one — it is truncated before
//! and pushed after the per-node computes — so a derived chain borrows
//! it and owns only what it adds.

use std::ops::Index;

use crate::node::Node;

/// An ancestor chain, root first, in two runs read as one: `upper`, the
/// walk's chain, and `lower`, the entries a derived chain adds below it
/// (empty on a flat chain). Every depth is the one a flat copy would
/// give, which is what the [`FoldCache`](super::FoldCache) slots are
/// keyed on, so a node reached through a derived chain uses the fold
/// slots it uses through the walk's own. A derived chain carries no
/// [`SiblingCache`](super::SiblingCache): the walk never recorded the
/// nodes below its own chain.
#[derive(Clone, Copy, Debug)]
pub(super) struct Chain<'tree, 'chain> {
    pub(super) upper: &'chain [Node<'tree>],
    pub(super) lower: &'chain [Node<'tree>],
}

impl<'tree, 'chain> Chain<'tree, 'chain> {
    /// `nodes` as one run.
    pub(super) const fn flat(nodes: &'chain [Node<'tree>]) -> Self {
        Self {
            upper: nodes,
            lower: &[],
        }
    }

    #[inline]
    pub(super) const fn len(self) -> usize {
        self.upper.len() + self.lower.len()
    }

    #[inline]
    pub(super) fn get(self, depth: usize) -> Option<&'chain Node<'tree>> {
        match depth.checked_sub(self.upper.len()) {
            Some(below) => self.lower.get(below),
            None => self.upper.get(depth),
        }
    }

    #[inline]
    pub(super) fn last(self) -> Option<&'chain Node<'tree>> {
        self.lower.last().or_else(|| self.upper.last())
    }

    /// The entries above `depth`: the chain of the entry there, as
    /// `&chain[..depth]` is on a slice, and panicking where that does.
    #[inline]
    pub(super) fn above(self, depth: usize) -> Self {
        match depth.checked_sub(self.upper.len()) {
            Some(below) => Self {
                upper: self.upper,
                lower: &self.lower[..below],
            },
            None => Self::flat(&self.upper[..depth]),
        }
    }

    /// The last entry and the chain above it, as [`slice::split_last`].
    #[inline]
    pub(super) fn split_last(self) -> Option<(&'chain Node<'tree>, Self)> {
        self.lower
            .split_last()
            .map(|(last, lower)| (last, Self { lower, ..self }))
            .or_else(|| {
                self.upper
                    .split_last()
                    .map(|(last, upper)| (last, Self::flat(upper)))
            })
    }
}

impl<'tree> Index<usize> for Chain<'tree, '_> {
    type Output = Node<'tree>;

    /// The entry at `depth`, panicking out of bounds as a slice does.
    #[inline]
    fn index(&self, depth: usize) -> &Node<'tree> {
        match depth.checked_sub(self.upper.len()) {
            Some(below) => &self.lower[below],
            None => &self.upper[depth],
        }
    }
}

/// A [`Chain`] whose lower run is owned and can move: what a
/// [`ChildAncestors`](super::ChildAncestors) holds.
///
/// Built from a chain and the node whose children it describes, it
/// borrows that chain's upper run and copies only its lower one, so it
/// costs `O(1)` from a walk's flat chain and `O(lower)` from a derived
/// one, which every caller bounds by a fixed structural nesting (see
/// [`Ancestors::of_children`](super::Ancestors::of_children)). A descent
/// that is not bounded moves one held chain with
/// [`descend`](Self::descend) and [`ascend`](Self::ascend) instead of
/// deriving per level.
#[derive(Debug)]
pub(super) struct HeldChain<'tree, 'chain> {
    upper: &'chain [Node<'tree>],
    /// Never empty: [`below`](Self::below) seeds it with the node and
    /// [`ascend`](Self::ascend) stops at `floor >= 1`, so
    /// [`descend`](Self::descend) reads `lower.last()` as the chain's
    /// last entry.
    lower: Vec<Node<'tree>>,
    /// `lower`'s length as built: the inherited lower run plus the
    /// node. [`ascend`](Self::ascend) never pops below it, so a holder
    /// derived from a derived chain never ascends into the ancestry it
    /// inherited.
    floor: usize,
}

/// The longest lower run a derivation may inherit. Every production
/// site derives through one level of fixed structural nesting (a struct
/// to its field list, a module to its `do` block, a `then` wrapper to
/// its statement); two allows a parse error's children judged through a
/// `then` wrapper, should a language with a doc binding gain Ruby's
/// wrapper shape. A derivation copies the run it inherits, so a site that
/// derived once per wrapper level would be quadratic in the nest again
/// (#1805); it moves one held chain with [`HeldChain::descend`] /
/// [`ascend`](HeldChain::ascend) instead.
const MAX_INHERITED_RUN: usize = 2;

impl<'tree, 'chain> HeldChain<'tree, 'chain> {
    /// `chain` extended by `node`, which `chain.last()` must contain:
    /// the same `O(1)` consequence of "`chain.last()` is `node`'s parent"
    /// that `Ancestors::for_walk_chain_only` asks of a walk's chain, and
    /// the exact question under `--cfg chain_audit`.
    pub(super) fn below(chain: Chain<'tree, 'chain>, node: Node<'tree>) -> Self {
        #[cfg(chain_audit)]
        assert_eq!(
            chain.last().map(Node::id),
            node.parent().map(|parent| parent.id()),
            "derived a chain for the children of a {} node that is not its own",
            node.kind()
        );
        debug_assert!(
            chain.last().is_none_or(|parent| {
                parent.id() != node.id()
                    && parent.start_byte() <= node.start_byte()
                    && node.end_byte() <= parent.end_byte()
            }),
            "derived a chain for the children of a {} node its last entry does not contain",
            node.kind()
        );
        debug_assert!(
            chain.lower.len() <= MAX_INHERITED_RUN,
            "derived from a chain {} levels below the walk's: derive once and descend, \
             not once per level (#1805)",
            chain.lower.len()
        );
        let lower = [chain.lower, std::slice::from_ref(&node)].concat();
        Self {
            upper: chain.upper,
            floor: lower.len(),
            lower,
        }
    }

    pub(super) fn view(&self) -> Chain<'tree, '_> {
        Chain {
            upper: self.upper,
            lower: &self.lower,
        }
    }

    /// Pushes `child`, which the last entry must contain.
    pub(super) fn descend(&mut self, child: &Node<'tree>) {
        debug_assert!(
            self.lower.last().is_some_and(|parent| {
                parent.id() != child.id()
                    && parent.start_byte() <= child.start_byte()
                    && child.end_byte() <= parent.end_byte()
            }),
            "descended into a {} node the chain's last entry does not contain",
            child.kind()
        );
        self.lower.push(*child);
    }

    /// Pops one [`descend`](Self::descend), never below the floor.
    pub(super) fn ascend(&mut self) {
        debug_assert!(
            self.lower.len() > self.floor,
            "ascended above the node this chain was built for"
        );
        if self.lower.len() > self.floor {
            self.lower.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Tree;

    /// Every query on a chain split between its two runs answers what the
    /// same query on the flat slice does, at every split — an empty upper
    /// run, which a root's children derive from, and an empty lower one,
    /// which every flat chain has, among them — and at every depth, one
    /// past the end included. `split_last` run to exhaustion is what
    /// `Ancestors::iter` does, so it must hand out every entry once, in
    /// order, across the seam.
    #[cfg(feature = "rust")]
    #[test]
    fn a_split_chain_reads_as_its_flat_slice() {
        let tree = Tree::new::<crate::langs::RustCode>(b"mod a { mod b { fn f() { g(1); } } }\n");
        let nodes: Vec<Node> = tree.get_root().preorder().take(7).collect();
        assert_eq!(nodes.len(), 7, "the fixture holds seven nodes");
        let ids = |nodes: &[Node]| nodes.iter().map(Node::id).collect::<Vec<_>>();
        let unsplit = |chain: Chain| {
            let mut entries = Vec::new();
            let mut rest = chain;
            while let Some((last, above)) = rest.split_last() {
                entries.push(last.id());
                rest = above;
            }
            entries.reverse();
            entries
        };
        for split in 0..=nodes.len() {
            let (upper, lower) = nodes.split_at(split);
            let chain = Chain { upper, lower };
            assert_eq!(chain.len(), nodes.len(), "split {split}");
            assert_eq!(chain.last().map(Node::id), nodes.last().map(Node::id));
            assert_eq!(unsplit(chain), ids(&nodes), "split {split}");
            for depth in 0..=nodes.len() {
                assert_eq!(
                    chain.get(depth).map(Node::id),
                    nodes.get(depth).map(Node::id),
                    "split {split}, depth {depth}"
                );
                if let Some(node) = nodes.get(depth) {
                    assert_eq!(chain[depth].id(), node.id(), "split {split}, depth {depth}");
                }
                assert_eq!(
                    unsplit(chain.above(depth)),
                    ids(&nodes[..depth]),
                    "split {split}, above {depth}"
                );
            }
        }
    }

    // Seven nodes of the parity fixture, split four and three so a depth
    // past the end lands in the non-empty lower run, the arm a clamp
    // would hide in.
    #[cfg(feature = "rust")]
    fn seven_nodes(tree: &Tree) -> Vec<Node<'_>> {
        let nodes: Vec<Node> = tree.get_root().preorder().take(7).collect();
        assert_eq!(nodes.len(), 7, "the fixture holds seven nodes");
        nodes
    }

    #[cfg(feature = "rust")]
    const SPLIT_FIXTURE: &[u8] = b"mod a { mod b { fn f() { g(1); } } }\n";

    /// `above` panics exactly where `&chain[..depth]` does: one past the
    /// whole chain. The parity test reaches `above(len)`, the whole
    /// chain, which a slice permits; a lower arm that clamped would pass
    /// it and fail here. The setup cannot raise the expected message.
    #[cfg(feature = "rust")]
    #[test]
    #[should_panic(expected = "range end index 4 out of range for slice of length 3")]
    fn above_past_the_end_panics_as_a_slice_does() {
        let tree = Tree::new::<crate::langs::RustCode>(SPLIT_FIXTURE);
        let nodes = seven_nodes(&tree);
        let (upper, lower) = nodes.split_at(4);
        let _ = Chain { upper, lower }.above(nodes.len() + 1);
    }

    /// `Index` panics at `len`, as a slice does, on the same split.
    #[cfg(feature = "rust")]
    #[test]
    #[should_panic(expected = "index out of bounds: the len is 3 but the index is 3")]
    fn index_at_the_end_panics_as_a_slice_does() {
        let tree = Tree::new::<crate::langs::RustCode>(SPLIT_FIXTURE);
        let nodes = seven_nodes(&tree);
        let (upper, lower) = nodes.split_at(4);
        let _ = &Chain { upper, lower }[nodes.len()];
    }
}
