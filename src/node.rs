// bca: suppress-file(halstead, nargs, nexits, nom, wmc)
// AST `Node` wrapper plus per-language node-kind dispatch helpers; the
// offenders are arm-count and many-fn aggregation artifacts, not
// per-function logic complexity.

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

use tree_sitter::Node as OtherNode;
use tree_sitter::Tree as OtherTree;
use tree_sitter::{Parser, TreeCursor};

use crate::checker::Checker;
use crate::traits::{LanguageInfo, Search};

/// A parsed source tree wrapping a [`tree_sitter::Tree`].
///
/// The "open parse seam" (see issue #251) is reached by external
/// callers through [`crate::Ast::from_tree_sitter`], which accepts a
/// caller-built `tree_sitter::Tree` directly; this wrapper stays
/// internal so the metric walker is the only thing that observes it.
#[derive(Clone, Debug)]
pub(crate) struct Tree(OtherTree);

impl Tree {
    pub(crate) fn new<T: LanguageInfo>(code: &[u8]) -> Self {
        let mut parser = Parser::new();
        // `Tree::new::<T>` is only reachable from the `mk_action!`
        // dispatchers, which themselves cfg-gate each `LANG::*` arm
        // behind the matching per-language feature (see #252). When
        // the feature is off the dispatcher returns
        // `Err(LanguageDisabled)` before we get here, so
        // `get_ts_language` is provably `Ok` at this call site.
        let language = T::lang().get_ts_language().expect(
            "invariant: dispatcher cfg-gates this call behind the per-language Cargo feature",
        );
        parser
            .set_language(&language)
            .expect("invariant: grammar version is pinned and compatible with bundled tree-sitter");

        Self(
            parser
                .parse(code, None)
                .expect("invariant: parser has a language set and no cancellation flag"),
        )
    }

    pub(crate) fn from_ts_tree(tree: OtherTree) -> Self {
        Self(tree)
    }

    pub(crate) fn get_root(&self) -> Node<'_> {
        Node(self.0.root_node())
    }

    pub(crate) fn as_ts_tree(&self) -> &OtherTree {
        &self.0
    }
}

/// An `AST` node.
///
/// The inner [`tree_sitter::Node`] is reached through
/// [`Node::as_tree_sitter`] for advanced use cases that need direct
/// access to the underlying tree-sitter API; the field itself is
/// private so a `tree-sitter` version bump cannot silently reshape
/// this struct's public layout.
#[derive(Clone, Copy, Debug)]
pub struct Node<'a>(OtherNode<'a>);

