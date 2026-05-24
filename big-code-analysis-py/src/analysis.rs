//! Bridge between the Python entry points and the upstream library.
//!
//! These functions own the I/O, language resolution, and tree-sitter
//! parsing concerns; the result is always a JSON `String`
//! representation of a [`big_code_analysis::FuncSpace`], produced by
//! `serde_json::to_string(&space)` — the exact same serializer the
//! `bca` CLI uses for its `--output-format json` path. The caller in
//! [`crate::lib`] hands the string to [`crate::conversion::json_string_to_py`]
//! for the final hop into a Python `dict`.
//!
//! Routing through `serde_json::to_string` (rather than `to_value` +
//! recursive Python conversion) is what makes the bindings'
//! `analyze()` output byte-for-byte identical to the CLI's *at the
//! `FuncSpace` boundary*: both sides serialise the same struct
//! through the same serializer in `Serialize`-impl declaration
//! order. `to_value` would silently re-order keys alphabetically
//! because `serde_json::Map` is a `BTreeMap` without the
//! `preserve_order` Cargo feature.
//!
//! The CLI's `is_generated` walker filter is now mirrored via the
//! `skip_generated` kwarg on `analyze` (default `true`, matching the
//! CLI walker); the bindings return `None` for files whose leading
//! bytes carry an `@generated` / `DO NOT EDIT` / `GENERATED CODE`
//! marker so callers can drop them without paying parse cost (#317).
//! The `--exclude-tests` flag is mirrored via the `exclude_tests`
//! kwarg (#315). Shebang / emacs-mode language detection is also
//! mirrored — both sides resolve language through
//! [`big_code_analysis::guess_language`]. See `_native.pyi`'s
//! `analyze.__doc__` for the user-facing contract.

use std::path::Path;

use big_code_analysis::{
    LANG, Metric, MetricSet, MetricsOptions, Source, analyze, guess_language, is_generated,
};

use crate::language::parse_language_name;

/// Canonical metric-name table.
///
/// The single source of truth for which strings the Python
/// `metrics=` kwarg (and the `bca.METRIC_NAMES` module constant)
/// accepts. Each entry parses through
/// [`<Metric as std::str::FromStr>::from_str`] in the upstream
/// crate; the only difference is that table omits the `"exit"`
/// alias (the canonical JSON output key is `"nexits"`, so that is
/// the name advertised to users — both parse to [`Metric::Exit`]).
///
/// Alphabetised so the order matches what callers see in
/// `bca.METRIC_NAMES` and what the `unknown metric` error message
/// suggests; never re-order on a whim.
pub(crate) const METRIC_NAMES: &[&str] = &[
    "abc",
    "cognitive",
    "cyclomatic",
    "halstead",
    "loc",
    "mi",
    "nargs",
    "nexits",
    "nom",
    "npa",
    "npm",
    "tokens",
    "wmc",
];

/// Resolve a Python-side `metrics=` list into a [`MetricSet`].
///
/// Returns `Err(message)` for the two programmer-error shapes
/// described on the public Python entry points:
///
/// - Empty list → `"provide at least one metric, or omit the argument"`.
/// - Unknown name → `"unknown metric: <bad>; valid: <comma list>"`,
///   listing every entry in [`METRIC_NAMES`].
///
/// Returns `Ok` for any non-empty list of recognised names; duplicates
/// are silently collapsed because [`MetricSet::from_slice_with_deps`]'s
/// bitfield insert is idempotent. The resolved set carries each
/// requested metric's transitive dependencies — `["mi"]` produces a
/// set that also contains `Loc + Cyclomatic + Halstead`, so callers
/// don't have to spell out the dependency closure.
///
/// The error type is `String` (not `AnalysisError`) because a bad
/// `metrics=` is discovered *before* any file I/O and is therefore a
/// caller bug — folding it into `AnalysisError` would falsely surface
/// it as a per-file failure when `analyze_batch` routes per-file
/// errors through `AnalysisError`. The Python entry points wrap the
/// `String` with `PyValueError::new_err`.
pub(crate) fn parse_metric_names(names: &[String]) -> Result<MetricSet, String> {
    if names.is_empty() {
        return Err("provide at least one metric, or omit the argument".to_owned());
    }
    // Pre-allocate against the input length: the worst case is one
    // `Metric` per name (no duplicates); duplicates collapse later in
    // the bitfield closure and the extra capacity is harmless.
    let mut parsed: Vec<Metric> = Vec::with_capacity(names.len());
    for name in names {
        match name.parse::<Metric>() {
            Ok(m) => parsed.push(m),
            Err(_) => {
                return Err(format!(
                    "unknown metric: {bad}; valid: {valid}",
                    bad = name,
                    valid = METRIC_NAMES.join(", "),
                ));
            }
        }
    }
    Ok(MetricSet::from_slice_with_deps(&parsed))
}

