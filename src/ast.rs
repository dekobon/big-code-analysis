// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::if_not_else, clippy::wildcard_imports)]

use serde::{Deserialize, Serialize};

use crate::*;

/// Start and end positions of a node in a code in terms of lines, columns,
/// and byte offsets.
///
/// Serialized as a flat object
/// `{start_line, start_col, end_line, end_col, start_byte, end_byte}`. The
/// line/column pairs are 1-based; the byte offsets are 0-based half-open
/// (`[start_byte, end_byte)`) indices into the parsed source bytes
/// ([`Ast::source`](crate::Ast::source)). The `*_line` vocabulary aligns the
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
    pub children: Vec<AstNode>,
}

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

fn build<T: ParserTrait>(parser: &T, span: bool, comment: bool) -> Option<AstNode> {
    // Iterative depth-first walk that materializes `AstNode`s bottom-up.
    // Each frame holds the pending parent node, the grammar field name
    // through which its own parent reached it (None for the root), the
    // already-materialized child `AstNode`s, and the next child index to
    // descend into. The parent's `field_name_for_child(idx)` lookup is
    // O(1) and avoids the parallel cursor walk that was required when
    // field names had to be captured via `TreeCursor::field_name()`.
    struct Frame<'a> {
        node: crate::Node<'a>,
        field: Option<&'static str>,
        children: Vec<AstNode>,
        next_child_index: usize,
    }

    let code = parser.code();
    let root = parser.root();
    let mut stack: Vec<Frame<'_>> = vec![Frame {
        node: root,
        field: None,
        children: Vec::with_capacity(root.child_count()),
        next_child_index: 0,
    }];

    loop {
        let frame = stack
            .last_mut()
            .expect("stack invariant: loop only runs while stack is non-empty");
        let child_count = frame.node.child_count();
        if frame.next_child_index < child_count {
            let idx = frame.next_child_index;
            frame.next_child_index += 1;
            // `Node::child` is O(1) (direct tree-sitter pointer
            // arithmetic); `field_name_for_child` returns the static
            // grammar field for that child position. Tree-sitter caps
            // child indices at u32, so the cast is safe by invariant.
            let child = frame
                .node
                .child(idx)
                .expect("stack invariant: idx < child_count so the child exists");
            let field = frame.node.field_name_for_child(
                u32::try_from(idx).expect("invariant: tree-sitter caps child indices at u32::MAX"),
            );
            stack.push(Frame {
                node: child,
                field,
                children: Vec::with_capacity(child.child_count()),
                next_child_index: 0,
            });
        } else {
            let frame = stack
                .pop()
                .expect("stack invariant: just observed non-empty via last_mut()");
            let node = T::Checker::get_ast_node(
                &frame.node,
                code,
                span,
                comment,
                frame.field,
                frame.children,
            );
            match (node, stack.last_mut()) {
                (Some(ast), Some(parent)) => parent.children.push(ast),
                (Some(ast), None) => return Some(ast),
                (None, None) => return None,
                (None, Some(_)) => {}
            }
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

/// Build the AST dump for `parser` under `cfg`. Backs [`crate::Ast::dump`];
/// the AST-extraction analogue of [`crate::spaces::metrics_inner`] /
/// [`crate::ops::ops_inner`].
pub(crate) fn dump_inner<T: ParserTrait>(parser: &T, cfg: AstCfg) -> AstResponse {
    AstResponse {
        id: cfg.id,
        language: cfg.language,
        root: build(parser, cfg.span, cfg.comment),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn build_ast<P: ParserTrait>(code: &[u8], filename: &str) -> AstNode {
        let path = PathBuf::from(filename);
        let parser = P::new(code.to_vec(), &path, None);
        let cfg = AstCfg {
            id: String::new(),
            language: String::new(),
            comment: false,
            span: false,
        };
        dump_inner(&parser, cfg)
            .root
            .expect("parser should produce a root AST node")
    }

    fn build_ast_with_span<P: ParserTrait>(code: &[u8], filename: &str) -> AstNode {
        let path = PathBuf::from(filename);
        let parser = P::new(code.to_vec(), &path, None);
        let cfg = AstCfg {
            id: String::new(),
            language: String::new(),
            comment: false,
            span: true,
        };
        dump_inner(&parser, cfg)
            .root
            .expect("parser should produce a root AST node")
    }

    fn find_first<'a>(node: &'a AstNode, kind: &str) -> Option<&'a AstNode> {
        if node.r#type == kind {
            return Some(node);
        }
        node.children.iter().find_map(|c| find_first(c, kind))
    }

    fn find_child<'a>(parent: &'a AstNode, field: &str) -> Option<&'a AstNode> {
        parent.children.iter().find(|c| c.field_name == Some(field))
    }

    #[test]
    fn root_has_no_field_name() {
        let root = build_ast::<crate::RustParser>(b"fn main() {}", "test.rs");
        assert_eq!(root.field_name, None);
    }

    #[test]
    fn rust_assignment_carries_left_and_right_field_names() {
        // `assignment_expression` in the Rust grammar names its operands
        // `left` and `right`. Without `FieldName` exposed in the JSON,
        // downstream consumers cannot distinguish the two `identifier`
        // children. This is the canonical example from issue #244.
        let root =
            build_ast::<crate::RustParser>(b"fn f() { let mut a = 0; a = a + 1; }", "test.rs");
        let assign = find_first(&root, "assignment_expression")
            .expect("expected an assignment_expression node");
        let left = find_child(assign, "left").expect("expected a `left` child");
        let right = find_child(assign, "right").expect("expected a `right` child");
        assert_eq!(left.field_name, Some("left"));
        assert_eq!(right.field_name, Some("right"));
        // Anonymous `=` token is a child too, with no field name.
        assert!(
            assign
                .children
                .iter()
                .any(|c| c.r#type == "=" && c.field_name.is_none()),
            "expected the `=` token child to carry no field name; got {:?}",
            assign
                .children
                .iter()
                .map(|c| (c.r#type, c.field_name))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn rust_function_carries_name_and_body_field_names() {
        // `function_item` names children `name`, `parameters`, `body`.
        // Assert the field name directly on the AstNode so a bug that
        // misnames a field (e.g. always emits "body") fails even if
        // the target node kinds coincidentally line up.
        let root =
            build_ast::<crate::RustParser>(b"fn greet(name: &str) -> &str { name }", "test.rs");
        let func = find_first(&root, "function_item").expect("expected a function_item node");
        let name_child = find_child(func, "name").expect("function_item should have a name child");
        assert_eq!(name_child.field_name, Some("name"));
        assert_eq!(name_child.r#type, "identifier");
        let params_child =
            find_child(func, "parameters").expect("function_item should have a parameters child");
        assert_eq!(params_child.field_name, Some("parameters"));
        assert_eq!(params_child.r#type, "parameters");
        let body_child = find_child(func, "body").expect("function_item should have a body child");
        assert_eq!(body_child.field_name, Some("body"));
        assert_eq!(body_child.r#type, "block");
    }

    #[test]
    fn cpp_assignment_carries_left_and_right_field_names() {
        // Cross-language confirmation: the C/C++ grammar uses the same
        // `left`/`right` field names for `assignment_expression`.
        let root =
            build_ast::<crate::CppParser>(b"int main(){ int x = 0; x = x + 1; }", "test.cpp");
        let assign = find_first(&root, "assignment_expression")
            .expect("expected an assignment_expression node");
        assert_eq!(
            find_child(assign, "left").map(|n| n.r#type),
            Some("identifier")
        );
        assert_eq!(
            find_child(assign, "right").map(|n| n.r#type),
            Some("binary_expression")
        );
    }

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

    #[test]
    fn span_serializes_as_named_object() {
        // The span is a flat named object preserving the 1-based
        // tree-sitter line/column values in the original tuple order
        // (start_line, start_col, end_line, end_col). The `*_line`
        // vocabulary matches /function and /metrics (#638).
        // `fn f(){}` is 8 bytes on one line, so the root spans the whole
        // half-open byte range [0, 8) (#727).
        let root = build_ast_with_span::<crate::RustParser>(b"fn f(){}", "test.rs");
        let span = root
            .span
            .expect("root span present when span tracking is on");
        assert_eq!(span, Span::new(1, 1, 1, 9, 0, 8));
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
}
