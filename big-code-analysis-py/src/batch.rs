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
use std::str::FromStr;

use pyo3::Bound;
use pyo3::Py;
use pyo3::PyAny;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::analysis::{self, AnalysisError, AnalyzeOptions};
use crate::conversion;

/// Closed taxonomy for [`PyAnalysisError::error_kind`].
///
/// Kept as a private enum so the three permitted values live in one
/// place — every internal construction site goes through the
/// exhaustive `match` in [`PyAnalysisError::from_internal`] (a new
/// upstream `AnalysisError` variant fails the match at compile
/// time), and the public `py_new` constructor parses Python strings
/// through [`FromStr`] so out-of-taxonomy values raise `ValueError`
/// at the FFI boundary. The Python-facing `error_kind` attribute
/// still surfaces as a `str` via the [`AsRef<str>`] / `as_str`
/// projection — the enum is an internal validation device, not an
/// API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorKind {
    UnsupportedLanguage,
    ParseError,
    IoError,
}

impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedLanguage => "UnsupportedLanguage",
            Self::ParseError => "ParseError",
            Self::IoError => "IoError",
        }
    }
}

impl FromStr for ErrorKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "UnsupportedLanguage" => Ok(Self::UnsupportedLanguage),
            "ParseError" => Ok(Self::ParseError),
            "IoError" => Ok(Self::IoError),
            _ => Err(()),
        }
    }
}

/// Structured per-file failure surfaced by [`analyze_batch`].
///
/// `path` is the caller-supplied path, rendered via `to_str` when the
/// `PathBuf` is valid UTF-8 (the common case) and via `to_string_lossy`
/// otherwise. The lossy fallback applies *only* to the diagnostic
/// `path` field on this struct — the `FuncSpace.name` identifier in
/// successful results is still strict UTF-8, so the project rule
/// against lossy identifiers (AGENTS.md) is not relaxed.
///
/// `error_kind` surfaces as a `String` carrying one of the three
/// [`ErrorKind`] variants (`"UnsupportedLanguage"`, `"ParseError"`,
/// `"IoError"`). The class itself is **not** an exception subclass —
/// instances appear in the return list of [`analyze_batch`], they
/// are never raised — so `isinstance(r, AnalysisError)` is the
/// canonical discriminator.
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
    /// callers that want to deduplicate batch errors into a `set`
    /// — equality / hashing covers `(path, error, error_kind)` (all
    /// three fields), so two failures of the same kind on the same
    /// path but with differing `error` messages remain distinct.
    /// Bucket on `(r.path, r.error_kind)` explicitly if message
    /// drift is undesirable for the dedup key.
    #[new]
    #[pyo3(signature = (path, error, error_kind))]
    fn py_new(path: String, error: String, error_kind: &str) -> PyResult<Self> {
        let kind = error_kind.parse::<ErrorKind>().map_err(|()| {
            PyValueError::new_err(format!(
                "error_kind must be one of [UnsupportedLanguage, ParseError, IoError], \
                 got {error_kind:?}",
            ))
        })?;
        Ok(Self::new_internal(path, error, kind))
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
            AnalysisError::Io { source, .. } => (source.to_string(), ErrorKind::IoError),
            AnalysisError::NonUtf8Path => (
                "path is not valid UTF-8 and cannot be encoded as a FuncSpace \
                 name; analyze_batch surfaces this under error_kind='IoError' \
                 to keep the public taxonomy at three kinds — filter the \
                 batch input upstream if you need to distinguish the two"
                    .to_owned(),
                ErrorKind::IoError,
            ),
            AnalysisError::UnsupportedLanguage(msg) => (msg, ErrorKind::UnsupportedLanguage),
            AnalysisError::Parse(e) => (e.to_string(), ErrorKind::ParseError),
            // `Serialization` is reachable only if a future upstream
            // `FuncSpace::Serialize` impl introduces a fallible path
            // — see `crate::analysis::AnalysisError::Serialization`.
            // Lump it into `ParseError` so the public taxonomy stays
            // at three kinds; the message preserves the JSON detail.
            AnalysisError::Serialization(e) => (
                format!("internal serialization error: {e}"),
                ErrorKind::ParseError,
            ),
        };
        Self::new_internal(path_str, error, kind)
    }

    /// Internal constructor that owns the conversion from the typed
    /// [`ErrorKind`] enum to the Python-facing `String` field.
    ///
    /// Every non-test Rust-side construction site flows through
    /// here, so the exhaustive `match` in [`Self::from_internal`]
    /// is the single place a future `AnalysisError` variant must be
    /// mapped — the compiler enforces the taxonomy. `py_new`
    /// similarly parses Python strings through
    /// [`ErrorKind::from_str`] before arriving here, so the
    /// `error_kind` field is provably one of the three documented
    /// values without runtime validation. (The `equal_errors_hash_equal`
    /// unit test below does build a `PyAnalysisError` via struct
    /// literal because that test exercises the `Hash` / `Eq`
    /// derives directly — production paths never do.)
    fn new_internal(path: String, error: String, kind: ErrorKind) -> Self {
        Self {
            path,
            error,
            error_kind: kind.as_str().to_owned(),
        }
    }
}

