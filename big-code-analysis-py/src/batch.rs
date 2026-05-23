//! Batch entry point and the structured `AnalysisError` Python class.
//!
//! Where [`crate::analysis`] raises a Python exception per failing
//! file, this module's [`analyze_batch`] sweeps an iterable of paths
//! and folds per-file failures into [`PyAnalysisError`] values
//! interleaved with successful result dicts. The contract is
//! *never-raise on per-file errors* so pipeline / workflow callers
//! can keep going past a missing file, an unknown extension, or a
//! parser failure without a `try` / `except` per path. Programmer
//! errors (non-iterable `paths`, empty `metrics=` list) still raise,
//! because they indicate a broken call site, not a recoverable input.
//!
//! The error taxonomy mirrors the design in issue #103:
//!
//! * `UnsupportedLanguage` — extension lookup and shebang inference
//!   both came up empty, or the upstream `MetricsError::LanguageDisabled`
//!   bubbled up (grammar not in this build).
//! * `ParseError` — every `MetricsError::Parse*` variant maps here,
//!   including the upstream reserved-but-not-yet-emitted `EmptyRoot`
//!   and `ParseHasErrors`. Today this arm is unreachable from Python
//!   for syntactically broken sources because tree-sitter is
//!   permissive (it produces an error tree, not an `Err`), but the
//!   mapping is in place for the day strict parsing lands upstream.
//! * `IoError` — `std::fs::read` failed (the most common kind), or
//!   the path is not valid UTF-8 (treated as a path-resolution
//!   failure since we cannot honour it without violating the
//!   identifier-path rule from AGENTS.md).

use std::hash::Hash;
use std::path::{Path, PathBuf};

use pyo3::Bound;
use pyo3::Py;
use pyo3::PyAny;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::analysis::{self, AnalysisError, AnalyzeOptions};
use crate::conversion;

/// Permitted values for the `error_kind` discriminator.
///
/// Kept as a typed constant rather than inlined into the constructor
/// so the test suite and any future serializer can iterate it without
/// re-stringifying.
pub(crate) const ERROR_KINDS: [&str; 3] = ["UnsupportedLanguage", "ParseError", "IoError"];

/// Structured per-file failure surfaced by [`analyze_batch`].
///
/// `path` is the caller-supplied path, rendered via `to_str` when the
/// `PathBuf` is valid UTF-8 (the common case) and via `to_string_lossy`
/// otherwise. The lossy fallback applies *only* to the diagnostic
/// `path` field on this struct — the `FuncSpace.name` identifier in
/// successful results is still strict UTF-8, so the project rule
/// against lossy identifiers (AGENTS.md) is not relaxed.
///
/// `error_kind` is one of [`ERROR_KINDS`]. The class itself is **not**
/// an exception subclass — instances appear in the return list of
/// [`analyze_batch`], they are never raised — so `isinstance(r,
/// AnalysisError)` is the canonical discriminator.
#[pyclass(
    frozen,
    name = "AnalysisError",
    module = "big_code_analysis._native",
    eq,
    hash,
    // Skip the auto `FromPyObject` derive that PyO3 generates for
    // `Clone` `#[pyclass]` types. `PyAnalysisError` only flows
    // *out* of the FFI boundary (it is built in Rust and returned
    // to Python); never accepting it as a `FromPyObject` argument
    // future-proofs the contract — and silences the 0.28
    // deprecation warning about the implicit derive.
    skip_from_py_object
)]
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PyAnalysisError {
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    error: String,
    #[pyo3(get)]
    error_kind: String,
}

#[pymethods]
impl PyAnalysisError {
    /// Build an `AnalysisError` directly. Useful for tests and for
    /// callers that want to deduplicate batch errors into a set keyed
    /// on `(path, error_kind)`.
    #[new]
    #[pyo3(signature = (path, error, error_kind))]
    fn py_new(path: String, error: String, error_kind: String) -> PyResult<Self> {
        if !ERROR_KINDS.contains(&error_kind.as_str()) {
            return Err(PyValueError::new_err(format!(
                "error_kind must be one of {ERROR_KINDS:?}, got {error_kind:?}",
            )));
        }
        Ok(Self {
            path,
            error,
            error_kind,
        })
    }

