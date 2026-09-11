// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

//! Node finding by kind or category.

use crate::node::{Ancestors, Node};

use crate::error::MetricsError;
use crate::traits::ParserTrait;

/// Finds the types of nodes specified in the input slice. Crate-internal
/// walk core reached through the `big_code_analysis::Ast::find` seam.
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
// propagated unchanged through `Ast::find`;
// `unnecessary_wraps` would have us drop it and break that uniform
// `?`-able shape across the walk cores.
#[allow(clippy::unnecessary_wraps)]
pub fn find<'a, T: ParserTrait>(
    parser: &'a T,
    filters: &[String],
) -> Result<Vec<Node<'a>>, MetricsError> {
    let filters = parser.filters(filters);
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let mut good = Vec::new();
    // The ancestry of whichever node is about to be popped. A predicate
    // that asks about an enclosing construct — `"function"` for the
    // JS family and Elixir, `"string"` for the Tcl family — would
    // otherwise climb with `Node::parent`, which restarts at the root
    // and so costs `O(depth)` *per lookup*: `bca find --type string`
    // over 8 KB of nested Tcl braces took 823 ms that way and 5 ms with
    // the chain (#1381, the shape of #1052 / #1122).
    //
    // Maintained exactly as `spaces::compute::metrics_inner` does —
    // truncate to the node's depth before the visit, push after it —
    // so `Ancestors::checked` has the same meaning here as there.
    let mut chain: Vec<Node<'a>> = Vec::new();

    stack.push((node, 0_usize));

    while let Some((node, depth)) = stack.pop() {
        chain.truncate(depth);
        if filters.any(&node, Ancestors::checked(&chain, &node)) {
            good.push(node);
        }
        chain.push(node);
        // Source order in, tail reversed in place, so the LIFO `stack`
        // yields the leftmost child first — matches were already
        // returned in source order and must stay that way.
        let first_child = stack.len();
        stack.extend(node.children_with(&mut cursor).map(|c| (c, depth + 1)));
        stack[first_child..].reverse();
    }
    Ok(good)
}