/// File-analysis policy knobs threaded through [`analyze_path`].
///
/// Each field maps one-to-one to a keyword-only kwarg on the public
/// `bca.analyze` Python entry point. The struct is `pub(crate)` and
/// owned by the bridge layer; the `PyO3` dispatcher in [`crate::lib`]
/// builds it from the kwargs and hands the whole record across the
/// crate boundary so call sites read as
/// `analyze_path(&path, AnalyzeOptions::default())` for CLI-walker
/// parity (the [`Default`] impl below sets `skip_generated=true`,
/// matching the Python `analyze(skip_generated=True)` default) or as
/// `AnalyzeOptions { skip_generated: false, ..Default::default() }`
/// when overriding a single field — rather than relying on positional
/// bool order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AnalyzeOptions {
    /// Mirror `bca metrics --exclude-tests`: prune Rust `#[test]` /
    /// `#[cfg(test)]` subtrees before any per-metric `compute` runs
    /// (#315).
    pub exclude_tests: bool,
    /// Substitute U+FFFD for non-UTF-8 path bytes via
    /// `Path::to_string_lossy` instead of returning
    /// `AnalysisError::NonUtf8Path`. Opt-in by design — the strict
    /// default keeps `FuncSpace.name` a round-trippable identifier
    /// (#316).
    pub allow_lossy_path: bool,
    /// Skip the file (return `Ok(None)`) when its leading window
    /// matches [`big_code_analysis::is_generated`]'s `@generated` /
    /// `DO NOT EDIT` / `GENERATED CODE` predicate. Default `true`
    /// matches the CLI walker (#317).
    pub skip_generated: bool,
    /// Which metrics to compute. Default [`MetricSet::all`] matches
    /// the pre-#268 "compute everything" behaviour. The Python
    /// `metrics=` kwarg builds this via [`parse_metric_names`]; bypass
    /// it (or pass `MetricSet::all()`) when the caller did not request
    /// a subset (#268).
    pub metrics: MetricSet,
}

// Manual `Default` impl — a `#[derive(Default)]` would set every bool
// to `false`, contradicting both the per-field docstring above ("Default
// `true` matches the CLI walker") and the Python entry point's
// kwarg default (`skip_generated=True`). Rust callers using
// `AnalyzeOptions::default()` get CLI-walker parity here without
// having to remember which field flips polarity.
impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            exclude_tests: false,
            allow_lossy_path: false,
            skip_generated: true,
            // `MetricSet::all()` (rather than `MetricSet::default()`)
            // is named explicitly so the intent reads at the field
            // site: full suite, every metric computed. They are
            // currently identical, but a future change to
            // `MetricSet::default` would silently shift this baseline
            // — pin the literal.
            metrics: MetricSet::all(),
        }
    }
}

