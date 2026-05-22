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
//! `analyze()` output byte-for-byte identical to the CLI's: both
//! sides serialise the same struct through the same serializer in
//! `Serialize`-impl declaration order. `to_value` would silently
//! re-order keys alphabetically because `serde_json::Map` is a
//! `BTreeMap` without the `preserve_order` Cargo feature.

use std::path::Path;

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

use crate::language::parse_language_name;

/// Errors surfaced from the bridge layer. Mapped to specific Python
/// exception types in [`crate::lib`].
#[derive(Debug)]
pub(crate) enum AnalysisError {
    /// I/O failure when reading the source file (mapped to Python
    /// `OSError`).
    Io(std::io::Error),
    /// Path could not be expressed as UTF-8 — required for the
    /// `FuncSpace.name` field, which round-trips through JSON.
    NonUtf8Path,
    /// File extension was unrecognised, or the explicit `language`
    /// argument named an unknown language.
    UnsupportedLanguage(String),
    /// Tree-sitter parser failed to produce a usable tree.
    Parse(big_code_analysis::MetricsError),
    /// Result could not be serialised through `serde_json`. In
    /// practice this is unreachable for `FuncSpace` round-trips —
    /// the metric `Serialize` impls treat non-finite floats as
    /// `null` rather than emitting them — and exists as a
    /// defensive arm to keep the `?` chain clean.
    Serialization(serde_json::Error),
}

impl From<std::io::Error> for AnalysisError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<big_code_analysis::MetricsError> for AnalysisError {
    fn from(err: big_code_analysis::MetricsError) -> Self {
        match err {
            // A disabled language reaches Python through the same
            // surface as an unknown language name — both mean
            // "cannot service this request". Mapping it to
            // `UnsupportedLanguageError` keeps the Python-side
            // taxonomy honest (the variant name lookup succeeded
            // but the grammar is not in the build).
            big_code_analysis::MetricsError::LanguageDisabled(lang) => {
                Self::UnsupportedLanguage(format!(
                    "language {} is recognised but its grammar was not compiled into this build",
                    lang.get_name()
                ))
            }
            other => Self::Parse(other),
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
    let code = std::fs::read(path)?;
    let name = path.to_str().ok_or(AnalysisError::NonUtf8Path)?.to_owned();
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
    fn analyze_source_emits_declaration_order_keys() {
        // The whole point of routing through `to_string` (rather than
        // `to_value` + recursive Python conversion) is that the
        // serialised bytes preserve the `FuncSpace` `Serialize`
        // impl's field order: `name`, `start_line`, `end_line`,
        // `kind`, `spaces`, `metrics`. Pin that here so a future
        // refactor that re-introduces `to_value` (which would re-sort
        // keys alphabetically through `BTreeMap`) is caught.
        let s = analyze_source("rust", b"fn main() {}", Some("x.rs".to_owned()))
            .expect("rust snippet parses");
        // Find the first six top-level keys in the order they appear
        // in the JSON string. Using a raw text scan because parsing
        // through `serde_json::from_str` would push them through a
        // `BTreeMap` again and erase the order.
        let mut seen = Vec::new();
        for key in [
            "name",
            "start_line",
            "end_line",
            "kind",
            "spaces",
            "metrics",
        ] {
            let needle = format!("\"{key}\"");
            let pos = s.find(&needle).unwrap_or_else(|| {
                panic!("key {key} not found in serialised output: {s}");
            });
            seen.push((pos, key));
        }
        seen.sort_by_key(|&(pos, _)| pos);
        let observed: Vec<&str> = seen.into_iter().map(|(_, k)| k).collect();
        assert_eq!(
            observed,
            [
                "name",
                "start_line",
                "end_line",
                "kind",
                "spaces",
                "metrics"
            ],
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
