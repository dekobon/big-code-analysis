//! Shared helpers for the crate's in-tree unit tests.
//!
//! Every metric module under `src/metrics/` drives its assertions through
//! the same handful of parse-and-inspect shims. They live here, in a
//! `#[cfg(test)]`-only module, rather than in a production file: the
//! self-scan gate measures production files, and helpers parked in one
//! would spend that file's metric budget on code that never ships
//! (#1066). `.bcaignore` excludes this file from the walk for the same
//! reason.

use std::path::PathBuf;

use crate::node::{Node, Tree};
use crate::spaces::metrics_inner;
use crate::traits::LanguageInfo;
use crate::{
    CodeMetrics, FuncSpace, LANG, MetricsOptions, ParserTrait, Source, SpaceKind, analyze,
};

/// Parses `source` as `filename` and hands the resulting root
/// [`FuncSpace`] to `check`.
///
/// The source is normalised the way [`crate::read_file_with_eol`] would
/// normalise it on the way in: CRLF/CR collapse to LF, and the trailing
/// newline is regularised to exactly one. Use [`metrics_verbatim`] when a
/// test's input must reach the parser untouched.
pub(crate) fn check_func_space<T: ParserTrait, F: Fn(FuncSpace)>(
    source: &str,
    filename: &str,
    check: F,
) {
    let path = PathBuf::from(filename);
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut trimmed_bytes = normalized.trim_end().trim_matches('\n').as_bytes().to_vec();
    trimmed_bytes.push(b'\n');
    let parser = T::new(trimmed_bytes, &path, None);
    let func_space = metrics_inner(
        &parser,
        path.to_str().map(str::to_owned),
        MetricsOptions::default(),
    )
    .expect("metrics_inner returns Some for a parsed source");

    check(func_space);
}

/// Parses `source` as `filename` and hands the root space's
/// [`CodeMetrics`] to `check`.
pub(crate) fn check_metrics<T: ParserTrait>(source: &str, filename: &str, check: fn(CodeMetrics)) {
    check_func_space::<T, _>(source, filename, |func_space| {
        check(func_space.metrics.clone());
    });
}

/// Analyses `source` **byte-for-byte** and returns its metrics.
///
/// Use this, not [`check_metrics`], when a test's input must reach the
/// parser unaltered: `check_func_space` normalises CRLF and trims then
/// re-appends a trailing newline, so a construct ending at EOF is
/// unreachable through it and such a test passes vacuously (#1051). It
/// also returns a value, which `check_metrics`' bare `fn` callback
/// cannot. Restrict `options` in timing-sensitive tests so an unrelated
/// metric's cost cannot dominate and misattribute a regression.
#[track_caller]
pub(crate) fn metrics_verbatim(lang: LANG, source: &[u8], options: MetricsOptions) -> CodeMetrics {
    // `FuncSpace` has an iterative `Drop` impl (#1056), so the field
    // cannot be moved out — clone it, as this helper always did.
    space_verbatim(lang, source, options).metrics.clone()
}

/// Analyses `source` **byte-for-byte** and returns its root
/// [`FuncSpace`], nested spaces included.
///
/// The [`metrics_verbatim`] rationale applies verbatim; use this variant
/// when the assertion is about a *nested* space's metrics rather than the
/// root's aggregate (e.g. #1067's per-function `sloc`).
#[track_caller]
pub(crate) fn space_verbatim(lang: LANG, source: &[u8], options: MetricsOptions) -> FuncSpace {
    analyze(Source::new(lang, source), options).expect("verbatim source must analyse")
}

/// Parses `source` as `lang` through the [`crate::Ast`] seam, naming the
/// unit `name`.
///
/// Returns the [`crate::Ast`] rather than one of its projections so a
/// caller can reach both the seam's output (`ops`, `metrics`) and the
/// underlying tree-sitter tree; the `ops` tests need both to compare a
/// space count against a node count.
#[track_caller]
pub(crate) fn parse_named(lang: LANG, name: &str, source: &str) -> crate::Ast {
    crate::Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.to_owned())))
        .expect("language feature enabled")
}

/// Asserts a per-language fixture list is non-empty.
///
/// Lists of this shape gate each entry on its own language feature, so a
/// build with none of them enabled leaves an empty list, a loop with zero
/// iterations, and a test that reports as passing while asserting
/// nothing. Calling this is what makes that state loud.
#[track_caller]
pub(crate) fn assert_fixtures_present<T>(fixtures: &[T]) {
    assert!(
        !fixtures.is_empty(),
        "at least one language feature must be enabled for this test to mean anything"
    );
}

/// Asserts that `func_space` has a direct child space named `name` and that
/// its `kind` matches `expected`.
///
/// Used by annotation-type / class / interface tests that need to verify
/// the structural FuncSpace tree (not just metric values), since vacuous
/// metric assertions can pass even when `is_func_space` has been reverted
/// for the node kind under test.
#[track_caller]
pub(crate) fn assert_child_space_kind(func_space: &FuncSpace, name: &str, expected: SpaceKind) {
    let child = child_space(func_space, name);
    assert_eq!(
        child.kind, expected,
        "child FuncSpace {name:?} kind: got {:?}, expected {:?}",
        child.kind, expected,
    );
}

/// Returns the direct child [`FuncSpace`] named `name`, panicking if
/// absent. Lets per-space-metric tests assert accessors on the owning
/// class / interface space (where the value is nonzero) rather than on the
/// always-zero file-unit root.
#[track_caller]
pub(crate) fn child_space<'a>(func_space: &'a FuncSpace, name: &str) -> &'a FuncSpace {
    func_space
        .spaces
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("expected a child FuncSpace named {name:?}"))
}

/// Returns the [`SpaceKind::Function`] space named `name` anywhere in
/// `func_space`'s subtree, panicking unless exactly one matches.
///
/// [`child_space`] looks only at direct children and ignores `kind`, so it
/// cannot reach a function nested inside a class inside a function — the
/// shape a cross-language nested-function comparison needs. Requiring a
/// unique match keeps a fixture that later grows a second same-named
/// function from silently asserting on whichever one the walk reached
/// first.
#[track_caller]
pub(crate) fn function_space<'a>(func_space: &'a FuncSpace, name: &str) -> &'a FuncSpace {
    let mut found: Vec<&FuncSpace> = Vec::new();
    let mut stack = vec![func_space];
    while let Some(space) = stack.pop() {
        if space.kind == SpaceKind::Function && space.name.as_deref() == Some(name) {
            found.push(space);
        }
        stack.extend(space.spaces.iter());
    }
    match found.as_slice() {
        [space] => space,
        other => panic!(
            "expected exactly one function FuncSpace named {name:?}, found {}",
            other.len()
        ),
    }
}

/// Visits `code`'s tree in pre-order, maintaining the ancestor chain
/// exactly as `spaces::compute::metrics_inner` does, and hands each
/// node to `check` together with that chain.
///
/// Keeping the bookkeeping identical to the walker's is the point: a
/// test that built the chain some other way would prove
/// [`crate::node::Ancestors`] self-consistent without proving the
/// walker feeds it the right slice.
pub(crate) fn for_each_node_with_chain<L: LanguageInfo>(
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
