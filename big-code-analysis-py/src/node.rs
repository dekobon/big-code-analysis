//! Lazy `Node` handle over the tree retained by [`PyAst`] (#728).
//!
//! A [`PyNode`] is a py-tree-sitter-style cursor into the parsed tree: it
//! exposes `kind`, byte offsets, points, `children`, `child_by_field_name`,
//! `text`, a pre-order `walk()`, and `descendants_by_kind()` **without**
//! materialising the tree into Python dicts the way [`PyAst::dump`] does. A
//! structural consumer that visits a small fraction of nodes pays only for
//! the nodes it touches, and an existing py-tree-sitter matcher ports over
//! unchanged because the kinds are the *raw*, unaltered grammar kinds and
//! the points are 0-based.
//!
//! # Coordinate systems
//!
//! Each node carries its one location in every vocabulary a caller might
//! already use, so nothing has to be converted by hand:
//!
//! * `start_byte` / `end_byte` — byte offsets into [`PyAst::source`];
//! * `start_point` / `end_point` — **0-based** `(row, col)` tuples, exact
//!   py-tree-sitter parity;
//! * `start_line` / `end_line` and the `span` dict — **1-based**, matching
//!   `dump()`'s span and the rest of the bca surface.
//!
//! So `start_line == start_point[0] + 1` and `start_col == start_point[1] +
//! 1`.
//!
//! # Safety
//!
//! `tree_sitter::Node<'tree>` borrows its `Tree`, but a `#[pyclass]` cannot
//! carry a lifetime, so [`PyNode`] stores a `Node<'static>` whose lifetime
//! brand has been erased by [`detach`]. This is sound — and is exactly
//! py-tree-sitter's own architecture (a `TSNode` value plus a strong
//! reference to the tree object) — because of three invariants:
//!
//! 1. **Keep-alive.** Every `PyNode` (and the [`PyNodeWalk`] iterator) holds
//!    a strong `Py<PyAst>`. The `Ast` — and therefore the `Tree` and the
//!    source bytes — cannot be dropped while any node handle exists. `PyAst`
//!    holds no back-reference to nodes, so there is no reference cycle for
//!    the GC to break.
//! 2. **Allocation stability.** `tree_sitter::Tree` owns the C tree through
//!    a `NonNull<TSTree>`; moving the Rust `Tree`/`Ast`/`PyAst` never moves
//!    the C allocation, and a `Node`'s interior `*const TSTree` points at
//!    that stable allocation. (The `PyAst` is also frozen and lives behind
//!    `Py`, so it does not move regardless.)
//! 3. **Immutability.** `Ast` is a documented, edit-less snapshot — it has
//!    no `tree_sitter::InputEdit` / incremental-reparse API. Outstanding
//!    nodes therefore stay valid for the tree's whole life.
//!
//! Invariant 3 is load-bearing and invisible to the compiler: adding an
//! incremental-reparse API to `Ast` would invalidate outstanding `TSNode`s
//! and silently break this module's soundness. Any such future change must
//! revisit [`detach`].
//!
//! The same three invariants cover [`PyNodeWalk`]'s `TreeCursor<'static>`,
//! which is derived from an already-erased node and lives no longer than
//! the iterator's own keep-alive.
//!
//! `tree_sitter::Tree`, `Node`, and `TreeCursor` are `Send + Sync` under
//! the pinned `=0.26.13`, so the pyclasses are sendable (no `unsendable`)
//! and compose with `ThreadPoolExecutor` fan-out like [`PyAst`] itself.
//! That version literal is a premise, not decoration — the layouts and
//! the `&mut self` on `Tree::edit` are what make the argument above
//! checkable, and only at a named release. `utils/check-safety-doc-pin.py`
//! fails the build when the pin moves and this line does not (#1057).
//!
//! ## What has been checked
//!
//! The invariants above were probed against a built `_native.abi3.so`,
//! not argued from the source alone. Nothing below broke; it is recorded
//! here because a future auditor of this `unsafe` block cannot cheaply
//! re-derive it (#1057).
//!
//! * `Node` is not GC-tracked. It declares no `__traverse__`, so
//!   `Py_TPFLAGS_HAVE_GC` is unset and the collector has no mechanism to
//!   clear a keep-alive; `PyAst` holds no Python-object fields, so a
//!   cycle through one is not constructible either.
//! * No Python-reachable constructor exists — `Node.__new__` rejects, and
//!   copy / deepcopy / pickle all raise — so a `Node` cannot exist
//!   without an `Ast`. A mispaired handle is therefore unconstructible
//!   from Python, which is why the pairing is enforced at the Rust module
//!   boundary ([`owned`]) rather than by a runtime test that could only
//!   pass vacuously.
//! * [`PyNodeWalk::next_node`] bumps the refcount per yielded node rather
//!   than sharing the iterator's own handle, so a yielded node safely
//!   outlives the walk that produced it.
//! * 3,000 parse-and-drop cycles under `gc.set_threshold(1, 1, 1)` with a
//!   survivor pool, and 8 threads x 4,000 iterations plus 6 threads
//!   racing `next()` on a shared `NodeWalk`: no crash, no
//!   `PyBorrowMutError`.
//!
//! The residual risk is source-level — a future Rust change — rather than
//! input-level, which is what invariant 3 says from the other direction.