// Compile-time assertion that `PyAnalysisError` continues to satisfy
// the `Hash + Eq` bounds the `#[pyclass(eq, hash)]` derives wire to
// Python's `__eq__` / `__hash__`. Using a `const _:` evaluator (not
// a free `fn`) so a future contributor reading the file does not
// mistake it for dead code — the body type-checks at compile time
// without participating in the runtime call graph.
const _: fn() = || {
    fn assert_hash_eq<T: Hash + Eq>() {}
    assert_hash_eq::<PyAnalysisError>();
};

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

    // Use the input's `__length_hint__` (Python's sized-iterable
    // protocol) to preallocate when the caller passes a list or
    // tuple. Generators report 0 / fall through; for them
    // `Vec::with_capacity(0)` is identical to `Vec::new()`. The
    // hint is best-effort by Python contract — over-allocation is
    // acceptable; we only pay one extra capacity slot in the
    // common list case and save ~14 doubling reallocations on a
    // 10k-path batch.
    let cap = paths.len().unwrap_or(0);
    let mut results: Vec<Py<PyAny>> = Vec::with_capacity(cap);
    for item in iter {
        let item = item?;
        let path: PathBuf = item.extract()?;
        // Release the GIL across the file read and tree-sitter
        // parse so other Python threads can run during the
        // sequential sweep. `analyze_path` touches no Python
        // objects (`Source`, `MetricsOptions`, `FuncSpace`,
        // `serde_json::to_string` all live on the Rust side), so
        // `py.detach` is sound; the GIL is re-acquired before
        // `json_string_to_py` builds the Python `dict`. (PyO3
        // 0.28 renamed `allow_threads` → `detach`.)
        let outcome = py.detach(|| analysis::analyze_path(&path, opts));
        match outcome {
            Ok(Some(json)) => {
                let dict = conversion::json_string_to_py(py, &json)?;
                results.push(dict.unbind());
            }
            // `skip_generated = false` makes `Ok(None)` unreachable
            // from `analyze_path` today. `unreachable!` rather than
            // a silent fallback means a future bridge-layer change
            // that introduces a second skip surface (e.g. a
            // `.gitignore`-style filter) fails loudly in
            // development — preferable to silently shifting a
            // result slot into an IoError-kind entry that
            // downstream dashboards would count as a real I/O
            // failure. The matching invariant is also pinned by
            // `analyze_path`'s test
            // `analyze_path_parses_generated_file_when_skip_generated_is_false`.
            Ok(None) => unreachable!(
                "bridge layer returned Ok(None) despite AnalyzeOptions::skip_generated == false; \
                 batch's 1:1 ordering invariant is broken — audit analyze_path() for new skip surfaces"
            ),
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
        // Pin path + error message too so a regression that
        // swapped fields or emitted an empty diagnostic on the
        // Io arm is caught — the other arms' tests use the same
        // shape, keep them symmetric.
        assert_eq!(err.path, "/nope");
        assert!(
            !err.error.is_empty(),
            "Io arm must surface std::io::Error::Display text, got empty string",
        );
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
