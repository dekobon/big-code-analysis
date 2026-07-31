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

mod parser_cache;

use tree_sitter::Node as OtherNode;
use tree_sitter::Tree as OtherTree;
use tree_sitter::TreeCursor;

use crate::checker::Checker;
use crate::traits::{LanguageInfo, Search};

use parser_cache::parse_on_scratch_parser;

// Sibling lookups that resolved the parent from the node, on this
// thread.
//
// [`Node::previous_sibling`] answers exactly what the chain-based
// [`Ancestors::previous_sibling`] does, so no assertion on a metric
// value can tell a walk that uses one from a walk that uses the
// other — only the cost differs, and only on a deep tree. #1096 took
// the last of these out of the metric bodies and #1100 out of the
// `exclude_tests` prune; the counter is what makes putting one back a
// test failure rather than a silent quadratic.
crate::observation::counter!(node_resolved_sibling_lookups);

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
        // `Tree::new::<T>` is only reachable from the `mk_action!`
        // dispatchers, which themselves cfg-gate each `LANG::*` arm
        // behind the matching per-language feature (see #252). When
        // the feature is off the dispatcher returns
        // `Err(LanguageDisabled)` before we get here, so
        // `get_ts_language` is provably `Ok` at this call site.
        let language = T::lang().get_ts_language().expect(
            "invariant: dispatcher cfg-gates this call behind the per-language Cargo feature",
        );
        Self(parse_on_scratch_parser(&language, code))
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

    /// Returns this node's parent.
    ///
    /// **`O(depth)`, not `O(1)`.** tree-sitter stores no parent pointer:
    /// `ts_node_parent` restarts at the tree root and descends. A single
    /// call in a per-node metric therefore costs `O(nodes × depth)` over
    /// a walk, and an ancestor *loop* built on it costs `O(depth²)` per
    /// call.
    ///
    /// This has bitten the analyzer for real: #1052 was a per-leaf
    /// `successors(node, Node::parent)` walk in `Tokens` that made the
    /// metric `O(leaves × depth²)`, so a 2 KB file of nested parentheses
    /// took ~19 s. Prefer inheriting state downward through the
    /// traversal (see `Walk` in `spaces::compute`) over rediscovering it
    /// upward, and where a predicate genuinely needs an ancestor, take
    /// an [`Ancestors`] rather than calling this (#1084).
    ///
    /// As of #1096 no code the metric, `ops`, `bca function`, or
    /// comment-removal walks reach calls this; the remaining callers are
    /// `Ancestors` itself (the no-chain fallback), the one-off start
    /// node of a `dump`, and tests. `rg '\.parent\(\)' src/` is how
    /// to check that is still true.
    pub(crate) fn parent(&self) -> Option<Node<'a>> {
        self.0.parent().map(Node)
    }

    /// Returns `true` if this node's parent has any direct child with
    /// the given grammar `kind_id` (the parent's children include this
    /// node itself, so a self-match counts). Delegates to [`wraps_any`]
    /// on the parent. This sits on the JS/TS arrow-function
    /// closure-classification hot path (`check_if_arrow_func!`); see
    /// #521.
    ///
    /// `ancestors` supplies the parent, because [`Node::parent`] costs
    /// `O(depth)` — `tree_sitter` stores no parent pointer and resolves
    /// one by descending from the root (#1088).
    ///
    /// [`wraps_any`]: Self::wraps_any
    #[inline]
    pub(crate) fn has_sibling(&self, ancestors: Ancestors<'a, '_>, id: u16) -> bool {
        ancestors
            .parent(self)
            .is_some_and(|parent| parent.is_child(id))
    }

    /// The sibling immediately before this node.
    ///
    /// **`O(depth)`, not `O(1)`**, for [`Node::parent`]'s reason:
    /// `ts_node__prev_sibling` opens with `ts_node_parent`. Callers on a
    /// walk should use [`previous_sibling_under`] or
    /// [`Ancestors::previous_sibling`] instead (#1096).
    ///
    /// [`previous_sibling_under`]: Self::previous_sibling_under
    pub(crate) fn previous_sibling(&self) -> Option<Node<'a>> {
        node_resolved_sibling_lookups::record();
        self.0.prev_sibling().map(Node)
    }

    /// The sibling immediately before this node among `parent`'s
    /// children, or `None` when this node is `parent`'s first child.
    ///
    /// `tree_sitter`'s own `prev_sibling` resolves the parent first
    /// (`ts_node__prev_sibling` opens with `ts_node_parent`), so it
    /// carries [`Node::parent`]'s `O(depth)` cost. Callers that already
    /// hold the parent — every ABC condition walker does, because it
    /// descended from it — pay a cursor walk over the siblings instead
    /// (#1096).
    ///
    /// A one-element chain is all [`Ancestors::previous_sibling`] reads,
    /// so this delegates rather than repeating the scan — including its
    /// fallback for a node that is not among `parent`'s children, which
    /// is a caller error here but a legitimate chain/node mismatch
    /// there.
    pub(crate) fn previous_sibling_under(&self, parent: &Node<'a>) -> Option<Node<'a>> {
        Ancestors::known(std::slice::from_ref(parent)).previous_sibling(self)
    }

    /// Returns `true` if any direct child has the given grammar
    /// `kind_id`. See #217 for the motivating perf finding from the
    /// JS/TS template-literal hot path.
    #[inline]
    pub(crate) fn is_child(&self, id: u16) -> bool {
        self.wraps_any(&[id])
    }

    /// Returns `true` if any direct child matches one of the given
    /// grammar `kind_id`s. The single-id [`is_child`] delegates here, so
    /// both share one child scan (the `#[inline]` makes the
    /// single-element `contains` collapse to an equality check).
    /// Generalizing the check to a set lets the shared
    /// string-interpolation operand skip declare its rule once (issue
    /// #420).
    ///
    /// # Why a cursor rather than `child(0)` + `next_sibling()`
    ///
    /// #217 replaced the cursor walk with a `next_sibling()` chain to
    /// dodge the `TreeCursor` heap allocation, on the premise that a
    /// sibling step is `O(1)`. It is not: `ts_node_next_sibling`
    /// resolves the parent first, and `tree_sitter` stores no parent
    /// pointer — it descends from the root — so each step cost
    /// `O(depth)` and the scan `O(children × depth)`. That made every
    /// caller of this method `O(depth)` per node, which is the same
    /// defect #1084 removed from the predicates that ask for an
    /// ancestor outright. The cursor iterator is `O(children)` after one
    /// allocation, and measured faster on real input as well as on the
    /// pathological one: the `nom/nested-arrow` probe went from
    /// quadratic (17.6 s at depth 4000) to linear (6.3 ms), and a walk
    /// over the 384-file `pdf.js` corpus dropped from ~443 ms to
    /// ~370 ms (#1088).
    ///
    /// [`is_child`]: Self::is_child
    #[inline]
    pub(crate) fn wraps_any(&self, ids: &[u16]) -> bool {
        self.children().any(|c| ids.contains(&c.kind_id()))
    }

    pub(crate) fn child_count(&self) -> usize {
        self.0.child_count()
    }

    /// Number of nodes in this node's subtree, counting the node itself.
    ///
    /// `O(1)`: `tree_sitter` stores the visible-descendant count on each
    /// subtree, so this is a field read rather than a walk. It counts the
    /// same nodes the metric walk visits — visible children, named and
    /// anonymous alike — which is what makes it usable as an exact
    /// capacity for a per-node map (see `spaces::compute::metrics_inner`).
    pub(crate) fn descendant_count(&self) -> usize {
        self.0.descendant_count()
    }

    // Returns `true` if this node is a named grammar production
    // (as opposed to an anonymous token such as a punctuation or
    // keyword literal). Used to skip anonymous tokens like the
    // leading `|` in an or-pattern.
    pub(crate) fn is_named(&self) -> bool {
        self.0.is_named()
    }

    /// Returns the direct child reached through the grammar `field_name`,
    /// if any. The child carries the underlying tree lifetime `'a` (the
    /// `tree_sitter::Node` it wraps is [`Copy`] and valid for the whole
    /// tree), so callers may hold it past the borrow of `&self` — matching
    /// the sibling accessors ([`child`], [`parent`], [`children`], …) rather
    /// than over-narrowing to the method-call borrow (see issue #786).
    ///
    /// [`child`]: Self::child
    /// [`parent`]: Self::parent
    /// [`children`]: Self::children
    pub(crate) fn child_by_field_name(&self, name: &str) -> Option<Node<'a>> {
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
            node = node.parent()?;
            level -= 1;
        }

        Some(node)
    }

    /// Counts this node's ancestors satisfying `check`, walking upward
    /// from the parent and stopping at (and excluding) the first
    /// ancestor satisfying `stop`. An ancestor that is the `if` of an
    /// `else if` chain never counts — it is a continuation of the
    /// branch above it, not a new enclosing one.
    ///
    /// `ancestors` is the chain the caller descended through. Passing
    /// [`Ancestors::unknown`] is always correct and answers identically;
    /// it just pays `O(depth)` per step instead of `O(1)`.
    pub(crate) fn count_specific_ancestors<C: Checker>(
        &self,
        ancestors: Ancestors<'a, '_>,
        check: fn(&Node) -> bool,
        stop: fn(&Node) -> bool,
    ) -> usize {
        let mut count = 0;
        for (parent, above_parent) in ancestors.iter(self) {
            if stop(&parent) {
                break;
            }
            if check(&parent) && !C::is_else_if(&parent, above_parent) {
                count += 1;
            }
        }
        count
    }

    /// Returns `true` iff this node's parent satisfies `parent_pred`
    /// AND that parent's own parent (this node's grandparent)
    /// satisfies `grand_pred`. Returns `false` as soon as either link
    /// is absent or its predicate fails, so a misordered predicate
    /// cannot silently degrade to a single-predicate check.
    ///
    /// `ancestors` is the chain the caller descended through. Passing
    /// [`Ancestors::unknown`] is always correct and answers
    /// identically; it just pays [`Node::parent`]'s `O(depth)` for each
    /// of the two links, which on a per-node metric arm is quadratic in
    /// nesting depth (#1096).
    pub(crate) fn parent_grandparent_match(
        &self,
        ancestors: Ancestors<'a, '_>,
        parent_pred: fn(&Node) -> bool,
        grand_pred: fn(&Node) -> bool,
    ) -> bool {
        let mut climb = ancestors.iter(self);
        let Some((parent, _)) = climb.next() else {
            return false;
        };
        if !parent_pred(&parent) {
            return false;
        }
        let Some((grand, _)) = climb.next() else {
            return false;
        };
        grand_pred(&grand)
    }

    /// Returns a pre-order iterator over this node and all of its
    /// descendants (this node first, then each child subtree left to
    /// right).
    ///
    /// The traversal is allocation-light: it reuses one work stack and
    /// visits each node exactly once, so a full walk is O(n) in the
    /// subtree size. Every yielded [`Node`] carries the underlying tree
    /// lifetime `'a`, so callers may collect or retain the handles.
    ///
    /// This is the Rust counterpart of the Python `Node.walk()` binding
    /// (issue #728): the binding wraps each yielded node, so Rust and
    /// Python share one traversal order.
    #[must_use]
    pub fn preorder(&self) -> Preorder<'a> {
        Preorder { stack: vec![*self] }
    }

    /// Collects every node in this subtree (this node included) whose
    /// [`kind`](tree_sitter::Node::kind) is listed in `kinds`, in
    /// pre-order.
    ///
    /// Membership is an exact match against the raw grammar kind — the
    /// same unaltered vocabulary [`crate::Ast::root_node`] exposes, not
    /// the `Alterator`-curated kinds [`crate::Ast::dump`] emits. This is
    /// the Rust counterpart of the Python `Node.descendants_by_kind()`
    /// binding (issue #728).
    #[must_use]
    pub fn descendants_by_kind(&self, kinds: &[&str]) -> Vec<Node<'a>> {
        self.preorder()
            .filter(|node| kinds.contains(&node.kind()))
            .collect()
    }
}