use big_code_analysis::tree_sitter::{Node as TsNode, TreeCursor};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use crate::ast::PyAst;

/// Erase the lifetime *brand* on a `tree_sitter::Node`.
///
/// `Node<'tree>` is `(ffi::TSNode, PhantomData<&'tree ()>)` — the lifetime
/// is a phantom marker only; the wrapped `TSNode` is a 32-byte `Copy` value
/// that borrows nothing. The cast changes no bits.
///
/// # Safety
///
/// The returned node is valid only while the `Tree` that produced `node`
/// outlives it. Every caller upholds this by storing the node alongside a
/// strong `Py<PyAst>` that keeps that `Tree` alive — see the module-level
/// soundness argument.
unsafe fn detach<'a>(node: TsNode<'a>) -> TsNode<'static> {
    // SAFETY: lifetimes do not affect layout and the brand borrows nothing,
    // so this only relabels the phantom lifetime; the keep-alive `Py<PyAst>`
    // upholds the validity precondition.
    unsafe { std::mem::transmute::<TsNode<'a>, TsNode<'static>>(node) }
}

/// Push `node`'s children onto `stack` in reverse document order, so the
/// next `pop` yields the leftmost child — a pre-order step without a
/// temporary. Shared by the lazy [`PyNodeWalk`] and the eager
/// [`PyNode::descendants_by_kind`], the same shape as the Rust
/// `Node::preorder` iterator.
///
/// The cursor is the caller's rather than one built here: `TreeCursor`
/// heap-allocates its stack and frees it on drop, so building one per
/// visited node costs a `malloc`/`free` pair per node. This mirrors what
/// #1112 did to the Rust `Node::preorder` via `Node::children_with`.
/// `tree_sitter::Node::children` reseats the cursor itself, so a caller
/// only has to hold it.
fn push_children_for_preorder(
    stack: &mut Vec<TsNode<'static>>,
    cursor: &mut TreeCursor<'static>,
    node: TsNode<'static>,
) {
    let first_child = stack.len();
    stack.extend(node.children(cursor));
    stack[first_child..].reverse();
}

/// Which relative of a node a getter asks for.
#[derive(Clone, Copy)]
enum Relative {
    Parent,
    Next { named: bool },
    Prev { named: bool },
}

/// The visible parent of the zero-width `node`, found by walking down
/// from `root` rather than through `ts_node_parent`.
///
/// In the pinned runtime, `ts_node_parent` asks
/// `ts_node_child_with_descendant`, which calls itself once per tree
/// level when the node it looks for is zero-width. Asking for the parent
/// of a `MISSING` node some 15,000 levels deep overflowed the
/// interpreter's stack and killed it (#1937). This walk keeps its place
/// in a cursor and enters only nodes whose span holds the node's
/// position, so it costs the siblings along that one path.
fn zero_width_parent<'t>(root: TsNode<'t>, node: TsNode<'_>) -> Option<TsNode<'t>> {
    let at = node.start_byte();
    let holds = |here: &TsNode<'_>| here.start_byte() <= at && at <= here.end_byte();
    let mut cursor = root.walk();
    while cursor.node().id() != node.id() {
        let entered = holds(&cursor.node()) && cursor.goto_first_child();
        if !entered && !skip_subtree(&mut cursor) {
            return None;
        }
    }
    cursor.goto_parent().then(|| cursor.node())
}