    fn __repr__(&self) -> String {
        // PyO3's `eq` / `hash` derives populate `__eq__` / `__hash__`
        // from the Rust `PartialEq` / `Hash` impls, so this `__repr__`
        // is the only dunder we hand-write. Mirror Python repr
        // conventions (`Type(field=value, ...)`) with explicit field
        // names so `eval(repr(x))` would construct an equivalent
        // object if the class were importable in the eval'd scope.
        format!(
            "AnalysisError(path={:?}, error={:?}, error_kind={:?})",
            self.path, self.error, self.error_kind,
        )
    }
}

impl PyAnalysisError {
    /// Build a [`PyAnalysisError`] from a bridge-layer
    /// [`AnalysisError`] plus the path that produced it.
    ///
    /// The `path` field always comes from the caller-supplied input —
    /// even when the underlying error already captured a path of its
    /// own (the `Io` arm does) — so callers can reliably zip the
    /// result list back against their input list. The lossy fallback
    /// on `to_str()` is the only place where non-UTF-8 path bytes can
    /// reach a user-visible string from this crate; it is confined to
    /// the diagnostic surface and never feeds back into `FuncSpace`.
    pub(crate) fn from_internal(err: AnalysisError, path: &Path) -> Self {
        let path_str = path
            .to_str()
            .map_or_else(|| path.to_string_lossy().into_owned(), str::to_owned);
        let (error, kind) = match err {
            AnalysisError::Io { source, .. } => (source.to_string(), "IoError"),
            AnalysisError::NonUtf8Path => (
                "path is not valid UTF-8; analyze_batch treats it as an I/O failure \
                 so the never-raise contract holds — pre-filter or call analyze() \
                 directly with allow_lossy_path=True to opt into U+FFFD substitution"
                    .to_owned(),
                "IoError",
            ),
            AnalysisError::UnsupportedLanguage(msg) => (msg, "UnsupportedLanguage"),
            AnalysisError::Parse(e) => (e.to_string(), "ParseError"),
            // `Serialization` is reachable only if a future upstream
            // `FuncSpace::Serialize` impl introduces a fallible path
            // — see `crate::analysis::AnalysisError::Serialization`.
            // Lump it into `ParseError` so the public taxonomy stays
            // at three kinds; the message preserves the JSON detail.
            AnalysisError::Serialization(e) => {
                (format!("internal serialization error: {e}"), "ParseError")
            }
        };
        Self {
            path: path_str,
            error,
            error_kind: kind.to_owned(),
        }
    }
}

// `PartialEq` / `Hash` are derived, but pin the `Hash`-`Eq` contract
// in an explicit test so a future field reorder cannot drift the two
// impls apart. The derives already guarantee this, but the test
// documents the invariant so a `#[derive(Hash)]` removal would
// surface immediately rather than at runtime in a Python dict.
fn _hash_eq_pin_compile_check() {
    fn assert_hash<T: Hash + Eq>() {}
    assert_hash::<PyAnalysisError>();
}

