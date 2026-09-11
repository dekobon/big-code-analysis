// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

//! [`Parser<T>`](Parser) and the node [`Filter`]s `count` / `find` accept.

use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use crate::checker::Checker;
use crate::node::Ancestors;

use crate::alterator::Alterator;
use crate::getter::Getter;

use crate::c_macro;
use crate::langs::*;
use crate::node::{Node, Tree};
use crate::preproc::{PreprocResults, visible_macros};
use crate::traits::*;

/// Parsed source plus the tree-sitter `Tree` for a given language `T`.
///
/// Construct with [`Parser::new`] and feed the result into the metric,
/// alterator, or AST-dump entry points. The type parameter `T` is one
/// of the language code tags (`RustCode`, `PythonCode`, etc.) declared
/// by the internal `mk_code!` macro.
#[derive(Debug)]
pub struct Parser<T: LanguageInfo + Alterator + Checker + Getter> {
    code: Vec<u8>,
    tree: Tree,
    phantom: PhantomData<T>,
}

/// A single node-matching predicate. The `'a` bound lets a predicate
/// borrow the parser's source buffer, which the `"function"` and
/// `"string"` filters need to answer for a language whose functions or
/// string literals are identified by their text rather than their kind
/// (#1162, #1381).
///
/// The `Ancestors` argument is the chain the caller descended through.
/// Both of those predicates ask about an enclosing construct, and
/// resolving one from the node alone costs [`Node::parent`]'s
/// `O(depth)` *per lookup* — which over a whole walk is the quadratic
/// #1052 and #1122 warn about. The higher-ranked bound ties the chain's
/// tree lifetime to the node's, so a predicate cannot be handed the
/// ancestry of a different tree.
type FilterFn<'a> = dyn for<'t, 'c> Fn(&Node<'t>, Ancestors<'t, 'c>) -> bool + 'a;

/// Collection of node-matching predicates the `find` and `count` walks
/// apply to each node they visit.
pub struct Filter<'a> {
    filters: Vec<Box<FilterFn<'a>>>,
}

impl Filter<'_> {
    /// Returns `true` if *any* of the configured predicates matches
    /// `node`, reached through `ancestors`.
    ///
    /// A walker that maintains an ancestor chain should pass it. The
    /// `"function"` and `"string"` predicates ask about an enclosing
    /// construct, and off [`Ancestors::unknown`] each such lookup is
    /// [`Node::parent`]'s `O(depth)` — which made `count --type string`
    /// quadratic in nesting depth until `find` and `count` threaded a
    /// real chain (#1381).
    #[must_use]
    pub fn any<'t>(&self, node: &Node<'t>, ancestors: Ancestors<'t, '_>) -> bool {
        for f in &self.filters {
            if f(node, ancestors) {
                return true;
            }
        }
        false
    }
}

#[inline]
fn get_fake_code<T: LanguageInfo>(
    code: &[u8],
    path: &Path,
    pr: Option<Arc<PreprocResults>>,
) -> Option<Vec<u8>> {
    if let Some(pr) = pr {
        match T::lang() {
            // The C-family languages share the preprocessor
            // macro-replacement pass: `C` (#721), upstream `Cpp`, and the
            // `Mozcpp` Gecko dialect (#720) all need `#define` expansion.
            LANG::C | LANG::Cpp | LANG::Mozcpp => {
                let macros = visible_macros(path, &pr.files);
                c_macro::replace(code, &macros)
            }
            _ => None,
        }
    } else {
        None
    }
}

