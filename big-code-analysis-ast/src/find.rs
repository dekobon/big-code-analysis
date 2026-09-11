// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

//! Node finding by kind or category.

use crate::node::Node;

use crate::error::MetricsError;
use crate::traits::{ParserTrait, Search};

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
    let mut good = Vec::new();
    // `act_on_node` visits in source-order pre-order, so matches come back
    // in source order, and it hands each node its ancestor chain. The
    // `"function"` (JS family, Elixir) and `"string"` (Tcl family)
    // predicates ask about an enclosing construct; off an unknown chain
    // each lookup is `Node::parent`'s `O(depth)`, which made this walk
    // quadratic in nesting depth (#1381). Sharing the walk rather than
    // copying its truncate/push bookkeeping keeps one copy under
    // `make chain-audit`.
    parser.root().act_on_node(&mut |node, ancestors| {
        if filters.any(node, ancestors) {
            good.push(*node);
        }
    });
    Ok(good)
}
