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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::find;
    use crate::count::count;
    use crate::langs::RustParser;
    use crate::node::Node;
    use crate::traits::ParserTrait;

    // The `error` filter is `Checker::is_error`: a node that is a parse
    // error or holds one. A clean source matches nothing, which an
    // `is_error` answering `true` for every node fails. A broken one
    // matches the path from the root down to its `ERROR` and nothing
    // beside that path, which an `is_error` answering `false` fails. The
    // second source is one rustc rejects, by necessity: a tree with no
    // parse error gives the filter nothing to match.
    #[cfg(feature = "rust")]
    #[test]
    fn the_error_filter_matches_the_path_to_a_parse_error_and_nothing_else() {
        let error = ["error".to_owned()];
        let parse = |source: &str| {
            RustParser::new(source.as_bytes().to_vec(), &PathBuf::from("t.rs"), None)
        };
        let kind_and_row = |node: &Node| (node.kind(), node.start_row());

        let clean = parse("fn ok() {}\nfn also() {}\n");
        // expected: no node of a clean parse is or holds an error, so
        // `find` returns nothing and `count` tallies none of the nodes.
        let found = find(&clean, &error).expect("`find` is infallible");
        assert_eq!(found.iter().map(kind_and_row).collect::<Vec<_>>(), []);
        let (good, total) = count(&clean, &error);
        assert_eq!(good, 0);
        assert!(total > 1, "the clean fixture parsed to {total} nodes");

        let broken = parse("fn ok() {}\nfn broken() { let = ; }\n");
        // The recovered shape: `let =` is the tree's one `ERROR`, in
        // the body of the second function, and no token is invented.
        let errors: Vec<_> = broken.root().preorder().filter(Node::is_error).collect();
        let [error_node] = errors[..] else {
            panic!("the fixture must recover into one `ERROR`, got {errors:?}");
        };
        assert!(broken.root().preorder().all(|node| !node.is_missing()));
        // expected: the `ERROR` and each of its ancestors, root first,
        // which is the order `find` visits them in. `fn ok` on row 0
        // holds no error and is on no such path.
        let mut path: Vec<_> = std::iter::successors(Some(error_node), Node::parent)
            .map(|node| kind_and_row(&node))
            .collect();
        path.reverse();
        assert_eq!(
            path,
            [
                ("source_file", 0),
                ("function_item", 1),
                ("block", 1),
                ("ERROR", 1)
            ]
        );
        let found = find(&broken, &error).expect("`find` is infallible");
        assert_eq!(found.iter().map(kind_and_row).collect::<Vec<_>>(), path);
        assert_eq!(count(&broken, &error).0, path.len());
    }
}