impl<'a> Node<'a> {
    /// Returns the underlying [`tree_sitter::Node`] for callers that
    /// want to drive their own traversal alongside the metric walker.
    ///
    /// `tree_sitter::Node` is [`Copy`], so the node is returned by
    /// value. The returned node borrows the same source-tree lifetime
    /// as `self`.
    ///
    /// The `tree-sitter` re-export this exposes is *value-not-stable*:
    /// the underlying pin may bump in any minor release, so node shape
    /// and node-kind ids are not part of this crate's stability
    /// contract (see the [`tree_sitter`](crate::tree_sitter) re-export
    /// note in the crate root).
    #[must_use]
    #[inline]
    pub fn as_tree_sitter(&self) -> OtherNode<'a> {
        self.0
    }

    /// Checks if a node represents a syntax error or contains any syntax errors
    /// anywhere within it.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.0.has_error()
    }

    pub(crate) fn id(&self) -> usize {
        self.0.id()
    }

    pub(crate) fn kind(&self) -> &'static str {
        self.0.kind()
    }

    pub(crate) fn kind_id(&self) -> u16 {
        self.0.kind_id()
    }

    pub(crate) fn utf8_text(&self, data: &'a [u8]) -> Option<&'a str> {
        self.0.utf8_text(data).ok()
    }

    pub(crate) fn start_byte(&self) -> usize {
        self.0.start_byte()
    }

    pub(crate) fn end_byte(&self) -> usize {
        self.0.end_byte()
    }

    pub(crate) fn start_position(&self) -> (usize, usize) {
        let temp = self.0.start_position();
        (temp.row, temp.column)
    }

    pub(crate) fn end_position(&self) -> (usize, usize) {
        let temp = self.0.end_position();
        (temp.row, temp.column)
    }

    pub(crate) fn start_row(&self) -> usize {
        self.0.start_position().row
    }

    pub(crate) fn end_row(&self) -> usize {
        self.0.end_position().row
    }

    pub(crate) fn parent(&self) -> Option<Node<'a>> {
        self.0.parent().map(Node)
    }

    /// Returns `true` if this node's parent has any direct child with
    /// the given grammar `kind_id` (the parent's children include this
    /// node itself, so a self-match counts). Delegates to [`wraps_any`]
    /// on the parent so the scan reuses the allocation-free `child(0)` +
    /// `next_sibling()` walk rather than `children(&mut parent.walk())`,
    /// which heap-allocates a `TreeCursor` per call. This sits on the
    /// JS/TS arrow-function closure-classification hot path
    /// (`check_if_arrow_func!`), the same path #217 optimized for
    /// `wraps_any` / `is_child` but missed here. See #521.
    ///
    /// [`wraps_any`]: Self::wraps_any
    #[inline]
    pub(crate) fn has_sibling(&self, id: u16) -> bool {
        self.0
            .parent()
            .is_some_and(|parent| Node(parent).is_child(id))
    }

    pub(crate) fn previous_sibling(&self) -> Option<Node<'a>> {
        self.0.prev_sibling().map(Node)
    }

    /// Returns `true` if any direct child has the given grammar
    /// `kind_id`. Walks via `child(0)` + `next_sibling()` instead of
    /// `children(&mut self.0.walk())` so the implementation avoids
    /// the per-call `TreeCursor` heap allocation that the iterator
    /// form requires. Each `next_sibling()` is O(1) (tree-sitter
    /// stores siblings as a linked list), so total cost is O(n)
    /// without cursor overhead. See #217 for the motivating perf
    /// finding from the JS/TS template-literal hot path.
    #[inline]
    pub(crate) fn is_child(&self, id: u16) -> bool {
        self.wraps_any(&[id])
    }

    /// Returns `true` if any direct child matches one of the given
    /// grammar `kind_id`s. The single-id [`is_child`] delegates here, so
    /// both share one allocation-free sibling walk (the `#[inline]` makes
    /// the single-element `contains` collapse to an equality check — the
    /// #217 hot-path optimization is preserved). Generalizing the check to
    /// a set lets the shared string-interpolation operand skip declare its
    /// rule once (issue #420).
    ///
    /// [`is_child`]: Self::is_child
    #[inline]
    pub(crate) fn wraps_any(&self, ids: &[u16]) -> bool {
        let mut cur = self.0.child(0);
        while let Some(c) = cur {
            if ids.contains(&c.kind_id()) {
                return true;
            }
            cur = c.next_sibling();
        }
        false
    }

    pub(crate) fn child_count(&self) -> usize {
        self.0.child_count()
    }

    // Returns `true` if this node is a named grammar production
    // (as opposed to an anonymous token such as a punctuation or
    // keyword literal). Used to skip anonymous tokens like the
    // leading `|` in an or-pattern.
    pub(crate) fn is_named(&self) -> bool {
        self.0.is_named()
    }

    pub(crate) fn child_by_field_name(&self, name: &str) -> Option<Node<'_>> {
        self.0.child_by_field_name(name).map(Node)
    }

    pub(crate) fn child(&self, pos: usize) -> Option<Node<'a>> {
        self.0.child(pos as u32).map(Node)
    }

    /// Returns the tree-sitter grammar field name through which this
    /// node reaches the child at `child_index`, if any. Used by the
    /// AST builder to thread the parent's `field_name` into each child
    /// without a parallel cursor walk.
    pub(crate) fn field_name_for_child(&self, child_index: u32) -> Option<&'static str> {
        self.0.field_name_for_child(child_index)
    }

    pub(crate) fn children(&self) -> Children<'a> {
        let mut cursor = self.cursor();
        // `goto_first_child` returns false when the node has no
        // children, in which case the iterator is empty from the
        // outset. Termination is then driven entirely by the cursor
        // (see `Children::next`), so the iterator stops exactly when
        // the tree reports no further siblings — it can never pad the
        // sequence with duplicate nodes if `child_count` and the
        // cursor walk ever disagree.
        let done = !cursor.goto_first_child();
        Children {
            cursor,
            done,
            // `child_count` is the authoritative length for the
            // `ExactSizeIterator` contract; for well-formed trees it
            // equals the cursor sibling walk, so the reported length
            // and the emitted data agree. A childless node (`done`
            // already set) reports `0` so the empty iterator's length
            // matches its (lack of) data.
            remaining: if done { 0 } else { self.child_count() },
        }
    }

    pub(crate) fn cursor(&self) -> Cursor<'a> {
        Cursor(self.0.walk())
    }

    #[allow(dead_code)]
    pub(crate) fn get_parent(&self, level: usize) -> Option<Node<'a>> {
        let mut level = level;
        let mut node = *self;
        while level != 0 {
            if let Some(parent) = node.parent() {
                node = parent;
            } else {
                return None;
            }
            level -= 1;
        }

        Some(node)
    }

    pub(crate) fn count_specific_ancestors<C: Checker>(
        &self,
        check: fn(&Node) -> bool,
        stop: fn(&Node) -> bool,
    ) -> usize {
        let mut count = 0;
        let mut node = *self;
        while let Some(parent) = node.parent() {
            if stop(&parent) {
                break;
            }
            if check(&parent) && !C::is_else_if(&parent) {
                count += 1;
            }
            node = parent;
        }
        count
    }

    /// Returns `true` iff this node's parent satisfies `parent_pred`
    /// AND that parent's own parent (this node's grandparent)
    /// satisfies `grand_pred`. Returns `false` as soon as either link
    /// is absent or its predicate fails, so a misordered predicate
    /// cannot silently degrade to a single-predicate check.
    pub(crate) fn parent_grandparent_match(
        &self,
        parent_pred: fn(&Node) -> bool,
        grand_pred: fn(&Node) -> bool,
    ) -> bool {
        let Some(parent) = self.parent() else {
            return false;
        };
        if !parent_pred(&parent) {
            return false;
        }
        let Some(grand) = parent.parent() else {
            return false;
        };
        grand_pred(&grand)
    }
}

