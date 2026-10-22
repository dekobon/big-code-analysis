// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::if_not_else, clippy::wildcard_imports)]

//! The AST dump: [`AstNode`], [`Span`], [`AstCfg`], [`AstResponse`], and
//! the walk that builds them.

use std::ops::ControlFlow;

use serde::{Deserialize, Serialize};

use crate::*;

/// Start and end positions of a node in a code in terms of lines, columns,
/// and byte offsets.
///
/// Serialized as a flat object
/// `{start_line, start_col, end_line, end_col, start_byte, end_byte}`. The
/// line/column pairs are 1-based; the byte offsets are 0-based half-open
/// (`[start_byte, end_byte)`) indices into the parsed source bytes
/// (`big_code_analysis::Ast::source`). The `*_line` vocabulary aligns the
/// `/ast` span field names with the `/function` and `/metrics` endpoints
/// (`start_line` / `end_line`), so a client correlating spans across
/// endpoints no longer special-cases `*_row` vs `*_line` per endpoint
/// (#638). The former `start_row` / `end_row` keys were renamed as a
/// `2.0`-line break.
///
/// The byte offsets let structural consumers slice the original source for a
/// node — including internal nodes, whose `value` text the dump omits — so a
/// caller can recover any subtree's exact bytes without re-deriving offsets
/// from lines and columns (#727). They mirror tree-sitter's own
/// `Node::start_byte` / `Node::end_byte`.
///
/// The struct is `#[non_exhaustive]`: construct it through [`Span::new`] and
/// read its public fields, but do not rely on struct-literal construction or
/// exhaustive destructuring from outside the crate. This is the last planned
/// shape break to the type. The two byte fields carry `#[serde(default)]` so
/// span objects serialized before they existed still deserialize (the
/// offsets default to `0`).
///
/// A node's span is `None` for the root and any node when span tracking is
/// disabled; in that case the wrapping `Option<Span>` serializes as `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Span {
    /// Line of the start position (1-based).
    pub start_line: usize,
    /// Column of the start position (1-based).
    pub start_col: usize,
    /// Line of the end position (1-based).
    pub end_line: usize,
    /// Column of the end position (1-based).
    pub end_col: usize,
    /// Byte offset of the node's first byte in the source (0-based).
    #[serde(default)]
    pub start_byte: usize,
    /// Byte offset one past the node's last byte in the source (0-based,
    /// exclusive).
    #[serde(default)]
    pub end_byte: usize,
}

impl Span {
    /// Builds a [`Span`] from 1-based line/column pairs and 0-based,
    /// half-open byte offsets (`[start_byte, end_byte)`).
    ///
    /// This is the supported construction path now that the struct is
    /// `#[non_exhaustive]`.
    #[must_use]
    pub fn new(
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        start_byte: usize,
        end_byte: usize,
    ) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
            start_byte,
            end_byte,
        }
    }
}

/// The payload of an `Ast` request.
///
/// Unknown fields are rejected with a deserialization error naming the
/// offending key, so a typo'd field cannot silently change request
/// semantics (#633). The web boundary renders that as a `400` carrying the
/// `unknown_field` `error_kind` token.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AstPayload {
    /// The id associated to a request for an `AST`.
    ///
    /// Optional on the wire (#645): an omitted `id` deserializes to the
    /// empty string, which every downstream surface already treats as
    /// "no correlation id". Defaulting it stops clients eating a `400`
    /// for a field whose absence has an obvious meaning.
    #[serde(default)]
    pub id: String,
    /// The filename associated to a source code file
    pub file_name: String,
    /// The code to be represented as an `AST`
    pub code: String,
    /// If `true`, nodes representing comments are ignored. Optional on
    /// the wire (#645): omitting it defaults to `false`, matching the
    /// `bool` default and the most common request shape.
    #[serde(default)]
    pub comment: bool,
    /// If `true`, the start and end positions of a node in a code
    /// are considered. Optional on the wire (#645): omitting it defaults
    /// to `false`.
    #[serde(default)]
    pub span: bool,
}

/// The response of an `AST` request.
///
/// The envelope echoes the resolved `language` slug alongside `id` and
/// `root`, matching the `/function`, `/comment`, and `/metrics` analysis
/// endpoints (#654). AST node kinds are grammar-specific, so an `/ast`
/// consumer most needs to confirm which grammar actually parsed the
/// source. `language` is the #540 canonical lowercase slug (the same value
/// the sibling endpoints emit). The added field is a `2.0`-line shape
/// change to this published library type (STABILITY.md).
#[derive(Debug, Serialize)]
pub struct AstResponse {
    /// The id associated to a request for an `AST`
    pub id: String,
    /// The resolved source-language slug that produced this tree (#654).
    ///
    /// The #540 canonical lowercase slug (e.g. `cpp`, `python`), matching
    /// the other analysis endpoints' `language` echo.
    pub language: String,
    /// The root node of an `AST`
    ///
    /// If `None`, an error has occurred
    pub root: Option<AstNode>,
}