/// The visible parent of `node` in the tree rooted at `root`: tree-sitter's
/// own lookup for a node with width, [`zero_width_parent`] for a
/// zero-width one, on which `ts_node_parent` recurses once per level (#1937).
fn parent_within<'t>(root: TsNode<'t>, node: TsNode<'t>) -> Option<TsNode<'t>> {
    if node.start_byte() == node.end_byte() {
        zero_width_parent(root, node)
    } else {
        node.parent()
    }
}

/// Moves `cursor` past the subtree it sits on, to the next node in
/// pre-order: its next sibling, or the next sibling of the nearest
/// ancestor that has one. `false` once the walk has left the tree.
fn skip_subtree(cursor: &mut TreeCursor<'_>) -> bool {
    while !cursor.goto_next_sibling() {
        if !cursor.goto_parent() {
            return false;
        }
    }
    true
}

/// The child of `parent` before `node` and the one after it, counting only
/// named children when `named`, read off `parent` rather than through
/// tree-sitter's sibling lookups, which ask `ts_node_parent` first.
fn neighbours<'t>(
    parent: TsNode<'t>,
    node: TsNode<'_>,
    named: bool,
) -> (Option<TsNode<'t>>, Option<TsNode<'t>>) {
    let counts = |child: &TsNode<'_>| !named || child.is_named();
    let mut cursor = parent.walk();
    let mut children = parent.children(&mut cursor);
    let mut before = None;
    while let Some(child) = children.next() {
        if child.id() == node.id() {
            return (before, children.find(counts));
        }
        if counts(&child) {
            before = Some(child);
        }
    }
    (None, None)
}

/// Ownership boundary for the erased-lifetime pair (#1057).
///
/// Both pyclasses pair an erased `Node<'static>` / `TreeCursor<'static>`
/// with the `Py<PyAst>` that keeps the owning tree alive, and that
/// pairing *is* [`detach`]'s precondition: a handle paired with the
/// wrong `Ast` outlives its tree and dereferences freed memory. Rust
/// field privacy alone does not enforce it — `PyNode { ast, node }`
/// compiles anywhere in the defining module, which is how three
/// construction sites accumulated on one precondition. Fields private
/// to *this* module make the struct literal unavailable to the
/// `#[pymethods]` blocks outside it, so every handle is built by one of
/// the constructors below and the pairing is structural rather than
/// reviewed. The accessors are the price.
///
/// Everything except [`PyNode::wrap`] is `pub(super)`, not
/// `pub(crate)`: `wrap` is the one entry point another module needs
/// (`ast.rs` builds the root handle), while `node` + `rewrap` together
/// *are* the mispairing primitive — `a.rewrap(py, b.node())` pairs one
/// tree's node with another tree's keep-alive. Keeping them file-local
/// is what makes the boundary above true beyond this file.
mod owned {
    use pyo3::prelude::*;

    use super::{PyAst, TreeCursor, TsNode, detach, push_children_for_preorder};

    /// A lazy handle to one node of a parsed [`Ast`](big_code_analysis::Ast).
    ///
    /// Reached from [`Ast.root_node`](PyAst) or [`Ast.find`](PyAst), then walked
    /// with `children` / `parent` / `walk()` / `descendants_by_kind()`. Holds a
    /// strong reference to its `Ast`, so it stays valid even after the caller
    /// drops every other reference to the parse.
    #[pyclass(name = "Node", module = "big_code_analysis._native", frozen)]
    pub(crate) struct PyNode {
        // Keep-alive: owns the `Ast` (Tree + source) the erased node borrows.
        ast: Py<PyAst>,
        node: TsNode<'static>,
    }