impl<T: 'static + LanguageInfo + Alterator + Checker + Getter> ParserTrait for Parser<T> {
    type Checker = T;
    type Getter = T;

    fn new(code: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Self {
        let fake_code = get_fake_code::<T>(&code, path, pr);
        let code = if let Some(fake) = fake_code {
            fake
        } else {
            code
        };

        let tree = Tree::new::<T>(&code);

        Self {
            code,
            tree,
            phantom: PhantomData,
        }
    }

    #[inline]
    fn root(&self) -> Node<'_> {
        self.tree.get_root()
    }

    #[inline]
    fn code(&self) -> &[u8] {
        &self.code
    }

    fn filters(&self, requested: &[String]) -> Filter<'_> {
        // Borrowed by the `"function"` and `"string"` arms below, which
        // is why `Filter` carries a lifetime.
        let code = self.code();
        let mut res: Vec<Box<FilterFn<'_>>> = Vec::new();
        for f in requested {
            let f = f.as_str();
            match f {
                "all" => res.push(Box::new(|_: &Node, _| -> bool { true })),
                // `is_call` / `is_comment` / `is_error` take `&Node` and
                // nothing else, so no language *can* make them
                // text-dependent. The #1162 gap is confined by
                // construction to the `Checker` predicates that accept
                // `code`, and `"function"` and `"string"` are the two
                // filters that reach one.
                "call" => res.push(Box::new(|node: &Node, _| T::is_call(node))),
                "comment" => res.push(Box::new(|node: &Node, _| T::is_comment(node))),
                "error" => res.push(Box::new(|node: &Node, _| T::is_error(node))),
                // `is_string_with_code`, not `is_string`: a Tcl-family
                // `braced_word` is a string literal in a value position
                // and a `proc` / `when` body everywhere else, and only
                // the bytes of the enclosing command's leading word
                // separate the two (#1381). This arm is the only caller
                // of either spelling in the workspace, so the byte-less
                // one now serves purely as the per-language kind table
                // that each override narrows.
                //
                // This is also the arm that makes the chain load-bearing
                // rather than merely cheaper. `braced_word` is *every*
                // Tcl block and list literal, so the candidate set is
                // dense, and the Tcl override asks about the enclosing
                // command — roughly ten ancestor lookups per candidate.
                // Off an unknown chain each is `Node::parent`'s
                // `O(depth)`, which took `bca find --type string` on 8 KB
                // of nested braces from 5 ms to 823 ms before the chain
                // was threaded here.
                "string" => res.push(Box::new(move |node: &Node, ancestors| {
                    T::is_string_with_code(node, code, ancestors)
                })),
                // The JS-family `is_func` and Elixir's `is_func_with_code`
                // consult the chain to tell a named function from a
                // closure and a `def` `Call` from any other (#1088,
                // #1162). Both answer the same off an unknown chain —
                // only the cost differs, and `find` / `count` now supply
                // a real one.
                "function" => res.push(Box::new(move |node: &Node, ancestors| {
                    T::is_func_with_code(node, code, ancestors)
                })),
                _ => {
                    if let Ok(n) = f.parse::<u16>() {
                        // A numeric `-t`/`--type` value matches by raw
                        // tree-sitter `kind_id` rather than node-type name.
                        // This is an escape hatch for grammar inspection
                        // (matching a kind that has no stable name, or one
                        // the string path mis-resolves), but `kind_id` is an
                        // index into the grammar's symbol table and is *not*
                        // stable across grammar versions — the same number
                        // names different nodes after a bump, and `-t 0` is
                        // the end/ERROR sentinel. Documented as unstable in
                        // big-code-analysis-book/src/commands/nodes.md; the
                        // string (`kind()`) path below is the supported one.
                        res.push(Box::new(move |node: &Node, _| -> bool {
                            node.kind_id() == n
                        }));
                    } else {
                        // Exact match on `node.kind()` — the CLI documents
                        // `find <NODE>` / `count <NODE_TYPE>` as searching
                        // for a specific node type, not a substring (see
                        // big-code-analysis-book/src/commands/nodes.md and
                        // issue #293).
                        let f = f.to_owned();
                        res.push(Box::new(move |node: &Node, _| -> bool { node.kind() == f }));
                    }
                }
            }
        }
        if res.is_empty() {
            res.push(Box::new(|_: &Node, _| -> bool { true }));
        }

        Filter { filters: res }
    }
}