/// An `AST` cursor.
#[derive(Clone)]
pub(crate) struct Cursor<'a>(TreeCursor<'a>);

impl<'a> Cursor<'a> {
    pub(crate) fn reset(&mut self, node: &Node<'a>) {
        self.0.reset(node.0);
    }

    pub(crate) fn goto_next_sibling(&mut self) -> bool {
        self.0.goto_next_sibling()
    }

    pub(crate) fn goto_first_child(&mut self) -> bool {
        self.0.goto_first_child()
    }

    pub(crate) fn node(&self) -> Node<'a> {
        Node(self.0.node())
    }
}

/// Iterator over a node's direct children, returned by
/// [`Node::children`].
///
/// Termination is driven by the cursor alone: each step yields the
/// cursor's current node, then advances with `goto_next_sibling`,
/// stopping the moment that returns false. This makes the cursor the
/// single source of truth for both the emitted data and when to stop,
/// so the sequence can never be padded with duplicates if
/// `child_count` and the actual sibling walk disagree.
///
/// The `ExactSizeIterator` length is reported from `child_count`
/// (tracked in `remaining`). For well-formed trees the cursor walk and
/// `child_count` agree, so the advertised length matches the data
/// exactly.
pub(crate) struct Children<'a> {
    cursor: Cursor<'a>,
    done: bool,
    remaining: usize,
}