    impl PyNode {
        /// Wrap `node`, erasing its lifetime brand and keeping its owning
        /// `Ast` alive through `ast`.
        ///
        /// The one place the pairing is a *caller* obligation: `node` must
        /// have come from the tree held by `ast`. The callers are
        /// [`PyAst::root_node`] / [`PyAst::find`], which pass a node from
        /// the very `Ast` they are wrapping, and `PyNode::relative`, which
        /// passes a node it found by walking down from that `Ast`'s root
        /// for a zero-width node's relatives. Every other handle in this
        /// module is derived from an existing one by [`rewrap`](Self::rewrap)
        /// or [`PyNodeWalk::next_node`], which carry the pairing over
        /// instead of restating it.
        pub(crate) fn wrap(ast: Py<PyAst>, node: TsNode<'_>) -> Self {
            // SAFETY: `ast` is the very `Ast` whose `Tree` produced `node`, and
            // it is stored here for the whole life of the returned `PyNode`, so
            // the erased node never outlives its tree.
            let node = unsafe { detach(node) };
            Self { ast, node }
        }

        /// Re-wrap a sibling/child node discovered during navigation, sharing
        /// this node's keep-alive `Ast`. The input already carries the erased
        /// `'static` brand (it came from `self.node`), so no further `detach`
        /// is needed — only a refcount bump on the `Ast`.
        ///
        /// Callers pass a node reached from `self.node` by a `tree_sitter`
        /// navigation call, so it belongs to the same tree by construction.
        pub(super) fn rewrap(&self, py: Python<'_>, node: TsNode<'static>) -> Self {
            Self {
                ast: self.ast.clone_ref(py),
                node,
            }
        }

        /// The keep-alive handle to the `Ast` this node borrows from.
        pub(super) fn ast(&self) -> &Py<PyAst> {
            &self.ast
        }

        /// The erased node. `Node<'static>` is `Copy`, so this hands out a
        /// value rather than a borrow — it is only valid while `self` (and
        /// therefore [`ast`](Self::ast)) is alive.
        pub(super) fn node(&self) -> TsNode<'static> {
            self.node
        }
    }

    /// Lazy pre-order iterator over a node and its descendants, returned by
    /// [`PyNode::walk`].
    ///
    /// Holds a work stack of not-yet-visited erased nodes plus the keep-alive
    /// `Py<PyAst>`. Each `__next__` pops the next node, pushes its children
    /// (leftmost on top), and yields the popped node — pre-order, one node at a
    /// time, so traversal never materialises the whole subtree at once.
    ///
    /// The `TreeCursor` is held for the whole walk rather than built per
    /// step, for [`push_children_for_preorder`]'s reason. It is branded
    /// `'static` by the same erasure as the nodes it enumerates, and stays
    /// valid for the same reason: `ast` keeps the owning parse alive.
    ///
    /// Field order is load-bearing. Rust drops fields in declaration order,
    /// so `cursor` is declared *before* the `ast` that keeps its tree alive
    /// — otherwise the keep-alive would be released first and the doc claim
    /// above would be false by construction. Today's `Drop for TreeCursor`
    /// only frees the cursor's own stack and never reads the tree, so the
    /// current order is not unsound; this makes the stated invariant hold
    /// regardless, and survives a future tree-sitter destructor that does
    /// touch it. (`Vec<TsNode>` has no drop glue, so `cursor` is the only
    /// field that raises the question.)
    #[pyclass(name = "NodeWalk", module = "big_code_analysis._native")]
    pub(crate) struct PyNodeWalk {
        cursor: TreeCursor<'static>,
        stack: Vec<TsNode<'static>>,
        ast: Py<PyAst>,
    }

    impl PyNodeWalk {
        /// Root a walk at `node`, sharing its keep-alive `Ast`.
        ///
        /// Takes the `PyNode` rather than a `(Ast, node)` pair, so the
        /// cursor, the seed node, and the keep-alive all come from one
        /// already-valid handle: there is no argument to get wrong.
        pub(super) fn rooted_at(node: &PyNode, py: Python<'_>) -> Self {
            Self {
                cursor: node.node.walk(),
                stack: vec![node.node],
                ast: node.ast.clone_ref(py),
            }
        }

        /// Pop the next node in pre-order, pushing its children first.
        ///
        /// Lives here rather than in `__next__` because it is the walk's
        /// only reader of `stack` / `cursor` / `ast`, and the node it
        /// yields is paired with the `Ast` the walk was rooted at.
        pub(super) fn next_node(&mut self, py: Python<'_>) -> Option<PyNode> {
            let node = self.stack.pop()?;
            push_children_for_preorder(&mut self.stack, &mut self.cursor, node);
            // Each yielded node carries its own keep-alive `Py<PyAst>`: it may
            // outlive this iterator, so the per-node refcount bump is required
            // for soundness, not an optimisation to hoist out of the loop.
            Some(PyNode {
                ast: self.ast.clone_ref(py),
                node,
            })
        }
    }
}