/// Run [`crate::analysis::analyze_path`] against every path in
/// `paths` and fold per-file errors into [`PyAnalysisError`] values.
///
/// `paths` is any Python iterable of `str | os.PathLike[str]`;
/// generators work because iteration is done lazily via `PyO3`'s
/// `try_iter` (which calls Python's `iter()` builtin under the hood).
/// The output list has the same length as the input iterable and
/// preserves order one-to-one, so callers can `zip(inputs, results)`
/// without losing the pairing.
///
/// `metrics=` is reserved for the metric-selection work in issue
/// #268 — it is validated here (an empty list raises `ValueError`
/// because that is a programmer error, not a per-file failure) but
/// the value is not yet forwarded into the analysis. The kwarg lives
/// on the Python signature today so adding `MetricSet` plumbing in
/// the follow-up phase does not change the function's call shape.
///
/// Batch always runs with `skip_generated=false` so that every input
/// position produces either a `dict` or an `AnalysisError`. The
/// `analyze()` per-file entry point still defaults to
/// `skip_generated=true` (and would return `None`); callers who want
/// the same behaviour for batches should pre-filter or call
/// `analyze()` directly.
#[pyfunction]
#[pyo3(signature = (paths, /, *, metrics = None))]
// `metrics: Option<Vec<String>>` is taken by value to match the PyO3
// keyword-argument FFI shape (the macro materialises an owned `Vec`
// out of the Python list); clippy's `needless_pass_by_value` lint
// can't see across the macro boundary, so silence it here.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn analyze_batch<'py>(
    py: Python<'py>,
    paths: &Bound<'py, PyAny>,
    metrics: Option<Vec<String>>,
) -> PyResult<Vec<Py<PyAny>>> {
    if let Some(ref m) = metrics
        && m.is_empty()
    {
        // Reject the empty-list case up front so the error surface
        // matches the issue contract: a *missing* selection (`None`,
        // the default) is fine; an *explicit but empty* selection is
        // a programmer error. The wording mentions the eventual
        // `metrics=` semantics so callers chasing the message find
        // their way to phase 4 (#268) once it lands.
        return Err(PyValueError::new_err(
            "metrics= must be None or a non-empty list of metric names",
        ));
    }

    let iter = paths.try_iter()?;
    let opts = AnalyzeOptions {
        exclude_tests: false,
        allow_lossy_path: false,
        // Batch processes every readable file: see the module-level
        // discussion above.
        skip_generated: false,
    };

    let mut results: Vec<Py<PyAny>> = Vec::new();
    for item in iter {
        let item = item?;
        let path: PathBuf = item.extract()?;
        match analysis::analyze_path(&path, opts) {
            Ok(Some(json)) => {
                let dict = conversion::json_string_to_py(py, &json)?;
                results.push(dict.unbind());
            }
            // `skip_generated = false` makes `Ok(None)` unreachable
            // from `analyze_path`; the explicit arm exists so a
            // future bridge-layer change that introduces a second
            // skip surface (e.g. a `.gitignore`-style filter) does
            // not silently shift a position out of the result list.
            // If it ever fires we surface it as an `IoError`-kind
            // entry so the 1:1 ordering invariant holds.
            Ok(None) => {
                let err = PyAnalysisError {
                    path: path
                        .to_str()
                        .map_or_else(|| path.to_string_lossy().into_owned(), str::to_owned),
                    error: "file skipped by bridge layer (unexpected in batch mode)".to_owned(),
                    error_kind: "IoError".to_owned(),
                };
                results.push(Py::new(py, err)?.into_any());
            }
            Err(err) => {
                let py_err = PyAnalysisError::from_internal(err, &path);
                results.push(Py::new(py, py_err)?.into_any());
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn from_internal_maps_unsupported_language_arm() {
        let err = PyAnalysisError::from_internal(
            AnalysisError::UnsupportedLanguage("no language for `.xyz`".to_owned()),
            Path::new("/tmp/a.xyz"),
        );
        assert_eq!(err.error_kind, "UnsupportedLanguage");
        assert_eq!(err.path, "/tmp/a.xyz");
        assert!(err.error.contains("no language"));
    }

    #[test]
    fn from_internal_maps_io_arm() {
        let io = std::io::Error::from(std::io::ErrorKind::NotFound);
        let err = PyAnalysisError::from_internal(
            AnalysisError::Io {
                source: io,
                path: PathBuf::from("/nope"),
            },
            Path::new("/nope"),
        );
        assert_eq!(err.error_kind, "IoError");
    }

    #[test]
    fn from_internal_maps_non_utf8_path_to_io_error() {
        // The `NonUtf8Path` variant gets folded into `IoError` so the
        // public taxonomy stays at three kinds. Test-via-revert:
        // changing the arm back to a fourth kind makes this fail.
        let err = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new("/x"));
        assert_eq!(err.error_kind, "IoError");
        assert!(err.error.contains("not valid UTF-8"));
    }

    #[test]
    fn equal_errors_hash_equal() {
        // PyO3's `#[pyclass(eq, hash)]` derives `__eq__` / `__hash__`
        // from `PartialEq` / `Hash`. This Rust-side check pins the
        // contract for the Python `set` / `dict` use case called out
        // in the issue body without needing the GIL.
        let a = PyAnalysisError {
            path: "p".into(),
            error: "m".into(),
            error_kind: "IoError".into(),
        };
        let b = a.clone();
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }
}