/// The chain of a node's ancestors, root first, as recorded by a walker
/// that descended to that node.
///
/// `tree_sitter` stores no parent pointer: [`Node::parent`] restarts at
/// the tree root and descends, so it costs `O(depth)`. A predicate that
/// asks a node for its parent is therefore `O(depth)` per node and
/// `O(depth²)` over a deeply nested file, however few parent steps it
/// takes (#1084). A walker that visits parents before children already
/// holds the chain, and handing it down turns each step into a slice
/// index — the upward counterpart of the downward flag propagation
/// #1052 and #1062 used.
///
/// Callers that reached a node some other way pass
/// [`Ancestors::unknown`], which climbs with [`Node::parent`]: the same
/// answers at the original cost.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ancestors<'tree, 'chain>(Option<&'chain [Node<'tree>]>);

impl<'tree, 'chain> Ancestors<'tree, 'chain> {
    /// No chain is available; every query climbs with [`Node::parent`].
    pub(crate) const fn unknown() -> Self {
        Self(None)
    }

    /// `chain` lists every ancestor of the node about to be queried,
    /// root first — so `chain.last()` is its parent and an empty chain
    /// means the node is the root.
    pub(crate) const fn known(chain: &'chain [Node<'tree>]) -> Self {
        Self(Some(chain))
    }

    /// [`Ancestors::known`], but in debug builds first checks that
    /// `chain` really is `node`'s ancestry.
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
    /// Debug-only, because [`Node::parent`] is the `O(depth)` lookup
    /// #1084 exists to remove and must never run in a release build.
    pub(crate) fn checked(chain: &'chain [Node<'tree>], node: &Node<'tree>) -> Self {
        debug_assert_eq!(
            chain.last().map(Node::id),
            node.parent().map(|parent| parent.id()),
            "ancestor chain desynchronised on a {} node",
            node.kind()
        );
        Self::known(chain)
    }

    /// `node`'s parent.
    pub(crate) fn parent(self, node: &Node<'tree>) -> Option<Node<'tree>> {
        match self.0 {
            Some(chain) => chain.last().copied(),
            None => node.parent(),
        }
    }

    /// The sibling immediately before `node`, or `None` when `node` is
    /// its parent's first child or has no parent.
    ///
    /// `tree_sitter`'s own `prev_sibling` resolves the parent first
    /// (`ts_node__prev_sibling` opens with `ts_node_parent`), so it
    /// carries the same `O(depth)` cost [`Node::parent`] does. With a
    /// known chain the parent is free and what remains is a cursor walk
    /// over the siblings.
    pub(crate) fn previous_sibling(self, node: &Node<'tree>) -> Option<Node<'tree>> {
        let Some(chain) = self.0 else {
            return node.previous_sibling();
        };
        let parent = chain.last()?;
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
    pub(crate) fn iter(self, node: &Node<'tree>) -> AncestorIter<'tree, 'chain> {
        match self.0 {
            Some(chain) => AncestorIter::Chain(chain),
            None => AncestorIter::Climb(node.parent()),
        }
    }
}

/// Ancestor iterator returned by [`Ancestors::iter`], nearest first.
pub(crate) enum AncestorIter<'tree, 'chain> {
    /// The not-yet-yielded prefix of a known chain. Its last element is
    /// the next ancestor, and the prefix before it is that ancestor's
    /// own chain — so splitting from the back hands out both at once.
    Chain(&'chain [Node<'tree>]),
    /// The next ancestor to yield, reached by climbing.
    Climb(Option<Node<'tree>>),
}

impl<'tree, 'chain> Iterator for AncestorIter<'tree, 'chain> {
    type Item = (Node<'tree>, Ancestors<'tree, 'chain>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Chain(remaining) => {
                let (&nearest, above) = remaining.split_last()?;
                *remaining = above;
                Some((nearest, Ancestors::known(above)))
            }
            Self::Climb(next) => {
                let nearest = (*next)?;
                *next = nearest.parent();
                Some((nearest, Ancestors::unknown()))
            }
        }
    }
}