pub(crate) use owned::{PyNode, PyNodeWalk};

impl PyNode {
    /// `relative` of this node. A node with width keeps tree-sitter's own
    /// lookup, which is iterative; a zero-width one, such as a `MISSING`
    /// node, finds its parent from the root instead ([`zero_width_parent`],
    /// #1937) and reads its siblings off that parent.
    fn relative(&self, py: Python<'_>, relative: Relative) -> Option<PyNode> {
        let node = self.node();
        if node.start_byte() != node.end_byte() {
            let found = match relative {
                Relative::Parent => node.parent(),
                Relative::Next { named: false } => node.next_sibling(),
                Relative::Next { named: true } => node.next_named_sibling(),
                Relative::Prev { named: false } => node.prev_sibling(),
                Relative::Prev { named: true } => node.prev_named_sibling(),
            };
            return found.map(|n| self.rewrap(py, n));
        }
        let ast = self.ast().bind(py).get();
        let parent = zero_width_parent(ast.ast_ref().root_node().as_tree_sitter(), node)?;
        let found = match relative {
            Relative::Parent => Some(parent),
            Relative::Next { named } => neighbours(parent, node, named).1,
            Relative::Prev { named } => neighbours(parent, node, named).0,
        };
        found.map(|n| PyNode::wrap(self.ast().clone_ref(py), n))
    }
}

