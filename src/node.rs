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
/// callers through [`crate::Parser::from_tree`] or
/// [`crate::metrics_from_tree`], which accept a caller-built
/// `tree_sitter::Tree` directly; this wrapper stays internal so
/// the metric walker is the only thing that observes it.
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
        let language = T::get_lang().get_ts_language().expect(
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
/// The inner `tree_sitter::Node` is exposed for advanced use cases
/// where direct access to the underlying tree-sitter API is needed.
#[derive(Clone, Copy, Debug)]
pub struct Node<'a>(pub OtherNode<'a>);

impl<'a> Node<'a> {
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

    #[inline]
    pub(crate) fn has_sibling(&self, id: u16) -> bool {
        self.0.parent().is_some_and(|parent| {
            parent
                .children(&mut parent.walk())
                .any(|child| child.kind_id() == id)
        })
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
        let mut cur = self.0.child(0);
        while let Some(c) = cur {
            if c.kind_id() == id {
                return true;
            }
            cur = c.next_sibling();
        }
        false
    }

    /// Returns `true` if any direct child matches one of the given
    /// grammar `kind_id`s. Generalizes [`is_child`] to a set so the
    /// shared string-interpolation operand skip can declare its rule
    /// once (issue #420); shares the same allocation-free sibling walk.
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

    pub(crate) fn children(&self) -> impl ExactSizeIterator<Item = Node<'a>> + use<'a> {
        let mut cursor = self.cursor();
        cursor.goto_first_child();
        (0..self.child_count()).map(move |_| {
            let result = cursor.node();
            cursor.goto_next_sibling();
            result
        })
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

    pub(crate) fn count_specific_ancestors<T: crate::ParserTrait>(
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
            if check(&parent) && !T::Checker::is_else_if(&parent) {
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
pub struct Cursor<'a>(TreeCursor<'a>);

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