/// Errors surfaced from the bridge layer. Mapped to specific Python
/// exception types in [`crate::lib`].
#[derive(Debug)]
pub(crate) enum AnalysisError {
    /// I/O failure when reading the source file. Carries the source
    /// `std::io::Error` AND the path that triggered it so the Python
    /// side can build an `OSError(errno, msg, filename)` 3-tuple —
    /// the form `CPython` needs to dispatch to the right subclass
    /// (`FileNotFoundError`, `PermissionError`, `IsADirectoryError`,
    /// …) and to populate `err.errno` / `err.filename`.
    Io {
        source: std::io::Error,
        path: std::path::PathBuf,
    },
    /// Path could not be expressed as UTF-8 — required for the
    /// `FuncSpace.name` field, which round-trips through JSON.
    NonUtf8Path,
    /// File extension was unrecognised, or the explicit `language`
    /// argument named an unknown language.
    UnsupportedLanguage(String),
    /// Tree-sitter parser failed to produce a usable tree, or any
    /// other `MetricsError` variant that does not map cleanly onto
    /// a more specific Python exception.
    Parse(big_code_analysis::MetricsError),
    /// Result could not be serialised through `serde_json`. In
    /// practice this is unreachable for `FuncSpace` round-trips
    /// today — `serde_json::to_string`'s `serialize_f64` writes
    /// `null` for non-finite floats rather than erroring, and no
    /// other failure mode is reachable from our struct shape — but
    /// the arm exists as a defensive boundary in case a future
    /// upstream `Serialize` impl introduces a fallible path (e.g.
    /// a metric that uses `serde_json::Number::from_f64` directly,
    /// which DOES return `None` on non-finite). The
    /// `non_finite_floats_round_trip_as_python_none` test in
    /// `conversion.rs` pins the current round-trip contract.
    Serialization(serde_json::Error),
}

impl AnalysisError {
    /// Construct an [`AnalysisError::Io`] from a `std::io::Error` and
    /// the path that triggered it. Centralised so both `analyze_path`
    /// and `language::language_for_file` capture path + source in
    /// the same shape — the dispatcher in [`crate::lib`] expects
    /// both fields to populate the resulting `OSError`'s `filename`
    /// and `errno`.
    pub(crate) fn io(source: std::io::Error, path: &Path) -> Self {
        Self::Io {
            source,
            path: path.to_path_buf(),
        }
    }
}

// `AnalysisError::Io` is constructed only at the file-read site so
// the path can be captured alongside the `std::io::Error`. A blanket
// `From<std::io::Error>` impl would let `?` lose that path silently.

impl From<big_code_analysis::MetricsError> for AnalysisError {
    // The explicit `EmptyRoot | ParseHasErrors` arm maps to the same
    // expression as the catch-all (`Self::Parse(err)`), which clippy
    // flags via `match_same_arms`. The redundancy is intentional —
    // the explicit arm is a *tripwire*: a future upstream rename or
    // removal of either reserved variant will produce a compile
    // error here, forcing a review of the Python-side taxonomy.
    // Collapsing into the catch-all would lose that signal.
    #[allow(clippy::match_same_arms)]
    fn from(err: big_code_analysis::MetricsError) -> Self {
        use big_code_analysis::MetricsError;
        match err {
            // A disabled language reaches Python through the same
            // surface as an unknown language name — both mean
            // "cannot service this request". Mapping it to
            // `UnsupportedLanguageError` keeps the Python-side
            // taxonomy honest (the variant name lookup succeeded
            // but the grammar is not in the build).
            MetricsError::LanguageDisabled(lang) => Self::UnsupportedLanguage(format!(
                "language {} is recognised but its grammar was not compiled into this build",
                lang.get_name()
            )),
            // `EmptyRoot` is reserved upstream (no walker emits it
            // today) and `ParseHasErrors` is reserved for a future
            // strict-parse mode. Both belong in the parse-failure
            // bucket on the Python side.
            MetricsError::EmptyRoot | MetricsError::ParseHasErrors => Self::Parse(err),
            // Upstream `NonUtf8Path` is reserved for a future
            // strict-identifier validator; the bindings already
            // reject non-UTF-8 paths themselves in `analyze_path`,
            // but if the upstream layer ever surfaces this variant
            // it should yield the same Python exception class
            // (`ValueError`) so callers' handling stays consistent
            // across both detection sites.
            MetricsError::NonUtf8Path => Self::NonUtf8Path,
            // `MetricsError` is `#[non_exhaustive]`, which *requires*
            // this wildcard arm — so it does NOT act as a compile-time
            // tripwire. New upstream variants will silently default to
            // `ParseError` on the Python side until someone teaches
            // this match a more specific mapping. That is a sensible
            // landing zone for "something went wrong during analysis",
            // but it does mean a `cargo update` can shift error
            // taxonomy without warning; audit this arm whenever the
            // upstream `MetricsError` enum gains a variant.
            _ => Self::Parse(err),
        }
    }
}