#[pymethods]
impl PyNode {
    /// The raw grammar kind of this node (e.g. `"function_item"`).
    ///
    /// This is the *unaltered* tree-sitter kind, not the `Alterator`-curated
    /// kind [`PyAst::dump`] emits — the two intentionally disagree on
    /// altered nodes (string literals, etc.).
    #[getter]
    fn kind(&self) -> &'static str {
        self.node().kind()
    }

    /// py-tree-sitter-compatible alias for [`kind`](PyNode::kind).
    ///
    /// `kind` stays the canonical bca-vocabulary name (it matches the Rust
    /// `Node::kind()` and the CLI `count` / `dump` vocabulary); `type` is the
    /// py-tree-sitter spelling so an existing matcher ports over unchanged.
    #[getter]
    fn r#type(&self) -> &'static str {
        self.node().kind()
    }

    /// The numeric grammar id behind [`kind`](PyNode::kind).
    #[getter]
    fn kind_id(&self) -> u16 {
        self.node().kind_id()
    }

    /// Whether this node is a *named* grammar production (as opposed to an
    /// anonymous token such as punctuation or a keyword literal).
    #[getter]
    fn is_named(&self) -> bool {
        self.node().is_named()
    }

    /// Whether this node is an `ERROR` node produced by the parser.
    #[getter]
    fn is_error(&self) -> bool {
        self.node().is_error()
    }

    /// Whether this node is a zero-width `MISSING` node the parser inserted
    /// to recover from a syntax error.
    #[getter]
    fn is_missing(&self) -> bool {
        self.node().is_missing()
    }

    /// Whether this node is an `extra` (a node that may appear anywhere,
    /// such as a comment in most grammars).
    #[getter]
    fn is_extra(&self) -> bool {
        self.node().is_extra()
    }

    /// Whether this node or any node beneath it is an error or missing node.
    #[getter]
    fn has_error(&self) -> bool {
        self.node().has_error()
    }

    /// Start byte offset (inclusive) into [`PyAst::source`].
    #[getter]
    fn start_byte(&self) -> usize {
        self.node().start_byte()
    }

    /// End byte offset (exclusive) into [`PyAst::source`].
    #[getter]
    fn end_byte(&self) -> usize {
        self.node().end_byte()
    }

    /// 0-based `(row, column)` of the node's start — py-tree-sitter parity.
    #[getter]
    fn start_point(&self) -> (usize, usize) {
        let p = self.node().start_position();
        (p.row, p.column)
    }

    /// 0-based `(row, column)` of the node's end — py-tree-sitter parity.
    #[getter]
    fn end_point(&self) -> (usize, usize) {
        let p = self.node().end_position();
        (p.row, p.column)
    }

    /// 1-based start line — matches `dump()`'s span and the bca `*_line`
    /// vocabulary (`start_line == start_point[0] + 1`).
    #[getter]
    fn start_line(&self) -> usize {
        self.node().start_position().row + 1
    }

    /// 1-based end line (`end_line == end_point[0] + 1`).
    #[getter]
    fn end_line(&self) -> usize {
        self.node().end_position().row + 1
    }

    /// The node's span as the same 1-based dict `dump()` emits:
    /// `{start_line, start_col, end_line, end_col, start_byte, end_byte}`.
    #[getter]
    fn span<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let start = self.node().start_position();
        let end = self.node().end_position();
        let dict = PyDict::new(py);
        dict.set_item("start_line", start.row + 1)?;
        dict.set_item("start_col", start.column + 1)?;
        dict.set_item("end_line", end.row + 1)?;
        dict.set_item("end_col", end.column + 1)?;
        dict.set_item("start_byte", self.node().start_byte())?;
        dict.set_item("end_byte", self.node().end_byte())?;
        Ok(dict)
    }

    /// The grammar field name through which this node's parent reaches it
    /// (e.g. `"declarator"`), or `None` for the root and for nodes their
    /// parent reaches without a field.
    #[getter]
    fn field_name(&self, py: Python<'_>) -> Option<&'static str> {
        let node = self.node();
        let ast = self.ast().bind(py).get();
        let parent = parent_within(ast.ast_ref().root_node().as_tree_sitter(), node)?;
        let id = node.id();
        let mut cursor = parent.walk();
        if !cursor.goto_first_child() {
            return None;
        }
        loop {
            if cursor.node().id() == id {
                return cursor.field_name();
            }
            if !cursor.goto_next_sibling() {
                return None;
            }
        }
    }

    /// The number of direct children (named and anonymous).
    #[getter]
    fn child_count(&self) -> usize {
        self.node().child_count()
    }

    /// The number of direct *named* children.
    #[getter]
    fn named_child_count(&self) -> usize {
        self.node().named_child_count()
    }

    /// All direct children (named and anonymous), in document order.
    #[getter]
    fn children(&self, py: Python<'_>) -> Vec<PyNode> {
        let mut cursor = self.node().walk();
        self.node()
            .children(&mut cursor)
            .map(|c| self.rewrap(py, c))
            .collect()
    }

    /// The direct *named* children, in document order.
    #[getter]
    fn named_children(&self, py: Python<'_>) -> Vec<PyNode> {
        let mut cursor = self.node().walk();
        self.node()
            .named_children(&mut cursor)
            .map(|c| self.rewrap(py, c))
            .collect()
    }

    /// This node's parent, or `None` at the root.
    #[getter]
    fn parent(&self, py: Python<'_>) -> Option<PyNode> {
        self.relative(py, Relative::Parent)
    }

    /// The next sibling (named or anonymous), or `None`.
    #[getter]
    fn next_sibling(&self, py: Python<'_>) -> Option<PyNode> {
        self.relative(py, Relative::Next { named: false })
    }

    /// The previous sibling (named or anonymous), or `None`.
    #[getter]
    fn prev_sibling(&self, py: Python<'_>) -> Option<PyNode> {
        self.relative(py, Relative::Prev { named: false })
    }

    /// The next *named* sibling, or `None`.
    #[getter]
    fn next_named_sibling(&self, py: Python<'_>) -> Option<PyNode> {
        self.relative(py, Relative::Next { named: true })
    }

    /// The previous *named* sibling, or `None`.
    #[getter]
    fn prev_named_sibling(&self, py: Python<'_>) -> Option<PyNode> {
        self.relative(py, Relative::Prev { named: true })
    }

    /// The child at `index` (named and anonymous children counted), or
    /// `None` when out of range.
    #[pyo3(signature = (index, /))]
    fn child(&self, py: Python<'_>, index: usize) -> Option<PyNode> {
        u32::try_from(index)
            .ok()
            .and_then(|i| self.node().child(i))
            .map(|c| self.rewrap(py, c))
    }

    /// The *named* child at `index`, or `None` when out of range.
    #[pyo3(signature = (index, /))]
    fn named_child(&self, py: Python<'_>, index: usize) -> Option<PyNode> {
        u32::try_from(index)
            .ok()
            .and_then(|i| self.node().named_child(i))
            .map(|c| self.rewrap(py, c))
    }

    /// The first child reached through the grammar field `name`, or `None`.
    #[pyo3(signature = (name, /))]
    fn child_by_field_name(&self, py: Python<'_>, name: &str) -> Option<PyNode> {
        self.node()
            .child_by_field_name(name)
            .map(|c| self.rewrap(py, c))
    }

    /// Every child reached through the grammar field `name`, in order.
    #[pyo3(signature = (name, /))]
    fn children_by_field_name(&self, py: Python<'_>, name: &str) -> Vec<PyNode> {
        let mut cursor = self.node().walk();
        self.node()
            .children_by_field_name(name, &mut cursor)
            .map(|c| self.rewrap(py, c))
            .collect()
    }

    /// The grammar field name through which this node reaches its child at
    /// `index`, or `None`.
    #[pyo3(signature = (index, /))]
    fn field_name_for_child(&self, index: usize) -> Option<&'static str> {
        u32::try_from(index)
            .ok()
            .and_then(|i| self.node().field_name_for_child(i))
    }

    /// This node's source text, the `source[start_byte:end_byte]` slice of
    /// [`PyAst::source`] (raw bytes, exactly like py-tree-sitter's
    /// `node.text` property).
    ///
    /// This re-slices the source bytes on each read; cache the result if it
    /// is hot.
    #[getter]
    fn text<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let source = self.ast().bind(py).get().ast_ref().source();
        // Tree-sitter guarantees the span lies within the source, but slice
        // defensively rather than risk a panic on a malformed input.
        let slice = source
            .get(self.node().start_byte()..self.node().end_byte())
            .unwrap_or(&[]);
        PyBytes::new(py, slice)
    }

    /// A lazy pre-order iterator over this node and all its descendants
    /// (this node first, then each child subtree left to right).
    ///
    /// Yields [`Node`](PyNode) handles one at a time, so a caller can stop
    /// early without materialising the whole subtree — the memory payoff of
    /// the lazy surface. Mirrors the Rust `Node::preorder` (#728).
    fn walk(&self, py: Python<'_>) -> PyNodeWalk {
        PyNodeWalk::rooted_at(self, py)
    }

    /// Collect every node in this subtree (this node included) whose
    /// [`kind`](PyNode::kind) is in `kinds`, in pre-order.
    ///
    /// Matches the *raw* grammar kind exactly. Mirrors the Rust
    /// `Node::descendants_by_kind` (#728); for a whole-tree search with the
    /// richer `bca count` filter vocabulary, use [`Ast.find`](PyAst).
    #[pyo3(signature = (kinds, /))]
    // PyO3 `FromPyObject` materializes an owned `Vec<String>`; there is no
    // borrowed `&[String]` to take instead (mirrors `Ast.find` / `count`).
    #[allow(clippy::needless_pass_by_value)]
    fn descendants_by_kind(&self, py: Python<'_>, kinds: Vec<String>) -> Vec<PyNode> {
        let mut out = Vec::new();
        let mut stack = vec![self.node()];
        // One cursor for the whole subtree, not one per visited node.
        let mut cursor = self.node().walk();
        while let Some(node) = stack.pop() {
            // Only matches pay the `rewrap` (a keep-alive refcount bump), so
            // a selective filter does not allocate a handle per visited node.
            if kinds.iter().any(|k| k == node.kind()) {
                out.push(self.rewrap(py, node));
            }
            push_children_for_preorder(&mut stack, &mut cursor, node);
        }
        out
    }

    /// Two nodes are equal iff they come from the same `Ast` object and
    /// refer to the same tree-sitter node id — stable for the tree's life.
    ///
    /// The `as_ptr()` identity check is sound precisely because each
    /// `PyNode` holds a strong `Py<PyAst>`: a live node's `Ast` cannot be
    /// deallocated, so its address cannot be recycled for a *different*
    /// `Ast`. Pointer equality therefore implies the same `Ast` instance.
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<PyRef<'_, PyNode>>().is_ok_and(|o| {
            self.ast().as_ptr() == o.ast().as_ptr() && self.node().id() == o.node().id()
        })
    }

    /// Hash consistent with [`__eq__`](PyNode::__eq__): the `(Ast object
    /// identity, node id)` pair.
    // A Python hash legitimately wraps `u64` into the platform `isize`; only
    // stability and equality-consistency matter here, not a faithful cast.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn __hash__(&self) -> isize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self.ast().as_ptr() as usize).hash(&mut hasher);
        self.node().id().hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __repr__(&self) -> String {
        let start = self.node().start_position();
        format!(
            "Node(kind='{}', start_point=({}, {}), byte_range={}..{})",
            self.node().kind(),
            start.row,
            start.column,
            self.node().start_byte(),
            self.node().end_byte(),
        )
    }
}

