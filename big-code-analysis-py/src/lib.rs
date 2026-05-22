//! `PyO3` entry point for the `big_code_analysis._native` extension
//! module.
//!
//! Everything callable from Python lives here; the heavy lifting is
//! delegated to the `analysis`, `conversion`, and `language`
//! sub-modules so the bindings can be unit-tested at the Rust level
//! without spinning up a Python interpreter.

#![allow(unsafe_op_in_unsafe_fn)]
// The `#[pymodule]` macro expands to an `extern "C"` init function
// that PyO3 marks `#[unsafe(no_mangle)]`. The expansion contains
// unsafe FFI shims that the macro itself wraps in `unsafe { ... }`;
// the lint above silences the workspace-wide warn so this crate stays
// `unsafe`-free at the source level (the only `unsafe` is generated
// by the PyO3 macro).

mod analysis;
mod conversion;
mod language;

use std::path::PathBuf;

use pyo3::Bound;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::create_exception;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyByteArray, PyBytes, PyModule, PyModuleMethods, PyString};
use pyo3::wrap_pyfunction;

use crate::analysis::{AnalysisError, AnalyzeOptions, PACKAGE_VERSION};

// Python exception types. Both subclass `ValueError` per the API
// contract in #265 — `UnsupportedLanguageError` covers unknown
// extensions or explicit-language strings; `ParseError` covers
// tree-sitter parser failures bubbled up through the upstream
// `MetricsError::Parse` family.
create_exception!(
    big_code_analysis._native,
    UnsupportedLanguageError,
    PyValueError,
    "Raised when a file extension or explicit language name is not recognised."
);
create_exception!(
    big_code_analysis._native,
    ParseError,
    PyValueError,
    "Raised when the tree-sitter parser fails on the supplied source."
);

/// Convert an internal `AnalysisError` to a concrete Python exception.
///
/// Kept as a free function (rather than a `From<AnalysisError>` impl
/// on `PyErr`) so the orphan rules let it live next to the exception
/// types in this crate.
fn analysis_error_to_py(err: AnalysisError) -> PyErr {
    match err {
        // CPython's `OSError(errno, msg, filename)` 3-tuple
        // constructor dispatches to the right subclass
        // (`FileNotFoundError` for ENOENT, `PermissionError` for
        // EACCES, etc.) and populates `err.errno` / `err.filename`
        // so idiomatic `except FileNotFoundError as e: e.filename`
        // works. Passing `e.to_string()` alone collapses every
        // I/O failure to bare `OSError` with `errno is None`.
        AnalysisError::Io { source, path } => {
            let errno = source.raw_os_error();
            let msg = source.to_string();
            // `analyze_path` resolves the `FuncSpace.name` *before*
            // `std::fs::read`, so in strict mode (`allow_lossy_path
            // = False`) any path reaching this arm is valid UTF-8
            // and `path.display()` is lossless. In opt-in lossy mode
            // (#316) the caller has explicitly accepted U+FFFD
            // substitution at the `name`-field boundary, and the
            // same convention applies to the `filename` field on
            // the resulting `OSError`: both surfaces are user-facing
            // text, not identifiers, so `display()` is the right
            // call here — the caller has already certified that
            // lossy rendering is acceptable for this path.
            PyOSError::new_err((errno, msg, path.display().to_string()))
        }
        AnalysisError::NonUtf8Path => PyValueError::new_err(
            "path is not valid UTF-8 and cannot be used as a FuncSpace name; \
             pass allow_lossy_path=True to mirror the CLI's U+FFFD substitution",
        ),
        AnalysisError::UnsupportedLanguage(msg) => UnsupportedLanguageError::new_err(msg),
        AnalysisError::Parse(e) => ParseError::new_err(e.to_string()),
        AnalysisError::Serialization(e) => PyValueError::new_err(format!(
            "internal error: failed to serialise FuncSpace to JSON: {e}"
        )),
    }
}