/// Greatest AST depth [`AstNode`] will serialize.
///
/// An `AstNode` tree is as deep as the parsed expression nesting, which a
/// caller controls directly — 100 000 levels fit in a 200 KB payload, well
/// inside `bca-web`'s body cap. `serde` cannot emit a tree without one
/// native stack frame per level, and overflowing that stack aborts the
/// process rather than raising a catchable panic (#1056), so the depth is
/// bounded: a deeper tree fails serialization with an ordinary serializer
/// error naming the limit.
///
/// The bound is set well clear of real source: the deepest AST across the
/// ~8 000-file corpus under `tests/repositories` (TensorFlow, serde,
/// DeepSpeech, …) is 188 levels. It is also set well clear of the stack:
/// the earliest measured overflow of any emitted format was 2 000 levels
/// on a debug build's default 2 MiB thread.
///
/// Read that 188 as a corpus figure, not a conversion rate. The depth
/// counted here is the **`AstNode` tree after [`Alterator::alterate`]**,
/// and how many levels one level of *source* nesting costs is a
/// per-language property of what that alterator flattens. The Tcl family
/// is the outlier: since #1381 a script body keeps its children, so one
/// brace level spans `braced_word` → `command` → `word_list` and costs
/// three levels here — 196 levels over 64 nested `eval` braces, measured
/// by `a_tcl_brace_level_costs_three_ast_levels` below — putting the
/// ceiling near 170 nested braces rather than 512. Nothing in the corpus
/// approaches either number — it holds no Tcl at all — but a caller
/// re-deriving this bound from the 188 alone would be reasoning about
/// the wrong quantity.
///
/// [`Alterator::alterate`]: crate::alterator::Alterator::alterate
pub const MAX_AST_SERIALIZE_DEPTH: usize = 512;

/// Serializes an [`AstNode`]'s children one level deeper, refusing to
/// descend past [`MAX_AST_SERIALIZE_DEPTH`].
fn serialize_children<S: serde::Serializer>(
    children: &[AstNode],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    crate::recursion::serialize_bounded(children, MAX_AST_SERIALIZE_DEPTH, "AstNode", serializer)
}

/// Information on an `AST` node.
///
/// Serialized as a flat object with `snake_case` keys: `type`, `value`,
/// `span`, `field_name`, `children`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AstNode {
    /// The type of node
    pub r#type: &'static str,
    /// The code associated to a node
    pub value: String,
    /// The start and end positions of a node in a code
    pub span: Option<Span>,
    /// Tree-sitter grammar field name through which the parent reaches
    /// this node (e.g. `left`, `right`, `name`, `body`).
    ///
    /// `None` for the root node, anonymous tokens (punctuation, keywords),
    /// and any child that does not occupy a named grammar field. Consumers
    /// of the JSON output rely on this to distinguish structurally
    /// equivalent children without grammar-specific positional knowledge.
    pub field_name: Option<&'static str>,
    /// The children of a node
    #[serde(serialize_with = "serialize_children")]
    pub children: Vec<AstNode>,
}

// AST depth is caller-controlled — 100 000 levels of nested parentheses
// fit in a 200 KB payload — so the compiler-generated `Drop` glue would
// recurse once per level and abort the process (#1056). See
// [`crate::recursion`].
crate::recursion::impl_iterative_drop!(AstNode, children);

impl AstNode {
    /// Builds an `AstNode` with the supplied type, value, span, and
    /// children. The `field_name` is set to `None`; use
    /// [`AstNode::with_field_name`] to record the tree-sitter grammar
    /// field through which the parent reaches this node.
    #[must_use]
    pub fn new(
        r#type: &'static str,
        value: String,
        span: Option<Span>,
        children: Vec<AstNode>,
    ) -> Self {
        Self::with_field_name(r#type, value, span, None, children)
    }

    /// Builds an `AstNode` carrying the tree-sitter grammar field name
    /// (`left`, `right`, `name`, `body`, ...) through which the parent
    /// reaches this node.
    #[must_use]
    pub fn with_field_name(
        r#type: &'static str,
        value: String,
        span: Option<Span>,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> Self {
        Self {
            r#type,
            value,
            span,
            field_name,
            children,
        }
    }
}

// Entries into each export builder, on this thread.
//
// The two builders produce the same tree — `build_cursor` exists
// because enumerating a flat `ERROR`'s children by index is quadratic
// in its width, not because it answers differently — so no assertion
// on a dump can tell which one `dump_inner` routed to. Rerouting the
// default back to the indexed walk would pass every output test and
// show up only as a slow `/ast` response on a recovery tree; the
// counters are what make it a test failure instead (#1684).
crate::observation::counter!(cursor_builds, cfg(any(test, feature = "test-support")));
crate::observation::counter!(indexed_builds, cfg(any(test, feature = "test-support")));

/// The pending state of one node while its subtree is being exported,
/// bottom-up: the node, the grammar field its parent reached it
/// through (`None` for the root), the children exported so far, and —
/// read by [`build_indexed`] alone — the index of the next child to
/// enter.
struct Frame<'a> {
    node: crate::Node<'a>,
    field: Option<&'static str>,
    children: Vec<AstNode>,
    next_child: usize,
}

impl<'a> Frame<'a> {
    fn new(node: crate::Node<'a>, field: Option<&'static str>) -> Self {
        Self {
            node,
            field,
            children: Vec::with_capacity(node.child_count()),
            next_child: 0,
        }
    }
}

/// What both builders share: the frame stack and the ancestor chain
/// that mirrors it, so the two cannot disagree on how a node is
/// finished.
///
/// `chain` mirrors `stack`'s nodes, so popping both leaves `chain`
/// holding exactly the popped node's ancestry. `Alterator::keeps_children`
/// is the only consumer, and it is the reason this exists rather than
/// climbing with `Node::parent`: that lookup is `O(depth)` and asking
/// it per node made `Ast::dump` quadratic in nesting depth — 10 ms
/// against 418 ms on 8 KB of nested Tcl braces, measured by swapping
/// this for `Ancestors::unknown()` (#1381, the shape of #1052 / #1122).
///
/// That swap is invisible to the test suite: an unknown chain answers
/// identically and only costs more, so nothing here pins the chain by
/// asserting on output. Three things do. `Ancestors::checked` in
/// [`Export::finish`] carries a `debug_assert!` that catches a dropped
/// pop on every fixture the dump walk sees, and `make chain-audit`
/// upgrades it to the exact `chain.last() == node.parent()` check —
/// the only one that sees a dropped *push*, since a chain that never
/// grows ends at the root, which contains every node. The default
/// build pins the push through `keeps_children` instead, in
/// `each_strategy_hands_the_alterator_the_real_parent` below. And the
/// swap itself is priced by `dump/tcl-nested-eval` in the benchmark
/// harness, which it takes from 0.75 to 2.00.
struct Export<'a, 'code> {
    code: &'code [u8],
    span: bool,
    comment: bool,
    stack: Vec<Frame<'a>>,
    chain: Vec<crate::Node<'a>>,
}

impl<'a, 'code> Export<'a, 'code> {
    fn new(root: crate::Node<'a>, code: &'code [u8], span: bool, comment: bool) -> Self {
        Self {
            code,
            span,
            comment,
            stack: vec![Frame::new(root, None)],
            chain: vec![root],
        }
    }