impl From<serde_json::Error> for AnalysisError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err)
    }
}

/// Analyse a single file on disk.
///
/// Reads the file, resolves its language via
/// [`big_code_analysis::guess_language`] — the same helper the `bca`
/// CLI uses, which inspects the path extension first and falls back
/// to a `#!`-shebang line or an emacs `-*- mode: … -*-` declaration
/// for extension-less scripts — parses, and returns the serialised
/// `FuncSpace` as a JSON `String`.
///
/// The file is read *before* language inference (CLI parity): a
/// missing or unreadable file therefore raises an I/O error even
/// when its extension is unknown, where the prior extension-only
/// path would have raised `UnsupportedLanguageError` without
/// touching the filesystem.
///
/// `exclude_tests` mirrors the CLI's `--exclude-tests` flag (#315):
/// when `true`, the analysis runs under
/// `MetricsOptions::default().with_exclude_tests(true)`, which the
/// Rust language checker uses to prune `#[test]` / `#[cfg(test)]` /
/// `#[tokio::test]` subtrees before any metric runs. Languages
/// without a `Checker::should_skip_subtree` override ignore the
/// flag; the default `false` preserves prior bindings behaviour.
///
/// `allow_lossy_path` selects the non-UTF-8 path policy (#316):
///
/// * `false` (default) — non-UTF-8 paths are rejected with
///   [`AnalysisError::NonUtf8Path`], honouring the project-wide
///   "never use `to_string_lossy` on identifier paths" rule from
///   AGENTS.md. The `FuncSpace.name` field is guaranteed to be a
///   real (round-trippable) UTF-8 string.
/// * `true` — non-UTF-8 byte sequences in the path are replaced
///   with U+FFFD via [`Path::to_string_lossy`], matching the
///   `bca metrics --output-format json` CLI exactly. The opt-in
///   makes the lossy substitution explicit at the call site.
///
/// `skip_generated` selects the generated-file policy (#317):
///
/// * `true` (default) — files whose leading window matches
///   [`big_code_analysis::is_generated`] (`@generated` /
///   `DO NOT EDIT` / `GENERATED CODE` markers) return `Ok(None)`
///   without parsing, mirroring the CLI walker which emits no
///   record for these files.
/// * `false` — the marker check is bypassed and every readable
///   file produces a populated `FuncSpace`.
///
/// The check runs after the file read but before language inference
/// and parsing: a file with a generated marker but an unknown
/// extension still skips silently rather than raising
/// `UnsupportedLanguage`, matching the CLI walker's ordering.
pub(crate) fn analyze_path(
    path: &Path,
    opts: AnalyzeOptions,
) -> Result<Option<String>, AnalysisError> {
    // Resolve the `FuncSpace.name` *before* the file read so a
    // non-UTF-8 path in strict mode can never reach `path.display()`
    // in the `AnalysisError::Io` arm — the `filename` field on the
    // resulting Python `OSError` would otherwise carry U+FFFD
    // replacement characters for the lossy bytes.
    //
    // When `allow_lossy_path` is true the caller has explicitly
    // accepted U+FFFD substitution for the `FuncSpace.name` field,
    // so `to_string_lossy` is the documented behaviour rather than
    // a silent identifier corruption.
    let name = match path.to_str() {
        Some(s) => s.to_owned(),
        None if opts.allow_lossy_path => path.to_string_lossy().into_owned(),
        None => return Err(AnalysisError::NonUtf8Path),
    };
    // Capture the path on I/O failure so the Python OSError carries
    // `filename` and CPython can dispatch to FileNotFoundError /
    // PermissionError / IsADirectoryError based on `errno`.
    let code = std::fs::read(path).map_err(|source| AnalysisError::io(source, path))?;
    // Generated-file filter (CLI parity, #317): runs *before* language
    // inference so a generated file with an unknown extension still
    // returns `Ok(None)` rather than raising `UnsupportedLanguage` —
    // matches the CLI walker, which discards generated files without
    // attempting to resolve their language.
    if opts.skip_generated && is_generated(&code) {
        return Ok(None);
    }
    // `guess_language` returns a `(Option<LANG>, &str)` tuple — we
    // only care about the variant; the display name is recovered
    // downstream from `LANG::get_name` via the metric serialiser.
    let lang = guess_language(&code, path).0.ok_or_else(|| {
        AnalysisError::UnsupportedLanguage(format!(
            "no language registered for path {}",
            path.display()
        ))
    })?;
    analyze_bytes(lang, &code, Some(name), opts).map(Some)
}