#[pymethods]
impl PyNodeWalk {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> Option<PyNode> {
        self.next_node(py)
    }
}

#[cfg(test)]
mod tests {
    use big_code_analysis::{Ast, LANG, Source};

    use super::{TsNode, neighbours, zero_width_parent};

    /// `void f(){x = ((…()…));}`, `depth` parentheses deep: the innermost
    /// pair holds a zero-width `MISSING` identifier.
    fn nested_missing(depth: usize) -> Ast {
        let code = format!(
            "void f(){{x = {}{};}}\n",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        Ast::parse(Source::new(LANG::C, code.as_bytes())).expect("C is a default feature")
    }

    /// Every zero-width node of `root`'s tree, found on an explicit stack.
    fn zero_width_nodes(root: TsNode<'_>) -> Vec<TsNode<'_>> {
        let mut found = Vec::new();
        let mut pending = vec![root];
        let mut cursor = root.walk();
        while let Some(node) = pending.pop() {
            if node.start_byte() == node.end_byte() {
                found.push(node);
            }
            pending.extend(node.children(&mut cursor));
        }
        found
    }

    /// Where tree-sitter's own lookups are safe, at a shallow depth, the
    /// walk from the root answers what they answer, for every zero-width
    /// node (#1937).
    #[test]
    fn the_root_walk_agrees_with_tree_sitter_where_it_is_safe() {
        let ast = nested_missing(6);
        let root = ast.root_node().as_tree_sitter();
        let nodes = zero_width_nodes(root);
        assert!(!nodes.is_empty(), "the input must hold a zero-width node");
        for node in nodes {
            let parent = zero_width_parent(root, node);
            assert_eq!(parent, node.parent(), "{}", node.kind());
            let parent = parent.expect("a zero-width node below the root has a parent");
            assert_eq!(
                neighbours(parent, node, false),
                (node.prev_sibling(), node.next_sibling())
            );
            assert_eq!(
                neighbours(parent, node, true),
                (node.prev_named_sibling(), node.next_named_sibling())
            );
        }
    }

    /// 20,000 levels above the `MISSING` node, far past the depth at which
    /// `ts_node_parent` overflowed the interpreter's main-thread stack, on
    /// a 512 KiB one. expected: the innermost `parenthesized_expression`,
    /// whose `(` and `)` flank the node.
    #[test]
    fn a_deep_zero_width_node_finds_its_parent_on_a_small_stack() {
        // Parentheses around the `MISSING` node.
        const DEPTH: usize = 20_000;
        // A deliberately small test-thread stack.
        const SMALL_STACK: usize = 512 * 1024;
        std::thread::Builder::new()
            .stack_size(SMALL_STACK)
            .spawn(|| {
                let ast = nested_missing(DEPTH);
                let root = ast.root_node().as_tree_sitter();
                let missing = zero_width_nodes(root)
                    .into_iter()
                    .find(TsNode::is_missing)
                    .expect("the innermost pair holds a MISSING node");
                let parent = zero_width_parent(root, missing).expect("a parent");
                assert_eq!(parent.kind(), "parenthesized_expression");
                let (before, after) = neighbours(parent, missing, false);
                assert_eq!(before.map(|n| n.kind()), Some("("));
                assert_eq!(after.map(|n| n.kind()), Some(")"));
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("the root walk must not overflow a 512 KiB stack");
    }
}
