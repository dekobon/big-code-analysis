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
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;

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
/// `PathBuf` is valid UTF-8 (the common case). Non-UTF-8 paths are
/// rendered byte-by-byte via Rust's `Debug` formatting so distinct
/// byte sequences map to distinct strings — `to_string_lossy` would
/// collapse them onto a single U+FFFD-substituted rendering and
/// silently break the documented `set(results)` dedup contract, which
/// keys on `(path, error, error_kind)`. The Debug rendering for
/// non-UTF-8 paths surrounds the value with double quotes (e.g.
/// `"/a\xff"`) — a visible cue that the path was not valid UTF-8 and
/// a deliberate signal to callers not to feed `err.path` back into
/// the filesystem as-is. The `FuncSpace.name` identifier in
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
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        // Defer each field's escape to Python's `repr()` built-in
        // instead of Rust's `{:?}` formatter. Rust's `Debug` escapes
        // non-printable codepoints (e.g. `\x01`) and non-BMP bytes
        // via `\u{N}` brace syntax, which Python's parser does NOT
        // accept — so the docstring claim that `eval(repr(x))`
        // round-trips would otherwise break for any path containing
        // a control character. Python's `repr()` emits `\xNN` /
        // `\uNNNN` / `\UNNNNNNNN` escapes that the same parser
        // accepts, restoring the round-trip property for arbitrary
        // string content.
        let builtins = py.import("builtins")?;
        let repr_fn = builtins.getattr("repr")?;
        let path_r: String = repr_fn.call1((&self.path,))?.extract()?;
        let error_r: String = repr_fn.call1((&self.error,))?.extract()?;
        let kind_r: String = repr_fn.call1((&self.error_kind,))?.extract()?;
        Ok(format!(
            "AnalysisError(path={path_r}, error={error_r}, error_kind={kind_r})"
        ))
    }

    /// Pickle hook so callers can ship `AnalysisError` values back
    /// from `multiprocessing.Pool` / `concurrent.futures.ProcessPoolExecutor`
    /// workers. The 2-tuple shape is the pickle convention
    /// `(callable, args)`: the class itself is the callable, and
    /// `(path, error, error_kind)` is the constructor arg tuple —
    /// which round-trips through [`py_new`]'s `ErrorKind::from_str`
    /// validation, so an out-of-taxonomy `error_kind` cannot sneak
    /// in via a tampered pickle stream.
    fn __reduce__<'py>(&self, py: Python<'py>) -> (Bound<'py, PyType>, (String, String, String)) {
        (
            py.get_type::<Self>(),
            (
                self.path.clone(),
                self.error.clone(),
                self.error_kind.clone(),
            ),
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
    /// result list back against their input list. Non-UTF-8 paths
    /// flow through [`encode_path_for_field`], which uses Rust's
    /// `Debug` formatting (byte-preserving) instead of `to_string_lossy`
    /// so distinct byte sequences map to distinct strings — the
    /// documented `set(results)` dedup contract depends on this
    /// because `path` participates in `__eq__` / `__hash__`.
    pub(crate) fn from_internal(err: AnalysisError, path: &Path) -> Self {
        let path_str = encode_path_for_field(path);
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
                // Mirror the prefix shape used by the synthetic
                // error sites in `analyze_batch` (`"internal:
                // <subkind>: <detail>"`) so a retry classifier or
                // log filter only has to learn one form to spot
                // an internal-bridge failure.
                format!("internal: serialization error: {e}"),
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

    /// Build a synthetic [`PyAnalysisError`] from a caller path plus
    /// a free-form message and kind. Used by [`analyze_batch`] to
    /// surface internal contract violations (e.g. a future
    /// `analyze_path` skip surface that returns `Ok(None)` despite
    /// `skip_generated=false`, or a `json_string_to_py` failure on
    /// the success arm) as per-file [`PyAnalysisError`] values
    /// rather than propagating them as Python exceptions — the
    /// never-raise-on-per-file-errors contract requires every input
    /// position to yield either a `dict` or an `AnalysisError`.
    fn synthetic_internal(path: &Path, message: String, kind: ErrorKind) -> Self {
        Self::new_internal(encode_path_for_field(path), message, kind)
    }
}

/// Encode a path for the diagnostic `path` field on [`PyAnalysisError`].
///
/// Valid UTF-8 paths pass through unchanged (the common case). Non-UTF-8
/// paths are rendered via Rust's `Debug` formatting — which escapes
/// invalid bytes via `\xNN` hex escapes — so two distinct non-UTF-8
/// byte sequences produce two distinct strings. `to_string_lossy` would
/// collapse them onto a single U+FFFD-substituted rendering and break
/// the documented `set(results)` dedup contract: `path` is one of the
/// three fields that participate in `__eq__` / `__hash__`. The Debug
/// fallback surrounds the rendering with double quotes — a visible cue
/// that the path was not valid UTF-8 and a deliberate signal to callers
/// not to feed `err.path` back into the filesystem as-is.
fn encode_path_for_field(path: &Path) -> String {
    // `clippy::unnecessary_debug_formatting` suggests `path.display()`
    // instead of `{:?}`, but that is precisely the bug this helper
    // exists to avoid: `Display` (and `to_string_lossy`) substitute
    // U+FFFD for invalid bytes, collapsing distinct non-UTF-8 paths
    // onto the same string. The `Debug` impl on `Path` / `OsStr` is
    // byte-preserving (`\xNN` hex escapes for invalid bytes), which
    // is what the `set(results)` dedup contract requires.
    #[allow(clippy::unnecessary_debug_formatting)]
    path.to_str()
        .map_or_else(|| format!("{path:?}"), str::to_owned)
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

    // Use `__len__` (Python's `len()` builtin, which is what PyO3's
    // `Bound::len()` calls) to preallocate when the caller passes a
    // list, tuple, or any other sized container. Iterators and
    // generators have no `__len__` and fall through to `cap = 0`;
    // for them `Vec::with_capacity(0)` is identical to `Vec::new()`.
    // Only `TypeError` ("object has no len") is silently swallowed —
    // any other exception from a custom `__len__` (e.g. a lazy proxy
    // raising `RuntimeError` to signal a transient backing-store
    // failure) surfaces as a Python error rather than being lost.
    let cap = match paths.len() {
        Ok(n) => n,
        Err(e) if e.is_instance_of::<PyTypeError>(py) => 0,
        Err(e) => return Err(e),
    };
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
            Ok(Some(json)) => match conversion::json_string_to_py(py, &json) {
                Ok(dict) => results.push(dict.unbind()),
                Err(err) => {
                    // Fold internal JSON-to-Python conversion
                    // failures into the per-file error stream so
                    // the never-raise contract holds even on the
                    // success arm. In practice `json.loads`
                    // cannot fail on a string `serde_json::to_string`
                    // produced — the fallback exists for the
                    // unreachable-today case where a future
                    // `FuncSpace` field serialises to something
                    // `json.loads` rejects, exactly the failure
                    // mode `AnalysisError::Serialization` already
                    // anticipates on the Rust side.
                    let py_err = PyAnalysisError::synthetic_internal(
                        &path,
                        format!("internal: JSON-to-Python conversion failed: {err}"),
                        ErrorKind::ParseError,
                    );
                    results.push(Py::new(py, py_err)?.into_any());
                }
            },
            // `skip_generated = false` makes `Ok(None)` unreachable
            // from `analyze_path` today, but defensively surface it
            // as an `IoError`-kind per-file error rather than
            // panicking — a `panic!` / `unreachable!` here would
            // bubble out as `PyO3`'s `PanicException` (a
            // `BaseException` subclass that `except Exception`
            // does not catch) and violate the documented
            // never-raise contract the day a future `analyze_path`
            // refactor adds another `Ok(None)` skip surface
            // (gitignore filter, size cap, etc.). The error
            // message names the invariant break loudly enough
            // that telemetry surfaces it for triage without
            // crashing the whole sweep and discarding every
            // already-pushed result.
            Ok(None) => {
                let py_err = PyAnalysisError::synthetic_internal(
                    &path,
                    "internal: analyze_path returned Ok(None) despite \
                     skip_generated=false; batch's 1:1 ordering invariant is \
                     preserved by surfacing this as IoError, but audit \
                     analyze_path() for new skip surfaces"
                        .to_owned(),
                    ErrorKind::IoError,
                );
                results.push(Py::new(py, py_err)?.into_any());
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
        // Use *distinct* caller and embedded paths so a regression
        // that read `AnalysisError::Io { source, path }`'s embedded
        // path instead of the caller-supplied path (a real risk
        // given `from_internal`'s docstring explicitly contrasts
        // the two — the 1:1 ordering invariant relies on the
        // caller path winning) is caught by the equality assertion
        // below. Lesson #31 in docs/development/lessons_learned.md
        // warns against the prior `/nope`-for-both pattern.
        let io = std::io::Error::from(std::io::ErrorKind::NotFound);
        let err = PyAnalysisError::from_internal(
            AnalysisError::Io {
                source: io,
                path: PathBuf::from("/embedded-from-analyze-path"),
            },
            Path::new("/caller-supplied"),
        );
        assert_eq!(err.error_kind, "IoError");
        assert_eq!(
            err.path, "/caller-supplied",
            "from_internal must surface the caller path, not the embedded \
             Io::path (1:1 ordering invariant depends on this)",
        );
        assert!(
            !err.error.is_empty(),
            "Io arm must surface std::io::Error::Display text, got empty string",
        );
    }

    #[test]
    fn from_internal_maps_parse_arm() {
        // `MetricsError::EmptyRoot` is the cheapest variant to
        // construct (no associated data) and is the canonical
        // reserved parse-failure variant upstream — exercises the
        // `Parse(_)` → `ParseError` arm without depending on a
        // grammar build.
        let err = PyAnalysisError::from_internal(
            AnalysisError::Parse(big_code_analysis::MetricsError::EmptyRoot),
            Path::new("/tmp/a.rs"),
        );
        assert_eq!(err.error_kind, "ParseError");
        assert_eq!(err.path, "/tmp/a.rs");
        assert!(
            !err.error.is_empty(),
            "Parse arm must surface MetricsError::Display text, got empty string",
        );
    }

    #[test]
    fn from_internal_maps_serialization_arm() {
        // Build a fabricated `serde_json::Error` via a guaranteed-
        // failure parse so the test is not dependent on a future
        // change to FuncSpace serialisation. The arm maps to
        // `ParseError` (the closed taxonomy keeps three kinds);
        // the message is required to carry the "internal
        // serialization error" prefix so a retry classifier
        // keyed on `error_kind` can still distinguish the case
        // by message inspection.
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = PyAnalysisError::from_internal(
            AnalysisError::Serialization(json_err),
            Path::new("/tmp/a.rs"),
        );
        assert_eq!(err.error_kind, "ParseError");
        assert_eq!(err.path, "/tmp/a.rs");
        assert!(
            err.error.contains("internal: serialization error"),
            "Serialization arm must prefix the message so callers can \
             distinguish it from a real ParseError despite the shared kind \
             — and the prefix shape must match the other synthetic-error \
             sites in analyze_batch (`internal: <subkind>: <detail>`), \
             got: {}",
            err.error,
        );
    }

    // `#[cfg(unix)]` lives at the function level (matching the
    // pattern in `analysis::tests::analyze_path_rejects_non_utf8_path_by_default`)
    // so the test is hidden cleanly on non-Unix targets rather than
    // compiling to an empty body that vacuously passes — Windows
    // paths are WTF-8 / UTF-16 with their own non-UTF-8 surface,
    // out of scope for this fixture.
    #[cfg(unix)]
    #[test]
    fn from_internal_preserves_byte_uniqueness_for_distinct_non_utf8_paths() {
        // The two paths below differ in their non-UTF-8 byte
        // sequence; `to_string_lossy` would collapse both to
        // "/a\u{FFFD}" (one U+FFFD character) and break the
        // documented `set(results)` dedup contract. With the
        // `Debug`-format fallback they map to two distinct strings.
        use std::os::unix::ffi::OsStrExt;
        let p1 = std::ffi::OsStr::from_bytes(b"/a\xff");
        let p2 = std::ffi::OsStr::from_bytes(b"/a\xfe");
        let e1 = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new(p1));
        let e2 = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new(p2));
        assert_ne!(
            e1.path, e2.path,
            "distinct non-UTF-8 paths must produce distinct PyAnalysisError.path \
             strings; collapsing them violates the set(results) dedup contract",
        );
        assert_ne!(e1, e2, "PartialEq must distinguish the two errors");
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
        //
        // Construct two *independent* instances rather than cloning
        // — a clone is byte-identical so the test would pass even
        // under a stateful `new_internal` regression (e.g. one that
        // mixed a per-call counter into the error field). Going
        // through the constructor twice exercises the contract that
        // matters: two equal-by-value batch failures must hash and
        // compare equal so `set(results)` actually de-duplicates.
        let make =
            || PyAnalysisError::new_internal("p".to_owned(), "m".to_owned(), ErrorKind::IoError);
        let a = make();
        let b = make();
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }
}
