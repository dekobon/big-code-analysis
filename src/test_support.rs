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

use crate::spaces::metrics_inner;
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