impl<T: 'static + LanguageInfo + Alterator + Checker + Getter> Parser<T> {
    /// Builds a [`Parser`] from a pre-parsed [`tree_sitter::Tree`]
    /// and the matching source bytes.
    ///
    /// Use this when the caller already drives `tree-sitter` for
    /// other purposes (e.g. an editor doing incremental reparsing)
    /// and wants the metric walker to reuse the parse instead of
    /// running its own. The standard byte-based entry point
    /// remains [`ParserTrait::new`].
    ///
    /// The supplied `tree` must have been produced from `code` with
    /// the tree-sitter language matching `T` — typically obtained
    /// via [`crate::LANG::tree_sitter_language`]. A mismatch is
    /// not `unsafe`, but metric values will be nonsensical because
    /// the tree's `kind_id` values will not correspond to the per-
    /// language enum the metric `compute` functions match on.
    #[must_use]
    pub fn from_tree(tree: tree_sitter::Tree, code: Vec<u8>) -> Self {
        Self {
            code,
            tree: Tree::from_ts_tree(tree),
            phantom: PhantomData,
        }
    }

    /// Borrow the underlying [`tree_sitter::Tree`] for callers that
    /// want to drive their own traversal alongside the metric walker.
    ///
    /// `Parser` is `pub` in this crate but carries no stability
    /// promise (see the crate root); the stable spelling of this
    /// accessor is `big_code_analysis::Ast::as_tree_sitter`.
    #[must_use]
    pub fn ts_tree(&self) -> &tree_sitter::Tree {
        self.tree.as_ts_tree()
    }
}

#[cfg(test)]
mod tests {
    use crate::count::count;
    use crate::langs::PythonParser;
    use crate::traits::ParserTrait;
    use std::path::PathBuf;

    fn parse_python(source: &str) -> PythonParser {
        PythonParser::new(source.as_bytes().to_vec(), &PathBuf::from("t.py"), None)
    }

    fn count_kind(source: &str, filter: &str) -> usize {
        count(&parse_python(source), &[filter.to_string()]).0
    }

    // Regression for #293: a named filter that is not a hardcoded
    // keyword (`all`/`call`/`comment`/`error`/`string`/`function`) and
    // not a numeric `kind_id` must match `node.kind()` exactly, not via
    // substring containment.

    #[test]
    fn get_filters_exact_match_hits_named_kind() {
        // Python's `if`/`elif`/`else` clauses each appear as their own
        // `if_statement` / `elif_clause` / `else_clause` nodes. Filter
        // `if_statement` should match exactly one node here.
        let src = "if x:\n    pass\nelif y:\n    pass\nelse:\n    pass\n";
        assert_eq!(count_kind(src, "if_statement"), 1);
    }

    #[test]
    fn get_filters_no_substring_match() {
        // Filter `expression` must not match `expression_statement`,
        // `binary_expression`, etc. Under the old substring behaviour
        // every expression-bearing node would count; under exact-match
        // there is no node literally named `expression` in this source.
        let src = "x = 1 + 2\ny = foo(3)\n";
        assert_eq!(
            count_kind(src, "expression"),
            0,
            "exact match must not collapse all *_expression kinds"
        );
        // Sanity: `assignment` is a real node kind in tree-sitter-python
        // and matches exactly twice (one per statement).
        assert_eq!(count_kind(src, "assignment"), 2);
    }

    #[test]
    fn get_filters_unknown_kind_returns_empty() {
        // A filter that names no real node kind matches nothing — the
        // previous substring behaviour would still return 0 here, but
        // pinning it guards against a future regression that re-enables
        // fuzzy matching.
        let src = "x = 1\n";
        assert_eq!(count_kind(src, "definitely_not_a_python_kind"), 0);
    }

    #[test]
    fn get_filters_empty_request_matches_every_node() {
        // Requesting nothing means "match everything": `filters` falls
        // back to a match-all predicate when no arm pushed one, because
        // a `Filter` holding no predicates would make `Filter::any`
        // return `false` for every node and `bca find` / `bca count`
        // silently report zero on a bare invocation.
        //
        // Asserting against `"all"` rather than a hand-counted total is
        // what makes this able to fail: the two paths push the identical
        // closure, so dropping the fallback sends the empty request to 0
        // while `"all"` stays put. A `> 0` assertion could not tell the
        // two apart from a source that simply had nodes.
        let src = "if x:\n    pass\nelse:\n    y = foo(1 + 2)\n";
        let parser = parse_python(src);
        let everything = count(&parser, &["all".to_string()]).0;
        let unfiltered = count(&parser, &[]).0;

        assert!(
            everything > 1,
            "fixture must hold several nodes or this asserts nothing; got {everything}"
        );
        assert_eq!(
            unfiltered, everything,
            "an empty filter request must match what `all` matches"
        );
    }
}
