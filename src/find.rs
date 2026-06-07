// bca: suppress-file(halstead)
// Per-language node-find dispatch; file-level halstead is a many-fn
// aggregation artifact, not per-function logic complexity.

// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::node::Node;

use crate::error::MetricsError;
use crate::traits::ParserTrait;

/// Finds the types of nodes specified in the input slice. Crate-internal
/// walk core reached through the [`crate::Ast::find`] seam.
///
/// "No matches" is represented by `Ok(Vec::new())` rather than an
/// error — it is a normal outcome, not a failure mode. The
/// [`Result`] return type is for forward compatibility with the
/// other entry points; today no [`MetricsError`] variant is produced
/// by `find`, but a future strict-parsing mode may surface one here.
///
/// # Errors
///
/// Currently infallible; the [`Result`] wrapper aligns the signature
/// with the other walk cores so callers can use the `?` operator
/// uniformly.
// The `Result` is deliberate forward-compat (see doc above) and is
// propagated unchanged through `AstInner::run_find` / `Ast::find`;
// `unnecessary_wraps` would have us drop it and break that uniform
// `?`-able shape across the walk cores.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn find<'a, T: ParserTrait>(
    parser: &'a T,
    filters: &[String],
) -> Result<Vec<Node<'a>>, MetricsError> {
    let filters = parser.filters(filters);
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let mut good = Vec::new();
    let mut children = Vec::new();

    stack.push(node);

    while let Some(node) = stack.pop() {
        if filters.any(&node) {
            good.push(node);
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
    Ok(good)
}