impl<'a> Iterator for Children<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = self.cursor.node();
        // The cursor is the single source of truth for termination:
        // once there is no next sibling this yield is the last one.
        self.done = !self.cursor.goto_next_sibling();
        // Keep the advertised length consistent with termination: when
        // the cursor stops, nothing remains. For well-formed trees this
        // equals `child_count - emitted`; if the cursor walk and
        // `child_count` ever disagree, this still honors the
        // `ExactSizeIterator` contract (`len() == 0` exactly at
        // exhaustion) rather than reporting a phantom remainder.
        self.remaining = if self.done {
            0
        } else {
            self.remaining.saturating_sub(1)
        };
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Children<'_> {}

impl<'a> Search<'a> for Node<'a> {
    fn first_occurrence(&self, pred: fn(u16) -> bool) -> Option<Node<'a>> {
        let mut cursor = self.cursor();
        let mut stack = Vec::new();
        let mut children = Vec::new();

        stack.push(*self);

        while let Some(node) = stack.pop() {
            if pred(node.kind_id()) {
                return Some(node);
            }
            cursor.reset(&node);
            if cursor.goto_first_child() {
                loop {
                    children.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                for child in children.drain(..).rev() {
                    stack.push(child);
                }
            }
        }

        None
    }

    fn act_on_node(&self, action: &mut dyn FnMut(&Node<'a>)) {
        let mut cursor = self.cursor();
        let mut stack = Vec::new();
        let mut children = Vec::new();

        stack.push(*self);

        while let Some(node) = stack.pop() {
            action(&node);
            cursor.reset(&node);
            if cursor.goto_first_child() {
                loop {
                    children.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                for child in children.drain(..).rev() {
                    stack.push(child);
                }
            }
        }
    }

    fn first_child(&self, pred: fn(u16) -> bool) -> Option<Node<'a>> {
        self.children().find(|&child| pred(child.kind_id()))
    }

    fn act_on_child(&self, action: &mut dyn FnMut(&Node<'a>)) {
        for child in self.children() {
            action(&child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langs::MozjsCode;

    /// The cursor-free [`Node::has_sibling`] (issue #521) must yield the
    /// exact same result as the original `parent.children(&mut
    /// parent.walk()).any(...)` form for every node and every kind in a
    /// real tree: same child set (named + anonymous), same order, same
    /// short-circuit. Comparing against the literal old logic node-by-node
    /// proves equivalence without hardcoding grammar `kind_id`s.
    fn old_has_sibling(node: OtherNode, id: u16) -> bool {
        node.parent().is_some_and(|parent| {
            parent
                .children(&mut parent.walk())
                .any(|child| child.kind_id() == id)
        })
    }

    #[test]
    fn has_sibling_matches_cursor_iterator_form() {
        // Arrow functions exercise the `check_if_arrow_func!` call site
        // that motivated #521 (PropertyIdentifier siblings on the JS/TS
        // closure-classification hot path).
        let code = b"const o = { m: (a) => a + 1, n: function () {} }; foo.bar();";
        let tree = Tree::new::<MozjsCode>(code);
        let ts_tree = tree.as_ts_tree();

        // Collect the grammar kinds that actually occur, so the
        // equivalence check covers present-sibling (true) cases.
        let mut kinds = std::collections::BTreeSet::new();
        let mut stack = vec![ts_tree.root_node()];
        while let Some(n) = stack.pop() {
            kinds.insert(n.kind_id());
            let mut child = n.child(0);
            while let Some(c) = child {
                stack.push(c);
                child = c.next_sibling();
            }
        }
        // Include an id that does not occur anywhere for absent-sibling
        // (false) coverage.
        let absent_id = u16::MAX;

        let mut stack = vec![ts_tree.root_node()];
        while let Some(n) = stack.pop() {
            let wrapped = Node(n);
            for &id in kinds.iter().chain(std::iter::once(&absent_id)) {
                assert_eq!(
                    wrapped.has_sibling(id),
                    old_has_sibling(n, id),
                    "has_sibling diverged from cursor-iterator form at node kind {} for id {id}",
                    n.kind(),
                );
            }
            let mut child = n.child(0);
            while let Some(c) = child {
                stack.push(c);
                child = c.next_sibling();
            }
        }

        // No-parent node (root) always reports no sibling.
        let root = Node(ts_tree.root_node());
        assert!(!root.has_sibling(absent_id));
        for &id in &kinds {
            assert!(
                !root.has_sibling(id),
                "root node has no parent → no sibling"
            );
        }
    }

    /// `children()` must yield exactly the node's direct children, in
    /// order, for every node in a real tree — including the empty
    /// (leaf) and single-child cases. Termination is cursor-driven, so
    /// the emitted set is compared node-by-node against the raw
    /// tree-sitter `child(i)` walk (the ground truth for both order and
    /// count). This pins the no-duplicate-padding property: a desync
    /// between `child_count` and the cursor walk would surface here as
    /// extra trailing duplicates or a length mismatch.
    #[test]
    fn children_matches_tree_sitter_child_walk() {
        // Mix of leaf nodes (no children), single-child wrappers, and
        // multi-child constructs to cover all arities.
        let code = b"const o = { m: (a) => a + 1 }; foo(); ;";
        let tree = Tree::new::<MozjsCode>(code);
        let ts_tree = tree.as_ts_tree();

        let mut stack = vec![ts_tree.root_node()];
        while let Some(n) = stack.pop() {
            let wrapped = Node(n);

            // Ground truth: walk children by index off the raw node.
            let expected: Vec<_> = (0..n.child_count() as u32)
                .filter_map(|i| n.child(i))
                .map(|c| (c.id(), c.kind_id()))
                .collect();

            let mut iter = wrapped.children();
            // ExactSizeIterator length must equal the child count up
            // front and stay exact as the iterator is consumed.
            assert_eq!(
                iter.len(),
                expected.len(),
                "children().len() disagreed with child_count at kind {}",
                n.kind(),
            );

            let mut actual = Vec::new();
            let mut remaining = expected.len();
            while let Some(child) = iter.next() {
                remaining -= 1;
                assert_eq!(
                    iter.len(),
                    remaining,
                    "size_hint drifted mid-iteration at kind {}",
                    n.kind(),
                );
                actual.push((child.id(), child.kind_id()));
            }
            assert_eq!(iter.len(), 0, "iterator not drained to zero len");
            assert_eq!(
                actual,
                expected,
                "children() diverged from child(i) walk at kind {}",
                n.kind(),
            );

            for i in 0..n.child_count() as u32 {
                if let Some(c) = n.child(i) {
                    stack.push(c);
                }
            }
        }
    }

    /// `Node::as_tree_sitter` (issue #556) must hand back the *same*
    /// underlying `tree_sitter::Node` the wrapper holds: identical
    /// `kind()` / `kind_id()` and a usable tree-sitter API. Obtaining
    /// the wrapper through the public `CppParser` + `ParserTrait::root`
    /// path (rather than the in-module `Tree::new`) proves the accessor
    /// is the public seam that replaced the former `pub` `.0` field.
    #[test]
    fn as_tree_sitter_round_trips_wrapper_kind() {
        use crate::{CppParser, ParserTrait};
        use std::path::Path;

        let source = b"int main() { return 0; }";
        let parser = CppParser::new(source.to_vec(), Path::new("example.cpp"), None);
        let root = parser.root();

        let ts_root = root.as_tree_sitter();

        // A well-formed C++ translation unit roots at `translation_unit`.
        assert_eq!(ts_root.kind(), "translation_unit");
        // The accessor must agree with the wrapper's own kind views.
        assert_eq!(ts_root.kind(), root.kind());
        assert_eq!(ts_root.kind_id(), root.kind_id());
        // The returned node is usable as a tree-sitter node, not a copy
        // that has lost its tree linkage: the parse is error-free and
        // the root has children.
        assert!(!ts_root.has_error());
        assert!(ts_root.child_count() > 0);
    }
}
