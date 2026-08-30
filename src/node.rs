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

// Child scans that built their own `TreeCursor`, on this thread.
//
// [`Node::children_with`] yields exactly what [`Node::children`] yields
// — it exists to reuse one cursor across a traversal instead of
// heap-allocating and freeing one per visited node, and no assertion on
// a metric value can tell the two apart. #1112 moved the per-node
// traversals that could hoist a cursor onto it; the counter is what
// makes moving one back a test failure rather than a silent allocation
// per node.
//
// All five consumers are guarded: `preorder` and `act_on_node` here,
// `metrics::npa::python` and the suppression DFS through a `metrics()`
// / `suppression_markers` call, and `output::dump`'s renderer from that
// module's own tests. The accessor is `pub(crate)` (not `pub(super)`)
// precisely so the last one can be asserted from where it lives — see
// `crate::observation`.
crate::observation::counter!(child_scan_cursors);

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

    /// The 1-based **last line this node occupies** — the inclusive end
    /// of its line span.
    ///
    /// Converting tree-sitter's 0-based end row needs a `+ 1` only when
    /// the node actually occupies the row it ends on. A node whose end
    /// column is 0 finished at the *start* of the row below its last
    /// content row, having absorbed the preceding newline, so that row
    /// is not part of its span.
    ///
    /// Getting this wrong is invisible in almost every grammar, because
    /// almost every function node ends just past its closing delimiter
    /// and therefore at a column above 0. Perl's trailing `sub` does
    /// not: it ends at column 0 of the row below its `}`, exactly where
    /// the file root ends, and a blanket `+ 1` then reported it ending a
    /// line past both its enclosing space and EOF (#1163) — the shape
    /// behind the release `usize` underflow in #1051.
    pub(crate) fn end_line(&self) -> usize {
        let end = self.0.end_position();
        if end.column == 0 {
            end.row
        } else {
            end.row + 1
        }
    }

    /// Returns this node's parent.
    ///
    /// **`O(depth)`, not `O(1)`.** tree-sitter stores no parent pointer:
    /// `ts_node_parent` restarts at the tree root and descends. A single
    /// call in a per-node metric therefore costs `O(nodes × depth)` over a
    /// walk, and an ancestor *loop* built on it costs `O(depth²)` per call.
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
    /// `Ancestors` itself (the no-chain fallback), the one-off start node
    /// of a `dump`, and tests. `rg '\.parent\(\)' src/` re-checks that.
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
    /// `ancestors` supplies the parent, for [`Node::parent`]'s
    /// `O(depth)` reason (#1088).
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
    /// walk should use [`Ancestors::previous_sibling`] instead (#1096).
    pub(crate) fn previous_sibling(&self) -> Option<Node<'a>> {
        node_resolved_sibling_lookups::record();
        self.0.prev_sibling().map(Node)
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

    /// Iterator over this node's direct children.
    ///
    /// Builds a [`Cursor`], which heap-allocates. A loop that visits
    /// many nodes should hoist one cursor and use [`children_with`]
    /// instead; see its note for when the difference is worth the
    /// plumbing.
    ///
    /// [`children_with`]: Self::children_with
    pub(crate) fn children(&self) -> Children<'a> {
        child_scan_cursors::record();
        // `descend`, not `seed`: `ts_node_walk` already ran the
        // `ts_tree_cursor_init` that `Cursor::reset` would run again.
        let mut cursor = self.cursor();
        let scan = ChildScan::descend(self, &mut cursor);
        Children { cursor, scan }
    }

    /// [`children`], over a cursor the caller owns.
    ///
    /// `tree_sitter::TreeCursor` heap-allocates its stack when built and
    /// frees it when dropped, so a traversal that calls [`children`]
    /// once per visited node pays a `malloc`/`free` pair per node.
    /// `ts_tree_cursor_reset` keeps the allocation, so a loop that can
    /// hoist one cursor pays for it once however many nodes it visits
    /// (#1112).
    ///
    /// Worth the plumbing only where a loop visits many nodes. Measured
    /// on the corpus slice, a full metric walk reaches [`children`] on
    /// 3-6 % of nodes in C++, Rust, JavaScript, and Java, 16 % in C#,
    /// and 60 % in Python, where one scan — the instance-attribute walk
    /// in `metrics::npa::python` — was 92 % of the total, and
    /// [`preorder`] plus the suppression DFS are the rest. Crate-wide
    /// there is one more per-node consumer, `output::dump`'s renderer,
    /// which no metric walk runs and so is absent from that total.
    /// Predicates
    /// that hold a bare `&Node` and scan one node's children keep
    /// [`children`]: threading a cursor to them would cross the
    /// `Checker` / `Getter` trait surface for one allocation per call.
    ///
    /// [`preorder`]: Self::preorder
    ///
    /// [`children`]: Self::children
    pub(crate) fn children_with<'c>(&self, cursor: &'c mut Cursor<'a>) -> ChildrenWith<'c, 'a> {
        let scan = ChildScan::seed(self, cursor);
        ChildrenWith { cursor, scan }
    }

    pub(crate) fn cursor(&self) -> Cursor<'a> {
        Cursor(self.0.walk())
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
    /// The traversal is allocation-light: it reuses one work stack *and
    /// one cursor*, and visits each node exactly once, so a full walk is
    /// O(n) in the subtree size and allocates only the stack's growth.
    /// Building a fresh cursor per visited node instead would cost a
    /// `malloc`/`free` pair per node (#1112). Every yielded [`Node`]
    /// carries the underlying tree lifetime `'a`, so callers may collect
    /// or retain the handles.
    ///
    /// This is the Rust counterpart of the Python `Node.walk()` binding
    /// (issue #728): the binding wraps each yielded node, so Rust and
    /// Python share one traversal order.
    #[must_use]
    pub fn preorder(&self) -> Preorder<'a> {
        Preorder {
            stack: vec![*self],
            cursor: self.cursor(),
        }
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
    /// very lookup #1084 exists to remove. Per node, on all five walks
    /// that construct a checked chain, it made every debug-build walk
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
    pub(crate) fn checked(chain: &'chain [Node<'tree>], node: &Node<'tree>) -> Self {
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
    pub(crate) fn depth(self) -> Option<usize> {
        self.0.map(<[Node<'tree>]>::len)
    }

    /// `node`'s parent.
    pub(crate) fn parent(self, node: &Node<'tree>) -> Option<Node<'tree>> {
        match self.0 {
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
    pub(crate) fn parent_has_kind(self, node: &Node<'tree>, kind: u16) -> bool {
        self.parent(node).is_some_and(|p| p.kind_id() == kind)
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
/// Holds a single work stack of not-yet-visited nodes, and a single
/// cursor to enumerate each node's children with. Each step pops the
/// next node, pushes its children so the leftmost is visited first, and
/// yields the popped node — so the sequence is the node, then each child
/// subtree in order. Both are reused across steps (children are pushed
/// then the freshly-pushed slice is reversed in place), so the walk
/// allocates only the stack's growth: no fresh buffer per node, and no
/// fresh `TreeCursor` either (#1112).
pub struct Preorder<'a> {
    stack: Vec<Node<'a>>,
    cursor: Cursor<'a>,
}

impl<'a> Iterator for Preorder<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Push children in document order, then reverse just the slice we
        // appended so the leftmost child ends up on top of the stack and
        // is visited next — pre-order without a per-node temporary.
        let first_child = self.stack.len();
        // Destructured so the stack and the cursor are borrowed as the
        // disjoint fields they are.
        let Self { stack, cursor } = self;
        stack.extend(node.children_with(cursor));
        stack[first_child..].reverse();
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

/// Position of a child scan, independent of who owns the cursor driving
/// it.
///
/// [`Children`] and [`ChildrenWith`] differ only in that — one owns its
/// cursor, the other borrows the caller's — and `children_with` exists
/// to save an allocation, not to answer differently. Keeping the
/// termination rule here rather than in each iterator is what stops the
/// two from drifting.
struct ChildScan {
    done: bool,
    remaining: usize,
}

impl ChildScan {
    /// Seats `cursor` on `node`'s first child.
    ///
    /// `goto_first_child` returns false when the node has no children,
    /// in which case the scan is exhausted from the outset. Termination
    /// is then driven entirely by the cursor (see [`ChildScan::step`]),
    /// so the iterator stops exactly when the tree reports no further
    /// siblings — it can never pad the sequence with duplicate nodes if
    /// `child_count` and the cursor walk ever disagree.
    ///
    /// `child_count` is the authoritative length for the
    /// `ExactSizeIterator` contract; for well-formed trees it equals the
    /// cursor sibling walk, so the reported length and the emitted data
    /// agree. A childless node reports `0` so the empty iterator's
    /// length matches its (lack of) data.
    fn seed<'a>(node: &Node<'a>, cursor: &mut Cursor<'a>) -> Self {
        cursor.reset(node);
        Self::descend(node, cursor)
    }

    /// [`ChildScan::seed`], for a cursor already seated on `node` —
    /// which is what [`Node::cursor`] hands back, and `ts_node_walk` and
    /// `ts_tree_cursor_reset` run the same `ts_tree_cursor_init`. Only
    /// [`Node::children`] may skip the reset; every other caller reuses
    /// a cursor left wherever the previous scan ended.
    fn descend<'a>(node: &Node<'a>, cursor: &mut Cursor<'a>) -> Self {
        let done = !cursor.goto_first_child();
        Self {
            done,
            remaining: if done { 0 } else { node.child_count() },
        }
    }

    /// Yields the cursor's current child and advances past it.
    fn step<'a>(&mut self, cursor: &mut Cursor<'a>) -> Option<Node<'a>> {
        if self.done {
            return None;
        }
        let result = cursor.node();
        // The cursor is the single source of truth for termination:
        // once there is no next sibling this yield is the last one.
        self.done = !cursor.goto_next_sibling();
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

/// Iterator over a node's direct children, returned by
/// [`Node::children`]. Owns the cursor it walks with.
///
/// Termination is driven by the cursor alone: each step yields the
/// cursor's current node, then advances with `goto_next_sibling`,
/// stopping the moment that returns false. This makes the cursor the
/// single source of truth for both the emitted data and when to stop, so
/// the sequence can never be padded with duplicates if `child_count` and
/// the actual sibling walk disagree.
///
/// The `ExactSizeIterator` length is reported from `child_count` (tracked
/// in [`ChildScan`]). For well-formed trees the cursor walk and
/// `child_count` agree, so the advertised length matches the data.
pub(crate) struct Children<'a> {
    cursor: Cursor<'a>,
    scan: ChildScan,
}

impl<'a> Iterator for Children<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.scan.step(&mut self.cursor)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.scan.size_hint()
    }
}