pub(crate) fn analyze_source(
    language: &str,
    code: &[u8],
    name: Option<String>,
    opts: AnalyzeOptions,
) -> Result<String, AnalysisError> {
    let lang = parse_language_name(language)
        .ok_or_else(|| AnalysisError::UnsupportedLanguage(language.to_owned()))?;
    analyze_bytes(lang, code, name, opts)
}

fn analyze_bytes(
    lang: LANG,
    code: &[u8],
    name: Option<String>,
    opts: AnalyzeOptions,
) -> Result<String, AnalysisError> {
    let source = Source::new(lang, code).with_name(name);
    // Build `MetricsOptions` once from the bridge struct: `exclude_tests`
    // flows through unchanged, and the already-resolved `MetricSet` is
    // attached without re-running `with_only`'s slice→set conversion
    // (the `parse_metric_names` helper above has already produced the
    // closed-over set, so re-resolving here would duplicate that work).
    let options = MetricsOptions::default()
        .with_exclude_tests(opts.exclude_tests)
        .with_metric_set(opts.metrics);
    let space = analyze(source, options)?;
    Ok(serde_json::to_string(&space)?)
}

/// Convenience accessor used by the `__version__` module attribute.
/// The version comes from the crate's own `CARGO_PKG_VERSION` — that
/// value is inherited from the workspace via `version.workspace =
/// true`, so it stays in lockstep with the Rust library version.
pub(crate) const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("analyze output is valid JSON")
    }

    // Drift guard: every entry in `METRIC_NAMES` must round-trip
    // through `Metric::from_str`. If the slice grows out of sync
    // with the upstream `FromStr` impl — by typo, by reordering,
    // or by adding a name for a not-yet-implemented variant — this
    // test fails on `cargo test` before any Python-side test runs.
    #[test]
    fn metric_names_all_parse_via_from_str() {
        for name in METRIC_NAMES {
            name.parse::<Metric>().unwrap_or_else(|_| {
                panic!(
                    "METRIC_NAMES contains {name:?} but \
                     `Metric::from_str` rejects it; the bindings \
                     advertise a name the upstream crate cannot \
                     parse"
                )
            });
        }
    }

    // Pin the alphabetised order. The user-facing
    // `unknown metric: …; valid: <list>` error message lists names
    // in slice order; a re-order would silently shift that list
    // and the public `bca.METRIC_NAMES` tuple, surprising callers.
    #[test]
    fn metric_names_is_alphabetised() {
        let mut sorted: Vec<&str> = METRIC_NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            METRIC_NAMES,
            sorted.as_slice(),
            "METRIC_NAMES must stay alphabetised",
        );
    }

    #[test]
    fn analyze_source_returns_json_object_for_rust_snippet() {
        let s = analyze_source(
            "rust",
            b"fn main() {}",
            Some("x.rs".to_owned()),
            AnalyzeOptions::default(),
        )
        .expect("rust snippet parses");
        let value = parse_json(&s);
        let obj = value.as_object().expect("top-level is an object");
        assert_eq!(
            obj.get("name").and_then(serde_json::Value::as_str),
            Some("x.rs"),
        );
        // The walker always emits a synthetic top-level Unit FuncSpace.
        assert_eq!(
            obj.get("kind").and_then(serde_json::Value::as_str),
            Some("unit"),
        );
    }

    #[test]
    fn analyze_source_emits_funcspace_top_level_keys_in_declaration_order() {
        // Rust-side sentinel for the upstream `FuncSpace::Serialize`
        // impl's field order. The Python `test_analyze_key_order_matches_cli`
        // is the canonical regression test (it walks the dict via
        // CPython's insertion-order semantics, robust to nested-key
        // collisions), but it requires `maturin develop` and pytest
        // — a Rust contributor running `cargo test --workspace
        // --all-features` would otherwise see no signal if the
        // `FuncSpace` field order regressed.
        //
        // Check the JSON *prefix* rather than `s.find()` positions:
        // any reorder that puts a different field before `name`
        // would change the leading bytes, including the alphabetical
        // re-sort the `to_value` path would produce (which starts
        // with `"end_line"` because `e` < `k` < `m` < `n`). Keeping
        // the assertion narrow this way avoids the P9 nested-key
        // trap that a positional check (`name_pos < start_line_pos`)
        // would re-introduce: alphabetical also satisfies that
        // particular inequality (`name@4 < start_line@6` after sort).
        let s = analyze_source(
            "rust",
            b"fn main() {}",
            Some("x.rs".to_owned()),
            AnalyzeOptions::default(),
        )
        .expect("rust snippet parses");
        assert!(
            s.starts_with(r#"{"name":"x.rs","start_line":"#),
            "expected `FuncSpace::Serialize` to emit `name` then `start_line` \
             as the first two top-level fields — routing through \
             `serde_json::to_value` would re-sort the JSON object \
             alphabetically (starting with `\"end_line\"`); got: {s}",
        );
    }

    #[test]
    fn analyze_source_rejects_unknown_language() {
        let err = analyze_source("klingon", b"qaplah", None, AnalyzeOptions::default());
        assert!(matches!(err, Err(AnalysisError::UnsupportedLanguage(_))));
    }

    #[test]
    fn analyze_source_passes_arbitrary_source_to_tree_sitter() {
        // Tree-sitter is famously permissive about malformed input —
        // it produces an error tree rather than failing. Confirm that
        // the bridge surfaces *something* (a JSON object) for
        // syntactically invalid Rust rather than propagating a
        // `MetricsError`.
        let s = analyze_source(
            "rust",
            b"fn missing_brace(",
            None,
            AnalyzeOptions::default(),
        )
        .expect("tree-sitter recovers on malformed input");
        assert!(parse_json(&s).is_object());
    }

    #[test]
    fn analyze_source_exclude_tests_prunes_rust_test_attribute() {
        // The flag is plumbed all the way to `MetricsOptions::exclude_tests`
        // in the parent crate, which prunes Rust `#[test]` /
        // `#[cfg(test)]` subtrees inside `metrics_inner`. A single-file
        // call to `analyze_source` therefore observes the pruning at the
        // top-level `FuncSpace.metrics.nom.functions.sum` count.
        //
        // Test-via-revert: temporarily change the body of `analyze_bytes`
        // to ignore `exclude_tests` (always pass `MetricsOptions::default()`)
        // and this assertion fails because `pruned_functions` then matches
        // `baseline_functions` (both 2).
        let source = b"fn prod() -> i32 { 1 + 2 }\n\n#[test]\nfn t() { assert_eq!(1 + 1, 2); }\n";

        let baseline = analyze_source("rust", source, None, AnalyzeOptions::default())
            .expect("rust snippet parses");
        let pruned = analyze_source(
            "rust",
            source,
            None,
            AnalyzeOptions {
                exclude_tests: true,
                ..AnalyzeOptions::default()
            },
        )
        .expect("rust snippet parses");

        // The `nom` serializer emits the field as `"functions"` (see
        // `src/metrics/nom.rs::Serialize`), not `functions_sum`.
        let baseline_functions = parse_json(&baseline)["metrics"]["nom"]["functions"]
            .as_f64()
            .expect("functions is numeric");
        let pruned_functions = parse_json(&pruned)["metrics"]["nom"]["functions"]
            .as_f64()
            .expect("functions is numeric");

        assert!(
            (baseline_functions - 2.0).abs() < f64::EPSILON,
            "baseline must count both `prod` and `#[test] fn t`; got {baseline_functions}",
        );
        assert!(
            (pruned_functions - 1.0).abs() < f64::EPSILON,
            "exclude_tests=true must elide `#[test] fn t`; got {pruned_functions}",
        );
    }

    #[test]
    fn analyze_path_rejects_unknown_extension() {
        // After the #314 reordering (read before language inference),
        // a missing file surfaces as `Io` regardless of extension —
        // the extension check no longer short-circuits. Write a real
        // file with an unknown extension to exercise the
        // `UnsupportedLanguage` arm.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("thing.unknownext");
        std::fs::write(&path, b"noise\n").expect("write fixture");
        let err = analyze_path(&path, AnalyzeOptions::default());
        assert!(matches!(err, Err(AnalysisError::UnsupportedLanguage(_))));
    }

    #[test]
    fn analyze_path_resolves_shebang_for_extension_less_script() {
        // CLI parity: an extension-less file whose first line is a
        // recognised shebang must be analysed, not rejected. The
        // pre-#314 bindings consulted only the extension and would
        // raise `UnsupportedLanguage` here; the `guess_language`
        // path falls through to the shebang interpreter table.
        //
        // The fixture defines a Python function so the inner
        // `spaces` array carries a `function`-kind child only if
        // tree-sitter parsed the bytes as Python. A regression that
        // mis-routes the shebang to a different language (or leaves
        // the synthetic top-level `unit` empty) would change the
        // inner shape — a bare `kind == "unit"` check on the top
        // level would pass for *any* successful analysis and would
        // not catch such a drift.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("install");
        let source = b"#!/usr/bin/env python\ndef hello(name):\n    return name\n";
        std::fs::write(&path, source).expect("write fixture");
        let json = analyze_path(&path, AnalyzeOptions::default())
            .expect("shebang script analyses")
            .expect("non-generated file yields a populated FuncSpace");
        let value = parse_json(&json);
        let obj = value.as_object().expect("top-level is an object");
        assert_eq!(
            obj.get("kind").and_then(serde_json::Value::as_str),
            Some("unit"),
            "top-level FuncSpace kind",
        );
        let spaces = obj
            .get("spaces")
            .and_then(serde_json::Value::as_array)
            .expect("`spaces` is an array");
        let child = spaces
            .iter()
            .find_map(|s| s.as_object())
            .expect("at least one inner FuncSpace");
        assert_eq!(
            child.get("name").and_then(serde_json::Value::as_str),
            Some("hello"),
            "inner space name pins the Python `def hello(...)` parse",
        );
        assert_eq!(
            child.get("kind").and_then(serde_json::Value::as_str),
            Some("function"),
            "inner space kind pins shebang→python routing",
        );
    }

    // Non-UTF-8 paths are constructed from raw `OsStr` bytes via
    // `OsStrExt::from_bytes`, which is unix-only. Windows has its
    // own non-UTF-8 mechanism (unpaired surrogates in WTF-16 via
    // `OsStringExt::from_wide`) — out of scope for this test; the
    // `allow_lossy_path` policy applies on all platforms but
    // constructing a fixture for it differs per OS.
    #[cfg(unix)]
    #[test]
    fn analyze_path_rejects_non_utf8_path_by_default() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().expect("tempdir");
        // 0xff 0xff is not a valid UTF-8 lead byte sequence; the `.rs`
        // suffix keeps language detection on the happy path so the
        // failure mode under test is the UTF-8 check, not extension
        // resolution.
        let raw = [0xffu8, 0xff, b'.', b'r', b's'];
        let path = dir.path().join(OsStr::from_bytes(&raw));
        std::fs::write(&path, b"fn main() {}\n").expect("write fixture");
        let err = analyze_path(&path, AnalyzeOptions::default());
        assert!(
            matches!(err, Err(AnalysisError::NonUtf8Path)),
            "strict mode must reject non-UTF-8 path, got {err:?}",
        );
    }

    #[cfg(unix)]
    #[test]
    fn analyze_path_accepts_non_utf8_path_when_allow_lossy_path_is_true() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // Test-via-revert: changing the `None if allow_lossy_path`
        // arm back to `None => return Err(...)` makes this test fail
        // with `AnalysisError::NonUtf8Path` — it exercises the new
        // branch, not the strict path that the previous test covers.
        let dir = tempfile::tempdir().expect("tempdir");
        let raw = [0xffu8, 0xff, b'.', b'r', b's'];
        let path = dir.path().join(OsStr::from_bytes(&raw));
        std::fs::write(&path, b"fn main() {}\n").expect("write fixture");
        let json = analyze_path(
            &path,
            AnalyzeOptions {
                allow_lossy_path: true,
                ..AnalyzeOptions::default()
            },
        )
        .expect("lossy mode parses non-UTF-8 path")
        .expect("non-generated file yields a populated FuncSpace");
        let value = parse_json(&json);
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .expect("name is a string");
        // U+FFFD is the Unicode REPLACEMENT CHARACTER; `to_string_lossy`
        // emits one per invalid byte sequence. Asserting on its presence
        // (rather than the full string) keeps the test robust against
        // tempdir-prefix variation while still pinning the load-bearing
        // behaviour: invalid bytes were translated, not rejected.
        assert!(
            name.contains('\u{FFFD}'),
            "expected U+FFFD substitution in name, got {name:?}",
        );
        // Spot-check the analysis still produced a populated
        // FuncSpace — a regression that returned the lossy name but
        // an empty body would slip past the U+FFFD assertion alone.
        assert_eq!(
            value.get("kind").and_then(serde_json::Value::as_str),
            Some("unit"),
        );
    }

    #[test]
    fn analyze_path_returns_none_for_generated_file_when_skip_generated_is_true() {
        // CLI parity (#317): a file whose leading window matches
        // `is_generated` must be skipped without parsing, just like
        // `bca metrics --output-format json` discards it from the
        // walker. Returning `Ok(None)` is the Python-facing "no
        // record emitted" signal.
        //
        // Test-via-revert: removing the `if skip_generated &&
        // is_generated(&code) { return Ok(None); }` branch makes
        // this test fail because the file parses fine and the
        // function returns `Ok(Some(_))`.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gen.rs");
        std::fs::write(
            &path,
            b"// @generated by some-tool. DO NOT EDIT.\npub fn x() {}\n",
        )
        .expect("write fixture");
        let out = analyze_path(
            &path,
            AnalyzeOptions {
                skip_generated: true,
                ..AnalyzeOptions::default()
            },
        )
        .expect("generated file is not an error");
        assert!(
            out.is_none(),
            "skip_generated=true must elide the FuncSpace, got Some(_)",
        );
    }

    #[test]
    fn analyze_path_parses_generated_file_when_skip_generated_is_false() {
        // Opt-out path: the same marker no longer triggers the skip,
        // so the function returns the populated FuncSpace JSON. Pins
        // that the kwarg is load-bearing rather than always-skip.
        //
        // A bare `kind == "unit"` check would miss a regression where
        // the parser ran but emitted an empty `spaces` array; assert
        // the inner `pub fn x` shows up so the test exercises the
        // full parse path, not just the unit envelope.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gen.rs");
        std::fs::write(
            &path,
            b"// @generated by some-tool. DO NOT EDIT.\npub fn x() {}\n",
        )
        .expect("write fixture");
        let json = analyze_path(
            &path,
            AnalyzeOptions {
                skip_generated: false,
                ..AnalyzeOptions::default()
            },
        )
        .expect("generated file parses with skip_generated=false")
        .expect("skip_generated=false must yield Some(json)");
        let value = parse_json(&json);
        assert_eq!(
            value.get("kind").and_then(serde_json::Value::as_str),
            Some("unit"),
        );
        let spaces = value
            .get("spaces")
            .and_then(serde_json::Value::as_array)
            .expect("spaces is an array");
        let inner_names: Vec<&str> = spaces
            .iter()
            .filter_map(|s| s.get("name").and_then(serde_json::Value::as_str))
            .collect();
        assert!(
            inner_names.contains(&"x"),
            "expected inner `x` fn in spaces, got {inner_names:?}",
        );
    }

    #[test]
    fn analyze_path_skips_generated_file_before_language_inference() {
        // CLI walker ordering: `is_generated` runs before
        // `guess_language`, so a generated file with an unrecognised
        // extension still skips silently. A regression that moved the
        // marker check after the language lookup would surface as
        // `UnsupportedLanguage` here.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gen.unknownext");
        std::fs::write(&path, b"// @generated\npub fn x() {}\n").expect("write fixture");
        let out = analyze_path(
            &path,
            AnalyzeOptions {
                skip_generated: true,
                ..AnalyzeOptions::default()
            },
        )
        .expect("skip must precede language inference");
        assert!(out.is_none(), "expected None, got {out:?}");
    }
}
