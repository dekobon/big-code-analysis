//! Parse-and-inspect helpers shared by this crate's unit tests and, behind
//! the `test-support` feature, by `big-code-analysis`'s metric tests.
//!
//! Kept out of any production file so the self-scan gate does not spend a
//! shipping module's metric budget on test-only code (#1066).

use crate::node::{Node, Tree};
use crate::traits::{LanguageInfo, ParserTrait};

/// Visits `code`'s tree in pre-order, maintaining the ancestor chain
/// exactly as `big_code_analysis::spaces::compute::metrics_inner` does,
/// and hands each node to `check` together with that chain.
///
/// Keeping the bookkeeping identical to the walker's is the point: a
/// test that built the chain some other way would prove
/// [`Ancestors`](crate::node::Ancestors) self-consistent without proving
/// the walker feeds it the right slice. The walker lives in the other
/// crate, so nothing links the two but this sentence — a change to its
/// truncate/push discipline must be mirrored here by hand
/// (`.claude/rules/testing.md`, lesson #82).
///
/// # Panics
///
/// When `L`'s per-language Cargo feature is disabled in this build (see
/// [`ParserTrait::new`]), or when the fixture does not parse cleanly, so
/// the walk cannot be measuring error recovery by accident.
pub fn for_each_node_with_chain<L: LanguageInfo>(
    code: &[u8],
    mut check: impl FnMut(&Node<'_>, &[Node<'_>]),
) -> usize {
    let tree = Tree::new::<L>(code);
    let root = tree.get_root();
    assert!(
        !root.has_error(),
        "fixture must parse cleanly, else the walk covers error recovery"
    );

    let mut chain: Vec<Node<'_>> = Vec::new();
    let mut stack = vec![(root, 0_usize)];
    let mut visited = 0;
    while let Some((node, depth)) = stack.pop() {
        chain.truncate(depth);
        check(&node, &chain);
        visited += 1;
        chain.push(node);
        let first = stack.len();
        stack.extend(node.children().map(|child| (child, depth + 1)));
        stack[first..].reverse();
    }
    visited
}

/// Walks `parser`'s tree and reports whether any node has `kind_id ==
/// target`.
///
/// The drift marker behind lesson 34. A passing `!ast_has_kind_id(…)`
/// proves an enum variant is unreachable at the pinned grammar, so a
/// defensive dispatch arm listing it is an explicit promise rather than
/// silent dead code; a bump that starts emitting the kind fails the
/// assertion instead of quietly changing a metric.
pub fn ast_has_kind_id<P: ParserTrait>(parser: &P, target: u16) -> bool {
    parser.root().preorder().any(|n| n.kind_id() == target)
}

/// A Tcl-family line whose argument list a walk can read only by
/// scanning it: `eval` with `width` braced words and one stray `]` among
/// them. Every word asks whether the list is readable, so a walk that
/// lends its `FoldCache` scans the list once and one that does not scans
/// it `width` times — the two answer identically, and only
/// `lang_helpers::argument_slot_scans` tells them apart (#1686). Shared
/// by one test per walk, in this crate and the root's.
#[must_use]
pub fn tcl_wide_erroring_eval(width: usize) -> String {
    let half = "{a b} ".repeat(width / 2);
    format!("eval {half}] {half}\n")
}

/// Asserts that `predicate` holds for every input in `yes` and for none
/// in `no`, naming the offending input on failure.
#[track_caller]
pub fn assert_classifies(predicate: fn(&[u8]) -> bool, yes: &[&[u8]], no: &[&[u8]]) {
    for input in yes {
        assert!(predicate(input), "{}", String::from_utf8_lossy(input));
    }
    for input in no {
        assert!(!predicate(input), "{}", String::from_utf8_lossy(input));
    }
}