/// Pre-order iterator over a node and its descendants, returned by
/// [`Node::preorder`].
///
/// Holds a single work stack of not-yet-visited nodes. Each step pops the
/// next node, pushes its children so the leftmost is visited first, and
/// yields the popped node — so the sequence is the node, then each child
/// subtree in order. The stack is reused across steps (children are pushed
/// then the freshly-pushed slice is reversed in place), so the walk
/// allocates only the stack's growth, not a fresh buffer per node.
pub struct Preorder<'a> {
    stack: Vec<Node<'a>>,
}

impl<'a> Iterator for Preorder<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Push children in document order, then reverse just the slice we
        // appended so the leftmost child ends up on top of the stack and
        // is visited next — pre-order without a per-node temporary.
        let first_child = self.stack.len();
        self.stack.extend(node.children());
        self.stack[first_child..].reverse();
        Some(node)
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

    fn act_on_node(&self, action: &mut dyn FnMut(&Node<'a>, Ancestors<'a, '_>)) {
        let mut cursor = self.cursor();
        let mut stack = Vec::new();
        let mut children = Vec::new();
        // Ancestor chain of the node being visited, root first. Kept by
        // the same truncate/push rule as the metric walk, so a predicate
        // the action applies can read an ancestor as a slice index
        // rather than through the `O(depth)` `Node::parent` (#1088).
        //
        // Seeded with this subtree root's own ancestry rather than left
        // empty: `Ancestors` reads an empty chain as "this node is the
        // tree root", so on a subtree an empty seed would report no
        // parent for `*self` — silently costing e.g. the JS getters the
        // binding a `function_expression` takes its name from. One
        // climb, and none at all for the tree root this is called on
        // today.
        let mut chain: Vec<Node<'a>> = std::iter::successors(self.parent(), Node::parent).collect();
        chain.reverse();
        let depth = chain.len();

        stack.push((*self, depth));

        while let Some((node, depth)) = stack.pop() {
            chain.truncate(depth);
            action(&node, Ancestors::checked(&chain, &node));
            chain.push(node);
            cursor.reset(&node);
            if cursor.goto_first_child() {
                loop {
                    children.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                for child in children.drain(..).rev() {
                    stack.push((child, depth + 1));
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
    use crate::test_support::for_each_node_with_chain;

    /// Under a parent narrow enough to read forward, the
    /// `exclude_tests` prune finds the run of `#[…]` siblings before an
    /// item through the walker's ancestor chain, never by resolving
    /// siblings from the node.
    ///
    /// Nothing in the output says so: the backward walk this replaced
    /// returns the same answer, only `O(depth)` per step (#1100), and
    /// `rust_outer_attr_scans_agree` in `checker.rs` exists precisely
    /// to prove the two agree. The counter is the sole observable, so a
    /// revert is a silent quadratic without this.
    ///
    /// Every parent in the fixture holds at most five children, which
    /// keeps it under `MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN` — the
    /// backward walk is still the deliberate reading above that width,
    /// so a wider fixture would assert the opposite of what it looks
    /// like it asserts.
    ///
    /// Seeding a real lookup first is what makes the assertion
    /// falsifiable: compared against zero it would also pass with
    /// `record()` never wired up at all.
    #[cfg(feature = "rust")]
    #[test]
    fn the_exclude_tests_prune_resolves_no_sibling_from_a_node() {
        let source = "#[cfg(test)]\nmod tests {\nfn t() {}\n}\n\
                      #[inline]\nfn kept() {\n#[allow(dead_code)]\nfn nested() {}\nlet x = 1;\n}\n";
        let ast = crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", source);

        let root = Node(ast.as_tree_sitter().root_node());
        let last = root.children().last().expect("the file has items");
        let _ = last.previous_sibling();
        let seeded = node_resolved_sibling_lookups::observed();
        assert!(seeded > 0, "the seed call must be counted");

        ast.metrics(crate::MetricsOptions::default().with_exclude_tests(true))
            .expect("the walk must yield a top-level space");

        assert_eq!(
            node_resolved_sibling_lookups::observed(),
            seeded,
            "the metric walk resolved a sibling from a node; \
             read it off the ancestor chain instead (#1096 / #1100)"
        );
    }

    /// The `child(0)` + `next_sibling()` chain [`Node::wraps_any`] used
    /// between #217 and #1088, kept here as the reference the cursor
    /// walk that replaced it is checked against.
    ///
    /// The swap was made for cost, not for behaviour: a sibling step
    /// resolves its parent, and `tree_sitter` resolves a parent by
    /// descending from the root, so the chain was `O(children × depth)`
    /// where the cursor is `O(children)`. Nothing about the *set* of
    /// children was supposed to change, and this is what says so —
    /// node-by-node over a real tree, same order and same short-circuit,
    /// without hardcoding grammar `kind_id`s.
    fn sibling_chain_has_sibling(node: OtherNode, id: u16) -> bool {
        node.parent().is_some_and(|parent| {
            let mut cur = parent.child(0);
            while let Some(c) = cur {
                if c.kind_id() == id {
                    return true;
                }
                cur = c.next_sibling();
            }
            false
        })
    }

    #[test]
    fn has_sibling_matches_the_retired_sibling_chain() {
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
        let mut matched = 0;
        while let Some(n) = stack.pop() {
            let wrapped = Node(n);
            for &id in kinds.iter().chain(std::iter::once(&absent_id)) {
                let found = wrapped.has_sibling(Ancestors::unknown(), id);
                assert_eq!(
                    found,
                    sibling_chain_has_sibling(n, id),
                    "has_sibling diverged from the retired sibling chain at node kind {} for id {id}",
                    n.kind(),
                );
                matched += usize::from(found);
            }
            let mut child = n.child(0);
            while let Some(c) = child {
                stack.push(c);
                child = c.next_sibling();
            }
        }
        // The comment above claims the collected kinds cover the
        // present-sibling case; this enforces it. Both sides answering
        // `false` everywhere would agree without either scan ever
        // running to a match.
        assert!(
            matched > 0,
            "every answer was `false`, so the sibling scan was never exercised"
        );

        // No-parent node (root) always reports no sibling.
        let root = Node(ts_tree.root_node());
        assert!(!root.has_sibling(Ancestors::unknown(), absent_id));
        for &id in &kinds {
            assert!(
                !root.has_sibling(Ancestors::unknown(), id),
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

    /// `child_by_field_name` (issue #786) must return the child at the
    /// underlying tree lifetime `'a`, not the method-call borrow of
    /// `&self`. The proof is a helper whose return type *requires* the
    /// child to outlive an intermediate `&Node` borrow: under the old
    /// `Option<Node<'_>>` signature the returned node would be tied to
    /// `parent`'s borrow and this would fail to compile. Binding the
    /// child to a variable that outlives the `&parent` reborrow inside
    /// the helper exercises the widened lifetime.
    #[test]
    fn child_by_field_name_outlives_self_borrow() {
        // `find_named_child` takes the parent by value, reborrows it
        // through a `&` reference to call `child_by_field_name`, and
        // returns the child. The returned `Node<'a>` must survive past
        // that inner `&parent` borrow — only possible because the child
        // carries the tree lifetime, not the borrow of `&parent`.
        fn find_named_child<'a>(parent: Node<'a>) -> Option<Node<'a>> {
            let borrowed: &Node<'a> = &parent;
            borrowed.child_by_field_name("declarator")
        }

        let code = b"int answer = 42;";
        let tree = Tree::new::<crate::langs::CppCode>(code);
        let root = tree.get_root();

        // Walk to the `declaration` node, then pull its `declarator`
        // child out and hold it after the producing borrow has ended.
        let mut held: Option<Node> = None;
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if n.kind() == "declaration" {
                // `find_named_child` consumes a copy of `n`; the result
                // must remain valid here, well past the inner borrow.
                held = find_named_child(n);
                break;
            }
            for child in n.children() {
                stack.push(child);
            }
        }

        let declarator = held.expect("C declaration has a `declarator` field");
        // The held node is still usable: it kept its tree linkage rather
        // than dangling at the end of the producing borrow.
        assert_eq!(declarator.kind(), "init_declarator");
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

    /// Ground-truth pre-order walk over the raw tree-sitter node, by
    /// document order (`child(0..child_count)`). [`Node::preorder`] must
    /// emit exactly this sequence of node ids — node first, then each
    /// child subtree left to right.
    fn ground_truth_preorder(node: OtherNode) -> Vec<usize> {
        let mut out = vec![node.id()];
        for i in 0..node.child_count() as u32 {
            if let Some(child) = node.child(i) {
                out.extend(ground_truth_preorder(child));
            }
        }
        out
    }

    #[test]
    fn preorder_matches_recursive_document_order() {
        // A nested construct (function holding a declaration and a call)
        // gives the walk real depth and sibling fan-out to order.
        let code = b"int main() { int x = 1; foo(x); return 0; }";
        let tree = Tree::new::<crate::langs::CppCode>(code);
        let root = tree.get_root();

        let actual: Vec<usize> = root.preorder().map(|n| n.id()).collect();
        let expected = ground_truth_preorder(root.as_tree_sitter());

        assert_eq!(
            actual, expected,
            "preorder diverged from recursive child(0..n) document order"
        );
        // Sanity: a non-trivial tree, and the root is visited first.
        assert!(actual.len() > 5, "expected a multi-node tree");
        assert_eq!(actual[0], root.id(), "root must be yielded first");
    }

    #[test]
    fn descendants_by_kind_collects_matching_subtree_nodes() {
        // `x` is declared once and used twice, so three `identifier`
        // nodes exist under the function; `main` is an identifier too.
        let code = b"int main() { int x = 1; return x + x; }";
        let tree = Tree::new::<crate::langs::CppCode>(code);
        let root = tree.get_root();

        let found = root.descendants_by_kind(&["identifier"]);
        // Cross-check against an independent pre-order count so the helper
        // cannot pass by matching everything or nothing.
        let expected: Vec<usize> = root
            .preorder()
            .filter(|n| n.kind() == "identifier")
            .map(|n| n.id())
            .collect();
        let actual: Vec<usize> = found.iter().map(Node::id).collect();
        assert_eq!(actual, expected);
        assert!(
            found.len() >= 3,
            "expected at least the `main`, `x` decl, and `x` uses"
        );
        assert!(
            found.iter().all(|n| n.kind() == "identifier"),
            "every collected node must match the requested kind"
        );

        // An absent kind yields nothing; a multi-kind filter unions.
        assert!(root.descendants_by_kind(&["no_such_kind"]).is_empty());
        assert!(
            root.descendants_by_kind(&["identifier", "number_literal"])
                .len()
                > found.len(),
            "adding `number_literal` must widen the match set"
        );
    }

    /// `descendant_count` must count the same nodes the metric walk
    /// visits, because `spaces::compute::metrics_inner` uses it as the
    /// exact capacity for a map that ends up holding one entry per
    /// visited node.
    ///
    /// The risk it guards is silent: `ts_node_descendant_count` counts
    /// *visible* descendants, so were it ever to narrow to named nodes
    /// only, the reserve would under-size by the anonymous-token share
    /// of the tree — roughly half — and the map would quietly go back
    /// to rehashing, with no test failing. The source below is chosen to
    /// carry plenty of anonymous tokens (`int`, `(`, `{`, `=`, `;`) so
    /// the named-only reading is not accidentally equal.
    #[test]
    fn descendant_count_matches_the_walked_node_population() {
        let code = b"int main() { int x = 1; foo(x); return 0; }";
        let tree = Tree::new::<crate::langs::CppCode>(code);
        let root = tree.get_root();

        // `preorder` yields the node itself and then every descendant,
        // enumerating children exactly as the metric walk's
        // `push_children` does.
        let walked = root.preorder().count();
        assert_eq!(
            root.descendant_count(),
            walked,
            "descendant_count must equal the pre-order node count"
        );

        let named = root.preorder().filter(Node::is_named).count();
        assert!(
            named < walked,
            "fixture must contain anonymous tokens, else the assertion \
             above cannot distinguish a named-only count"
        );
    }

    /// Ancestor ids yielded by `ancestors`, nearest first.
    fn ancestor_ids(ancestors: Ancestors<'_, '_>, node: &Node<'_>) -> Vec<usize> {
        ancestors.iter(node).map(|(a, _)| a.id()).collect()
    }

    /// A known chain must answer every ancestor question exactly as
    /// climbing with `Node::parent` does — that equivalence is the whole
    /// premise of #1084, and it is what lets the predicates keep their
    /// original logic while dropping the `O(depth)` lookup.
    ///
    /// Checked node-by-node over one fixture per grammar family that
    /// actually consults an ancestor: C-family (`is_else_if` via the
    /// parent clause, `loc`'s declaration gate), JVM-family
    /// (`is_else_if` via the preceding `else` token), Python (the
    /// grandparent shape), and Elixir (`quote` templates).
    #[test]
    fn a_known_chain_answers_exactly_what_climbing_answers() {
        /// `must_nest` names kinds that have to appear *inside another
        /// node of the same kind* in the fixture. `visited > 20` alone
        /// does not keep a fixture honest: a grammar bump that flattened
        /// the nesting a row was added for would leave a large,
        /// clean-parsing tree that no longer exercises the shape, and
        /// the parity assertions would keep passing over it.
        fn assert_parity<L: LanguageInfo>(label: &str, code: &[u8], must_nest: &[&str]) {
            let mut nested_seen = vec![false; must_nest.len()];
            let visited = for_each_node_with_chain::<L>(code, |node, chain| {
                for (slot, kind) in nested_seen.iter_mut().zip(must_nest) {
                    *slot |= node.kind() == *kind
                        && chain.iter().any(|ancestor| ancestor.kind() == *kind);
                }
                let known = Ancestors::known(chain);
                let climbing = Ancestors::unknown();
                assert_eq!(
                    known.parent(node).map(|p| p.id()),
                    climbing.parent(node).map(|p| p.id()),
                    "{label}: parent of {} disagrees",
                    node.kind()
                );
                assert_eq!(
                    known.previous_sibling(node).map(|p| p.id()),
                    climbing.previous_sibling(node).map(|p| p.id()),
                    "{label}: previous sibling of {} disagrees",
                    node.kind()
                );
                assert_eq!(
                    ancestor_ids(known, node),
                    ancestor_ids(climbing, node),
                    "{label}: ancestor chain of {} disagrees",
                    node.kind()
                );
                // Each ancestor is handed *its* own chain, so a
                // predicate applied one level up stays as cheap and as
                // correct as one applied to the node itself.
                for (ancestor, above) in known.iter(node) {
                    assert_eq!(
                        above.parent(&ancestor).map(|p| p.id()),
                        ancestor.parent().map(|p| p.id()),
                        "{label}: sub-chain handed to {} is not its own",
                        ancestor.kind()
                    );
                }
            });
            assert!(visited > 20, "{label}: fixture is too small to prove much");
            for (found, kind) in nested_seen.iter().zip(must_nest) {
                assert!(
                    found,
                    "{label}: no `{kind}` sits inside another `{kind}`, so the \
                     fixture no longer exercises the nesting it was added for"
                );
            }
        }

        assert_parity::<crate::langs::CCode>(
            "c",
            b"int main() { if (a) { int x; } else if (b) { for (int i = 0; i < 2; i++) x; } }",
            &[],
        );
        assert_parity::<crate::langs::JavaCode>(
            "java",
            b"class A { void m() { if (a) {} else if (b) {} else {} for (int i = 0; i < 2; i++) {} } }",
            &[],
        );
        assert_parity::<crate::langs::PythonCode>(
            "python",
            b"def f(a, b):\n    if a:\n        pass\n    else:\n        if b:\n            pass\n    return a and b or a\n",
            &[],
        );
        assert_parity::<crate::langs::ElixirCode>(
            "elixir",
            b"defmodule M do\n  def g do\n    :ok\n  end\n  quote do\n    def f do\n      :ok\n    end\n  end\nend\n",
            &["call"],
        );

        // The shapes #1062 added as consumers, which the four fixtures
        // above do not contain: a function nested inside a function
        // (every language's `increment_function_depth` arm walks the
        // chain looking for one) and the two default-arm checks that
        // now read `Ancestors::parent` — Kotlin's `else ->` inside a
        // `when` and Ruby's `else` inside a `case`. Parity over the
        // machinery is not parity over the shape a caller asks about.
        assert_parity::<crate::langs::RustCode>(
            "rust",
            b"fn f(a: bool) { if a { } else if a { } fn g(b: bool) { if b { } } }\n",
            &["function_item"],
        );
        assert_parity::<crate::langs::KotlinCode>(
            "kotlin",
            b"fun f(x: Int) {\n    when (x) {\n        1 -> {}\n        else -> {}\n    }\n    fun g() {\n        if (x > 0) {}\n    }\n}\n",
            &["function_declaration"],
        );
        assert_parity::<crate::langs::RubyCode>(
            "ruby",
            b"def f(x)\n  case x\n  when 1 then 1\n  else 2\n  end\n  def g\n    if x\n    end\n  end\nend\n",
            &["method"],
        );

        // The shape #1088 added as a consumer: the JS-family
        // `Checker::is_func` / `is_closure` walk upward from an
        // `arrow_function` / `function_expression` looking for the
        // binding that names it, and end on `Ancestors::previous_sibling`
        // through `has_sibling`. None of the fixtures above contains
        // either node.
        assert_parity::<crate::langs::JavascriptCode>(
            "javascript",
            b"const f = a => { a => { g(() => 1); }; };\nconst o = { m: function () { return 1; } };\n",
            &["arrow_function"],
        );
    }

    /// [`Node::has_sibling`] must answer the same whether its parent
    /// comes off a known chain or from `Node::parent`.
    ///
    /// The parent lookup is the only thing #1088 changed here, and it is
    /// the half a caller cannot see: `check_if_arrow_func!` folds the
    /// answer into a disjunction, so a wrong parent would silently
    /// reclassify an arrow function rather than fail. Checked for every
    /// node against every kind the fixture contains, plus one that never
    /// occurs so the absent-sibling answer is covered too.
    #[test]
    fn has_sibling_agrees_between_known_and_climbing() {
        // Object-literal methods and an arrow bound to a property are
        // the shapes whose `PropertyIdentifier` sibling the JS closure
        // check asks about.
        let code = b"const o = { m: (a) => a + 1, n: function () {} };\nconst p = a => a;\n";
        let mut kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<crate::langs::JavascriptCode>(code, |node, _| {
            kinds.insert(node.kind_id());
        });
        // An id no node in the fixture carries, so the `false` answer is
        // exercised as well as the `true` one.
        let absent = u16::MAX;
        let mut agreed_true = 0;
        let visited =
            for_each_node_with_chain::<crate::langs::JavascriptCode>(code, |node, chain| {
                for &id in kinds.iter().chain(std::iter::once(&absent)) {
                    let known = node.has_sibling(Ancestors::known(chain), id);
                    let climbing = node.has_sibling(Ancestors::unknown(), id);
                    assert_eq!(
                        known,
                        climbing,
                        "has_sibling({id}) on {} disagrees between chain and climb",
                        node.kind()
                    );
                    agreed_true += usize::from(known);
                }
            });
        assert!(visited > 20, "fixture is too small to prove much");
        assert!(
            agreed_true > 0,
            "every answer was `false`, so the sibling scan never ran to a match"
        );
    }

    /// [`Search::act_on_node`] must hand each node its true ancestry
    /// even when the walk starts below the tree root.
    ///
    /// The seed is what decides this. [`Ancestors`] reads an empty chain
    /// as "this node is the root", so seeding empty — which is correct
    /// for the one caller that exists today, `bca function`, whose walk
    /// starts at the root — would report no parent for the subtree root
    /// and shift every answer beneath it. For the JS getters that means
    /// losing the `variable_declarator` a `function_expression` takes
    /// its name from, so the space would silently be named
    /// `<anonymous>`.
    ///
    /// No caller passes a subtree yet, so nothing else would catch this;
    /// the fixture below is the guard, and it fails against an empty
    /// seed both here and through `Ancestors::checked`'s debug
    /// assertion.
    #[test]
    fn act_on_node_hands_a_subtree_its_real_ancestry() {
        let code = b"var outer = function () { return 1; };\n";
        let tree = Tree::new::<MozjsCode>(code);
        let root = tree.get_root();
        let subtree = root
            .preorder()
            .find(|n| n.kind() == "variable_declarator")
            .expect("fixture has a variable_declarator");
        assert!(
            subtree.parent().is_some(),
            "the walk must start below the root, else the seed is vacuous"
        );

        let mut visited = 0;
        subtree.act_on_node(&mut |node, ancestors| {
            assert_eq!(
                ancestors.parent(node).map(|p| p.id()),
                node.parent().map(|p| p.id()),
                "parent of {} disagrees with the tree",
                node.kind()
            );
            visited += 1;
        });
        assert!(visited > 3, "subtree is too small to prove much");
    }

    /// `previous_sibling` must not answer "no previous sibling" when the
    /// chain it was handed belongs to a different node.
    ///
    /// The known path finds the answer by scanning the chain's last
    /// entry for `node`; a miss means the caller paired the two wrongly.
    /// Reporting `None` there would be a wrong answer dressed as a
    /// legitimate one, so the fallback re-asks the tree.
    #[test]
    fn previous_sibling_falls_back_on_a_chain_that_is_not_this_nodes() {
        let code = b"int main() { int a; int b; }";
        let tree = Tree::new::<crate::langs::CCode>(code);
        let root = tree.get_root();
        let body = root
            .preorder()
            .find(|n| n.kind() == "compound_statement")
            .expect("fixture has a function body");
        let declarations: Vec<Node<'_>> = body
            .children()
            .filter(|n| n.kind() == "declaration")
            .collect();
        assert_eq!(declarations.len(), 2, "fixture has two declarations");

        let second = declarations[1];
        let expected = second
            .previous_sibling()
            .map(|p| p.id())
            .expect("the second declaration has a previous sibling");
        // A chain ending in the *root* does not describe `second`, whose
        // parent is the function body.
        let foreign = [root];
        assert_eq!(
            Ancestors::known(&foreign)
                .previous_sibling(&second)
                .map(|p| p.id()),
            Some(expected),
            "a mismatched chain must fall back, not report `None`"
        );
        assert!(
            Ancestors::known(&[]).previous_sibling(&second).is_none(),
            "an empty chain means `second` is the root, which has no siblings"
        );
    }

    /// `previous_sibling_under` must answer exactly what the
    /// authoritative `Node::previous_sibling` does, for every child of
    /// the parent — including the first, whose answer is `None` for a
    /// reason (no earlier sibling) rather than by accident. The ABC
    /// container walkers depend on this: they seed their
    /// boolean-context flag from whether a ternary's `?` / `:` precedes
    /// the operand (#1096).
    #[test]
    fn previous_sibling_under_agrees_with_the_authoritative_lookup() {
        // Anonymous tokens (`(`, `,`, `)`) sit between the named
        // arguments, so the sequence exercises both kinds of sibling.
        let code = b"int main() { f(a, b, c); }";
        let tree = Tree::new::<crate::langs::CCode>(code);
        let arguments = tree
            .get_root()
            .preorder()
            .find(|n| n.kind() == "argument_list")
            .expect("fixture has an argument list");
        let children: Vec<Node<'_>> = arguments.children().collect();
        assert!(
            children.len() > 3,
            "fixture must have several siblings, got {}",
            children.len()
        );

        let mut first_is_none = false;
        for child in &children {
            let expected = child.previous_sibling().map(|p| p.id());
            assert_eq!(
                child.previous_sibling_under(&arguments).map(|p| p.id()),
                expected,
                "disagreed on the sibling before a {} node",
                child.kind()
            );
            first_is_none |= expected.is_none();
        }
        assert!(
            first_is_none,
            "the first child's `None` is part of what this pins"
        );
        // One break this cannot see: a scan that never finds `node`
        // among the children falls back to the authoritative lookup and
        // so still answers correctly, just at `Node::parent`'s cost.
        // That failure mode is a perf regression, not a wrong answer,
        // and the `abc/nested-if` probe is what covers it. Every
        // *wrong-answer* break does fail here — returning the following
        // sibling, or the parent's first child, fails on the first
        // child alone.
    }

    /// `count_specific_ancestors` must return the same count whichever
    /// way it reaches the ancestors. Uses `loc`'s real C predicate pair
    /// (`while`/`for`/`if` header, stopping at the enclosing block), so
    /// the fixture exercises both the counted case (the `for`-header
    /// declaration) and the stopped case (the block-scoped ones).
    #[test]
    fn count_specific_ancestors_agrees_between_known_and_climbing() {
        let code =
            b"int main() { int a; if (x) { int b; } for (int i = 0; i < 2; i++) { int c; } }";
        let mut counted = 0;
        let mut nonzero = 0;
        let visited = for_each_node_with_chain::<crate::langs::CCode>(code, |node, chain| {
            if node.kind() != "declaration" {
                return;
            }
            let check: fn(&Node) -> bool = |n| {
                matches!(
                    n.kind(),
                    "while_statement" | "for_statement" | "if_statement"
                )
            };
            let stop: fn(&Node) -> bool = |n| n.kind() == "compound_statement";
            let known = node.count_specific_ancestors::<crate::langs::CCode>(
                Ancestors::known(chain),
                check,
                stop,
            );
            let climbing = node.count_specific_ancestors::<crate::langs::CCode>(
                Ancestors::unknown(),
                check,
                stop,
            );
            assert_eq!(
                known,
                climbing,
                "declaration at row {}: known chain counted {known}, climbing counted {climbing}",
                node.start_row()
            );
            counted += 1;
            nonzero += usize::from(known > 0);
        });
        assert!(visited > 20);
        assert_eq!(counted, 4, "fixture must hold four declarations");
        assert_eq!(
            nonzero, 1,
            "only the `for`-header declaration sits under a header with no block between"
        );
    }
}
