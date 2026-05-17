// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::if_not_else, clippy::wildcard_imports)]

use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

use crate::*;

/// Start and end positions of a node in a code in terms of rows and columns.
///
/// The first and second fields represent the row and column associated to
/// the start position of a node.
///
/// The third and fourth fields represent the row and column associated to
/// the end position of a node.
pub type Span = Option<(usize, usize, usize, usize)>;

/// The payload of an `Ast` request.
#[derive(Debug, Deserialize, Serialize)]
pub struct AstPayload {
    /// The id associated to a request for an `AST`
    pub id: String,
    /// The filename associated to a source code file
    pub file_name: String,
    /// The code to be represented as an `AST`
    pub code: String,
    /// If `true`, nodes representing comments are ignored
    pub comment: bool,
    /// If `true`, the start and end positions of a node in a code
    /// are considered
    pub span: bool,
}

/// The response of an `AST` request.
#[derive(Debug, Serialize)]
pub struct AstResponse {
    /// The id associated to a request for an `AST`
    pub id: String,
    /// The root node of an `AST`
    ///
    /// If `None`, an error has occurred
    pub root: Option<AstNode>,
}

/// Information on an `AST` node.
#[derive(Debug)]
pub struct AstNode {
    /// The type of node
    pub r#type: &'static str,
    /// The code associated to a node
    pub value: String,
    /// The start and end positions of a node in a code
    pub span: Span,
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

impl Serialize for AstNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("Node", 5)?;
        st.serialize_field("Type", &self.r#type)?;
        st.serialize_field("TextValue", &self.value)?;
        st.serialize_field("Span", &self.span)?;
        st.serialize_field("FieldName", &self.field_name)?;
        st.serialize_field("Children", &self.children)?;
        st.end()
    }
}

impl AstNode {
    /// Builds an `AstNode` with the supplied type, value, span, and
    /// children. The `field_name` is set to `None`; use
    /// [`AstNode::with_field_name`] to record the tree-sitter grammar
    /// field through which the parent reaches this node.
    #[must_use]
    pub fn new(r#type: &'static str, value: String, span: Span, children: Vec<AstNode>) -> Self {
        Self::with_field_name(r#type, value, span, None, children)
    }

    /// Builds an `AstNode` carrying the tree-sitter grammar field name
    /// (`left`, `right`, `name`, `body`, ...) through which the parent
    /// reaches this node.
    #[must_use]
    pub fn with_field_name(
        r#type: &'static str,
        value: String,
        span: Span,
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

    let code = parser.get_code();
    let root = parser.get_root();
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
                frame.children,
                span,
                comment,
                frame.field,
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

/// Type tag identifying the AST extraction action; carries no data.
pub struct AstCallback {
    _guard: (),
}

/// Configuration options for retrieving the nodes of an `AST`.
#[derive(Debug)]
pub struct AstCfg {
    /// The id associated to a request for an `AST`
    pub id: String,
    /// If `true`, nodes representing comments are ignored
    pub comment: bool,
    /// If `true`, the start and end positions of a node in a code
    /// are considered
    pub span: bool,
}

impl Callback for AstCallback {
    type Res = AstResponse;
    type Cfg = AstCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        AstResponse {
            id: cfg.id,
            root: build(parser, cfg.span, cfg.comment),
        }
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
            comment: false,
            span: false,
        };
        AstCallback::call(cfg, &parser)
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
        let root =
            build_ast::<crate::RustParser>(b"fn greet(name: &str) -> &str { name }", "test.rs");
        let func = find_first(&root, "function_item").expect("expected a function_item node");
        assert_eq!(
            find_child(func, "name").map(|n| n.r#type),
            Some("identifier"),
        );
        assert_eq!(
            find_child(func, "parameters").map(|n| n.r#type),
            Some("parameters"),
        );
        assert_eq!(find_child(func, "body").map(|n| n.r#type), Some("block"),);
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
        // Regression for the Serialize impl: every node must serialize
        // a `FieldName` key (null or string). Verifying via JSON
        // string-match catches accidental removal of the field from
        // the serializer.
        let root = build_ast::<crate::RustParser>(b"fn f(){ let a = 1; }", "test.rs");
        let json = serde_json::to_string(&root).expect("serialize");
        assert!(
            json.contains("\"FieldName\""),
            "FieldName missing from JSON: {json}"
        );
        // The let binding's `pattern` and `value` fields should both
        // appear as string values in the JSON.
        assert!(
            json.contains("\"FieldName\":\"pattern\""),
            "expected pattern field name; got {json}"
        );
        assert!(
            json.contains("\"FieldName\":\"value\""),
            "expected value field name; got {json}"
        );
    }
}
