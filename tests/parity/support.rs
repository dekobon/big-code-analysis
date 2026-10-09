//! The two parse steps the parity suites share, so a suite reads as its
//! comparison rather than as setup.
//!
//! Neither helper names a language: each takes the `LANG` its caller is
//! sweeping, so they carry no grammar gate of their own.

use big_code_analysis::{Ast, FuncSpace, LANG, MetricsOptions, Ops, Source, analyze};

/// The metric walk's space tree for `source` parsed as `lang`.
pub(crate) fn metrics_space(lang: LANG, source: &str, name: &str) -> FuncSpace {
    analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name.to_owned())),
        MetricsOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"))
}

/// The `ops` walk's space tree for `source` parsed as `lang`.
pub(crate) fn ops_space(lang: LANG, source: &str, name: &str) -> Ops {
    Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.to_owned())))
        .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
        .ops()
        .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"))
}
