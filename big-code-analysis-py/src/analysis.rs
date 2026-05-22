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
//! Note that this layer's parity claim does not extend to the CLI's
//! surrounding behaviours: shebang / emacs-mode language detection
//! (the CLI uses `guess_language`, the bindings only consult the
//! path extension), the `--exclude-tests` flag (always off here),
//! and the `is_generated` walker filter (always off here). See
//! `_native.pyi`'s `analyze.__doc__` for the user-facing contract.

use std::path::Path;

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

use crate::language::parse_language_name;

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
/// Reads the file, infers its language from the path extension via
/// [`big_code_analysis::get_language_for_file`], parses, and returns
/// the serialised `FuncSpace` as a JSON `String`.
pub(crate) fn analyze_path(path: &Path) -> Result<String, AnalysisError> {
    let lang = big_code_analysis::get_language_for_file(path).ok_or_else(|| {
        AnalysisError::UnsupportedLanguage(format!(
            "no language registered for path {}",
            path.display()
        ))
    })?;
    // UTF-8 validation runs *before* the file read so a non-UTF-8
    // path can never reach `path.display()` in the
    // `AnalysisError::Io` arm — the `filename` field on the resulting
    // Python `OSError` would otherwise carry U+FFFD replacement
    // characters for the lossy bytes. `get_language_for_file` above
    // only validates the *extension* (not the whole path), so a path
    // like `\xff\xff.rs` would reach this point with a valid `LANG`
    // and a non-UTF-8 prefix.
    let name = path.to_str().ok_or(AnalysisError::NonUtf8Path)?.to_owned();
    // Capture the path on I/O failure so the Python OSError carries
    // `filename` and CPython can dispatch to FileNotFoundError /
    // PermissionError / IsADirectoryError based on `errno`.
    let code = std::fs::read(path).map_err(|source| AnalysisError::Io {
        source,
        path: path.to_path_buf(),
    })?;
    analyze_bytes(lang, &code, Some(name))
}

/// Analyse an in-memory source buffer with an explicit language.
///
/// `name` is the optional display name attached to the top-level
/// [`big_code_analysis::FuncSpace`]; for the Python `analyze_source`
/// entry point this is `None` because the caller hands over raw bytes
/// without a path.
pub(crate) fn analyze_source(
    language: &str,
    code: &[u8],
    name: Option<String>,
) -> Result<String, AnalysisError> {
    let lang = parse_language_name(language)
        .ok_or_else(|| AnalysisError::UnsupportedLanguage(language.to_owned()))?;
    analyze_bytes(lang, code, name)
}

fn analyze_bytes(lang: LANG, code: &[u8], name: Option<String>) -> Result<String, AnalysisError> {
    let source = Source::new(lang, code).with_name(name);
    let space = analyze(source, MetricsOptions::default())?;
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

    #[test]
    fn analyze_source_returns_json_object_for_rust_snippet() {
        let s = analyze_source("rust", b"fn main() {}", Some("x.rs".to_owned()))
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
        let s = analyze_source("rust", b"fn main() {}", Some("x.rs".to_owned()))
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
        let err = analyze_source("klingon", b"qaplah", None);
        assert!(matches!(err, Err(AnalysisError::UnsupportedLanguage(_))));
    }

    #[test]
    fn analyze_source_passes_arbitrary_source_to_tree_sitter() {
        // Tree-sitter is famously permissive about malformed input —
        // it produces an error tree rather than failing. Confirm that
        // the bridge surfaces *something* (a JSON object) for
        // syntactically invalid Rust rather than propagating a
        // `MetricsError`.
        let s = analyze_source("rust", b"fn missing_brace(", None)
            .expect("tree-sitter recovers on malformed input");
        assert!(parse_json(&s).is_object());
    }

    #[test]
    fn analyze_path_rejects_unknown_extension() {
        let err = analyze_path(Path::new("nonexistent.xyz"));
        assert!(matches!(err, Err(AnalysisError::UnsupportedLanguage(_))));
    }
}