/// Run the metric analysis on a single file path.
///
/// Returns a Python `dict` matching the JSON emitted by
/// `bca metrics --output-format json` for the same input at the
/// `FuncSpace` boundary — same field order, same numeric formatting,
/// same shape. Both sides serialise the same `FuncSpace` through
/// `serde_json::to_string` and the bindings parse that JSON with
/// `CPython`'s `json.loads` (which preserves insertion order).
///
/// Pass `exclude_tests=True` (keyword-only) to mirror the CLI's
/// global `--exclude-tests` flag: the bindings then thread
/// `MetricsOptions::default().with_exclude_tests(true)` into the
/// analysis, which the Rust language-checker hook uses to prune
/// Rust `#[test]` / `#[cfg(test)]` / `#[tokio::test]` subtrees
/// before any metric runs. Languages without a
/// `Checker::should_skip_subtree` override ignore the flag.
///
/// Pass `allow_lossy_path=True` (keyword-only) to mirror the CLI's
/// non-UTF-8 path handling: the `FuncSpace.name` field is filled
/// via `Path::to_string_lossy` (U+FFFD substitution) instead of
/// raising `ValueError`. Default is `False` — strict mode rejects
/// non-UTF-8 paths so the `name` field is always a round-trippable
/// identifier (#316).
///
/// Pass `skip_generated=False` (keyword-only) to opt out of the
/// CLI's generated-file walker filter. The default is `True`,
/// matching the CLI walker: files whose leading window matches
/// `@generated` / `DO NOT EDIT` / `GENERATED CODE` return `None`
/// without parsing. The `is_generated` check runs before language
/// inference so a generated file with an unknown extension still
/// returns `None` rather than raising `UnsupportedLanguageError`
/// (#317).
///
/// Language detection mirrors the CLI — path extension first,
/// then a `#!`-shebang line or emacs `-*- mode: … -*-` declaration
/// via `big_code_analysis::guess_language`. Non-UTF-8 paths match
/// the CLI byte-for-byte when `allow_lossy_path=True`. Generated
/// files are skipped on both sides when `skip_generated=True` (the
/// default), so the parity claim is now exact across all four
/// CLI-walker behaviours.
#[pyfunction]
#[pyo3(signature = (path, /, *, exclude_tests = false, allow_lossy_path = false, skip_generated = true))]
#[allow(clippy::needless_pass_by_value)]
// `path: PathBuf` (rather than `&Path`) is mandated by PyO3's
// path conversion: `FromPyObject` materializes a fresh `PathBuf`
// out of the `os.PathLike` argument, and there is no borrow to
// extract a `&Path` from.
fn analyze(
    py: Python<'_>,
    path: PathBuf,
    exclude_tests: bool,
    allow_lossy_path: bool,
    skip_generated: bool,
) -> PyResult<Option<Bound<'_, PyAny>>> {
    let opts = AnalyzeOptions {
        exclude_tests,
        allow_lossy_path,
        skip_generated,
    };
    analysis::analyze_path(&path, opts)
        .map_err(analysis_error_to_py)?
        .map(|json| conversion::json_string_to_py(py, &json))
        .transpose()
    // Keyword-only kwargs stay split at the PyO3 boundary (PyO3 has
    // no struct-literal binding for `#[pyo3(signature)]`); the
    // `AnalyzeOptions` struct lives on the Rust side of the FFI so
    // every internal callsite reads its policy by name, not by
    // positional bool order.
}

/// Run the metric analysis on an in-memory source buffer.
///
/// `code` accepts `str`, `bytes`, or `bytearray`. `language` is a
/// language name from [`supported_languages`] (case-insensitive).
/// Output shape matches [`analyze`]. `exclude_tests` mirrors the
/// CLI's `--exclude-tests` flag — see [`analyze`] for the details.
#[pyfunction]
#[pyo3(signature = (code, language, /, *, exclude_tests = false))]
fn analyze_source<'py>(
    py: Python<'py>,
    code: &Bound<'py, PyAny>,
    language: &str,
    exclude_tests: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes = extract_source_bytes(code)?;
    let json = analysis::analyze_source(language, &bytes, None, exclude_tests)
        .map_err(analysis_error_to_py)?;
    conversion::json_string_to_py(py, &json)
}

/// Pull a `Vec<u8>` out of a Python value that may be `str`, `bytes`,
/// or `bytearray`.
///
/// `str` is encoded as UTF-8 — the same encoding tree-sitter expects
/// from the upstream library when it accepts a `&[u8]`.
fn extract_source_bytes(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(s) = value.cast::<PyString>() {
        return Ok(s.to_str()?.as_bytes().to_vec());
    }
    if let Ok(b) = value.cast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    if let Ok(ba) = value.cast::<PyByteArray>() {
        // `PyByteArray::to_vec` copies under the GIL, side-stepping
        // the borrow-while-mutated hazard of `as_bytes()`.
        return Ok(ba.to_vec());
    }
    Err(PyValueError::new_err(
        "code must be str, bytes, or bytearray",
    ))
}

/// Return the language name that matches the given path's extension,
/// or `None` when the extension is not recognised.
///
/// Never raises — mirrors [`big_code_analysis::get_language_for_file`].
#[pyfunction]
#[pyo3(signature = (path, /))]
#[allow(clippy::needless_pass_by_value)]
// `PathBuf` (not `&Path`) is required by PyO3's path conversion —
// see the comment on `analyze` above.
fn language_for_file(path: PathBuf) -> Option<&'static str> {
    language::language_for_file(&path)
}

/// Return the supported language names, in declaration order.
#[pyfunction]
fn supported_languages() -> Vec<&'static str> {
    language::supported_languages()
}

/// Return the file extensions registered for `language`.
///
/// Raises [`UnsupportedLanguageError`] when `language` is not a known
/// language name.
#[pyfunction]
#[pyo3(signature = (language, /))]
fn language_extensions(language: &str) -> PyResult<Vec<&'static str>> {
    language::language_extensions(language)
        .ok_or_else(|| UnsupportedLanguageError::new_err(language.to_owned()))
}

/// `big_code_analysis._native` module entry point.
///
/// Re-exported by the pure-Python `big_code_analysis` package so
/// users see `big_code_analysis.analyze(...)`, not
/// `big_code_analysis._native.analyze(...)`. The exception classes
/// are placed both on `_native` (for `isinstance` / `except`) and on
/// the top-level package via the same re-export.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", PACKAGE_VERSION)?;
    m.add(
        "UnsupportedLanguageError",
        m.py().get_type::<UnsupportedLanguageError>(),
    )?;
    m.add("ParseError", m.py().get_type::<ParseError>())?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_source, m)?)?;
    m.add_function(wrap_pyfunction!(language_for_file, m)?)?;
    m.add_function(wrap_pyfunction!(supported_languages, m)?)?;
    m.add_function(wrap_pyfunction!(language_extensions, m)?)?;
    Ok(())
}