    /// Descends into `child`, reached through `field`.
    fn enter(&mut self, child: crate::Node<'a>, field: Option<&'static str>) {
        self.stack.push(Frame::new(child, field));
        self.chain.push(child);
    }

    /// Exports the node on top of the stack — every child of it has
    /// been exported by now — and attaches it to its parent's frame.
    /// Breaks with the export once the node finished was the root.
    fn finish<T: ParserTrait>(&mut self) -> ControlFlow<Option<AstNode>> {
        let frame = self
            .stack
            .pop()
            .expect("stack invariant: finish is called only while a frame is pending");
        self.chain.pop();
        let node = T::Checker::get_ast_node(
            &frame.node,
            self.code,
            self.span,
            self.comment,
            frame.field,
            frame.children,
            Ancestors::checked(&self.chain, &frame.node),
        );
        match self.stack.last_mut() {
            Some(parent) => {
                parent.children.extend(node);
                ControlFlow::Continue(())
            }
            None => ControlFlow::Break(node),
        }
    }
}

/// The export walk over one cursor, which is what `dump_inner` runs.
///
/// A cursor steps to a sibling in amortised `O(1)` whatever the
/// parent's shape — each hidden entry on its stack is pushed once and
/// popped once — and reports the field of the node it sits on from the
/// path it descended, at the cost of the hidden levels between the
/// node and its visible parent: `O(log n)` under a balanced `repeat()`,
/// `O(1)` under a flat node. The enumeration is therefore `O(n log n)`
/// over a wide balanced list and `O(n)` over a flat one, never
/// quadratic. Indexing the children instead is quadratic on a flat node
/// — see [`build_indexed`] — and that difference is the whole reason
/// there are two builders (#1684). What the alterator does per node is
/// priced separately: `keeps_children` reads the ancestor chain, which
/// `dump/tcl-nested-eval` covers, and its own predicates are the
/// language's to keep linear — Tcl's is not yet (#1686).
///
/// The cursor sits on the node of the top frame throughout. Each
/// iteration descends as far as a first child exists, then finishes
/// frames upward until a next sibling exists, which it enters, or the
/// root is finished. A frame is never re-entered: `goto_parent` lands
/// on a node whose children are all exported, and the only moves from
/// there are a sibling or another climb. The field is read while the
/// cursor is on the child, before descending further, because the
/// cursor answers for its current position only.
fn build_cursor<T: ParserTrait>(parser: &T, span: bool, comment: bool) -> Option<AstNode> {
    cursor_builds::record();
    let root = parser.root();
    let mut export = Export::new(root, parser.code(), span, comment);
    let mut cursor = root.cursor();
    loop {
        while cursor.goto_first_child() {
            export.enter(cursor.node(), cursor.field_name());
        }
        loop {
            if let ControlFlow::Break(root) = export.finish::<T>() {
                return root;
            }
            if cursor.goto_next_sibling() {
                export.enter(cursor.node(), cursor.field_name());
                break;
            }
            // The root frame is finished above before this can fail,
            // so a `false` here cannot leave the walk stranded.
            let climbed = cursor.goto_parent();
            debug_assert!(
                climbed,
                "the cursor must sit below the root while a frame is pending"
            );
        }
    }
}

/// The export walk by child index, which `dump_indexed_inner` runs.
///
/// Each frame also remembers the next child index to enter.
/// `Node::child` and `Node::field_name_for_child` both scan the
/// parent's child array to that index, skipping hidden subtrees by
/// their cached counts; see the `# Cost` note on
/// [`Node::child`](crate::Node::child) for which nodes are flat.
/// Enumerating a flat node this way is quadratic in its width, which
/// `dump/perl-error-tail-calls` in the benchmark harness prices
/// (#1684). This is kept as the explicit alternative for consumers
/// that want index-addressed export, and as the second implementation
/// the parity tests hold the default to.
fn build_indexed<T: ParserTrait>(parser: &T, span: bool, comment: bool) -> Option<AstNode> {
    indexed_builds::record();
    let root = parser.root();
    let mut export = Export::new(root, parser.code(), span, comment);
    loop {
        let frame = export
            .stack
            .last_mut()
            .expect("stack invariant: loop only runs while a frame is pending");
        let idx = frame.next_child;
        if idx < frame.node.child_count() {
            frame.next_child += 1;
            let child = frame
                .node
                .child(idx)
                .expect("stack invariant: idx < child_count so the child exists");
            let field = frame.node.field_name_for_child(
                u32::try_from(idx).expect("invariant: tree-sitter caps child indices at u32::MAX"),
            );
            export.enter(child, field);
        } else if let ControlFlow::Break(root) = export.finish::<T>() {
            return root;
        }
    }
}

/// Configuration options for retrieving the nodes of an `AST`.
#[derive(Debug)]
pub struct AstCfg {
    /// The id associated to a request for an `AST`
    pub id: String,
    /// The resolved source-language slug to echo in the response
    /// envelope (#654). The #540 canonical lowercase slug.
    pub language: String,
    /// If `true`, nodes representing comments are ignored
    pub comment: bool,
    /// If `true`, the start and end positions of a node in a code
    /// are considered
    pub span: bool,
}

/// Build the AST dump for `parser` under `cfg`. Backs `big_code_analysis::Ast::dump`;
/// the AST-extraction analogue of `big_code_analysis::spaces::metrics_inner` /
/// `big_code_analysis::ops::ops_inner`.
///
/// Walks the tree with one cursor — the same export as
/// [`dump_cursor_inner`]. [`dump_indexed_inner`] is the explicit
/// alternative that enumerates children by index; both produce the
/// same tree, and differ only in cost (#1684).
pub fn dump_inner<T: ParserTrait>(parser: &T, cfg: AstCfg) -> AstResponse {
    dump_cursor_inner(parser, cfg)
}

/// [`dump_inner`], naming its strategy: one cursor walks the tree,
/// stepping between siblings in amortised `O(1)` whatever the parent's
/// shape.
pub fn dump_cursor_inner<T: ParserTrait>(parser: &T, cfg: AstCfg) -> AstResponse {
    AstResponse {
        id: cfg.id,
        language: cfg.language,
        root: build_cursor(parser, cfg.span, cfg.comment),
    }
}

/// [`dump_inner`] by child index: each child is reached through
/// [`Node::child`](crate::Node::child) and its field through
/// [`Node::field_name_for_child`](crate::Node::field_name_for_child).
/// Same tree as the cursor walk; quadratic in the width of a flat
/// recovery node, where the cursor walk is linear (#1684).
pub fn dump_indexed_inner<T: ParserTrait>(parser: &T, cfg: AstCfg) -> AstResponse {
    AstResponse {
        id: cfg.id,
        language: cfg.language,
        root: build_indexed(parser, cfg.span, cfg.comment),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// One export entry point, generic over the parser it dumps.
    #[cfg(any(feature = "cpp", feature = "python", feature = "rust", feature = "tcl"))]
    type Dump<P> = fn(&P, AstCfg) -> AstResponse;

    /// The two explicit strategies, by name.
    #[cfg(any(feature = "cpp", feature = "python", feature = "rust", feature = "tcl"))]
    fn strategies<P: ParserTrait>() -> [(&'static str, Dump<P>); 2] {
        [
            ("cursor", dump_cursor_inner),
            ("indexed", dump_indexed_inner),
        ]
    }

    #[cfg(any(feature = "cpp", feature = "python", feature = "rust", feature = "tcl"))]
    fn cfg_with(span: bool, comment: bool) -> AstCfg {
        AstCfg {
            id: String::new(),
            language: String::new(),
            comment,
            span,
        }
    }

    #[cfg(any(feature = "cpp", feature = "python", feature = "rust", feature = "tcl"))]
    fn build_ast_by<P: ParserTrait>(
        dump: Dump<P>,
        code: &[u8],
        filename: &str,
        span: bool,
        comment: bool,
    ) -> AstNode {
        let path = PathBuf::from(filename);
        let parser = P::new(code.to_vec(), &path, None);
        dump(&parser, cfg_with(span, comment))
            .root
            .expect("parser should produce a root AST node")
    }

    /// Runs `check` on the root each strategy exports for `code`, so an
    /// anchored assertion holds against each implementation on its own.
    /// Parity between the two cannot see a defect they share, and a
    /// value asserted through the default alone says nothing about the
    /// other strategy.
    #[cfg(any(feature = "cpp", feature = "rust", feature = "tcl"))]
    fn for_each_strategy<P: ParserTrait>(
        code: &[u8],
        filename: &str,
        span: bool,
        check: impl Fn(&str, &AstNode),
    ) {
        for (name, dump) in strategies::<P>() {
            check(name, &build_ast_by(dump, code, filename, span, false));
        }
    }

    #[cfg(any(feature = "rust", feature = "tcl"))]
    fn build_ast<P: ParserTrait>(code: &[u8], filename: &str) -> AstNode {
        build_ast_by::<P>(dump_inner, code, filename, false, false)
    }

    #[cfg(feature = "rust")]
    fn build_ast_with_span<P: ParserTrait>(code: &[u8], filename: &str) -> AstNode {
        build_ast_by::<P>(dump_inner, code, filename, true, false)
    }

    /// Each direct child's `(type, field_name)`.
    #[cfg(any(feature = "python", feature = "rust"))]
    fn kinds_and_fields(node: &AstNode) -> Vec<(&'static str, Option<&'static str>)> {
        node.children
            .iter()
            .map(|c| (c.r#type, c.field_name))
            .collect()
    }

    /// How many nodes in `root`'s subtree have a type in `kinds`,
    /// counted on an explicit stack.
    #[cfg(feature = "rust")]
    fn count_kinds(root: &AstNode, kinds: &[&str]) -> usize {
        let mut pending = vec![root];
        let mut found = 0;
        while let Some(node) = pending.pop() {
            found += usize::from(kinds.contains(&node.r#type));
            pending.extend(&node.children);
        }
        found
    }

    #[cfg(any(feature = "cpp", feature = "rust"))]
    fn find_first<'a>(node: &'a AstNode, kind: &str) -> Option<&'a AstNode> {
        if node.r#type == kind {
            return Some(node);
        }
        node.children.iter().find_map(|c| find_first(c, kind))
    }

    #[cfg(any(feature = "cpp", feature = "rust"))]
    fn find_child<'a>(parent: &'a AstNode, field: &str) -> Option<&'a AstNode> {
        parent.children.iter().find(|c| c.field_name == Some(field))
    }

    #[cfg(feature = "rust")]
    #[test]
    fn root_has_no_field_name() {
        for_each_strategy::<crate::RustParser>(b"fn main() {}", "test.rs", false, |name, root| {
            assert_eq!(root.field_name, None, "{name}");
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_assignment_carries_left_and_right_field_names() {
        // `assignment_expression` in the Rust grammar names its operands
        // `left` and `right`. Without `FieldName` exposed in the JSON,
        // downstream consumers cannot distinguish the two `identifier`
        // children. This is the canonical example from issue #244.
        let code = b"fn f() { let mut a = 0; a = a + 1; }";
        for_each_strategy::<crate::RustParser>(code, "test.rs", false, |name, root| {
            let assign = find_first(root, "assignment_expression")
                .expect("expected an assignment_expression node");
            let left = find_child(assign, "left").expect("expected a `left` child");
            let right = find_child(assign, "right").expect("expected a `right` child");
            assert_eq!(left.field_name, Some("left"), "{name}");
            assert_eq!(right.field_name, Some("right"), "{name}");
            // The anonymous `=` token is a child too, with no field name,
            // and nothing else sits between the operands.
            assert_eq!(
                kinds_and_fields(assign),
                [
                    ("identifier", Some("left")),
                    ("=", None),
                    ("binary_expression", Some("right")),
                ],
                "{name}"
            );
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_function_carries_name_and_body_field_names() {
        // `function_item` names children `name`, `parameters`, `body`.
        // Assert the field name directly on the AstNode so a bug that
        // misnames a field (e.g. always emits "body") fails even if
        // the target node kinds coincidentally line up.
        let code = b"fn greet(name: &str) -> &str { name }";
        for_each_strategy::<crate::RustParser>(code, "test.rs", false, |name, root| {
            let func = find_first(root, "function_item").expect("expected a function_item node");
            let name_child =
                find_child(func, "name").expect("function_item should have a name child");
            assert_eq!(name_child.field_name, Some("name"), "{name}");
            assert_eq!(name_child.r#type, "identifier", "{name}");
            let params_child = find_child(func, "parameters")
                .expect("function_item should have a parameters child");
            assert_eq!(params_child.field_name, Some("parameters"), "{name}");
            assert_eq!(params_child.r#type, "parameters", "{name}");
            let body_child =
                find_child(func, "body").expect("function_item should have a body child");
            assert_eq!(body_child.field_name, Some("body"), "{name}");
            assert_eq!(body_child.r#type, "block", "{name}");
        });
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_assignment_carries_left_and_right_field_names() {
        // Cross-language confirmation: the C/C++ grammar uses the same
        // `left`/`right` field names for `assignment_expression`.
        let code = b"int main(){ int x = 0; x = x + 1; }";
        for_each_strategy::<crate::CppParser>(code, "test.cpp", false, |name, root| {
            let assign = find_first(root, "assignment_expression")
                .expect("expected an assignment_expression node");
            assert_eq!(
                find_child(assign, "left").map(|n| n.r#type),
                Some("identifier"),
                "{name}"
            );
            assert_eq!(
                find_child(assign, "right").map(|n| n.r#type),
                Some("binary_expression"),
                "{name}"
            );
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn serialized_json_includes_field_name_key() {
        // Regression for the Serialize derive: every node must serialize
        // a `field_name` key (null or string). Verifying via JSON
        // string-match catches accidental removal of the field from
        // the serializer.
        let root = build_ast::<crate::RustParser>(b"fn f(){ let a = 1; }", "test.rs");
        let json = serde_json::to_string(&root).expect("serialize");
        assert!(
            json.contains("\"field_name\""),
            "field_name missing from JSON: {json}"
        );
        // The let binding's `pattern` and `value` fields should both
        // appear as string values in the JSON.
        assert!(
            json.contains("\"field_name\":\"pattern\""),
            "expected pattern field name; got {json}"
        );
        assert!(
            json.contains("\"field_name\":\"value\""),
            "expected value field name; got {json}"
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn serialized_json_uses_snake_case_keys() {
        // The serialized AST shape uses snake_case keys (#535). This
        // anchors the key scheme against accidental reversion to the
        // former PascalCase `Type`/`TextValue`/`Span`/`Children`.
        let root = build_ast_with_span::<crate::RustParser>(b"fn f(){}", "test.rs");
        let json = serde_json::to_string(&root).expect("serialize");
        for key in [
            "\"type\":",
            "\"value\":",
            "\"span\":",
            "\"field_name\":",
            "\"children\":",
        ] {
            assert!(json.contains(key), "expected key {key}; got {json}");
        }
        for legacy in ["\"Type\"", "\"TextValue\"", "\"Span\"", "\"Children\""] {
            assert!(
                !json.contains(legacy),
                "unexpected PascalCase key {legacy}; got {json}"
            );
        }
    }

    #[cfg(feature = "rust")]
    #[test]
    fn span_serializes_as_named_object() {
        // The span is a flat named object preserving the 1-based
        // tree-sitter line/column values in the original tuple order
        // (start_line, start_col, end_line, end_col). The `*_line`
        // vocabulary matches /function and /metrics (#638).
        // `fn f(){}` is 8 bytes on one line, so the root spans the whole
        // half-open byte range [0, 8) (#727).
        for_each_strategy::<crate::RustParser>(b"fn f(){}", "test.rs", true, |name, root| {
            let span = root
                .span
                .expect("root span present when span tracking is on");
            assert_eq!(span, Span::new(1, 1, 1, 9, 0, 8), "{name}");
            let json = serde_json::to_string(&root.span).expect("serialize span");
            assert!(
                json.contains("\"start_line\":1")
                    && json.contains("\"start_col\":1")
                    && json.contains("\"end_line\":1")
                    && json.contains("\"end_col\":9")
                    && json.contains("\"start_byte\":0")
                    && json.contains("\"end_byte\":8"),
                "expected named span object with byte offsets; got {json}"
            );
            // The pre-2.0 `*_row` keys must be gone (#638).
            assert!(
                !json.contains("start_row") && !json.contains("end_row"),
                "unexpected pre-2.0 *_row span keys; got {json}"
            );
        });
    }

    #[test]
    fn span_round_trips_through_serde() {
        // Span derives Deserialize for wire round-trip parity.
        let span = Span::new(2, 3, 4, 5, 11, 42);
        let json = serde_json::to_string(&span).expect("serialize");
        let back: Span = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(span, back);
    }

    #[test]
    fn span_deserializes_pre_byte_offsets_with_default() {
        // The byte fields carry `#[serde(default)]`, so a span object
        // serialized before they existed (line/col only) still
        // deserializes, with the offsets defaulting to 0 (#727).
        let legacy = r#"{"start_line":2,"start_col":3,"end_line":4,"end_col":5}"#;
        let span: Span = serde_json::from_str(legacy).expect("deserialize legacy span");
        assert_eq!(span, Span::new(2, 3, 4, 5, 0, 0));
    }

    // -----------------------------------------------------------------
    // Stack-depth regression tests (#1056)
    // -----------------------------------------------------------------

    /// A chain of `depth` nested [`AstNode`]s below the root, built
    /// directly so the depth is exact rather than a function of how many
    /// AST levels a grammar spends per source construct.
    fn ast_chain(depth: usize) -> AstNode {
        let mut node = AstNode::new("leaf", String::new(), None, Vec::new());
        for _ in 0..depth {
            node = AstNode::new("branch", String::new(), None, vec![node]);
        }
        node
    }

    #[test]
    fn ast_depth_at_the_serialize_limit_is_accepted_and_one_deeper_is_not() {
        let at_limit = ast_chain(MAX_AST_SERIALIZE_DEPTH);
        serde_json::to_string(&at_limit).expect("the limit itself must serialize");

        let past_limit = ast_chain(MAX_AST_SERIALIZE_DEPTH + 1);
        let err = serde_json::to_string(&past_limit).expect_err("one deeper must be refused");
        assert!(
            err.to_string().contains(&format!(
                "AstNode nesting is deeper than the serialization limit of \
                 {MAX_AST_SERIALIZE_DEPTH} levels"
            )),
            "the error must name the type and the limit, got: {err}"
        );
    }

    /// The deepest `children` chain in a tree, measured iteratively so a
    /// pathological input cannot overflow the measurement itself.
    #[cfg(feature = "tcl")]
    fn ast_depth(root: &AstNode) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(root, 1usize)];
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            stack.extend(node.children.iter().map(|child| (child, depth + 1)));
        }
        deepest
    }

    /// What one level of Tcl brace nesting costs in `AstNode` levels, and
    /// therefore how far [`MAX_AST_SERIALIZE_DEPTH`] actually reaches for
    /// this language.
    ///
    /// The bound's doc quotes a corpus figure of 188 levels, which is a
    /// count of `AstNode` levels and not a conversion rate from source
    /// nesting. Since #1381 a Tcl script body keeps its children, so the
    /// two quantities came apart here more than anywhere else: this test
    /// is what keeps the number in that doc honest, and what would notice
    /// if an alterator change moved it again.
    #[cfg(feature = "tcl")]
    #[test]
    fn a_tcl_brace_level_costs_three_ast_levels() {
        // `eval` evaluates every argument, so each level keeps its
        // children rather than flattening to a verbatim leaf.
        let levels = 64;
        let code = format!("{}puts hi{}", "eval {".repeat(levels), "}".repeat(levels));

        let root = build_ast::<crate::TclParser>(code.as_bytes(), "deep.tcl");
        let depth = ast_depth(&root);
        let per_level = depth / levels;

        // Measured 196 over 64 at the pinned grammar: `braced_word` ->
        // `command` -> `word_list` before the next `braced_word`. The
        // range admits one level of drift either way so a grammar bump
        // reports the change rather than simply going red.
        assert!(
            (3..=4).contains(&per_level),
            "a braced script level should cost 3 AstNode levels \
             (braced_word -> command -> word_list); measured {depth} \
             levels over {levels} braces = {per_level}"
        );
        // The consequence worth stating in one place: the 512-level bound
        // is a low-hundreds brace ceiling for Tcl, not a 512 one.
        let ceiling = MAX_AST_SERIALIZE_DEPTH / per_level;
        assert!(
            (120..=180).contains(&ceiling),
            "the effective Tcl brace ceiling should sit near 170, got \
             {ceiling} from {per_level} levels per brace"
        );
    }

    // -----------------------------------------------------------------
    // Two export strategies (#1684)
    // -----------------------------------------------------------------

    /// Which builder each entry point runs.
    ///
    /// Nothing in the output can say: both builders produce the same
    /// tree, so routing `dump_inner` back to `build_indexed` would pass
    /// every assertion on a dump and show up only as a quadratic `/ast`
    /// response on a recovery tree. Each delta is asserted against a
    /// seeded count rather than zero, so a `record()` that was never
    /// wired up cannot pass either.
    #[cfg(feature = "rust")]
    #[test]
    fn each_entry_point_enters_exactly_its_own_builder() {
        let parser = crate::RustParser::new(b"fn f() {}".to_vec(), &PathBuf::from("a.rs"), None);
        let _ = dump_cursor_inner(&parser, cfg_with(false, false));
        let _ = dump_indexed_inner(&parser, cfg_with(false, false));
        let (cursor_seed, indexed_seed) = (cursor_builds::observed(), indexed_builds::observed());
        assert!(
            cursor_seed > 0 && indexed_seed > 0,
            "the seed calls must be counted"
        );

        let entries: [(&str, Dump<crate::RustParser>, usize, usize); 3] = [
            ("dump_inner", dump_inner, 1, 0),
            ("dump_cursor_inner", dump_cursor_inner, 1, 0),
            ("dump_indexed_inner", dump_indexed_inner, 0, 1),
        ];
        let (mut cursor_total, mut indexed_total) = (cursor_seed, indexed_seed);
        for (name, dump, cursor_delta, indexed_delta) in entries {
            let _ = dump(&parser, cfg_with(false, false));
            cursor_total += cursor_delta;
            indexed_total += indexed_delta;
            assert_eq!(
                (cursor_builds::observed(), indexed_builds::observed()),
                (cursor_total, indexed_total),
                "{name} must enter the cursor builder {cursor_delta} time(s) and the indexed \
                 builder {indexed_delta} time(s)"
            );
        }
    }

    /// A cursor climbs no higher than the node it was built on, and
    /// answers no field for that node: whole-tree parity cannot show
    /// either, since the walk there starts at the root, which has no
    /// parent to climb to or field to hide.
    #[cfg(feature = "rust")]
    #[test]
    fn a_cursor_reports_fields_below_its_start_and_cannot_climb_above_it() {
        let parser = crate::RustParser::new(
            b"fn f(a: u8) { let b = a; }".to_vec(),
            &PathBuf::from("a.rs"),
            None,
        );
        let func = parser
            .root()
            .children()
            .find(|n| n.kind() == "function_item")
            .expect("fixture has a function");
        let mut cursor = func.cursor();

        assert_eq!(cursor.node().id(), func.id());
        // `function_item` sits in no field of `source_file`, so this is
        // `None` for two reasons; the subtree case below is the one
        // that tells them apart.
        assert_eq!(cursor.field_name(), None);
        assert!(!cursor.goto_parent(), "the start node is the ceiling");
        assert_eq!(
            cursor.node().id(),
            func.id(),
            "a refused climb must not move"
        );

        assert!(cursor.goto_first_child());
        let mut fields = Vec::new();
        loop {
            fields.push((cursor.node().kind(), cursor.field_name()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        assert_eq!(
            fields,
            [
                ("fn", None),
                ("identifier", Some("name")),
                ("parameters", Some("parameters")),
                ("block", Some("body")),
            ],
            "fields read off the cursor must match the grammar's"
        );

        assert!(cursor.goto_parent());
        assert_eq!(cursor.node().id(), func.id());
        assert!(
            !cursor.goto_parent(),
            "back at the start node, the ceiling holds"
        );

        // A node reached by descending from a cursor started above it
        // *does* know its field, and the same node hides it again once
        // a reset makes it the start: the `None` is the ceiling, not
        // the grammar.
        let let_decl = func
            .preorder()
            .find(|n| n.kind() == "let_declaration")
            .expect("fixture has a let");
        let mut from_parent = let_decl.cursor();
        assert!(from_parent.goto_first_child() && from_parent.goto_next_sibling());
        assert_eq!(from_parent.node().kind(), "identifier");
        assert_eq!(from_parent.field_name(), Some("pattern"));
        cursor.reset(&from_parent.node());
        assert_eq!(
            cursor.node().id(),
            from_parent.node().id(),
            "the reset must move the cursor before its answer means anything"
        );
        assert_eq!(
            cursor.field_name(),
            None,
            "after a reset the new start node hides its field exactly as the original did"
        );
    }

    /// A field a visible rule inherits from a hidden one is reported,
    /// with the same name, by both strategies. The cursor reads it off
    /// the token's hidden parent on the way up and the index lookup
    /// carries it down through the same level, and the parity sweep's
    /// fixtures name no such field, so a grammar that broke the
    /// agreement would be caught only by accident there. Python's
    /// `comparison_operator` reaches each operator token through
    /// `operators`, an entry its `parser.c` marks `.inherited`; the
    /// token sits one hidden `_repeat1` level down, so this pins the
    /// propagation, not a multi-level climb.
    #[cfg(feature = "python")]
    #[test]
    fn each_strategy_reports_a_field_inherited_through_a_hidden_rule() {
        let code = b"a < b <= c\n";
        for (name, dump) in strategies::<crate::PythonParser>() {
            let root = build_ast_by(dump, code, "c.py", false, false);
            let comparison = kinds_and_fields(&root.children[0].children[0]);
            // expected: the two operator tokens carry `operators`; the
            // three operands sit in no field.
            assert_eq!(
                comparison,
                [
                    ("identifier", None),
                    ("<", Some("operators")),
                    ("identifier", None),
                    ("<=", Some("operators")),
                    ("identifier", None),
                ],
                "{name}"
            );
        }
    }

    /// The comment filter, asserted on each strategy against an
    /// independently derived expectation. The parity sweep's fixtures
    /// hold almost no comments, and the filter lives in the step both
    /// builders share, so a defect there would hold parity while
    /// dropping or keeping the wrong nodes on both.
    #[cfg(feature = "rust")]
    #[test]
    fn each_strategy_filters_comments_only_when_asked() {
        let code = b"// lead\nfn f() {\n    // inner\n    let a = 1; /* trail */\n}\n";
        for (name, dump) in strategies::<crate::RustParser>() {
            for (comment, expected) in [(false, 3), (true, 0)] {
                let root = build_ast_by(dump, code, "c.rs", true, comment);
                let comments = count_kinds(&root, &["line_comment", "block_comment"]);
                // expected: the fixture spells two `//` and one `/* */`
                // comment; with the filter on, none survives, and the
                // retained content is untouched either way.
                assert_eq!(comments, expected, "{name}, comment={comment}");
                assert_eq!(
                    count_kinds(&root, &["let_declaration"]),
                    1,
                    "{name}, comment={comment}: the `let` must survive the filter"
                );
            }
        }
    }

    /// Both envelope fields pass through each explicit entry point
    /// unchanged, with values that are not the empty-string default so
    /// a dropped or swapped field cannot read as correct.
    #[cfg(feature = "rust")]
    #[test]
    fn each_strategy_echoes_the_request_metadata() {
        let parser = crate::RustParser::new(b"fn f() {}".to_vec(), &PathBuf::from("a.rs"), None);
        let default: (&str, Dump<crate::RustParser>) = ("default", dump_inner);
        // The explicit strategies first, so a defect in one of them is
        // reported under its own name rather than as `default`.
        for (name, dump) in strategies().into_iter().chain([default]) {
            let resp = dump(
                &parser,
                AstCfg {
                    id: "req-42".to_owned(),
                    language: "rust".to_owned(),
                    comment: false,
                    span: false,
                },
            );
            assert_eq!(
                (resp.id.as_str(), resp.language.as_str()),
                ("req-42", "rust"),
                "{name}"
            );
            assert_eq!(resp.root.map(|r| r.r#type), Some("source_file"), "{name}");
        }
    }

    /// The ancestor chain each builder hands `keeps_children` names the
    /// node's real parent, on every push as well as every pop.
    ///
    /// `Ancestors::checked`'s default-build `debug_assert!` sees a
    /// dropped *pop* — the stale entry no longer contains the node —
    /// but not a dropped *push*: a chain that never grows ends at the
    /// root, which contains everything. Only `make chain-audit` asks
    /// the exact question, so this pins the push half through its one
    /// consumer. A braced word directly under a `command` is that
    /// command's *name*, a literal the alterator flattens to a leaf;
    /// the same word under the root or under a `word_list` would be a
    /// script and keep its children. `{puts} hi` is therefore a leaf
    /// only when the parent handed down really is the `command`, and
    /// `eval {puts a}` beside it is the script case, so a chain that
    /// answers the root for everything moves exactly one of the two.
    #[cfg(feature = "tcl")]
    #[test]
    fn each_strategy_hands_the_alterator_the_real_parent() {
        let code = b"{puts} hi\neval {puts a}\n";
        for_each_strategy::<crate::TclParser>(code, "name.tcl", false, |name, root| {
            let braced: Vec<(&str, Vec<&str>)> = root
                .children
                .iter()
                .flat_map(|command| command.children.iter())
                .flat_map(|word| {
                    // The command name sits directly under `command`;
                    // the script argument is one `word_list` down.
                    std::iter::once(word).chain(word.children.iter())
                })
                .filter(|node| node.r#type == "braced_word")
                .map(|node| {
                    let kinds: Vec<&str> = node.children.iter().map(|c| c.r#type).collect();
                    (node.value.as_str(), kinds)
                })
                .collect();
            // expected: the name `{puts}` is a literal leaf carrying its
            // text; the `eval` body is an internal node — no text —
            // keeping `{`, the inner `command`, and `}`.
            assert_eq!(
                braced,
                [("{puts}", vec![]), ("", vec!["{", "command", "}"])],
                "{name}"
            );
        });
    }

    /// A file whose only child is a filtered comment: the cursor walk
    /// finishes that child with nothing to attach, climbs, and must
    /// still finish the root rather than look for another sibling.
    #[cfg(feature = "rust")]
    #[test]
    fn a_comment_only_source_exports_a_childless_root_under_either_strategy() {
        let code = b"// only\n";
        for (name, dump) in strategies::<crate::RustParser>() {
            let keep = build_ast_by(dump, code, "a.rs", false, false);
            assert_eq!(
                keep.children.len(),
                1,
                "{name}: without the filter the root keeps its one comment child"
            );
            let root = build_ast_by(dump, code, "a.rs", false, true);
            assert_eq!(
                kinds_and_fields(&root),
                [],
                "{name}: the comment child must be filtered"
            );
        }
    }

    #[test]
    fn a_pathologically_deep_ast_errors_and_tears_down_without_overflowing() {
        // The chain is built directly rather than parsed, but this depth
        // is reachable through `bca-web`'s `/ast`: 50 000 levels of
        // nested parentheses fit in 100 KB, an order of magnitude inside
        // the 4 MiB body cap. Both the recursive `Serialize` and the
        // compiler-generated `Drop` glue used to abort the process here —
        // `SIGABRT`, which `spawn_blocking` cannot contain (#1056).
        const DEPTH: usize = 50_000;
        // The stack a `bca` consumer thread and a `tokio` blocking thread
        // get; the serialization limit is dimensioned against it.
        const PRODUCTION_STACK: usize = 2 * 1024 * 1024;
        let message = std::thread::Builder::new()
            .stack_size(PRODUCTION_STACK)
            .spawn(|| {
                let root = ast_chain(DEPTH);
                let message = serde_json::to_string(&root)
                    .expect_err("depth past the limit must fail, not serialize")
                    .to_string();
                // `root` drops here: the iterative `Drop` must unwind
                // 50 000 levels without touching the stack.
                message
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("bounded-stack thread must not overflow");
        assert!(
            message.contains("AstNode nesting is deeper than the serialization limit"),
            "the error must explain the refusal, got: {message}"
        );
    }
}