impl ExactSizeIterator for Children<'_> {}

/// Iterator over a node's direct children, returned by
/// [`Node::children_with`]. Borrows the caller's cursor rather than
/// building one, which is the whole of the difference: it yields exactly
/// what [`Children`] yields, through the same [`ChildScan`].
pub(crate) struct ChildrenWith<'c, 'a> {
    cursor: &'c mut Cursor<'a>,
    scan: ChildScan,
}

impl<'a> Iterator for ChildrenWith<'_, 'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.scan.step(self.cursor)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.scan.size_hint()
    }
}

impl ExactSizeIterator for ChildrenWith<'_, '_> {}

impl<'a> Search<'a> for Node<'a> {
    fn act_on_node(&self, action: &mut dyn FnMut(&Node<'a>, Ancestors<'a, '_>)) {
        let mut cursor = self.cursor();
        let mut stack = Vec::new();
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
            // Source order in, tail reversed in place, so the LIFO
            // `stack` yields the leftmost child first — pre-order with
            // no staging buffer.
            let first_child = stack.len();
            stack.extend(
                node.children_with(&mut cursor)
                    .map(|child| (child, depth + 1)),
            );
            stack[first_child..].reverse();
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

    /// Which arm the `exclude_tests` attribute-scan dispatch takes, at
    /// the boundary in both directions and on both of its axes.
    ///
    /// `rust_outer_attr_scans_agree` in `checker.rs` proves the two
    /// readings answer the same thing, which is exactly why it cannot
    /// see which one ran — it passes at any budget, including one that
    /// never reads forward. This counter is the only observable that
    /// tells them apart, and it lives here, so the boundary is pinned
    /// here too.
    ///
    /// The third case is the one #1100 got wrong: dispatching on width
    /// alone sent any over-wide body to the `O(depth)` walk however deep
    /// it sat, which on a nested `mod` tree is quadratic (a 3_200-deep
    /// fixture measured 2.67 s against 0.045 s for the same shape one
    /// child narrower).
    #[cfg(feature = "rust")]
    #[test]
    fn the_exclude_tests_prune_reads_forward_up_to_its_depth_scaled_budget() {
        // Three attributed items make a `source_file` exactly six
        // children wide — the depth-1 budget. A fourth, bare item makes
        // seven, one over. Wrapping that in a `mod` puts the same seven
        // between two braces, so its `declaration_list` is nine wide, at
        // depth 3 — where the budget is also exactly nine.
        let at_budget = "#[cfg(test)]\nfn a() {}\n#[inline]\nfn b() {}\n#[cfg(test)]\nfn c() {}\n";
        let past_budget = format!("{at_budget}fn d() {{}}\n");
        let nested = format!("mod m {{\n{past_budget}}}\n");

        for (shape, source, resolves_siblings) in [
            ("six children at depth 1", at_budget.to_string(), false),
            ("seven children at depth 1", past_budget, true),
            ("nine children at depth 3", nested, false),
        ] {
            let before = node_resolved_sibling_lookups::observed();
            crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", &source)
                .metrics(crate::MetricsOptions::default().with_exclude_tests(true))
                .expect("the walk must yield a top-level space");
            let resolved = node_resolved_sibling_lookups::observed() > before;
            assert_eq!(
                resolved, resolves_siblings,
                "{shape}: the prune took the wrong dispatch arm"
            );
        }
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

            let actual =
                drain_checking_exact_size(wrapped.children(), expected.len(), "children", n.kind());
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

    /// Drains `iter`, holding it to the `ExactSizeIterator` contract at
    /// every step, and returns the `(id, kind_id)` of each child yielded.
    ///
    /// `len()` must equal the node's `child_count` before the first step,
    /// fall by exactly one per yield, and be zero at exhaustion. Both
    /// child iterators are checked against it, so the contract is stated
    /// once — `children_with` exists to save an allocation, and a
    /// separate copy of this is how the two would come to disagree.
    fn drain_checking_exact_size<'a>(
        mut iter: impl ExactSizeIterator<Item = Node<'a>>,
        child_count: usize,
        what: &str,
        kind: &str,
    ) -> Vec<(usize, u16)> {
        assert_eq!(
            iter.len(),
            child_count,
            "{what}().len() disagreed with child_count at kind {kind}"
        );
        let mut remaining = child_count;
        let mut drained = Vec::with_capacity(remaining);
        while let Some(child) = iter.next() {
            remaining -= 1;
            assert_eq!(
                iter.len(),
                remaining,
                "{what}() size_hint drifted mid-iteration at kind {kind}"
            );
            drained.push((child.id(), child.kind_id()));
        }
        assert_eq!(
            iter.len(),
            0,
            "{what}() was not drained to zero len at kind {kind}"
        );
        drained
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

    /// The traversals #1112 moved onto [`Node::children_with`] must scan
    /// a whole tree on one cursor, not one per node.
    ///
    /// Nothing in the output says so: `children_with` yields exactly
    /// what `children` yields, so every metric, marker, and pre-order
    /// assertion in the suite holds just as well with a fresh
    /// `TreeCursor` built and freed per visited node. The counter is the
    /// only observable, which is why reverting one of these loops has to
    /// be a test failure rather than a silent allocation per node.
    ///
    /// Seeding a real scan first is what makes it falsifiable: compared
    /// against zero these assertions would also pass with `record()`
    /// never wired up at all.
    #[cfg(all(feature = "c", feature = "mozjs", feature = "python", feature = "rust"))]
    #[test]
    fn the_converted_traversals_scan_a_tree_on_one_cursor() {
        use crate::traits::ParserTrait;

        let seed_tree = Tree::new::<crate::langs::CCode>(b"int main() { int a; }");
        let _ = seed_tree.get_root().children().count();
        assert!(
            child_scan_cursors::observed() > 0,
            "the seed scan must be counted"
        );

        // `preorder` over a tree far larger than any per-call constant,
        // so "one per node" and "one per walk" cannot be confused.
        let tree = Tree::new::<MozjsCode>(
            b"const o = { m: (a) => a + 1, n: function () { return [1, 2, 3]; } };\nfoo(o);\n",
        );
        let before = child_scan_cursors::observed();
        let visited = tree.get_root().preorder().count();
        assert!(visited > 40, "fixture is too small to prove much");
        assert_eq!(
            child_scan_cursors::observed(),
            before,
            "preorder built a cursor per node; it holds one for the walk (#1112)"
        );

        // The Python instance-attribute scan walks every method body of
        // a class. Before #1112 it was 92 % of the metric walk's child
        // scans on the Python corpus slice — one per node under the
        // class. It is not the only scan a `metrics()` call makes, so
        // the bound is a fraction of the node count rather than zero.
        // Measured on this fixture: 18 scans over 81 nodes with the
        // cursor hoisted, 91 without, so the bound separates the two
        // with room on both sides.
        let source = "class C:\n    def a(self):\n        self.x = 1\n        self.y = [1, 2]\n\
                      \n    def b(self):\n        self.z, self.w = 1, 2\n        \
                      if self.x:\n            self.v = self.y\n";
        let ast = crate::test_support::parse_named(crate::LANG::Python, "c.py", source);
        let nodes = ast.root_node().preorder().count();
        let before = child_scan_cursors::observed();
        ast.metrics(crate::MetricsOptions::default())
            .expect("the walk must yield a top-level space");
        let scans = child_scan_cursors::observed() - before;
        assert!(nodes > 60, "fixture is too small to prove much");
        assert!(
            scans < nodes / 2,
            "the Python metric walk built {scans} cursors over {nodes} nodes; the \
             instance-attribute scan is meant to hold one for the subtree (#1112)"
        );

        // The suppression scan is a full-tree DFS of its own: 0 scans
        // over this fixture's 29 nodes with the cursor hoisted, 29
        // without.
        let parser = crate::langs::RustParser::new(
            b"// bca: suppress(cognitive)\nfn f() { if a { g(1, 2); } }\n".to_vec(),
            std::path::Path::new("lib.rs"),
            None,
        );
        let nodes = parser.root().preorder().count();
        let before = child_scan_cursors::observed();
        let markers = crate::suppression::suppression_markers(&parser);
        let scans = child_scan_cursors::observed() - before;
        assert_eq!(markers.len(), 1, "fixture carries one marker");
        assert!(nodes > 20, "fixture is too small to prove much");
        assert!(
            scans < nodes / 2,
            "the suppression scan built {scans} cursors over {nodes} nodes (#1112)"
        );

        // The `Search` walk, `act_on_node`. The counter records in
        // `children()`, the allocating form, so a walk that hoists its
        // cursor records nothing at all and a per-node one records once
        // per interior node. Asserting the exact zero is what tells a
        // hoisted cursor from a per-node one; a bound like `< nodes / 2`
        // would hold for either on a small fixture.
        let tree = Tree::new::<MozjsCode>(
            b"function f(a) { return { g: (b) => b + 1, h: [1, 2, 3] }; }\nf(2);\n",
        );
        let root = tree.get_root();
        let nodes = root.preorder().count();
        assert!(nodes > 30, "fixture is too small to prove much");

        let before = child_scan_cursors::observed();
        let mut seen = 0_usize;
        root.act_on_node(&mut |_, _| seen += 1);
        let scans = child_scan_cursors::observed() - before;
        assert_eq!(seen, nodes, "act_on_node must visit every node");
        assert_eq!(
            scans, 0,
            "act_on_node built {scans} cursors over {nodes} nodes; it holds one \
             for the walk (#1112)"
        );
    }

    /// [`Node::parent_grandparent_match`] must answer `false` when
    /// either link is missing, rather than degrading to a
    /// single-predicate check.
    ///
    /// Its doc states that invariant and Python's `Cyclomatic` `else`
    /// arm depends on it, but nothing exercised either absent-link
    /// return: every call in the suite runs on a node that has both a
    /// parent and a grandparent. Both predicates answer `true` here, so
    /// a `false` result can only come from the missing link — an
    /// implementation that skipped the second `climb.next()` and
    /// returned `parent_pred`'s answer would pass every other test and
    /// fail this one.
    ///
    /// Checked through both `Ancestors` constructors: the chain and the
    /// climb reach the end by different code paths (`split_last` on an
    /// empty slice, versus `Node::parent` returning `None`).
    #[test]
    fn parent_grandparent_match_is_false_when_either_link_is_absent() {
        let tree = Tree::new::<crate::langs::CCode>(b"int main() { int a; }");
        let root = tree.get_root();
        let child = root.children().next().expect("the file has an item");
        let grandchild = child
            .children()
            .next()
            .expect("the function definition has children");
        let yes: fn(&Node) -> bool = |_| true;

        // No parent at all: the root, reached either way.
        assert!(!root.parent_grandparent_match(Ancestors::unknown(), yes, yes));
        assert!(!root.parent_grandparent_match(Ancestors::known(&[]), yes, yes));

        // A parent but no grandparent: a direct child of the root.
        assert!(!child.parent_grandparent_match(Ancestors::unknown(), yes, yes));
        assert!(!child.parent_grandparent_match(Ancestors::known(&[root]), yes, yes));

        // Both links present, so the same predicates now answer `true`.
        // Without this the assertions above would also hold for a
        // function that always returned `false`.
        assert!(grandchild.parent_grandparent_match(Ancestors::unknown(), yes, yes));
        assert!(grandchild.parent_grandparent_match(Ancestors::known(&[root, child]), yes, yes));
    }

    /// [`Node::children_with`] must yield exactly what
    /// [`Node::children`] yields — same nodes, same order, same
    /// `ExactSizeIterator` length at every step — for every node of a
    /// real tree.
    ///
    /// Checked against the raw `child(i)` walk rather than against
    /// `children()`: the two iterators share [`ChildScan`], so a
    /// comparison between them would pass just as happily if the shared
    /// step were wrong. It also covers the reuse itself — one cursor
    /// drives every node's scan here, so a `reset` that failed to rewind
    /// would show as the second node inheriting the first's position.
    #[test]
    fn children_with_yields_exactly_what_children_does() {
        let code = b"const o = { m: (a) => a + 1, n: function () {} }; foo(); ;";
        let tree = Tree::new::<MozjsCode>(code);
        let root = tree.get_root();

        let mut cursor = root.cursor();
        let mut leaves = 0;
        let mut widest = 0;
        for node in root.preorder() {
            // Ground truth is the raw `child(i)` walk, not `children()`.
            // The two iterators share `ChildScan`, so checking one
            // against the other would pass just as happily if the shared
            // step were wrong.
            let raw = node.as_tree_sitter();
            let expected: Vec<_> = (0..raw.child_count() as u32)
                .filter_map(|i| raw.child(i))
                .map(|c| (c.id(), c.kind_id()))
                .collect();

            let actual = drain_checking_exact_size(
                node.children_with(&mut cursor),
                expected.len(),
                "children_with",
                node.kind(),
            );
            assert_eq!(
                actual,
                expected,
                "children_with diverged from the child(i) walk at kind {}",
                node.kind()
            );

            leaves += usize::from(expected.is_empty());
            widest = widest.max(expected.len());
        }
        // Both ends of the arity range, else the comparison could hold
        // over nothing but one-child wrappers.
        assert!(leaves > 0, "fixture must contain childless nodes");
        assert!(widest > 2, "fixture must contain a multi-child node");
    }

    /// The `O(1)` guard [`Ancestors::checked`] keeps on by default must
    /// accept every chain a walker really builds — over several grammar
    /// families, not just the one fixture a failure would surface in.
    ///
    /// The assertion that an equal-span pair was seen is what makes the
    /// containment non-strict on purpose rather than by luck: a
    /// single-child wrapper (`expression_statement` over its expression,
    /// say) spans exactly what its child spans, so tightening either
    /// bound to `<` would reject a correct chain on most real input.
    #[test]
    fn checked_accepts_the_chains_the_walkers_build() {
        let mut equal_span_pairs = 0;
        let mut check = |node: &Node<'_>, chain: &[Node<'_>]| {
            let _ = Ancestors::checked(chain, node);
            if let Some(parent) = chain.last()
                && parent.start_byte() == node.start_byte()
                && parent.end_byte() == node.end_byte()
            {
                equal_span_pairs += 1;
            }
        };
        let visited = for_each_node_with_chain::<crate::langs::CCode>(
            b"int main() { if (a) { int x; } else { f(a, b); } }",
            &mut check,
        ) + for_each_node_with_chain::<crate::langs::JavascriptCode>(
            b"const o = { m: (a) => a + 1 };\nfoo.bar();\n",
            &mut check,
        ) + for_each_node_with_chain::<crate::langs::PythonCode>(
            b"def f(a):\n    if a:\n        return [x for x in a]\n",
            &mut check,
        );

        assert!(visited > 60, "fixtures are too small to prove much");
        assert!(
            equal_span_pairs > 0,
            "no parent spans exactly what its child does, so this fixture set \
             cannot tell non-strict containment from strict"
        );
    }

    /// A `push` moved ahead of the per-node computes leaves the node
    /// itself as `chain.last()`. Spans alone cannot see that — a node
    /// contains itself — so the identity half of the guard is what
    /// catches it.
    ///
    /// Debug-gated because `debug_assert!` compiles out under
    /// `--release`, where `checked` degrades to `known` by design.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "ancestor chain desynchronised")]
    fn checked_rejects_a_chain_ending_in_the_node_itself() {
        let tree = Tree::new::<crate::langs::CCode>(b"int main() { int a; }");
        let body = tree
            .get_root()
            .preorder()
            .find(|n| n.kind() == "compound_statement")
            .expect("fixture has a function body");
        let _ = Ancestors::checked(std::slice::from_ref(&body), &body);
    }

    /// A dropped `truncate` leaves the previous subtree's path in place,
    /// so the next node up gets a `chain.last()` from a sibling subtree —
    /// disjoint from it in bytes. That is the containment half.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "ancestor chain desynchronised")]
    fn checked_rejects_a_chain_from_a_disjoint_subtree() {
        let tree = Tree::new::<crate::langs::CCode>(b"int main() { int a; int b; }");
        let body = tree
            .get_root()
            .preorder()
            .find(|n| n.kind() == "compound_statement")
            .expect("fixture has a function body");
        let declarations: Vec<Node<'_>> = body
            .children()
            .filter(|n| n.kind() == "declaration")
            .collect();
        assert_eq!(declarations.len(), 2, "fixture has two declarations");
        // `int a;` neither contains nor equals `int b;`.
        let _ = Ancestors::checked(&declarations[..1], &declarations[1]);
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
